//! Conservative whole-track movement-profile inference for the one-click command.
//!
//! This does NOT infer movement modes on individual pieces, change thresholds,
//! modify points, or treat an abnormal speed as evidence for a vehicle. Unclear
//! (short, slow vehicle, bicycle, mixed, corrupted or timestamp-free) tracks
//! deliberately fall back to `Unknown`.
use super::{haversine_m, Track, TrackPoint};
use super::recovery::MovementMode;

const WINDOW_SECONDS: f64 = 20.0;
const MAX_WINDOW_SECONDS: f64 = 60.0;
const MAX_FIX_GAP_SECONDS: f64 = 45.0;
const MAX_TRUSTED_HDOP: f64 = 30.0;
const MAX_STEP_SPEED_M_S: f64 = 55.0;

#[derive(Default)]
struct Evidence {
    windows: usize,
    foot: usize,
    vehicle: usize,
    intermediate: usize,
    foot_distance: f64,
    vehicle_distance: f64,
    trusted_lat: Vec<f64>,
    trusted_lon: Vec<f64>,
}

fn timestamp(p: &TrackPoint) -> Option<f64> {
    p.time.filter(|t| t.is_finite())
}

fn trustworthy_window(points: &[TrackPoint]) -> Option<(f64, f64)> {
    let start = points.first()?;
    let end = points.last()?;
    let dt = timestamp(end)? - timestamp(start)?;
    if !(WINDOW_SECONDS..=MAX_WINDOW_SECONDS).contains(&dt) || points.len() < 4 {
        return None;
    }
    // Reject windows crossing serious receiver-quality degradation. Missing
    // HDOP is not automatically poor HDOP (e.g. GPX recorded without it).
    let poor = points.iter().filter(|p| {
        p.hdop.is_some_and(|h| !h.is_finite() || h < 0.0 || h > MAX_TRUSTED_HDOP)
    }).count();
    if poor * 4 > points.len() || [start, end].iter().any(|p| {
        p.hdop.is_some_and(|h| !h.is_finite() || h < 0.0 || h > MAX_TRUSTED_HDOP)
    }) {
        return None;
    }
    let displacement = haversine_m(start.lat, start.lon, end.lat, end.lon);
    if !displacement.is_finite() { return None; }
    let mut length = 0.0;
    for pair in points.windows(2) {
        let step_dt = timestamp(&pair[1])? - timestamp(&pair[0])?;
        if step_dt <= 0.0 || step_dt > MAX_FIX_GAP_SECONDS { return None; }
        let step = haversine_m(pair[0].lat, pair[0].lon, pair[1].lat, pair[1].lon);
        if !step.is_finite() || (step >= 35.0 && step / step_dt > MAX_STEP_SPEED_M_S) {
            return None;
        }
        length += step;
    }
    let speed = displacement / dt;
    if speed > 45.0 { return None; }
    // Single GPS jumps often produce a large net displacement but a much
    // larger travelled polyline; they are not vehicle-speed evidence.
    if speed >= 7.0 && length > displacement * 2.2 + 30.0 { return None; }

    // Receiver speed, when recorded, is corroboration. Do not interpret its
    // absence as a zero speed. Use a window median, not a single bad sample.
    let mut measured: Vec<f64> = points.iter().filter_map(|p| {
        let hdop_ok = p.hdop.map_or(true, |h| {
            h.is_finite() && h >= 0.0 && h <= MAX_TRUSTED_HDOP
        });
        if !hdop_ok { return None; }
        p.speed_m_s.filter(|v| v.is_finite() && *v >= 0.0)
    }).collect();
    if measured.len() >= 3 {
        measured.sort_by(|a,b| a.total_cmp(b));
        let recorded = measured[measured.len() / 2];
        if speed >= 7.0 && recorded < 4.0 { return None; }
        if speed <= 3.3 && recorded > 7.0 { return None; }
    }
    Some((speed, displacement))
}

/// Return a single profile for the entire track. No per-segment switching.
/// A 20-second displacement suppresses short GPS hedgehogs and point spikes;
/// independent windows and minimum travelled distance prevent classification
/// from one anomalous measurement or a stationary receiver.
pub fn infer_movement_mode(track: &Track) -> MovementMode {
    let mut e = Evidence::default();
    for seg in &track.segments {
        let points = &seg.points;
        let mut first = 0;
        while first + 3 < points.len() {
            let Some(t0) = timestamp(&points[first]) else { first += 1; continue; };
            let mut last = first + 1;
            while last < points.len() {
                match timestamp(&points[last]) {
                    Some(t) if t > t0 && t - t0 < WINDOW_SECONDS => last += 1,
                    _ => break,
                }
            }
            if last >= points.len() { break; }
            if let Some((speed, distance)) = trustworthy_window(&points[first..=last]) {
                e.windows += 1;
                e.trusted_lat.push(points[first].lat);
                e.trusted_lon.push(points[first].lon);
                if (0.5..=3.3).contains(&speed) {
                    e.foot += 1;
                    e.foot_distance += distance;
                } else if (7.0..=45.0).contains(&speed) {
                    e.vehicle += 1;
                    e.vehicle_distance += distance;
                } else if speed > 3.3 && speed < 7.0 {
                    e.intermediate += 1;
                }
            }
            // Disjoint windows: one burst cannot cast dozens of votes.
            first = last;
        }
    }
    if e.windows < 8 { return MovementMode::Unknown; }
    // Fast evidence must be sustained and outweigh genuinely slow motion.
    // Stationary windows do not defeat a vehicle profile (traffic/parking).
    if e.vehicle >= 6 && e.vehicle * 4 >= e.windows
        && e.vehicle > e.foot && e.vehicle_distance >= 400.0 {
        return MovementMode::Vehicle;
    }
    // A stationary GPS receiver with a ~15 m 'hedgehog' cannot establish a
    // walking profile: require substantial spatial extent as well as repeated
    // slow-motion windows. Trim the outer 10% of coordinate samples to prevent
    // one isolated bad fix from manufacturing the extent of a real walk.
    e.trusted_lat.sort_by(|a, b| a.total_cmp(b));
    e.trusted_lon.sort_by(|a, b| a.total_cmp(b));
    let n = e.trusted_lat.len();
    let low = n / 10;
    let high = n - 1 - low;
    let extent = haversine_m(e.trusted_lat[low], e.trusted_lon[low],
        e.trusted_lat[high], e.trusted_lon[high]);
    if e.foot >= 7 && e.foot * 3 >= e.windows
        && e.vehicle == 0 && e.intermediate * 10 <= e.windows
        && e.foot_distance >= 150.0 && extent.is_finite() && extent >= 65.0 {
        return MovementMode::Foot;
    }
    MovementMode::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FileSource, TrackProps, TrackSegment};

    fn make_track(speed_m_s: f64, fixes: usize) -> Track {
        let pts = (0..fixes).map(|i| TrackPoint {
            lat: 55.0, lon: 37.0 + i as f64 * speed_m_s / 63_770.0,
            ele: None, time: Some(i as f64), hdop: Some(5.0),
            speed_m_s: Some(speed_m_s),
        }).collect();
        Track { id: 1, name: "profile".into(), color: [0,0,0], visible: true,
            segments: vec![TrackSegment { points: pts }], source: FileSource::New,
            props: TrackProps::default(), cached_points: 0, cached_distance_m: 0.0 }
    }

    #[test]
    fn stable_vehicle_and_walking_are_distinct() {
        assert_eq!(infer_movement_mode(&make_track(14.0, 301)), MovementMode::Vehicle);
        assert_eq!(infer_movement_mode(&make_track(1.35, 301)), MovementMode::Foot);
    }

    #[test]
    fn short_and_timestamp_free_tracks_remain_unknown() {
        assert_eq!(infer_movement_mode(&make_track(14.0, 60)), MovementMode::Unknown);
        let mut track = make_track(1.35, 301);
        for p in &mut track.segments[0].points { p.time = None; }
        assert_eq!(infer_movement_mode(&track), MovementMode::Unknown);
    }

    #[test]
    fn solitary_teleport_does_not_turn_walking_into_driving() {
        let mut track = make_track(1.4, 501);
        track.segments[0].points[231].lon += 0.02;
        track.segments[0].points[231].hdop = Some(1000.0);
        assert_eq!(infer_movement_mode(&track), MovementMode::Foot);
    }

    #[test]
    fn stationary_hedgehog_is_not_walking_or_driving() {
        let mut track = make_track(0.0, 501);
        for (i, p) in track.segments[0].points.iter_mut().enumerate() {
            // Repeated small stationary drifts within roughly 15 meters.
            p.lon += ((i * 17 % 29) as f64 - 14.0) / 63_770.0;
            p.speed_m_s = None;
        }
        assert_eq!(infer_movement_mode(&track), MovementMode::Unknown);
    }

    #[test]
    fn bicycle_speeds_remain_ambiguous() {
        assert_eq!(infer_movement_mode(&make_track(5.0, 401)), MovementMode::Unknown);
    }

    #[test]
    fn segmented_track_never_bridges_an_excluded_gap() {
        let mut track = make_track(14.0, 601);
        let p = &mut track.segments[0].points;
        let tail = p.split_off(300);
        track.segments.push(TrackSegment { points: tail });
        assert_eq!(infer_movement_mode(&track), MovementMode::Vehicle);
    }
}
