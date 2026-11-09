use serde::{Deserialize, Serialize};
use super::FileSource;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackPoint {
    pub lon: f64,
    pub lat: f64,
    pub ele: Option<f64>,
    pub time: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackSegment {
    pub points: Vec<TrackPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LineStyle { Solid, Dash, Dot, DashDot, DashDotDot }
impl Default for LineStyle { fn default() -> Self { Self::Solid } }
impl std::fmt::Display for LineStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{:?}", self) }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PolyType { Line, Polygon }
impl Default for PolyType { fn default() -> Self { Self::Line } }
impl std::fmt::Display for PolyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{:?}", self) }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FillType { None, Diagonal1, Diagonal2, CrossHatch, Solid }
impl Default for FillType { fn default() -> Self { Self::None } }
impl std::fmt::Display for FillType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{:?}", self) }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackProps {
    pub description: String,
    pub line_color: [u8; 3],
    pub line_width: f32,
    pub line_style: LineStyle,
    pub poly_type: PolyType,
    pub fill_color: [u8; 3],
    pub fill_type: FillType,
    
    pub date_str: String,
    pub callsign: String,
    pub num1: u32,
    pub num2: Option<u32>,
    pub suffix: String,
}

impl Default for TrackProps {
    fn default() -> Self {
        Self {
            description: String::new(),
            line_color: [255, 0, 0],
            line_width: 2.0,
            line_style: LineStyle::Solid,
            poly_type: PolyType::Line,
            fill_color: [0, 128, 0],
            fill_type: FillType::None,
            date_str: String::new(),
            callsign: "Lisa".into(),
            num1: 1,
            num2: None,
            suffix: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: u64,
    pub name: String,
    pub color: [u8; 3],
    pub visible: bool,
    pub segments: Vec<TrackSegment>,
    pub source: FileSource,
    pub props: TrackProps,
    
    #[serde(skip)]
    pub cached_points: usize,
    #[serde(skip)]
    pub cached_distance_m: f64,
}

fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6_371_000.0;
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = (lat2 - lat1).to_radians();
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * R * a.sqrt().asin()
}

pub fn recompute_track_stats(track: &mut Track) {
    let mut points = 0usize;
    let mut dist = 0.0f64;
    for seg in &track.segments {
        points += seg.points.len();
        for w in seg.points.windows(2) {
            dist += haversine_m(w[0].lat, w[0].lon, w[1].lat, w[1].lon);
        }
    }
    track.cached_points = points;
    track.cached_distance_m = dist;
}
