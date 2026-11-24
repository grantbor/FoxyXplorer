//! Offline geographic timezone resolution for GPS waypoints.
//! The timezone is based on recording coordinates, NOT the computer's timezone.
//! A UTC instant is never modified; the offset is used only for Ozi WPT field 25.
use chrono::{DateTime, Offset, Utc};
use std::sync::LazyLock;
use tzf_rs::DefaultFinder;

// Construct once: loading the bundled polygon dataset for every point would be expensive.
static FINDER: LazyLock<DefaultFinder> = LazyLock::new(DefaultFinder::new);

pub fn offset_for_zone(zone: &str, utc_seconds: f64) -> Option<i32> {
    if !utc_seconds.is_finite() || utc_seconds < i64::MIN as f64 || utc_seconds >= i64::MAX as f64 {
        return None;
    }
    let instant = DateTime::<Utc>::from_timestamp(utc_seconds.floor() as i64, 0)?;
    let tz: chrono_tz::Tz = zone.parse().ok()?;
    Some(instant.with_timezone(&tz).offset().fix().local_minus_utc() / 60)
}

/// Returns an IANA zone ID and the historically correct UTC offset in minutes.
/// Returns None for unrecognized coordinates (e.g. outside timezone polygons).
pub fn at(lon: f64, lat: f64, utc_seconds: f64) -> Option<(String, i32)> {
    if !lon.is_finite() || !lat.is_finite() || !(-180.0..=180.0).contains(&lon)
        || !(-90.0..=90.0).contains(&lat)
    {
        return None;
    }
    // The library API takes longitude first, then latitude.
    let name = FINDER.get_tz_name(lon, lat);
    if name.is_empty() { return None; }
    Some((name.to_owned(), offset_for_zone(name, utc_seconds)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_location_not_host_timezone() {
        let utc = 1_795_529_600.0; // UTC instant; host timezone is irrelevant.
        let (novosibirsk, nz) = at(82.922, 55.507, utc).expect("Novosibirsk");
        let (yekaterinburg, yz) = at(60.60, 56.84, utc).expect("Yekaterinburg");
        assert_eq!(novosibirsk, "Asia/Novosibirsk");
        assert_eq!(yekaterinburg, "Asia/Yekaterinburg");
        assert_eq!(nz, 420);
        assert_eq!(yz, 300);
    }

    #[test]
    fn historical_dst_uses_recording_date() {
        let winter = 1_768_521_600.0; // January 2026
        let summer = 1_784_160_000.0; // July 2026
        assert_eq!(at(16.37, 48.21, winter).unwrap().1, 60);
        assert_eq!(at(16.37, 48.21, summer).unwrap().1, 120);
    }

    #[test]
    fn invalid_coordinates_do_not_fall_back_to_host_time() {
        assert!(at(f64::NAN, 55.0, 1_764_000_000.0).is_none());
        assert!(at(190.0, 55.0, 1_764_000_000.0).is_none());
    }
}
