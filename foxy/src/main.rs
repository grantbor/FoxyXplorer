mod canvas;
mod domain;
mod io;
mod osm;
mod session;
mod ui;
mod util;
mod widgets;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use coord::{format_pair, CoordFormat};
use iced::mouse::ScrollDelta;
use iced::widget::canvas::{Cache, Canvas};
use iced::widget::image::Handle as ImageHandle; // <-- если ImageHandle ещё используется в App
// use iced::widget::tooltip;

use std::collections::{HashMap, HashSet};

use iced::widget::{
    column, container, mouse_area, row, stack, text,
};
use iced::{
    Color, Element, Length, Point, Size, Task,
};

use iced::keyboard::Modifiers;
use iced::Subscription;

use native_theme_iced::ResolvedTheme; 

use util::geo::{merc_of, lonlat_of, affine};
use util::coords::{today, date_offset, build_template, parse_coords};
use widgets::palette::load_palette;

const VIEW_W: f32 = 1100.0;
const VIEW_H: f32 = 720.0;


#[derive(Default)]
struct Selection {
    tracks: HashSet<u64>,
    waypoints: HashSet<u64>,
}


pub(crate) struct Layer {
    name: String,
    kit_name: String,
    ozf: RefCell<ozf::Ozf>,
    georef: ozmap::Georef,
    /// (ozf-индекс_уровня, ratio_x, ratio_y), от крупного к мелкому
    levels: Vec<(usize, f64, f64)>,
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
pub(crate) enum Message {
    Layer(usize),
    ToggleGrid,
    ToggleLabels,
    CycleFormat,
    //Decl(f64),
    //DeclReset,
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
    DraftStyleChanged(domain::LineStyle),
    DraftTypeChanged(domain::PolyType),
    DraftFillColorChanged([u8; 3]),
    DraftFillTypeChanged(domain::FillType),
    
    DraftCallsignChanged(String),
    DraftNum1Changed(String),
    DraftNum2Changed(String),
    DraftSuffixChanged(String),
    DraftDateYesterday,
    DraftDateToday,
    DraftDateChanged(String),
    ApplyTrackProps, 
    ToggleColorPicker,
    WindowResized(Size),
    WindowMoved(Point),
    DividerPressed,
    DividerReleased,
    CursorMoved(Point),
    OpenWaypointProps(u64),
    CloseWaypointProps,
    ApplyWaypointProps,
    
    DraftWpNameChanged(String),
    DraftWpDescChanged(String),
    DraftWpEleChanged(String),
    DraftWpTextColorChanged([u8; 3]),
    DraftWpBgColorChanged([u8; 3]),
    DraftWpCoordsChanged(String),
    
    AddNewWaypoint,
    ToggleTrackSaveMenu,
    ToggleWpSaveMenu,
    RefreshSystemTheme,
    None,
}

fn save_session(app: &App) {
    let format_str = match app.format {
        CoordFormat::Decimal => "Decimal",
        CoordFormat::DegMin => "DegMin",
        CoordFormat::DegMinSec => "DegMinSec",
    }.to_string();

    let sess = session::Session {
        version: 1,
        last_kit: app.last_kit.clone(),
        view: session::SessionView {
            cx: app.cx,
            cy: app.cy,
            scale: app.scale,
            active: app.active,
            show_grid: app.show_grid,
            show_labels: app.show_labels,
            format_str,
            window_size: Some((app.window_size.width, app.window_size.height)),
            window_pos: Some((app.window_pos.x, app.window_pos.y)),
        },
        tracks: app.tracks.clone(),
        waypoints: app.waypoints.clone(),
        next_id: app.next_id,
    };

    session::write_session(&sess);
}

pub(crate) struct App {
    layers: Vec<Layer>,
    active: usize, // 0..=layers.len(); == layers.len() означает OSM
    grid: Option<grid::Grid>,
    lines: Vec<Vec<(f64, f64)>>,
    show_grid: bool,
    show_labels: bool,
    declination: f64,
    //decl_offset: f64,
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
    tracks: Vec<domain::Track>,
    waypoints: Vec<domain::Waypoint>,
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
    track_backup: Option<domain::Track>, 
    last_track_click: Option<(u64, std::time::Instant)>,
    show_color_picker: bool,
    window_size: Size,
    window_pos: Point,
    right_panel_width: f32,
    resizing_panel: bool,
    waypoint_backup: Option<domain::Waypoint>,
    last_waypoint_click: Option<(u64, std::time::Instant)>,
    show_track_save_menu: bool,
    show_wp_save_menu: bool,
    theme: iced::Theme,
    resolved_theme: ResolvedTheme, 
}
enum Modal {
    TrackProps { track_id: u64, draft: domain::TrackProps },
    WaypointProps { 
        waypoint_id: u64, 
        draft: domain::Waypoint,
        coord_input: String,
        is_new: bool, // <-- ДОБАВИТЬ
    },
}

impl App {
    pub(crate) fn osm_active(&self) -> bool {
        self.active >= self.layers.len()
    }

    fn new(theme: iced::Theme, resolved_theme: ResolvedTheme) -> (App, Task<Message>) {
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
            if let Some(session) = session::load_session() {
                let mut app = App {
                    layers: Vec::new(),
                    active: 0, // Будет восстановлено из сессии
                    grid: None,
                    lines: Vec::new(),
                    show_grid: true,
                    show_labels: true,
                    declination: 0.0,
                    //*decl_offset: 0.0*,
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
                    window_size: session.view.window_size
                        .map(|(w, h)| Size::new(w, h))
                        .unwrap_or(Size::new(1130.0, 880.0)),
                    window_pos: session.view.window_pos
                        .map(|(x, y)| Point::new(x, y))
                        .unwrap_or(Point::new(100.0, 100.0)),
                    right_panel_width: 360.0,
                    resizing_panel: false,
                    waypoint_backup: None,
                    last_waypoint_click: None,
                    show_track_save_menu: false,
                    show_wp_save_menu: false,
                    // Временная заглушка, реальная тема придет из main
                    theme,
                    resolved_theme,
                };

                // Пересчитываем кэш статистики (количество точек и расстояние) для восстановленных треков
                for t in &mut app.tracks {
                    domain::recompute_track_stats(t);
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
                //app.decl_offset = session.view.decl_offset;
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
            //*decl_offset: 0.0*,
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
            window_size: Size::new(1130.0, 880.0),
            window_pos: Point::new(100.0, 100.0),
            right_panel_width: 340.0,
            resizing_panel: false,
            waypoint_backup: None,
            last_waypoint_click: None,
            show_track_save_menu: false,
            show_wp_save_menu: false,
            // Временная заглушка, реальная тема придет из main
            theme,
            resolved_theme,
        };

        // Определяем, какой комплект пробовать загрузить.
        let kit_path: Option<PathBuf> = if force_osm {
            None
        } else if let Some(d) = dir_arg {
            Some(PathBuf::from(d))
        } else {
            session::load_last_kit()
        };

        if let Some(p) = kit_path {
            if app.apply_kit(load_kit(&p)) {
                app.last_kit = Some(p.clone()); // <-- Сохраняем в состояние приложения
                session::save_last_kit(&p);
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
            if let Some(n) = g.nearest(lat, lon, self.declination) { // <-- Убрали + self.decl_offset
                s += &format!(
                    "   |   ближайший узел {}: {:.0} м, магнитный азимут {:.0}°",
                    n.node.name, n.distance_m, n.magnetic_azimuth
                );
            }
        }
        
        // ДОБАВЛЯЕМ СКЛОНЕНИЕ В КОНЕЦ СТРОКИ:
        s += &format!("   |   Склонение: {:.1}°", self.declination);
        
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
        let mut subs: Vec<Subscription<Message>> = vec![
            // 1. Отслеживаем зажатие клавиш Ctrl / Shift
            iced::event::listen_with(|event, _status, _id| {
                if let iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) = event {
                    Some(Message::ModifiersChanged(modifiers))
                } else {
                    None
                }
            }),
            
            // 2. Отслеживаем запрос на закрытие окна
            iced::window::close_requests().map(|_| Message::WindowCloseRequested),
            
            // 3. Отслеживаем изменение размера и позиции окна
            iced::window::events().map(|(_id, event)| {
                match event {
                    iced::window::Event::Resized(size) => Message::WindowResized(size),
                    iced::window::Event::Moved(pos) => Message::WindowMoved(pos),
                    _ => Message::None,
                }
            }),

            // 4. Отслеживаем смену системной темы (через встроенную подписку iced)
            iced::system::theme_changes().map(|_| Message::RefreshSystemTheme),
        ];

        // 5. Ловим мышь только пока тянем разделитель панели
        if self.resizing_panel {
            subs.push(iced::event::listen_with(|event, _status, _id| {
                match event {
                    iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                        Some(Message::CursorMoved(position))
                    }
                    iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                        Some(Message::DividerReleased)
                    }
                    _ => None,
                }
            }));
        }

        Subscription::batch(subs)
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
            // Message::Decl(d) => {
            //     self.decl_offset += d;
            //     self.refresh_status();
            // }
            // Message::DeclReset => {
            //     self.decl_offset = 0.0;
            //     self.refresh_status();
            // }
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
                    session::save_last_kit(&path);
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
                    match io::load_user_file(&path, &mut self.next_id) {
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
                let now = std::time::Instant::now();
                let is_double_click = self.last_waypoint_click.map_or(false, |(last_id, last_time)| {
                    last_id == id && now.duration_since(last_time).as_millis() < 300
                });

                if is_double_click {
                    // Двойной клик: открываем свойства
                    self.last_waypoint_click = None;
                    if let Some(w) = self.waypoints.iter().find(|w| w.id == id) {
                        self.waypoint_backup = Some(w.clone());
                        self.modal = Some(Modal::WaypointProps { 
                            waypoint_id: id, 
                            draft: w.clone(),
                            // Инициализируем строку текущими координатами
                            coord_input: format!("{:.6} {:.6}", w.lat, w.lon),
                            is_new: false,
                        });
                    }
                } else {
                    // Одиночный клик: запоминаем время и делаем выделение
                    self.last_waypoint_click = Some((id, now));

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
                        self.selection.tracks.clear();
                        self.selection.waypoints.insert(id);
                    }
                    self.last_waypoint_clicked = Some(id);
                }
            }
            Message::DeleteSelectedTracks => {
                let ids = self.selection.tracks.clone();
                self.tracks.retain(|t| !ids.contains(&t.id));
                self.selection.tracks.clear();
                self.selection.waypoints.clear();
                self.tracks_cache.clear();
            }
            Message::SaveTracksPlt => {
                // Ищем первый выделенный трек, чтобы взять его имя
                let name = self.tracks.iter()
                    .find(|t| self.selection.tracks.contains(&t.id))
                    .map(|t| if t.props.description.is_empty() { t.name.clone() } else { t.props.description.clone() })
                    .unwrap_or_else(|| "track".to_string());

                let mut dialog = rfd::AsyncFileDialog::new()
                    .set_file_name(format!("{}.plt", name)) // <-- Подставляем имя
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
                if let Err(e) = io::save_any(&sel_tracks, &sel_wps, &path) {
                    self.status = format!("Ошибка: {e}");
                } else {
                    self.status = format!("Сохранено в {}", path.display());
                }
            }
            Message::SaveTracksPltPicked(None) => {}

            Message::SaveTracksGpx => {
                let name = self.tracks.iter()
                    .find(|t| self.selection.tracks.contains(&t.id))
                    .map(|t| if t.props.description.is_empty() { t.name.clone() } else { t.props.description.clone() })
                    .unwrap_or_else(|| "tracks".to_string());

                let mut dialog = rfd::AsyncFileDialog::new()
                    .set_file_name(format!("{}.gpx", name)) // <-- Подставляем имя
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
                if let Err(e) = io::save_any(&sel_tracks, &sel_wps, &path) {
                    self.status = format!("Ошибка: {e}");
                } else {
                    self.status = format!("Сохранено в {}", path.display());
                }
            }
            Message::SaveTracksGpxPicked(None) => {}

            Message::SaveTracksMs => {
                let name = self.tracks.iter()
                    .find(|t| self.selection.tracks.contains(&t.id))
                    .map(|t| if t.props.description.is_empty() { t.name.clone() } else { t.props.description.clone() })
                    .unwrap_or_else(|| "tracks".to_string());

                let mut dialog = rfd::AsyncFileDialog::new()
                    .set_file_name(format!("{}.ms", name)) // <-- Подставляем имя
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
                if let Err(e) = io::save_any(&sel_tracks, &sel_wps, &path) {
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
                if let Err(e) = io::save_any(&[], &sel_wps, &path) {
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
                if let Ok(content) = io::load_user_file(&path, &mut self.next_id) {
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
                if let Err(e) = io::save_any(&[], &refs, &path) {
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
                        if let Err(e) = io::save_plt(t, &path) {
                            self.status = format!("Ошибка сохранения: {e}");
                        } else {
                            self.status = format!("Сохранено в {}", path.display());
                            if let Some(t_mut) = self.tracks.iter_mut().find(|t| t.id == *track_id) {
                                t_mut.source = domain::FileSource::File { 
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
            Message::WindowResized(size) => {
                self.window_size = size;
            }
            Message::WindowMoved(pos) => {
                self.window_pos = pos;
            }
            Message::DividerPressed => {
                self.resizing_panel = true;
            }
            Message::DividerReleased => {
                self.resizing_panel = false;
            }
            Message::CursorMoved(p) => {
                if self.resizing_panel {
                    let win_w = self.window_size.width;
                    let new_w = (win_w - p.x).clamp(200.0, win_w * 0.6);
                    self.right_panel_width = new_w;
                }
            }
            Message::OpenWaypointProps(id) => {
                if let Some(w) = self.waypoints.iter().find(|w| w.id == id) {
                    self.waypoint_backup = Some(w.clone());
                    self.modal = Some(Modal::WaypointProps { 
                        waypoint_id: id, 
                        draft: w.clone(),
                        // Инициализируем строку текущими координатами
                        coord_input: format!("{:.6} {:.6}", w.lat, w.lon),
                        is_new: false,
                    });
                }
            }
            Message::CloseWaypointProps => {
                if let Some(Modal::WaypointProps { waypoint_id, is_new, .. }) = &self.modal {
                    if *is_new {
                        // Если это была новая точка — удаляем её из списка
                        let id = *waypoint_id;
                        self.waypoints.retain(|w| w.id != id);
                    }
                }
                // Откатываем изменения для существующих
                if let Some(backup) = self.waypoint_backup.take() {
                    if let Some(w) = self.waypoints.iter_mut().find(|w| w.id == backup.id) {
                        *w = backup;
                    }
                }
                self.tracks_cache.clear();
                self.modal = None;
            }
            Message::ApplyWaypointProps => {
                self.waypoint_backup = None;
                self.modal = None;
            }
            Message::DraftWpNameChanged(s) => {
                let mut tid = None;
                if let Some(Modal::WaypointProps { waypoint_id, draft, .. }) = &mut self.modal {
                    draft.name = s; tid = Some(*waypoint_id);
                }
                if let Some(id) = tid { self.update_waypoint_preview(id); }
            }
            Message::DraftWpDescChanged(s) => {
                let mut tid = None;
                if let Some(Modal::WaypointProps { waypoint_id, draft, .. }) = &mut self.modal {
                    draft.description = Some(s); tid = Some(*waypoint_id);
                }
                if let Some(id) = tid { self.update_waypoint_preview(id); }
            }
            Message::DraftWpEleChanged(s) => {
                let mut tid = None;
                if let Some(Modal::WaypointProps { waypoint_id, draft, .. }) = &mut self.modal {
                    draft.ele = s.parse::<f64>().ok(); tid = Some(*waypoint_id);
                }
                if let Some(id) = tid { self.update_waypoint_preview(id); }
            }
            Message::DraftWpTextColorChanged(c) => {
                let mut tid = None;
                if let Some(Modal::WaypointProps { waypoint_id, draft, .. }) = &mut self.modal {
                    draft.text_color = c; tid = Some(*waypoint_id);
                }
                if let Some(id) = tid { self.update_waypoint_preview(id); }
            }
            Message::DraftWpBgColorChanged(c) => {
                let mut tid = None;
                if let Some(Modal::WaypointProps { waypoint_id, draft, .. }) = &mut self.modal {
                    draft.bg_color = c; tid = Some(*waypoint_id);
                }
                if let Some(id) = tid { self.update_waypoint_preview(id); }
            }
            Message::DraftWpCoordsChanged(s) => {
                let mut tid = None;
                if let Some(Modal::WaypointProps { waypoint_id, draft, coord_input, .. }) = &mut self.modal {
                    *coord_input = s.clone();
                    
                    // Передаем self.declination
                    if let Some((lat, lon)) = parse_coords(&s, self.grid.as_ref(), self.declination) {
                        if lat.abs() <= 90.0 && lon.abs() <= 180.0 {
                            draft.lat = lat;
                            draft.lon = lon;
                            draft.ele = None;
                            tid = Some(*waypoint_id);
                        }
                    }
                }
                if let Some(id) = tid { self.update_waypoint_preview(id); }
            }
            Message::AddNewWaypoint => {
                let (lon, lat) = lonlat_of(self.cx, self.cy);
                let new_wp = domain::Waypoint {
                    id: self.next_id,
                    name: "New WP".into(),
                    lon,
                    lat,
                    ele: None, time: None, symbol: None, description: None,
                    visible: true,
                    source: domain::FileSource::New,
                    text_color: [0, 0, 0],
                    bg_color: [0, 255, 255],
                };
                self.next_id += 1;
                self.waypoints.push(new_wp.clone());
                self.waypoint_backup = Some(new_wp.clone());
                self.modal = Some(Modal::WaypointProps {
                    waypoint_id: new_wp.id,
                    draft: new_wp,
                    coord_input: format!("{:.6} {:.6}", lat, lon),
                    is_new: true, // <-- помечаем как новую
                });
                self.tracks_cache.clear();
            }
            Message::ToggleTrackSaveMenu => {
                self.show_track_save_menu = !self.show_track_save_menu;
                self.show_wp_save_menu = false; // закрываем другое меню
            }
            Message::ToggleWpSaveMenu => {
                self.show_wp_save_menu = !self.show_wp_save_menu;
                self.show_track_save_menu = false;
            }
            Message::RefreshSystemTheme => {
                if let Ok((theme, resolved, _, _)) = native_theme_iced::from_system() {
                    self.theme = theme;
                    self.resolved_theme = resolved;
                }
            }
            Message::None => {}
        }

        // Догружаем очередную пачку тайлов OSM, если он активен.
        self.request_osm_tiles()
    }

    fn update_waypoint_preview(&mut self, id: u64) {
        if let Some(Modal::WaypointProps { draft, .. }) = &self.modal {
            if let Some(w) = self.waypoints.iter_mut().find(|w| w.id == id) {
                *w = draft.clone();
                self.tracks_cache.clear();
            }
        }
    }

    fn format_name(&self) -> &'static str {
        match self.format {
            CoordFormat::Decimal => "dd.dddddd",
            CoordFormat::DegMin => "dd°mm.mmmm′",
            CoordFormat::DegMinSec => "dd°mm′ss.ss″",
        }
    }
    
 

    fn view(&self) -> Element<'_, Message> {
        // Формируем строку с информацией (без геометрии)
        let name = if self.osm_active() {
            let pending = self.osm_cache.pending.borrow().len();
            let cached = self.osm_cache.mem.borrow().len();
            format!("OSM z{} (кэш {}, в очереди {})", self.osm_z, cached, pending)
        } else {
            self.layers
                .get(self.active)
                .map(|l| l.name.clone())
                .unwrap_or_else(|| "-".into())
        };
        
        let info = format!(
            "Склонение: {:.2}° (WMM2025)  |  масштаб: {:.2} м/пикс  |  слой: {}",
            self.declination,
            self.scale,
            name
        );

        // 1. Собираем базовый интерфейс
        let base_ui = column![
            self.view_top_toolbar(),
            row![
                container(self.view_tool_palette()).width(Length::Fixed(56.0)).height(Length::Fill),
                container(self.view_map()).width(Length::Fill).height(Length::Fill),
                // Разделитель
                mouse_area(
                    container(text(""))
                        .width(Length::Fixed(6.0))
                        .height(Length::Fill)
                        .style(|_| container::Style {
                            background: Some(Color::from_rgb(0.2, 0.2, 0.25).into()),
                            ..Default::default()
                        })
                )
                .on_press(Message::DividerPressed)
                .on_release(Message::DividerReleased),
                container(self.view_right_panel())
                    .width(Length::Fixed(self.right_panel_width))
                    .height(Length::Fill),
            ]
            .spacing(4)
            .height(Length::Fill),
            text(self.status.as_str()),
            text(info),
        ]
        .spacing(4)
        .padding(4)
        .width(Length::Fill)
        .height(Length::Fill);

        // 2. Накладываем модальные окна (если они открыты)
        if let Some(Modal::TrackProps { track_id, draft }) = &self.modal {
            let modal_ui = self.view_track_props_modal(*track_id, draft);
            
            stack![
                base_ui,
                container(text(""))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.6).into()),
                        ..Default::default()
                    }),
                container(modal_ui)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
            ]
            .into()
        } else if let Some(Modal::WaypointProps { waypoint_id, draft, coord_input, .. }) = &self.modal {
            let modal_ui = self.view_waypoint_props_modal(*waypoint_id, draft, coord_input);
            
            stack![
                base_ui,
                container(text(""))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.6).into()),
                        ..Default::default()
                    }),
                container(modal_ui)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
            ]
            .into()
        } else {
            base_ui.into()
        }
    }

    fn view_map(&self) -> Element<'_, Message> {
        let map_canvas = Canvas::new(canvas::MapProgram { app: self })
            .width(Length::Fill)
            .height(Length::Fill);
        let tracks_canvas = Canvas::new(canvas::TrackProgram { app: self })
            .width(Length::Fill)
            .height(Length::Fill);
        let grid_canvas = Canvas::new(canvas::GridProgram { app: self })
            .width(Length::Fill)
            .height(Length::Fill);

        mouse_area(stack![map_canvas, tracks_canvas, grid_canvas])
            .on_press(Message::Pressed)
            .on_release(Message::Released)
            .on_move(Message::Moved)
            .on_scroll(Message::Scrolled)
            .into()
    }
}




fn main() -> iced::Result {
    // 1. Читаем сессию заранее
    let session = session::load_session(); 
    
    let mut window_settings = iced::window::Settings {
        size: Size::new(1130.0, 880.0),
        min_size: Some(Size::new(640.0, 480.0)),
        exit_on_close_request: false,
        ..Default::default()
    };

    if let Some(s) = &session {
        if let Some((w, h)) = s.view.window_size {
            window_settings.size = Size::new(w, h);
        }
        if let Some((x, y)) = s.view.window_pos {
            window_settings.position = iced::window::Position::Specific(Point::new(x, y));
        }
    }

    // Получаем системную тему (с фоллбэком)
    let (initial_theme, initial_resolved) = 
        if let Ok((t, r, _, _)) = native_theme_iced::from_system() {
            (t, r)
        } else if let Ok((t, r)) = native_theme_iced::from_preset("catppuccin-mocha", true) {
            (t, r)
        } else {
            // Аварийный фоллбэк на стандартную тему iced, если крейт не смог загрузить ничего
            (iced::Theme::Dark, native_theme_iced::from_preset("default", true).unwrap().1)
        };

    // 3. Запускаем приложение, передавая тему
    iced::application(
        move || App::new(initial_theme.clone(), initial_resolved.clone()), 
        App::update, 
        App::view
    )
    .title("FoxyXplorer")
    .window(window_settings)
    .subscription(App::subscription)
    .theme(|app: &App| app.theme.clone()) // <--- ВАЖНО: iced будет брать тему из App
    .run()
}
