pub fn airplay_normalized_to_sonos_volume(volume: f32) -> u8 {
    let volume = if volume.is_finite() { volume } else { 0.0 };
    (volume.clamp(0.0, 1.0) * 100.0).round() as u8
}

pub fn airplay_db_to_sonos_volume(db: f32) -> u8 {
    if !db.is_finite() || db <= -144.0 {
        return 0;
    }

    let normalized = 10.0_f32.powf(db.clamp(-60.0, 0.0) / 20.0);
    airplay_normalized_to_sonos_volume(normalized)
}

pub fn sonos_volume_to_airplay_db(volume: u8) -> f32 {
    if volume == 0 {
        return -144.0;
    }

    let normalized = (volume as f32 / 100.0).clamp(0.0, 1.0);
    if normalized <= 0.0 {
        return -144.0;
    }

    20.0 * normalized.log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_normalized_airplay_volume_to_sonos_range() {
        assert_eq!(airplay_normalized_to_sonos_volume(-0.5), 0);
        assert_eq!(airplay_normalized_to_sonos_volume(0.0), 0);
        assert_eq!(airplay_normalized_to_sonos_volume(0.424), 42);
        assert_eq!(airplay_normalized_to_sonos_volume(1.2), 100);
    }

    #[test]
    fn maps_db_airplay_volume_to_sonos_range() {
        assert_eq!(airplay_db_to_sonos_volume(0.0), 100);
        assert_eq!(airplay_db_to_sonos_volume(-144.0), 0);
        assert_eq!(airplay_db_to_sonos_volume(f32::NAN), 0);
        assert!(airplay_db_to_sonos_volume(-20.0) < airplay_db_to_sonos_volume(-10.0));
    }

    #[test]
    fn maps_sonos_volume_to_airplay_db_range() {
        assert_eq!(sonos_volume_to_airplay_db(0), -144.0);
        assert_eq!(sonos_volume_to_airplay_db(100), 0.0);
        assert!((sonos_volume_to_airplay_db(42) - (-7.535_024)).abs() < 0.001);
    }

    #[test]
    fn sonos_and_airplay_volume_round_trip() {
        for sonos_volume in [1, 10, 42, 75, 100] {
            let db = sonos_volume_to_airplay_db(sonos_volume);
            let round_trip = airplay_db_to_sonos_volume(db);
            assert_eq!(round_trip, sonos_volume);
        }
    }
}
