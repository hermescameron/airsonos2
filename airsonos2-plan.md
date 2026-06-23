# AirSonos2: AirPlay 2 Bridge for Legacy Sonos

## Summary

Build an open-source Rust service that exposes each legacy Sonos S2 room as a virtual AirPlay 2 speaker, receives AirPlay 2 audio from iPhone apps, transcodes it into a Sonos-friendly live HTTP stream, and controls Sonos playback over local UPnP/SOAP.

## Implementation Status

Completed in this repository:

- Nix dev flake with Rust 1.86, clippy, rustfmt, ffmpeg, and supporting tools.
- Cargo workspace with `airsonos2-cli`, `airsonos2-core`, `airsonos2-airplay`, `airsonos2-sonos`, `airsonos2-stream`, and `airsonos2-diagnostics`.
- CLI commands: `serve`, `discover`, `doctor`, `pairings list`, `pairings reset --zone`, and `calibrate --zones`.
- TOML config parsing with defaults matching the public configuration interface.
- Core types, deterministic RTSP port allocation, stable virtual hardware addresses, zone filtering, AirPlay-to-Sonos volume mapping, and sync offset math.
- `shairplay` receiver adapter pinned to `=0.2.0` with `ap2` and `resample` features for Rust 1.86 compatibility.
- Sonos SSDP discovery, device description parsing, topology parsing, and SOAP control actions for transport URI, play, stop, volume, and standalone coordinator mode.
- Live stream registry, chunked MP3 HTTP routes, generated ffmpeg test tone endpoint, PCM f32-to-s16le conversion, and supervised per-session ffmpeg encoder wrapper.
- Bridge runtime wiring AirPlay session events to ffmpeg streams and Sonos playback control.
- Dockerfile, systemd unit, sample config, Proxmox LXC notes, hardware acceptance checklist, README, and GitHub Actions.

Still requires hardware validation:

- Real iPhone AirPlay 2 app matrix against the pinned receiver layer.
- Pairing behavior across daemon restarts as exposed by `shairplay`.
- Sonos playback of the generated test tone and live ffmpeg streams on target S2 legacy rooms.
- Two-, three-, and six-zone group sync calibration.
- Native HomePod/native AirPlay sync experiments.

Primary target:

- Runtime: Linux LXC on Proxmox.
- Network assumption: behaves like one flat LAN; router handles inter-VLAN mDNS/multicast.
- Sonos target: S2 legacy speakers.
- AirPlay goal: Apple-managed grouping of 6+ virtual Sonos endpoints.
- Quality bar: imperceptible sync between bridged Sonos rooms after calibration.
- First codec: stable MP3/AAC-style live stream, not lossless.
- Controls: AirPlay volume maps to Sonos volume; disconnect stops Sonos.
- Packaging: both container image and systemd install early.
- AirPlay receiver base: `shairplay-rust`, with a fallback gate if real-device tests fail.

Important reality check: syncing bridged Sonos rooms with each other may be feasible with calibration; syncing them imperceptibly with native HomePods/AirPlay 2 speakers is experimental because Sonos adds an independent HTTP pull/buffer layer after AirPlay timing.

## Sources Consulted

- `shairplay-rust`: AP1/AP2 Rust receiver, pre-1.0, LGPL-3.0-or-later, PCM callback API, AP2 status notes: https://github.com/metaneutrons/shairplay-rust
- Shairport Sync: mature AirPlay/AirPlay 2 receiver, protocol fragility notes, PTP/NQPTP background: https://github.com/mikebrady/shairport-sync
- Sonos supported stream formats: https://docs.sonos.com/docs/supported-audio-formats
- Sonos playback model: HTTP pull, buffering, range requests: https://docs.sonos.com/docs/playback-on-sonos
- Sonos AVTransport UPnP actions including `SetAVTransportURI`, `Play`, `Stop`: https://sonos.svrooij.io/services/av-transport
- AirConnect notes on AirPlay-to-UPnP buffering mismatch: https://github.com/philippe44/AirConnect

## Architecture

Create a Rust Cargo workspace with:

```text
airsonos2/
  Cargo.toml
  crates/
    airsonos2-cli/
    airsonos2-core/
    airsonos2-airplay/
    airsonos2-sonos/
    airsonos2-stream/
    airsonos2-diagnostics/
  packaging/
    docker/
    systemd/
  docs/
```

Use Rust stable 1.86+.

Core dependencies:

- `shairplay` from `shairplay-rust`, pinned exactly, with `ap2` and `resample` features.
- `tokio` for async runtime.
- `axum` or `hyper` for HTTP stream and diagnostics endpoints.
- `reqwest` for Sonos SOAP calls.
- `quick-xml` for SOAP/XML parsing.
- `serde`, `toml`, `clap`, `tracing`, `prometheus` or equivalent metrics.
- `ffmpeg` as the first encoder backend, run as a supervised child process per active stream.

## Public CLI

Implement these commands:

```bash
airsonos2 serve --config /etc/airsonos2/config.toml
airsonos2 discover
airsonos2 doctor
airsonos2 pairings list
airsonos2 pairings reset --zone <zone-id>
airsonos2 calibrate --zones <zone-a,zone-b,...>
```

`serve` runs the daemon.

`discover` prints Sonos rooms, IPs, coordinator status, model names, and detected stream support.

`doctor` checks:

- `_airplay._tcp` and `_raop._tcp` mDNS advertisement visibility.
- Sonos SSDP discovery.
- Sonos SOAP reachability on port `1400`.
- HTTP stream reachability from the Sonos VLAN.
- AirPlay timing/PTP-related UDP reachability where exposed by the receiver stack.
- Port conflicts for virtual AirPlay endpoints.

## Configuration Interface

Use TOML:

```toml
[server]
bind = "0.0.0.0"
http_port = 7000
state_dir = "/var/lib/airsonos2"
log_level = "info"

[airplay]
name_template = "{room} AirSonos2"
pin = "3939"
base_rtsp_port = 5000
output_sample_rate = 48000
output_channels = 2
max_clients_per_zone = 1

[sonos]
auto_discover = true
include_rooms = []
exclude_rooms = []
force_standalone_on_start = true
stop_on_disconnect = true
volume_mode = "sonos"

[stream]
codec = "mp3"
mp3_bitrate_kbps = 320
prebuffer_ms = 3000
http_chunked = true
icy_metadata = true

[diagnostics]
metrics_addr = "0.0.0.0:9100"

[sync]
default_offset_ms = 0

[sync.zone_offsets_ms]
# "Kitchen" = 120
# "Office" = 80
```

Default behavior:

- Auto-discover all Sonos rooms.
- Create one AirPlay 2 virtual endpoint per included Sonos room.
- Persist each virtual endpoint’s AirPlay identity and pairing keys under `state_dir`.
- On AirPlay connect, make the target Sonos room a standalone group coordinator before playback.
- On disconnect, stop playback but do not restore prior queue/group state in v1.

## Internal Types

Define these core types:

```rust
struct SonosZone {
    id: ZoneId,
    room_name: String,
    ip: IpAddr,
    model: String,
    rincon_id: String,
    is_visible_room: bool,
    is_group_coordinator: bool,
}

struct VirtualAirPlayEndpoint {
    zone_id: ZoneId,
    display_name: String,
    rtsp_port: u16,
    persisted_hwaddr: [u8; 6],
    pairing_store_path: PathBuf,
}

struct AirPlaySession {
    session_id: SessionId,
    zone_id: ZoneId,
    started_at: Instant,
    volume: Option<f32>,
}

struct PcmFrame {
    sample_rate: u32,
    channels: u8,
    samples_f32_interleaved: Vec<f32>,
    presentation_time: Option<Instant>,
}

struct StreamSession {
    session_id: SessionId,
    zone_id: ZoneId,
    codec: StreamCodec,
    local_url: Url,
    encoder_state: EncoderState,
}

struct ZoneSyncConfig {
    zone_id: ZoneId,
    offset_ms: i64,
}
```

## Implementation Phases

### Phase 0: Feasibility Spike

Goal: prove `shairplay-rust` works with real iPhone apps before building the bridge.

Tasks:

- Create a minimal Rust binary using `shairplay-rust`.
- Advertise one AirPlay 2 receiver.
- Persist identity and pairing keys.
- Confirm iPhone sees it after restart as the same device.
- Confirm PCM callbacks from:
  - Apple Music
  - Podcasts
  - YouTube
  - Spotify via AirPlay
- Write PCM to a WAV file for inspection.
- Run two virtual endpoints on different ports.

Fallback rule:

- If `shairplay-rust` cannot reliably receive AP2 audio from the target apps, switch the receiver layer to Shairport Sync process integration and keep the rest of the architecture unchanged.

### Phase 1: Sonos Discovery And Control

Tasks:

- Implement SSDP discovery for Sonos devices.
- Query device description XML.
- Identify rooms and coordinators.
- Implement SOAP calls:
  - `AVTransport.SetAVTransportURI`
  - `AVTransport.Play`
  - `AVTransport.Stop`
  - `RenderingControl.SetVolume`
  - `AVTransport.BecomeCoordinatorOfStandaloneGroup`
- Add `airsonos2 discover`.
- Add an HTTP test stream endpoint serving a generated MP3 tone.
- Prove one Sonos room can play the generated stream.

### Phase 2: Single-Zone Bridge

Tasks:

- Connect `shairplay-rust` PCM callback to a per-session encoder pipeline.
- Convert `f32` PCM to `s16le`.
- Pipe PCM into `ffmpeg`.
- Serve live `audio/mpeg` over HTTP with chunked transfer.
- On AirPlay start:
  - Create stream session.
  - Start encoder.
  - Set Sonos transport URI to the local stream URL.
  - Call `Play`.
- On AirPlay volume change:
  - Map normalized AirPlay volume to Sonos `0..100`.
- On AirPlay stop/disconnect:
  - Stop Sonos.
  - Tear down encoder and stream session.

### Phase 3: Multi-Zone Apple Grouping

Tasks:

- Start one virtual AirPlay endpoint per Sonos room.
- Allocate deterministic ports from `base_rtsp_port`.
- Persist stable virtual MAC/device IDs per zone.
- Support at least six simultaneous active sessions.
- Add per-zone metrics:
  - PCM frames received.
  - Encoder input queue depth.
  - HTTP bytes served.
  - Sonos GET connected/disconnected.
  - Encoder lag.
  - dropped frames.
  - AirPlay session start/stop.
- Validate iOS can select multiple virtual Sonos endpoints as an AirPlay group.

### Phase 4: Sync Calibration

Tasks:

- Add configurable per-zone delay offsets.
- Delay faster zones to match the slowest observed zone.
- Add `airsonos2 calibrate` to run a test mode and print recommended manual offset adjustments.
- Store offsets in config.
- Validate:
  - 2-zone group.
  - 3-zone group.
  - 6-zone group.
- Acceptance target: no obvious echo between bridged Sonos rooms in adjacent listening areas.

Native HomePod/native AirPlay sync remains experimental. Document that bridged Sonos may lag native AirPlay speakers because Sonos buffers after the AirPlay receiver.

### Phase 5: Packaging And Open Source Readiness

Tasks:

- Add Dockerfile with `ffmpeg` included.
- Add systemd unit and install docs.
- Add sample Proxmox LXC notes:
  - host or bridged networking preferred.
  - multicast/mDNS required.
  - Sonos port `1400` reachable.
  - service HTTP port reachable from Sonos.
- Add GitHub Actions:
  - `cargo fmt --check`
  - `cargo clippy --workspace --all-features -- -D warnings`
  - `cargo test --workspace --all-features`
  - Docker image build.
- Add README with protocol-risk disclaimer.
- Include LGPL dependency notice for `shairplay-rust`.

## Test Plan

Unit tests:

- AirPlay-to-Sonos volume mapping.
- Config parsing and defaults.
- Zone include/exclude filtering.
- Stream session lifecycle.
- SOAP request generation.
- XML response parsing.
- Per-zone port allocation.
- Sync offset math.

Integration tests:

- Fake Sonos HTTP/SOAP server.
- Fake encoder process.
- HTTP stream client tests for chunked MP3 responses.
- Session cleanup on encoder crash.
- Multiple simultaneous zone sessions.
- Metrics endpoint output.

Hardware acceptance tests:

- iPhone sees all virtual endpoints.
- Pairings survive daemon restart.
- Apple Music plays to one Sonos room.
- Podcasts, YouTube, and Spotify-over-AirPlay play to one Sonos room.
- iPhone groups two virtual Sonos endpoints.
- iPhone groups six virtual Sonos endpoints.
- Volume changes affect the correct Sonos room.
- Disconnect stops the correct Sonos room.
- Daemon restart recovers cleanly.
- `doctor` passes on the Proxmox LXC network.

## Verification Commands For Code Changes

Every implementation PR must run:

```bash
cargo fmt --check
cargo clippy --workspace --all-features -- -D warnings
cargo test --workspace --all-features
docker build -f packaging/docker/Dockerfile .
```

If files are formatted during implementation, run:

```bash
cargo fmt
```

## Key Risks

- AirPlay 2 is reverse engineered and may break with future iOS updates.
- `shairplay-rust` is promising but pre-1.0 and low-adoption; the spike is mandatory.
- Sonos HTTP pull buffering may prevent imperceptible sync with native AirPlay 2 devices.
- Live streams without fixed `Content-Length` can expose Sonos edge cases.
- Six simultaneous encoders may require CPU tuning.
- Inter-VLAN multicast can appear “mostly working” while still breaking discovery or timing.
- AP2 receiver-to-iPhone remote control is limited for third-party receivers; avoid promising phone playback control beyond received volume/session events.

## Explicit Assumptions

- We will use `shairplay-rust` first.
- LGPL-3.0-or-later dependency is acceptable.
- The project will be open source.
- Initial stream codec is MP3 at 320 kbps CBR through `ffmpeg`.
- The first usable release does not restore previous Sonos queues/groups.
- Router mDNS/inter-VLAN setup will make the LXC behave like it shares one LAN with iPhones and Sonos.
- The main success target is Apple grouping between bridged Sonos endpoints; native HomePod sync is a stretch goal.
