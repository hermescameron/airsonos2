use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::Duration;

use crate::ZoneId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZoneDelay {
    pub zone_id: ZoneId,
    pub delay_ms: u64,
}

pub fn recommended_delay_for_zone(zone_offset_ms: i64, slowest_offset_ms: i64) -> u64 {
    slowest_offset_ms.saturating_sub(zone_offset_ms).max(0) as u64
}

pub fn delays_from_offsets(offsets: &BTreeMap<ZoneId, i64>) -> Vec<ZoneDelay> {
    let slowest = offsets.values().copied().max().unwrap_or_default();

    offsets
        .iter()
        .map(|(zone_id, offset)| ZoneDelay {
            zone_id: zone_id.clone(),
            delay_ms: recommended_delay_for_zone(*offset, slowest),
        })
        .collect()
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ZoneStartupTiming {
    pub set_uri_ms: Option<u64>,
    pub subscriber_connect_ms: Option<u64>,
    pub first_bytes_ms: Option<u64>,
    pub play_ms: Option<u64>,
}

impl ZoneStartupTiming {
    pub fn measured_lag_ms(&self) -> Option<u64> {
        [
            self.subscriber_connect_ms,
            self.first_bytes_ms,
            self.play_ms,
            self.set_uri_ms,
        ]
        .into_iter()
        .flatten()
        .max()
    }
}

#[derive(Clone, Debug)]
pub struct StartupDelayEstimator {
    sample_limit: usize,
    min_samples: usize,
    samples: HashMap<ZoneId, VecDeque<u64>>,
}

impl StartupDelayEstimator {
    pub fn new(sample_limit: usize, min_samples: usize) -> Self {
        Self {
            sample_limit: sample_limit.max(1),
            min_samples: min_samples.max(1),
            samples: HashMap::new(),
        }
    }

    pub fn record(&mut self, zone_id: ZoneId, timing: &ZoneStartupTiming) {
        let Some(lag_ms) = timing.measured_lag_ms() else {
            return;
        };
        let samples = self.samples.entry(zone_id).or_default();
        samples.push_back(lag_ms);
        while samples.len() > self.sample_limit {
            samples.pop_front();
        }
    }

    pub fn median_lag_ms(&self, zone_id: &ZoneId) -> Option<u64> {
        let samples = self.samples.get(zone_id)?;
        if samples.len() < self.min_samples {
            return None;
        }
        let mut sorted: Vec<u64> = samples.iter().copied().collect();
        sorted.sort_unstable();
        Some(sorted[sorted.len() / 2])
    }

    pub fn automatic_delay_ms(&self, zones: &[ZoneId], zone_id: &ZoneId, cap_ms: u64) -> u64 {
        let lags: BTreeMap<ZoneId, u64> = zones
            .iter()
            .filter_map(|zone| self.median_lag_ms(zone).map(|lag| (zone.clone(), lag)))
            .collect();
        let Some(zone_lag) = lags.get(zone_id).copied() else {
            return 0;
        };
        let slowest = lags.values().copied().max().unwrap_or(zone_lag);
        slowest.saturating_sub(zone_lag).min(cap_ms)
    }
}

pub fn combine_sync_delay(
    automatic_delay_ms: u64,
    manual_offset_ms: i64,
    default_offset_ms: i64,
) -> Duration {
    let manual_ms = manual_offset_ms.saturating_add(default_offset_ms).max(0) as u64;
    Duration::from_millis(automatic_delay_ms.saturating_add(manual_ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faster_zones_get_delayed_to_slowest_zone() {
        let mut offsets = BTreeMap::new();
        offsets.insert(ZoneId::new("Kitchen"), 120);
        offsets.insert(ZoneId::new("Office"), 80);
        offsets.insert(ZoneId::new("Den"), 120);

        let delays = delays_from_offsets(&offsets);

        assert_eq!(delays[0].delay_ms, 0);
        assert_eq!(delays[1].delay_ms, 0);
        assert_eq!(delays[2].delay_ms, 40);
    }

    #[test]
    fn startup_estimator_uses_median_not_newest_sample() {
        let zone = ZoneId::new("Kitchen");
        let mut estimator = StartupDelayEstimator::new(20, 3);
        for lag in [100, 300, 900] {
            estimator.record(
                zone.clone(),
                &ZoneStartupTiming {
                    subscriber_connect_ms: Some(lag),
                    ..ZoneStartupTiming::default()
                },
            );
        }

        assert_eq!(estimator.median_lag_ms(&zone), Some(300));
    }

    #[test]
    fn startup_estimator_ignores_zones_with_too_few_samples() {
        let zone = ZoneId::new("Kitchen");
        let mut estimator = StartupDelayEstimator::new(20, 3);
        estimator.record(
            zone.clone(),
            &ZoneStartupTiming {
                subscriber_connect_ms: Some(100),
                ..ZoneStartupTiming::default()
            },
        );

        assert_eq!(estimator.median_lag_ms(&zone), None);
    }

    #[test]
    fn automatic_delay_is_capped() {
        let fast = ZoneId::new("Fast");
        let slow = ZoneId::new("Slow");
        let mut estimator = StartupDelayEstimator::new(20, 1);
        estimator.record(
            fast.clone(),
            &ZoneStartupTiming {
                subscriber_connect_ms: Some(100),
                ..ZoneStartupTiming::default()
            },
        );
        estimator.record(
            slow.clone(),
            &ZoneStartupTiming {
                subscriber_connect_ms: Some(1_500),
                ..ZoneStartupTiming::default()
            },
        );

        assert_eq!(
            estimator.automatic_delay_ms(&[fast.clone(), slow], &fast, 1_000),
            1_000
        );
    }

    #[test]
    fn manual_offsets_combine_with_learned_compensation() {
        assert_eq!(combine_sync_delay(40, 80, 10), Duration::from_millis(130));
    }
}
