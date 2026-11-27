//! Conservative diagnosis of GPX chronology and coordinate anomalies.
//!
//! This module does not mutate tracks. The one-click command may safely
//! reorder the original with Undo or select suspect fixes for manual editing.
//! "Not flagged" does NOT mean "verified".
use std::cmp::Ordering;

use super::{haversine_m, recompute_track_stats, quality, Track, TrackPoint, TrackSegment};

// Deliberately conservative, independent of the user's mode of transport.
// Thresholds are screening parameters, NOT a statistical claim of accuracy.
// Small Wi-Fi / network-assisted fixes are best detected with a three-point
// return test. The modes control sensitivity, not how the source was damaged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementMode { Foot, Vehicle, Unknown }

impl std::fmt::Display for MovementMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Foot => crate::t!("msg_0467"),
            Self::Vehicle => crate::t!("msg_0468"),
            Self::Unknown => crate::t!("msg_0469"),
        };
        write!(f, "{name}")
    }
}

#[derive(Clone, Copy)]
struct Thresholds {
    jump_m: f64,
    jump_speed: f64,
    large_jump_speed: f64,
    single_min_leg_m: f64,
    single_min_speed: f64,
    single_min_saving_m: f64,
    max_bridge_speed: f64,
    block_min_saving_m: f64,
}

impl MovementMode {
    fn thresholds(self) -> Thresholds {
        match self {
            // Walking includes running; these are deliberately *review* limits.
            Self::Foot => Thresholds {
                jump_m: 24.0, jump_speed: 6.0, large_jump_speed: 6.0,
                single_min_leg_m: 12.0, single_min_speed: 3.5,
                single_min_saving_m: 20.0, max_bridge_speed: 5.0,
                block_min_saving_m: 40.0,
            },
            Self::Vehicle => Thresholds {
                jump_m: 80.0, jump_speed: 48.0, large_jump_speed: 30.0,
                single_min_leg_m: 50.0, single_min_speed: 18.0,
                single_min_saving_m: 85.0, max_bridge_speed: 50.0,
                block_min_saving_m: 250.0,
            },
            Self::Unknown => Thresholds {
                jump_m: 3_000.0, jump_speed: 60.0, large_jump_speed: 45.0,
                single_min_leg_m: 70.0, single_min_speed: 25.0,
                single_min_saving_m: 120.0, max_bridge_speed: 55.0,
                block_min_saving_m: 450.0,
            },
        }
    }
}
const MAX_CANDIDATE_SECONDS: f64 = 1_800.0;
const MAX_CANDIDATE_POINTS: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diagnosis {
    Chronology,
    SuspectCoordinates,
    InsufficientData,
    NoObviousAnomalies,
}

/// Inclusive zero-based point indices within one original GPX segment.
#[derive(Debug, Clone)]
pub struct SuspectRange {
    pub segment: usize,
    pub start: usize,
    pub end: usize,
    pub entry_m: f64,
    pub exit_m: f64,
    pub bridge_m: f64,
}

impl SuspectRange {
    pub fn count(&self) -> usize { self.end - self.start + 1 }
}

/// A severe step with no automatically inferred side to exclude.
#[derive(Debug, Clone)]
pub struct SevereJump {
    pub segment: usize,
    pub to_index: usize,
    pub distance_m: f64,
    pub speed_m_s: f64,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub diagnosis: Diagnosis,
    pub points: usize,
    pub reversals: usize,
    pub missing_times: usize,
    pub repeated_times: usize,
    pub original_distance_m: f64,
    pub reordered_distance_m: f64,
    pub original_max_step_m: f64,
    pub reordered_max_step_m: f64,
    pub can_reorder: bool,
    pub severe_transitions: usize,
    pub extensive_damage: bool,
    pub severe_jumps: Vec<SevereJump>,
    pub ranges: Vec<SuspectRange>,
    pub quality: quality::QualityReport,
}

impl Report {
    /// All unaddressed impossible steps remain visible until a range is
    /// actually checked. Suggested-but-unchecked cuts do not resolve jumps.
    pub fn unresolved_jump_indices(&self) -> Vec<usize> {
        self.unresolved_jump_indices_for(std::iter::empty())
    }
    pub fn unresolved_jump_indices_for(&self, selected: impl IntoIterator<Item=usize>) -> Vec<usize> {
        self.unresolved_jump_indices_with_reviews(selected, std::iter::empty())
    }
    pub fn unresolved_jump_indices_with_reviews(&self,
        selected: impl IntoIterator<Item=usize>,
        selected_reviews: impl IntoIterator<Item=usize>) -> Vec<usize> {
        let mut intervals: Vec<(usize,usize,usize)> = selected.into_iter()
            .filter_map(|i| self.ranges.get(i))
            .map(|r| (r.segment,r.start,r.end)).collect();
        intervals.extend(selected_reviews.into_iter()
            .filter_map(|i| self.quality.episodes.get(i))
            .filter(|ep| ep.verdict == quality::Verdict::Review)
            .map(|ep| (ep.segment,ep.start,ep.end)));
        self.severe_jumps.iter().enumerate().filter_map(|(i,j)| {
            let removed_endpoint = intervals.iter().any(|&(seg,start,end)| {
                seg == j.segment &&
                ((j.to_index >= start && j.to_index <= end) ||
                 (j.to_index > 0 && j.to_index - 1 >= start && j.to_index - 1 <= end))
            });
            (!removed_endpoint).then_some(i)
        }).collect()
    }
}

fn distance(a: &TrackPoint, b: &TrackPoint) -> f64 {
    haversine_m(a.lat, a.lon, b.lat, b.lon)
}

fn length_and_max(segments: &[TrackSegment]) -> (f64, f64) {
    let mut total: f64 = 0.0;
    let mut max_step: f64 = 0.0;
    for segment in segments {
        for pair in segment.points.windows(2) {
            let d = distance(&pair[0], &pair[1]);
            if d.is_finite() {
                total += d;
                max_step = max_step.max(d);
            }
        }
    }
    (total, max_step)
}

/// Stable, per-segment sort. Never merges segments or invents timestamps.
pub fn chronological_segments(track: &Track) -> Option<Vec<TrackSegment>> {
    let mut segments = track.segments.clone();
    for segment in &mut segments {
        if segment.points.iter().any(|p| p.time.map_or(true, |t| !t.is_finite())) {
            return None;
        }
        segment.points.sort_by(|a, b| {
            // Finite timestamps were checked above.
            a.time.unwrap().total_cmp(&b.time.unwrap())
        });
        if segment.points.windows(2).any(|p| p[0].time == p[1].time) {
            return None;
        }
    }
    Some(segments)
}

fn ordered_is_plausible(segments: &[TrackSegment]) -> bool {
    segments.iter().all(|seg| seg.points.windows(2).all(|p| {
        let dt = p[1].time.unwrap() - p[0].time.unwrap();
        // A long recording gap is not evidence for a trustworthy reconstruction.
        dt > 0.0 && dt <= 120.0 && distance(&p[0], &p[1]) <= 55.0 * dt + 20.0
    }))
}

fn identify_teleports(segments: &[TrackSegment], mode: MovementMode) -> (Vec<SevereJump>, Vec<SuspectRange>) {
    let t = mode.thresholds();
    let mut jumps = Vec::new();
    let mut proposals = Vec::new();
    for (seg_idx, seg) in segments.iter().enumerate() {
        let points = &seg.points;
        if points.len() < 3 { continue; }
        // A boundary is suspicious, but does not tell us which side is false.
        let mut severe = Vec::new();
        for i in 1..points.len() {
            let (Some(a), Some(b)) = (points[i-1].time, points[i].time) else { continue; };
            let dt = b - a;
            if !dt.is_finite() || dt <= 0.0 { continue; }
            let dist = distance(&points[i-1], &points[i]);
            let speed = dist / dt;
            // The second rule catches *large* questionable jumps that could be
            // physically possible in a car but deserve manual inspection.
            if (dist >= t.jump_m && speed > t.jump_speed)
                || (dist >= 2_000.0 && speed > t.large_jump_speed)
            {
                jumps.push(SevereJump {
                    segment: seg_idx, to_index: i,
                    distance_m: dist, speed_m_s: speed,
                });
                // Only truly extreme steps may anchor a block proposal.
                if speed > t.jump_speed { severe.push(i); }
            }
        }

        // Independent test for isolated false fixes (typically 20-500 m):
        // both legs are implausibly fast for the selected mode, the two
        // neighbours are mutually reachable, and deleting the middle point
        // saves a large detour. Missing or non-monotone timestamps do not pass.
        for i in 1..points.len()-1 {
            let (Some(a), Some(b), Some(c)) =
                (points[i-1].time, points[i].time, points[i+1].time) else { continue; };
            let dt1 = b - a;
            let dt2 = c - b;
            if !dt1.is_finite() || !dt2.is_finite() || dt1 <= 0.0 || dt2 <= 0.0
                || dt1 > 60.0 || dt2 > 60.0 { continue; }
            let entry_m = distance(&points[i-1], &points[i]);
            let exit_m = distance(&points[i], &points[i+1]);
            let bridge_m = distance(&points[i-1], &points[i+1]);
            if !entry_m.is_finite() || !exit_m.is_finite() || !bridge_m.is_finite() { continue; }
            if entry_m < t.single_min_leg_m || exit_m < t.single_min_leg_m
                || entry_m / dt1 < t.single_min_speed
                || exit_m / dt2 < t.single_min_speed
                || bridge_m / (dt1 + dt2) > t.max_bridge_speed
                || (entry_m + exit_m) / bridge_m.max(6.0) < 3.5
                || entry_m + exit_m - bridge_m < t.single_min_saving_m
            { continue; }
            proposals.push((entry_m + exit_m - bridge_m, SuspectRange {
                segment: seg_idx, start: i, end: i,
                entry_m, exit_m, bridge_m,
            }));
        }

        // Longer excursions still use a paired-boundary hypothesis. Do not
        // claim certainty: a legitimate detour can also be a long loop.
        for (k, &entry) in severe.iter().enumerate() {
            for &exit in severe.iter().skip(k + 1) {
                if exit <= entry || exit - entry > MAX_CANDIDATE_POINTS { break; }
                let dt = points[exit].time.unwrap() - points[entry-1].time.unwrap();
                if dt > MAX_CANDIDATE_SECONDS { break; }
                if dt <= 0.0 { continue; }

                let entry_m = distance(&points[entry-1], &points[entry]);
                let exit_m = distance(&points[exit-1], &points[exit]);
                let bridge_m = distance(&points[entry-1], &points[exit]);
                if bridge_m / dt > t.max_bridge_speed { continue; }
                if (entry_m + exit_m) / bridge_m.max(200.0) < 2.5 { continue; }
                let saving = entry_m + exit_m - bridge_m;
                if saving < t.block_min_saving_m { continue; }
                proposals.push((saving, SuspectRange {
                    segment: seg_idx,
                    start: entry,
                    end: exit - 1,
                    entry_m,
                    exit_m,
                    bridge_m,
                }));
            }
        }
    }
    // Greedy nonoverlap is only a *suggestion* selection heuristic. All flags
    // begin unchecked in the UI, and leftover transitions remain visible.
    proposals.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
    let mut ranges: Vec<SuspectRange> = Vec::new();
    for (_, proposed) in proposals {
        if ranges.iter().any(|existing| {
            existing.segment == proposed.segment
                && proposed.start <= existing.end && proposed.end >= existing.start
        }) { continue; }
        ranges.push(proposed);
    }
    ranges.sort_by_key(|r| (r.segment, r.start));
    (jumps, ranges)
}

pub fn analyze_with_mode(track: &Track, mode: MovementMode) -> Report {
    let points = track.segments.iter().map(|s| s.points.len()).sum();
    let (original_distance_m, original_max_step_m) = length_and_max(&track.segments);
    let mut reversals = 0;
    let mut missing_times = 0;
    let mut repeated_times = 0;
    for seg in &track.segments {
        missing_times += seg.points.iter()
            .filter(|p| p.time.map_or(true, |t| !t.is_finite())).count();
        for pair in seg.points.windows(2) {
            if let (Some(a), Some(b)) = (pair[0].time, pair[1].time) {
                if a.is_finite() && b.is_finite() {
                    if b < a { reversals += 1; }
                    if b == a { repeated_times += 1; }
                }
            }
        }
    }

    let candidate = if reversals > 0 && missing_times == 0 {
        chronological_segments(track)
    } else { None };
    let (reordered_distance_m, reordered_max_step_m, can_reorder) = if let Some(ref segments) = candidate {
        let (len, max) = length_and_max(segments);
        let success = len < original_distance_m * 0.75
            && max < original_max_step_m * 0.5
            && ordered_is_plausible(segments);
        (len, max, success)
    } else { (original_distance_m, original_max_step_m, false) };

    // Coordinate suspicion is always measured in the original point order.
    // After a safe reorder the user can inspect the derived copy again.
    let (severe_jumps, mut ranges) = identify_teleports(&track.segments, mode);
    let quality = quality::analyze(&track.segments, mode);
    // A multi-signal episode takes precedence over overlapping isolated
    // geometric guesses. Each is still unchecked until the user confirms it.
    for episode in &quality.episodes {
        if episode.verdict != quality::Verdict::Suggest { continue; }
        let Some(seg) = track.segments.get(episode.segment) else { continue };
        if episode.start == 0 || episode.end+1 >= seg.points.len() { continue; }
        let r = SuspectRange {
            segment: episode.segment,
            start: episode.start, end: episode.end,
            entry_m: distance(&seg.points[episode.start-1],&seg.points[episode.start]),
            exit_m: distance(&seg.points[episode.end],&seg.points[episode.end+1]),
            bridge_m: distance(&seg.points[episode.start-1],&seg.points[episode.end+1]),
        };
        // Preserve a broader geometric hypothesis when a shorter quality
        // episode intersects it. Previous code discarded an entire teleport
        // candidate on ANY overlap, causing v3 to regress on severe tracks.
        if ranges.iter().any(|old| old.segment == r.segment
            && old.start <= r.start && old.end >= r.end) {
            continue;
        }
        ranges.retain(|old| old.segment != r.segment
            || old.start < r.start || old.end > r.end);
        ranges.push(r);
    }
    ranges.sort_by_key(|r| (r.segment, r.start));
    let severe_transitions = severe_jumps.len();
    // Count damage density, not only the total number of jumps. Localized
    // spoofed clusters should not condemn an otherwise long useful recording.
    let speed_samples: Vec<f64> = track.segments.iter()
        .flat_map(|s| s.points.iter()).filter_map(|p| p.speed_m_s)
        .filter(|v| v.is_finite() && *v >= 0.0).collect();
    let plateau = speed_samples.len() >= 100 && severe_transitions >= 2 && {
        let mut ordered = speed_samples.clone();
        ordered.sort_by(|a, b| a.total_cmp(b));
        let typical = ordered[ordered.len() / 2];
        let stable_fast = ordered.iter().filter(|&&v| (v - typical).abs() <= 1.0).count();
        typical >= 50.0 && stable_fast * 10 >= ordered.len() * 7
    };
    let jump_density = points > 0 && severe_transitions >= 10
        && severe_transitions as f64 / points as f64 > 0.02;
    let extensive_damage = !can_reorder && (plateau || jump_density);
    // Extensive damage means the derived geometry is NOT trustworthy. It
    // must never hide concrete findings: keep the individually unchecked
    // hypotheses, but warn the operator not to treat a bulk cut as recovery.
    // Previous v3 cleared every range here, losing v2's useful diagnostics.
    // The preview warns when most points would disappear.
    let diagnosis = if can_reorder {
        Diagnosis::Chronology
    } else if !ranges.is_empty() || !quality.episodes.is_empty() || severe_transitions > 0 {
        Diagnosis::SuspectCoordinates
    } else if missing_times > 0 || repeated_times > 0 || reversals > 0 {
        Diagnosis::InsufficientData
    } else {
        Diagnosis::NoObviousAnomalies
    };
    Report {
        diagnosis, points, reversals, missing_times, repeated_times,
        original_distance_m, reordered_distance_m,
        original_max_step_m, reordered_max_step_m,
        can_reorder, severe_transitions, extensive_damage, severe_jumps, ranges, quality,
    }
}

/// Build independent GPX segments. A removed region never becomes a connecting edge.
/// Single-point segments are retained rather than silently discarded.
pub fn exclude_ranges(track: &Track, ranges: &[SuspectRange]) -> Vec<TrackSegment> {
    let mut result = Vec::new();
    for (seg_idx, seg) in track.segments.iter().enumerate() {
        let mut excluded = vec![false; seg.points.len()];
        for r in ranges.iter().filter(|r| r.segment == seg_idx) {
            for index in r.start..=r.end.min(seg.points.len().saturating_sub(1)) {
                if let Some(mark) = excluded.get_mut(index) { *mark = true; }
            }
        }
        let mut chunk = Vec::new();
        for (index, pt) in seg.points.iter().enumerate() {
            if excluded[index] {
                if !chunk.is_empty() {
                    result.push(TrackSegment { points: std::mem::take(&mut chunk) });
                }
            } else {
                chunk.push(pt.clone());
            }
        }
        if !chunk.is_empty() { result.push(TrackSegment { points: chunk }); }
    }
    result
}

/// Name and metadata are set by the caller. This function does not modify `source`.
pub fn with_segments(original: &Track, segments: Vec<TrackSegment>) -> Track {
    let mut copy = original.clone();
    copy.segments = segments;
    recompute_track_stats(&mut copy);
    copy
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FileSource, TrackProps};

    fn pt(lat: f64, lon: f64, time: f64) -> TrackPoint {
        TrackPoint { lat, lon, time: Some(time), ele: None, hdop: None, speed_m_s: None }
    }
    fn example(points: Vec<TrackPoint>) -> Track {
        Track {
            id: 7, name: "test".into(), visible: true, color: [255, 0, 0],
            segments: vec![TrackSegment { points }],
            source: FileSource::New, props: TrackProps::default(),
            cached_points: 0, cached_distance_m: 0.0,
        }
    }

    #[test]
    fn chronology_reconstruction_keeps_every_point() {
        let mut points: Vec<_> = (0..120)
            .map(|i| pt(55.0, 82.0 + i as f64 * 0.00001, i as f64)).collect();
        for i in (20..100).step_by(5) { points.swap(i, i + 2); }
        let t = example(points);
        let report = analyze_with_mode(&t, MovementMode::Unknown);
        assert_eq!(report.diagnosis, Diagnosis::Chronology);
        assert!(report.can_reorder);
        let sorted = chronological_segments(&t).unwrap();
        assert_eq!(sorted[0].points.len(), 120);
        assert!(sorted[0].points.windows(2).all(|p| p[0].time < p[1].time));
    }

    #[test]
    fn teleport_block_is_flagged_without_chronology_change() {
        let mut points = Vec::new();
        for i in 0..40 {
            let lon = 37.0 + i as f64 * 0.00001;
            let lon = if (15..26).contains(&i) { lon + 1.5 } else { lon };
            points.push(pt(56.0, lon, i as f64));
        }
        let t = example(points);
        let report = analyze_with_mode(&t, MovementMode::Unknown);
        assert_eq!(report.reversals, 0);
        assert!(!report.can_reorder);
        assert_eq!(report.diagnosis, Diagnosis::SuspectCoordinates);
        assert!(report.ranges.iter().any(|r| r.start == 15 && r.end == 25));
    }

    #[test]
    fn removal_creates_real_gap_and_keeps_original() {
        let t = example((0..7).map(|i| pt(55.0, 82.0 + i as f64, i as f64)).collect());
        let before = t.segments[0].points.len();
        let chunks = exclude_ranges(&t, &[SuspectRange {
            segment: 0, start: 2, end: 4,
            entry_m: 0.0, exit_m: 0.0, bridge_m: 0.0,
        }]);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].points.len(), 2);
        assert_eq!(chunks[1].points.len(), 2);
        assert_eq!(t.segments[0].points.len(), before);
    }

    #[test]
    fn walking_isolated_40_meter_fix_is_proposed() {
        let mut points: Vec<_> = (0..20)
            .map(|i| pt(55.0, 37.0 + i as f64 * 0.000008, i as f64)).collect();
        points[10].lat += 0.0004; // approx 44 m, then returns
        let t = example(points);
        let r = analyze_with_mode(&t, MovementMode::Foot);
        assert!(!r.can_reorder);
        assert_eq!(r.reversals, 0);
        assert!(r.ranges.iter().any(|r| r.start == 10 && r.end == 10));
        let cleaned = exclude_ranges(&t, &r.ranges);
        assert_eq!(cleaned.iter().map(|s| s.points.len()).sum::<usize>(), 19);
        assert_eq!(cleaned.len(), 2); // no false connecting line
    }

    #[test]
    fn vehicle_isolated_150_meter_fix_is_proposed() {
        let mut points: Vec<_> = (0..25)
            .map(|i| pt(55.0, 37.0 + i as f64 * 0.00012, i as f64)).collect();
        points[12].lat += 0.0014; // approx 156 m
        let r = analyze_with_mode(&example(points), MovementMode::Vehicle);
        assert!(r.ranges.iter().any(|r| r.start == 12 && r.end == 12));
    }

    #[test]
    fn genuine_fast_straight_car_motion_is_not_a_spike() {
        let points: Vec<_> = (0..40)
            .map(|i| pt(55.0, 37.0 + i as f64 * 0.00047, i as f64)).collect();
        let r = analyze_with_mode(&example(points), MovementMode::Vehicle);
        assert!(r.ranges.is_empty());
        assert!(r.severe_jumps.is_empty());
    }

    #[test]
    fn missing_time_does_not_create_isolated_candidate() {
        let mut t = example(vec![
            pt(55.0, 37.0, 0.0), pt(55.0004, 37.0, 1.0),
            pt(55.0, 37.00001, 2.0),
        ]);
        t.segments[0].points[1].time = None;
        let r = analyze_with_mode(&t, MovementMode::Foot);
        assert!(r.ranges.is_empty());
    }

    #[test]
    fn isolated_proposal_preserves_neighbours_and_original() {
        let mut points: Vec<_> = (0..10)
            .map(|i| pt(55.0, 37.0 + i as f64 * 0.00001, i as f64)).collect();
        points[5].lat += 0.0004;
        let t = example(points);
        let ranges = analyze_with_mode(&t, MovementMode::Foot).ranges;
        let chunks = exclude_ranges(&t, &ranges);
        assert_eq!(t.segments[0].points.len(), 10);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].points.len(), 5);
        assert_eq!(chunks[1].points.len(), 4);
    }

    #[test]
    fn unmatched_jump_is_reported_without_a_deletion_proposal() {
        let t = example(vec![
            pt(55.0, 37.0, 0.0),
            pt(55.4, 37.0, 1.0),
            pt(55.40001, 37.0, 2.0),
        ]);
        let report = analyze_with_mode(&t, MovementMode::Foot);
        assert!(report.ranges.is_empty());
        assert_eq!(report.unresolved_jump_indices(), vec![0]);
    }

    #[test]
    fn duplicate_or_missing_times_never_sort() {
        let mut t = example(vec![pt(55.0, 82.0, 3.0), pt(55.0, 82.0001, 1.0)]);
        t.segments[0].points.push(pt(55.0, 82.0002, 1.0));
        assert!(chronological_segments(&t).is_none());
        t.segments[0].points[2].time = None;
        assert!(chronological_segments(&t).is_none());
        assert!(!analyze_with_mode(&t, MovementMode::Unknown).can_reorder);
    }
    #[test]
    fn proposal_is_not_counted_resolved_until_selected() {
        let mut points: Vec<_> = (0..20)
            .map(|i| pt(55.0,37.0+i as f64*0.00001,i as f64)).collect();
        points[10].lat += 0.0015;
        let r = analyze_with_mode(&example(points), MovementMode::Foot);
        assert!(!r.ranges.is_empty());
        assert!(r.unresolved_jump_indices().len() >= 2);
        assert!(r.unresolved_jump_indices_for([0]).len() < r.unresolved_jump_indices().len());
    }
    #[test]
    fn old_trackpoint_json_can_be_read_without_quality_fields() {
        let point: TrackPoint = serde_json::from_str(
            r#"{"lat":55.0,"lon":37.0,"ele":null,"time":1234.0}"#).unwrap();
        assert_eq!(point.hdop, None);
        assert_eq!(point.speed_m_s, None);
    }
    #[test]
    fn systemic_damage_does_not_hide_detected_ranges() {
        let mut points=Vec::new();
        for i in 0..160 {
            let mut p=pt(55.0,if i%2==0 {37.0} else {38.0},i as f64);
            p.speed_m_s=Some(56.0);
            points.push(p);
        }
        let r=analyze_with_mode(&example(points),MovementMode::Vehicle);
        assert!(r.extensive_damage);
        // Explicitly unchecked in the UI. Warning must NOT erase the
        // underlying geometric diagnosis, as v3 did on GBR-like tracks.
        assert!(!r.ranges.is_empty());
    }

}
