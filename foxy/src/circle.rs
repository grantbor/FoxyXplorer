//! Temporary interactive circle. Committed circles are ordinary polygon tracks.
//! Distances and area are measured on a spherical Earth, not in screen pixels.
use iced::{Point, Size};
use crate::util::geo::{lonlat_of, merc_of};

const EARTH_RADIUS_M: f64 = 6_371_000.0;
const RING_STEPS: usize = 128;
const EDGE_TOLERANCE_PX: f32 = 12.0;

#[derive(Debug, Clone)]
pub(crate) struct CircleDraft {
    pub center_lon: f64,
    pub center_lat: f64,
    pub radius_m: f64,
    pub fixed: bool,
    pub dragging_edge: bool,
}

impl CircleDraft {
    pub fn new(lon: f64, lat: f64) -> Self {
        Self { center_lon: lon, center_lat: lat, radius_m: 0.0, fixed: false, dragging_edge: false }
    }

    pub fn set_radius_to(&mut self, lon: f64, lat: f64) {
        self.radius_m = crate::domain::haversine_m(self.center_lat, self.center_lon, lat, lon)
            .clamp(0.0, 5_000_000.0);
    }

    pub fn pointer_moved(&mut self, lon: f64, lat: f64) -> bool {
        if !self.fixed || self.dragging_edge {
            self.set_radius_to(lon, lat);
            true
        } else {
            false
        }
    }

    pub fn lock(&mut self, lon: f64, lat: f64) {
        if !self.fixed {
            self.set_radius_to(lon, lat);
            self.fixed = true;
        }
    }

    pub fn can_commit(&self) -> bool {
        self.fixed && !self.dragging_edge && self.radius_m >= 1.0
    }

    pub fn area_m2(&self) -> f64 {
        4.0 * std::f64::consts::PI * EARTH_RADIUS_M.powi(2)
            * (self.radius_m / (2.0 * EARTH_RADIUS_M)).sin().powi(2)
    }

    /// Clockwise geodesic destinations, closing at exactly the first point.
    pub fn ring(&self) -> Vec<(f64, f64)> {
        if self.radius_m < 1.0 { return Vec::new(); }
        let lat0 = self.center_lat.to_radians();
        let lon0 = self.center_lon.to_radians();
        let distance = self.radius_m / EARTH_RADIUS_M;
        let mut result = Vec::with_capacity(RING_STEPS + 1);
        for i in 0..RING_STEPS {
            let angle = i as f64 * std::f64::consts::TAU / RING_STEPS as f64;
            let lat = (lat0.sin() * distance.cos()
                + lat0.cos() * distance.sin() * angle.cos()).clamp(-1.0, 1.0).asin();
            let dl = (angle.sin() * distance.sin() * lat0.cos())
                .atan2(distance.cos() - lat0.sin() * lat.sin());
            // Keep longitudes unwrapped about the centre, including at the dateline.
            let lon = (lon0 + dl).to_degrees();
            // Mercator cannot represent a pole; protect the preview and output.
            result.push((lon, lat.to_degrees().clamp(-85.051_128_78, 85.051_128_78)));
        }
        result.push(result[0]);
        result
    }

    /// Closest position ON the circle outline, provided the pointer is close enough
    /// to grab it. Shared by hover feedback and actual dragging.
    pub fn edge_hit(&self, pointer: Point, size: Size, cx: f64, cy: f64, scale: f64) -> Option<Point> {
        if !self.fixed || self.radius_m < 1.0 { return None; }
        let mut closest = None;
        let mut best_distance = EDGE_TOLERANCE_PX;
        let mut previous: Option<Point> = None;
        for (lon, lat) in self.ring() {
            let (mx, my) = merc_of(lon, lat);
            let current = Point::new(
                size.width / 2.0 + ((mx - cx) / scale) as f32,
                size.height / 2.0 - ((my - cy) / scale) as f32,
            );
            if let Some(a) = previous {
                let dx = current.x - a.x;
                let dy = current.y - a.y;
                let length_sq = dx * dx + dy * dy;
                let t = if length_sq > 0.0 {
                    (((pointer.x - a.x) * dx + (pointer.y - a.y) * dy) / length_sq).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let on_edge = Point::new(a.x + t * dx, a.y + t * dy);
                let distance = (pointer.x - on_edge.x).hypot(pointer.y - on_edge.y);
                if distance <= best_distance {
                    best_distance = distance;
                    closest = Some(on_edge);
                }
            }
            previous = Some(current);
        }
        closest
    }

    pub fn start_edge_drag(&mut self, pointer: Point, size: Size, cx: f64, cy: f64, scale: f64) -> bool {
        self.dragging_edge = self.edge_hit(pointer, size, cx, cy, scale).is_some();
        self.dragging_edge
    }
}

pub(crate) fn map_point_to_geo(p: Point, size: Size, cx: f64, cy: f64, scale: f64) -> (f64, f64) {
    lonlat_of(cx + (p.x as f64 - size.width as f64 / 2.0) * scale,
        cy - (p.y as f64 - size.height as f64 / 2.0) * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geodesic_circle_is_closed_with_requested_radius() {
        let mut circle = CircleDraft::new(11.4041, 47.2692);
        circle.radius_m = 1250.0;
        let ring = circle.ring();
        assert_eq!(ring.len(), RING_STEPS + 1);
        assert_eq!(ring.first(), ring.last());
        for &(lon, lat) in ring.iter().take(RING_STEPS) {
            let distance = crate::domain::haversine_m(circle.center_lat, circle.center_lon, lat, lon);
            assert!((distance - circle.radius_m).abs() < 0.02);
        }
        assert!((circle.area_m2() / (std::f64::consts::PI * 1250.0_f64.powi(2)) - 1.0).abs() < 0.00001);
    }

    #[test]
    fn second_click_locks_and_only_edge_drag_changes_radius() {
        let mut circle = CircleDraft::new(11.0, 47.0);
        circle.pointer_moved(11.01, 47.0);
        circle.lock(11.01, 47.0);
        let initial_radius = circle.radius_m;
        assert!(circle.can_commit());
        assert!(!circle.pointer_moved(11.1, 47.0));
        assert_eq!(circle.radius_m, initial_radius);
        let (cx, cy) = merc_of(11.0, 47.0);
        let size = Size::new(1000.0, 800.0);
        assert!(!circle.start_edge_drag(Point::new(500.0, 400.0), size, cx, cy, 10.0));
        let (mx, my) = merc_of(circle.ring()[0].0, circle.ring()[0].1);
        let p = Point::new(size.width / 2.0 + ((mx - cx) / 10.0) as f32,
            size.height / 2.0 - ((my - cy) / 10.0) as f32);
        assert!(circle.start_edge_drag(p, size, cx, cy, 10.0));
        assert!(circle.pointer_moved(11.02, 47.0));
        assert!(circle.radius_m > initial_radius);
        assert!(!circle.can_commit());
        circle.dragging_edge = false;
        assert!(circle.can_commit());
    }

    #[test]
    fn hover_snaps_to_edge_only_after_second_click() {
        let mut circle = CircleDraft::new(11.0, 47.0);
        let (cx, cy) = merc_of(circle.center_lon, circle.center_lat);
        let size = Size::new(1000.0, 800.0);
        circle.radius_m = 1000.0;
        let (lon, lat) = circle.ring()[0];
        let (mx, my) = merc_of(lon, lat);
        let on_edge = Point::new(size.width / 2.0 + ((mx - cx) / 10.0) as f32,
            size.height / 2.0 - ((my - cy) / 10.0) as f32);
        assert!(circle.edge_hit(on_edge, size, cx, cy, 10.0).is_none());
        circle.lock(lon, lat);
        let nearby = Point::new(on_edge.x, on_edge.y - 6.0);
        let snapped = circle.edge_hit(nearby, size, cx, cy, 10.0).unwrap();
        assert!((snapped.x - on_edge.x).hypot(snapped.y - on_edge.y) < 7.0);
        assert!((snapped.x - nearby.x).hypot(snapped.y - nearby.y) > 3.0);
        assert!(circle.edge_hit(Point::new(500.0, 400.0), size, cx, cy, 10.0).is_none());
    }

    #[test]
    fn unfinished_or_empty_circle_cannot_be_committed() {
        let mut circle = CircleDraft::new(0.0, 0.0);
        assert!(!circle.can_commit());
        circle.lock(0.0, 0.0);
        assert!(!circle.can_commit());
        assert!(circle.ring().is_empty());
    }
}
