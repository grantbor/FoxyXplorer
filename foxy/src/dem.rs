//! Copernicus DEM GLO-30 adaptive-colour viewport overlay.
//!
//! Windowed HTTP Range access to Copernicus COG blocks, with a bounded
//! whole-file fallback for uncommon TIFF layouts. The separate cache stores
//! original compressed DEM blocks (or legacy complete GeoTIFFs), not colours.
use std::collections::HashMap;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use iced::widget::image::Handle;
// Copernicus COGs use floating-point TIFF predictor 3 (not decoded by the
// `tiff` crate); rusty_tiff supports DEFLATE + predictor 3.
use rusty_tiff::{Decoder, DecodingResult};

use crate::util::geo::lonlat_of;

const BASE_URL: &str = "https://copernicus-dem-30m.s3.eu-central-1.amazonaws.com";
const MAX_TILES: usize = 4;
const COG_HEADER_BYTES: u64 = 128 * 1024;
const MAX_COG_BLOCK_BYTES: u64 = 12 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_CACHE_AGE: Duration = Duration::from_secs(90 * 24 * 3600);
const CACHE_TOUCH_INTERVAL: Duration = Duration::from_secs(60 * 60);
const STALE_PART_AGE: Duration = Duration::from_secs(24 * 3600);
// A cache-only attempt starts quickly; HTTP is never permitted until the
// original one-second inactivity deadline, counted from the last view change.
const CACHE_DEBOUNCE: Duration = Duration::from_millis(300);
const NETWORK_DEBOUNCE: Duration = Duration::from_secs(1);
static PART_ID: AtomicU64 = AtomicU64::new(0);
static INITIAL_CACHE_PRUNE: AtomicBool = AtomicBool::new(false);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoadMode {
    CacheOnly,
    Network,
}

#[derive(Default)]
pub(crate) struct State {
    pub(crate) enabled: bool,
    observed: Option<View>,
    deadline: Option<Instant>,
    network_deadline: Option<Instant>,
    cache_attempted: bool,
    generation: u64,
    loading: Option<(u64, LoadMode)>,
    displayed: Option<Overlay>,
}

impl State {
    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.observed = None;
        self.deadline = None;
        self.network_deadline = None;
        self.cache_attempted = false;
        self.loading = None;
        self.displayed = None;
        self.generation = self.generation.wrapping_add(1);
    }

    /// Called after any app update, including map movement and resize.
    pub(crate) fn sync(&mut self, current: View) {
        if !self.enabled { return; }
        if self.observed != Some(current) {
            self.observed = Some(current);
            let now = Instant::now();
            self.deadline = Some(now + CACHE_DEBOUNCE);
            self.network_deadline = Some(now + NETWORK_DEBOUNCE);
            self.cache_attempted = false;
            self.loading = None;
            self.displayed = None;
            self.generation = self.generation.wrapping_add(1);
        }
    }

    // Resizing the right sidebar changes the canvas size only on the *next*
    // frame. Start the timer now and let its next tick read the updated bounds.
    pub(crate) fn force_refresh(&mut self) {
        if self.enabled {
            self.observed = None;
            let now = Instant::now();
            self.deadline = Some(now + CACHE_DEBOUNCE);
            self.network_deadline = Some(now + NETWORK_DEBOUNCE);
            self.cache_attempted = false;
            self.loading = None;
            self.displayed = None;
            self.generation = self.generation.wrapping_add(1);
        }
    }

    pub(crate) fn wants_tick(&self) -> bool {
        self.enabled && self.deadline.is_some()
    }

    pub(crate) fn begin_if_ready(&mut self, now: Instant) -> Option<(u64, View, LoadMode)> {
        if !self.enabled || self.loading.is_some() || self.deadline.is_none_or(|at| now < at) {
            return None;
        }
        let view = self.observed?;
        let mode = if self.cache_attempted { LoadMode::Network } else { LoadMode::CacheOnly };
        self.cache_attempted = true;
        self.deadline = None;
        self.loading = Some((self.generation, mode));
        Some((self.generation, view, mode))
    }

    pub(crate) fn complete(&mut self, generation: u64, result: Result<Option<Arc<Rendered>>, String>) -> Option<String> {
        if !self.enabled || generation != self.generation {
            return None;
        }
        let Some((active, mode)) = self.loading else { return None; };
        if active != generation { return None; }
        self.loading = None;
        match result {
            Ok(Some(result)) if self.observed == Some(result.view) => {
                self.deadline = None;
                self.network_deadline = None;
                self.displayed = Some(Overlay {
                    view: result.view,
                    image: Handle::from_rgba(result.width, result.height, result.rgba.clone()),
                    lower: result.lower,
                    upper: result.upper,
                });
                None
            }
            // A cache miss (or broken cached data) is not a user-visible error.
            // Only now arm the online attempt, at least one second after the
            // LAST movement. No network work can happen in the fast path.
            Ok(None) | Err(_) if mode == LoadMode::CacheOnly => {
                self.deadline = self.network_deadline.take();
                None
            }
            Ok(Some(_)) => None,
            Ok(None) => Some("Copernicus DEM: online loader returned no data".into()),
            Err(error) => Some(error),
        }
    }

    pub(crate) fn is_loading(&self) -> bool {
        self.enabled && self.loading.is_some_and(|(generation, _)| generation == self.generation)
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

// The DEM cache uses two independent timestamps:
// - modified: original download time, with a strict 90-day lifetime;
// - accessed: approximate last use, for least-recently-used eviction at 2 GiB.
// Updating access time MUST NOT refresh modification time. This preserves the
// original expiration rule even for frequently visited terrain.
fn prune_disk_cache_with_limits(dir: &std::path::Path, max_bytes: u64, max_age: Duration) {
    let mut entries = Vec::new();
    let Ok(items) = std::fs::read_dir(dir) else { return; };
    for item in items.flatten() {
        let path = item.path();
        let extension = path.extension().and_then(|s| s.to_str());
        let Ok(meta) = item.metadata() else { continue; };
        if !meta.is_file() { continue; }
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);

        if extension == Some("part") {
            // An interrupted download must not occupy disk forever. Never
            // remove active temporary files, which are normally much newer.
            let ours = path.file_name().and_then(|s| s.to_str())
                .is_some_and(|name| name.starts_with("dem-")
                    || name.starts_with("Copernicus_DSM_COG_10_"));
            if ours && modified.elapsed().is_ok_and(|age| age > STALE_PART_AGE) {
                let _ = std::fs::remove_file(path);
            }
            continue;
        }
        if !matches!(extension, Some("tif" | "hdr" | "blk")) { continue; }
        if modified.elapsed().is_ok_and(|age| age > max_age) {
            let _ = std::fs::remove_file(path);
            continue;
        }
        // Filesystems may not report access time. Use modification time in
        // that case, matching the old behaviour without losing data.
        let last_used = meta.accessed().unwrap_or(modified).max(modified);
        entries.push((last_used, modified, meta.len(), path));
    }
    entries.sort_by_key(|(last_used, modified, _, _)| (*last_used, *modified));
    let mut total = entries.iter().fold(0u64, |sum, (_, _, size, _)| sum.saturating_add(*size));
    for (_, _, size, path) in entries {
        if total <= max_bytes { break; }
        if std::fs::remove_file(path).is_ok() { total = total.saturating_sub(size); }
    }
}

fn prune_disk_cache(dir: &std::path::Path) {
    prune_disk_cache_with_limits(dir, MAX_CACHE_BYTES, MAX_CACHE_AGE);
}

// Run maintenance also for a session using only existing cached data. It is
// asynchronous and once per process, so the 300 ms cache render is not held
// up by a full-directory scan. New writes continue to prune after commit.
fn start_initial_cache_prune() {
    if !INITIAL_CACHE_PRUNE.swap(true, Ordering::Relaxed) {
        let dir = cache_dir();
        let _ = tokio::spawn(async move {
            let _ = tokio::task::spawn_blocking(move || prune_disk_cache(&dir)).await;
        });
    }
}

// FileTimes only changes atime, never mtime. Limit filesystem metadata writes
// to about one per hour per cached file during active map navigation.
fn record_cache_hit(path: &std::path::Path, accessed: Option<SystemTime>) {
    let now = SystemTime::now();
    if accessed.and_then(|at| now.duration_since(at).ok())
        .is_some_and(|age| age < CACHE_TOUCH_INTERVAL) {
        return;
    }
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = file.set_times(std::fs::FileTimes::new().set_accessed(now));
    }
}

// Files older than 90 days must be refreshed even when requested directly,
// not only after the next cache pruning pass.
async fn read_fresh_cache(path: &std::path::Path, max_bytes: u64) -> Option<Vec<u8>> {
    let meta = tokio::fs::metadata(path).await.ok()?;
    if !meta.is_file() || meta.len() < 8 || meta.len() > max_bytes
        || meta.modified().ok()?.elapsed().ok()? > MAX_CACHE_AGE {
        return None;
    }
    let accessed = meta.accessed().ok();
    let bytes = tokio::fs::read(path).await.ok()?;
    if bytes.len() < 8 || bytes.len() as u64 > max_bytes { return None; }
    // Do not let an atime update failure affect offline availability.
    if accessed.and_then(|at| SystemTime::now().duration_since(at).ok())
        .is_none_or(|age| age >= CACHE_TOUCH_INTERVAL) {
        let path = path.to_path_buf();
        let _ = tokio::task::spawn_blocking(move || record_cache_hit(&path, accessed)).await;
    }
    Some(bytes)
}

async fn get_tile(client: &reqwest::Client, lat: i32, lon: i32) -> Result<Vec<u8>, String> {
    let name = tile_name(lat, lon);
    let path = cache_dir().join(format!("{name}.tif"));
    if let Some(bytes) = read_fresh_cache(&path, MAX_FILE_BYTES).await {
        return Ok(bytes);
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
                } else {
                    let _ = tokio::fs::remove_file(&part).await;
                }
            }
        }
    }
    Ok(data)
}

// Persist a complete cache entry only after a successful write. Interrupted
// requests never become valid cache entries, including for range responses.
async fn store_cache(path: &std::path::Path, data: &[u8]) {
    let Some(parent) = path.parent() else { return; };
    if tokio::fs::create_dir_all(parent).await.is_err() { return; }
    let nonce = PART_ID.fetch_add(1, Ordering::Relaxed);
    let part = parent.join(format!("dem-{}-{nonce}.part", std::process::id()));
    if tokio::fs::write(&part, data).await.is_ok() {
        if tokio::fs::rename(&part, path).await.is_ok() {
            let dir = parent.to_path_buf();
            let _ = tokio::task::spawn_blocking(move || prune_disk_cache(&dir)).await;
            return;
        }
    }
    let _ = tokio::fs::remove_file(&part).await;
}

// S3 must return 206 with the exact requested offset. A 200 response means
// the server/proxy ignored Range; return None and use the bounded pilot path.
async fn get_range(client: &reqwest::Client, name: &str, start: u64, len: u64)
    -> Result<Option<Vec<u8>>, String>
{
    if len == 0 || len > MAX_COG_BLOCK_BYTES.max(COG_HEADER_BYTES)
        || start.checked_add(len).is_none_or(|end| end > MAX_FILE_BYTES) {
        return Err(format!("Copernicus {name}: COG byte range exceeds safety limits"));
    }
    let end = start + len - 1;
    let url = format!("{BASE_URL}/{name}/{name}.tif");
    let response = client.get(url)
        .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
        .timeout(Duration::from_secs(90)).send().await
        .map_err(|e| format!("Copernicus {name}: {e}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(format!("Copernicus {name}: no public GLO-30 data"));
    }
    if response.status() == reqwest::StatusCode::OK { return Ok(None); }
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(format!("Copernicus {name}: unexpected Range HTTP {}", response.status()));
    }
    let content_range = response.headers().get(reqwest::header::CONTENT_RANGE)
        .and_then(|h| h.to_str().ok())
        .and_then(parse_content_range)
        .ok_or_else(|| format!("Copernicus {name}: invalid Content-Range"))?;
    if content_range.0 != start || content_range.1 > end
        || content_range.2 > MAX_FILE_BYTES
        || (start != 0 && content_range.1 != end) {
        return Err(format!("Copernicus {name}: incorrect COG range response"));
    }
    let expected = content_range.1 - content_range.0 + 1;
    let mut response = response;
    let mut data = Vec::with_capacity(expected as usize);
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if data.len().saturating_add(chunk.len()) as u64 > expected {
            return Err(format!("Copernicus {name}: oversized COG range response"));
        }
        data.extend_from_slice(&chunk);
    }
    if data.len() as u64 != expected {
        return Err(format!("Copernicus {name}: truncated COG range response"));
    }
    Ok(Some(data))
}

fn parse_content_range(value: &str) -> Option<(u64, u64, u64)> {
    let (unit, rest) = value.split_once(' ')?;
    if unit != "bytes" { return None; }
    let (span, total) = rest.split_once('/')?;
    let (start, end) = span.split_once('-')?;
    let (start, end, total) = (start.parse().ok()?, end.parse().ok()?, total.parse().ok()?);
    if start > end || end >= total { return None; }
    Some((start, end, total))
}

#[derive(Debug)]
struct CogIndex {
    width: u32,
    height: u32,
    block_width: u32,
    block_height: u32,
    compression: u16,
    predictor: u16,
    offsets: Vec<u32>,
    sizes: Vec<u32>,
}

// Only the common little-endian classic TIFF, one Float32 band, DEFLATE and
// tiled layout is handled here. All other layouts retain the known-good,
// bounded full-file decoder. No guessed offsets or height samples.
fn parse_cog_index(header: &[u8]) -> Option<CogIndex> {
    fn u16le(b: &[u8], at: usize) -> Option<u16> {
        Some(u16::from_le_bytes(b.get(at..at.checked_add(2)?)?.try_into().ok()?))
    }
    fn u32le(b: &[u8], at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
    }
    if header.get(..4)? != b"II\x2a\x00" { return None; }
    let ifd = u32le(header, 4)? as usize;
    let count = u16le(header, ifd)? as usize;
    if count > 256 { return None; }
    let mut fields = HashMap::<u16, Vec<u32>>::new();
    for i in 0..count {
        let pos = ifd.checked_add(2)?.checked_add(i.checked_mul(12)?)?;
        let tag = u16le(header, pos)?;
        let typ = u16le(header, pos + 2)?;
        let n = u32le(header, pos + 4)? as usize;
        if !matches!(tag, 256 | 257 | 258 | 259 | 262 | 277 | 284 | 317 | 322 | 323 | 324 | 325 | 339) {
            continue;
        }
        if n == 0 || n > 4096 { return None; }
        let unit = match typ { 3 => 2usize, 4 => 4, _ => return None };
        let bytes = n.checked_mul(unit)?;
        let at = if bytes <= 4 { pos + 8 } else { u32le(header, pos + 8)? as usize };
        if at.checked_add(bytes)? > header.len() { return None; }
        let mut values = Vec::with_capacity(n);
        for j in 0..n {
            let at = at + j * unit;
            values.push(if unit == 2 { u16le(header, at)? as u32 } else { u32le(header, at)? });
        }
        fields.insert(tag, values);
    }
    let scalar = |tag: u16| -> Option<u32> { Some(*fields.get(&tag)?.first()?) };
    let (width, height) = (scalar(256)?, scalar(257)?);
    let (block_width, block_height) = (scalar(322)?, scalar(323)?);
    if width == 0 || height == 0 || width as u64 * height as u64 > 30_000_000
        || block_width == 0 || block_height == 0
        || block_width as u64 * block_height as u64 > 4_194_304
        || scalar(258)? != 32 || scalar(339)? != 3
        || scalar(277).unwrap_or(1) != 1 || scalar(284).unwrap_or(1) != 1
        || scalar(262).unwrap_or(1) != 1 {
        return None;
    }
    let compression = scalar(259)? as u16;
    let predictor = scalar(317).unwrap_or(1) as u16;
    if !matches!(compression, 8 | 32946) || !matches!(predictor, 1 | 3) {
        return None;
    }
    let offsets = fields.remove(&324)?;
    let sizes = fields.remove(&325)?;
    let nx = width.div_ceil(block_width) as usize;
    let ny = height.div_ceil(block_height) as usize;
    if nx.checked_mul(ny)? != offsets.len() || offsets.len() != sizes.len() {
        return None;
    }
    for (&off, &len) in offsets.iter().zip(&sizes) {
        if len == 0 || len as u64 > MAX_COG_BLOCK_BYTES
            || off as u64 + len as u64 > MAX_FILE_BYTES {
            return None;
        }
    }
    Some(CogIndex { width, height, block_width, block_height, compression, predictor, offsets, sizes })
}

// Decode a single compressed COG tile with the EXISTING rusty_tiff decoder.
// Build a minimal one-tile TIFF wrapper, preserving the raw compressed bytes
// and predictor. No second DEFLATE or floating-point predictor implementation.
fn decode_cog_block(index: &CogIndex, compressed: &[u8]) -> Result<Vec<f32>, String> {
    let tags: [(u16, u16, u32); 13] = [
        (256, 4, index.block_width), (257, 4, index.block_height),
        (258, 3, 32), (259, 3, index.compression as u32),
        (262, 3, 1), (277, 3, 1), (284, 3, 1),
        (317, 3, index.predictor as u32), (322, 4, index.block_width),
        (323, 4, index.block_height), (324, 4, 0),
        (325, 4, compressed.len() as u32), (339, 3, 3),
    ];
    let data_offset = 8 + 2 + (tags.len() * 12) + 4;
    let mut tiff = Vec::with_capacity(data_offset + compressed.len());
    tiff.extend_from_slice(b"II\x2a\x00");
    tiff.extend_from_slice(&8u32.to_le_bytes());
    tiff.extend_from_slice(&(tags.len() as u16).to_le_bytes());
    for (tag, typ, value) in tags {
        tiff.extend_from_slice(&tag.to_le_bytes());
        tiff.extend_from_slice(&typ.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        let value = if tag == 324 { data_offset as u32 } else { value };
        tiff.extend_from_slice(&value.to_le_bytes());
    }
    tiff.extend_from_slice(&0u32.to_le_bytes());
    tiff.extend_from_slice(compressed);
    let mut decoder = Decoder::new(Cursor::new(tiff)).map_err(|e| format!("DEM COG block: {e}"))?;
    let pixels = match decoder.read_image().map_err(|e| format!("DEM COG block: {e}"))? {
        DecodingResult::F32(v) => v,
        other => return Err(format!("DEM COG block: unsupported samples: {other:?}")),
    };
    if pixels.len() != index.block_width as usize * index.block_height as usize {
        return Err("DEM COG block: incorrect pixel count".into());
    }
    Ok(pixels)
}

// Keep TIFF decoding off the UI executor and return the compressed bytes so
// they can be cached only after the decoder has accepted them.
async fn decode_cog_block_worker(index: &CogIndex, compressed: Vec<u8>)
    -> Result<(Result<Vec<f32>, String>, Vec<u8>), String>
{
    let block_index = CogIndex {
        width: index.width, height: index.height,
        block_width: index.block_width, block_height: index.block_height,
        compression: index.compression, predictor: index.predictor,
        offsets: Vec::new(), sizes: Vec::new(),
    };
    tokio::task::spawn_blocking(move || {
        let result = decode_cog_block(&block_index, &compressed);
        (result, compressed)
    }).await.map_err(|e| format!("DEM block worker: {e}"))
}

// A corrupted cached block gets discarded and fetched once. The supplied
// future is lazy: when the cached bytes decode correctly, it is never polled
// and no HTTP request takes place. Never cache undecodable network bytes.
async fn load_cog_block<F>(path: &std::path::Path, index: &CogIndex, id: usize, fetch: F)
    -> Result<Option<Vec<f32>>, String>
where
    F: std::future::Future<Output = Result<Option<Vec<u8>>, String>>,
{
    let expected = index.sizes[id] as usize;
    let cached = read_fresh_cache(path, MAX_COG_BLOCK_BYTES).await;
    if cached.is_none() {
        // Remove truncated, expired or unreadable entries too. This allows
        // atomic replacement via rename on Windows as well as on Linux.
        let _ = tokio::fs::remove_file(path).await;
    }
    if let Some(compressed) = cached {
        if compressed.len() == expected {
            let (decoded, _) = decode_cog_block_worker(index, compressed).await?;
            if let Ok(values) = decoded {
                return Ok(Some(values));
            }
        }
        // The file can have the correct length yet fail the TIFF/DEFLATE
        // decoder. Remove it before replacing it (also works on Windows).
        let _ = tokio::fs::remove_file(path).await;
    }

    // Exactly one remote attempt. If offline or if new data also fail to
    // decode, leave the map usable and do not poison the cache again.
    let Some(compressed) = fetch.await? else { return Ok(None); };
    if compressed.len() != expected {
        return Err(format!("Copernicus DEM block {id}: incorrect compressed byte count"));
    }
    let (decoded, compressed) = decode_cog_block_worker(index, compressed).await?;
    let values = decoded.map_err(|e| format!("Copernicus DEM block {id}: {e}"))?;
    store_cache(path, &compressed).await;
    Ok(Some(values))
}

// The 1-degree COG grid is sampled exactly as in the previous implementation.
// Fetch all blocks intersecting the viewport, never a screen-coloured PNG.
fn viewport_blocks(view: View, index: &CogIndex, south: i32, west: i32) -> Vec<usize> {
    let (left, top) = view.lonlat_at(0.0, 0.0);
    let (right, bottom) = view.lonlat_at(view.width as f64, view.height as f64);
    let x0 = ((left - west as f64) * index.width as f64).floor().max(0.0) as u32;
    let x1 = ((right - west as f64) * index.width as f64).ceil().max(0.0) as u32;
    let y0 = ((south as f64 + 1.0 - top) * index.height as f64).floor().max(0.0) as u32;
    let y1 = ((south as f64 + 1.0 - bottom) * index.height as f64).ceil().max(0.0) as u32;
    let (x0, x1) = (x0.min(index.width), x1.min(index.width));
    let (y0, y1) = (y0.min(index.height), y1.min(index.height));
    if x0 >= x1 || y0 >= y1 { return Vec::new(); }
    let stride = index.width.div_ceil(index.block_width) as usize;
    let mut ids = Vec::new();
    for row in (y0 / index.block_height)..=((y1 - 1) / index.block_height) {
        for col in (x0 / index.block_width)..=((x1 - 1) / index.block_width) {
            ids.push(row as usize * stride + col as usize);
        }
    }
    ids
}

async fn decode_full_fallback(client: &reqwest::Client, lat: i32, lon: i32)
    -> Result<Tile, String>
{
    let bytes = get_tile(client, lat, lon).await?;
    tokio::task::spawn_blocking(move || decode_tile(bytes))
        .await.map_err(|e| format!("DEM decode worker: {e}"))?
}

async fn get_window_tile(client: Option<&reqwest::Client>, lat: i32, lon: i32, view: View)
    -> Result<Option<Tile>, String>
{
    let name = tile_name(lat, lon);
    let dir = cache_dir();
    let full_path = dir.join(format!("{name}.tif"));
    // Pre-existing pilot cache remains valid, offline, and requires no migration.
    if let Some(bytes) = read_fresh_cache(&full_path, MAX_FILE_BYTES).await {
        return tokio::task::spawn_blocking(move || decode_tile(bytes))
            .await.map_err(|e| format!("DEM decode worker: {e}"))?.map(Some);
    }
    let header_path = dir.join(format!("{name}.hdr"));
    let header = match read_fresh_cache(&header_path, COG_HEADER_BYTES).await {
        Some(bytes) => bytes,
        None => {
            let Some(client) = client else { return Ok(None); };
            match get_range(client, &name, 0, COG_HEADER_BYTES).await? {
                Some(bytes) => bytes,
                None => return decode_full_fallback(client, lat, lon).await.map(Some),
            }
        },
    };
    let Some(index) = parse_cog_index(&header) else {
        // BigTIFF, non-tiled or another compression: unchanged bounded pilot.
        return match client {
            Some(client) => decode_full_fallback(client, lat, lon).await.map(Some),
            None => Ok(None),
        };
    };
    // Store only a verified COG header, not malformed network metadata.
    if read_fresh_cache(&header_path, COG_HEADER_BYTES).await.is_none() {
        store_cache(&header_path, &header).await;
    }
    let block_ids = viewport_blocks(view, &index, lat, lon);
    // Avoid decoding several multi-megabyte blocks only to discover that a
    // later block is absent. Check coverage before cache-only rendering.
    if client.is_none() {
        for &id in &block_ids {
            let path = dir.join(format!("{name}_{id}.blk"));
            let Ok(meta) = tokio::fs::metadata(&path).await else { return Ok(None); };
            if !meta.is_file() || meta.len() != index.sizes[id] as u64
                || meta.modified().ok().and_then(|m| m.elapsed().ok())
                    .is_none_or(|age| age > MAX_CACHE_AGE)
            {
                return Ok(None);
            }
        }
    }
    let mut heights = vec![f32::NAN; index.width as usize * index.height as usize];
    let stride = index.width.div_ceil(index.block_width) as usize;
    for id in block_ids {
        let path = dir.join(format!("{name}_{id}.blk"));
        let Some(block) = load_cog_block(
            &path, &index, id,
            async {
                match client {
                    Some(client) => get_range(client, &name, index.offsets[id] as u64, index.sizes[id] as u64).await,
                    None => Ok(None),
                }
            },
        ).await? else {
            return match client {
                Some(client) => decode_full_fallback(client, lat, lon).await.map(Some),
                None => Ok(None),
            };
        };
        let width = index.block_width as usize;
        let height = index.block_height as usize;
        let row = id / stride;
        let col = id % stride;
        let x0 = col * width;
        let y0 = row * height;
        for y in 0..height.min(index.height as usize - y0) {
            let dst = (y0 + y) * index.width as usize + x0;
            let len = width.min(index.width as usize - x0);
            heights[dst..dst + len].copy_from_slice(&block[y * width..y * width + len]);
        }
    }
    Ok(Some(Tile { width: index.width, height: index.height, heights }))
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
        // sampling to preserve the existing display. Pixel-centre offsets and
        // bilinear resampling are separate future changes.
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

async fn load_and_render_mode(view: View, client: Option<&reqwest::Client>)
    -> Result<Option<Arc<Rendered>>, String>
{
    let needed = required_tiles(view)?;
    start_initial_cache_prune();
    let mut tiles = HashMap::with_capacity(needed.len());
    for (lat, lon) in needed {
        let Some(tile) = get_window_tile(client, lat, lon, view).await? else {
            return Ok(None);
        };
        tiles.insert((lat, lon), tile);
    }
    let image = tokio::task::spawn_blocking(move || render_tiles(view, tiles).map(Arc::new))
        .await.map_err(|e| format!("DEM render worker: {e}"))??;
    Ok(Some(image))
}

// Cache-only means cache-only: this function cannot issue any HTTP request.
// A miss is reported to State, which waits for the original 1 s deadline.
pub(crate) async fn load_and_render_cached(view: View) -> Result<Option<Arc<Rendered>>, String> {
    load_and_render_mode(view, None).await
}

pub(crate) async fn load_and_render(view: View, client: reqwest::Client) -> Result<Arc<Rendered>, String> {
    load_and_render_mode(view, Some(&client)).await?
        .ok_or_else(|| "Copernicus DEM: unexpected online cache miss".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cache_test_file(
        dir: &std::path::Path, name: &str, size: usize,
        modified: SystemTime, accessed: SystemTime,
    ) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, vec![42u8; size]).unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_times(std::fs::FileTimes::new()
            .set_modified(modified).set_accessed(accessed)).unwrap();
        path
    }

    #[test]
    fn dem_cache_evicts_least_recently_used_across_all_formats() {
        let dir = std::env::temp_dir().join(format!("foxy-dem-lru-{}-{}",
            std::process::id(), PART_ID.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let now = SystemTime::now();
        let downloaded = now - Duration::from_secs(24 * 3600);
        let old = make_cache_test_file(&dir, "older.blk", 16, downloaded,
            now - Duration::from_secs(3 * 3600));
        let middle = make_cache_test_file(&dir, "middle.tif", 16, downloaded,
            now - Duration::from_secs(2 * 3600));
        let recent = make_cache_test_file(&dir, "recent.hdr", 16, downloaded,
            now - Duration::from_secs(3600));
        let unrelated = make_cache_test_file(&dir, "other.png", 16, downloaded, downloaded);

        // Only two DEM entries fit; LRU uses atime, not the identical mtime.
        prune_disk_cache_with_limits(&dir, 32, MAX_CACHE_AGE);
        assert!(!old.exists());
        assert!(middle.exists());
        assert!(recent.exists());
        assert!(unrelated.exists(), "do not touch other cache formats");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dem_cache_expires_downloads_and_cleans_only_abandoned_parts() {
        let dir = std::env::temp_dir().join(format!("foxy-dem-expiry-{}-{}",
            std::process::id(), PART_ID.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let now = SystemTime::now();
        let expired = make_cache_test_file(&dir, "expired.blk", 16,
            now - MAX_CACHE_AGE - Duration::from_secs(3600), now);
        let valid = make_cache_test_file(&dir, "valid.blk", 16,
            now - MAX_CACHE_AGE + Duration::from_secs(3600), now);
        let abandoned = make_cache_test_file(&dir, "dem-dead.part", 16,
            now - STALE_PART_AGE - Duration::from_secs(3600), now);
        let active = make_cache_test_file(&dir, "dem-active.part", 16, now, now);
        let unknown = make_cache_test_file(&dir, "third-party.part", 16,
            now - STALE_PART_AGE - Duration::from_secs(3600), now);

        prune_disk_cache_with_limits(&dir, 100, MAX_CACHE_AGE);
        assert!(!expired.exists(), "90-day download age wins over recent use");
        assert!(valid.exists());
        assert!(!abandoned.exists());
        assert!(active.exists(), "do not remove in-flight temporary files");
        assert!(unknown.exists(), "ignore unrelated partial files");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn cached_read_updates_access_not_download_time() {
        let dir = std::env::temp_dir().join(format!("foxy-dem-touch-{}-{}",
            std::process::id(), PART_ID.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let now = SystemTime::now();
        let old = now - Duration::from_secs(2 * 24 * 3600);
        let path = make_cache_test_file(&dir, "tile_0.blk", 16, old, old);
        assert_eq!(read_fresh_cache(&path, 1024).await.unwrap(), vec![42u8; 16]);
        let meta = std::fs::metadata(&path).unwrap();
        assert!(meta.accessed().unwrap() >= now - Duration::from_secs(60));
        assert!(meta.modified().unwrap() <= now - Duration::from_secs(24 * 3600),
            "cache reads must not reset the 90-day expiry");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn copernicus_names_follow_sw_corner() {
        assert_eq!(tile_name(47, 11), "Copernicus_DSM_COG_10_N47_00_E011_00_DEM");
        assert_eq!(tile_name(-3, -7), "Copernicus_DSM_COG_10_S03_00_W007_00_DEM");
    }


    #[test]
    fn http_content_ranges_are_validated() {
        assert_eq!(parse_content_range("bytes 10-19/100"), Some((10, 19, 100)));
        assert_eq!(parse_content_range("bytes 10-9/100"), None);
        assert_eq!(parse_content_range("bytes 90-100/100"), None);
        assert_eq!(parse_content_range("bytes */100"), None);
    }

    #[test]
    fn one_compressed_cog_block_decodes_float_elevations() {
        let index = CogIndex {
            width: 2, height: 2, block_width: 2, block_height: 2,
            compression: 8, predictor: 1,
            offsets: vec![0], sizes: vec![23],
        };
        // zlib-compressed little-endian [100.0, 200.0, 300.0, 400.0] f32.
        let compressed = [120, 156, 99, 96, 56, 225, 196, 192, 224, 225, 204, 192,
            48, 13, 136, 79, 56, 3, 0, 26, 43, 3, 122];
        assert_eq!(decode_cog_block(&index, &compressed).unwrap(),
            vec![100.0, 200.0, 300.0, 400.0]);
    }


    #[tokio::test]
    async fn corrupt_cached_cog_block_is_replaced_and_reused_offline() {
        let index = CogIndex {
            width: 2, height: 2, block_width: 2, block_height: 2,
            compression: 8, predictor: 1,
            offsets: vec![0], sizes: vec![23],
        };
        let compressed = vec![120, 156, 99, 96, 56, 225, 196, 192, 224, 225,
            204, 192, 48, 13, 136, 79, 56, 3, 0, 26, 43, 3, 122];
        let dir = std::env::temp_dir().join(format!("foxy-dem-repair-{}-{}",
            std::process::id(), PART_ID.fetch_add(1, Ordering::Relaxed)));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("tile_0.blk");
        tokio::fs::write(&path, vec![0u8; 23]).await.unwrap();

        let repaired = load_cog_block(&path, &index, 0, async {
            Ok(Some(compressed.clone()))
        }).await.unwrap().unwrap();
        assert_eq!(repaired, vec![100.0, 200.0, 300.0, 400.0]);
        assert_eq!(tokio::fs::read(&path).await.unwrap(), compressed);

        // The request future must not be polled for a valid cached block.
        let offline = load_cog_block(&path, &index, 0, async {
            Err("unexpected network request".to_string())
        }).await.unwrap().unwrap();
        assert_eq!(offline, repaired);
        tokio::fs::remove_dir_all(&dir).await.unwrap();
    }

    #[tokio::test]
    async fn broken_cog_block_does_not_poison_cache_when_offline_or_downloaded() {
        let index = CogIndex {
            width: 2, height: 2, block_width: 2, block_height: 2,
            compression: 8, predictor: 1,
            offsets: vec![0], sizes: vec![23],
        };
        let dir = std::env::temp_dir().join(format!("foxy-dem-offline-{}-{}",
            std::process::id(), PART_ID.fetch_add(1, Ordering::Relaxed)));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("tile_0.blk");
        tokio::fs::write(&path, vec![0u8; 23]).await.unwrap();

        let failed = load_cog_block(&path, &index, 0, async {
            Err("offline".to_string())
        }).await;
        assert_eq!(failed.unwrap_err(), "offline");
        assert!(!path.exists(), "bad cached bytes must be removed");

        let failed = load_cog_block(&path, &index, 0, async {
            Ok(Some(vec![0u8; 23]))
        }).await;
        assert!(failed.is_err(), "corrupted server data must be rejected");
        assert!(!path.exists(), "bad server bytes must not be cached");
        tokio::fs::remove_dir_all(&dir).await.unwrap();
    }

    #[test]
    fn classic_cog_index_reads_tile_offsets_and_dimensions() {
        // The same minimal TIFF IFD structure as the per-block wrapper, but
        // with two tiles and 2-element offset and byte-count arrays.
        let tags: [(u16, u16, u32, u32); 13] = [
            (256, 4, 1, 4), (257, 4, 1, 2), (258, 3, 1, 32),
            (259, 3, 1, 8), (262, 3, 1, 1), (277, 3, 1, 1),
            (284, 3, 1, 1), (317, 3, 1, 3), (322, 4, 1, 2),
            (323, 4, 1, 2), (324, 4, 2, 0), (325, 4, 2, 0),
            (339, 3, 1, 3),
        ];
        let array_start = 8 + 2 + 12 * tags.len() + 4;
        let mut tiff = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
        tiff.extend_from_slice(&(tags.len() as u16).to_le_bytes());
        for (tag, typ, count, mut value) in tags {
            if tag == 324 { value = array_start as u32; }
            if tag == 325 { value = (array_start + 8) as u32; }
            tiff.extend_from_slice(&tag.to_le_bytes());
            tiff.extend_from_slice(&typ.to_le_bytes());
            tiff.extend_from_slice(&count.to_le_bytes());
            tiff.extend_from_slice(&value.to_le_bytes());
        }
        tiff.extend_from_slice(&0u32.to_le_bytes());
        tiff.extend_from_slice(&[192u32.to_le_bytes(), 215u32.to_le_bytes()].concat());
        tiff.extend_from_slice(&[23u32.to_le_bytes(), 23u32.to_le_bytes()].concat());
        let meta = parse_cog_index(&tiff).unwrap();
        assert_eq!((meta.width, meta.height), (4, 2));
        assert_eq!((meta.block_width, meta.block_height), (2, 2));
        assert_eq!(meta.offsets, vec![192, 215]);
        assert_eq!(meta.sizes, vec![23, 23]);
    }

    #[test]
    fn viewport_selects_only_intersecting_cog_blocks() {
        let index = CogIndex {
            width: 3600, height: 3600, block_width: 512, block_height: 512,
            compression: 8, predictor: 3,
            offsets: vec![], sizes: vec![],
        };
        let (cx, cy) = crate::util::geo::merc_of(11.50, 47.50);
        // Approximately 1 km wide: one block near the middle of N47/E011.
        let view = View::new(cx, cy, 2.0, 500.0, 400.0);
        let blocks = viewport_blocks(view, &index, 47, 11);
        assert!(!blocks.is_empty());
        assert!(blocks.len() <= 4, "small viewport requested too many COG blocks");
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
        let (old, _, mode) = state.begin_if_ready(Instant::now() + Duration::from_secs(2)).unwrap();
        assert_eq!(mode, LoadMode::CacheOnly);
        state.sync(b);
        assert!(state.complete(old, Err("obsolete".into())).is_none());
        assert!(state.visible(a).is_none());
        assert!(state.wants_tick());
    }

    #[test]
    fn adaptive_debounce_waits_for_network_after_cache_miss() {
        let mut state = State::default();
        state.set_enabled(true);
        state.sync(View::new(1000.0, 2000.0, 10.0, 600.0, 400.0));
        let fast_at = state.deadline.unwrap();
        let online_at = state.network_deadline.unwrap();
        assert_eq!(online_at.duration_since(fast_at), NETWORK_DEBOUNCE - CACHE_DEBOUNCE);
        assert!(state.begin_if_ready(fast_at - Duration::from_millis(1)).is_none());
        let (generation, _, mode) = state.begin_if_ready(fast_at).unwrap();
        assert_eq!(mode, LoadMode::CacheOnly);
        assert!(!state.wants_tick());
        // A slow local render must not race a second, online render.
        assert!(state.begin_if_ready(online_at + Duration::from_secs(1)).is_none());
        assert!(state.complete(generation, Ok(None)).is_none());
        assert!(state.begin_if_ready(online_at - Duration::from_millis(1)).is_none());
        let (_, _, mode) = state.begin_if_ready(online_at).unwrap();
        assert_eq!(mode, LoadMode::Network);
        assert!(!state.wants_tick());
    }

    #[test]
    fn cached_render_skips_network_deadline_entirely() {
        let mut state = State::default();
        state.set_enabled(true);
        let view = View::new(1000.0, 2000.0, 10.0, 600.0, 400.0);
        state.sync(view);
        let (generation, _, mode) = state.begin_if_ready(state.deadline.unwrap()).unwrap();
        assert_eq!(mode, LoadMode::CacheOnly);
        let image = Arc::new(Rendered {
            view, width: 1, height: 1, rgba: vec![1, 2, 3, 170],
            lower: 100.0, upper: 200.0,
        });
        assert!(state.complete(generation, Ok(Some(image))).is_none());
        assert!(state.visible(view).is_some());
        assert!(!state.wants_tick());
        assert!(state.network_deadline.is_none());
    }

    #[test]
    fn cache_failure_and_stale_fast_results_never_start_early_network() {
        let mut state = State::default();
        state.set_enabled(true);
        let a = View::new(1000.0, 2000.0, 10.0, 600.0, 400.0);
        let b = View::new(1100.0, 2000.0, 10.0, 600.0, 400.0);
        state.sync(a);
        let first_fast = state.deadline.unwrap();
        let (old_gen, _, _) = state.begin_if_ready(first_fast).unwrap();
        state.sync(b);
        let new_fast = state.deadline.unwrap();
        let new_online = state.network_deadline.unwrap();
        assert!(state.complete(old_gen, Ok(None)).is_none());
        assert!(state.begin_if_ready(new_fast - Duration::from_millis(1)).is_none());
        let (new_gen, _, mode) = state.begin_if_ready(new_fast).unwrap();
        assert_eq!(mode, LoadMode::CacheOnly);
        assert!(state.complete(new_gen, Err("corrupt cache".into())).is_none());
        // The cache failure transfers the original online deadline into
        // `deadline`; `network_deadline` is intentionally consumed by take().
        assert_eq!(state.deadline, Some(new_online));
        assert!(state.network_deadline.is_none());
        assert!(state.begin_if_ready(new_online - Duration::from_millis(1)).is_none());
        let (generation, view, mode) = state.begin_if_ready(new_online).unwrap();
        assert_eq!(generation, new_gen);
        assert_eq!(view, b);
        assert_eq!(mode, LoadMode::Network);
    }
}
