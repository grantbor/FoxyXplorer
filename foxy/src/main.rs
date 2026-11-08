mod document;
mod osm;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use coord::{format_pair, CoordFormat};
use iced::mouse::ScrollDelta;
use iced::widget::canvas::{self, Cache, Canvas, Geometry, Path as CanvasPath, Stroke};
use iced::widget::image::Handle as ImageHandle;

use std::collections::{HashMap, HashSet};

use iced::widget::{
    button, checkbox, column, container, mouse_area, pick_list, row, scrollable, stack, text,
    text_input, Space,
};
use iced::{
    Alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Task, Theme,
};

use iced::keyboard::Modifiers;
use iced::Subscription;
use serde::{Deserialize, Serialize};

const VIEW_W: f32 = 1100.0;
const VIEW_H: f32 = 720.0;
const R: f64 = 6_378_137.0;
const BG: [u8; 4] = [38, 38, 42, 255];


pub fn merc_of(lon: f64, lat: f64) -> (f64, f64) {
    (
        R * lon.to_radians(),
        R * (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.0).tan().ln(),
    )
}

pub fn lonlat_of(x: f64, y: f64) -> (f64, f64) {
    (
        (x / R).to_degrees(),
        (2.0 * (y / R).exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees(),
    )
}


#[derive(Default)]
struct Selection {
    tracks: HashSet<u64>,
    waypoints: HashSet<u64>,
}

/// Аффинное приближение «Меркатор (м) → пиксель растра уровня 0» в точке (cx, cy).
struct Affine {
    x0: f64,
    y0: f64,
    ax: f64,
    ay: f64,
    bx: f64,
    by: f64,
}

fn affine(g: &ozmap::Georef, cx: f64, cy: f64) -> Affine {
    const D: f64 = 1000.0;
    let at = |mx: f64, my: f64| {
        let (lon, lat) = lonlat_of(mx, my);
        g.lonlat_to_pixel(lon, lat)
    };
    let (x0, y0) = at(cx, cy);
    let (x1, y1) = at(cx + D, cy);
    let (x2, y2) = at(cx, cy + D);
    Affine {
        x0,
        y0,
        ax: (x1 - x0) / D,
        ay: (y1 - y0) / D,
        bx: (x2 - x0) / D,
        by: (y2 - y0) / D,
    }
}

struct Layer {
    name: String,
    kit_name: String,
    ozf: RefCell<ozf::Ozf>,
    georef: ozmap::Georef,
    /// (ozf-индекс_уровня, ratio_x, ratio_y), от крупного к мелкому
    levels: Vec<(usize, f64, f64)>,
}

fn today() -> (i32, u16) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let (y, m, d, ..) = geoio::time::parts(secs);
    let ordinal = geoio::time::days_from_civil(y, m, d) - geoio::time::days_from_civil(y, 1, 1) + 1;
    (y as i32, ordinal as u16)
}

fn date_offset(days: i64) -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        + days * 86400;
    let (y, m, d, ..) = geoio::time::parts(secs as f64);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn build_template(d: &document::TrackProps) -> String {
    if d.callsign == "Zadacha" {
        format!("Zadacha{}_{}", d.num1, d.date_str.replace("-", ""))
    } else {
        let n2 = d.num2.map(|n| format!("_{}", n)).unwrap_or_default();
        let sfx = if d.suffix.is_empty() { String::new() } else { format!("_{}", d.suffix) };
        format!("{}_{}_{}{}{}", d.date_str.replace("-", ""), d.callsign, d.num1, n2, sfx)
    }
}

type GridData = (grid::Grid, Vec<Vec<(f64, f64)>>);

fn load_kit(root: &Path) -> Result<(Vec<Layer>, Option<GridData>, Vec<String>), String> {
    let mut notes = Vec::new();

    // --- 1. Собираем директории с .map и .gpx ---
    let mut map_dirs: Vec<PathBuf> = Vec::new();
    let mut data_dirs: Vec<PathBuf> = Vec::new();

    // (a) Передан один OZI-каталог: root/Maps и root/Data
    if root.join("Maps").is_dir() {
        map_dirs.push(root.join("Maps"));
    }
    if root.join("Data").is_dir() {
        data_dirs.push(root.join("Data"));
    }

    // (b) Передан корень комплекта: root/*/Maps и root/*/Data
    if let Ok(entries) = std::fs::read_dir(root) {
        for e in entries.flatten() {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let maps_sub = p.join("Maps");
            let data_sub = p.join("Data");
            if maps_sub.is_dir() {
                map_dirs.push(maps_sub);
            }
            if data_sub.is_dir() {
                data_dirs.push(data_sub);
            }
        }
    }

    // (c) Плоский комплект (старый формат) — обратная совместимость
    if map_dirs.is_empty() {
        map_dirs.push(root.to_path_buf());
    }
    if data_dirs.is_empty() {
        data_dirs.push(root.to_path_buf());
    }

    // --- 2. Читаем .map-файлы ---
    let mut maps: Vec<PathBuf> = Vec::new();
    for d in &map_dirs {
        if let Ok(entries) = std::fs::read_dir(d) {
            for e in entries.flatten() {
                let p = e.path();
                let ext = p
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_lowercase());
                if ext.as_deref() == Some("map") {
                    maps.push(p);
                }
            }
        }
    }
    maps.sort();

    let mut layers = Vec::new();
    // Получаем имя корневой папки комплекта (например, "2023-09-21_Novosibirsk")
    let kit_name = root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    for p in &maps {
        let stem = p
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
            
        let map = match ozmap::MapFile::open(p) {
            Ok(m) => m,
            Err(e) => {
                notes.push(format!("{stem}: {e}"));
                continue;
            }
        };
        if !map.projection.to_lowercase().contains("mercator") {
            notes.push(format!(
                "{stem}: проекция «{}» пока не поддерживается",
                map.projection
            ));
            continue;
        }
        let georef = match map.georef() {
            Ok(g) => g,
            Err(e) => {
                notes.push(format!("{stem}: {e}"));
                continue;
            }
        };

        // Растр лежит рядом с .map (в той же Maps/). Если image_file абсолютный —
        // Path::join корректно его подставит, если относительный — склеит с parent.
        let parent = p.parent().unwrap_or(root);
        let raster = parent.join(&map.image_file);
        let ozf = match ozf::Ozf::open(&raster) {
            Ok(o) => o,
            Err(e) => {
                notes.push(format!("{stem}: {} — {e}", map.image_file));
                continue;
            }
        };
        if ozf.levels.is_empty() {
            notes.push(format!(
                "{stem}: пустой список уровней в {}",
                map.image_file
            ));
            continue;
        }
        let bw = ozf.levels.iter().map(|l| l.width).max().unwrap_or(1) as f64;
        let bh = ozf.levels.iter().map(|l| l.height).max().unwrap_or(1) as f64;
        let mut levels: Vec<(usize, f64, f64)> = ozf
            .levels
            .iter()
            .enumerate()
            .map(|(i, l)| (i, l.width as f64 / bw, l.height as f64 / bh))
            .collect();
        levels.sort_by(|a, b| b.1.total_cmp(&a.1));

        // --- Формирование короткого имени для списка карт ---
        let mut short = stem.strip_suffix("_ozf").unwrap_or(&stem).to_string();

        // Пытаемся отрезать префикс комплекта (если он есть в имени файла)
        if !kit_name.is_empty() && short.starts_with(&kit_name) {
            short = short[kit_name.len()..].trim_start_matches('_').to_string();
        } else if short.len() > 11
            && short.chars().nth(4) == Some('-')
            && short.chars().nth(7) == Some('-')
        {
            // Fallback: если папка называется иначе, но файл начинается с даты (YYYY-MM-DD_),
            // отрезаем дату и название города (до следующего подчеркивания).
            short = short.chars().skip(11).collect::<String>();
            if let Some(idx) = short.find('_') {
                short = short[idx + 1..].to_string();
            }
        }

        layers.push(Layer {
            name: short,
            kit_name: kit_name.clone(),
            ozf: RefCell::new(ozf),
            georef,
            levels,
        });
    }

    if layers.is_empty() {
        return Err(format!(
            "не найдено ни одной пригодной карты (.map + .ozf2).\n\
             Проверены каталоги: {}.\n{}",
            map_dirs
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
            notes.join("; ")
        ));
    }

    // --- 3. Ищем сетку среди всех Data/*.gpx ---
    let mut gpxs: Vec<PathBuf> = Vec::new();
    for d in &data_dirs {
        if let Ok(entries) = std::fs::read_dir(d) {
            for e in entries.flatten() {
                let p = e.path();
                let ext = p
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_lowercase());
                if ext.as_deref() == Some("gpx") {
                    gpxs.push(p);
                }
            }
        }
    }

    let mut best: Option<GridData> = None;
    for p in &gpxs {
        let Ok(bytes) = std::fs::read(p) else { continue };
        let Ok(g) = grid::Grid::from_gpx(&geoio::text::decode(&bytes)) else {
            continue;
        };
        if best
            .as_ref()
            .map_or(true, |(b, _)| g.nodes.len() > b.nodes.len())
        {
            let mut lines: Vec<Vec<(f64, f64)>> = Vec::new();
            if let Ok(data) = geoio::gpx::read(&bytes) {
                for t in &data.tracks {
                    for seg in &t.segments {
                        lines.push(seg.iter().map(|pt| merc_of(pt.lon, pt.lat)).collect());
                    }
                }
            }
            best = Some((g, lines));
        }
    }
    if best.is_none() {
        notes.push(format!(
            "сетка (GPX с именованными точками) не найдена в: {}",
            data_dirs
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    Ok((layers, best, notes))
}

#[derive(Debug, Clone)]
enum Message {
    Layer(usize),
    ToggleGrid,
    ToggleLabels,
    CycleFormat,
    Decl(f64),
    DeclReset,
    Pressed,
    Released,
    Moved(Point),
    Scrolled(ScrollDelta),
    Fit,
    PickKit,
    KitPicked(Option<PathBuf>),
    OsmTileLoaded(osm::TileId, Result<Vec<u8>, String>),
    PickTrack,
    TracksPicked(Option<Vec<PathBuf>>),
    ToggleTrackVisible(u64, bool),
    SelectTrack(u64),
    ToggleAllTracksVisible,
    DeleteSelectedTracks,
    SaveTracksPlt,
    SaveTracksPltPicked(Option<PathBuf>),
    SaveTracksGpx,
    SaveTracksGpxPicked(Option<PathBuf>),
    SaveTracksMs,
    SaveTracksMsPicked(Option<PathBuf>),

    ToggleWaypointVisible(u64, bool),
    SelectWaypoint(u64),
    SaveWaypointsWptNew,
    SaveWaypointsWptNewPicked(Option<PathBuf>),
    SaveWaypointsWptAppend,
    SaveWaypointsWptAppendPicked(Option<PathBuf>),
    ToggleAllWaypointsVisible,
    DeleteSelectedWaypoints,
    ModifiersChanged(Modifiers),
    SearchTracksChanged(String),
    SearchWaypointsChanged(String),
    WindowCloseRequested,
    
    OpenTrackProps(u64),
    CloseTrackProps,
    SaveTrackProps,
    SaveTrackPropsPicked(Option<PathBuf>),
    
    // Изменения в драфте
    DraftDescChanged(String),
    DraftColorChanged([u8; 3]),
    DraftWidthChanged(String),
    DraftStyleChanged(document::LineStyle),
    DraftTypeChanged(document::PolyType),
    DraftFillColorChanged([u8; 3]),
    DraftFillTypeChanged(document::FillType),
    
    DraftCallsignChanged(String),
    DraftNum1Changed(String),
    DraftNum2Changed(String),
    DraftSuffixChanged(String),
    DraftDateYesterday,
    DraftDateToday,
    DraftDateChanged(String),
    ApplyTrackProps, 
    ToggleColorPicker,
    ColorChannelChanged(usize, u8), // 0 = R, 1 = G, 2 = B
}

#[derive(Serialize, Deserialize)]
struct SessionView {
    cx: f64,
    cy: f64,
    scale: f64,
    active: usize,
    show_grid: bool,
    show_labels: bool,
    format_str: String, // "Decimal", "DegMin", "DegMinSec"
    decl_offset: f64,
}

#[derive(Serialize, Deserialize)]
struct Session {
    version: u32,
    last_kit: Option<PathBuf>,
    view: SessionView,
    tracks: Vec<document::Track>,
    waypoints: Vec<document::Waypoint>,
    next_id: u64,
}

fn session_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("foxy")
        .join("session.json")
}

fn load_palette() -> Vec<[u8; 3]> {
    let path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("foxy")
        .join("palette.json");

    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(arr) = serde_json::from_str::<Vec<[u8; 3]>>(&content) {
            return arr;
        }
    }

    // Дефолтная палитра (как на скриншоте)
    let default = vec![
        [255, 0, 0],     // Red
        [0, 0, 255],     // Blue
        [0, 128, 0],     // Green
        [255, 255, 255], // White
        [0, 255, 255],   // Cyan
        [255, 255, 0],   // Yellow
        [255, 0, 255],   // Magenta
        [0, 255, 0],     // Lime
        [0, 0, 0],       // Black
    ];

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, serde_json::to_string_pretty(&default).unwrap_or_default());

    default
}

fn get_extended_palette() -> Vec<[u8; 3]> {
    let mut colors = Vec::new();
    // Градации серого и базовые
    colors.extend_from_slice(&[[0,0,0], [85,85,85], [170,170,170], [255,255,255]]);
    // Основные цвета (6 градаций яркости)
    for &(r, g, b) in &[(255,0,0), (0,255,0), (0,0,255), (255,255,0), (0,255,255), (255,0,255), (128,0,0), (0,128,0), (0,0,128), (128,128,0), (0,128,128), (128,0,128)] {
        colors.push([r, g, b]);
        colors.push([r/2, g/2, b/2]);
        colors.push([(r as u16 * 3 / 4) as u8, (g as u16 * 3 / 4) as u8, (b as u16 * 3 / 4) as u8]);
    }
    colors
}

fn save_session(app: &App) {
    let format_str = match app.format {
        CoordFormat::Decimal => "Decimal",
        CoordFormat::DegMin => "DegMin",
        CoordFormat::DegMinSec => "DegMinSec",
    }.to_string();

    let session = Session {
        version: 1,
        last_kit: app.last_kit.clone(),
        view: SessionView {
            cx: app.cx,
            cy: app.cy,
            scale: app.scale,
            active: app.active,
            show_grid: app.show_grid,
            show_labels: app.show_labels,
            format_str,
            decl_offset: app.decl_offset,
        },
        tracks: app.tracks.clone(),
        waypoints: app.waypoints.clone(),
        next_id: app.next_id,
    };

    match serde_json::to_string_pretty(&session) {
        Ok(json) => {
            let path = session_path();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::write(&path, json) {
                Ok(_) => eprintln!("DEBUG: Сессия успешно сохранена в {:?}", path),
                Err(e) => eprintln!("DEBUG: Ошибка записи файла сессии: {}", e),
            }
        }
        Err(e) => eprintln!("DEBUG: Ошибка сериализации сессии: {}", e),
    }
}

fn load_session() -> Option<Session> {
    let path = session_path();
    let content = std::fs::read_to_string(path).ok()?;
    let session: Session = serde_json::from_str(&content).ok()?;
    if session.version == 1 {
        Some(session)
    } else {
        None
    }
}

struct App {
    layers: Vec<Layer>,
    active: usize, // 0..=layers.len(); == layers.len() означает OSM
    grid: Option<grid::Grid>,
    lines: Vec<Vec<(f64, f64)>>,
    show_grid: bool,
    show_labels: bool,
    declination: f64,
    decl_offset: f64,
    format: CoordFormat,
    cx: f64,
    cy: f64,
    scale: f64,
    canvas_cache: Cache,
    grid_cache: Cache,
    osm_canvas_cache: Cache,
    tile_handles: RefCell<HashMap<(usize, usize, u32, u32), Option<ImageHandle>>>,
    dragging: bool,
    cursor: Option<Point>,
    status: String,
    last_geom_ms: Cell<f64>,
    last_bounds: Cell<Size>,
    osm_cache: osm::OsmCache,
    osm_client: reqwest::Client,
    osm_z: u8,
    tracks: Vec<document::Track>,
    waypoints: Vec<document::Waypoint>,
    next_id: u64,
    tracks_cache: Cache,
    selection: Selection,
    modifiers: Modifiers,
    last_track_clicked: Option<u64>,
    last_waypoint_clicked: Option<u64>,
    search_tracks: String,
    search_waypoints: String,
    last_kit: Option<PathBuf>, 
    palette: Vec<[u8; 3]>,
    modal: Option<Modal>,
    track_backup: Option<document::Track>, 
    last_track_click: Option<(u64, std::time::Instant)>,
    show_color_picker: bool,
}
enum Modal {
    TrackProps { track_id: u64, draft: document::TrackProps },
}

fn foxy_config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("foxy")
}

fn last_kit_path() -> PathBuf {
    foxy_config_dir().join("last_kit")
}

fn load_last_kit() -> Option<PathBuf> {
    std::fs::read_to_string(last_kit_path())
        .ok()
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| p.is_dir())
}

fn save_last_kit(path: &Path) {
    let _ = std::fs::create_dir_all(foxy_config_dir());
    let _ = std::fs::write(last_kit_path(), path.to_string_lossy().as_bytes());
}


impl App {
    fn osm_active(&self) -> bool {
        self.active >= self.layers.len()
    }

    fn new() -> (App, Task<Message>) {
        let args: Vec<String> = std::env::args().collect();
        let force_osm = args.iter().any(|a| a == "--osm");
        let dir_arg = args
            .iter()
            .skip(1)
            .find(|a| !a.starts_with("--"))
            .cloned();

        // Настройки кэша OSM (env, потом settings.toml).
        let cache_cfg = osm::OsmCacheConfig {
            max_age_days: std::env::var("FOXY_CACHE_MAX_AGE_DAYS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(90),
            max_total_size: std::env::var("FOXY_CACHE_MAX_SIZE_MB")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .map(|mb| mb * 1024 * 1024)
                .unwrap_or(2 * 1024 * 1024 * 1024),
            cleanup_interval_secs: std::env::var("FOXY_CACHE_CLEANUP_HOURS")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .map(|h| h * 3600)
                .unwrap_or(24 * 3600),
        };

        // ==========================================
        // 1. ПОПЫТКА ЗАГРУЗИТЬ СЕССИЮ
        // ==========================================
        if !force_osm {
            if let Some(session) = load_session() {
                let mut app = App {
                    layers: Vec::new(),
                    active: 0, // Будет восстановлено из сессии
                    grid: None,
                    lines: Vec::new(),
                    show_grid: true,
                    show_labels: true,
                    declination: 0.0,
                    decl_offset: 0.0,
                    format: CoordFormat::Decimal,
                    cx: 0.0,
                    cy: 0.0,
                    scale: 1.0,
                    canvas_cache: Cache::default(),
                    grid_cache: Cache::default(),
                    osm_canvas_cache: Cache::default(),
                    tile_handles: RefCell::new(HashMap::new()),
                    dragging: false,
                    cursor: None,
                    status: String::new(),
                    last_geom_ms: Cell::new(0.0),
                    last_bounds: Cell::new(Size::new(VIEW_W, VIEW_H)),
                    osm_cache: osm::OsmCache::new(&cache_cfg),
                    osm_client: osm::client(),
                    osm_z: 0,
                    
                    // Восстанавливаем пользовательские данные из сессии
                    tracks: session.tracks,
                    waypoints: session.waypoints,
                    next_id: session.next_id,
                    tracks_cache: Cache::default(),
                    selection: Selection::default(),
                    
                    // Новые поля
                    last_kit: session.last_kit.clone(),
                    modifiers: iced::keyboard::Modifiers::default(),
                    last_track_clicked: None,
                    last_waypoint_clicked: None,
                    search_tracks: String::new(),
                    search_waypoints: String::new(),
                    palette: load_palette(),
                    modal: None,
                    track_backup: None,
                    last_track_click: None, 
                    show_color_picker: false,
                };

                // Пересчитываем кэш статистики (количество точек и расстояние) для восстановленных треков
                for t in &mut app.tracks {
                    document::recompute_track_stats(t);
                }

                // Загружаем карты из last_kit (они не хранятся в JSON, только путь)
                if let Some(kit_path) = &app.last_kit {
                    app.apply_kit(load_kit(kit_path));
                }

                // Восстанавливаем вид и настройки поверх загруженных карт
                app.cx = session.view.cx;
                app.cy = session.view.cy;
                app.scale = session.view.scale;
                app.active = session.view.active.min(app.layers.len()); // защита от выхода за границы
                app.show_grid = session.view.show_grid;
                app.show_labels = session.view.show_labels;
                app.decl_offset = session.view.decl_offset;
                app.format = match session.view.format_str.as_str() {
                    "DegMin" => CoordFormat::DegMin,
                    "DegMinSec" => CoordFormat::DegMinSec,
                    _ => CoordFormat::Decimal,
                };

                app.status = format!(
                    "Сессия восстановлена ({} треков, {} точек)",
                    app.tracks.len(),
                    app.waypoints.len()
                );

                let task = app.request_osm_tiles();
                return (app, task);
            }
        }

        // ==========================================
        // 2. ЕСЛИ СЕССИИ НЕТ (ИЛИ ПЕРЕДАН --osm)
        // ==========================================
        let mut app = App {
            layers: Vec::new(),
            active: 0,
            grid: None,
            lines: Vec::new(),
            show_grid: true,
            show_labels: true,
            declination: 0.0,
            decl_offset: 0.0,
            format: CoordFormat::Decimal,
            cx: 0.0,
            cy: 0.0,
            scale: 1.0,
            canvas_cache: Cache::default(),
            grid_cache: Cache::default(),
            osm_canvas_cache: Cache::default(),
            tile_handles: RefCell::new(HashMap::new()),
            dragging: false,
            cursor: None,
            status: String::new(),
            last_geom_ms: Cell::new(0.0),
            last_bounds: Cell::new(Size::new(VIEW_W, VIEW_H)),
            osm_cache: osm::OsmCache::new(&cache_cfg),
            osm_client: osm::client(),
            osm_z: 0,
            tracks: Vec::new(),
            waypoints: Vec::new(),
            next_id: 0,
            tracks_cache: Cache::default(),
            selection: Selection::default(),
            // Новые поля
            last_kit: None,
            modifiers: iced::keyboard::Modifiers::default(),
            last_track_clicked: None,
            last_waypoint_clicked: None,
            search_tracks: String::new(),
            search_waypoints: String::new(),
            palette: load_palette(),
            modal: None,
            track_backup: None,
            last_track_click: None,
            show_color_picker: false,
        };

        // Определяем, какой комплект пробовать загрузить.
        let kit_path: Option<PathBuf> = if force_osm {
            None
        } else if let Some(d) = dir_arg {
            Some(PathBuf::from(d))
        } else {
            load_last_kit()
        };

        if let Some(p) = kit_path {
            if app.apply_kit(load_kit(&p)) {
                app.last_kit = Some(p.clone()); // <-- Сохраняем в состояние приложения
                save_last_kit(&p);
            }
        }

        // Если после этого OSM — задаём стартовый вид и запускаем тайлы.
        let task = if app.osm_active() && app.layers.is_empty() {
            let (lon, lat) = (37.6173, 55.7558); // Москва, z~10
            let (cx, cy) = merc_of(lon, lat);
            app.cx = cx;
            app.cy = cy;
            app.scale = 20.0;
            if app.status.is_empty() {
                app.status = "Источник: OpenStreetMap".into();
            } else {
                app.status = format!("OSM. {}", app.status);
            }
            app.request_osm_tiles()
        } else {
            Task::none()
        };

        (app, task)
    }

    fn fit_to_layer(&mut self, idx: usize) {
        let Some(l) = self.layers.get(idx) else { return };
        let base_idx = l.levels.first().map(|t| t.0).unwrap_or(0);
        let (w0, h0) = {
            let ozf_r = l.ozf.borrow();
            let b = &ozf_r.levels[base_idx];
            (b.width as f64, b.height as f64)
        };
        let (lon, lat) = l.georef.pixel_to_lonlat(w0 / 2.0, h0 / 2.0);
        let (cx, cy) = merc_of(lon, lat);
        let a = affine(&l.georef, cx, cy);
        let ppm = (a.ax * a.ax + a.ay * a.ay).sqrt();
        let sz = self.last_bounds.get();
        self.cx = cx;
        self.cy = cy;
        self.scale = 1.05 * ((w0 / ppm) / sz.width as f64).max((h0 / ppm) / sz.height as f64);
    }

    fn screen_to_lonlat(&self, p: Point) -> (f64, f64) {
        let sz = self.last_bounds.get();
        let mx = self.cx + (p.x as f64 - sz.width as f64 / 2.0) * self.scale;
        let my = self.cy - (p.y as f64 - sz.height as f64 / 2.0) * self.scale;
        lonlat_of(mx, my)
    }

    fn refresh_status(&mut self) {
        let Some(p) = self.cursor else { return };
        let (lon, lat) = self.screen_to_lonlat(p);
        let mut s = format!("Курсор: {}", format_pair(lat, lon, self.format));
        if let Some(g) = &self.grid {
            if let Some(n) = g.nearest(lat, lon, self.declination + self.decl_offset) {
                s += &format!(
                    "   |   ближайший узел {}: {:.0} м, магнитный азимут {:.0}°",
                    n.node.name, n.distance_m, n.magnetic_azimuth
                );
            }
        }
        self.status = s;
    }

    fn update_track_preview(&mut self, id: u64) {
        if let Some(Modal::TrackProps { draft, .. }) = &self.modal {
            if let Some(t) = self.tracks.iter_mut().find(|t| t.id == id) {
                t.props = draft.clone();
                // Синхронизируем имя и цвет для списка треков
                t.name = draft.description.clone();
                t.color = draft.line_color;
                self.tracks_cache.clear();
            }
        }
    }

    fn apply_kit(
        &mut self,
        result: Result<(Vec<Layer>, Option<GridData>, Vec<String>), String>,
    ) -> bool {
        match result {
            Ok((layers, gd, notes)) => {
                let n_layers = layers.len();
                let n_gpx = gd.as_ref().map_or(0, |(g, _)| g.nodes.len());
                self.layers = layers;
                self.grid = None;
                self.lines.clear();
                if let Some((g, lines)) = gd {
                    self.grid = Some(g);
                    self.lines = lines;
                }
                self.active = 0;
                self.status = if notes.is_empty() {
                    format!("слоёв: {n_layers}, точек GPX: {n_gpx}")
                } else {
                    format!("слоёв: {n_layers}, точек GPX: {n_gpx}; {}", notes.join("; "))
                };
                self.fit_to_layer(0);
                if let Some(g) = &self.grid {
                    let (lat, lon) = g.center();
                    let (y, ord) = today();
                    if let Ok(d) = declination::declination_deg(lat, lon, y, ord) {
                        self.declination = d;
                    }
                }
                self.tile_handles.borrow_mut().clear();
                self.canvas_cache.clear();
                self.grid_cache.clear();
                self.osm_canvas_cache.clear();
                self.tracks_cache.clear();
                true
            }
            Err(e) => {
                self.status = format!("Ошибка загрузки: {e}");
                false
            }
        }
    }
    
    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            // Отслеживаем зажатие Ctrl / Shift
            iced::event::listen_with(|event, _status, _id| {
                if let iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) = event {
                    Some(Message::ModifiersChanged(modifiers))
                } else {
                    None
                }
            }),
            // Отслеживаем закрытие окна (правильный способ в iced 0.14)
            iced::window::close_requests().map(|_| Message::WindowCloseRequested),
        ])
    }

    fn request_osm_tiles(&mut self) -> Task<Message> {
        if !self.osm_active() {
            return Task::none();
        }

        let sz = self.last_bounds.get();
        let (z, tiles) = osm::visible_tiles(
            self.cx,
            self.cy,
            self.scale,
            sz.width as f64,
            sz.height as f64,
        );
        self.osm_z = z;

        let mut tasks: Vec<Task<Message>> = Vec::new();
        for id in tiles {
            if self.osm_cache.get(id).is_some() {
                continue;
            }
            if self.osm_cache.pending.borrow().contains(&id) {
                continue;
            }
            if tasks.len() >= 2 {
                break;
            }
            self.osm_cache.pending.borrow_mut().insert(id);

            let client = self.osm_client.clone();
            let disk = self.osm_cache.disk_path(id);
            let url = format!(
                "https://tile-a.openstreetmap.fr/hot/{}/{}/{}.png",
                id.0, id.1, id.2
            );
            tasks.push(Task::perform(
                async move {
                    if let Ok(bytes) = std::fs::read(&disk) {
                        return (id, Ok(bytes));
                    }
                    let res = client.get(&url).send().await;
                    match res {
                        Ok(r) if r.status().is_success() => match r.bytes().await {
                            Ok(b) => {
                                let v = b.to_vec();
                                if let Some(parent) = disk.parent() {
                                    let _ = std::fs::create_dir_all(parent);
                                }
                                let _ = std::fs::write(&disk, &v);
                                (id, Ok(v))
                            }
                            Err(e) => (id, Err(e.to_string())),
                        },
                        Ok(r) => (id, Err(format!("HTTP {}", r.status()))),
                        Err(e) => (id, Err(e.to_string())),
                    }
                },
                |(id, result)| Message::OsmTileLoaded(id, result),
            ));
        }
        Task::batch(tasks)
    }

    fn update(&mut self, m: Message) -> Task<Message> {
        match m {
            Message::Layer(i) => {
                if i <= self.layers.len() {
                    self.active = i;
                    // НЕ вызываем fit_to_layer(i) — сохраняем текущие cx, cy и scale
                    self.canvas_cache.clear();
                    self.grid_cache.clear();
                    self.osm_canvas_cache.clear();
                    self.tracks_cache.clear();
                    self.refresh_status();
                }
            }
            Message::ToggleGrid => {
                self.show_grid = !self.show_grid;
                self.grid_cache.clear();
            }
            Message::ToggleLabels => {
                self.show_labels = !self.show_labels;
                self.grid_cache.clear();
            }
            Message::CycleFormat => {
                self.format = match self.format {
                    CoordFormat::Decimal => CoordFormat::DegMin,
                    CoordFormat::DegMin => CoordFormat::DegMinSec,
                    CoordFormat::DegMinSec => CoordFormat::Decimal,
                };
                self.refresh_status();
            }
            Message::Decl(d) => {
                self.decl_offset += d;
                self.refresh_status();
            }
            Message::DeclReset => {
                self.decl_offset = 0.0;
                self.refresh_status();
            }
            Message::Pressed => self.dragging = true,
            Message::Released => {
                if self.dragging {
                    self.dragging = false;
                    self.canvas_cache.clear();
                    self.grid_cache.clear();
                    self.osm_canvas_cache.clear();
                    self.tracks_cache.clear();
                    self.refresh_status();
                }
            }
            Message::Moved(p) => {
                if self.dragging {
                    if let Some(prev) = self.cursor {
                        self.cx -= (p.x - prev.x) as f64 * self.scale;
                        self.cy += (p.y - prev.y) as f64 * self.scale;
                        self.canvas_cache.clear();
                        self.grid_cache.clear();
                        self.osm_canvas_cache.clear();
                        self.tracks_cache.clear();
                    }
                    self.cursor = Some(p);
                } else {
                    self.cursor = Some(p);
                    self.refresh_status();
                }
            }
            Message::Scrolled(d) => {
                let steps = match d {
                    ScrollDelta::Lines { y, .. } => y as f64,
                    ScrollDelta::Pixels { y, .. } => y as f64 / 40.0,
                };
                let sz = self.last_bounds.get();
                let p = self
                    .cursor
                    .unwrap_or(Point::new(sz.width / 2.0, sz.height / 2.0));
                let (ox, oy) = (
                    p.x as f64 - sz.width as f64 / 2.0,
                    p.y as f64 - sz.height as f64 / 2.0,
                );
                let (mx, my) = (self.cx + ox * self.scale, self.cy - oy * self.scale);
                self.scale = (self.scale * 0.8f64.powf(steps)).clamp(0.05, 2000.0);
                self.cx = mx - ox * self.scale;
                self.cy = my + oy * self.scale;
                self.canvas_cache.clear();
                self.grid_cache.clear();
                self.osm_canvas_cache.clear();
                self.tracks_cache.clear();
                self.refresh_status();
            }
            Message::Fit => {
                if !self.osm_active() {
                    let idx = self.active;
                    self.fit_to_layer(idx);
                    self.canvas_cache.clear();
                    self.grid_cache.clear();
                    self.osm_canvas_cache.clear();
                    self.refresh_status();
                }
            }
            Message::PickKit => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .pick_folder()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    Message::KitPicked,
                );
            }
            Message::KitPicked(Some(path)) => {
                if self.apply_kit(load_kit(&path)) {
                    self.last_kit = Some(path.clone());
                    save_last_kit(&path);
                }
            }
            Message::KitPicked(None) => {}
            Message::OsmTileLoaded(id, result) => {
                self.osm_cache.unmark_pending(id);
                let handle = result
                    .ok()
                    .map(|bytes| iced::widget::image::Handle::from_bytes(bytes));
                self.osm_cache.insert(id, handle);
                self.osm_canvas_cache.clear();
            }
            Message::PickTrack => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .add_filter("Track/Points", &["gpx", "plt", "ms", "wpt"])
                            .pick_files()
                            .await
                    },
                    |files| Message::TracksPicked(
                        files.map(|fs| fs.into_iter().map(|f| f.path().to_path_buf()).collect())
                    ),
                );
            }
            Message::TracksPicked(Some(paths)) => {
                for path in paths {
                    match document::load_user_file(&path, &mut self.next_id) {
                        Ok(content) => {
                            self.tracks.extend(content.tracks);
                            self.waypoints.extend(content.waypoints);
                            let mut s = format!("Загружено: {}", path.file_name().unwrap_or_default().to_string_lossy());
                            if !content.notes.is_empty() {
                                s += &format!(" (предупреждения: {})", content.notes.join("; "));
                            }
                            self.status = s;
                        }
                        Err(e) => {
                            self.status = format!("Ошибка загрузки: {e}");
                        }
                    }
                }
                self.tracks_cache.clear();
            }
            Message::TracksPicked(None) => {}
                        Message::ToggleTrackVisible(id, v) => {
                if let Some(t) = self.tracks.iter_mut().find(|t| t.id == id) {
                    t.visible = v;
                    self.tracks_cache.clear();
                }
            }
            Message::ToggleAllTracksVisible => {
                let all_visible = self.tracks.iter().all(|t| t.visible);
                for t in &mut self.tracks {
                    t.visible = !all_visible;
                }
                self.tracks_cache.clear();
            }
            Message::ModifiersChanged(mods) => {
                self.modifiers = mods;
            }
            Message::SearchTracksChanged(q) => {
                self.search_tracks = q;
                self.tracks_cache.clear(); // если бы поиск влиял на отрисовку карты (пока нет)
            }
            Message::SearchWaypointsChanged(q) => {
                self.search_waypoints = q;
                self.tracks_cache.clear();
            }
            
            Message::SelectTrack(id) => {
                let now = std::time::Instant::now();
                let is_double_click = self.last_track_click.map_or(false, |(last_id, last_time)| {
                    last_id == id && now.duration_since(last_time).as_millis() < 300
                });

                if is_double_click {
                    self.last_track_click = None;
                    if let Some(t) = self.tracks.iter().find(|t| t.id == id) {
                        self.track_backup = Some(t.clone());
                        self.modal = Some(Modal::TrackProps { track_id: id, draft: t.props.clone() });
                    }
                } else {
                    self.last_track_click = Some((id, now));

                    let ctrl = self.modifiers.control() || self.modifiers.command();
                    let shift = self.modifiers.shift();

                    if shift {
                        if let Some(last_id) = self.last_track_clicked {
                            if let (Some(start), Some(end)) = (
                                self.tracks.iter().position(|t| t.id == last_id),
                                self.tracks.iter().position(|t| t.id == id),
                            ) {
                                let (min, max) = if start < end { (start, end) } else { (end, start) };
                                for i in min..=max {
                                    self.selection.tracks.insert(self.tracks[i].id);
                                }
                            }
                        } else {
                            self.selection.tracks.insert(id);
                        }
                    } else if ctrl {
                        if !self.selection.tracks.remove(&id) {
                            self.selection.tracks.insert(id);
                        }
                    } else {
                        // ОБЫЧНЫЙ КЛИК: сбрасываем ВСЁ и выбираем только этот трек
                        self.selection.tracks.clear();
                        self.selection.waypoints.clear(); // <--- Вот эта строка должна быть здесь
                        self.selection.tracks.insert(id);
                    }
                    self.last_track_clicked = Some(id);
                }
            }
            // Аналогично для SelectWaypoint:
            Message::SelectWaypoint(id) => {
                let ctrl = self.modifiers.control() || self.modifiers.command();
                let shift = self.modifiers.shift();

                if shift {
                    if let Some(last_id) = self.last_waypoint_clicked {
                        if let (Some(start), Some(end)) = (
                            self.waypoints.iter().position(|w| w.id == last_id),
                            self.waypoints.iter().position(|w| w.id == id),
                        ) {
                            let (min, max) = if start < end { (start, end) } else { (end, start) };
                            for i in min..=max {
                                self.selection.waypoints.insert(self.waypoints[i].id);
                            }
                        }
                    } else {
                        self.selection.waypoints.insert(id);
                    }
                } else if ctrl {
                    if !self.selection.waypoints.remove(&id) {
                        self.selection.waypoints.insert(id);
                    }
                } else {
                    // ОБЫЧНЫЙ КЛИК: сбрасываем ВСЁ и выбираем только эту точку
                    self.selection.waypoints.clear();
                    self.selection.tracks.clear(); // <--- Вот эта строка должна быть здесь
                    self.selection.waypoints.insert(id);
                }
                self.last_waypoint_clicked = Some(id);
            }
            Message::DeleteSelectedTracks => {
                let ids = self.selection.tracks.clone();
                self.tracks.retain(|t| !ids.contains(&t.id));
                self.selection.tracks.clear();
                self.selection.waypoints.clear();
                self.tracks_cache.clear();
            }
            Message::SaveTracksPlt => {
                let mut dialog = rfd::AsyncFileDialog::new()
                    .set_file_name("track.plt")
                    .add_filter("OziExplorer Track", &["plt"]);
                if let Some(kit) = &self.last_kit {
                    let dir = kit.join("10-Tracks");
                    if dir.is_dir() { dialog = dialog.set_directory(&dir); } else { dialog = dialog.set_directory(kit); }
                }
                return Task::perform(async move { dialog.save_file().await.map(|h| h.path().to_path_buf()) }, Message::SaveTracksPltPicked);
            }
            Message::SaveTracksPltPicked(Some(path)) => {
                let sel_tracks: Vec<_> = self.tracks.iter().filter(|t| self.selection.tracks.contains(&t.id)).collect();
                let sel_wps: Vec<_> = self.waypoints.iter().filter(|w| self.selection.waypoints.contains(&w.id)).collect();
                if let Err(e) = document::save_any(&sel_tracks, &sel_wps, &path) {
                    self.status = format!("Ошибка: {e}");
                } else {
                    self.status = format!("Сохранено в {}", path.display());
                }
            }
            Message::SaveTracksPltPicked(None) => {}

            Message::SaveTracksGpx => {
                let mut dialog = rfd::AsyncFileDialog::new()
                    .set_file_name("tracks.gpx")
                    .add_filter("GPX", &["gpx"]);
                if let Some(kit) = &self.last_kit {
                    let dir = kit.join("10-Tracks");
                    if dir.is_dir() { dialog = dialog.set_directory(&dir); } else { dialog = dialog.set_directory(kit); }
                }
                return Task::perform(async move { dialog.save_file().await.map(|h| h.path().to_path_buf()) }, Message::SaveTracksGpxPicked);
            }
            Message::SaveTracksGpxPicked(Some(path)) => {
                let sel_tracks: Vec<_> = self.tracks.iter().filter(|t| self.selection.tracks.contains(&t.id)).collect();
                let sel_wps: Vec<_> = self.waypoints.iter().filter(|w| self.selection.waypoints.contains(&w.id)).collect();
                if let Err(e) = document::save_any(&sel_tracks, &sel_wps, &path) {
                    self.status = format!("Ошибка: {e}");
                } else {
                    self.status = format!("Сохранено в {}", path.display());
                }
            }
            Message::SaveTracksGpxPicked(None) => {}

            Message::SaveTracksMs => {
                let mut dialog = rfd::AsyncFileDialog::new()
                    .set_file_name("tracks.ms")
                    .add_filter("Guru Maps", &["ms"]);
                if let Some(kit) = &self.last_kit {
                    let dir = kit.join("10-Tracks");
                    if dir.is_dir() { dialog = dialog.set_directory(&dir); } else { dialog = dialog.set_directory(kit); }
                }
                return Task::perform(async move { dialog.save_file().await.map(|h| h.path().to_path_buf()) }, Message::SaveTracksMsPicked);
            }
            Message::SaveTracksMsPicked(Some(path)) => {
                let sel_tracks: Vec<_> = self.tracks.iter().filter(|t| self.selection.tracks.contains(&t.id)).collect();
                let sel_wps: Vec<_> = self.waypoints.iter().filter(|w| self.selection.waypoints.contains(&w.id)).collect();
                if let Err(e) = document::save_any(&sel_tracks, &sel_wps, &path) {
                    self.status = format!("Ошибка: {e}");
                } else {
                    self.status = format!("Сохранено в {}", path.display());
                }
            }
            Message::SaveTracksMsPicked(None) => {}
            Message::ToggleWaypointVisible(id, v) => {
                if let Some(w) = self.waypoints.iter_mut().find(|w| w.id == id) {
                    w.visible = v;
                    self.tracks_cache.clear();
                }
            }
            Message::ToggleAllWaypointsVisible => {
                let all_visible = self.waypoints.iter().all(|w| w.visible);
                for w in &mut self.waypoints {
                    w.visible = !all_visible;
                }
                self.tracks_cache.clear();
            }
            Message::DeleteSelectedWaypoints => {
                let ids = self.selection.waypoints.clone();
                self.waypoints.retain(|w| !ids.contains(&w.id));
                self.selection.waypoints.clear();
                self.selection.tracks.clear();
                self.tracks_cache.clear();
            }
            Message::SaveWaypointsWptNew => {
                // Формируем имя: Waypoints_YYYYMMDD.wpt
                let date_str = date_offset(0).replace("-", "");
                let default_name = format!("Waypoints_{}.wpt", date_str);

                let mut dialog = rfd::AsyncFileDialog::new()
                    .set_file_name(&default_name) // Используем сформированное имя
                    .add_filter("OziExplorer Waypoints", &["wpt"]);
                if let Some(kit) = &self.last_kit {
                    let dir = kit.join("10-Tracks");
                    if dir.is_dir() { dialog = dialog.set_directory(&dir); } else { dialog = dialog.set_directory(kit); }
                }
                return Task::perform(
                    async move { dialog.save_file().await.map(|h| h.path().to_path_buf()) }, 
                    Message::SaveWaypointsWptNewPicked
                );
            }
            Message::SaveWaypointsWptNewPicked(Some(path)) => {
                let sel_wps: Vec<_> = self.waypoints.iter().filter(|w| self.selection.waypoints.contains(&w.id)).collect();
                if let Err(e) = document::save_any(&[], &sel_wps, &path) {
                    self.status = format!("Ошибка: {e}");
                } else {
                    self.status = format!("Сохранено в {}", path.display());
                }
            }
            Message::SaveWaypointsWptNewPicked(None) => {}

            Message::SaveWaypointsWptAppend => {
                let mut dialog = rfd::AsyncFileDialog::new()
                    .add_filter("OziExplorer Waypoints", &["wpt"]);
                if let Some(kit) = &self.last_kit {
                    let dir = kit.join("10-Tracks");
                    if dir.is_dir() { dialog = dialog.set_directory(&dir); } else { dialog = dialog.set_directory(kit); }
                }
                return Task::perform(async move { dialog.pick_file().await.map(|h| h.path().to_path_buf()) }, Message::SaveWaypointsWptAppendPicked);
            }
            Message::SaveWaypointsWptAppendPicked(Some(path)) => {
                let mut all_wps = Vec::new();
                // 1. Читаем то, что уже есть в файле (используем наш же загрузчик)
                if let Ok(content) = document::load_user_file(&path, &mut self.next_id) {
                    all_wps.extend(content.waypoints);
                }
                // 2. Добавляем выделенные точки
                for w in &self.waypoints {
                    if self.selection.waypoints.contains(&w.id) {
                        all_wps.push(w.clone());
                    }
                }
                // 3. Сохраняем обратно
                let refs: Vec<_> = all_wps.iter().collect();
                if let Err(e) = document::save_any(&[], &refs, &path) {
                    self.status = format!("Ошибка: {e}");
                } else {
                    self.status = format!("Добавлено в {}", path.display());
                }
            }
            Message::SaveWaypointsWptAppendPicked(None) => {}
            Message::WindowCloseRequested => {
                eprintln!("DEBUG: Получено событие закрытия окна");
                save_session(self);
                return iced::exit();
            }
            Message::OpenTrackProps(id) => {
                if let Some(t) = self.tracks.iter().find(|t| t.id == id) {
                    self.track_backup = Some(t.clone()); // Сохраняем для отмены
                    self.modal = Some(Modal::TrackProps { track_id: id, draft: t.props.clone() });
                }
            }
            Message::CloseTrackProps => {
                if let Some(backup) = self.track_backup.take() {
                    if let Some(t) = self.tracks.iter_mut().find(|t| t.id == backup.id) {
                        *t = backup; // Откатываем изменения
                        self.tracks_cache.clear();
                    }
                }
                self.modal = None;
            }
            // При изменении полей:
            Message::SaveTrackProps => {
                if let Some(Modal::TrackProps { draft, .. }) = &self.modal {
                    let default_name = format!("{}.plt", draft.description); 
                    let mut dialog = rfd::AsyncFileDialog::new()
                        .set_file_name(&default_name)
                        .add_filter("OziExplorer Track", &["plt"]);
                    
                    // Устанавливаем папку 10-Tracks из комплекта карт
                    if let Some(kit) = &self.last_kit {
                        let tracks_dir = kit.join("10-Tracks");
                        if tracks_dir.is_dir() {
                            dialog = dialog.set_directory(&tracks_dir);
                        } else if kit.is_dir() {
                            dialog = dialog.set_directory(kit);
                        }
                    }

                    return Task::perform(
                        async move { dialog.save_file().await.map(|h| h.path().to_path_buf()) },
                        Message::SaveTrackPropsPicked,
                    );
                }
            }
            // Drafts 
            Message::DraftDescChanged(s) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    draft.description = s;
                    tid = Some(*track_id);
                }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftDateChanged(s) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    draft.date_str = s;
                    draft.description = build_template(draft);
                    tid = Some(*track_id);
                }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftCallsignChanged(cs) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    draft.callsign = cs.clone();
                    draft.line_color = match cs.as_str() {
                        "Veter" => [255, 0, 255],
                        "BPLA" => [0, 255, 255],
                        "Bort" => [255, 255, 255],
                        "Zadacha" => [0, 0, 0],
                        _ => draft.line_color,
                    };
                    draft.line_width = if cs == "BPLA" { 1.0 } else { 2.0 };
                    
                    // Проверка на запрещенные цвета
                    let is_lisa_like = matches!(cs.as_str(), "Lisa" | "Avtonom" | "Kinolog" | "Pegas");
                    let forbidden = [[0, 255, 255], [255, 0, 255], [255, 255, 255], [0, 0, 0]];
                    if is_lisa_like && forbidden.contains(&draft.line_color) {
                        draft.line_color = [255, 0, 0]; // Принудительно красный
                    }

                    draft.description = build_template(draft);
                    tid = Some(*track_id);
                }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftNum1Changed(s) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    if let Ok(n) = s.parse::<u32>() { draft.num1 = n; draft.description = build_template(draft); tid = Some(*track_id); }
                }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftNum2Changed(s) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    draft.num2 = s.parse::<u32>().ok(); draft.description = build_template(draft); tid = Some(*track_id);
                }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftSuffixChanged(s) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    draft.suffix = s; draft.description = build_template(draft); tid = Some(*track_id);
                }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftDateToday => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    draft.date_str = date_offset(0); draft.description = build_template(draft); tid = Some(*track_id);
                }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftDateYesterday => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    draft.date_str = date_offset(-1); draft.description = build_template(draft); tid = Some(*track_id);
                }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftColorChanged(c) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    let is_lisa_like = matches!(draft.callsign.as_str(), "Lisa" | "Avtonom" | "Kinolog" | "Pegas");
                    let forbidden = [[0, 255, 255], [255, 0, 255], [255, 255, 255], [0, 0, 0]];
                    
                    if is_lisa_like && forbidden.contains(&c) {
                        draft.line_color = [255, 0, 0]; // Принудительно красный
                    } else {
                        draft.line_color = c;
                    }
                    tid = Some(*track_id);
                }
                self.show_color_picker = false;
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::ToggleColorPicker => {
                self.show_color_picker = !self.show_color_picker;
            }
            Message::ColorChannelChanged(ch, val) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    let mut new_c = draft.line_color;
                    new_c[ch] = val;
                    
                    let is_lisa_like = matches!(draft.callsign.as_str(), "Lisa" | "Avtonom" | "Kinolog" | "Pegas");
                    let forbidden = [[0, 255, 255], [255, 0, 255], [255, 255, 255], [0, 0, 0]];
                    if is_lisa_like && forbidden.contains(&new_c) {
                        draft.line_color = [255, 0, 0]; // Принудительно красный
                    } else {
                        draft.line_color = new_c;
                    }
                    tid = Some(*track_id);
                }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftFillColorChanged(c) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal { draft.fill_color = c; tid = Some(*track_id); }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftWidthChanged(s) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    if let Ok(w) = s.parse::<f32>() { draft.line_width = w; tid = Some(*track_id); }
                }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftStyleChanged(s) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal { draft.line_style = s; tid = Some(*track_id); }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftTypeChanged(t) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal { draft.poly_type = t; tid = Some(*track_id); }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftFillTypeChanged(t) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal { draft.fill_type = t; tid = Some(*track_id); }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::ApplyTrackProps => {
                // Применяем и закрываем без сохранения в файл
                self.track_backup = None; // Сбрасываем бэкап, чтобы Close не откатил изменения
                self.modal = None;
            }
            Message::SaveTrackPropsPicked(Some(path)) => {
                if let Some(Modal::TrackProps { track_id, .. }) = &self.modal {
                    if let Some(t) = self.tracks.iter().find(|t| t.id == *track_id) {
                        if let Err(e) = document::save_plt(t, &path) {
                            self.status = format!("Ошибка сохранения: {e}");
                        } else {
                            self.status = format!("Сохранено в {}", path.display());
                            if let Some(t_mut) = self.tracks.iter_mut().find(|t| t.id == *track_id) {
                                t_mut.source = document::FileSource::File { 
                                    path: path.clone(), 
                                    read_only: false 
                                };
                                t_mut.name = t_mut.props.description.clone(); // обновляем имя
                            }
                            self.modal = None;
                            self.track_backup = None;
                        }
                    }
                }
            }            
            Message::SaveTrackPropsPicked(None) => {}
        }
        

        // Догружаем очередную пачку тайлов OSM, если он активен.
        self.request_osm_tiles()
    }

    fn format_name(&self) -> &'static str {
        match self.format {
            CoordFormat::Decimal => "dd.dddddd",
            CoordFormat::DegMin => "dd°mm.mmmm′",
            CoordFormat::DegMinSec => "dd°mm′ss.ss″",
        }
    }

    fn view(&self) -> Element<'_, Message> {
        // 1. Формируем базовый интерфейс (как было до модального окна)
        let top = self.view_top_toolbar();
        let palette = self.view_tool_palette();
        let map = self.view_map();
        let right = self.view_right_panel();
        let status = text(self.status.as_str());

        let base_ui = column![
            top,
            row![
                container(palette).width(Length::Fixed(56.0)).height(Length::Fill),
                container(map).width(Length::Fill).height(Length::Fill),
                container(right).width(Length::Fixed(340.0)).height(Length::Fill),
            ]
            .spacing(4)
            .height(Length::Fill),
            status,
        ]
        .spacing(4)
        .padding(4)
        .width(Length::Fill)
        .height(Length::Fill);

        // 2. Если открыто модальное окно — накладываем его сверху
        if let Some(Modal::TrackProps { track_id, draft }) = &self.modal {
            let modal_ui = self.view_track_props_modal(*track_id, draft);
            
            stack![
                base_ui,
                // Затемняющий фон
                container(text(""))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.6).into()),
                        ..Default::default()
                    }),
                // Само окно по центру
                container(modal_ui)
                    .center_x(Length::Fill)  // В iced 0.14 center_x требует Length
                    .center_y(Length::Fill)
            ]
            .into()
        } else {
            base_ui.into()
        }
    }

    fn view_top_toolbar(&self) -> Element<'_, Message> {
        let source_label = if self.osm_active() {
            "Карта: OSM".to_string()
        } else {
            self.layers
                .get(self.active)
                .map(|l| format!("Карта: {}", l.name))
                .unwrap_or_else(|| "Карта: —".into())
        };

        row![
            button(text("Открыть")).on_press(Message::PickKit),
            button(text("Открыть трек")).on_press(Message::PickTrack),
            text("|"),
            text(source_label).size(13),
            text("|"),
            button(text(if self.show_grid { "Сетка: вкл" } else { "Сетка: выкл" }))
                .on_press(Message::ToggleGrid),
            button(text(if self.show_labels { "Подписи: вкл" } else { "Подписи: выкл" }))
                .on_press(Message::ToggleLabels),
            button(text("Fit")).on_press(Message::Fit),
            button(text(self.format_name())).on_press(Message::CycleFormat),
            button(text("Скл −0.1°")).on_press(Message::Decl(-0.1)),
            button(text("Скл +0.1°")).on_press(Message::Decl(0.1)),
            button(text("Скл: сброс")).on_press(Message::DeclReset),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
    }

    fn view_tool_palette(&self) -> Element<'_, Message> {
        column![
            button(text("✋")).on_press(Message::Pressed),
            button(text("▲")).on_press(Message::Pressed),
            button(text("⊕")).on_press(Message::Pressed),
            button(text("●")).on_press(Message::Pressed),
            button(text("✎")).on_press(Message::Pressed),
            button(text("✕")).on_press(Message::Pressed),
        ]
        .spacing(4)
        .width(Length::Fill)
        .into()
    }

    fn view_map(&self) -> Element<'_, Message> {
        let map_canvas = Canvas::new(MapProgram { app: self })
            .width(Length::Fill)
            .height(Length::Fill);
        let tracks_canvas = Canvas::new(TrackProgram { app: self })
            .width(Length::Fill)
            .height(Length::Fill);
        let grid_canvas = Canvas::new(GridProgram { app: self })
            .width(Length::Fill)
            .height(Length::Fill);

        mouse_area(stack![map_canvas, tracks_canvas, grid_canvas])
            .on_press(Message::Pressed)
            .on_release(Message::Released)
            .on_move(Message::Moved)
            .on_scroll(Message::Scrolled)
            .into()
    }

    fn view_right_panel(&self) -> Element<'_, Message> {
        column![
            self.view_maps_section(),
            self.view_tracks_section(),
            self.view_waypoints_section(),
        ]
        .spacing(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_maps_section(&self) -> Element<'_, Message> {
        let kit_name = self.layers.first().map(|l| l.kit_name.as_str()).unwrap_or("");
        
        let header = row![
            text(format!("Maps ({})", self.layers.len() + 1)).size(14),
            Space::new().width(Length::Fill),
            if !kit_name.is_empty() {
                text(kit_name).size(11).color(Color::from_rgb(0.6, 0.6, 0.6))
            } else {
                text("").size(11)
            }
        ]
        .align_y(Alignment::Center);

        let mut list = column![].spacing(2).padding([0.0, 12.0]); // отступ 12px справа и слева
        for (i, l) in self.layers.iter().enumerate() {
            let marker = if self.active == i { "◉" } else { "○" };
            list = list.push(
                button(text(format!("{marker} {}", l.name)))
                    .on_press(Message::Layer(i))
                    .width(Length::Fill),
            );
        }
        let osm_idx = self.layers.len();
        let osm_marker = if self.osm_active() { "◉" } else { "○" };
        list = list.push(
            button(text(format!("{osm_marker} OSM (HOT)")))
                .on_press(Message::Layer(osm_idx))
                .width(Length::Fill),
        );

        column![
            header,
            container(scrollable(list).height(Length::Fixed(150.0))).padding(2), // ограничили 150px
        ]
        .spacing(4)
        .width(Length::Fill)
        .into()
    }

    fn view_tracks_section(&self) -> Element<'_, Message> {
        let header = text(format!("Tracks ({})", self.tracks.len())).size(14);

        let all_visible = self.tracks.iter().all(|t| t.visible);
        let top_bar = row![
            button(text(if all_visible { "👁 Скрыть все" } else { "👁 Показать все" }))
                .on_press(Message::ToggleAllTracksVisible),
            iced::widget::text_input("Поиск треков...", &self.search_tracks)
                .on_input(Message::SearchTracksChanged)
                .padding(4)
                .size(12),
        ]
        .spacing(4);

        let cols = row![
            Space::new().width(16.0),
            Space::new().width(24.0),
            Space::new().width(16.0),
            container(text("Имя").size(11)).width(Length::Fill),
            container(text("Точек").size(11))
                .width(Length::Fixed(50.0))
                .align_x(Alignment::End),
            container(text("Расст.").size(11))
                .width(Length::Fixed(70.0))
                .align_x(Alignment::End),
        ]
        .spacing(4);

        let query = self.search_tracks.to_lowercase();
        let mut list = column![].spacing(2).padding([0.0, 12.0]);
        
        for t in &self.tracks {
            if !query.is_empty() && !t.name.to_lowercase().contains(&query) {
                continue;
            }

            let selected = self.selection.tracks.contains(&t.id);
            
            let sel_dot = container(
                text(if selected { "●" } else { "" }).size(12).color(Color::WHITE)
            ).width(16);

            let color_box = container(text(""))
                .width(12)
                .height(12)
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(Color::from_rgb8(
                        t.color[0], t.color[1], t.color[2],
                    ))),
                    border: iced::Border {
                        color: Color::from_rgb8(60, 60, 60),
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                });

            let dist = if t.cached_distance_m >= 1000.0 {
                format!("{:.1}км", t.cached_distance_m / 1000.0)
            } else {
                format!("{:.0}м", t.cached_distance_m)
            };

            let name_text = if selected {
                format!("● {}", t.name)
            } else {
                t.name.clone()
            };

            let r = row![
                sel_dot,
                checkbox(t.visible)
                    .on_toggle(move |v| Message::ToggleTrackVisible(t.id, v)),
                color_box,
                container(
                    button(
                        text(name_text).wrapping(iced::widget::text::Wrapping::None)
                    )
                    .on_press(Message::SelectTrack(t.id))
                    .width(Length::Fill)
                    .padding(0)
                )
                .width(Length::Fill)
                .clip(true),
                container(text(t.cached_points.to_string()).size(12))
                    .width(Length::Fixed(50.0))
                    .align_x(Alignment::End),
                container(text(dist).size(12))
                    .width(Length::Fixed(70.0))
                    .align_x(Alignment::End),
            ]
            .spacing(4)
            .align_y(Alignment::Center);
            
            list = list.push(r);
        }

        let has_tracks = !self.selection.tracks.is_empty();
        let has_wps = !self.selection.waypoints.is_empty();
        let one_track = self.selection.tracks.len() == 1;
        let selected_id = self.selection.tracks.iter().next().copied();

        let btn_props = if one_track {
            button(text("⚙")).on_press(Message::OpenTrackProps(selected_id.unwrap()))
        } else {
            button(text("⚙"))
        };

        let btn_del = if has_tracks {
            button(text("🗑")).on_press(Message::DeleteSelectedTracks)
        } else {
            button(text("🗑"))
        };

        let btn_plt = if has_tracks && !has_wps {
            button(text("PLT")).on_press(Message::SaveTracksPlt)
        } else {
            button(text("PLT"))
        };

        let btn_gpx = if has_tracks || has_wps {
            button(text("GPX")).on_press(Message::SaveTracksGpx)
        } else {
            button(text("GPX"))
        };

        let btn_ms = if has_tracks || has_wps {
            button(text("MS")).on_press(Message::SaveTracksMs)
        } else {
            button(text("MS"))
        };

        let bottom_bar = row![
            btn_props,
            btn_del,
            btn_plt,
            btn_gpx,
            btn_ms,
        ]
        .spacing(4)
        .align_y(Alignment::Center);

        column![
            header,
            container(top_bar).padding(2),
            container(cols).padding(2),
            container(scrollable(list).height(Length::FillPortion(1))).padding(2),
            container(bottom_bar).padding(2),
        ]
        .spacing(4)
        .width(Length::Fill)
        .height(Length::FillPortion(1))
        .into()
    }

    fn view_waypoints_section(&self) -> Element<'_, Message> {
        let header = text(format!("Waypoints ({})", self.waypoints.len())).size(14);

        let all_visible = self.waypoints.iter().all(|w| w.visible);
        let top_bar = row![
            button(text(if all_visible { "👁 Скрыть все" } else { "👁 Показать все" }))
                .on_press(Message::ToggleAllWaypointsVisible),
            iced::widget::text_input("Поиск точек...", &self.search_waypoints)
                .on_input(Message::SearchWaypointsChanged)
                .padding(4)
                .size(12),
        ]
        .spacing(4);

        let cols = row![
            Space::new().width(16.0),
            Space::new().width(24.0),
            Space::new().width(14.0),
            container(text("Имя").size(11)).width(Length::Fill),
            container(text("Координаты").size(11))
                .width(Length::Fixed(120.0))
                .align_x(Alignment::End),
        ]
        .spacing(4);

        let query = self.search_waypoints.to_lowercase();
        let mut list = column![].spacing(2).padding([0.0, 12.0]);

        for w in &self.waypoints {
            if !query.is_empty() && !w.name.to_lowercase().contains(&query) {
                continue;
            }

            let selected = self.selection.waypoints.contains(&w.id);
            let sel_dot = container(
                text(if selected { "●" } else { "" }).size(12).color(Color::WHITE)
            ).width(16);

            let icon = container(
                text("●")
                    .size(11)
                    .color(Color::from_rgb8(255, 200, 0)),
            ).width(14);
            
            let coord = coord::format_pair(w.lat, w.lon, self.format);

            let r = row![
                sel_dot,
                checkbox(w.visible)
                    .on_toggle(move |v| Message::ToggleWaypointVisible(w.id, v)),
                icon,
                container(
                    button(
                        text(w.name.clone()).wrapping(iced::widget::text::Wrapping::None)
                    )
                    .on_press(Message::SelectWaypoint(w.id))
                    .width(Length::Fill)
                    .padding(0)
                )
                .width(Length::Fill)
                .clip(true),
                container(text(coord).size(11).font(iced::Font::MONOSPACE))
                    .width(Length::Fixed(120.0))
                    .align_x(Alignment::End),
            ]
            .spacing(4)
            .align_y(Alignment::Center);
            
            list = list.push(r);
        }

        let has_wps = !self.selection.waypoints.is_empty();
        let has_tracks = !self.selection.tracks.is_empty();

        let btn_props = button(text("⚙"));

        let btn_del = if has_wps {
            button(text("🗑")).on_press(Message::DeleteSelectedWaypoints)
        } else {
            button(text("🗑"))
        };

        let wpt_enabled = has_wps && !has_tracks;

        let btn_wpt_new = if wpt_enabled {
            button(text("WPT")).on_press(Message::SaveWaypointsWptNew)
        } else {
            button(text("WPT"))
        };

        let btn_wpt_append = if wpt_enabled {
            button(text("WPT+")).on_press(Message::SaveWaypointsWptAppend)
        } else {
            button(text("WPT+"))
        };

        let mixed_enabled = has_wps || has_tracks;

        let btn_gpx = if mixed_enabled {
            button(text("GPX")).on_press(Message::SaveTracksGpx)
        } else {
            button(text("GPX"))
        };

        let btn_ms = if mixed_enabled {
            button(text("MS")).on_press(Message::SaveTracksMs)
        } else {
            button(text("MS"))
        };

        let bottom_bar = row![
            btn_props,
            btn_del,
            btn_wpt_new,
            btn_wpt_append,
            btn_gpx,
            btn_ms,
        ]
        .spacing(4)
        .align_y(Alignment::Center);

        column![
            header,
            container(top_bar).padding(2),
            container(cols).padding(2),
            container(scrollable(list).height(Length::FillPortion(1))).padding(2),
            container(bottom_bar).padding(2),
        ]
        .spacing(4)
        .width(Length::Fill)
        .height(Length::FillPortion(1))
        .into()
    }
    fn view_track_props_modal<'a>(&'a self, _id: u64, draft: &'a document::TrackProps) -> Element<'a, Message> {
        let callsigns = vec![
            "Lisa".to_string(), "Veter".to_string(), "Kinolog".to_string(), 
            "BPLA".to_string(), "Bort".to_string(), "Zadacha".to_string(), 
            "Avtonom".to_string(), "Pegas".to_string()
        ];
        
        let line_styles = vec![
            document::LineStyle::Solid, document::LineStyle::Dash, document::LineStyle::Dot, 
            document::LineStyle::DashDot, document::LineStyle::DashDotDot
        ];
        let poly_types = vec![document::PolyType::Line, document::PolyType::Polygon];
        let fill_types = vec![
            document::FillType::None, document::FillType::Diagonal1, document::FillType::Diagonal2, 
            document::FillType::CrossHatch, document::FillType::Solid
        ];

        let template = build_template(draft);

        // Блок полей для #_# и Суффикса (блокируются для Задачи)
        let extra_fields: Element<'a, Message> = if draft.callsign == "Zadacha" {
            row![].into()
        } else {
            row![
                container(text("#_#")).padding([0, 4]),
                text_input("", &draft.num2.map(|n| n.to_string()).unwrap_or_default())
                    .on_input(Message::DraftNum2Changed).width(50),
                container(text("Суффикс")).padding([0, 4]),
                text_input("", &draft.suffix)
                    .on_input(Message::DraftSuffixChanged).width(80),
            ].align_y(Alignment::Center).spacing(4).into()
        };

        // Палитра цветов (Line)
        let mut color_btns: Vec<Element<'_, Message>> = Vec::new();
        for &c in &self.palette {
            color_btns.push(
                button(text("")).width(20).height(20)
                    .style(move |_: &Theme, _| button::Style {
                        background: Some(iced::Background::Color(Color::from_rgb8(c[0], c[1], c[2]))),
                        border: iced::Border { color: Color::from_rgb(0.5, 0.5, 0.5), width: 1.0, radius: 2.0.into() },
                        ..Default::default()
                    })
                    .on_press(Message::DraftColorChanged(c))
                    .into()
            );
        }
        // Кнопка вызова пикера
        color_btns.push(
            button(text("↓")).width(20).height(20)
                .on_press(Message::ToggleColorPicker)
                .into()
        );
        let color_row = row(color_btns).spacing(2);

        // Палитра цветов (Fill)
        let mut fill_color_btns: Vec<Element<'_, Message>> = Vec::new();
        for &c in &self.palette {
            fill_color_btns.push(
                button(text("")).width(20).height(20)
                    .style(move |_: &Theme, _| button::Style {
                        background: Some(iced::Background::Color(Color::from_rgb8(c[0], c[1], c[2]))),
                        border: iced::Border { color: Color::from_rgb(0.5, 0.5, 0.5), width: 1.0, radius: 2.0.into() },
                        ..Default::default()
                    })
                    .on_press(Message::DraftFillColorChanged(c))
                    .into()
            );
        }
        let fill_color_row = row(fill_color_btns).spacing(2);
        // Блок полей заливки (появляется только для Polygon)
        let fill_fields: Element<'a, Message> = if draft.poly_type == document::PolyType::Polygon {
            column![
                row![text("Fill Color:").width(120), fill_color_row].align_y(Alignment::Center),
                row![
                    text("Fill Type:").width(120),
                    pick_list(fill_types, Some(draft.fill_type.clone()), Message::DraftFillTypeChanged),
                ].align_y(Alignment::Center),
            ].spacing(10).into()
        } else {
            column![].into()
        };
        // Блок пикера цветов
        let color_picker_ui: Element<'a, Message> = if self.show_color_picker {
            let ext_palette = get_extended_palette();
            let mut grid_rows: Vec<Element<'a, Message>> = Vec::new();
            
            // Разбиваем по 8 цветов в ряд
            for chunk in ext_palette.chunks(8) {
                let mut row_btns: Vec<Element<'a, Message>> = Vec::new();
                for &c in chunk {
                    row_btns.push(
                        button(text("")).width(24).height(24)
                            .style(move |_: &Theme, _| button::Style {
                                background: Some(iced::Background::Color(Color::from_rgb8(c[0], c[1], c[2]))),
                                border: iced::Border { color: Color::from_rgb(0.3, 0.3, 0.3), width: 1.0, radius: 2.0.into() },
                                ..Default::default()
                            })
                            .on_press(Message::DraftColorChanged(c))
                            .into()
                    );
                }
                grid_rows.push(row(row_btns).spacing(2).into());
            }
            
            let grid_ui = column(grid_rows).spacing(2);
            container(grid_ui).padding([0, 120]).into()
        } else {
            column![].into()
        };
        let content = column![
            text("Свойства трека").size(20).color(Color::WHITE),
            
            // Description
            row![
                text("Description:").width(120),
                text_input("Введите описание...", &draft.description).on_input(Message::DraftDescChanged).width(280)
            ].align_y(Alignment::Center),

            // Дата (без Завтра, с ручным вводом)
            row![
                text("Дата:").width(120),
                button("Вчера").on_press(Message::DraftDateYesterday),
                button("Сегодня").on_press(Message::DraftDateToday),
                text_input("YYYY-MM-DD", &draft.date_str).on_input(Message::DraftDateChanged).width(120),
            ].align_y(Alignment::Center).spacing(4),

            // Позывной и Номера
            row![
                text("Позывной:").width(120),
                pick_list(callsigns, Some(draft.callsign.clone()), Message::DraftCallsignChanged),
                container(text("#")).padding([0, 4]),
                text_input("", &draft.num1.to_string()).on_input(Message::DraftNum1Changed).width(50),
                extra_fields
            ].align_y(Alignment::Center).spacing(4),

            // Line Color
            row![text("Line Color:").width(120), color_row].align_y(Alignment::Center),

            // Встроенный RGB-пикер
            color_picker_ui,

            // Width & Line Style
            row![
                text("Width:").width(120),
                text_input("", &draft.line_width.to_string()).on_input(Message::DraftWidthChanged).width(50),
                container(text("Line Style:")).padding([0, 10]),
                pick_list(line_styles, Some(draft.line_style.clone()), Message::DraftStyleChanged),
            ].align_y(Alignment::Center),

            // Type (Line/Polygon)
            row![
                text("Type:").width(120),
                pick_list(poly_types, Some(draft.poly_type.clone()), Message::DraftTypeChanged),
            ].align_y(Alignment::Center),

            // Сюда вставляем наши поля для Polygon
            fill_fields,

            // Шаблон
            row![
                text("Шаблон:").width(120),
                text(template).color(Color::from_rgb8(
                    draft.line_color[0], 
                    draft.line_color[1], 
                    draft.line_color[2]
                )).size(16)
            ].align_y(Alignment::Center),

            // Кнопки
            row![
                button("Сохранить").on_press(Message::SaveTrackProps),
                button("Закрыть").on_press(Message::ApplyTrackProps),
                button("Отмена").on_press(Message::CloseTrackProps),
            ].spacing(10)
        ]
        .spacing(12)
        .padding(20);

        container(content)
            .style(|_| container::Style {
                background: Some(Color::from_rgb(0.15, 0.15, 0.18).into()),
                border: iced::Border { color: Color::from_rgb(0.3, 0.3, 0.3), width: 1.0, radius: 8.0.into() },
                ..Default::default()
            })
            .into()
    }  
}

// ----- Программа отрисовки карты (фон + тайлы OZF2) -----

struct MapProgram<'a> {
    app: &'a App,
}

    fn draw_scale_bar(
        frame: &mut canvas::Frame,
        bounds: Rectangle,
        scale_m_per_px: f64,
        cx: f64,
        cy: f64,
    ) {
        // Масштаб Mercator → «наземные» метры на широте центра.
        let (_, lat_c) = lonlat_of(cx, cy);
        let cos_lat = lat_c.to_radians().cos().abs().max(1e-6);
        let ground_m_per_px = scale_m_per_px * cos_lat;

        // Ищем «круглую» дистанцию для ширины ~140 px.
        let target_px = 140.0;
        let raw = ground_m_per_px * target_px;
        let exp = raw.log10().floor();
        let mag = 10f64.powf(exp);
        let norm = raw / mag;
        let nice = if norm < 1.5 {
            1.0
        } else if norm < 3.5 {
            2.0
        } else if norm < 7.5 {
            5.0
        } else {
            10.0
        };
        let distance_m = nice * mag;
        let bar_w = (distance_m / ground_m_per_px) as f32;

        let label = if distance_m >= 1000.0 {
            format!("{:.0} км", distance_m / 1000.0)
        } else if distance_m >= 1.0 {
            format!("{:.0} м", distance_m)
        } else {
            format!("{:.0} см", distance_m * 100.0)
        };

        // Раскладка: нижний левый угол.
        let margin = 16.0;
        let bar_h = 8.0;
        let label_h = 18.0;
        let bg_pad = 6.0;
        let x0 = margin;
        let y0 = bounds.height - margin - label_h - bar_h;
        let bar_y = y0 + label_h;

        // Полупрозрачная подложка.
        frame.fill_rectangle(
            Point::new(x0 - bg_pad, y0 - bg_pad),
            Size::new(bar_w + 2.0 * bg_pad, label_h + bar_h + 2.0 * bg_pad),
            Color::from_rgba8(0, 0, 0, 0.55),
        );

        // 4 сегмента в шахматном порядке.
        let n_seg = 4;
        let seg_w = bar_w / n_seg as f32;
        let color_a = Color::from_rgba8(240, 240, 240, 1.0);
        let color_b = Color::from_rgba8(60, 60, 60, 1.0);
        for i in 0..n_seg {
            let color = if i % 2 == 0 { color_a } else { color_b };
            frame.fill_rectangle(
                Point::new(x0 + i as f32 * seg_w, bar_y),
                Size::new(seg_w, bar_h),
                color,
            );
        }

        // Рамка.
        let border = canvas::Path::rectangle(Point::new(x0, bar_y), Size::new(bar_w, bar_h));
        frame.stroke(
            &border,
            canvas::Stroke::default()
                .with_color(Color::WHITE)
                .with_width(1.0),
        );

        // Подписи: «0» слева, дистанция справа.
        frame.fill_text(canvas::Text {
            content: "0".to_string(),
            position: Point::new(x0, bar_y - 2.0),
            color: Color::WHITE,
            size: iced::Pixels(11.0),
            align_x: iced::widget::text::Alignment::Left,
            align_y: iced::alignment::Vertical::Bottom,
            ..Default::default()
        });
        frame.fill_text(canvas::Text {
            content: label,
            position: Point::new(x0 + bar_w, bar_y - 2.0),
            color: Color::WHITE,
            size: iced::Pixels(11.0),
            align_x: iced::widget::text::Alignment::Right,
            align_y: iced::alignment::Vertical::Bottom,
            ..Default::default()
        });
    }

impl<'a> canvas::Program<Message> for MapProgram<'a> {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let app = self.app;
        let started = Instant::now();
        app.last_bounds.set(bounds.size());

        let geom = if app.osm_active() {
            app.osm_canvas_cache
                .draw(renderer, bounds.size(), |frame| {
                    frame.fill_rectangle(
                        Point::ORIGIN,
                        bounds.size(),
                        Color::from_rgba8(BG[0], BG[1], BG[2], BG[3] as f32 / 255.0),
                    );

                    let sz = bounds.size();
                    let (_z, tiles) = osm::visible_tiles(
                        app.cx,
                        app.cy,
                        app.scale,
                        sz.width as f64,
                        sz.height as f64,
                    );

                    for id in tiles {
                        let rect = osm::tile_screen_rect(
                            id,
                            app.cx,
                            app.cy,
                            app.scale,
                            sz.width as f64,
                            sz.height as f64,
                        );
                        if rect.x + rect.width < 0.0
                            || rect.y + rect.height < 0.0
                            || rect.x > bounds.width
                            || rect.y > bounds.height
                        {
                            continue;
                        }
                        match app.osm_cache.get(id) {
                            Some(Some(handle)) => {
                                frame.draw_image(rect, canvas::Image::new(handle));
                            }
                            Some(None) => {
                                frame.fill_rectangle(
                                    Point::new(rect.x, rect.y),
                                    Size::new(rect.width, rect.height),
                                    Color::from_rgba8(60, 40, 40, 1.0),
                                );
                            }
                            None => {
                                frame.fill_rectangle(
                                    Point::new(rect.x, rect.y),
                                    Size::new(rect.width, rect.height),
                                    Color::from_rgba8(45, 45, 50, 1.0),
                                );
                            }
                        }
                    }

                    // Атрибуция OSM/HOT.
                    let attr_w = 470.0_f32.min(bounds.width);
                    let attr_h = 20.0_f32;
                    frame.fill_rectangle(
                        Point::new(
                            bounds.width - attr_w - 4.0,
                            bounds.height - attr_h - 4.0,
                        ),
                        Size::new(attr_w, attr_h),
                        Color::from_rgba8(0, 0, 0, 0.6),
                    );
                    frame.fill_text(canvas::Text {
                        content: "© OpenStreetMap contributors, Tiles courtesy of Humanitarian OpenStreetMap Team"
                            .to_string(),
                        position: Point::new(
                            bounds.width - 8.0,
                            bounds.height - attr_h / 2.0 - 4.0,
                        ),
                        color: Color::WHITE,
                        size: iced::Pixels(11.0),
                        align_x: iced::widget::text::Alignment::Right,
                        align_y: iced::alignment::Vertical::Center,
                        ..Default::default()
                    });
                })
        } else {
            app.canvas_cache
                .draw(renderer, bounds.size(), |frame| {
                    frame.fill_rectangle(
                        Point::ORIGIN,
                        bounds.size(),
                        Color::from_rgba8(BG[0], BG[1], BG[2], BG[3] as f32 / 255.0),
                    );

                    if let Some(layer) = app.layers.get(app.active) {
                        let g = &layer.georef;
                        let a = affine(g, app.cx, app.cy);
                        let ppm = (a.ax * a.ax + a.ay * a.ay).sqrt();
                        let r = app.scale * ppm;

                        if r > 1e-12 && !layer.levels.is_empty() {
                            let mut pick = layer.levels[0];
                            for &lv in &layer.levels {
                                if lv.1 * r >= 0.97 {
                                    pick = lv;
                                }
                            }
                            let (lidx, fx, fy) = pick;

                            let (lv_w, lv_h, lv_tx, lv_ty) = {
                                let ozf_r = layer.ozf.borrow();
                                let lv = &ozf_r.levels[lidx];
                                (lv.width, lv.height, lv.tiles_x, lv.tiles_y)
                            };

                            if lv_w > 0 && lv_h > 0 && lv_tx > 0 && lv_ty > 0 {
                                let hw_f = bounds.width as f64 / 2.0;
                                let hh_f = bounds.height as f64 / 2.0;
                                let det = a.ax * a.by - a.bx * a.ay;

                                if det.abs() > 1e-20 {
                                    let level_px_at =
                                        |sx: f64, sy: f64| -> (f64, f64) {
                                            let dx_m = (sx - hw_f) * app.scale;
                                            let dy_m = -(sy - hh_f) * app.scale;
                                            let px0 = a.x0 + a.ax * dx_m + a.bx * dy_m;
                                            let py0 = a.y0 + a.ay * dx_m + a.by * dy_m;
                                            (px0 * fx, py0 * fy)
                                        };
                                    let (px_c0, py_c0) = level_px_at(0.0, 0.0);
                                    let (px_c1, py_c1) =
                                        level_px_at(bounds.width as f64, bounds.height as f64);

                                    let px_lo = px_c0.min(px_c1).max(0.0);
                                    let px_hi = px_c0.max(px_c1).min(lv_w as f64);
                                    let py_lo = py_c0.min(py_c1).max(0.0);
                                    let py_hi = py_c0.max(py_c1).min(lv_h as f64);

                                    if px_hi > px_lo && py_hi > py_lo {
                                        let tile = ozf::TILE as f64;
                                        let tx0 = (px_lo / tile).floor() as u32;
                                        let ty0 = (py_lo / tile).floor() as u32;
                                        let tx1 =
                                            (((px_hi / tile).floor() as u32) + 1).min(lv_tx);
                                        let ty1 =
                                            (((py_hi / tile).floor() as u32) + 1).min(lv_ty);

                                        let inv_scale = 1.0 / app.scale;
                                        let to_screen =
                                            |px_l: f64, py_l: f64| -> (f32, f32) {
                                                let px0 = px_l / fx - a.x0;
                                                let py0 = py_l / fy - a.y0;
                                                let dx_m = (a.by * px0 - a.bx * py0) / det;
                                                let dy_m = (-a.ay * px0 + a.ax * py0) / det;
                                                (
                                                    (hw_f + dx_m * inv_scale) as f32,
                                                    (hh_f - dy_m * inv_scale) as f32,
                                                )
                                            };

                                        for ty in ty0..ty1 {
                                            for tx in tx0..tx1 {
                                                let (sx_tl, sy_tl) = to_screen(
                                                    tx as f64 * tile,
                                                    ty as f64 * tile,
                                                );
                                                let (sx_br, sy_br) = to_screen(
                                                    (tx as f64 + 1.0) * tile,
                                                    (ty as f64 + 1.0) * tile,
                                                );

                                                let rect = Rectangle {
                                                    x: sx_tl.min(sx_br),
                                                    y: sy_tl.min(sy_br),
                                                    width: (sx_br - sx_tl).abs(),
                                                    height: (sy_br - sy_tl).abs(),
                                                };

                                                if rect.width < 0.5 || rect.height < 0.5 {
                                                    continue;
                                                }
                                                if rect.x + rect.width < 0.0
                                                    || rect.y + rect.height < 0.0
                                                    || rect.x > bounds.width
                                                    || rect.y > bounds.height
                                                {
                                                    continue;
                                                }

                                                let key = (app.active, lidx, tx, ty);
                                                let cached = app
                                                    .tile_handles
                                                    .borrow()
                                                    .get(&key)
                                                    .cloned();
                                                let handle_opt = match cached {
                                                    Some(h) => h,
                                                    None => {
                                                        let rgba = layer
                                                            .ozf
                                                            .borrow_mut()
                                                            .tile_rgba(lidx, tx, ty)
                                                            .ok();
                                                        let h = rgba.map(|r| {
                                                            ImageHandle::from_rgba(
                                                                ozf::TILE as u32,
                                                                ozf::TILE as u32,
                                                                r,
                                                            )
                                                        });
                                                        app.tile_handles
                                                            .borrow_mut()
                                                            .insert(key, h.clone());
                                                        h
                                                    }
                                                };

                                                if let Some(handle) = handle_opt {
                                                    frame.draw_image(
                                                        rect,
                                                        canvas::Image::new(handle),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                })
        };

        app.last_geom_ms
            .set(started.elapsed().as_secs_f64() * 1000.0);
        vec![geom]
    }
}

// ----- Программа отрисовки сетки (GPX) — отдельный виджет сверху -----

struct GridProgram<'a> {
    app: &'a App,
}

impl<'a> canvas::Program<Message> for GridProgram<'a> {
    type State = ();
    
    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let app = self.app;

        let geom = app.grid_cache.draw(renderer, bounds.size(), |frame| {
            app.last_bounds.set(bounds.size());
            if !app.show_grid {
                let hw_f = bounds.width as f64 / 2.0;
                let hh_f = bounds.height as f64 / 2.0;
                let to_screen = |mx: f64, my: f64| -> Point {
                    Point::new(
                        (hw_f + (mx - app.cx) / app.scale) as f32,
                        (hh_f - (my - app.cy) / app.scale) as f32,
                    )
                };

                let line_color = Color::from_rgba8(255, 70, 70, 1.0);
                for line in &app.lines {
                    let path = CanvasPath::new(|p| {
                        let mut first = true;
                        for &(mx, my) in line {
                            let pt = to_screen(mx, my);
                            if first {
                                p.move_to(pt);
                                first = false;
                            } else {
                                p.line_to(pt);
                            }
                        }
                    });
                    frame.stroke(
                        &path,
                        Stroke::default().with_color(line_color).with_width(1.5),
                    );
                }

                if let Some(g) = &app.grid {
                    let node_color = Color::from_rgba8(255, 40, 40, 1.0);
                    let size_f: f32 = 13.0;
                    let pad: f32 = 3.0;
                    let bg_color = Color::from_rgba8(0, 0, 0, 0.7);

                    for n in &g.nodes {
                        let (mx, my) = merc_of(n.lon, n.lat);
                        let pt = to_screen(mx, my);

                        // Отсечение по вьюпорту — до любых fill_*.
                        if pt.x < -200.0
                            || pt.y < -50.0
                            || pt.x > bounds.width + 200.0
                            || pt.y > bounds.height + 50.0
                        {
                            continue;
                        }

                        // Маркер узла
                        frame.fill_rectangle(
                            Point::new(pt.x - 1.5, pt.y - 1.5),
                            Size::new(3.0, 3.0),
                            node_color,
                        );

                        // Подпись
                        if app.show_labels && !n.name.is_empty() {
                            let n_chars = n.name.chars().count() as f32;
                            let text_w = n_chars * size_f * 0.55;
                            let bg_w = text_w + 2.0 * pad;
                            let bg_h = size_f + 2.0 * pad;
                            let bg_pos = Point::new(pt.x + 6.0, pt.y - 6.0 - bg_h);

                            frame.fill_rectangle(bg_pos, Size::new(bg_w, bg_h), bg_color);
                            frame.fill_text(canvas::Text {
                                content: n.name.clone(),
                                position: Point::new(bg_pos.x + pad, bg_pos.y + bg_h / 2.0),
                                color: Color::WHITE,
                                size: iced::Pixels(size_f),
                                align_x: iced::widget::text::Alignment::Left,
                                align_y: iced::alignment::Vertical::Center,
                                ..Default::default()
                            });
                        }
                    }
                }
            } 
            // Линейка масштаба — рисуется всегда, независимо от сетки.
            draw_scale_bar(frame, bounds, app.scale, app.cx, app.cy);
        });
        vec![geom]
    }
}

struct TrackProgram<'a> {
    app: &'a App,
}

impl<'a> canvas::Program<Message> for TrackProgram<'a> {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let app = self.app;
        let geom = app.tracks_cache.draw(renderer, bounds.size(), |frame| {
            let hw_f = bounds.width as f64 / 2.0;
            let hh_f = bounds.height as f64 / 2.0;
            let to_screen = |mx: f64, my: f64| -> Point {
                Point::new(
                    (hw_f + (mx - app.cx) / app.scale) as f32,
                    (hh_f - (my - app.cy) / app.scale) as f32,
                )
            };

            // Треки
            for t in &app.tracks {
                if !t.visible { continue; }
                // Берем цвет и ширину из props!
                let color = Color::from_rgb8(t.props.line_color[0], t.props.line_color[1], t.props.line_color[2]);
                let width = t.props.line_width;
                
                for seg in &t.segments {
                    if seg.points.len() < 2 { continue; }
                    
                    // Создаем path для линии
                    let path = CanvasPath::new(|p| {
                        let mut first = true;
                        for pt in &seg.points {
                            let (mx, my) = merc_of(pt.lon, pt.lat);
                            let sp = to_screen(mx, my);
                            if first {
                                p.move_to(sp);
                                first = false;
                            } else {
                                p.line_to(sp);
                            }
                        }
                    });
                    
                    frame.stroke(&path, Stroke::default().with_color(color).with_width(width));
                }
            }
            for t in &app.tracks {
                if !t.visible { continue; }
                let color = Color::from_rgb8(t.props.line_color[0], t.props.line_color[1], t.props.line_color[2]);
                let width = t.props.line_width;
                
                for seg in &t.segments {
                    if seg.points.len() < 2 { continue; }
                    
                    let path = CanvasPath::new(|p| {
                        let mut first = true;
                        for pt in &seg.points {
                            let (mx, my) = merc_of(pt.lon, pt.lat);
                            let sp = to_screen(mx, my);
                            if first {
                                p.move_to(sp);
                                first = false;
                            } else {
                                p.line_to(sp);
                            }
                        }
                        // Если это полигон, замыкаем контур
                        if t.props.poly_type == document::PolyType::Polygon {
                            p.close();
                        }
                    });
                    
                        // Если полигон и есть заливка — заливаем
                        if t.props.poly_type == document::PolyType::Polygon && t.props.fill_type != document::FillType::None {
                            // Для штриховки делаем цвет полупрозрачным, чтобы визуально отличать от сплошной заливки
                            let alpha = if t.props.fill_type == document::FillType::Solid { 1.0 } else { 0.3 };
                            let fill_c = Color::from_rgba8(
                                t.props.fill_color[0], 
                                t.props.fill_color[1], 
                                t.props.fill_color[2], 
                                alpha
                            );
                            frame.fill(&path, fill_c);
                        }
                    
                    frame.stroke(&path, Stroke::default().with_color(color).with_width(width));
                }
            }            

            // Точки
        for wp in &app.waypoints {
            if !wp.visible { continue; }
            let (mx, my) = merc_of(wp.lon, wp.lat);
            let pt = to_screen(mx, my);
            let circle = CanvasPath::circle(pt, 4.0);
            frame.fill(&circle, Color::from_rgb8(255, 200, 0));
            frame.stroke(&circle, Stroke::default().with_color(Color::WHITE).with_width(1.0));
            frame.fill_text(canvas::Text {
                content: wp.name.clone(),
                position: Point::new(pt.x + 6.0, pt.y - 6.0),
                color: Color::BLACK,
                size: iced::Pixels(12.0),
                ..Default::default()
            });
        }
        });
        vec![geom]
    }
}

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title("FoxyXplorer")
        .window_size(Size::new(1130.0, 880.0))
        .subscription(App::subscription)
        .exit_on_close_request(false) 
        .run()
}
