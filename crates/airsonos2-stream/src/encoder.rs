use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use airsonos2_core::PcmFrame;
use bytes::Bytes;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::task::JoinError;
use tokio::time;
use tracing::debug;

use airsonos2_core::StreamCodec;

use crate::pcm::f32_pcm_to_s16le_bytes;
use crate::registry::LiveStream;

#[derive(Clone, Debug)]
pub struct FfmpegEncoderConfig {
    pub ffmpeg_path: PathBuf,
    pub sample_rate: u32,
    pub channels: u8,
    pub mp3_bitrate_kbps: u16,
    pub codec: StreamCodec,
}

#[derive(Debug)]
pub struct FfmpegEncoder {
    input: mpsc::Sender<PcmFrame>,
    task: tokio::task::JoinHandle<Result<(), EncoderError>>,
}

impl FfmpegEncoder {
    pub fn spawn(config: FfmpegEncoderConfig, stream: LiveStream) -> Result<Self, EncoderError> {
        if config.codec == StreamCodec::Wav {
            return Ok(Self::spawn_wav(config, stream));
        }

        let mut child = Command::new(&config.ffmpeg_path)
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-fflags")
            .arg("nobuffer")
            .arg("-f")
            .arg("s16le")
            .arg("-ar")
            .arg(config.sample_rate.to_string())
            .arg("-ac")
            .arg(config.channels.to_string())
            .arg("-i")
            .arg("pipe:0")
            .arg("-f")
            .arg("mp3")
            .arg("-codec:a")
            .arg("libmp3lame")
            .arg("-b:a")
            .arg(format!("{}k", config.mp3_bitrate_kbps))
            .arg("-flush_packets")
            .arg("1")
            .arg("-write_xing")
            .arg("0")
            .arg("pipe:1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| EncoderError::Spawn {
                path: config.ffmpeg_path.clone(),
                source,
            })?;

        let stdin = child
            .stdin
            .take()
            .ok_or(EncoderError::MissingPipe("stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or(EncoderError::MissingPipe("stdout"))?;
        let stderr = child.stderr.take();
        let (input, mut rx) = mpsc::channel::<PcmFrame>(512);
        let first_pcm = Arc::new(AtomicBool::new(false));
        let first_mp3 = Arc::new(AtomicBool::new(false));
        let first_pcm_reader = first_pcm.clone();
        let first_mp3_reader = first_mp3.clone();

        let task = tokio::spawn(async move {
            let writer = tokio::spawn(async move {
                let mut stdin = stdin;
                while let Some(frame) = rx.recv().await {
                    if !first_pcm_reader.swap(true, Ordering::Relaxed) {
                        debug!(
                            sample_rate = frame.sample_rate,
                            channels = frame.channels,
                            samples = frame.samples_f32_interleaved.len(),
                            "first PCM frame written to ffmpeg"
                        );
                    }
                    let bytes = f32_pcm_to_s16le_bytes(&frame.samples_f32_interleaved);
                    stdin.write_all(&bytes).await?;
                }
                stdin.shutdown().await
            });

            let reader = tokio::spawn(async move {
                let mut stdout = stdout;
                let mut buf = vec![0_u8; 16 * 1024];
                loop {
                    let read = stdout.read(&mut buf).await?;
                    if read == 0 {
                        break;
                    }
                    if !first_mp3_reader.swap(true, Ordering::Relaxed) {
                        debug!(bytes = read, "first encoded chunk read from ffmpeg");
                    }
                    stream.publish(Bytes::copy_from_slice(&buf[..read]));
                }
                Ok::<(), std::io::Error>(())
            });

            let stderr_reader = stderr.map(|mut stderr| {
                tokio::spawn(async move {
                    let mut stderr_text = String::new();
                    let _ = stderr.read_to_string(&mut stderr_text).await;
                    stderr_text
                })
            });

            writer.await??;
            reader.await??;
            let status = child.wait().await?;

            if status.success() {
                Ok(())
            } else {
                let stderr_text = match stderr_reader {
                    Some(task) => task.await.unwrap_or_default(),
                    None => String::new(),
                };
                Err(EncoderError::Exited {
                    status: status.code(),
                    stderr: stderr_text,
                })
            }
        });

        Ok(Self { input, task })
    }

    fn spawn_wav(config: FfmpegEncoderConfig, stream: LiveStream) -> Self {
        let (input, mut rx) = mpsc::channel::<PcmFrame>(512);
        let task = tokio::spawn(async move {
            let mut sent_header = false;
            let mut first_pcm = false;

            while let Some(frame) = rx.recv().await {
                if !first_pcm {
                    first_pcm = true;
                    debug!(
                        sample_rate = frame.sample_rate,
                        channels = frame.channels,
                        samples = frame.samples_f32_interleaved.len(),
                        "first PCM frame received for WAV stream"
                    );
                }

                if !sent_header {
                    sent_header = true;
                    stream.publish_prelude(Bytes::from(wav_stream_header(
                        config.sample_rate,
                        config.channels,
                    )));
                    debug!("WAV stream header published");
                }

                stream.publish_timed_pcm(
                    Bytes::from(f32_pcm_to_s16le_bytes(&frame.samples_f32_interleaved)),
                    frame.presentation_time,
                    frame.sample_rate,
                    frame.channels,
                );
            }

            Ok(())
        });

        Self { input, task }
    }

    pub async fn write_frame(&self, frame: PcmFrame) -> Result<(), EncoderError> {
        self.input
            .send(frame)
            .await
            .map_err(|_| EncoderError::InputClosed)
    }

    pub async fn shutdown(self) -> Result<(), EncoderError> {
        drop(self.input);
        let joined = time::timeout(Duration::from_secs(2), self.task)
            .await
            .map_err(|_| EncoderError::ShutdownTimeout)?;
        joined?
    }
}

fn wav_stream_header(sample_rate: u32, channels: u8) -> Vec<u8> {
    let bits_per_sample = 16_u16;
    let channels_u16 = channels as u16;
    let block_align = channels_u16 * (bits_per_sample / 8);
    let byte_rate = sample_rate * u32::from(block_align);

    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&u32::MAX.to_le_bytes());
    header.extend_from_slice(b"WAVE");
    header.extend_from_slice(b"fmt ");
    header.extend_from_slice(&16_u32.to_le_bytes());
    header.extend_from_slice(&1_u16.to_le_bytes());
    header.extend_from_slice(&channels_u16.to_le_bytes());
    header.extend_from_slice(&sample_rate.to_le_bytes());
    header.extend_from_slice(&byte_rate.to_le_bytes());
    header.extend_from_slice(&block_align.to_le_bytes());
    header.extend_from_slice(&bits_per_sample.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&u32::MAX.to_le_bytes());
    header
}

#[derive(Debug, Error)]
pub enum EncoderError {
    #[error("failed to spawn ffmpeg at {path}: {source}")]
    Spawn {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("ffmpeg child was missing {0} pipe")]
    MissingPipe(&'static str),
    #[error("ffmpeg input channel closed")]
    InputClosed,
    #[error("ffmpeg exited with status {status:?}: {stderr}")]
    Exited { status: Option<i32>, stderr: String },
    #[error("ffmpeg shutdown timed out")]
    ShutdownTimeout,
    #[error("ffmpeg IO failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("ffmpeg task failed: {0}")]
    Join(#[from] JoinError),
}
