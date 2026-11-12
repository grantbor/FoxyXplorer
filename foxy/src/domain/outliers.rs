//! Удаление выбросов: комбинированный адаптивный подход.

use crate::domain::{haversine_m, TrackPoint};

const SPEED_K: f64 = 8.0;
const SPEED_MIN: f64 = 40.0;
const SPEED_MAX: f64 = 250.0;
const DETOUR_RATIO: f64 = 1.6;
const SPATIAL_THRESHOLD_M: f64 = 500.0;
const MAX_ITER: usize = 10;

fn speed_m_s(a: &TrackPoint, b: &TrackPoint) -> f64 {
    let (ta, tb) = match (a.time, b.time) {
        (Some(x), Some(y)) => (x, y),
        _ => return 0.0,
    };
    let dt = (tb - ta).abs();
    if dt < 1e-3 { return 0.0; }
    haversine_m(a.lat, a.lon, b.lat, b.lon) / dt
}

/// Порог скорости = медиана скоростей на движении × K.
fn adaptive_threshold(points: &[TrackPoint]) -> f64 {
    let mut speeds: Vec<f64> = points.windows(2).filter_map(|w| {
        let (t1, t2) = (w[0].time?, w[1].time?);
        let dt = (t2 - t1).abs();
        if dt < 0.5 { return None; }
        let d = haversine_m(w[0].lat, w[0].lon, w[1].lat, w[1].lon);
        if d < 5.0 { return None; }
        Some(d / dt)
    }).collect();

    if speeds.is_empty() { return SPEED_MAX; }
    speeds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let med = speeds[speeds.len() / 2];
    (med * SPEED_K).clamp(SPEED_MIN, SPEED_MAX)
}

/// Один проход скоростной детекции + детекции полу-выбросов.
fn speed_pass(points: &[TrackPoint]) -> Vec<TrackPoint> {
    let th = adaptive_threshold(points);
    let n = points.len();
    if n < 3 { return points.to_vec(); }

    let mut to_remove = vec![false; n];

    // 1. Кластеры: плохой вход + плохой выход + хорошее переподключение.
    let mut a = 1;
    while a < n - 1 {
        if speed_m_s(&points[a-1], &points[a]) <= th { a += 1; continue; }
        let mut found = false;
        for b in a..(n - 1) {
            if speed_m_s(&points[b], &points[b+1]) <= th { continue; }
            if speed_m_s(&points[a-1], &points[b+1]) <= th {
                for k in a..=b { to_remove[k] = true; }
                a = b + 2;
                found = true;
                break;
            }
        }
        if !found { a += 1; }
    }

    // 2. Полу-выбросы: одно ребро плохое, детур значительный.
    for i in 1..n - 1 {
        if to_remove[i] { continue; }
        let bad_in = speed_m_s(&points[i-1], &points[i]) > th;
        let bad_out = speed_m_s(&points[i], &points[i+1]) > th;
        if bad_in == bad_out { continue; }
        if speed_m_s(&points[i-1], &points[i+1]) > th { continue; }

        let d_in = haversine_m(points[i-1].lat, points[i-1].lon, points[i].lat, points[i].lon);
        let d_out = haversine_m(points[i].lat, points[i].lon, points[i+1].lat, points[i+1].lon);
        let d_direct = haversine_m(points[i-1].lat, points[i-1].lon, points[i+1].lat, points[i+1].lon);
        if d_direct > 1.0 && (d_in + d_out) / d_direct > DETOUR_RATIO {
            to_remove[i] = true;
        }
    }

    points.iter().enumerate()
        .filter(|(i, _)| !to_remove[*i])
        .map(|(_, p)| p.clone())
        .collect()
}

/// Один проход пространственного фильтра с заданным окном.
fn spatial_pass(points: &[TrackPoint], window: usize) -> Vec<TrackPoint> {
    let n = points.len();
    if n < window * 2 + 1 { return points.to_vec(); }

    let mut to_remove = vec![false; n];
    for i in 0..n {
        let start = i.saturating_sub(window);
        let end = (i + window + 1).min(n);
        let mut lats: Vec<f64> = (start..i).map(|j| points[j].lat)
            .chain((i+1..end).map(|j| points[j].lat)).collect();
        let mut lons: Vec<f64> = (start..i).map(|j| points[j].lon)
            .chain((i+1..end).map(|j| points[j].lon)).collect();
        if lats.is_empty() { continue; }
        lats.sort_by(|a, b| a.partial_cmp(b).unwrap());
        lons.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let (mlat, mlon) = (lats[lats.len()/2], lons[lons.len()/2]);
        if haversine_m(points[i].lat, points[i].lon, mlat, mlon) > SPATIAL_THRESHOLD_M {
            to_remove[i] = true;
        }
    }

    points.iter().enumerate()
        .filter(|(i, _)| !to_remove[*i])
        .map(|(_, p)| p.clone())
        .collect()
}

/// Каскад: скоростной → пространственный (малое окно) → пространственный (среднее) → пространственный (большое).
pub fn remove_outliers(points: &[TrackPoint]) -> Vec<TrackPoint> {
    let mut cur = points.to_vec();
    for _ in 0..MAX_ITER {
        let before = cur.len();
        cur = speed_pass(&cur);
        cur = spatial_pass(&cur, 5);
        cur = spatial_pass(&cur, 15);
        cur = spatial_pass(&cur, 30);
        if cur.len() == before { break; }
    }
    cur
}
