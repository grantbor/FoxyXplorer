use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use iced::widget::image::Handle;
use reqwest::Client;

pub type TileId = (u8, u32, u32);

pub const TILE_SIZE: f64 = 256.0;

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
}

impl OsmCache {
    pub fn new(cfg: &OsmCacheConfig) -> Self {
        let dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("foxy")
            .join("osm");
        let _ = std::fs::create_dir_all(&dir);
        cleanup_if_needed(&dir, cfg);
        Self {
            mem: RefCell::new(HashMap::new()),
            pending: RefCell::new(HashSet::new()),
            dir,
        }
    }

    pub fn disk_path(&self, id: TileId) -> PathBuf {
        self.dir
            .join(id.0.to_string())
            .join(id.1.to_string())
            .join(format!("{}.png", id.2))
    }

    pub fn get(&self, id: TileId) -> Option<Option<Handle>> {
        self.mem.borrow().get(&id).cloned()
    }

    pub fn insert(&self, id: TileId, h: Option<Handle>) {
        self.mem.borrow_mut().insert(id, h);
    }

    pub fn unmark_pending(&self, id: TileId) {
        self.pending.borrow_mut().remove(&id);
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
        } else if path.extension().and_then(|s| s.to_str()) == Some("png") {
            if let Ok(meta) = entry.metadata() {
                if let (Ok(mtime), size) = (meta.modified(), meta.len()) {
                    out.push(CacheFile { path, mtime, size });
                }
            }
        }
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
        "FoxyXplorer/{} (+https://github.com/grantbor/FoxyXplorer; contact: your-email@example.com)",
        env!("CARGO_PKG_VERSION")
    );
    Client::builder()
        .user_agent(user_agent)
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
) -> (u8, Vec<TileId>) {
    let (_lon_c, lat_c) = crate::util::geo::lonlat_of(cx, cy);
    let z = zoom_for_scale(scale, lat_c);

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
