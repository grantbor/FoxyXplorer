//! Multi-signal GPS quality diagnosis. Evidence is not ground truth.
//! Runs in O(n log n) per segment and never reconstructs coordinates.
use super::{haversine_m, TrackPoint, TrackSegment};
use super::recovery::MovementMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict { Suggest, Review }

/// Why a v3.4 residual finding needs an operator, rather than an automatic cut.
/// Each reason is diagnostic evidence, never a claim that the fix is false.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewReason { SegmentEdge, TrackTail, QualitySupportedHook, LateralExcursion }

#[derive(Debug, Clone)]
pub struct Episode {
    pub segment: usize,
    pub start: usize,
    pub end: usize,
    pub verdict: Verdict,
    pub reason: Option<ReviewReason>,
    pub bad_fixes: usize,
    pub suspect_steps: usize,
    pub internal_m: f64,
    pub bridge_m: f64,
}
impl Episode {
    pub fn count(&self) -> usize { self.end - self.start + 1 }
}

#[derive(Debug, Clone, Default)]
pub struct QualityReport {
    pub hdop_samples: usize,
    pub speed_samples: usize,
    pub poor_quality_samples: usize,
    pub episodes: Vec<Episode>,
}

fn dist(a: &TrackPoint, b: &TrackPoint) -> f64 {
    haversine_m(a.lat, a.lon, b.lat, b.lon)
}
fn time_delta(a: &TrackPoint, b: &TrackPoint) -> Option<f64> {
    let dt = b.time? - a.time?;
    (dt.is_finite() && dt > 0.0).then_some(dt)
}
fn valid(v: Option<f64>) -> Option<f64> { v.filter(|v| v.is_finite() && *v >= 0.0) }

// A GNSS quality warning is not an excuse to remove stationary observations.
// If measurements are absent, the corresponding evidence is absent too.
fn poor_fix(p: &TrackPoint, dt: f64, hdop_limit: f64) -> bool {
    valid(p.hdop).is_some_and(|h| h > hdop_limit || h > 100.0)
        && dt >= 3.5
        && (p.speed_m_s.is_none() || valid(p.hdop).unwrap_or(0.0) > 100.0)
}

fn bad_movement(a: &TrackPoint, b: &TrackPoint, mode: MovementMode) -> bool {
    let Some(dt) = time_delta(a,b) else { return false; };
    let d = dist(a,b);
    if !d.is_finite() { return false; }
    let v = d/dt;
    let max_speed = match mode {
        MovementMode::Foot => 8.0, MovementMode::Vehicle => 47.0,
        MovementMode::Unknown => 65.0,
    };
    (d >= 40.0 && v > max_speed) ||
        (d >= 32.0 && dt <= 15.0 && valid(b.speed_m_s)
            .is_some_and(|gps| v > (gps * 2.8 + 15.0).max(25.0)))
}

fn evidence(points: &[TrackPoint], first: usize, last: usize,
            poor: &[bool], motion: &[bool], segment: usize, mode: MovementMode) -> Option<Episode> {
    if first >= points.len() || last >= points.len() || first > last { return None; }
    let bad_fixes = poor[first..=last].iter().filter(|&&x| x).count();
    let suspect_steps = motion[first..=last].iter().filter(|&&x| x).count();
    let mut internal_m = 0.0;
    for pair in points[first..=last].windows(2) { internal_m += dist(&pair[0], &pair[1]); }
    let bridge_m = if first>0 && last+1<points.len() {
        dist(&points[first-1], &points[last+1])
    } else { f64::NAN };
    let available = first>0 && last+1<points.len()
        && time_delta(&points[first-1], &points[last+1]).is_some();
    let bridge_speed = if available {
        bridge_m / time_delta(&points[first-1], &points[last+1]).unwrap()
    } else { f64::INFINITY };
    let plausible_bridge = bridge_speed <= match mode {
        MovementMode::Foot => 6.0, MovementMode::Vehicle => 55.0,
        MovementMode::Unknown => 60.0,
    };
    // Quality samples are not proof of a bad coordinate. Require substantial
    // motion as well; a stationary receiver with HDOP=1000 is NOT a deletion.
    let affected = last - first + 1;
    let poor_share = bad_fixes as f64 / affected as f64;
    let missing_speed = points[first..=last].iter()
        .filter(|p| valid(p.speed_m_s).is_none()).count();
    let missing_share = missing_speed as f64 / affected as f64;
    let strong_excursion = suspect_steps >= 2 && internal_m >= 180.0
        && available && plausible_bridge && bridge_m * 2.4 + 80.0 < internal_m;
    // A whole unreliable fix run can move at an entirely plausible car speed.
    // In such cases require MULTIPLE independent receiver-quality signals:
    // sustained poor HDOP, a concurrent loss of measured speed, and a moving
    // geometry. Suggestions are still unchecked and require a preview.
    let persistent_quality_loss = bad_fixes >= 5 && poor_share >= 0.55
        && missing_share >= 0.5 && internal_m >= 150.0;
    let quality_excursion = bad_fixes >= 2 && poor_share >= 0.35
        && internal_m >= 180.0 && available && plausible_bridge
        && bridge_m * 2.2 + 80.0 < internal_m;
    let severe_quality = bad_fixes >= 3 && suspect_steps >= 1
        && (suspect_steps >= 2 || motion[first..=last].iter().enumerate()
            .any(|(j, &flag)| flag && first+j>0 &&
                time_delta(&points[first+j-1], &points[first+j])
                    .is_some_and(|dt| dist(&points[first+j-1], &points[first+j])/dt > 65.0)));
    // Short runs and partially convincing excursions are retained for manual
    // examination, never silently discarded because no cut was inferred.
    let review = (bad_fixes >= 2 && internal_m >= 110.0)
        || (suspect_steps >= 2 && internal_m >= 120.0);
    let verdict = if available && plausible_bridge
        && (strong_excursion || severe_quality || persistent_quality_loss || quality_excursion) {
            Verdict::Suggest
        } else if review { Verdict::Review } else { return None; };
    Some(Episode { segment, start:first, end:last, verdict, reason: None,
        bad_fixes, suspect_steps, internal_m, bridge_m })
}


// v3.4: second, strictly review-only pass for defects that may remain after
// the operator has cut large GPS errors. Each segment is analyzed independently:
// a GPX gap is NEVER an observed movement between adjacent points.
//
// Conservative multi-signal rules are used instead of lowering the base car
// speed threshold. They are not applied to walking/unknown modes.
fn planar_delta(a: &TrackPoint, b: &TrackPoint) -> (f64, f64) {
    const DEG_TO_M: f64 = 111_195.0;
    let lat = (a.lat + b.lat).to_radians() * 0.5;
    let mut dl = b.lon - a.lon;
    if dl > 180.0 { dl -= 360.0; }
    if dl < -180.0 { dl += 360.0; }
    (dl * lat.cos() * DEG_TO_M, (b.lat - a.lat) * DEG_TO_M)
}

fn heading_change_deg(a: &TrackPoint, b: &TrackPoint, c: &TrackPoint) -> Option<f64> {
    let (ax, ay) = planar_delta(a, b);
    let (bx, by) = planar_delta(b, c);
    let len_a = ax.hypot(ay);
    let len_b = bx.hypot(by);
    if len_a < 1.0 || len_b < 1.0 { return None; }
    Some(((ax * bx + ay * by) / (len_a * len_b)).clamp(-1.0, 1.0).acos().to_degrees())
}

/// Perpendicular offset of B from the finite chord A-C, or None if B
/// projects outside it (a legitimate hairpin must not look like a lateral fix).
fn lateral_offset_m(a: &TrackPoint, b: &TrackPoint, c: &TrackPoint) -> Option<f64> {
    let (bx, by) = planar_delta(a, b);
    let (cx, cy) = planar_delta(a, c);
    let chord_sq = cx * cx + cy * cy;
    if chord_sq < 50.0_f64.powi(2) { return None; }
    let projection = (bx * cx + by * cy) / chord_sq;
    if !(0.0..1.0).contains(&projection) { return None; }
    Some((bx * cy - by * cx).abs() / chord_sq.sqrt())
}

fn review_episode(points: &[TrackPoint], segment: usize, start: usize,
                  end: usize, reason: ReviewReason) -> Episode {
    let internal_m: f64 = points[start..=end].windows(2)
        .map(|pair| dist(&pair[0], &pair[1])).sum();
    let bridge_m = if start > 0 && end + 1 < points.len() {
        dist(&points[start-1], &points[end+1])
    } else { f64::NAN };
    let bad_fixes = points[start..=end].iter().filter(|p|
        valid(p.hdop).is_some_and(|h| h >= 40.0)).count();
    let suspect_steps = (start.max(1)..=end).filter(|&i|
        dist(&points[i-1], &points[i]) >= 70.0).count();
    Episode { segment, start, end, verdict: Verdict::Review, reason: Some(reason),
        bad_fixes, suspect_steps, internal_m, bridge_m }
}

fn residual_vehicle_reviews(points: &[TrackPoint], segment: usize) -> Vec<Episode> {
    let n = points.len();
    let mut results = Vec::new();
    if n < 5 { return results; }

    // First fix after an existing GPX split: a high-HDOP near-zero speed fix,
    // followed by a long recording gap and a resumed, better-quality car fix.
    // Without a left neighbour this is ONLY a one-sided review hypothesis.
    let a = &points[0];
    let b = &points[1];
    if valid(a.hdop).is_some_and(|v| v >= 45.0)
        && valid(a.speed_m_s).is_some_and(|v| v <= 1.5)
        && time_delta(a, b).is_some_and(|dt| dt >= 60.0)
        && dist(a, b) >= 150.0
        && valid(b.hdop).is_some_and(|v| v <= 30.0)
        && valid(b.speed_m_s).is_some_and(|v| v >= 4.0)
    {
        results.push(review_episode(points, segment, 0, 0, ReviewReason::SegmentEdge));
    }

    // Last fix BEFORE an existing split: an abrupt, high-HDOP, long sideways
    // displacement relative to the previous sustained heading. No connection
    // across the split is allowed, and no definite false point is inferred.
    let a = &points[n-2];
    let b = &points[n-1];
    let last_distance = dist(a, b);
    if n >= 6 && valid(b.hdop).is_some_and(|v| v >= 35.0)
        && time_delta(a, b).is_some_and(|dt| dt <= 15.0
            && last_distance >= 70.0
            && valid(b.speed_m_s).is_none_or(|v| v <= 2.0 * last_distance / dt))
        && dist(&points[n-4], a) >= 20.0
        && heading_change_deg(&points[n-4], a, b).is_some_and(|v| v >= 55.0)
    {
        results.push(review_episode(points, segment, n-1, n-1, ReviewReason::SegmentEdge));
    }

    // Damaged end of the recording: multiple consecutive very poor fixes
    // without measured speed, plus large *actual* jumps. A stationary stop
    // with HDOP=1000 has no comparable movement evidence and is never flagged.
    let mut tail_start = n;
    while tail_start > 0 {
        let p = &points[tail_start-1];
        if valid(p.hdop).is_some_and(|v| v >= 100.0)
            && valid(p.speed_m_s).is_none() {
            tail_start -= 1;
        } else { break; }
    }
    if tail_start >= 1 && tail_start + 3 <= n {
        let mut large_steps = 0;
        let mut travelled = 0.0;
        for i in tail_start..n {
            if time_delta(&points[i-1], &points[i]).is_none() { continue; }
            let step = dist(&points[i-1], &points[i]);
            if step >= 100.0 { large_steps += 1; }
            travelled += step;
        }
        if large_steps >= 2 && travelled >= 350.0 {
            results.push(review_episode(points, segment, tail_start, n-1,
                ReviewReason::TrackTail));
        }
    }

    // Mid-track candidates: an almost-geometric out-and-back supported by
    // missing receiver speed and very poor HDOP, or a moderate-speed lateral
    // excursion with worsening satellite geometry. Neither is self-proving.
    for i in 1..n-1 {
        let (a,b,c) = (&points[i-1], &points[i], &points[i+1]);
        let (Some(dt1), Some(dt2)) = (time_delta(a,b), time_delta(b,c)) else { continue; };
        if dt1 > 25.0 || dt2 > 25.0 { continue; }
        let ab = dist(a,b);
        let bc = dist(b,c);
        if ab < 40.0 || bc < 40.0 { continue; }
        let ac = dist(a,c);
        let detour = (ab + bc) / ac.max(6.0);
        if valid(b.hdop).is_some_and(|h| h >= 80.0)
            && valid(b.speed_m_s).is_none()
            && ab >= 100.0 && bc >= 100.0
            && detour >= 3.1 && ab + bc - ac >= 200.0 {
            results.push(review_episode(points, segment, i, i,
                ReviewReason::QualitySupportedHook));
            continue;
        }
        if valid(b.hdop).is_some_and(|h| h >= 40.0)
            && valid(c.hdop).is_some_and(|h| h >= 40.0)
            && dt1 >= 4.0 && dt2 >= 4.0
            && ab >= 55.0 && bc >= 55.0
            && lateral_offset_m(a,b,c).is_some_and(|offset| offset >= 60.0)
            && heading_change_deg(a,b,c).is_some_and(|turn| turn >= 60.0) {
            results.push(review_episode(points, segment, i, i,
                ReviewReason::LateralExcursion));
        }
    }
    results
}

pub fn analyze(segments: &[TrackSegment], mode: MovementMode) -> QualityReport {
    let mut report = QualityReport::default();
    for (segment, seg) in segments.iter().enumerate() {
        let pts = &seg.points;
        if pts.len() < 4 { continue; }
        let hdops: Vec<f64> = pts.iter().filter_map(|p| valid(p.hdop)).collect();
        report.hdop_samples += hdops.len();
        report.speed_samples += pts.iter().filter(|p| valid(p.speed_m_s).is_some()).count();
        let mut sorted_hdops = hdops;
        sorted_hdops.sort_by(|a,b| a.total_cmp(b));
        let median_hdop = sorted_hdops.get(sorted_hdops.len()/2).copied().unwrap_or(0.0);
        let hdop_limit = 30.0_f64.max(median_hdop * 2.0);
        let mut poor = vec![false;pts.len()];
        let mut motion = vec![false;pts.len()];
        for i in 1..pts.len() {
            if let Some(dt) = time_delta(&pts[i-1], &pts[i]) {
                poor[i] = poor_fix(&pts[i], dt, hdop_limit);
                motion[i] = bad_movement(&pts[i-1], &pts[i], mode);
                if poor[i] { report.poor_quality_samples += 1; }
            }
        }
        let mut proposals = Vec::new();
        // Group consecutive degraded fixes, allowing up to TWO intermediate
        // good samples. A long sampling gap does not validate the next fix;
        // do not split an otherwise continuous poor-quality run on time alone.
        // The geometric evidence gate below still blocks ordinary pauses.
        let mut i=1;
        while i<pts.len() {
            if !poor[i] { i+=1;continue; }
            let start=i;
            let mut last=i;
            let mut j=i+1;
            while j<pts.len() && j-last<=3 {
                if poor[j] { last=j; }
                j+=1;
            }
            if last>start && poor[start..=last].iter().filter(|&&v|v).count()>=2 {
                // Include the immediate return step when it ends the fix outage.
                let end = if last+1<pts.len() && motion[last+1] &&
                    !valid(pts[last+1].speed_m_s).is_some_and(|v|v>8.0) {
                    last+1
                } else { last };
                if let Some(ep)=evidence(pts,start,end,&poor,&motion,segment,mode) {
                    proposals.push(ep);
                }
            }
            i=last+1;
        }
        // Motion anomalies can be separated by several plausible-looking fixes:
        // a displaced trajectory may be internally smooth yet return home.
        let bad_steps:Vec<usize>=motion.iter().enumerate()
            .filter_map(|(i,&v)|v.then_some(i)).collect();
        let mut begin=0;
        while begin<bad_steps.len() {
            let mut finish=begin;
            while finish+1<bad_steps.len() {
                let a=bad_steps[finish]; let b=bad_steps[finish+1];
                if b-a>28 || time_delta(&pts[a],&pts[b]).is_none_or(|t|t>150.0) { break; }
                finish+=1;
            }
            if finish>begin {
                let first_bad=bad_steps[begin]; let last_bad=bad_steps[finish];
                let mut first=first_bad;
                let min=first_bad.saturating_sub(8).max(1);
                for p in (min..first_bad).rev() {
                    if poor[p] { first=p; }
                }
                let last = if last_bad+1<pts.len() &&
                     valid(pts[last_bad].speed_m_s).is_some_and(|v|v>8.0) {
                     last_bad.saturating_sub(1)
                } else { last_bad };
                if last>first {
                    if let Some(ep)=evidence(pts,first,last,&poor,&motion,segment,mode) {
                        proposals.push(ep);
                    }
                }
            }
            begin=finish+1;
        }
        // Merge overlapping reports, and NEARBY high-confidence quality runs
        // when almost the entire combined area is degraded. Short healthy
        // islands are not enough to split one incident into many checkboxes.
        // A long clean tail is NEVER swept in (e.g. 5693..5970 on the trial GPX).
        proposals.sort_by_key(|p|(p.start,p.end));
        let mut merged: Vec<Episode>=Vec::new();
        for ep in proposals {
            if let Some(previous)=merged.last_mut() {
                if ep.segment==previous.segment && ep.start<=previous.end.saturating_add(6) {
                    let overlap = ep.start<=previous.end;
                    let first=previous.start.min(ep.start);
                    let last=previous.end.max(ep.end);
                    if let Some(combined)=evidence(pts,first,last,&poor,&motion,segment,mode) {
                        let poor_share = combined.bad_fixes as f64 / combined.count() as f64;
                        let nearby_poor_run = ep.verdict==Verdict::Suggest
                            && previous.verdict==Verdict::Suggest
                            && combined.verdict==Verdict::Suggest && poor_share>=0.55;
                        if overlap || nearby_poor_run {
                            *previous=combined;
                            continue;
                        }
                    }
                }
            }
            merged.push(ep);
        }
        // The established high-confidence detector remains authoritative.
        // Additional v3.4 findings enter Review ONLY and do not alter its
        // Suggested intervals; duplicate/covered ranges are not re-listed.
        if mode == MovementMode::Vehicle {
            for residual in residual_vehicle_reviews(pts, segment) {
                if !merged.iter().any(|existing| existing.start <= residual.end
                    && existing.end >= residual.start) {
                    merged.push(residual);
                }
            }
            merged.sort_by_key(|p| (p.start, p.end));
        }
        report.episodes.extend(merged);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(x: f64, t: f64, h: Option<f64>, v: Option<f64>) -> TrackPoint {
        TrackPoint { lat:55.0,lon:37.0+x/64_000.0, ele:None,time:Some(t),hdop:h,speed_m_s:v }
    }
    fn seg(points:Vec<TrackPoint>) -> Vec<TrackSegment> {
        vec![TrackSegment {points}]
    }
    #[test]
    fn poor_hdop_stationary_pause_is_not_a_deletion() {
        let mut pts=Vec::new();
        for i in 0..30 { pts.push(p(0.0,i as f64*6.0,Some(300.0+i as f64),None)); }
        let q=analyze(&seg(pts),MovementMode::Vehicle);
        assert!(q.episodes.is_empty());
        assert!(q.poor_quality_samples>20);
    }
    #[test]
    fn corrupted_fix_run_is_suggested() {
        let mut pts=vec![p(0.0,0.0,Some(2.0),Some(8.0)),p(8.0,1.0,Some(2.0),Some(8.0))];
        for i in 0..12 { pts.push(p(500.0+(i%2) as f64*600.0,7.0+i as f64*6.0,Some(170.0),None)); }
        pts.push(p(700.0,85.0,Some(2.0),Some(8.0)));
        pts.push(p(715.0,86.0,Some(2.0),Some(8.0)));
        let q=analyze(&seg(pts),MovementMode::Vehicle);
        assert!(q.episodes.iter().any(|ep|ep.verdict==Verdict::Suggest));
    }
    #[test]
    fn sustained_loss_with_plausible_vehicle_speeds_is_detected() {
        let mut pts=vec![p(0.0,0.0,Some(2.0),Some(20.0))];
        for i in 1..=12 {
            pts.push(p(i as f64*120.0,i as f64*6.0,Some(160.0),None));
        }
        pts.push(p(13.0*120.0,78.0,Some(2.0),Some(20.0)));
        let q=analyze(&seg(pts),MovementMode::Vehicle);
        assert!(q.episodes.iter().any(|ep|ep.start==1 && ep.end==12
            && ep.verdict==Verdict::Suggest));
    }
    #[test]
    fn good_stationary_tail_is_not_swept_into_the_suspected_run() {
        let mut pts=vec![p(0.0,0.0,Some(2.0),Some(20.0))];
        for i in 1..=12 {
            pts.push(p(i as f64*120.0,i as f64*6.0,Some(160.0),None));
        }
        for i in 13..=200 {
            pts.push(p(13.0*120.0,i as f64*6.0,Some(2.0),Some(0.0)));
        }
        let q=analyze(&seg(pts),MovementMode::Vehicle);
        assert!(q.episodes.iter().any(|ep|ep.start<=1 && ep.end<=13
            && ep.verdict==Verdict::Suggest));
        assert!(q.episodes.iter().all(|ep|ep.end<20));
    }
    #[test]
    fn residual_first_segment_fix_is_review_only() {
        let mut pts = vec![p(0.0, 0.0, Some(51.0), Some(0.0))];
        for i in 1..9 {
            pts.push(p(770.0 + (i - 1) as f64 * 12.0,
                154.0 + (i - 1) as f64, Some(8.0), Some(12.0)));
        }
        let eps = residual_vehicle_reviews(&pts, 0);
        assert!(eps.iter().any(|ep| ep.start == 0 && ep.end == 0
            && ep.reason == Some(ReviewReason::SegmentEdge)
            && ep.verdict == Verdict::Review));
        assert!(residual_vehicle_reviews(&pts, 0).iter().all(|ep| ep.verdict == Verdict::Review));
    }

    #[test]
    fn residual_last_segment_fix_requires_quality_and_heading_change() {
        let mut pts: Vec<_> = (0..10)
            .map(|i| p(i as f64 * 15.0, i as f64, Some(3.0), Some(15.0)))
            .collect();
        pts[9].lat += 0.0012;
        pts[9].time = Some(13.0);
        pts[9].hdop = Some(47.0);
        let candidates = residual_vehicle_reviews(&pts, 0);
        assert!(candidates.iter().any(|ep| ep.start == 9 && ep.end == 9
            && ep.reason == Some(ReviewReason::SegmentEdge)));
        pts[9].hdop = Some(3.0);
        assert!(residual_vehicle_reviews(&pts, 0).iter().all(|ep| ep.end != 9));
    }

    #[test]
    fn very_bad_tail_is_reviewed_without_deleting_a_stationary_pause() {
        let mut pts = vec![p(0.0, 0.0, Some(2.0), Some(0.0))];
        for i in 1..8 { pts.push(p(0.0, i as f64, Some(2.0), Some(0.0))); }
        for (k, x) in [200.0, -200.0, 350.0, -300.0].iter().enumerate() {
            pts.push(p(*x, 20.0 + k as f64 * 12.0, Some(200.0), None));
        }
        assert!(residual_vehicle_reviews(&pts, 0).iter().any(|ep|
            ep.start == 8 && ep.end == 11
            && ep.reason == Some(ReviewReason::TrackTail)
            && ep.verdict == Verdict::Review));
        let stop_lon = pts[7].lon;
        for point in &mut pts[8..] { point.lon = stop_lon; }
        assert!(!residual_vehicle_reviews(&pts, 0).iter().any(|ep|
            ep.reason == Some(ReviewReason::TrackTail)));
    }

    #[test]
    fn borderline_hook_uses_missing_speed_and_hdop_as_extra_evidence() {
        let mut pts: Vec<_> = (0..9)
            .map(|i| p(i as f64 * 12.0, i as f64, Some(3.0), Some(12.0)))
            .collect();
        pts[4].lat += 0.0022; // approx 245 m; two sharp legs
        pts[4].hdop = Some(200.0);
        pts[4].speed_m_s = None;
        pts[4].time = Some(13.0);
        pts[5].time = Some(19.0);
        for i in 6..9 { pts[i].time = Some(19.0 + (i - 5) as f64); }
        assert!(residual_vehicle_reviews(&pts, 0).iter().any(|ep|
            ep.start == 4 && ep.reason == Some(ReviewReason::QualitySupportedHook)));
        pts[4].hdop = Some(5.0);
        assert!(!residual_vehicle_reviews(&pts, 0).iter().any(|ep|
            ep.reason == Some(ReviewReason::QualitySupportedHook)));
    }

    #[test]
    fn medium_speed_sideways_excursion_requires_poor_hdop() {
        let mut pts:Vec<_>=(0..12).map(|i| p(i as f64 * 10.0, i as f64,
            Some(4.0), Some(10.0))).collect();
        // Real measured geometry around the previously overlooked local fix.
        pts[5] = TrackPoint { lat:44.197971,lon:38.881135,time:Some(5.0),
            ele:None,hdop:Some(20.4),speed_m_s:Some(8.9) };
        pts[6] = TrackPoint { lat:44.198730,lon:38.880670,time:Some(15.0),
            ele:None,hdop:Some(50.9),speed_m_s:Some(9.3) };
        pts[7] = TrackPoint { lat:44.198323,lon:38.879201,time:Some(25.0),
            ele:None,hdop:Some(92.9),speed_m_s:None };
        // First compare only a synthetic three-point sample; the function
        // requires at least five points, so add stable context on either side.
        let example=vec![pts[5].clone(),pts[5].clone(),pts[5].clone(),
            pts[6].clone(),pts[7].clone(),pts[7].clone()];
        let results=residual_vehicle_reviews(&example,0);
        assert!(results.iter().any(|ep| ep.start == 3
            && ep.reason == Some(ReviewReason::LateralExcursion)));
        let mut good=example;
        good[3].hdop=Some(3.0);
        assert!(!residual_vehicle_reviews(&good,0).iter().any(|ep| ep.start == 3
            && ep.reason == Some(ReviewReason::LateralExcursion)));
    }

    #[test]
    fn residual_rules_do_not_run_in_unconfirmed_movement_modes() {
        let mut pts = vec![p(0.0, 0.0, Some(51.0), Some(0.0))];
        for i in 1..9 {
            pts.push(p(770.0 + (i - 1) as f64 * 12.0,
                154.0 + (i - 1) as f64, Some(8.0), Some(12.0)));
        }
        let vehicle=analyze(&seg(pts.clone()),MovementMode::Vehicle);
        let unknown=analyze(&seg(pts),MovementMode::Unknown);
        assert!(vehicle.episodes.iter().any(|ep| ep.reason == Some(ReviewReason::SegmentEdge)));
        assert!(unknown.episodes.iter().all(|ep| ep.reason.is_none()));
    }

    #[test]
    fn normal_gap_does_not_trigger_deletion() {
        let points=(0..20).map(|i|p(i as f64*100.0,i as f64*10.0,Some(2.0),Some(10.0))).collect();
        let q=analyze(&seg(points),MovementMode::Vehicle);
        assert!(q.episodes.is_empty());
    }
}
