mod app; // <-- ДОБАВЛЕН МОДУЛЬ APP
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
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use coord::{format_pair, CoordFormat};
use iced::mouse::ScrollDelta;
use iced::widget::canvas::{Cache, Canvas};
use iced::widget::image::Handle as ImageHandle;
use iced::widget::{column, container, mouse_area, row, stack, text};
use iced::{Color, Element, Length, Point, Size, Task};
use iced::keyboard::Modifiers;
use iced::Subscription;
use native_theme_iced::ResolvedTheme; 

use util::geo::{merc_of, lonlat_of, affine};
use util::coords::today;
use widgets::palette::load_palette;

const VIEW_W: f32 = 1100.0;
const VIEW_H: f32 = 720.0;

#[derive(Default)]
pub(crate) struct Selection {
    pub(crate) tracks: HashSet<u64>,
    pub(crate) waypoints: HashSet<u64>,
    pub(crate) points: HashSet<(u64, usize, usize)>,
}

pub(crate) struct Layer {
    pub(crate) name: String,
    pub(crate) kit_name: String,
    pub(crate) ozf: RefCell<ozf::Ozf>,
    pub(crate) georef: ozmap::Georef,
    pub(crate) levels: Vec<(usize, f64, f64)>,
}

pub(crate) type GridData = (grid::Grid, Vec<Vec<(f64, f64)>>);

#[derive(Clone)]
pub(crate) struct UndoState {
    pub tracks: Vec<Arc<domain::Track>>,
    pub waypoints: Vec<Arc<domain::Waypoint>>,
    pub next_id: u64,
    pub label: String,
}

pub(crate) struct TrackRowData {
    pub pid: usize,
    pub sect: usize,
    pub pt_idx: usize,
    pub lat: f64,
    pub lon: f64,
    pub ele: Option<f64>,
    pub time: Option<f64>,
    pub dist_m: f64,
    pub kph: f64,
}

pub(crate) struct TrackListState {
    pub track_id: u64,
    pub rows: Vec<TrackRowData>,
    pub selected_points: HashSet<(usize, usize)>, // (sect, pt_idx)
    pub last_clicked: Option<(usize, usize)>,
}

pub(crate) fn load_kit(root: &Path) -> Result<(Vec<Layer>, Option<GridData>, Vec<String>), String> {
    let mut notes = Vec::new();

    let mut map_dirs: Vec<PathBuf> = Vec::new();
    let mut data_dirs: Vec<PathBuf> = Vec::new();

    if root.join("Maps").is_dir() {
        map_dirs.push(root.join("Maps"));
    }
    if root.join("Data").is_dir() {
        data_dirs.push(root.join("Data"));
    }

    if let Ok(entries) = std::fs::read_dir(root) {
        for e in entries.flatten() {
            let p = e.path();
            if !p.is_dir() { continue; }
            if p.join("Maps").is_dir() { map_dirs.push(p.join("Maps")); }
            if p.join("Data").is_dir() { data_dirs.push(p.join("Data")); }
        }
    }

    if map_dirs.is_empty() { map_dirs.push(root.to_path_buf()); }
    if data_dirs.is_empty() { data_dirs.push(root.to_path_buf()); }

    let mut maps: Vec<PathBuf> = Vec::new();
    for d in &map_dirs {
        if let Ok(entries) = std::fs::read_dir(d) {
            for e in entries.flatten() {
                let p = e.path();
                let ext = p.extension().and_then(|s| s.to_str()).map(|s| s.to_lowercase());
                if ext.as_deref() == Some("map") { maps.push(p); }
            }
        }
    }
    maps.sort();

    let mut layers = Vec::new();
    let kit_name = root.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();

    for p in &maps {
        let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            
        let map = match ozmap::MapFile::open(p) {
            Ok(m) => m,
            Err(e) => { notes.push(format!("{stem}: {e}")); continue; }
        };
        if !map.projection.to_lowercase().contains("mercator") {
            notes.push(format!("{stem}: проекция «{}» пока не поддерживается", map.projection));
            continue;
        }
        let georef = match map.georef() {
            Ok(g) => g,
            Err(e) => { notes.push(format!("{stem}: {e}")); continue; }
        };

        let parent = p.parent().unwrap_or(root);
        let raster = parent.join(&map.image_file);
        let ozf = match ozf::Ozf::open(&raster) {
            Ok(o) => o,
            Err(e) => { notes.push(format!("{stem}: {} — {e}", map.image_file)); continue; }
        };
        if ozf.levels.is_empty() {
            notes.push(format!("{stem}: пустой список уровней в {}", map.image_file));
            continue;
        }
        let bw = ozf.levels.iter().map(|l| l.width).max().unwrap_or(1) as f64;
        let bh = ozf.levels.iter().map(|l| l.height).max().unwrap_or(1) as f64;
        let mut levels: Vec<(usize, f64, f64)> = ozf
            .levels.iter().enumerate()
            .map(|(i, l)| (i, l.width as f64 / bw, l.height as f64 / bh))
            .collect();
        levels.sort_by(|a, b| b.1.total_cmp(&a.1));

        let mut short = stem.strip_suffix("_ozf").unwrap_or(&stem).to_string();

        if !kit_name.is_empty() && short.starts_with(&kit_name) {
            short = short[kit_name.len()..].trim_start_matches('_').to_string();
        } else if short.len() > 11 && short.chars().nth(4) == Some('-') && short.chars().nth(7) == Some('-') {
            short = short.chars().skip(11).collect::<String>();
            if let Some(idx) = short.find('_') { short = short[idx + 1..].to_string(); }
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
            "не найдено ни одной пригодной карты (.map + .ozf2).\nПроверены каталоги: {}.\n{}",
            map_dirs.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "),
            notes.join("; ")
        ));
    }

    let mut gpxs: Vec<PathBuf> = Vec::new();
    for d in &data_dirs {
        if let Ok(entries) = std::fs::read_dir(d) {
            for e in entries.flatten() {
                let p = e.path();
                let ext = p.extension().and_then(|s| s.to_str()).map(|s| s.to_lowercase());
                if ext.as_deref() == Some("gpx") { gpxs.push(p); }
            }
        }
    }

    let mut best: Option<GridData> = None;
    for p in &gpxs {
        let Ok(bytes) = std::fs::read(p) else { continue };
        let Ok(g) = grid::Grid::from_gpx(&geoio::text::decode(&bytes)) else { continue; };
        if best.as_ref().map_or(true, |(b, _)| g.nodes.len() > b.nodes.len()) {
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
            data_dirs.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
        ));
    }

    Ok((layers, best, notes))
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    Layer(usize), ToggleGrid, ToggleLabels, CycleFormat,
    Moved(Point), Scrolled(ScrollDelta), Fit,
    PickKit, KitPicked(Option<PathBuf>),
    OsmTileLoaded(osm::TileId, Result<Vec<u8>, String>),
    PickTrack, TracksPicked(Option<Vec<PathBuf>>),
    ToggleTrackVisible(u64, bool), SelectTrack(u64), ToggleAllTracksVisible, DeleteSelectedTracks,
    SaveTracksPlt, SaveTracksPltPicked(Option<PathBuf>),
    SaveTracksGpx, SaveTracksGpxPicked(Option<PathBuf>),
    SaveTracksMs, SaveTracksMsPicked(Option<PathBuf>),
    ToggleWaypointVisible(u64, bool), SelectWaypoint(u64),
    SaveWaypointsWptNew, SaveWaypointsWptNewPicked(Option<PathBuf>),
    SaveWaypointsWptAppend, SaveWaypointsWptAppendPicked(Option<PathBuf>),
    ToggleAllWaypointsVisible, DeleteSelectedWaypoints,
    ModifiersChanged(Modifiers), SearchTracksChanged(String), SearchWaypointsChanged(String),
    WindowCloseRequested,
    OpenTrackProps(u64), CloseTrackProps, SaveTrackProps, SaveTrackPropsPicked(Option<PathBuf>),
    DraftDescChanged(String), DraftColorChanged([u8; 3]), DraftWidthChanged(String),
    DraftStyleChanged(domain::LineStyle), DraftTypeChanged(domain::PolyType),
    DraftFillColorChanged([u8; 3]), DraftFillTypeChanged(domain::FillType),
    DraftCallsignChanged(String), DraftNum1Changed(String), DraftNum2Changed(String), DraftSuffixChanged(String),
    DraftDateYesterday, DraftDateToday, DraftDateChanged(String),
    ApplyTrackProps, ToggleColorPicker,
    WindowResized(Size), WindowMoved(Point), DividerPressed, DividerReleased, CursorMoved(Point),
    OpenWaypointProps(u64), CloseWaypointProps, ApplyWaypointProps,
    DraftWpNameChanged(String), DraftWpDescChanged(String), DraftWpEleChanged(String),
    DraftWpTextColorChanged([u8; 3]), DraftWpBgColorChanged([u8; 3]), DraftWpCoordsChanged(String),
    AddNewWaypoint, ToggleTrackSaveMenu, ToggleWpSaveMenu, RefreshSystemTheme, SimplifyTracks,
    OpenTrackList(u64),
    CloseTrackList,
    SelectTrackPoint(usize, usize), // (sect, pt_idx)
    DeleteSelectedTrackPoints,
    CenterOnTrackPoint(usize, usize),
    SplitTrackAtPoint(usize, usize),
    SelectPointsOutsideGrid,
    ModalDragStart, TrackListUp, TrackListDown, FitTrackToScreen,CenterOnWaypoint(u64),
    ClearWaypointCrosshair,
    SelectTool(Tool),
    MapPressed(Point), MapReleased(Point),
    DeleteSelectedPoints,
    KeyPressed(iced::keyboard::Key, Modifiers),
    KeyReleased(iced::keyboard::Key),
    FreehandReleased,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tool {
    Pan,
    SelectObjects,
    SelectPoints,
    DrawTrack,
    Freehand,
    AddWaypoint,
    Ruler,
}

pub(crate) struct DrawingTrack {
    pub points: Vec<(f64, f64)>, // (lon, lat)
    pub target_track_id: Option<u64>,
}

pub(crate) fn save_session(app: &App) {
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
        // ИСПОЛЬЗУЕМ .as_ref().clone()
        tracks: app.tracks.iter().map(|t| t.as_ref().clone()).collect(),
        waypoints: app.waypoints.iter().map(|w| w.as_ref().clone()).collect(),
        next_id: app.next_id,
    };

    session::write_session(&sess);
}

pub(crate) struct App {
    pub(crate) layers: Vec<Layer>,
    pub(crate) active: usize,
    pub(crate) grid: Option<grid::Grid>,
    pub(crate) lines: Vec<Vec<(f64, f64)>>,
    pub(crate) show_grid: bool,
    pub(crate) show_labels: bool,
    pub(crate) declination: f64,
    pub(crate) format: CoordFormat,
    pub(crate) cx: f64,
    pub(crate) cy: f64,
    pub(crate) scale: f64,
    pub(crate) canvas_cache: Cache,
    pub(crate) grid_cache: Cache,
    pub(crate) osm_canvas_cache: Cache,
    pub(crate) tracks_cache: Cache,
    pub(crate) tile_handles: RefCell<HashMap<(usize, usize, u32, u32), Option<ImageHandle>>>,
    pub(crate) dragging: bool,
    pub(crate) cursor: Option<Point>,
    pub(crate) status: String,
    pub(crate) last_geom_ms: Cell<f64>,
    pub(crate) last_bounds: Cell<Size>,
    pub(crate) osm_cache: osm::OsmCache,
    pub(crate) osm_client: reqwest::Client,
    pub(crate) osm_z: u8,
    pub(crate) next_id: u64,
    pub(crate) selection: Selection,
    pub(crate) modifiers: Modifiers,
    pub(crate) last_track_clicked: Option<u64>,
    pub(crate) last_waypoint_clicked: Option<u64>,
    pub(crate) search_tracks: String,
    pub(crate) search_waypoints: String,
    pub(crate) last_kit: Option<PathBuf>, 
    pub(crate) palette: Vec<[u8; 3]>,
    pub(crate) modal: Option<Modal>,
    pub(crate) track_backup: Option<domain::Track>, 
    pub(crate) last_track_click: Option<(u64, std::time::Instant)>,
    pub(crate) show_color_picker: bool,
    pub(crate) window_size: Size,
    pub(crate) window_pos: Point,
    pub(crate) right_panel_width: f32,
    pub(crate) resizing_panel: bool,
    pub(crate) waypoint_backup: Option<domain::Waypoint>,
    pub(crate) last_waypoint_click: Option<(u64, std::time::Instant)>,
    pub(crate) show_track_save_menu: bool,
    pub(crate) show_wp_save_menu: bool,
    pub(crate) theme: iced::Theme,
    pub(crate) resolved_theme: ResolvedTheme, 
    pub(crate) simplify_tolerance: f64,     // Значение по умолчанию 2.0. В будущем будет меняться через настройки.
    pub(crate) tracks: Vec<Arc<domain::Track>>,
    pub(crate) waypoints: Vec<Arc<domain::Waypoint>>,
    pub(crate) undo_stack: Vec<UndoState>,
    pub(crate) redo_stack: Vec<UndoState>,
    pub(crate) modal_drag_offset: Point,
    pub(crate) modal_dragging: bool,
    pub(crate) modal_drag_start_cursor: Option<Point>,
    pub(crate) modal_drag_start_offset: Point,
    pub(crate) waypoint_crosshair: Option<(f64, f64)>,
    pub(crate) active_tool: Tool,
    pub(crate) space_pan_active: bool,
    pub(crate) moving_points: bool, 
    pub(crate) move_start_cursor: Option<Point>,
    pub(crate) point_press_start: Option<Point>,
    pub(crate) point_pressed_key: Option<(u64, usize, usize)>,
    pub(crate) point_is_dragging: bool,
    pub(crate) rubber_band_start: Option<Point>,
    pub(crate) rubber_band_end: Option<Point>,
    pub(crate) last_point_clicked: Option<(u64, usize, usize)>,
    pub(crate) drawing_track: Option<DrawingTrack>,
    pub(crate) freehand_is_drawing: bool,
    pub(crate) ruler_points: Vec<(f64, f64)>, // (lon, lat)
}

pub(crate) enum Modal {
    TrackProps { track_id: u64, draft: domain::TrackProps },
    WaypointProps { waypoint_id: u64, draft: domain::Waypoint, coord_input: String, is_new: bool },
    TrackList(TrackListState),
}

fn modal_drag_padding(offset: Point) -> iced::Padding {
    let mut pad = iced::Padding::new(0.0);
    if offset.y > 0.0 { pad = pad.top(offset.y * 2.0); } else { pad = pad.bottom(-offset.y * 2.0); }
    if offset.x > 0.0 { pad = pad.left(offset.x * 2.0); } else { pad = pad.right(-offset.x * 2.0); }
    pad
}

impl App {
    pub(crate) fn osm_active(&self) -> bool {
        self.active >= self.layers.len()
    }

    fn new(theme: iced::Theme, resolved_theme: ResolvedTheme) -> (App, Task<Message>) {
        let args: Vec<String> = std::env::args().collect();
        let force_osm = args.iter().any(|a| a == "--osm");
        let dir_arg = args.iter().skip(1).find(|a| !a.starts_with("--")).cloned();

        let cache_cfg = osm::OsmCacheConfig {
            max_age_days: std::env::var("FOXY_CACHE_MAX_AGE_DAYS").ok().and_then(|s| s.parse().ok()).unwrap_or(90),
            max_total_size: std::env::var("FOXY_CACHE_MAX_SIZE_MB").ok().and_then(|s| s.parse::<u64>().ok()).map(|mb| mb * 1024 * 1024).unwrap_or(2 * 1024 * 1024 * 1024),
            cleanup_interval_secs: std::env::var("FOXY_CACHE_CLEANUP_HOURS").ok().and_then(|s| s.parse::<u64>().ok()).map(|h| h * 3600).unwrap_or(24 * 3600),
        };

        if !force_osm {
            if let Some(session) = session::load_session() {
                let mut app = App {
                    layers: Vec::new(), active: 0, grid: None, lines: Vec::new(),
                    show_grid: true, show_labels: true, declination: 0.0, format: CoordFormat::Decimal,
                    cx: 0.0, cy: 0.0, scale: 1.0,
                    canvas_cache: Cache::default(), grid_cache: Cache::default(), osm_canvas_cache: Cache::default(),
                    tracks_cache: Cache::default(),
                    tile_handles: RefCell::new(HashMap::new()), dragging: false, cursor: None,
                    status: String::new(), last_geom_ms: Cell::new(0.0), last_bounds: Cell::new(Size::new(VIEW_W, VIEW_H)),
                    osm_cache: osm::OsmCache::new(&cache_cfg), osm_client: osm::client(), osm_z: 0,
                    tracks: session.tracks.into_iter().map(Arc::new).collect(), 
                    waypoints: session.waypoints.into_iter().map(Arc::new).collect(), 
                    next_id: session.next_id, selection: Selection::default(),
                    last_kit: session.last_kit.clone(), modifiers: iced::keyboard::Modifiers::default(),
                    last_track_clicked: None, last_waypoint_clicked: None,
                    search_tracks: String::new(), search_waypoints: String::new(),
                    palette: load_palette(), modal: None, track_backup: None, last_track_click: None, 
                    show_color_picker: false,
                    window_size: session.view.window_size.map(|(w, h)| Size::new(w, h)).unwrap_or(Size::new(1130.0, 880.0)),
                    window_pos: session.view.window_pos.map(|(x, y)| Point::new(x, y)).unwrap_or(Point::new(100.0, 100.0)),
                    right_panel_width: 360.0, resizing_panel: false, waypoint_backup: None, last_waypoint_click: None,
                    show_track_save_menu: false, show_wp_save_menu: false,
                    theme, resolved_theme, simplify_tolerance: 2.0, undo_stack: Vec::new(), redo_stack: Vec::new(),
                    modal_drag_offset: Point::new(0.0, 0.0), modal_dragging: false, modal_drag_start_cursor: None,
                    modal_drag_start_offset: Point::new(0.0, 0.0), waypoint_crosshair: None, active_tool: Tool::Pan, space_pan_active: false, 
                    moving_points: false, move_start_cursor: None,  point_press_start: None, 
                    point_pressed_key: None, point_is_dragging: false, rubber_band_start: None, 
                    rubber_band_end: None, last_point_clicked: None, drawing_track: None, freehand_is_drawing: false, ruler_points: Vec::new()  
                };

                for t in &mut app.tracks { 
                    domain::recompute_track_stats(Arc::make_mut(t)); 
                }

                if let Some(kit_path) = &app.last_kit { app.apply_kit(load_kit(kit_path)); }

                app.cx = session.view.cx; app.cy = session.view.cy; app.scale = session.view.scale;
                app.active = session.view.active.min(app.layers.len());
                app.show_grid = session.view.show_grid; app.show_labels = session.view.show_labels;
                app.format = match session.view.format_str.as_str() {
                    "DegMin" => CoordFormat::DegMin, "DegMinSec" => CoordFormat::DegMinSec, _ => CoordFormat::Decimal,
                };

                app.status = format!("Сессия восстановлена ({} треков, {} точек)", app.tracks.len(), app.waypoints.len());
                let task = app.request_osm_tiles();
                return (app, task);
            }
        }

        let mut app = App {
            layers: Vec::new(), active: 0, grid: None, lines: Vec::new(),
            show_grid: true, show_labels: true, declination: 0.0, format: CoordFormat::Decimal,
            cx: 0.0, cy: 0.0, scale: 1.0,
            canvas_cache: Cache::default(), grid_cache: Cache::default(), osm_canvas_cache: Cache::default(), tracks_cache: Cache::default(),
            tile_handles: RefCell::new(HashMap::new()), dragging: false, cursor: None,
            status: String::new(), last_geom_ms: Cell::new(0.0), last_bounds: Cell::new(Size::new(VIEW_W, VIEW_H)),
            osm_cache: osm::OsmCache::new(&cache_cfg), osm_client: osm::client(), osm_z: 0,
            tracks: Vec::new(), waypoints: Vec::new(), next_id: 0,
            selection: Selection::default(),
            last_kit: None, modifiers: iced::keyboard::Modifiers::default(),
            last_track_clicked: None, last_waypoint_clicked: None,
            search_tracks: String::new(), search_waypoints: String::new(),
            palette: load_palette(), modal: None, track_backup: None, last_track_click: None, show_color_picker: false,
            window_size: Size::new(1130.0, 880.0), window_pos: Point::new(100.0, 100.0),
            right_panel_width: 360.0, resizing_panel: false, waypoint_backup: None, last_waypoint_click: None,
            show_track_save_menu: false, show_wp_save_menu: false,
            theme, resolved_theme, simplify_tolerance: 2.0, undo_stack: Vec::new(), redo_stack: Vec::new(),
            modal_drag_offset: Point::new(0.0, 0.0), modal_dragging: false, modal_drag_start_cursor: None,
            modal_drag_start_offset: Point::new(0.0, 0.0), waypoint_crosshair: None, active_tool: Tool::Pan, space_pan_active: false,
            moving_points: false, move_start_cursor: None,  point_press_start: None, 
            point_pressed_key: None, point_is_dragging: false, rubber_band_start: None, 
            rubber_band_end: None, last_point_clicked: None, drawing_track: None, freehand_is_drawing: false, ruler_points: Vec::new()
        };

        let kit_path: Option<PathBuf> = if force_osm {
            None
        } else if let Some(d) = dir_arg {
            Some(PathBuf::from(d))
        } else {
            session::load_last_kit()
        };

        if let Some(p) = kit_path {
            if app.apply_kit(load_kit(&p)) {
                app.last_kit = Some(p.clone());
                session::save_last_kit(&p);
            }
        }

        let task = if app.osm_active() && app.layers.is_empty() {
            let (lon, lat) = (37.6173, 55.7558);
            let (cx, cy) = merc_of(lon, lat);
            app.cx = cx; app.cy = cy; app.scale = 20.0;
            if app.status.is_empty() { app.status = "Источник: OpenStreetMap".into(); } else { app.status = format!("OSM. {}", app.status); }
            app.request_osm_tiles()
        } else {
            Task::none()
        };

        (app, task)
    }

    pub(crate) fn fit_to_layer(&mut self, idx: usize) {
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
        self.cx = cx; self.cy = cy;
        self.scale = 1.05 * ((w0 / ppm) / sz.width as f64).max((h0 / ppm) / sz.height as f64);
    }

    fn screen_to_lonlat(&self, p: Point) -> (f64, f64) {
        let sz = self.last_bounds.get();
        let mx = self.cx + (p.x as f64 - sz.width as f64 / 2.0) * self.scale;
        let my = self.cy - (p.y as f64 - sz.height as f64 / 2.0) * self.scale;
        lonlat_of(mx, my)
    }
    
    pub(crate) fn refresh_status(&mut self) {
        let Some(p) = self.cursor else { return };
        let (lon, lat) = self.screen_to_lonlat(p);
        let mut s = format!("Курсор: {}", format_pair(lat, lon, self.format));
        
        if let Some(g) = &self.grid {
            if let Some(n) = g.nearest(lat, lon, self.declination) {
                s += &format!("   |   ближайший узел {}: {:.0} м, магнитный азимут {:.0}°", n.node.name, n.distance_m, n.magnetic_azimuth);
            }
        }
        
        self.status = s;
    }

    pub(crate) fn apply_kit(&mut self, result: Result<(Vec<Layer>, Option<GridData>, Vec<String>), String>) -> bool {
        self.undo_stack.clear();
        self.redo_stack.clear();
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
                self.canvas_cache.clear(); self.grid_cache.clear(); self.osm_canvas_cache.clear(); self.tracks_cache.clear();
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
            // 1. Отслеживаем нажатия клавиш
            // 1. Транслируем все нажатия клавиш в update
            iced::event::listen_with(|event, _status, _id| {
                if let iced::Event::Keyboard(kb_event) = event {
                    match kb_event {
                        iced::keyboard::Event::KeyPressed { key, modifiers, .. } => {
                            Some(Message::KeyPressed(key, modifiers))
                        }
                        iced::keyboard::Event::KeyReleased { key, .. } => {
                            Some(Message::KeyReleased(key))
                        }
                       iced::keyboard::Event::ModifiersChanged(modifiers) => {
                            Some(Message::ModifiersChanged(modifiers))
                       }
                    }
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

            // 4. Отслеживаем смену системной темы
            iced::system::theme_changes().map(|_| Message::RefreshSystemTheme),
        ];

        // 5. Ловим мышь только пока тянем разделитель или перетаскиваем модальное окно
        if self.resizing_panel || self.modal_dragging {
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
        if self.freehand_is_drawing {
            subs.push(iced::event::listen_with(|event, _status, _id| {
                if let iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) = event {
                    return Some(Message::FreehandReleased);
                }
                None
            }));
        }

        Subscription::batch(subs)
    }

    pub(crate) fn request_osm_tiles(&mut self) -> Task<Message> {
        if !self.osm_active() { return Task::none(); }

        let sz = self.last_bounds.get();
        let (z, tiles) = osm::visible_tiles(self.cx, self.cy, self.scale, sz.width as f64, sz.height as f64);
        self.osm_z = z;

        let mut tasks: Vec<Task<Message>> = Vec::new();
        for id in tiles {
            if self.osm_cache.get(id).is_some() || self.osm_cache.pending.borrow().contains(&id) { continue; }
            if tasks.len() >= 2 { break; }
            self.osm_cache.pending.borrow_mut().insert(id);

            let client = self.osm_client.clone();
            let disk = self.osm_cache.disk_path(id);
            let url = format!("https://tile-a.openstreetmap.fr/hot/{}/{}/{}.png", id.0, id.1, id.2);
            tasks.push(Task::perform(
                async move {
                    if let Ok(bytes) = std::fs::read(&disk) { return (id, Ok(bytes)); }
                    match client.get(&url).send().await {
                        Ok(r) if r.status().is_success() => match r.bytes().await {
                            Ok(b) => {
                                let v = b.to_vec();
                                if let Some(parent) = disk.parent() { let _ = std::fs::create_dir_all(parent); }
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

    pub(crate) fn format_name(&self) -> &'static str {
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
                .get(   self.active)
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
                // Накладываем панель инструментов поверх карты
                stack![
                    container(self.view_map())
                        .width(Length::Fill)
                        .height(Length::Fill),
                    container(self.view_tool_palette())
                        .width(Length::Fixed(56.0))
                        .height(Length::Fill)
                        .style(|_| container::Style::default()), // Полностью прозрачный фон
                ]
                .width(Length::Fill)
                .height(Length::Fill),

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
                
                // Правая панель
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
                        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.15).into()),
                        ..Default::default()
                    }),
                container(modal_ui)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .style(|_| container::Style::default()) // чтобы фон не растягивался
                    .padding(modal_drag_padding(self.modal_drag_offset))
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
                        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.15).into()),
                        ..Default::default()
                    }),
                container(modal_ui)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .style(|_| container::Style::default()) // чтобы фон не растягивался
                    .padding(modal_drag_padding(self.modal_drag_offset))
            ]
            .into()
        } else if let Some(Modal::TrackList(state)) = &self.modal {
            // НОВЫЙ БЛОК ДЛЯ ТАБЛИЦЫ ТОЧЕК
            let modal_ui = self.view_track_list_modal(state);
            
            stack![
                base_ui,
                container(text(""))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.15).into()),
                        ..Default::default()
                    }),
                container(modal_ui)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .style(|_| container::Style::default()) // чтобы фон не растягивался
                    .padding(modal_drag_padding(self.modal_drag_offset))
            ]
            .into()
        } else {
            base_ui.into()
        }
    }

    fn view_map(&self) -> Element<'_, Message> {
        let map_canvas = Canvas::new(canvas::MapProgram { app: self }).width(Length::Fill).height(Length::Fill);
        let tracks_canvas = Canvas::new(canvas::TrackProgram { app: self }).width(Length::Fill).height(Length::Fill);
        let grid_canvas = Canvas::new(canvas::GridProgram { app: self }).width(Length::Fill).height(Length::Fill);

        mouse_area(stack![map_canvas, tracks_canvas, grid_canvas])
            .on_move(Message::Moved).on_scroll(Message::Scrolled).into()
    }
}

fn main() -> iced::Result {
    let session = session::load_session(); 
    
    let mut window_settings = iced::window::Settings {
        size: Size::new(1130.0, 880.0),
        min_size: Some(Size::new(640.0, 480.0)),
        exit_on_close_request: false,
        ..Default::default()
    };

    if let Some(s) = &session {
        if let Some((w, h)) = s.view.window_size { window_settings.size = Size::new(w, h); }
        if let Some((x, y)) = s.view.window_pos { window_settings.position = iced::window::Position::Specific(Point::new(x, y)); }
    }

    let (initial_theme, initial_resolved) = 
        if let Ok((t, r, _, _)) = native_theme_iced::from_system() {
            (t, r)
        } else if let Ok((t, r)) = native_theme_iced::from_preset("catppuccin-mocha", true) {
            (t, r)
        } else {
            (iced::Theme::Dark, native_theme_iced::from_preset("default", true).unwrap().1)
        };

    iced::application(
        move || App::new(initial_theme.clone(), initial_resolved.clone()), 
        App::update, 
        App::view
    )
    .title("FoxyXplorer")
    .window(window_settings)
    .subscription(App::subscription)
    .theme(|app: &App| app.theme.clone())
    .run()
}
