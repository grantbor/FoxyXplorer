use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use coord::{format_pair, CoordFormat};
use iced::mouse::ScrollDelta;
use iced::widget::canvas::{self, Cache, Canvas, Geometry, Path as CanvasPath, Stroke};
use iced::widget::image::Handle as ImageHandle;
use iced::widget::{button, column, mouse_area, stack, text, Row};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

const VIEW_W: f32 = 1100.0;
const VIEW_H: f32 = 720.0;
const R: f64 = 6_378_137.0;
const BG: [u8; 4] = [38, 38, 42, 255];

fn merc_of(lon: f64, lat: f64) -> (f64, f64) {
    (
        R * lon.to_radians(),
        R * (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.0).tan().ln(),
    )
}

fn lonlat_of(x: f64, y: f64) -> (f64, f64) {
    (
        (x / R).to_degrees(),
        (2.0 * (y / R).exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees(),
    )
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

type GridData = (grid::Grid, Vec<Vec<(f64, f64)>>);

fn load_kit(dir: &Path) -> Result<(Vec<Layer>, Option<GridData>, Vec<String>), String> {
    let mut maps: Vec<PathBuf> = Vec::new();
    let mut gpxs: Vec<PathBuf> = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for e in entries {
        let p = e.map_err(|e| e.to_string())?.path();
        match p.extension().and_then(|s| s.to_str()).map(|s| s.to_lowercase()).as_deref() {
            Some("map") => maps.push(p),
            Some("gpx") => gpxs.push(p),
            _ => {}
        }
    }
    maps.sort();

    let mut notes = Vec::new();
    let mut layers = Vec::new();
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
        let raster = dir.join(&map.image_file);
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
            .levels
            .iter()
            .enumerate()
            .map(|(i, l)| (i, l.width as f64 / bw, l.height as f64 / bh))
            .collect();
        levels.sort_by(|a, b| b.1.total_cmp(&a.1));
        layers.push(Layer { name: stem, ozf: RefCell::new(ozf), georef, levels });
    }
    if layers.is_empty() {
        return Err(format!(
            "в папке нет пригодных карт (.map + .ozf2). {}",
            notes.join("; ")
        ));
    }

    let mut best: Option<GridData> = None;
    for p in &gpxs {
        let Ok(bytes) = std::fs::read(p) else { continue };
        let Ok(g) = grid::Grid::from_gpx(&geoio::text::decode(&bytes)) else { continue };
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
        notes.push("сетка (GPX с именованными точками) не найдена".into());
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
}

struct App {
    layers: Vec<Layer>,
    active: usize,
    grid: Option<grid::Grid>,
    lines: Vec<Vec<(f64, f64)>>,
    show_grid: bool,
    declination: f64,
    decl_offset: f64,
    format: CoordFormat,
    cx: f64,
    cy: f64,
    scale: f64,
    canvas_cache: Cache,
    grid_cache: Cache,
    /// Готовые к отрисовке тайлы: ключ `(layer, lvl, tx, ty)`.
    /// `Some(handle)` — декодировано, `None` — попытка была и провалилась.
    tile_handles: RefCell<HashMap<(usize, usize, u32, u32), Option<ImageHandle>>>,
    dragging: bool,
    cursor: Option<Point>,
    status: String,
    error: Option<String>,
    last_geom_ms: Cell<f64>,
    last_bounds: Cell<Size>,
    show_labels: bool,
}

impl App {
    fn new() -> App {
        let mut app = App {
            layers: Vec::new(),
            active: 0,
            grid: None,
            lines: Vec::new(),
            show_grid: true,
            declination: 0.0,
            decl_offset: 0.0,
            format: CoordFormat::Decimal,
            cx: 0.0,
            cy: 0.0,
            scale: 1.0,
            canvas_cache: Cache::default(),
            grid_cache: Cache::default(),
            tile_handles: RefCell::new(HashMap::new()),
            dragging: false,
            cursor: None,
            status: String::new(),
            error: None,
            last_geom_ms: Cell::new(0.0),
            last_bounds: Cell::new(Size::new(VIEW_W, VIEW_H)),
            show_labels: true,
        };
        let Some(dir) = std::env::args().nth(1) else {
            app.error = Some("Запуск: foxy <папка с распакованным комплектом>".into());
            return app;
        };
        match load_kit(Path::new(&dir)) {
            Ok((layers, gd, notes)) => {
                let n_layers = layers.len();
                let n_gpx = gd.as_ref().map_or(0, |(g, _)| g.nodes.len());
                app.layers = layers;
                if let Some((g, lines)) = gd {
                    app.grid = Some(g);
                    app.lines = lines;
                }
                app.status = if notes.is_empty() {
                    format!("слоёв: {n_layers}, точек GPX: {n_gpx}")
                } else {
                    format!("слоёв: {n_layers}, точек GPX: {n_gpx}; {}", notes.join("; "))
                };
            }
            Err(e) => {
                app.error = Some(e);
                return app;
            }
        }
        if let Some(g) = &app.grid {
            let (lat, lon) = g.center();
            let (y, ord) = today();
            match declination::declination_deg(lat, lon, y, ord) {
                Ok(d) => app.declination = d,
                Err(e) => app.status = format!("{} {e}", app.status),
            }
        }
        app.fit_to_layer(0);
        app
    }

    /// Центрирует вид на `idx`-м слое и подбирает масштаб под окно.
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
        self.scale = 1.05
            * ((w0 / ppm) / sz.width as f64).max((h0 / ppm) / sz.height as f64);
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

    fn update(&mut self, m: Message) {
        match m {
            Message::Layer(i) => {
                if i < self.layers.len() {
                    self.active = i;
                    self.canvas_cache.clear();
                    self.grid_cache.clear();
                    self.refresh_status();
                }
            }
            Message::ToggleGrid => {
                self.show_grid = !self.show_grid;
                self.canvas_cache.clear();
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
                self.refresh_status();
            }
            Message::ToggleLabels => {
                self.show_labels = !self.show_labels;
                self.grid_cache.clear();
            }
            Message::Fit => {
                let idx = self.active;
                self.fit_to_layer(idx);
                self.canvas_cache.clear();
                self.grid_cache.clear();
                self.refresh_status();
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
        let mut top = Row::new().spacing(6);
        for (i, l) in self.layers.iter().enumerate() {
            let label = if i == self.active {
                format!("● {}", l.name)
            } else {
                l.name.clone()
            };
            top = top.push(button(text(label)).on_press(Message::Layer(i)));
        }
        top = top
            .push(
                button(text(if self.show_grid { "Сетка: вкл" } else { "Сетка: выкл" }))
                    .on_press(Message::ToggleGrid),
            )
            .push(button(text("Fit")).on_press(Message::Fit))
            .push(
                button(text(if self.show_labels { "Подписи: вкл" } else { "Подписи: выкл" }))
                    .on_press(Message::ToggleLabels),
            )
            .push(button(text(self.format_name())).on_press(Message::CycleFormat))
            .push(button(text("Скл −0.1°")).on_press(Message::Decl(-0.1)))
            .push(button(text("Скл +0.1°")).on_press(Message::Decl(0.1)))
            .push(button(text("Скл: сброс")).on_press(Message::DeclReset));

        let map: Element<Message> = if self.error.is_some() || self.layers.is_empty() {
            text(self.error.clone().unwrap_or_else(|| "Нет карты".into())).into()
        } else {
            let map_canvas = Canvas::new(MapProgram { app: self })
                .width(Length::Fill)
                .height(Length::Fill);
            let grid_canvas = Canvas::new(GridProgram { app: self })
                .width(Length::Fill)
                .height(Length::Fill);

            mouse_area(stack![map_canvas, grid_canvas])
                .on_press(Message::Pressed)
                .on_release(Message::Released)
                .on_move(Message::Moved)
                .on_scroll(Message::Scrolled)
                .into()
        };

        let name = self.layers.get(self.active).map(|l| l.name.as_str()).unwrap_or("-");
        let info = format!(
            "Склонение: {:.2}° (WMM2025 {:.2}°, поправка {:+.2}°)  |  масштаб: {:.2} м/пикс  |  слой: {}  |  геометрия: {:.1} мс",
            self.declination + self.decl_offset,
            self.declination,
            self.decl_offset,
            self.scale,
            name,
            self.last_geom_ms.get()
        );
        column![top, map, text(self.status.as_str()), text(info)]
            .spacing(6)
            .padding(8)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

// ----- Программа отрисовки карты (фон + тайлы OZF2) -----

struct MapProgram<'a> {
    app: &'a App,
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

        let geom = app.canvas_cache.draw(renderer, bounds.size(), |frame| {
            app.last_bounds.set(bounds.size());
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
                            let level_px_at = |sx: f64, sy: f64| -> (f64, f64) {
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
                                let tx1 = (((px_hi / tile).floor() as u32) + 1).min(lv_tx);
                                let ty1 = (((py_hi / tile).floor() as u32) + 1).min(lv_ty);

                                let inv_scale = 1.0 / app.scale;
                                let to_screen = |px_l: f64, py_l: f64| -> (f32, f32) {
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
                                        let (sx_tl, sy_tl) =
                                            to_screen(tx as f64 * tile, ty as f64 * tile);
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
                                        let cached =
                                            app.tile_handles.borrow().get(&key).cloned();
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
        });

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
                return;
            }

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
        });

        vec![geom]
    }
}

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title("FoxyXplorer")
        .window_size(Size::new(1130.0, 880.0))
        .run()
}
