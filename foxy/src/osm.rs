use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::io::{self, Cursor, Write};
use std::fs::{self, OpenOptions};
use std::sync::atomic::{AtomicU64, Ordering};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use iced::widget::image::Handle;
use iced::task::Handle as TaskHandle;
use reqwest::Client;
use image::{ImageFormat, ImageReader};
use crate::BasemapSource;

pub type TileId = (u8, u32, u32);

pub const TILE_SIZE: f64 = 256.0;

// Keep both downloaded bodies and on-disk tiles bounded. A normal XYZ tile is
// 256x256; 2048 px allows higher-resolution variants without decode bombs.
const MAX_TILE_BYTES: usize = 8 * 1024 * 1024;
const MAX_TILE_DIMENSION: u32 = 2048;
static TEMP_TILE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) fn format_for_source(source: BasemapSource) -> ImageFormat {
    match source {
        BasemapSource::Osm => ImageFormat::Png,
        BasemapSource::Sentinel2 | BasemapSource::Esri => ImageFormat::Jpeg,
    }
}

/// Check the real file type AND decode the full image before trusting bytes
/// from disk or HTTP. Checking only PNG/JPEG magic does not detect truncation.
fn validate_tile(bytes: &[u8], format: ImageFormat) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() > MAX_TILE_BYTES {
        return Err("empty or oversized map tile".into());
    }
    if image::guess_format(bytes).ok() != Some(format) {
        return Err("map tile has the wrong image format".into());
    }
    // Some JPEG decoders recover from a missing end marker by filling in
    // pixels. Such a truncated download must still be rejected as cache data.
    if format == ImageFormat::Jpeg && !bytes.windows(2).any(|b| b == [0xFF, 0xD9]) {
        return Err("truncated JPEG map tile".into());
    }
    let dimensions = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions().map_err(|e| format!("invalid tile header: {e}"))?;
    if dimensions.0 == 0 || dimensions.1 == 0
        || dimensions.0 > MAX_TILE_DIMENSION || dimensions.1 > MAX_TILE_DIMENSION
    {
        return Err("map tile has invalid dimensions".into());
    }
    ImageReader::with_format(Cursor::new(bytes), format)
        .decode().map_err(|e| format!("invalid map tile: {e}"))?;
    Ok(())
}

/// Write to an adjacent, uniquely named .part file, then rename on the same
/// filesystem. On any I/O error the temporary file is removed. A cache write
/// failure does not prevent a valid tile from being shown on the map.
fn write_tile_atomically(disk: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = disk.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "tile has no parent directory")
    })?;
    fs::create_dir_all(parent)?;
    let filename = disk.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "tile has no file name")
    })?.to_string_lossy();
    let counter = TEMP_TILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = parent.join(format!(".{filename}.{}.{counter}.part", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, disk)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// The fetch callback makes cache recovery testable without a live tile server.
/// A bad cached tile is removed; a bad HTTP body is never saved or displayed.
pub(crate) async fn load_validated_tile<F, Fut>(
    disk: &Path,
    format: ImageFormat,
    fetch: F,
) -> Result<Vec<u8>, String>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Vec<u8>, String>>,
{
    // Check size before reading so a corrupt cache entry cannot allocate an
    // arbitrarily large buffer in memory.
    let within_limit = fs::metadata(disk)
        .map(|m| m.len() > 0 && m.len() <= MAX_TILE_BYTES as u64)
        .unwrap_or(false);
    if within_limit {
        if let Ok(bytes) = fs::read(disk) {
            if validate_tile(&bytes, format).is_ok() {
                return Ok(bytes);
            }
        }
    }
    // Best effort: if removal is forbidden, a valid network tile can still
    // be displayed; persistence will report its own error.
    if disk.exists() {
        let _ = fs::remove_file(disk);
    }

    let bytes = fetch().await?;
    validate_tile(&bytes, format)?;
    if let Err(e) = write_tile_atomically(disk, &bytes) {
        eprintln!("cannot cache tile {}: {e}", disk.display());
    }
    Ok(bytes)
}

/// Bound network memory even if the provider omits Content-Length.
pub(crate) async fn download_tile(client: &Client, url: &str) -> Result<Vec<u8>, String> {
    let mut response = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        // The caller treats 404/410 specially, as before.
        return Err(format!("HTTP {}", response.status().as_u16()));
    }
    if response.content_length().is_some_and(|len| len > MAX_TILE_BYTES as u64) {
        return Err("map tile response is too large".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if chunk.len() > MAX_TILE_BYTES - bytes.len() {
            return Err("map tile response is too large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}


// Bound the total number of in-flight requests, not just each batch.
pub(crate) const MAX_IN_FLIGHT: usize = 4;

// Retain a useful working set around the viewport, but do not accumulate
// every online tile ever visited in memory. Approximate decoded RGBA memory
// plus the compressed bytes kept by Iced image handles.
const MAX_MEMORY_TILES: usize = 256;
const MAX_MEMORY_ESTIMATED_BYTES: usize = 128 * 1024 * 1024;

pub(crate) fn estimated_tile_memory(bytes: &[u8], format: ImageFormat) -> usize {
    let decoded_bytes = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .ok()
        .and_then(|(width, height)| {
            (width as usize).checked_mul(height as usize)?.checked_mul(4)
        })
        .unwrap_or(MAX_TILE_BYTES);
    decoded_bytes.saturating_add(bytes.len())
}

#[derive(Clone, Copy)]
struct MemoryUse {
    last_access: u64,
    estimated_bytes: usize,
}


// Transient failures remain retryable; a disconnected client must not leave
// permanent holes on the map. Delays are deliberately bounded to avoid
// hammering tile providers when the network is down.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TileRetry {
    attempts: u32,
    retry_at: Instant,
}

impl TileRetry {
    pub(crate) fn after_failure(previous: Option<&Self>, now: Instant) -> Self {
        let attempts = previous.map_or(1, |s| s.attempts.saturating_add(1));
        let wait_secs = match attempts {
            1 => 2,
            2 => 5,
            3 => 10,
            4 => 20,
            _ => 30,
        };
        Self { attempts, retry_at: now + Duration::from_secs(wait_secs) }
    }

    pub(crate) fn ready(&self, now: Instant) -> bool {
        now >= self.retry_at
    }

    pub(crate) fn attempts(&self) -> u32 {
        self.attempts
    }
}

// Keep all scheduling decisions testable without an HTTP client or an Iced App.
// A cached None represents a genuinely missing tile (HTTP 404/410).
pub(crate) fn plan_tile_requests(
    visible: &[TileId],
    cached: &HashMap<TileId, Option<Handle>>,
    pending: &HashSet<TileId>,
    retries: &HashMap<TileId, TileRetry>,
    now: Instant,
) -> Vec<TileId> {
    let free_slots = MAX_IN_FLIGHT.saturating_sub(pending.len());
    visible.iter().copied()
        .filter(|id| !cached.contains_key(id) && !pending.contains(id))
        .filter(|id| retries.get(id).is_none_or(|retry| retry.ready(now)))
        .take(free_slots)
        .collect()
}

// Start from tiles near the middle of the visible region. This matters when
// bandwidth is limited: central tiles should not wait behind a whole row.
pub(crate) fn prioritize_visible_tiles(tiles: &mut [TileId]) {
    let Some(min_x) = tiles.iter().map(|t| t.1).min() else { return };
    let max_x = tiles.iter().map(|t| t.1).max().unwrap_or(min_x);
    let min_y = tiles.iter().map(|t| t.2).min().unwrap_or(0);
    let max_y = tiles.iter().map(|t| t.2).max().unwrap_or(min_y);
    let mid_x2 = i128::from(min_x) + i128::from(max_x);
    let mid_y2 = i128::from(min_y) + i128::from(max_y);
    let distance = |id: &TileId| {
        let dx = 2 * i128::from(id.1) - mid_x2;
        let dy = 2 * i128::from(id.2) - mid_y2;
        dx * dx + dy * dy
    };
    tiles.sort_unstable_by_key(|id| (distance(id), *id));
}

// Each attempt has its own token: completion of an aborted request must not
// unmark a newer request for the same coordinates, even from the same source.
struct InFlightTile {
    token: u64,
    handle: TaskHandle,
}

pub(crate) fn is_permanently_missing_tile_error(error: &str) -> bool {
    matches!(error, "HTTP 404" | "HTTP 410")
}

#[derive(Debug, Clone, Copy)]
pub struct OsmCacheConfig {
    pub max_age_days: u64,
    pub max_total_size: u64,
    pub cleanup_interval_secs: u64,
}

impl Default for OsmCacheConfig {
    fn default() -> Self {
        Self {
            max_age_days: 90,
            max_total_size: 2 * 1024 * 1024 * 1024, // 2 GiB
            cleanup_interval_secs: 24 * 3600,
        }
    }
}

pub struct OsmCache {
    pub mem: RefCell<HashMap<TileId, Option<Handle>>>,
    pub pending: RefCell<HashSet<TileId>>,
    pub dir: PathBuf,
    pub source: BasemapSource,
    memory_use: RefCell<HashMap<TileId, MemoryUse>>,
    memory_clock: Cell<u64>,
    in_flight: HashMap<TileId, InFlightTile>,
    next_request_token: u64,
}

impl OsmCache {
    pub fn new(cfg: &OsmCacheConfig) -> Self {
        let dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("foxy");       // ← без .join("osm")
        let _ = std::fs::create_dir_all(&dir);
        cleanup_if_needed(&dir, cfg);
        Self {
            mem: RefCell::new(HashMap::new()),
            pending: RefCell::new(HashSet::new()),
            dir,
            source: BasemapSource::Osm,
            memory_use: RefCell::new(HashMap::new()),
            memory_clock: Cell::new(0),
            in_flight: HashMap::new(),
            next_request_token: 0,
        }
    }

    pub fn set_source(&mut self, source: BasemapSource) {
        if self.source != source {
            self.abort_all_requests();
            self.source = source;
            self.mem.borrow_mut().clear();
            self.pending.borrow_mut().clear();
            self.memory_use.borrow_mut().clear();
            self.memory_clock.set(0);
        }
    }

    // A new map position releases real HTTP tasks, not just their pending
    // markers. Only visible requests continue to occupy one of four slots.
    pub(crate) fn abort_offscreen_requests(&mut self, visible: &[TileId]) {
        let visible: HashSet<TileId> = visible.iter().copied().collect();
        let old: Vec<TileId> = self.in_flight.keys().copied()
            .filter(|id| !visible.contains(id)).collect();
        for id in old {
            if let Some(request) = self.in_flight.remove(&id) {
                request.handle.abort();
            }
            self.pending.borrow_mut().remove(&id);
        }
    }

    pub(crate) fn abort_all_requests(&mut self) {
        for (_, request) in self.in_flight.drain() {
            request.handle.abort();
        }
        self.pending.borrow_mut().clear();
    }

    pub(crate) fn next_request_token(&mut self) -> u64 {
        self.next_request_token = self.next_request_token.wrapping_add(1);
        self.next_request_token
    }

    pub(crate) fn register_request(&mut self, id: TileId, token: u64, handle: TaskHandle) {
        // A previous attempt must not silently keep running in a slot.
        if let Some(previous) = self.in_flight.insert(id, InFlightTile { token, handle }) {
            previous.handle.abort();
        }
        self.pending.borrow_mut().insert(id);
    }

    // Returns false for late messages from cancelled/replaced tasks.
    pub(crate) fn finish_request(&mut self, id: TileId, token: u64) -> bool {
        if self.in_flight.get(&id).is_none_or(|r| r.token != token) {
            return false;
        }
        self.in_flight.remove(&id);
        self.pending.borrow_mut().remove(&id);
        true
    }

    pub fn disk_path(&self, id: TileId) -> PathBuf {
        let (src, ext) = match self.source {
            BasemapSource::Osm => ("osm", "png"),
            BasemapSource::Sentinel2 => ("s2", "jpg"),
            BasemapSource::Esri => ("esri", "jpg"),
        };
        self.dir
            .join(src)
            .join(id.0.to_string())
            .join(id.1.to_string())
            .join(format!("{}.{}", id.2, ext))
    }

    // Touch on drawing and insertion, not on disk access or map movement.
    // The counter avoids modifying file times or relying on clock precision.
    fn touch(&self, id: TileId, cost: Option<usize>) {
        let tick = self.memory_clock.get().saturating_add(1);
        self.memory_clock.set(tick);
        let mut usage = self.memory_use.borrow_mut();
        let entry = usage.entry(id).or_insert(MemoryUse {
            last_access: tick,
            estimated_bytes: cost.unwrap_or(0),
        });
        entry.last_access = tick;
        if let Some(cost) = cost {
            entry.estimated_bytes = cost;
        }
    }

    pub fn get(&self, id: TileId) -> Option<Option<Handle>> {
        let tile = self.mem.borrow().get(&id).cloned();
        if tile.is_some() {
            self.touch(id, None);
        }
        tile
    }

    pub fn insert(&self, id: TileId, h: Option<Handle>, estimated_bytes: usize) {
        self.mem.borrow_mut().insert(id, h);
        self.touch(id, Some(estimated_bytes));
    }

    // Protect all currently visible tiles, even if an unusually large window
    // requires more than the nominal budget. Never cause a visible tile to be
    // evicted and immediately re-requested by the next scheduler pass.
    pub fn prune_for_view(&self, visible: &[TileId]) {
        self.prune_with_limits(visible, MAX_MEMORY_TILES, MAX_MEMORY_ESTIMATED_BYTES);
    }

    fn prune_with_limits(&self, visible: &[TileId], max_tiles: usize, max_bytes: usize) {
        let mut tiles = self.mem.borrow_mut();
        let mut usage = self.memory_use.borrow_mut();
        let mut estimated_total = tiles.keys().fold(0usize, |total, id| {
            total.saturating_add(usage.get(id).map_or(0, |u| u.estimated_bytes))
        });
        if tiles.len() <= max_tiles && estimated_total <= max_bytes {
            return;
        }

        let protected: HashSet<TileId> = visible.iter().copied().collect();
        let mut candidates: Vec<(TileId, u64)> = tiles.keys()
            .filter(|id| !protected.contains(*id))
            .map(|id| (*id, usage.get(id).map_or(0, |u| u.last_access)))
            .collect();
        candidates.sort_unstable_by_key(|&(id, access)| (access, id));
        for (id, _) in candidates {
            if tiles.len() <= max_tiles && estimated_total <= max_bytes {
                break;
            }
            tiles.remove(&id);
            if let Some(removed) = usage.remove(&id) {
                estimated_total = estimated_total.saturating_sub(removed.estimated_bytes);
            }
        }
    }

}

struct CacheFile {
    path: PathBuf,
    mtime: SystemTime,
    size: u64,
}

fn collect_files(dir: &Path, out: &mut Vec<CacheFile>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            let ext = path.extension().and_then(|s| s.to_str());
            if matches!(ext, Some("png") | Some("jpg")) {
                if let Ok(meta) = entry.metadata() {
                    if let (Ok(mtime), size) = (meta.modified(), meta.len()) {
                        out.push(CacheFile { path, mtime, size });
                    }
                }
            }
        }
    }
}

// Interrupted atomic writes can leave .part files. Sweep only online-map
// subtrees; never delete DEM .part files from the shared foxy cache root.
fn cleanup_abandoned_tile_parts(root: &Path) {
    fn visit(dir: &Path, now: SystemTime) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, now);
            } else {
                let Some(name) = path.file_name().and_then(|s| s.to_str()) else { continue };
                if !(name.starts_with('.') && name.ends_with(".part")
                    && (name.contains(".png.") || name.contains(".jpg."))) {
                    continue;
                }
                let expired = entry.metadata().ok()
                    .and_then(|meta| meta.modified().ok())
                    .and_then(|modified| now.duration_since(modified).ok())
                    .is_some_and(|age| age > Duration::from_secs(24 * 3600));
                if expired { let _ = fs::remove_file(path); }
            }
        }
    }
    let now = SystemTime::now();
    for source in ["osm", "s2", "esri"] {
        visit(&root.join(source), now);
    }
}

fn remove_empty_dirs(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            remove_empty_dirs(&path);
            if std::fs::read_dir(&path)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false)
            {
                let _ = std::fs::remove_dir(&path);
            }
        }
    }
}

fn cleanup_cache(dir: &Path, cfg: &OsmCacheConfig) {
    let now = SystemTime::now();
    let max_age = Duration::from_secs(cfg.max_age_days * 24 * 3600);

    let mut files: Vec<CacheFile> = Vec::new();
    collect_files(dir, &mut files);
    let mut total_size: u64 = files.iter().map(|f| f.size).sum();

    // Проход 1: удалить всё старше max_age_days.
    files.retain(|f| {
        if now
            .duration_since(f.mtime)
            .map(|age| age > max_age)
            .unwrap_or(false)
        {
            if std::fs::remove_file(&f.path).is_ok() {
                total_size = total_size.saturating_sub(f.size);
            }
            return false;
        }
        true
    });

    // Проход 2: если всё ещё больше max_total_size — удалить самые старые.
    if total_size > cfg.max_total_size {
        files.sort_by(|a, b| a.mtime.cmp(&b.mtime));
        for f in &files {
            if total_size <= cfg.max_total_size {
                break;
            }
            if std::fs::remove_file(&f.path).is_ok() {
                total_size = total_size.saturating_sub(f.size);
            }
        }
    }

    // Sweep abandoned online tile writes without touching DEM cache.
    cleanup_abandoned_tile_parts(dir);

    // Проход 3: убрать пустые подкаталоги.
    remove_empty_dirs(dir);
}

fn cleanup_if_needed(dir: &Path, cfg: &OsmCacheConfig) {
    let marker = dir.join(".last_cleanup");
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if let Ok(content) = std::fs::read_to_string(&marker) {
        if let Ok(last) = content.trim().parse::<u64>() {
            if now.saturating_sub(last) < cfg.cleanup_interval_secs {
                return;
            }
        }
    }

    cleanup_cache(dir, cfg);
    let _ = std::fs::write(&marker, now.to_string());
}

pub fn client() -> Client {
    let user_agent = format!(
        "FoxyXplorer/{} (+https://github.com/grantbor/FoxyXplorer)",
        env!("CARGO_PKG_VERSION")
    );
    Client::builder()
        .user_agent(user_agent)
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .https_only(true)
        .build()
        .expect("reqwest client")
}

/// Зум OSM под текущий масштаб (метров на пиксель экрана).
pub fn zoom_for_scale(scale_m_per_px: f64, lat_deg: f64) -> u8 {
    let coslat = lat_deg.to_radians().cos().abs().max(1e-6);
    let ideal = (156543.03392 * coslat / scale_m_per_px).log2();
    (ideal.round() as i32).clamp(0, 19) as u8
}

/// Мировые пиксели Web Mercator по (lon, lat, z).
fn world_px(lon: f64, lat: f64, z: u8) -> (f64, f64) {
    let n = TILE_SIZE * 2f64.powi(z as i32);
    let x = (lon + 180.0) / 360.0 * n;
    let lat_r = lat.to_radians();
    let y = (1.0 - (lat_r.tan() + 1.0 / lat_r.cos()).ln() / std::f64::consts::PI) / 2.0 * n;
    (x, y)
}

/// Обратное: мировые пиксели → (lon, lat).
fn px_world(x: f64, y: f64, z: u8) -> (f64, f64) {
    let n = TILE_SIZE * 2f64.powi(z as i32);
    let lon = x / n * 360.0 - 180.0;
    let lat_r = (std::f64::consts::PI * (1.0 - 2.0 * y / n)).sinh().atan();
    (lon, lat_r.to_degrees())
}

pub fn visible_tiles(
    cx: f64,
    cy: f64,
    scale: f64,
    view_w: f64,
    view_h: f64,
    max_z: Option<u8>,
) -> (u8, Vec<TileId>) {
    let (_lon_c, lat_c) = crate::util::geo::lonlat_of(cx, cy);
    let mut z = zoom_for_scale(scale, lat_c);
    if let Some(m) = max_z {
        z = z.min(m);
    }

    let half_w_m = view_w / 2.0 * scale;
    let half_h_m = view_h / 2.0 * scale;

    let corners = [
        (cx - half_w_m, cy + half_h_m), // NW
        (cx + half_w_m, cy - half_h_m), // SE
    ];

    let mut xs = [0.0f64; 2];
    let mut ys = [0.0f64; 2];
    for (i, (mx, my)) in corners.iter().enumerate() {
        let (lon, lat) = crate::util::geo::lonlat_of(*mx, *my);
        let (px, py) = world_px(lon, lat, z);
        xs[i] = px;
        ys[i] = py;
    }

    let x0 = (xs[0].min(xs[1]) / TILE_SIZE).floor() as i64;
    let x1 = (xs[0].max(xs[1]) / TILE_SIZE).floor() as i64;
    let y0 = (ys[0].min(ys[1]) / TILE_SIZE).floor() as i64;
    let y1 = (ys[0].max(ys[1]) / TILE_SIZE).floor() as i64;

    let max_tile = (2u32.pow(z as u32) - 1) as i64;
    let mut out = Vec::new();
    for ty in y0..=y1 {
        if ty < 0 || ty > max_tile {
            continue;
        }
        for tx in x0..=x1 {
            if tx < 0 || tx > max_tile {
                continue;
            }
            out.push((z, tx as u32, ty as u32));
        }
    }
    (z, out)
}

pub fn tile_screen_rect(
    id: TileId,
    cx: f64,
    cy: f64,
    scale: f64,
    view_w: f64,
    view_h: f64,
) -> iced::Rectangle {
    let (z, tx, ty) = id;
    let (lon_tl, lat_tl) = px_world(tx as f64 * TILE_SIZE, ty as f64 * TILE_SIZE, z);
    let (lon_br, lat_br) = px_world(
        (tx as f64 + 1.0) * TILE_SIZE,
        (ty as f64 + 1.0) * TILE_SIZE,
        z,
    );
    let (mx_tl, my_tl) = crate::util::geo::merc_of(lon_tl, lat_tl);
    let (mx_br, my_br) = crate::util::geo::merc_of(lon_br, lat_br);
    let hw = view_w / 2.0;
    let hh = view_h / 2.0;
    let sx0 = hw + (mx_tl - cx) / scale;
    let sy0 = hh - (my_tl - cy) / scale;
    let sx1 = hw + (mx_br - cx) / scale;
    let sy1 = hh - (my_br - cy) / scale;
    iced::Rectangle {
        x: sx0.min(sx1) as f32,
        y: sy0.min(sy1) as f32,
        width: (sx1 - sx0).abs() as f32,
        height: (sy1 - sy0).abs() as f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_cache_for_test() -> OsmCache {
        OsmCache {
            mem: RefCell::new(HashMap::new()),
            pending: RefCell::new(HashSet::new()),
            dir: PathBuf::new(),
            source: BasemapSource::Osm,
            memory_use: RefCell::new(HashMap::new()),
            memory_clock: Cell::new(0),
            in_flight: HashMap::new(),
            next_request_token: 0,
        }
    }

    fn fake_task_handle() -> TaskHandle {
        let (_, handle) = iced::Task::<()>::none().abortable();
        handle
    }

    #[test]
    fn viewport_change_aborts_obsolete_requests_and_frees_slots() {
        let mut cache = memory_cache_for_test();
        let visible = (12, 3, 5);
        let old = (12, 9, 9);
        let first = fake_task_handle();
        let old_handle = first.clone();
        let token_1 = cache.next_request_token();
        cache.register_request(old, token_1, first);
        let second = fake_task_handle();
        let retained = second.clone();
        let token_2 = cache.next_request_token();
        cache.register_request(visible, token_2, second);

        cache.abort_offscreen_requests(&[visible]);
        assert!(old_handle.is_aborted());
        assert!(!retained.is_aborted());
        assert_eq!(cache.pending.borrow().len(), 1);
        assert!(cache.pending.borrow().contains(&visible));
        let empty = HashMap::new();
        assert_eq!(plan_tile_requests(&[(12, 4, 5)], &empty,
            &cache.pending.borrow(), &HashMap::new(), Instant::now()), vec![(12, 4, 5)]);
    }

    #[test]
    fn cancelled_result_cannot_replace_new_attempt_for_same_tile() {
        let mut cache = memory_cache_for_test();
        let id = (11, 42, 42);
        let token_1 = cache.next_request_token();
        let old_handle = fake_task_handle();
        let old_copy = old_handle.clone();
        cache.register_request(id, token_1, old_handle);
        cache.abort_all_requests();
        assert!(old_copy.is_aborted());

        let token_2 = cache.next_request_token();
        cache.register_request(id, token_2, fake_task_handle());
        assert!(!cache.finish_request(id, token_1));
        assert!(cache.pending.borrow().contains(&id));
        assert!(cache.finish_request(id, token_2));
        assert!(!cache.pending.borrow().contains(&id));
    }

    #[test]
    fn basemap_change_aborts_requests_and_clears_pending() {
        let mut cache = memory_cache_for_test();
        let id = (8, 2, 6);
        let handle = fake_task_handle();
        let observed = handle.clone();
        let token = cache.next_request_token();
        cache.register_request(id, token, handle);
        cache.set_source(BasemapSource::Esri);
        assert!(observed.is_aborted());
        assert!(cache.pending.borrow().is_empty());
        assert!(!cache.finish_request(id, token));
    }

    #[test]
    fn prioritize_center_tiles_instead_of_top_left_row() {
        let mut tiles: Vec<_> = (0..3).flat_map(|y| (0..3).map(move |x| (12, x, y))).collect();
        prioritize_visible_tiles(&mut tiles);
        assert_eq!(tiles[0], (12, 1, 1));
        assert!(tiles[1..5].iter().all(|id| {
            let dx = (id.1 as i32 - 1).abs();
            let dy = (id.2 as i32 - 1).abs();
            dx + dy == 1
        }));
    }

    fn fake_handle() -> Handle {
        // Memory eviction must not decode or inspect image bytes.
        Handle::from_bytes(vec![0])
    }

    #[test]
    fn memory_cache_evicts_least_recently_used_and_respects_byte_budget() {
        let cache = memory_cache_for_test();
        let ids: Vec<TileId> = (0..4).map(|x| (10, x, 5)).collect();
        for &id in &ids {
            cache.insert(id, Some(fake_handle()), 10);
        }
        // id[0] is recently drawn, while id[1] is visible and protected.
        assert!(cache.get(ids[0]).is_some());
        cache.prune_with_limits(&[ids[1]], 3, 25);
        assert_eq!(cache.mem.borrow().len(), 2);
        assert!(cache.get(ids[0]).is_some());
        assert!(cache.get(ids[1]).is_some());
        assert!(cache.get(ids[2]).is_none());
        assert!(cache.get(ids[3]).is_none());
        assert_eq!(cache.memory_use.borrow().len(), 2);
    }

    #[test]
    fn memory_cache_never_evicts_visible_tiles_even_above_budget() {
        let cache = memory_cache_for_test();
        let ids: Vec<TileId> = (0..5).map(|x| (11, x, 7)).collect();
        for &id in &ids {
            cache.insert(id, Some(fake_handle()), 1_024);
        }
        cache.prune_with_limits(&ids, 2, 2_048);
        assert_eq!(cache.mem.borrow().len(), ids.len());
        // Once the viewport changes, off-screen tiles can be removed.
        cache.prune_with_limits(&[ids[3]], 2, 2_048);
        assert_eq!(cache.mem.borrow().len(), 2);
        assert!(cache.get(ids[3]).is_some());
    }

    #[test]
    fn memory_cache_evicts_missing_markers_and_resets_on_source_change() {
        let mut cache = memory_cache_for_test();
        let missing = (12, 8, 9);
        let other = (12, 8, 10);
        cache.insert(missing, None, 1);
        cache.insert(other, Some(fake_handle()), 100);
        cache.prune_with_limits(&[other], 1, 100);
        assert!(cache.get(missing).is_none());
        assert!(cache.get(other).is_some());
        cache.pending.borrow_mut().insert(other);
        cache.set_source(BasemapSource::Esri);
        assert!(cache.mem.borrow().is_empty());
        assert!(cache.memory_use.borrow().is_empty());
        assert!(cache.pending.borrow().is_empty());
    }

    #[tokio::test]
    async fn evicted_memory_tile_remains_usable_from_disk_without_network() {
        let dir = test_dir("memory-eviction-disk");
        let disk = dir.join("17.png");
        let bytes = test_tile(ImageFormat::Png);
        fs::write(&disk, &bytes).unwrap();
        let cache = memory_cache_for_test();
        let old = (14, 10, 17);
        let visible = (14, 10, 18);
        cache.insert(old, Some(Handle::from_bytes(bytes.clone())), 100);
        cache.insert(visible, Some(fake_handle()), 100);
        cache.prune_with_limits(&[visible], 1, 100);
        assert!(cache.get(old).is_none());
        let loaded = load_validated_tile(&disk, ImageFormat::Png, || async {
            panic!("a memory eviction must not cause an HTTP request")
        }).await.unwrap();
        assert_eq!(loaded, bytes);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn memory_cache_estimate_includes_decoded_pixels_and_encoded_bytes() {
        let bytes = test_tile(ImageFormat::Png);
        assert_eq!(estimated_tile_memory(&bytes, ImageFormat::Png), bytes.len() + 16);
    }

    #[test]
    fn completed_tile_frees_slot_for_next_visible_tile() {
        let ids: Vec<TileId> = (0..8).map(|x| (8, x, 5)).collect();
        let cached = HashMap::new();
        let mut pending: HashSet<TileId> = ids[..4].iter().copied().collect();
        let failures = HashMap::new();
        let now = Instant::now();

        assert!(plan_tile_requests(&ids, &cached, &pending, &failures, now).is_empty());
        pending.remove(&ids[0]);
        assert_eq!(plan_tile_requests(&ids, &cached, &pending, &failures, now), vec![ids[0]]);
        // Once the completed tile is cached, the next missing tile is selected.
        let mut cached = HashMap::new();
        cached.insert(ids[0], Some(Handle::from_bytes(vec![1])));
        assert_eq!(plan_tile_requests(&ids, &cached, &pending, &failures, now), vec![ids[4]]);
    }

    #[test]
    fn transient_errors_wait_and_resume_without_navigation() {
        let now = Instant::now();
        let id = (10, 3, 4);
        let visible = [id];
        let cached = HashMap::new();
        let pending = HashSet::new();
        let mut retries = HashMap::new();

        for (attempt, delay) in [(1, 2), (2, 5), (3, 10), (4, 20), (5, 30), (6, 30)] {
            let fail = TileRetry::after_failure(retries.get(&id), now);
            assert_eq!(fail.attempts(), attempt);
            retries.insert(id, fail);
            assert!(plan_tile_requests(&visible, &cached, &pending, &retries,
                now + Duration::from_secs(delay - 1)).is_empty());
            assert_eq!(plan_tile_requests(&visible, &cached, &pending, &retries,
                now + Duration::from_secs(delay)), vec![id]);
        }
    }

    #[test]
    fn permanently_missing_tiles_are_not_retried() {
        assert!(is_permanently_missing_tile_error("HTTP 404"));
        assert!(is_permanently_missing_tile_error("HTTP 410"));
        assert!(!is_permanently_missing_tile_error("HTTP 503"));
        assert!(!is_permanently_missing_tile_error("connection reset"));

        let id = (7, 20, 21);
        let mut cached = HashMap::new();
        cached.insert(id, None);
        assert!(plan_tile_requests(&[id], &cached, &HashSet::new(),
            &HashMap::new(), Instant::now()).is_empty());
    }

    fn test_tile(format: ImageFormat) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 2)
            .write_to(&mut out, format).unwrap();
        out.into_inner()
    }

    fn test_dir(name: &str) -> PathBuf {
        let n = TEMP_TILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "foxy-online-cache-{name}-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn valid_disk_tile_is_reused_without_network() {
        let dir = test_dir("reuse");
        let disk = dir.join("tile.png");
        let valid = test_tile(ImageFormat::Png);
        fs::write(&disk, &valid).unwrap();
        let result = load_validated_tile(&disk, ImageFormat::Png, || async {
            panic!("valid cached tile must not be downloaded")
        }).await.unwrap();
        assert_eq!(result, valid);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn corrupted_cached_tile_is_replaced_once_and_then_reused_offline() {
        let dir = test_dir("repair");
        let disk = dir.join("10.png");
        let valid = test_tile(ImageFormat::Png);
        fs::write(&disk, &valid[..valid.len() / 2]).unwrap();
        let received = load_validated_tile(&disk, ImageFormat::Png, || {
            let valid = valid.clone();
            async move { Ok(valid) }
        }).await.unwrap();
        assert_eq!(received, valid);
        assert_eq!(fs::read(&disk).unwrap(), valid);
        let result = load_validated_tile(&disk, ImageFormat::Png, || async {
            panic!("repaired tile must work offline")
        }).await.unwrap();
        assert_eq!(result, valid);
        assert!(fs::read_dir(&dir).unwrap().all(|e| {
            !e.unwrap().file_name().to_string_lossy().ends_with(".part")
        }));
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn malformed_download_cannot_poison_cache() {
        let dir = test_dir("bad-response");
        let disk = dir.join("11.png");
        let bad = b"<html>upstream error</html>".to_vec();
        assert!(load_validated_tile(&disk, ImageFormat::Png, || async move {
            Ok(bad)
        }).await.is_err());
        assert!(!disk.exists());
        let valid = test_tile(ImageFormat::Png);
        assert_eq!(load_validated_tile(&disk, ImageFormat::Png, || {
            let valid = valid.clone();
            async move { Ok(valid) }
        }).await.unwrap(), valid);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn jpeg_sources_reject_png_and_truncated_files() {
        let dir = test_dir("formats");
        let disk = dir.join("12.jpg");
        let png = test_tile(ImageFormat::Png);
        let jpeg = test_tile(ImageFormat::Jpeg);
        assert!(validate_tile(&png, ImageFormat::Jpeg).is_err());
        assert!(validate_tile(&jpeg, ImageFormat::Jpeg).is_ok());
        assert!(validate_tile(&jpeg[..jpeg.len() / 2], ImageFormat::Jpeg).is_err());
        assert!(load_validated_tile(&disk, ImageFormat::Jpeg, || async move {
            Ok(png)
        }).await.is_err());
        assert!(!disk.exists());
        assert_eq!(load_validated_tile(&disk, ImageFormat::Jpeg, || {
            let jpeg = jpeg.clone();
            async move { Ok(jpeg) }
        }).await.unwrap(), jpeg);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn abandoned_online_part_cleanup_does_not_touch_dem() {
        let root = test_dir("parts");
        let source_dir = root.join("osm/8/120");
        let dem_dir = root.join("dem/glo30");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&dem_dir).unwrap();
        let stale = source_dir.join(".4.png.123.0.part");
        let fresh = source_dir.join(".5.png.123.1.part");
        let dem = dem_dir.join("download.part");
        fs::write(&stale, b"old").unwrap();
        fs::write(&fresh, b"active").unwrap();
        fs::write(&dem, b"dem").unwrap();
        let file = OpenOptions::new().write(true).open(&stale).unwrap();
        file.set_times(fs::FileTimes::new().set_modified(
            SystemTime::now() - Duration::from_secs(48 * 3600))).unwrap();
        cleanup_abandoned_tile_parts(&root);
        assert!(!stale.exists());
        assert!(fresh.exists());
        assert!(dem.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
