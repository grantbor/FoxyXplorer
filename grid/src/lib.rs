//! Сетка поиска (узлы из GPX) и расчёты азимут/удаление на эллипсоиде WGS-84.
use geographiclib_rs::{DirectGeodesic, Geodesic, InverseGeodesic};

#[derive(Debug, thiserror::Error)]
pub enum GridError {
    #[error("ошибка разбора XML: {0}")]
    Xml(String),
    #[error("в GPX нет ни одной именованной точки")]
    Empty,
    #[error("точка {0}: некорректные координаты")]
    BadCoords(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone)]
pub struct Grid {
    pub nodes: Vec<Node>,
}

/// Результат поиска ближайшего узла: расстояние в метрах и
/// МАГНИТНЫЙ азимут от заданной точки на узел, как показывает навигатор.
#[derive(Debug, Clone)]
pub struct NearestNode<'a> {
    pub node: &'a Node,
    pub distance_m: f64,
    pub magnetic_azimuth: f64,
}

fn norm360(a: f64) -> f64 {
    a.rem_euclid(360.0)
}

impl Grid {
    /// Читает путевые точки (<wpt>) с именем. Треки и маршруты игнорируются.
    /// Границы <bounds> не используются: в реальных файлах они неточны.
    pub fn from_gpx(text: &str) -> Result<Grid, GridError> {
        let doc = roxmltree::Document::parse(text.trim_start_matches('\u{feff}'))
            .map_err(|e| GridError::Xml(e.to_string()))?;
        let mut nodes = Vec::new();
        for w in doc.root_element().children().filter(|n| n.has_tag_name("wpt")) {
            let name = w
                .children()
                .find(|c| c.has_tag_name("name"))
                .and_then(|c| c.text())
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let lat = w.attribute("lat").and_then(|v| v.trim().parse::<f64>().ok());
            let lon = w.attribute("lon").and_then(|v| v.trim().parse::<f64>().ok());
            match (lat, lon) {
                (Some(lat), Some(lon))
                    if (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) =>
                {
                    nodes.push(Node { name, lat, lon })
                }
                _ => return Err(GridError::BadCoords(name)),
            }
        }
        if nodes.is_empty() {
            return Err(GridError::Empty);
        }
        Ok(Grid { nodes })
    }

    pub fn find(&self, name: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.name.eq_ignore_ascii_case(name.trim()))
    }

    /// Середина охватывающего прямоугольника узлов (широта, долгота).
    /// Её используем для магнитного склонения.
    pub fn center(&self) -> (f64, f64) {
        let (mut a, mut b, mut c, mut d) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for n in &self.nodes {
            a = a.min(n.lat);
            b = b.max(n.lat);
            c = c.min(n.lon);
            d = d.max(n.lon);
        }
        ((a + b) / 2.0, (c + d) / 2.0)
    }

    /// Ближайший узел к точке. `declination` — восточное склонение в градусах.
    pub fn nearest(&self, lat: f64, lon: f64, declination: f64) -> Option<NearestNode<'_>> {
        let geod = Geodesic::wgs84();
        self.nodes
            .iter()
            .map(|n| {
                let (s12, azi1, _azi2, _a12): (f64, f64, f64, f64) =
                    geod.inverse(lat, lon, n.lat, n.lon);
                (n, s12, azi1)
            })
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .map(|(node, s12, azi1)| NearestNode {
                node,
                distance_m: s12,
                magnetic_azimuth: norm360(azi1 - declination),
            })
    }
}

/// Координата человека по магнитному азимуту и удалению ДО узла сетки.
/// Азимут направлен от человека к узлу, поэтому от узла идём в обратную сторону.
/// `declination` — восточное склонение в градусах (+8°5' = 8.0833...).
pub fn locate_observer(
    node: &Node,
    magnetic_azimuth: f64,
    distance_m: f64,
    declination: f64,
) -> (f64, f64) {
    let true_azimuth = magnetic_azimuth + declination;
    let back = norm360(true_azimuth + 180.0);
    let (lat, lon): (f64, f64) = Geodesic::wgs84().direct(node.lat, node.lon, back, distance_m);
    (lat, lon)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECL: f64 = 8.0 + 5.0 / 60.0; // +8°5', WMM2025

    fn n(name: &str, lat: f64, lon: f64) -> Node {
        Node { name: name.into(), lat, lon }
    }

    /// Узлы взяты из WPT-сетки Новосибирска, ожидания — из OK-Helper.
    /// Допуск 1 м: тесты ловят ошибки знака и направления (десятки метров),
    /// а не округление до 6 знаков (~0.1 м) и погрешность навигатора.
    #[test]
    fn reference_cases() {
        let cases = [
            (n("H12", 54.945816, 83.084312), 120.0, 110.0, 54.946425, 83.082960),
            (n("G9", 54.952618, 83.08076), 32.0, 93.0, 54.951979, 83.079826),
            (n("K7", 54.95684, 83.096598), 275.0, 85.0, 54.956668, 83.097891),
            (n("M9", 54.952215, 83.104166), 315.0, 121.0, 54.951346, 83.105301),
        ];
        for (node, az, d, lat, lon) in cases {
            let (la, lo) = locate_observer(&node, az, d, DECL);
            let (_, back_d, _, _): (f64, f64, f64, f64) =
                (0.0, Geodesic::wgs84().inverse(la, lo, lat, lon), 0.0, 0.0);
            assert!(back_d < 1.0, "{}: расхождение {back_d:.2} м ({la}, {lo})", node.name);
        }
    }

    #[test]
    fn roundtrip_azimuth_and_distance() {
        let node = n("H12", 54.945816, 83.084312);
        let (la, lo) = locate_observer(&node, 120.0, 110.0, DECL);
        let g = Grid { nodes: vec![node] };
        let r = g.nearest(la, lo, DECL).unwrap();
        assert!((r.distance_m - 110.0).abs() < 0.01, "{}", r.distance_m);
        assert!((r.magnetic_azimuth - 120.0).abs() < 0.01, "{}", r.magnetic_azimuth);
    }

    #[test]
    fn parses_gpx_waypoints_only() {
        let gpx = r#"<?xml version="1.0"?>
<gpx version="1.1" xmlns="http://www.topografix.com/GPX/1/1">
  <wpt lat="54.970978" lon="83.058272"><name>A1</name></wpt>
  <wpt lat="54.970911" lon="83.062175"><name>B1</name></wpt>
  <trk><name>Grid_Line</name><trkseg>
    <trkpt lat="1" lon="1"/></trkseg></trk>
</gpx>"#;
        let g = Grid::from_gpx(gpx).unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.find("b1").unwrap().name, "B1");
    }
}
