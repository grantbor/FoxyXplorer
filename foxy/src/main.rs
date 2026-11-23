#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod app; 
mod foxydrop;
mod canvas;
mod domain;
mod io;
mod osm;
mod session;
mod ui;
mod util;
mod widgets;
pub mod i18n;
pub mod config;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use coord::{format_pair, CoordFormat};
use iced::mouse::ScrollDelta;
use iced::widget::canvas::{Cache, Canvas};
use iced::widget::image::Handle as ImageHandle;
use iced::widget::{column, container, mouse_area, row, stack, text, Space};
use iced::{Alignment, Color, Element, Length, Point, Size, Task};
use iced::keyboard::Modifiers;
use iced::Subscription;
use iced::window::settings::PlatformSpecific;
 

use util::geo::{merc_of, lonlat_of, affine};
use util::coords::today;
use widgets::palette::load_palette;
use util::icon::load_icon_from_svg;

pub(crate) const ICON_SVG: &[u8] = include_bytes!("../assets/icon.svg");

// Two-color SVG for the cursor: 4-unit white stroke under a 2-unit black
// stroke. The glyph geometry and its 24x24 viewBox do not change.
// Lucide-generated SVG icons contain path geometry inside a standard <svg> root.
fn outlined_tool_cursor_svg(source: &[u8]) -> Vec<u8> {
    let Ok(svg) = std::str::from_utf8(source) else {
        return source.to_vec();
    };
    let Some(start) = svg.find('>').map(|i| i + 1) else {
        return source.to_vec();
    };
    let Some(end) = svg.rfind("</svg>") else {
        return source.to_vec();
    };
    let shapes = &svg[start..end];
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke-linecap="round" stroke-linejoin="round">
<g stroke="#FFFFFF" stroke-width="4">{shapes}</g>
<g stroke="#000000" stroke-width="2">{shapes}</g>
</svg>"##
    )
    .into_bytes()
}

const VIEW_W: f32 = 1100.0;
const VIEW_H: f32 = 720.0;
pub(crate) const DEFAULT_CENTER_LON: f64 = 82.945971;
pub(crate) const DEFAULT_CENTER_LAT: f64 = 55.016359;
pub(crate) const DEFAULT_SCALE: f64 = 20.0;

#[derive(Default)]
pub(crate) struct Selection {
    pub(crate) tracks: HashSet<u64>,
    pub(crate) waypoints: HashSet<u64>,
    pub(crate) points: HashSet<(u64, usize, usize)>,
}

#[derive(Debug, Clone)]
pub(crate) enum ContextMenuItem {
    Action(String, Message),
    Label(String),
    Separator,
}

#[derive(Debug, Clone)]
pub(crate) struct ContextMenuState {
    pub x: f32,
    pub y: f32,
    pub items: Vec<ContextMenuItem>,
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

// Fixed row geometry allows scroll_to to center a selected table row reliably.
pub(crate) const TRACK_LIST_ROW_HEIGHT: f32 = 28.0;
pub(crate) const TRACK_LIST_ROW_SPACING: f32 = 1.0;
pub(crate) const TRACK_LIST_VIEW_HEIGHT: f32 = 400.0;
pub(crate) const TRACK_LIST_SCROLL_ID: &str = "track-list-points";

pub(crate) struct TrackListState {
    pub track_id: u64,
    pub rows: Vec<TrackRowData>,
    pub selected_points: HashSet<(usize, usize)>, // (sect, pt_idx)
    pub last_clicked: Option<(usize, usize)>,
}

pub(crate) struct SettingsDraft {
    pub coord_input: String,      // единое поле ввода координат
    pub parsed_lat: Option<f64>,  // результат последнего успешного парсинга
    pub parsed_lon: Option<f64>,
    pub lang: String,
    pub simplify_tolerance: f64,
    pub grid_color: [u8; 3],
    pub drop_folder: String,
    pub ui_theme: iced::Theme,
    pub original_grid_color: [u8; 3],
    pub original_ui_theme: iced::Theme,
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
            "{} (.map + .ozf2).\n{} {}.\n{}",
            t!("msg_0222"), t!("msg_0223"), map_dirs.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "),
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
    let mut best_source: Option<PathBuf> = None;
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
            best_source = Some(p.clone());
        }
    }
    if best.is_none() {
        notes.push(format!(
            "сетка (GPX с именованными точками) не найдена в: {}",
            data_dirs.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
        ));
    }

    if let Some(source) = best_source {
        if let Err(e) = copy_kit_grid_if_missing(root, &source) {
            // Copying is secondary: failure must not prevent the kit from opening.
            notes.push(format!("Не удалось скопировать сетку в 10-Tracks: {e}"));
        }
    }
    Ok((layers, best, notes))
}

// Keep the original grid GPX alongside the kit tracks, without overwriting
// a grid (possibly with a different filename) already stored there.
fn copy_kit_grid_if_missing(root: &Path, source: &Path) -> Result<(), String> {
    let tracks_dir = root.join("10-Tracks");

    if tracks_dir.is_dir() {
        let entries = std::fs::read_dir(&tracks_dir)
            .map_err(|e| format!("{}: {e}", tracks_dir.display()))?;
        for entry in entries {
            let path = entry.map_err(|e| e.to_string())?.path();
            if !path.is_file()
                || !path.extension().and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("gpx"))
            {
                continue;
            }
            let bytes = std::fs::read(&path)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            if grid::Grid::from_gpx(&geoio::text::decode(&bytes)).is_ok() {
                return Ok(());
            }
        }
    }

    std::fs::create_dir_all(&tracks_dir)
        .map_err(|e| format!("{}: {e}", tracks_dir.display()))?;
    let name = source.file_name().ok_or_else(|| "У сетки нет имени файла".to_string())?;
    let destination = tracks_dir.join(name);
    let mut input = std::fs::File::open(source)
        .map_err(|e| format!("{}: {e}", source.display()))?;
    // create_new ensures an existing file is never replaced, even if it
    // appears between our directory scan and the copy.
    let mut output = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
        Err(e) => return Err(format!("{}: {e}", destination.display())),
    };
    if let Err(e) = std::io::copy(&mut input, &mut output) {
        drop(output);
        let _ = std::fs::remove_file(&destination);
        return Err(format!("{}: {e}", destination.display()));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    Layer(usize), ToggleGrid, ToggleLabels, CycleFormat,
    Moved(Point), MapCursorExited, Scrolled(ScrollDelta), Fit,
    PickKit, KitPicked(Option<PathBuf>),
    OsmTileLoaded(BasemapSource, osm::TileId, Result<Vec<u8>, String>),
    PickTrack, TracksPicked(Option<Vec<PathBuf>>),
    ToggleTrackVisible(u64, bool), SelectTrack(u64), ToggleAllTracksVisible, DeleteSelectedTracks,
    SaveTracksPlt, SaveTracksPltPicked(Option<PathBuf>),
    SaveTracksGpx, SaveTracksGpxPicked(Option<PathBuf>),
    SaveTracksMs, SaveTracksMsPicked(Option<PathBuf>),
    SaveTracksKml, SaveTracksKmlPicked(Option<PathBuf>),
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
    ApplyTrackProps, ToggleColorPicker(ColorPickerKind),
    WindowResized(Size), WindowMoved(Point), DividerPressed, DividerReleased, CursorMoved(Point),
    OpenWaypointProps(u64), CloseWaypointProps, ApplyWaypointProps,
    DraftWpNameChanged(String), DraftWpDescChanged(String), DraftWpEleChanged(String),
    DraftWpTextColorChanged([u8; 3]), DraftWpBgColorChanged([u8; 3]), DraftWpCoordsChanged(String),
    AddNewWaypoint, ToggleTrackSaveMenu, ToggleWpSaveMenu, RefreshSystemTheme, SimplifyTracks, SplitTrackBySegments, MergeTracks,
    OpenTrackList(u64),
    CloseTrackList,
    SelectTrackPoint(usize, usize), // (sect, pt_idx)
    DeleteSelectedTrackPoints,
    CenterOnTrackPoint(usize, usize),
    SplitTrackAtPoint(usize, usize),
    StitchSegments,
    SelectPointsOutsideGrid,
    ModalDragStart, TrackListUp, TrackListDown, FitTrackToScreen,CenterOnWaypoint(u64),
    ClearWaypointCrosshair,
    SelectTool(Tool),
    MapPressed(Point), MapReleased(Point),
    MapMiddlePressed(Point), MapMiddleReleased,
    DeleteSelectedPoints,
    KeyPressed(iced::keyboard::Key, Modifiers),
    KeyReleased(iced::keyboard::Key),
    FreehandReleased,
    MeasureWaypoints,
    CloseWaypointMeasure,
    SaveSessionAs,
    SaveSessionAsPicked(Option<PathBuf>),
    LoadSessionPrompt,
    LoadSessionNoPrompt,
    LoadSessionPicked(Option<PathBuf>),
    SaveAndLoadSession, 
    SaveAndLoadSessionSaved(Option<PathBuf>),
    ConfirmClearAll,
    ExecuteClearAll,
    CancelClearAll,
    // Контекстное меню
    GlobalCursorMoved(Point),
    ShowContextMenu(Point, Vec<ContextMenuItem>),
    ContextMenuClose,
    ContextMenuSelect(usize),

    // Действия из контекстного меню
    DeleteWaypointById(u64),
    DeleteTrackById(u64),
    FitTrackById(u64),
    AddWaypointAt(f64, f64),  // lon, lat
    CopyCoordinates(f64, f64),  // lat, lon
    SplitTrackAt(u64, usize, usize), // track_id, seg_idx, pt_idx
    OpenWaypointProjection(u64),
    CloseWaypointProjection,
    ProjectAzimuthChanged(String),
    ProjectDistanceChanged(String),
    ExecuteProjection,
    ToggleTheme,
    AutoSaveTick,
    OpenAbout,
    CloseAbout,
    OpenUrl(String),
    OpenSettings,
    CloseSettings,
    ApplySettings,
    SettingsCoordChanged(String),
    SettingsLangChanged(String),
    SettingsToleranceChanged(f64),
    SettingsGridColorChanged([u8; 3]),
    SettingsThemeChanged(iced::Theme),
    SettingsChooseDropFolder,
    SettingsDropFolderPicked(Option<PathBuf>),
    OpenFoxyDrop,
    CloseFoxyDrop,
    FoxyDropStart,
    FoxyDropStop,
    FoxyDropIpSelected(std::net::Ipv4Addr),
    FoxyDropStarted(Result<std::net::SocketAddr, String>),
    FoxyDropStopped,
    FoxyDropHeartbeat,
    FoxyDropCopyUrl,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BasemapSource {
    Osm,
    Sentinel2,
    Esri,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppTheme {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColorPickerKind {
    LineColor,
    FillColor,
    WpTextColor,
    WpBgColor,
    GridColor,
}

pub(crate) struct DrawingTrack {
    pub points: Vec<(f64, f64)>, // (lon, lat)
    pub target_track_id: Option<u64>,
}

// В src/main.rs
pub(crate) fn build_session(app: &App) -> session::Session {
    let format_str = match app.format {
        CoordFormat::Decimal => "Decimal",
        CoordFormat::DegMin => "DegMin",
        CoordFormat::DegMinSec => "DegMinSec",
    }.to_string();


    
    session::Session {
        version: 1,
        last_kit: app.last_kit.clone(),
        last_open_dir: app.last_open_dir.clone(),
        last_save_dir: app.last_save_dir.clone(),
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
       theme: Some(match app.current_theme {
            AppTheme::Light => "light".to_string(),
            AppTheme::Dark  => "dark".to_string(),
        }),
        tracks: app.tracks.iter().map(|t| t.as_ref().clone()).collect(),
        waypoints: app.waypoints.iter().map(|w| w.as_ref().clone()).collect(),
        next_id: app.next_id,
    }
}

pub(crate) fn save_session(app: &App) {
    let sess = build_session(app);
    session::write_session(&sess);
}

pub(crate) struct App {
    pub(crate) layers: Vec<Layer>,
    pub(crate) active: usize,
    pub(crate) grid: Option<grid::Grid>,
    pub(crate) lines: Vec<Vec<(f64, f64)>>,
    pub(crate) show_grid: bool,
    pub(crate) grid_color: [u8; 3],
    pub(crate) drop_service: Arc<foxydrop::DropService>,
    pub(crate) drop_phase: foxydrop::Phase,
    pub(crate) drop_address: Option<std::net::SocketAddr>,
    pub(crate) drop_ips: Vec<std::net::Ipv4Addr>,
    pub(crate) drop_selected_ip: Option<std::net::Ipv4Addr>,
    pub(crate) drop_error: Option<String>,
    pub(crate) drop_restart_after_stop: bool,
    pub(crate) show_labels: bool,
    pub(crate) compact_view: bool,
    pub(crate) declination: f64,
    pub(crate) last_declination_calc: Option<std::time::Instant>,
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
    pub(crate) middle_panning: bool,
    pub(crate) cursor: Option<Point>,
    pub(crate) map_cursor_inside: bool,
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
    pub(crate) last_open_dir: Option<PathBuf>,
    pub(crate) last_save_dir: Option<PathBuf>,
    pub(crate) palette: Vec<[u8; 3]>,
    pub(crate) modal: Option<Modal>,
    pub(crate) track_backup: Option<domain::Track>, 
    pub(crate) last_track_click: Option<(u64, std::time::Instant)>,
    pub(crate) show_color_picker: Option<ColorPickerKind>,
    pub(crate) window_size: Size,
    pub(crate) window_pos: Point,
    pub(crate) right_panel_width: f32,
    pub(crate) resizing_panel: bool,
    pub(crate) waypoint_backup: Option<domain::Waypoint>,
    pub(crate) last_waypoint_click: Option<(u64, std::time::Instant)>,
    pub(crate) show_track_save_menu: bool,
    pub(crate) show_wp_save_menu: bool,
    pub(crate) theme: iced::Theme,
    pub(crate) current_theme: AppTheme,
    pub(crate) ui_theme: iced::Theme,

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
    pub(crate) context_menu: Option<ContextMenuState>,
    pub(crate) global_cursor: Point,
    pub(crate) search_focused: bool,
    pub(crate) tile_fail_count: std::collections::HashMap<(u8, u32, u32), u8>,
}

pub(crate) enum Modal {
    TrackProps { track_id: u64, draft: domain::TrackProps },
    WaypointProps { waypoint_id: u64, draft: domain::Waypoint, coord_input: String, is_new: bool },
    WaypointMeasure {
        from_name: String,
        from_lat: f64,
        from_lon: f64,
        to_name: String,
        to_lat: f64,
        to_lon: f64,
        dist_m: f64,
        mag_az: f64,
    },
    TrackList(TrackListState),
    ConfirmClearAll,
    ConfirmLoadSession,
    WaypointProjection {
        from_id: u64,
        azimuth_input: String,
        distance_input: String,
    },
    About,
    //Settings,
    Settings { draft: SettingsDraft },
    FoxyDrop,
}

fn modal_drag_padding(offset: Point) -> iced::Padding {
    let mut pad = iced::Padding::new(0.0);
    if offset.y > 0.0 { pad = pad.top(offset.y * 2.0); } else { pad = pad.bottom(-offset.y * 2.0); }
    if offset.x > 0.0 { pad = pad.left(offset.x * 2.0); } else { pad = pad.right(-offset.x * 2.0); }
    pad
}




/// One entry per theme family in Settings.
pub(crate) const THEME_FAMILY_LABELS: &[&str] = &[
    "Default", "Solarized", "Gruvbox",
    "Catppuccin Frappé", "Catppuccin Macchiato", "Catppuccin Mocha",
    "Tokyo Night", "Tokyo Night Storm", "Kanagawa", "Kanagawa Dragon",
    "Dracula", "Nord", "Moonfly", "Nightfly", "Oxocarbon", "Ferra",
];

/// Map old saved theme variants to their single dropdown family.
pub(crate) fn theme_family_name(theme: &iced::Theme) -> &'static str {
    use iced::Theme;
    match theme {
        Theme::Light | Theme::Dark => "Default",
        Theme::SolarizedLight | Theme::SolarizedDark => "Solarized",
        Theme::GruvboxLight | Theme::GruvboxDark => "Gruvbox",
        Theme::CatppuccinLatte | Theme::CatppuccinMocha => "Catppuccin Mocha",
        Theme::CatppuccinFrappe => "Catppuccin Frappé",
        Theme::CatppuccinMacchiato => "Catppuccin Macchiato",
        Theme::TokyoNightLight | Theme::TokyoNight => "Tokyo Night",
        Theme::TokyoNightStorm => "Tokyo Night Storm",
        Theme::KanagawaLotus | Theme::KanagawaWave => "Kanagawa",
        Theme::KanagawaDragon => "Kanagawa Dragon",
        Theme::Dracula => "Dracula",
        Theme::Nord => "Nord",
        Theme::Moonfly => "Moonfly",
        Theme::Nightfly => "Nightfly",
        Theme::Oxocarbon => "Oxocarbon",
        Theme::Ferra => "Ferra",
        Theme::Custom(_) => "Default",
    }
}

/// Dark representatives are converted to Light by the existing theme_for().
pub(crate) fn theme_from_family(family: &str) -> iced::Theme {
    use iced::Theme;
    match family {
        "Default" => Theme::Dark,
        "Solarized" => Theme::SolarizedDark,
        "Gruvbox" => Theme::GruvboxDark,
        "Catppuccin Frappé" => Theme::CatppuccinFrappe,
        "Catppuccin Macchiato" => Theme::CatppuccinMacchiato,
        "Catppuccin Mocha" => Theme::CatppuccinMocha,
        "Tokyo Night" => Theme::TokyoNight,
        "Tokyo Night Storm" => Theme::TokyoNightStorm,
        "Kanagawa" => Theme::KanagawaWave,
        "Kanagawa Dragon" => Theme::KanagawaDragon,
        "Dracula" => Theme::Dracula,
        "Nord" => Theme::Nord,
        "Moonfly" => Theme::Moonfly,
        "Nightfly" => Theme::Nightfly,
        "Oxocarbon" => Theme::Oxocarbon,
        "Ferra" => Theme::Ferra,
        _ => Theme::CatppuccinFrappe,
    }
}

/// Iced's built-in themes (excluding Custom). Stored in config.json by display name.
pub(crate) fn ui_theme_from_name(name: &str) -> iced::Theme {
    iced::Theme::ALL.iter()
        .find(|theme| theme.to_string() == name)
        .cloned()
        .unwrap_or(iced::Theme::CatppuccinFrappe)
}

/// Returns a single iced theme for the chosen family and Light/Dark mode.
/// Families without a second variant fall back to iced::Theme::Light in light mode.
pub(crate) fn theme_for(selected: &iced::Theme, mode: AppTheme) -> iced::Theme {
    use iced::Theme;
    match (selected, mode) {
        (Theme::Light | Theme::Dark, AppTheme::Light) => Theme::Light,
        (Theme::Light | Theme::Dark, AppTheme::Dark) => Theme::Dark,
        (Theme::SolarizedLight | Theme::SolarizedDark, AppTheme::Light) => Theme::SolarizedLight,
        (Theme::SolarizedLight | Theme::SolarizedDark, AppTheme::Dark) => Theme::SolarizedDark,
        (Theme::GruvboxLight | Theme::GruvboxDark, AppTheme::Light) => Theme::GruvboxLight,
        (Theme::GruvboxLight | Theme::GruvboxDark, AppTheme::Dark) => Theme::GruvboxDark,
        (Theme::CatppuccinLatte, AppTheme::Light) => Theme::CatppuccinLatte,
        (Theme::CatppuccinLatte, AppTheme::Dark) => Theme::CatppuccinMocha,
        (Theme::CatppuccinFrappe, AppTheme::Light) => Theme::CatppuccinLatte,
        (Theme::CatppuccinFrappe, AppTheme::Dark) => Theme::CatppuccinFrappe,
        (Theme::CatppuccinMacchiato, AppTheme::Light) => Theme::CatppuccinLatte,
        (Theme::CatppuccinMacchiato, AppTheme::Dark) => Theme::CatppuccinMacchiato,
        (Theme::CatppuccinMocha, AppTheme::Light) => Theme::CatppuccinLatte,
        (Theme::CatppuccinMocha, AppTheme::Dark) => Theme::CatppuccinMocha,
        (Theme::TokyoNightLight | Theme::TokyoNight, AppTheme::Light) => Theme::TokyoNightLight,
        (Theme::TokyoNightLight | Theme::TokyoNight, AppTheme::Dark) => Theme::TokyoNight,
        (Theme::TokyoNightStorm, AppTheme::Light) => Theme::TokyoNightLight,
        (Theme::TokyoNightStorm, AppTheme::Dark) => Theme::TokyoNightStorm,
        (Theme::KanagawaWave | Theme::KanagawaLotus, AppTheme::Light) => Theme::KanagawaLotus,
        (Theme::KanagawaWave | Theme::KanagawaLotus, AppTheme::Dark) => Theme::KanagawaWave,
        (Theme::KanagawaDragon, AppTheme::Light) => Theme::KanagawaLotus,
        (Theme::KanagawaDragon, AppTheme::Dark) => Theme::KanagawaDragon,
        (Theme::Custom(_), AppTheme::Light) => Theme::Light,
        (Theme::Custom(_), AppTheme::Dark) => Theme::Dark,
        (_, AppTheme::Light) => Theme::Light,
        (theme, AppTheme::Dark) => theme.clone(),
    }
}


impl App {
    pub(crate) fn online_basemap(&self) -> Option<BasemapSource> {
        if self.active == self.layers.len() {
            Some(BasemapSource::Osm)
        } else if self.active == self.layers.len() + 1 {
            Some(BasemapSource::Sentinel2)
        } else if self.active == self.layers.len() + 2 {
            Some(BasemapSource::Esri)
        } else {
            None
        }
    }

    pub(crate) fn osm_active(&self) -> bool {
        self.online_basemap().is_some()
    }
    
    fn tile_url(&self, id: osm::TileId) -> String {
        match self.online_basemap() {
            Some(BasemapSource::Osm) => format!(
                "https://tile-a.openstreetmap.fr/hot/{}/{}/{}.png",
                id.0, id.1, id.2
            ),
            Some(BasemapSource::Sentinel2) => format!(
                "https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless-2024_3857/default/g/{}/{}/{}.jpg",
                id.0, id.2, id.1   // ← ВАЖНО: {z}/{y}/{x}, не {z}/{x}/{y}
            ),
            Some(BasemapSource::Esri) => format!(
                "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{}/{}/{}.jpg",
                id.0, id.2, id.1   // порядок {z}/{y}/{x}, как у Sentinel-2
            ),
            None => String::new(),
        }
    }
    fn format_short_name(&self) -> &'static str {
        match self.format {
            CoordFormat::Decimal => "D",
            CoordFormat::DegMin => "DM",
            CoordFormat::DegMinSec => "DMS",
        }
    }

    fn new(initial_theme_kind: AppTheme) -> (App, Task<Message>) {
        let args: Vec<String> = std::env::args().collect();
        // --- Читаем пользовательский конфиг ---
        let cfg = crate::config::load();
        let ui_theme = ui_theme_from_name(&cfg.ui_theme);
        let theme = theme_for(&ui_theme, initial_theme_kind);
        // Применяем язык сразу, до создания App
        crate::i18n::set_lang(Box::leak(cfg.lang.clone().into_boxed_str()));
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
                    show_grid: true, grid_color: cfg.grid_color, show_labels: true, declination: 0.0, format: CoordFormat::Decimal,
                    drop_service: Arc::new(foxydrop::DropService::new(PathBuf::from(&cfg.drop_folder))),
                    drop_phase: foxydrop::Phase::Stopped,
                    drop_address: None,
                    drop_ips: foxydrop::local_ipv4_addresses(),
                    drop_selected_ip: None,
                    drop_error: None,
                    drop_restart_after_stop: false,
                    cx: 0.0, cy: 0.0, scale: 1.0,
                    canvas_cache: Cache::default(), grid_cache: Cache::default(), osm_canvas_cache: Cache::default(),
                    tracks_cache: Cache::default(),
                    tile_handles: RefCell::new(HashMap::new()), dragging: false, middle_panning: false, cursor: None, map_cursor_inside: false,
                    status: String::new(), last_geom_ms: Cell::new(0.0), last_bounds: Cell::new(Size::new(VIEW_W, VIEW_H)),
                    osm_cache: osm::OsmCache::new(&cache_cfg), osm_client: osm::client(), osm_z: 0,
                    tracks: session.tracks.into_iter().map(Arc::new).collect(), 
                    waypoints: session.waypoints.into_iter().map(Arc::new).collect(), 
                    next_id: session.next_id, selection: Selection::default(),
                    last_kit: session.last_kit.clone(), modifiers: iced::keyboard::Modifiers::default(),
                    last_track_clicked: None, last_waypoint_clicked: None,
                    search_tracks: String::new(), search_waypoints: String::new(), search_focused: false,
                    palette: load_palette(), modal: None, track_backup: None, last_track_click: None, 
                    show_color_picker: None,
                    window_size: session.view.window_size.map(|(w, h)| Size::new(w, h)).unwrap_or(Size::new(1130.0, 880.0)),
                    window_pos: session.view.window_pos.map(|(x, y)| Point::new(x, y)).unwrap_or(Point::new(100.0, 100.0)),
                    right_panel_width: 360.0, resizing_panel: false, waypoint_backup: None, last_waypoint_click: None,
                    show_track_save_menu: false, show_wp_save_menu: false,
                    theme, simplify_tolerance: cfg.simplify_tolerance, undo_stack: Vec::new(), redo_stack: Vec::new(),
                    modal_drag_offset: Point::new(0.0, 0.0), modal_dragging: false, modal_drag_start_cursor: None,
                    modal_drag_start_offset: Point::new(0.0, 0.0), waypoint_crosshair: None, active_tool: Tool::Pan, space_pan_active: false, 
                    moving_points: false, move_start_cursor: None,  point_press_start: None, 
                    point_pressed_key: None, point_is_dragging: false, rubber_band_start: None, 
                    rubber_band_end: None, last_point_clicked: None, drawing_track: None, freehand_is_drawing: false, ruler_points: Vec::new(), context_menu: None, global_cursor: Point::ORIGIN, tile_fail_count: HashMap::new(), last_declination_calc: None,
                    current_theme: initial_theme_kind, ui_theme: ui_theme.clone(), compact_view: false, last_open_dir: None, last_save_dir: None,
                
                };

                for t in &mut app.tracks { 
                    domain::recompute_track_stats(Arc::make_mut(t)); 
                }

                if let Some(kit_path) = &app.last_kit { app.apply_kit(load_kit(kit_path)); }

                app.cx = session.view.cx; app.cy = session.view.cy; app.scale = session.view.scale;
                // При восстановлении сессии любой онлайн-источник сводится к OSM.
                // Локальные карты (active < layers.len()) сохраняются как были.
                app.active = session.view.active.min(app.layers.len());
                app.osm_cache.set_source(crate::BasemapSource::Osm);
                app.show_grid = session.view.show_grid; app.show_labels = session.view.show_labels;
                app.format = match session.view.format_str.as_str() {
                    "DegMin" => CoordFormat::DegMin, "DegMinSec" => CoordFormat::DegMinSec, _ => CoordFormat::Decimal,
                };
                if let Some(t) = &session.theme {
                    app.current_theme = match t.as_str() {
                        "light" => AppTheme::Light,
                        _       => AppTheme::Dark,
                    };
                    app.theme = theme_for(&app.ui_theme, app.current_theme);
                }
                app.status = format!("{} ({} {} {} {})", t!("msg_0224"), t!("msg_0225"), app.tracks.len(), t!("msg_0226"), app.waypoints.len());
                let task = app.request_osm_tiles();
                app.maybe_update_declination(true);
                return (app, task);
            }
        }

        let mut app = App {
            layers: Vec::new(), active: 0, grid: None, lines: Vec::new(),
            show_grid: true, grid_color: cfg.grid_color, show_labels: true, declination: 0.0, format: CoordFormat::Decimal,
            drop_service: Arc::new(foxydrop::DropService::new(PathBuf::from(&cfg.drop_folder))),
            drop_phase: foxydrop::Phase::Stopped,
            drop_address: None,
            drop_ips: foxydrop::local_ipv4_addresses(),
            drop_selected_ip: None,
            drop_error: None,
            drop_restart_after_stop: false,
            cx: 0.0, cy: 0.0, scale: 1.0,
            canvas_cache: Cache::default(), grid_cache: Cache::default(), osm_canvas_cache: Cache::default(), tracks_cache: Cache::default(),
            tile_handles: RefCell::new(HashMap::new()), dragging: false, middle_panning: false, cursor: None, map_cursor_inside: false,
            status: String::new(), last_geom_ms: Cell::new(0.0), last_bounds: Cell::new(Size::new(VIEW_W, VIEW_H)),
            osm_cache: osm::OsmCache::new(&cache_cfg), osm_client: osm::client(), osm_z: 0,
            tracks: Vec::new(), waypoints: Vec::new(), next_id: 0,
            selection: Selection::default(),
            last_kit: None, modifiers: iced::keyboard::Modifiers::default(),
            last_track_clicked: None, last_waypoint_clicked: None,
            search_tracks: String::new(), search_waypoints: String::new(), search_focused: false,
            palette: load_palette(), modal: None, track_backup: None, last_track_click: None, show_color_picker: None,
            window_size: Size::new(1130.0, 880.0), window_pos: Point::new(100.0, 100.0),
            right_panel_width: 360.0, resizing_panel: false, waypoint_backup: None, last_waypoint_click: None,
            show_track_save_menu: false, show_wp_save_menu: false,
            theme, simplify_tolerance: cfg.simplify_tolerance, undo_stack: Vec::new(), redo_stack: Vec::new(),
            modal_drag_offset: Point::new(0.0, 0.0), modal_dragging: false, modal_drag_start_cursor: None,
            modal_drag_start_offset: Point::new(0.0, 0.0), waypoint_crosshair: None, active_tool: Tool::Pan, space_pan_active: false,
            moving_points: false, move_start_cursor: None,  point_press_start: None, 
            point_pressed_key: None, point_is_dragging: false, rubber_band_start: None, 
            rubber_band_end: None, last_point_clicked: None, drawing_track: None, freehand_is_drawing: false, ruler_points: Vec::new(), context_menu: None, global_cursor: Point::ORIGIN, tile_fail_count: HashMap::new(), last_declination_calc: None,
            current_theme: initial_theme_kind, ui_theme: ui_theme.clone(), compact_view: false, last_open_dir: None, last_save_dir: None,
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
                app.last_kit = Some(p.to_path_buf());
                session::save_last_kit(&p);
            }
        }
        
        let task = if app.osm_active() && app.layers.is_empty() {
            let (lon, lat) = (cfg.default_lon, cfg.default_lat);
            let (cx, cy) = merc_of(lon, lat);
            app.cx = cx; app.cy = cy; app.scale = DEFAULT_SCALE;
            if app.status.is_empty() { app.status = t!("msg_0012").into(); } else { app.status = format!("OSM. {}", app.status); }
            app.request_osm_tiles()
        } else {
            Task::none()
        };
        app.maybe_update_declination(true); 

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
        let mut s = format!("{} {}", t!("msg_0208"), format_pair(lat, lon, self.format));
        
        if let Some(g) = &self.grid {
            if let Some(n) = g.nearest(lat, lon, self.declination) {
                s += &format!("   |   {} {}: {:.0} м, {}  {:.0}°", t!("msg_0209"), n.node.name, n.distance_m, t!("msg_0210"), n.magnetic_azimuth);
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
                    //format!({} "слоёв: {n_layers}, точек GPX: {n_gpx}")
                    format!("{} {}, {} {}", t!("msg_0211"), {n_layers}, t!("msg_0212"), {n_gpx})
                } else {
                    format!("{} {}, {} {}, {}", t!("msg_0211"), {n_layers}, t!("msg_0212"), {n_gpx}, notes.join("; "))
                };
                self.fit_to_layer(0);
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
                //self.status = format!("Ошибка загрузки: {e}");
                self.status = format!("{} {}", t!("msg_0211"), {e});
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
            
            // Отслеживаем глобальное движение курсора для контекстного меню
            iced::event::listen_with(|event, _status, _id| {
                match event {
                    iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                        Some(Message::GlobalCursorMoved(position))
                    }
                    iced::Event::Mouse(
                        iced::mouse::Event::ButtonReleased(iced::mouse::Button::Middle)
                    ) => Some(Message::MapMiddleReleased),
                    _ => None,
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
            
            // 5. Автосохранение раз в 5 минут
            iced::time::every(std::time::Duration::from_secs(300)).map(|_| Message::AutoSaveTick),

            // 4. Отслеживаем смену системной темы
            iced::system::theme_changes().map(|_| Message::RefreshSystemTheme),
        ];

        if self.drop_phase == foxydrop::Phase::Running {
            subs.push(iced::time::every(std::time::Duration::from_secs(2))
                .map(|_| Message::FoxyDropHeartbeat));
        }

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

        let source = match self.online_basemap() {
            Some(s) => s,
            None => return Task::none(),
        };

        let sz = self.last_bounds.get();
        let max_z = match source {
            BasemapSource::Sentinel2 => Some(14u8),
            BasemapSource::Esri => Some(19u8),
            BasemapSource::Osm => None,
        };
        let (z, tiles) = osm::visible_tiles(
            self.cx, self.cy, self.scale,
            sz.width as f64, sz.height as f64,
            max_z,
        );
        self.osm_z = z;

        let mut tasks: Vec<Task<Message>> = Vec::new();
        for id in tiles {
            if self.osm_cache.get(id).is_some() || self.osm_cache.pending.borrow().contains(&id) {
                continue;
            }
            if tasks.len() >= 4 { break; }
            self.osm_cache.pending.borrow_mut().insert(id);

            let client = self.osm_client.clone();
            let disk = self.osm_cache.disk_path(id);
            let url = self.tile_url(id);

            tasks.push(Task::perform(
                async move {
                    if let Ok(bytes) = std::fs::read(&disk) {
                        return (id, Ok(bytes));
                    }
                    match client.get(&url).send().await {
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
                move |(id, result)| Message::OsmTileLoaded(source, id, result),
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
        let name = match self.online_basemap() {
            Some(src) => {
                let pending = self.osm_cache.pending.borrow().len();
                let cached = self.osm_cache.mem.borrow().len();
                let label = match src {
                    BasemapSource::Osm => "OSM",
                    BasemapSource::Sentinel2 => "Sentinel-2",
                    BasemapSource::Esri => "ESRI",
                };
                format!("{} z{} ({} {}, {} {})", label, self.osm_z, t!("msg_0218"), cached, t!("msg_0219"), pending)
            }
            None => self.layers
                .get(self.active)
                .map(|l| l.name.clone())
                .unwrap_or_else(|| "-".into()),
        };
        
        let info = format!("{} {:.2}° (WMM2025)  |  {} {:.2} {}  |  {} {}", t!("msg_0214"), self.declination, t!("msg_0215"), self.scale, t!("msg_0216"), t!("msg_0217"), name
        );

        // ---- Базовый интерфейс ----
        let base_ui: Element<'_, Message> = {
            let map_stack: Element<Message> = stack![
                container(self.view_map()).width(Length::Fill).height(Length::Fill),
                container(self.view_tool_palette())
                    .width(Length::Fixed(56.0))
                    .height(Length::Fill)
                    .style(|_| container::Style::default()),
            ].width(Length::Fill).height(Length::Fill).into();

            let map_row: Element<Message> = if self.compact_view {
                row![map_stack].spacing(4).height(Length::Fill).into()
            } else {
                row![
                    map_stack,
                    // Resize handle: themed background, visible grip and horizontal-resize cursor.
                    mouse_area(
                        container(
                            container(Space::new())
                                .width(Length::Fixed(3.0))
                                .height(Length::Fixed(56.0))
                                .style(|theme: &iced::Theme| container::Style {
                                    background: Some(theme.palette().primary.into()),
                                    border: iced::Border {
                                        radius: 2.0.into(),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                })
                        )
                        .width(Length::Fixed(8.0))
                        .height(Length::Fill)
                        .align_x(iced::alignment::Horizontal::Center)
                        .align_y(iced::alignment::Vertical::Center)
                        .style(|theme: &iced::Theme| container::Style {
                            background: Some(
                                theme.extended_palette().background.weak.color.into()
                            ),
                            ..Default::default()
                        })
                    )
                    .interaction(iced::mouse::Interaction::ResizingHorizontally)
                    .on_press(Message::DividerPressed)
                    .on_release(Message::DividerReleased),
                    container(self.view_right_panel())
                        .width(Length::Fixed(self.right_panel_width))
                        .height(Length::Fill),
                ].spacing(4).height(Length::Fill).into()
            };

            // В компактном режиме показываем подсказку "[Tab]" перед версией,
            // чтобы пользователь не забыл, как выйти.
            let right_info: Element<Message> = if self.compact_view {
                row![
                    text(t!("msg_0013")),
                    Space::new().width(Length::Fixed(16.0)),
                    text(format!("FoxyXplorer v{}", env!("CARGO_PKG_VERSION"))).size(11),
                ].align_y(Alignment::Center).into()
            } else {
                text(format!("FoxyXplorer v{}", env!("CARGO_PKG_VERSION"))).size(11).into()
            };

            let mut layout = column![]
                .spacing(4).padding(4)
                .width(Length::Fill).height(Length::Fill);
            if !self.compact_view {
                layout = layout.push(self.view_top_toolbar());
            }
            layout = layout.push(map_row);
            layout = layout.push(text(self.status.as_str()));
            layout = layout.push(
                row![
                    text(info),
                    Space::new().width(Length::Fill),
                    right_info,
                ].align_y(Alignment::Center)
            );
            layout.into()
        };

        // ---- Модальные окна (промежуточный слой) ----
        let composed_ui: Element<'_, Message> = if let Some(Modal::TrackProps { track_id, draft }) = &self.modal {
            let modal_ui = self.view_track_props_modal(*track_id, draft);
            stack![
                base_ui,
                container(text("")).width(Length::Fill).height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.15).into()),
                        ..Default::default()
                    }),
                container(modal_ui).center_x(Length::Fill).center_y(Length::Fill)
                    .padding(modal_drag_padding(self.modal_drag_offset))
            ].into()
        } else if let Some(Modal::WaypointProps { waypoint_id, draft, coord_input, .. }) = &self.modal {
            let modal_ui = self.view_waypoint_props_modal(*waypoint_id, draft, coord_input);
            stack![
                base_ui,
                container(text("")).width(Length::Fill).height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.15).into()),
                        ..Default::default()
                    }),
                container(modal_ui).center_x(Length::Fill).center_y(Length::Fill)
                    .padding(modal_drag_padding(self.modal_drag_offset))
            ].into()
        } else if let Some(Modal::TrackList(state)) = &self.modal {
            let modal_ui = self.view_track_list_modal(state);
            stack![
                base_ui,
                container(text("")).width(Length::Fill).height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.15).into()),
                        ..Default::default()
                    }),
                container(modal_ui).center_x(Length::Fill).center_y(Length::Fill)
                    .padding(modal_drag_padding(self.modal_drag_offset))
            ].into()
        } else if let Some(Modal::WaypointMeasure { from_name, from_lat, from_lon, to_name, to_lat, to_lon, dist_m, mag_az }) = &self.modal {
            let modal_ui = self.view_waypoint_measure_modal(
                from_name, *from_lat, *from_lon,
                to_name, *to_lat, *to_lon,
                *dist_m, *mag_az
            );
            stack![
                base_ui,
                container(text("")).width(Length::Fill).height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.15).into()),
                        ..Default::default()
                    }),
                container(modal_ui).center_x(Length::Fill).center_y(Length::Fill)
                    .padding(modal_drag_padding(self.modal_drag_offset))
            ].into()
        } else if let Some(Modal::ConfirmClearAll) = &self.modal {
            let modal_ui = self.view_confirm_clear_all_modal();
            stack![
                base_ui,
                container(text("")).width(Length::Fill).height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.15).into()),
                        ..Default::default()
                    }),
                container(modal_ui).center_x(Length::Fill).center_y(Length::Fill)
                    .padding(modal_drag_padding(self.modal_drag_offset))
            ].into()
        } else if let Some(Modal::ConfirmLoadSession) = &self.modal {
            let modal_ui = self.view_confirm_load_session_modal();
            stack![
                base_ui,
                container(text("")).width(Length::Fill).height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.15).into()),
                        ..Default::default()
                    }),
                container(modal_ui).center_x(Length::Fill).center_y(Length::Fill)
                    .padding(modal_drag_padding(self.modal_drag_offset))
            ].into()
        } else if let Some(Modal::WaypointProjection { from_id, azimuth_input, distance_input }) = &self.modal {
            let modal_ui = self.view_waypoint_projection_modal(*from_id, azimuth_input, distance_input);
            stack![
                base_ui,
                container(text("")).width(Length::Fill).height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.15).into()),
                        ..Default::default()
                    }),
                container(modal_ui).center_x(Length::Fill).center_y(Length::Fill)
                    .padding(modal_drag_padding(self.modal_drag_offset))
            ].into()
        } else if let Some(Modal::About) = &self.modal {
            let modal_ui = self.view_about_modal();
            stack![
                base_ui,
                container(text("")).width(Length::Fill).height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.15).into()),
                        ..Default::default()
                    }),
                container(modal_ui).center_x(Length::Fill).center_y(Length::Fill)
            ].into()
        } else if let Some(Modal::FoxyDrop) = &self.modal {
            let modal_ui = self.view_foxydrop_modal();
            stack![
                base_ui,
                container(text("")).width(Length::Fill).height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.15).into()),
                        ..Default::default()
                    }),
                container(modal_ui).center_x(Length::Fill).center_y(Length::Fill)
                    .padding(modal_drag_padding(self.modal_drag_offset))
            ].into()
        } else if let Some(Modal::Settings { draft }) = &self.modal {
            let modal_ui = self.view_settings_modal(draft);
            stack![
                base_ui,
                container(text("")).width(Length::Fill).height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.15).into()),
                        ..Default::default()
                    }),
                container(modal_ui).center_x(Length::Fill).center_y(Length::Fill)
                    .padding(modal_drag_padding(self.modal_drag_offset))
            ].into()
        } else {
            base_ui.into()
        };

        // ---- Контекстное меню (верхний слой) ----
        if let Some(ctx) = &self.context_menu {
            let menu_ui = self.view_context_menu(ctx);

            // Отступы через Space вместо padding
            let positioned_menu = container(
                column![
                    Space::new().height(Length::Fixed(ctx.y)),
                    row![
                        Space::new().width(Length::Fixed(ctx.x)),
                        menu_ui,
                    ]
                ]
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Start)
            .align_y(Alignment::Start);

            stack![
                composed_ui,
                mouse_area(
                    container(text(""))
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .style(|_| container::Style {
                            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.001).into()),
                            ..Default::default()
                        })
                ).on_press(Message::ContextMenuClose).on_right_press(Message::ContextMenuClose),
                positioned_menu,
            ]
            .into()
        } else {
            composed_ui
        }
    }  

    fn view_map(&self) -> Element<'_, Message> {
        let map_canvas = Canvas::new(canvas::MapProgram { app: self }).width(Length::Fill).height(Length::Fill);
        let tracks_canvas = Canvas::new(canvas::TrackProgram { app: self }).width(Length::Fill).height(Length::Fill);
        let grid_canvas = Canvas::new(canvas::GridProgram { app: self }).width(Length::Fill).height(Length::Fill);

        // Custom tool cursors are rendered as 24x24 SVGs. iced::mouse::Interaction
        // does not support custom SVG cursors. Each icon has its own hotspot,
        // whose position corresponds to the actual mouse event coordinates.
        // Generated outlines are cached once; only SVG handles are made in view().
        static OUTLINED_CURSORS: std::sync::OnceLock<[Vec<u8>; 5]> =
            std::sync::OnceLock::new();
        let cursor_images = OUTLINED_CURSORS.get_or_init(|| [
            outlined_tool_cursor_svg(lucide_iced::bytes::MOUSE_POINTER),
            outlined_tool_cursor_svg(lucide_iced::bytes::MOUSE_POINTER_2),
            outlined_tool_cursor_svg(lucide_iced::bytes::PEN_TOOL),
            outlined_tool_cursor_svg(lucide_iced::bytes::PENCIL),
            outlined_tool_cursor_svg(lucide_iced::bytes::CIRCLE_PLUS),
        ]);
        let cursor_icon: Option<(usize, f32, f32)> = match self.active_tool {
            Tool::SelectObjects => Some((0, 4.0, 3.0)),
            Tool::SelectPoints  => Some((1, 3.0, 3.0)),
            Tool::DrawTrack     => Some((2, 2.3, 2.3)),
            Tool::Freehand      => Some((3, 4.0, 20.0)),
            Tool::AddWaypoint   => Some((4, 12.0, 12.0)),
            _ => None,
        };

        // Do not replace the pointer inside modals or on the floating tool
        // palette on the left (56 px). Keep normal mouse events on the map.
        let show_tool_cursor = !self.middle_panning
            && self.modal.is_none()
            && self.context_menu.is_none()
            && self.map_cursor_inside
            && self.cursor.is_some_and(|p| p.x >= 56.0)
            && cursor_icon.is_some();

        let canvases = stack![map_canvas, tracks_canvas, grid_canvas];
        let contents: Element<'_, Message> = if show_tool_cursor {
            let (index, hotspot_x, hotspot_y) = cursor_icon.unwrap();
            let p = self.cursor.unwrap();
            let position = Point::new(p.x - hotspot_x, p.y - hotspot_y);
            // Use a plain SVG, not lucide_iced::themed_icon: the latter
            // recolors the entire image and would erase the white outline.
            let cursor_svg = iced::widget::svg(
                iced::widget::svg::Handle::from_memory(cursor_images[index].as_slice())
            )
            .width(Length::Fixed(24.0))
            .height(Length::Fixed(24.0))
            .style(|_, _| iced::widget::svg::Style { color: None });
            stack![
                canvases,
                iced::widget::pin(cursor_svg).position(position),
            ].into()
        } else {
            canvases.into()
        };

        let mut area = mouse_area(contents)
            .on_move(Message::Moved)
            .on_exit(Message::MapCursorExited)
            .on_scroll(Message::Scrolled);
        if self.middle_panning || self.dragging {
            area = area.interaction(iced::mouse::Interaction::Grabbing);
        } else if show_tool_cursor {
            area = area.interaction(iced::mouse::Interaction::Hidden);
        }
        area.into()
    }
}

fn main() -> iced::Result {

    // 1. Загружаем иконку (без паники, если что-то не так)
    let icon = match load_icon_from_svg(ICON_SVG, 256) {
        Ok(icon) => Some(icon),
        Err(e) => {
            eprintln!("Не удалось загрузить иконку окна: {e}");
            None
        }
    };

    // 2. Читаем сессию (как было)
    let session = session::load_session();
    
    #[cfg(target_os = "linux")]
    let platform_specific = PlatformSpecific {
        application_id: "foxy".to_string(),
        ..Default::default()
    };
    #[cfg(not(target_os = "linux"))]
    let platform_specific = PlatformSpecific::default();

    let mut window_settings = iced::window::Settings {
        size: Size::new(1130.0, 880.0),
        min_size: Some(Size::new(640.0, 480.0)),
        exit_on_close_request: false,
        icon,
        platform_specific,
        ..Default::default()
    };
    
    // Respect the last saved mode; the theme family itself comes from config.json.
    let initial_mode = match session.as_ref().and_then(|s| s.theme.as_deref()) {
        Some("light") => AppTheme::Light,
        _ => AppTheme::Dark,
    };

    if let Some(s) = &session {
        if let Some((w, h)) = s.view.window_size { window_settings.size = Size::new(w, h); }
        if let Some((x, y)) = s.view.window_pos { window_settings.position = iced::window::Position::Specific(Point::new(x, y)); }
    }
    i18n::set_lang("ru");   // default    
    iced::application(
        move || App::new(initial_mode),
        App::update,
        App::view,
    )
    .title("FoxyXplorer")
    .window(window_settings)
    .subscription(App::subscription)
    .theme(|app: &App| app.theme.clone())
    .run()
}
