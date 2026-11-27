use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use super::FileSource;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackPoint {
    pub lon: f64,
    pub lat: f64,
    pub ele: Option<f64>,
    pub time: Option<f64>,
    /// Optional GPS precision indicator. Old sessions omit this field.
    #[serde(default)]
    pub hdop: Option<f64>,
    /// Optional receiver-measured speed (m/s). Old sessions omit this field.
    #[serde(default)]
    pub speed_m_s: Option<f64>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FillType {
    Solid,
    None,
    Diagonal1,
    Diagonal2,
    CrossHatch,
    DiagCross,
    Horizontal,
    Vertical,
}

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


/// Remove zero-based point indices from their original segments.
///
/// With `leave_gap = true`, every contiguous run of retained points becomes a
/// separate segment. This prevents a fictitious line across deleted samples.
/// With `leave_gap = false`, the original segment boundaries are preserved and
/// retained neighbours are connected (the existing deletion behaviour).
/// Empty segments are discarded, but singleton runs are kept: they are still
/// real observations even though they do not draw a line.
/// Returns the number of points actually removed.
pub fn remove_selected_track_points(
    segments: &mut Vec<TrackSegment>,
    selected: &HashSet<(usize, usize)>,
    leave_gap: bool,
) -> usize {
    if selected.is_empty() { return 0; }

    let mut removed = 0;
    let mut output = Vec::with_capacity(segments.len());
    for (seg_idx, segment) in std::mem::take(segments).into_iter().enumerate() {
        let mut retained = Vec::new();
        for (point_idx, point) in segment.points.into_iter().enumerate() {
            if selected.contains(&(seg_idx, point_idx)) {
                removed += 1;
                if leave_gap && !retained.is_empty() {
                    output.push(TrackSegment { points: std::mem::take(&mut retained) });
                }
            } else {
                retained.push(point);
            }
        }
        if !retained.is_empty() {
            output.push(TrackSegment { points: retained });
        }
    }
    *segments = output;
    removed
}

#[cfg(test)]
mod removal_tests {
    use super::*;

    fn segment(ids: &[usize]) -> TrackSegment {
        TrackSegment { points: ids.iter().map(|&n| TrackPoint {
            lon: n as f64 * 0.001, lat: 0.0, ele: None, time: Some(n as f64),
            hdop: None, speed_m_s: None,
        }).collect() }
    }

    fn ids(segments: &[TrackSegment]) -> Vec<Vec<usize>> {
        segments.iter().map(|s| s.points.iter()
            .map(|p| (p.lon * 1000.0).round() as usize).collect()).collect()
    }

    #[test]
    fn deletes_ranges_and_keeps_their_borders_disconnected() {
        let mut segments = vec![segment(&[0, 1, 2, 3, 4, 5, 6, 7])];
        let selected = HashSet::from([(0, 2), (0, 3), (0, 5)]);
        assert_eq!(remove_selected_track_points(&mut segments, &selected, true), 3);
        assert_eq!(ids(&segments), vec![vec![0, 1], vec![4], vec![6, 7]]);
    }

    #[test]
    fn regular_deletion_still_connects_retained_neighbours() {
        let mut segments = vec![segment(&[0, 1, 2, 3, 4])];
        let selected = HashSet::from([(0, 1), (0, 3)]);
        assert_eq!(remove_selected_track_points(&mut segments, &selected, false), 2);
        assert_eq!(ids(&segments), vec![vec![0, 2, 4]]);
    }

    #[test]
    fn original_segment_boundaries_are_never_joined() {
        let mut segments = vec![segment(&[0, 1, 2]), segment(&[10, 11, 12])];
        let selected = HashSet::from([(0, 1), (1, 0)]);
        assert_eq!(remove_selected_track_points(&mut segments, &selected, true), 2);
        assert_eq!(ids(&segments), vec![vec![0], vec![2], vec![11, 12]]);
    }

    #[test]
    fn deleting_ends_or_every_point_does_not_create_empty_segments() {
        let mut segments = vec![segment(&[0, 1, 2, 3, 4])];
        let selected = HashSet::from([(0, 0), (0, 1), (0, 4)]);
        assert_eq!(remove_selected_track_points(&mut segments, &selected, true), 3);
        assert_eq!(ids(&segments), vec![vec![2, 3]]);

        let all = HashSet::from([(0, 0), (0, 1)]);
        assert_eq!(remove_selected_track_points(&mut segments, &all, true), 2);
        assert!(segments.is_empty());
    }

    #[test]
    fn deleted_region_does_not_contribute_a_bridge_to_length() {
        let mut segments = vec![segment(&[0, 1, 2, 3, 4])];
        let selected = HashSet::from([(0, 2)]);
        remove_selected_track_points(&mut segments, &selected, true);
        assert_eq!(ids(&segments), vec![vec![0, 1], vec![3, 4]]);
        let split_m: f64 = segments.iter().flat_map(|s| s.points.windows(2))
            .map(|p| haversine_m(p[0].lat, p[0].lon, p[1].lat, p[1].lon))
            .sum();
        let stitched: f64 = [0, 1, 3, 4].windows(2).map(|w| {
            haversine_m(0.0, w[0] as f64 * 0.001, 0.0, w[1] as f64 * 0.001)
        }).sum();
        assert!(split_m < stitched);
    }

    #[test]
    fn out_of_range_selection_is_ignored() {
        let mut segments = vec![segment(&[0, 1, 2])];
        assert_eq!(remove_selected_track_points(
            &mut segments, &HashSet::from([(0, 99), (7, 1)]), true,
        ), 0);
        assert_eq!(ids(&segments), vec![vec![0, 1, 2]]);
    }
}

pub fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
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
