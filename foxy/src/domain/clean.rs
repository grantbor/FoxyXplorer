//! Автоматическая очистка треков от GPS-артефактов.
//!
//! Три стадии:
//! A. Обнуление неправдоподобных высот (координаты не трогаем).
//! B. Отсечение изолированных точек через граф скоростной связности
//!    (max_speed оценивается по медиане мгновенных скоростей).
//! C. Удаление коротких "крюков": отлетели далеко и вернулись в ту же точку.

use super::track::{Track, TrackPoint, TrackSegment, recompute_track_stats};

const EARTH_R: f64 = 6_371_000.0;

fn dist(a: &TrackPoint, b: &TrackPoint) -> f64 {
    let (lat1, lat2) = (a.lat.to_radians(), b.lat.to_radians());
    let dlat = (b.lat - a.lat).to_radians();
    let dlon = (b.lon - a.lon).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_R * h.sqrt().asin()
}

pub struct CleanReport {
    pub removed: usize,
    pub segments_after: usize,
}

pub fn clean_track(track: &mut Track) -> CleanReport {
    let all: Vec<&TrackPoint> = track.segments.iter().flat_map(|s| s.points.iter()).collect();
    let max_speed = estimate_max_speed(&all);

    let before: usize = track.segments.iter().map(|s| s.points.len()).sum();
    let mut new_segments = Vec::new();

    for segment in track.segments.drain(..) {
        let mut pts = segment.points;
        if pts.len() < 3 {
            new_segments.push(TrackSegment { points: pts });
            continue;
        }

        clean_elevations(&mut pts);

        let mut keep = find_main_component(&pts, max_speed);
        drop_detours(&pts, &mut keep);

        let kept: Vec<TrackPoint> = pts
            .into_iter()
            .zip(keep)
            .filter_map(|(p, k)| if k { Some(p) } else { None })
            .collect();

        for chunk in split_at_gaps(&kept) {
            if chunk.len() >= 2 {
                new_segments.push(TrackSegment { points: chunk });
            }
        }
    }

    let after: usize = new_segments.iter().map(|s| s.points.len()).sum();
    let segments_after = new_segments.len();
    track.segments = new_segments;
    recompute_track_stats(track);

    CleanReport { removed: before.saturating_sub(after), segments_after }
}

/// Медиана мгновенных скоростей → порог max_speed с клампами.
fn estimate_max_speed(points: &[&TrackPoint]) -> f64 {
    let mut speeds: Vec<f64> = Vec::new();
    for w in points.windows(2) {
        let (Some(t1), Some(t2)) = (w[0].time, w[1].time) else { continue };
        let dt = t2 - t1;
        if !(1.0..=120.0).contains(&dt) { continue; }
        let d = dist(w[0], w[1]);
        if d < 2.0 { continue; }
        speeds.push(d / dt);
    }
    if speeds.len() < 5 {
        return 200.0 / 3.6;
    }
    speeds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p50 = speeds[speeds.len() / 2];
    // 8× медианы, но не меньше 100 км/ч и не больше 300 км/ч.
    (p50 * 8.0).clamp(100.0 / 3.6, 300.0 / 3.6)
}

fn clean_elevations(pts: &mut [TrackPoint]) {
    const MAX_ELE: f64 = 5000.0;
    const MIN_ELE: f64 = -500.0;
    const MAX_JUMP: f64 = 300.0;
    for i in 0..pts.len() {
        let Some(ele) = pts[i].ele else { continue };
        let out_of_range = ele > MAX_ELE || ele < MIN_ELE;
        let prev_bad = i > 0 && pts[i - 1].ele.map_or(false, |e| (ele - e).abs() > MAX_JUMP);
        let next_bad = i + 1 < pts.len() && pts[i + 1].ele.map_or(false, |e| (ele - e).abs() > MAX_JUMP);
        if out_of_range || (prev_bad && next_bad) {
            pts[i].ele = None;
        }
    }
}

fn find_main_component(pts: &[TrackPoint], max_speed: f64) -> Vec<bool> {
    const GPS_NOISE: f64 = 15.0;
    const REACH_TIME: f64 = 3600.0;
    let n = pts.len();
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    fn union(parent: &mut [usize], a: usize, b: usize) {
        let (ra, rb) = (find(parent, a), find(parent, b));
        if ra != rb { parent[ra] = rb; }
    }

    for i in 0..n {
        let Some(ti) = pts[i].time else { continue };
        for j in (i + 1)..n {
            let Some(tj) = pts[j].time else { continue };
            let dt = tj - ti;
            if dt > REACH_TIME { break; }
            if dist(&pts[i], &pts[j]) <= max_speed * dt + 2.0 * GPS_NOISE {
                union(&mut parent, i, j);
            }
        }
    }

    let mut sizes: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        *sizes.entry(r).or_insert(0) += 1;
    }
    let main_root = sizes.into_iter().max_by_key(|&(_, s)| s).map(|(r, _)| r);

    let mut keep = vec![false; n];
    if let Some(main) = main_root {
        for i in 0..n {
            keep[i] = find(&mut parent, i) == main;
        }
    }
    keep
}

/// Убираем «крюки»: от точки A далеко ушли и вернулись почти в A.
/// Условие: distance(A,B) < 600 м, путь > 5 км, ratio > 30, между ними ≤ 20 точек.
fn drop_detours(pts: &[TrackPoint], keep: &mut [bool]) {
    for _ in 0..50 {
        let kept: Vec<usize> = (0..pts.len()).filter(|&i| keep[i]).collect();
        if kept.len() < 3 { return; }
        let mut found = false;
        'outer: for i in 0..kept.len().saturating_sub(2) {
            let p_i = &pts[kept[i]];
            let mut path = 0.0;
            for j in (i + 2)..kept.len().min(i + 22) {
                path += dist(&pts[kept[j - 1]], &pts[kept[j]]);
                let direct = dist(p_i, &pts[kept[j]]);
                if direct < 600.0 && path > 5000.0 && path / direct.max(100.0) > 30.0 {
                    for k in (i + 1)..j {
                        keep[kept[k]] = false;
                    }
                    found = true;
                    break 'outer;
                }
            }
        }
        if !found { return; }
    }
}

fn split_at_gaps(pts: &[TrackPoint]) -> Vec<Vec<TrackPoint>> {
    const GAP_SEC: f64 = 300.0;
    const GAP_DIST: f64 = 100.0;
    let mut result = Vec::new();
    let mut current: Vec<TrackPoint> = Vec::new();
    for pt in pts {
        if let Some(last) = current.last() {
            if let (Some(t1), Some(t2)) = (last.time, pt.time) {
                if t2 - t1 > GAP_SEC && dist(last, pt) > GAP_DIST {
                    result.push(std::mem::take(&mut current));
                }
            }
        }
        current.push(pt.clone());
    }
    if !current.is_empty() { result.push(current); }
    result
}
