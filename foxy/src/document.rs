#![allow(dead_code)]
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

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
pub enum FileSource {
    File { path: PathBuf, read_only: bool },
    New,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: u64,
    pub name: String,
    pub color: [u8; 3],
    pub visible: bool,
    pub segments: Vec<TrackSegment>,
    pub source: FileSource,

    // Кэш статистики для правой панели
    #[serde(skip)]
    pub cached_points: usize,
    #[serde(skip)]
    pub cached_distance_m: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Waypoint {
    pub id: u64,
    pub name: String,
    pub lon: f64,
    pub lat: f64,
    pub ele: Option<f64>,
    pub time: Option<f64>,
    pub symbol: Option<String>,
    pub description: Option<String>,
    pub visible: bool,
    pub source: FileSource,
}

pub struct LoadedContent {
    pub tracks: Vec<Track>,
    pub waypoints: Vec<Waypoint>,
    pub notes: Vec<String>,
}

fn next_color(idx: usize) -> [u8; 3] {
    const PALETTE: [[u8; 3]; 8] = [
        [220, 60, 60],   // red
        [60, 120, 220],  // blue
        [60, 180, 80],   // green
        [220, 160, 40],  // orange
        [160, 60, 200],  // purple
        [40, 180, 180],  // teal
        [220, 100, 160], // pink
        [130, 130, 130], // gray
    ];
    PALETTE[idx % PALETTE.len()]
}

fn parse_gpx_color(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().to_lowercase();
    match s.as_str() {
        "red" => Some([255, 0, 0]),
        "green" => Some([0, 255, 0]),
        "blue" => Some([0, 0, 255]),
        "magenta" => Some([255, 0, 255]),
        "cyan" => Some([0, 255, 255]),
        "yellow" => Some([255, 255, 0]),
        "black" => Some([0, 0, 0]),
        "white" => Some([255, 255, 255]),
        _ => {
            if s.starts_with('#') && s.len() == 7 {
                let r = u8::from_str_radix(&s[1..3], 16).ok()?;
                let g = u8::from_str_radix(&s[3..5], 16).ok()?;
                let b = u8::from_str_radix(&s[5..7], 16).ok()?;
                Some([r, g, b])
            } else {
                None
            }
        }
    }
}

/// Ozi BGR -> RGB
fn ozi_to_rgb(c: u32) -> [u8; 3] {
    [(c & 0xFF) as u8, ((c >> 8) & 0xFF) as u8, ((c >> 16) & 0xFF) as u8]
}

/// RGB -> Ozi BGR
fn rgb_to_ozi(c: [u8; 3]) -> u32 {
    (c[0] as u32) | ((c[1] as u32) << 8) | ((c[2] as u32) << 16)
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


pub fn load_user_file(path: &Path, next_id: &mut u64) -> Result<LoadedContent, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let ext = path.extension().and_then(|s| s.to_str()).map(|s| s.to_lowercase()).unwrap_or_default();
    
    let (is_read_only, data) = match ext.as_str() {
        "gpx" => (true, geoio::gpx::read(&bytes)),
        "plt" => (false, geoio::plt::read(&bytes)),
        "ms" => (false, geoio::ms::read(&bytes)),
        "wpt" => (false, geoio::wpt::read(&bytes)),
        _ => return Err(format!("формат не поддерживается: {}", path.display())),
    };
    
    let gd = data.map_err(|e| format!("{}: {e}", path.display()))?;
    
    let mut tracks = Vec::new();
    for (i, t) in gd.tracks.into_iter().enumerate() {
        let color = if let Some(c) = t.gpx_color.as_deref().and_then(parse_gpx_color) {
            c
        } else if t.style.color != 0 {
            ozi_to_rgb(t.style.color)
        } else {
            next_color(i)
        };
        
        let mut segments = Vec::new();
        for seg in t.segments {
            let points = seg.into_iter().map(|p| TrackPoint {
                lon: p.lon,
                lat: p.lat,
                ele: p.ele_m,
                time: p.time,
            }).collect();
            segments.push(TrackSegment { points });
        }
        
        let name = if t.name.is_empty() {
            format!("{}: trk#{}", path.file_stem().unwrap_or_default().to_string_lossy(), i + 1)
        } else {
            t.name
        };
        
        let mut track = Track {
            id: *next_id,
            name,
            color,
            visible: true,
            segments,
            source: FileSource::File { path: path.to_path_buf(), read_only: is_read_only },
            cached_points: 0,
            cached_distance_m: 0.0,
        };
        recompute_track_stats(&mut track);
        tracks.push(track);
        *next_id += 1;
    }
    
    let mut waypoints = Vec::new();
    for (i, w) in gd.waypoints.into_iter().enumerate() {
        let name = if w.name.is_empty() {
            format!("{}: wp#{}", path.file_stem().unwrap_or_default().to_string_lossy(), i + 1)
        } else {
            w.name
        };
        
        waypoints.push(Waypoint {
            id: *next_id,
            name,
            lon: w.lon,
            lat: w.lat,
            ele: w.ele_m,
            time: w.time,
            symbol: None,
            description: if w.description.is_empty() { None } else { Some(w.description) },
            visible: true,
            source: FileSource::File { path: path.to_path_buf(), read_only: is_read_only },
        });
        *next_id += 1;
    }
    
    Ok(LoadedContent {
        tracks,
        waypoints,
        notes: gd.warnings,
    })
}

pub fn save_gpx(tracks: &[&Track], waypoints: &[&Waypoint], path: &Path) -> Result<(), String> {
    let mut gd = geoio::GeoData::default();
    
    for t in tracks {
        let segments = t.segments.iter().map(|seg| {
            seg.points.iter().map(|p| geoio::TrackPoint {
                lat: p.lat,
                lon: p.lon,
                ele_m: p.ele,
                time: p.time,
            }).collect()
        }).collect();
        
        let color_str = if t.color != [0,0,0] {
            Some(format!("#{:02X}{:02X}{:02X}", t.color[0], t.color[1], t.color[2]))
        } else { None };
        
        gd.tracks.push(geoio::Track {
            name: t.name.clone(),
            segments,
            gpx_color: color_str,
            ..Default::default()
        });
    }
    
    for w in waypoints {
        gd.waypoints.push(geoio::Waypoint {
            name: w.name.clone(),
            lon: w.lon,
            lat: w.lat,
            ele_m: w.ele,
            time: w.time,
            description: w.description.clone().unwrap_or_default(),
        });
    }
    
    let xml = geoio::gpx::write(&gd);
    std::fs::write(path, xml).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn save_plt(track: &Track, path: &Path) -> Result<(), String> {
    let segments = track.segments.iter().map(|seg| {
        seg.points.iter().map(|p| geoio::TrackPoint {
            lat: p.lat,
            lon: p.lon,
            ele_m: p.ele,
            time: p.time,
        }).collect()
    }).collect();
    
    let mut geo_track = geoio::Track {
        name: track.name.clone(),
        segments,
        ..Default::default()
    };
    geo_track.style.color = rgb_to_ozi(track.color);
    
    let bytes = geoio::plt::write(&geo_track, geoio::text::Encoding::Utf8);
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn save_wpt(waypoints: &[&Waypoint], path: &Path) -> Result<(), String> {
    let geo_wps: Vec<geoio::Waypoint> = waypoints.iter().map(|w| geoio::Waypoint {
        name: w.name.clone(),
        lon: w.lon,
        lat: w.lat,
        ele_m: w.ele,
        time: w.time,
        description: w.description.clone().unwrap_or_default(),
    }).collect();
    
    let bytes = geoio::wpt::write(&geo_wps, geoio::text::Encoding::Utf8);
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn save_ms(tracks: &[&Track], waypoints: &[&Waypoint], path: &Path) -> Result<(), String> {
    let mut gd = geoio::GeoData::default();
    
    for t in tracks {
        let segments = t.segments.iter().map(|seg| {
            seg.points.iter().map(|p| geoio::TrackPoint {
                lat: p.lat,
                lon: p.lon,
                ele_m: p.ele,
                time: p.time,
            }).collect()
        }).collect();
        
        gd.tracks.push(geoio::Track {
            name: t.name.clone(),
            segments,
            ..Default::default()
        });
    }
    
    for w in waypoints {
        gd.waypoints.push(geoio::Waypoint {
            name: w.name.clone(),
            lon: w.lon,
            lat: w.lat,
            ele_m: w.ele,
            time: w.time,
            description: w.description.clone().unwrap_or_default(),
        });
    }
    
    let map_name = path.file_stem().unwrap_or_default().to_string_lossy();
    let xml = geoio::ms::write(&gd, &map_name, &geoio::ms::MsStyle::default());
    std::fs::write(path, xml).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn save_any(tracks: &[&Track], waypoints: &[&Waypoint], path: &Path) -> Result<(), String> {
    let ext = path.extension().and_then(|s| s.to_str()).map(|s| s.to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "gpx" => save_gpx(tracks, waypoints, path),
        "plt" => {
            if !waypoints.is_empty() { return Err("Формат PLT не поддерживает waypoints".into()); }
            if tracks.len() != 1 { return Err("Формат PLT может содержать только один трек".into()); }
            save_plt(tracks[0], path)
        },
        "wpt" => {
            if !tracks.is_empty() { return Err("Формат WPT не поддерживает треки".into()); }
            save_wpt(waypoints, path)
        },
        "ms" => save_ms(tracks, waypoints, path),
        _ => Err(format!("Формат {} не поддерживается для сохранения", ext)),
    }
}
