use crate::domain::{Track, TrackPoint};
use crate::domain::track::recompute_track_stats;

/// Квадрат расстояния от точки p до отрезка ab (в метрах)
fn perp_dist_sq(p: &TrackPoint, a: &TrackPoint, b: &TrackPoint) -> f64 {
    // Плоская аппроксимация (достаточно для треков)
    let lat_c = ((a.lat + b.lat) / 2.0).to_radians();
    let kx = 111_320.0 * lat_c.cos(); // метров в 1 градусе долготы
    let ky = 111_320.0;               // метров в 1 градусе широты

    let ax = a.lon * kx;
    let ay = a.lat * ky;
    let bx = b.lon * kx;
    let by = b.lat * ky;
    let px = p.lon * kx;
    let py = p.lat * ky;

    let dx = bx - ax;
    let dy = by - ay;
    let l2 = dx * dx + dy * dy;

    if l2 == 0.0 {
        return (px - ax).powi(2) + (py - ay).powi(2);
    }

    let t = ((px - ax) * dx + (py - ay) * dy) / l2;
    let t = t.clamp(0.0, 1.0);
    let proj_x = ax + t * dx;
    let proj_y = ay + t * dy;

    (px - proj_x).powi(2) + (py - proj_y).powi(2)
}

/// Рекурсивный алгоритм Дугласа-Пекера для одного сегмента
fn douglas_peucker(pts: &[TrackPoint], tol_sq: f64) -> Vec<TrackPoint> {
    if pts.len() <= 2 {
        return pts.to_vec();
    }

    let mut max_d = 0.0;
    let mut idx = 0;

    for i in 1..pts.len() - 1 {
        let d = perp_dist_sq(&pts[i], &pts[0], &pts[pts.len() - 1]);
        if d > max_d {
            max_d = d;
            idx = i;
        }
    }

    if max_d > tol_sq {
        let mut left = douglas_peucker(&pts[0..=idx], tol_sq);
        let right = douglas_peucker(&pts[idx..], tol_sq);
        
        // Убираем дублирующуюся точку на стыке
        left.pop(); 
        left.extend(right);
        left
    } else {
        vec![pts[0].clone(), pts[pts.len() - 1].clone()]
    }
}

/// Упрощает трек, удаляя точки, которые лежат ближе `tolerance_m` метров к линии
pub fn simplify_track(track: &mut Track, tolerance_m: f64) {
    let tol_sq = tolerance_m * tolerance_m;
    
    for seg in &mut track.segments {
        if seg.points.len() > 2 {
            seg.points = douglas_peucker(&seg.points, tol_sq);
        }
    }
    
    // После упрощения нужно пересчитать кэш (количество точек и расстояние)
    recompute_track_stats(track);
}


pub fn simplify_points(pts: &[TrackPoint], tolerance_m: f64) -> Vec<TrackPoint> {
    if pts.len() <= 2 {
        return pts.to_vec();
    }
    douglas_peucker(pts, tolerance_m * tolerance_m)
}
