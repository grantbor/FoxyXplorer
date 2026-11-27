//! Copernicus DEM GLO-30 adaptive-colour viewport overlay.
//!
//! Pilot implementation: bounded whole-file GeoTIFF downloads, a separate
//! numeric-data disk cache and one colour scale for the entire viewport.
//! Production follow-up: windowed COG HTTP Range access, to avoid loading a
//! complete degree tile merely to view a small region.
use std::collections::HashMap;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use iced::widget::image::Handle;
// Copernicus COGs use floating-point TIFF predictor 3 (not decoded by the
// `tiff` crate); rusty_tiff supports DEFLATE + predictor 3.
use rusty_tiff::{Decoder, DecodingResult};

use crate::util::geo::lonlat_of;

const BASE_URL: &str = "https://copernicus-dem-30m.s3.eu-central-1.amazonaws.com";
const MAX_TILES: usize = 4;
const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_CACHE_AGE: Duration = Duration::from_secs(90 * 24 * 3600);
const DEBOUNCE: Duration = Duration::from_secs(1);
static PART_ID: AtomicU64 = AtomicU64::new(0);

// Equal points on a continuous palette; the ruler uses exactly this function.
const PALETTE: [[u8; 3]; 7] = [
    [18, 70, 165], [0, 138, 195], [16, 189, 156], [130, 206, 72],
    [237, 232, 78], [242, 139, 50], [216, 44, 53],
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct View {
    pub cx: f64,
    pub cy: f64,
    pub scale: f64,
    pub width: u32,
    pub height: u32,
}

impl View {
    pub(crate) fn new(cx: f64, cy: f64, scale: f64, width: f32, height: f32) -> Self {
        Self {
            cx, cy, scale,
            width: width.max(1.0).round() as u32,
            height: height.max(1.0).round() as u32,
        }
    }

    fn lonlat_at(&self, x: f64, y: f64) -> (f64, f64) {
        lonlat_of(
            self.cx + (x - self.width as f64 / 2.0) * self.scale,
            self.cy - (y - self.height as f64 / 2.0) * self.scale,
        )
    }
}

#[derive(Debug)]
pub(crate) struct Rendered {
    pub(crate) view: View,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
    pub(crate) lower: f32,
    pub(crate) upper: f32,
}

pub(crate) struct Overlay {
    pub(crate) view: View,
    pub(crate) image: Handle,
    pub(crate) lower: f32,
    pub(crate) upper: f32,
}

#[derive(Default)]
pub(crate) struct State {
    pub(crate) enabled: bool,
    observed: Option<View>,
    deadline: Option<Instant>,
    generation: u64,
    loading: Option<u64>,
    displayed: Option<Overlay>,
}

impl State {
    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.observed = None;
        self.deadline = None;
        self.loading = None;
        self.displayed = None;
        self.generation = self.generation.wrapping_add(1);
    }

    /// Called after any app update, including map movement and resize.
    pub(crate) fn sync(&mut self, current: View) {
        if !self.enabled { return; }
        if self.observed != Some(current) {
            self.observed = Some(current);
            self.deadline = Some(Instant::now() + DEBOUNCE);
            self.displayed = None;
            self.generation = self.generation.wrapping_add(1);
        }
    }

    // Resizing the right sidebar changes the canvas size only on the *next*
    // frame. Start the timer now and let its next tick read the updated bounds.
    pub(crate) fn force_refresh(&mut self) {
        if self.enabled {
            self.observed = None;
            self.deadline = Some(Instant::now() + DEBOUNCE);
            self.displayed = None;
            self.generation = self.generation.wrapping_add(1);
        }
    }

    pub(crate) fn wants_tick(&self) -> bool {
        self.enabled && self.deadline.is_some()
    }

    pub(crate) fn begin_if_ready(&mut self, now: Instant) -> Option<(u64, View)> {
        if !self.enabled || self.deadline.is_none_or(|at| now < at) { return None; }
        let view = self.observed?;
        self.deadline = None;
        self.loading = Some(self.generation);
        Some((self.generation, view))
    }

    pub(crate) fn complete(&mut self, generation: u64, result: Result<Arc<Rendered>, String>) -> Option<String> {
        if !self.enabled || generation != self.generation || self.loading != Some(generation) {
            return None;
        }
        self.loading = None;
        match result {
            Ok(result) if self.observed == Some(result.view) => {
                self.displayed = Some(Overlay {
                    view: result.view,
                    image: Handle::from_rgba(result.width, result.height, result.rgba.clone()),
                    lower: result.lower,
                    upper: result.upper,
                });
                None
            }
            Ok(_) => None,
            Err(error) => Some(error),
        }
    }

    pub(crate) fn is_loading(&self) -> bool {
        self.enabled && self.loading == Some(self.generation)
    }

    pub(crate) fn visible(&self, current: View) -> Option<&Overlay> {
        if self.enabled {
            self.displayed.as_ref().filter(|ready| ready.view == current)
        } else {
            None
        }
    }
}

pub(crate) fn palette(t: f32) -> [u8; 3] {
    let v = t.clamp(0.0, 1.0) * (PALETTE.len() - 1) as f32;
    let idx = (v.floor() as usize).min(PALETTE.len() - 2);
    let frac = v - idx as f32;
    std::array::from_fn(|channel| {
        (PALETTE[idx][channel] as f32 * (1.0 - frac)
            + PALETTE[idx + 1][channel] as f32 * frac).round() as u8
    })
}

fn tile_name(lat: i32, lon: i32) -> String {
    format!("Copernicus_DSM_COG_10_{}{:02}_00_{}{:03}_00_DEM",
        if lat >= 0 { 'N' } else { 'S' }, lat.abs(),
        if lon >= 0 { 'E' } else { 'W' }, lon.abs())
}

fn cache_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(|| PathBuf::from("."))
        .join("foxy").join("dem").join("glo30")
}

// Numeric tiles live apart from OSM/ESRI/Sentinel raster tiles.
// Purge old files and evict the oldest valid GeoTIFFs above the 2 GiB limit.
fn prune_disk_cache(dir: &std::path::Path) {
    let mut entries = Vec::new();
    let Ok(items) = std::fs::read_dir(dir) else { return; };
    for item in items.flatten() {
        let path = item.path();
        if path.extension().and_then(|s| s.to_str()) != Some("tif") { continue; }
        let Ok(meta) = item.metadata() else { continue; };
        if !meta.is_file() { continue; }
        let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        if modified.elapsed().is_ok_and(|age| age > MAX_CACHE_AGE) {
            let _ = std::fs::remove_file(path);
        } else {
            entries.push((modified, meta.len(), path));
        }
    }
    entries.sort_by_key(|(modified, _, _)| *modified);
    let mut total: u64 = entries.iter().map(|(_, size, _)| *size).sum();
    for (_, size, path) in entries {
        if total <= MAX_CACHE_BYTES { break; }
        if std::fs::remove_file(path).is_ok() { total = total.saturating_sub(size); }
    }
}

async fn get_tile(client: &reqwest::Client, lat: i32, lon: i32) -> Result<Vec<u8>, String> {
    let name = tile_name(lat, lon);
    let path = cache_dir().join(format!("{name}.tif"));
    if let Ok(bytes) = tokio::fs::read(&path).await {
        if bytes.len() >= 8 && bytes.len() as u64 <= MAX_FILE_BYTES {
            return Ok(bytes);
        }
    }
    let url = format!("{BASE_URL}/{name}/{name}.tif");
    let response = client.get(url).timeout(Duration::from_secs(90)).send().await
        .map_err(|e| format!("Copernicus {name}: {e}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(format!("Copernicus {name}: no public GLO-30 data"));
    }
    let response = response.error_for_status()
        .map_err(|e| format!("Copernicus {name}: {e}"))?;
    if response.content_length().is_some_and(|size| size > MAX_FILE_BYTES) {
        return Err(format!("Copernicus {name}: tile exceeds pilot download limit"));
    }
    let mut response = response;
    let mut data = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if data.len().saturating_add(chunk.len()) as u64 > MAX_FILE_BYTES {
            return Err(format!("Copernicus {name}: tile exceeds pilot download limit"));
        }
        data.extend_from_slice(&chunk);
    }
    if data.len() < 8 { return Err(format!("Copernicus {name}: empty GeoTIFF")); }
    if let Some(parent) = path.parent() {
        if tokio::fs::create_dir_all(parent).await.is_ok() {
            // Each request has its own temporary file. A failing or cancelled
            // download cannot leave a truncated final cache entry.
            let nonce = PART_ID.fetch_add(1, Ordering::Relaxed);
            let part = parent.join(format!("{name}-{}-{nonce}.part", std::process::id()));
            if tokio::fs::write(&part, &data).await.is_ok() {
                if tokio::fs::rename(&part, &path).await.is_ok() {
                    let parent = parent.to_path_buf();
                    let _ = tokio::task::spawn_blocking(move || prune_disk_cache(&parent)).await;
                }
            }
        }
    }
    Ok(data)
}

struct Tile {
    width: u32,
    height: u32,
    heights: Vec<f32>,
}

fn decode_tile(bytes: Vec<u8>) -> Result<Tile, String> {
    let mut decoder = Decoder::new(Cursor::new(bytes)).map_err(|e| format!("DEM GeoTIFF: {e}"))?;
    let (width, height) = decoder.dimensions().map_err(|e| e.to_string())?;
    if width == 0 || height == 0 || (width as u64) * (height as u64) > 30_000_000 {
        return Err("DEM GeoTIFF dimensions outside pilot limits".into());
    }
    let heights = match decoder.read_image().map_err(|e| format!("DEM GeoTIFF: {e}"))? {
        DecodingResult::F32(values) => values,
        DecodingResult::F64(values) => values.into_iter().map(|v| v as f32).collect(),
        other => return Err(format!("Unsupported DEM sample format: {other:?}")),
    };
    if heights.len() != (width as usize) * (height as usize) {
        return Err("Unsupported multiband DEM GeoTIFF".into());
    }
    Ok(Tile { width, height, heights })
}

impl Tile {
    fn sample(&self, lon: f64, lat: f64, west: i32, south: i32) -> f32 {
        // The COG source uses a one-degree grid; use pixel-centre nearest
        // sampling in this pilot. The sub-pixel grid-origin correction and
        // bilinear resampling belong to the windowed COG implementation.
        let xf = (lon - west as f64) * self.width as f64;
        let yf = (south as f64 + 1.0 - lat) * self.height as f64;
        if !(0.0..self.width as f64).contains(&xf)
            || !(0.0..self.height as f64).contains(&yf) {
            return f32::NAN;
        }
        let v = self.heights[yf.floor() as usize * self.width as usize + xf.floor() as usize];
        if v.is_finite() && (-12_000.0..=10_000.0).contains(&v) { v } else { f32::NAN }
    }
}

fn required_tiles(view: View) -> Result<Vec<(i32, i32)>, String> {
    let (west, north) = view.lonlat_at(0.0, 0.0);
    let (east, south) = view.lonlat_at(view.width as f64, view.height as f64);
    if !west.is_finite() || !east.is_finite() || !south.is_finite() || !north.is_finite()
        || west < -180.0 || east > 180.0 || south < -84.0 || north > 84.0
    {
        return Err("DEM pilot: viewport outside supported geographic limits".into());
    }
    let mut tiles = Vec::new();
    for lat in (south.floor() as i32)..=(north.floor() as i32) {
        for lon in (west.floor() as i32)..=(east.floor() as i32) {
            tiles.push((lat, lon));
            if tiles.len() > MAX_TILES {
                return Err("DEM pilot: zoom in to an area of at most four 1-degree tiles".into());
            }
        }
    }
    Ok(tiles)
}

fn render_tiles(view: View, tiles: HashMap<(i32, i32), Tile>) -> Result<Rendered, String> {
    // One sample per ~2 screen pixels; image stretches over the exact viewport.
    let width = view.width.div_ceil(2).clamp(1, 1024);
    let height = view.height.div_ceil(2).clamp(1, 768);
    let mut sampled = Vec::with_capacity(width as usize * height as usize);
    for y in 0..height {
        for x in 0..width {
            let (lon, lat) = view.lonlat_at(
                (x as f64 + 0.5) * view.width as f64 / width as f64,
                (y as f64 + 0.5) * view.height as f64 / height as f64,
            );
            let south = lat.floor() as i32;
            let west = lon.floor() as i32;
            let value = tiles.get(&(south, west))
                .map_or(f32::NAN, |tile| tile.sample(lon, lat, west, south));
            sampled.push(value);
        }
    }
    let mut valid: Vec<f32> = sampled.iter().copied().filter(|h| h.is_finite()).collect();
    if valid.is_empty() { return Err("Copernicus DEM: no valid elevation in viewport".into()); }
    valid.sort_by(f32::total_cmp);
    let lower = valid[((valid.len() - 1) as f64 * 0.02).round() as usize];
    let high = valid[((valid.len() - 1) as f64 * 0.98).round() as usize];
    let upper = if high > lower { high } else { lower + 1.0 };
    let mut rgba = Vec::with_capacity(sampled.len() * 4);
    for h in sampled {
        if h.is_finite() {
            rgba.extend_from_slice(&palette((h - lower) / (upper - lower)));
            rgba.push(170); // preserve labels and terrain detail from the base map
        } else {
            rgba.extend_from_slice(&[0, 0, 0, 0]);
        }
    }
    Ok(Rendered { view, width, height, rgba, lower, upper })
}

pub(crate) async fn load_and_render(view: View, client: reqwest::Client) -> Result<Arc<Rendered>, String> {
    let needed = required_tiles(view)?;
    let mut contents = Vec::with_capacity(needed.len());
    for (lat, lon) in needed {
        contents.push((lat, lon, get_tile(&client, lat, lon).await?));
    }
    let image = tokio::task::spawn_blocking(move || {
        let mut tiles = HashMap::with_capacity(contents.len());
        for (lat, lon, bytes) in contents {
            tiles.insert((lat, lon), decode_tile(bytes)?);
        }
        render_tiles(view, tiles).map(Arc::new)
    }).await.map_err(|e| format!("DEM render worker: {e}"))??;
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copernicus_names_follow_sw_corner() {
        assert_eq!(tile_name(47, 11), "Copernicus_DSM_COG_10_N47_00_E011_00_DEM");
        assert_eq!(tile_name(-3, -7), "Copernicus_DSM_COG_10_S03_00_W007_00_DEM");
    }

    #[test]
    fn palette_has_blue_and_red_endpoints() {
        assert_eq!(palette(0.0), PALETTE[0]);
        assert_eq!(palette(1.0), PALETTE[6]);
        assert_eq!(palette(-100.0), PALETTE[0]);
    }

    #[test]
    fn uniform_heights_have_safe_range() {
        let v = View::new(0.0, 0.0, 10.0, 100.0, 100.0);
        let mut tiles = HashMap::new();
        tiles.insert((0, 0), Tile { width: 1, height: 1, heights: vec![500.0] });
        let at = crate::util::geo::merc_of(0.5, 0.5);
        let v = View::new(at.0, at.1, v.scale, 100.0, 100.0);
        let image = render_tiles(v, tiles).unwrap();
        assert_eq!(image.lower, 500.0);
        assert_eq!(image.upper, 501.0);
    }

    #[test]
    fn debounce_discards_stale_generation() {
        let mut state = State::default();
        state.set_enabled(true);
        let a = View::new(1000.0, 2000.0, 10.0, 600.0, 400.0);
        let b = View::new(1100.0, 2000.0, 10.0, 600.0, 400.0);
        state.sync(a);
        assert!(state.begin_if_ready(Instant::now()).is_none());
        let (old, _) = state.begin_if_ready(Instant::now() + Duration::from_secs(2)).unwrap();
        state.sync(b);
        assert!(state.complete(old, Err("obsolete".into())).is_none());
        assert!(state.visible(a).is_none());
        assert!(state.wants_tick());
    }
}
