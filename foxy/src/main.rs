use iced::mouse::{self, ScrollDelta};
use iced::widget::canvas::{self, Cache, Canvas, Geometry, Path, Stroke};
use iced::widget::image::Handle as ImageHandle;
use iced::widget::{column, mouse_area, text, Row};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Task, Theme};

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path as StdPath;
use std::time::{SystemTime, UNIX_EPOCH};

use coord;
use declination;
use geoio;
use grid;
use ozf;
use ozmap;

const VIEW_W: f32 = 1100.0;
const VIEW_H: f32 = 720.0;
const R: f64 = 6_378_137.0;
const BG: [u8; 4] = [38, 38, 42, 255];
const LINE_COLOR: Color = Color::from_rgb(0.0, 0.8, 1.0);

// --- Геодезия ---
fn merc_of(lon: f64, lat: f64) -> (f64, f64) {
    (
        R * lon.to_radians(),
        R * (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.0)
            .tan()
            .ln(),
    )
}

fn lonlat_of(x: f64, y: f64) -> (f64, f64) {
    (
        (x / R).to_degrees(),
        (2.0 * (y / R).exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees(),
    )
}

struct Layer {
    name: String,
    ozf: RefCell<ozf::Ozf>,
    georef: ozmap::Georef,
    /// (ozf-индекс_уровня, ratio_x, ratio_y), отсортировано по убыванию ratio_x.
    /// ratio = размер уровня / размер ozf.levels[0] (к которому привязан georef).
    levels: Vec<(usize, f64, f64)>,
}

fn today() -> (i32, u16) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let (y, m, d, ..) = geoio::time::parts(secs);
    let ord = geoio::time::days_from_civil(y, m, d) - geoio::time::days_from_civil(y, 1, 1) + 1;
    (y as i32, ord as u16)
}

fn load_kit(
    dir: &StdPath,
) -> Result<
    (
        Vec<Layer>,
        Option<(grid::Grid, Vec<Vec<(f64, f64)>>)>,
        Vec<String>,
    ),
    String,
> {
    let mut maps = Vec::new();
    let mut gpxs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            match p
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_lowercase())
                .as_deref()
            {
                Some("map") => maps.push(p),
                Some("gpx") => gpxs.push(p),
                _ => {}
            }
        }
    }
    maps.sort();
    let notes = Vec::new();
    let mut layers = Vec::new();
    for p in &maps {
        let Ok(map) = ozmap::MapFile::open(p) else { continue };
        let Ok(georef) = map.georef() else { continue };
        let Ok(ozf) = ozf::Ozf::open(dir.join(&map.image_file)) else { continue };

        // База — ozf.levels[0], тот же уровень, к которому привязан georef.
        let base_w = ozf.levels[0].width as f64;
        let base_h = ozf.levels[0].height as f64;
        let mut lv: Vec<(usize, f64, f64)> = ozf
            .levels
            .iter()
            .enumerate()
            .map(|(i, l)| (i, l.width as f64 / base_w, l.height as f64 / base_h))
            .collect();
        lv.sort_by(|a, b| b.1.total_cmp(&a.1));

        layers.push(Layer {
            name: p.file_stem().unwrap().to_string_lossy().into(),
            ozf: RefCell::new(ozf),
            georef,
            levels: lv,
        });
    }

    let mut best: Option<(grid::Grid, Vec<Vec<(f64, f64)>>)> = None;
    for p in &gpxs {
        let Ok(bytes) = std::fs::read(p) else { continue };
        let Ok(g) = grid::Grid::from_gpx(&geoio::text::decode(&bytes)) else { continue };
        if best.as_ref().map_or(true, |(b, _)| g.nodes.len() > b.nodes.len()) {
            let mut lines = Vec::new();
            if let Ok(data) = geoio::gpx::read(&bytes) {
                for t in &data.tracks {
                    for seg in &t.segments {
                        lines.push(seg.iter().map(|q| merc_of(q.lon, q.lat)).collect());
                    }
                }
            }
            best = Some((g, lines));
        }
    }
    Ok((layers, best, notes))
}

#[derive(Debug, Clone)]
enum Message {
    Layer(usize),
    ToggleGrid,
    CycleFormat,
    Decl(f64),
    DeclReset,
    Pressed,
    Released,
    Moved(Point),
    Scrolled(ScrollDelta),
}

struct App {
    layers: Vec<Layer>,
    active: usize,
    grid: Option<(grid::Grid, Vec<Vec<(f64, f64)>>)>,
    show_grid: bool,
    decl: f64,
    decl_off: f64,
    format: coord::CoordFormat,
    cx: f64,
    cy: f64,
    scale: f64,
    canvas_cache: Cache,
    /// Кэш готовых к отрисовке тайлов: ключ `(layer, lvl, tx, ty)`.
    /// `Some(handle)` — декодировано, `None` — попытка была и провалилась.
    tile_cache: RefCell<HashMap<(usize, usize, u32, u32), Option<ImageHandle>>>,
    dragging: bool,
    cursor: Option<Point>,
    status: String,
}

impl App {
    fn new(dir: String) -> (Self, Task<Message>) {
        let mut app = Self {
            layers: Vec::new(),
            active: 0,
            grid: None,
            show_grid: true,
            decl: 0.0,
            decl_off: 0.0,
            format: coord::CoordFormat::Decimal,
            cx: 0.0,
            cy: 0.0,
            scale: 2.0,
            canvas_cache: Cache::default(),
            tile_cache: RefCell::new(HashMap::new()),
            dragging: false,
            cursor: None,
            status: String::new(),
        };

        if let Ok((layers, gd, notes)) = load_kit(StdPath::new(&dir)) {
            let n_layers = layers.len();
            let n_gpx = gd.as_ref().map_or(0, |(g, _)| g.nodes.len());
            app.layers = layers;
            app.grid = gd;
            app.status = if notes.is_empty() {
                format!("слоёв: {n_layers}, точек GPX: {n_gpx}")
            } else {
                format!("слоёв: {n_layers}, точек GPX: {n_gpx}; {}", notes.join("; "))
            };
        }

        if let Some((g, _)) = &app.grid {
            let (lat, lon) = g.center();
            let (y, ord) = today();
            if let Ok(d) = declination::declination_deg(lat, lon, y, ord) {
                app.decl = d;
            }
        }

        // Ставим центр вида в центр первого слоя.
        if let Some(l) = app.layers.first() {
            let (w0, h0) = {
                let r = l.ozf.borrow();
                (r.levels[0].width as f64, r.levels[0].height as f64)
            };
            let (lon, lat) = l.georef.pixel_to_lonlat(w0 / 2.0, h0 / 2.0);
            let (cx, cy) = merc_of(lon, lat);
            app.cx = cx;
            app.cy = cy;
        }

        (app, Task::none())
    }

    fn title(&self) -> String {
        String::from("FoxyXplorer")
    }

    fn update(&mut self, m: Message) -> Task<Message> {
        match m {
            Message::Layer(i) => {
                if i < self.layers.len() {
                    self.active = i;
                    self.canvas_cache.clear();
                }
            }
            Message::ToggleGrid => {
                self.show_grid = !self.show_grid;
                self.canvas_cache.clear();
            }
            Message::CycleFormat => {
                self.format = if self.format == coord::CoordFormat::Decimal {
                    coord::CoordFormat::DegMin
                } else {
                    coord::CoordFormat::Decimal
                };
            }
            Message::Decl(d) => self.decl_off += d,
            Message::DeclReset => self.decl_off = 0.0,
            Message::Pressed => self.dragging = true,
            Message::Released => self.dragging = false,
            Message::Moved(p) => {
                if self.dragging {
                    if let Some(prev) = self.cursor {
                        self.cx -= (p.x - prev.x) as f64 * self.scale;
                        self.cy += (p.y - prev.y) as f64 * self.scale;
                        self.canvas_cache.clear();
                    }
                }
                self.cursor = Some(p);
            }
            Message::Scrolled(d) => {
                let steps = match d {
                    ScrollDelta::Lines { y, .. } => y,
                    ScrollDelta::Pixels { y, .. } => y / 40.0,
                };
                self.scale = (self.scale * 0.8f64.powf(steps as f64)).clamp(0.05, 500.0);
                self.canvas_cache.clear();
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<Message> {
        let top = Row::new().push(text("FoxyXplorer")).spacing(10);

        let map = Canvas::new(self)
            .width(Length::Fixed(VIEW_W))
            .height(Length::Fixed(VIEW_H));

        column![
            top,
            mouse_area(map)
                .on_press(Message::Pressed)
                .on_release(Message::Released)
                .on_move(Message::Moved)
                .on_scroll(Message::Scrolled),
            text(self.status.clone()),
        ]
        .into()
    }
}

impl canvas::Program<Message> for App {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let geom = self.canvas_cache.draw(renderer, bounds.size(), |frame| {
            // Фон
            frame.fill_rectangle(
                Point::ORIGIN,
                bounds.size(),
                Color::from_rgba8(BG[0], BG[1], BG[2], 1.0),
            );

            if let Some(layer) = self.layers.get(self.active.min(self.layers.len().saturating_sub(1))) {
                let g = &layer.georef;

                // Меркаторских метров на пиксель ozf-уровня 0 (по x и по y).
                let (lon_a, lat_a) = g.pixel_to_lonlat(0.0, 0.0);
                let (lon_b, _) = g.pixel_to_lonlat(100.0, 0.0);
                let (lon_c, lat_c) = g.pixel_to_lonlat(0.0, 100.0);
                let (mx_a, my_a) = merc_of(lon_a, lat_a);
                let (mx_b, _) = merc_of(lon_b, lat_a);
                let (_, my_c) = merc_of(lon_c, lat_c);
                let base_px_x = (mx_b - mx_a).abs() / 100.0;
                let base_px_y = (my_c - my_a).abs() / 100.0;

                if base_px_x > 1e-9 && base_px_y > 1e-9 && !layer.levels.is_empty() {
                    // Выбор уровня: самый грубый, у которого пиксель ещё ≤ экранного.
                    let threshold = base_px_x.max(base_px_y) / self.scale;
                    let chosen = layer
                        .levels
                        .iter()
                        .rev() // от самого грубого к детальному
                        .find(|&&(_, rx, _)| rx >= threshold)
                        .copied()
                        .unwrap_or(layer.levels[0]); // при сильном зуме — самый детальный

                    let (ozf_lv, ratio_x, ratio_y) = chosen;

                    if ratio_x > 0.0 && ratio_y > 0.0 {
                        let (lv_w, lv_h, lv_tx, lv_ty) = {
                            let r = layer.ozf.borrow();
                            let lv = &r.levels[ozf_lv];
                            (lv.width, lv.height, lv.tiles_x, lv.tiles_y)
                        };

                        // Видимые границы в меркаторских метрах.
                        let hw = (bounds.width as f64 / 2.0) * self.scale;
                        let hh = (bounds.height as f64 / 2.0) * self.scale;
                        let (mx_w, mx_e) = (self.cx - hw, self.cx + hw);
                        let (my_s, my_n) = (self.cy - hh, self.cy + hh);

                        // Меркатор → пиксель выбранного уровня.
                        let merc_to_lv_px = |mx: f64, my: f64| -> (f64, f64) {
                            let (lon, lat) = lonlat_of(mx, my);
                            let (p0x, p0y) = g.lonlat_to_pixel(lon, lat);
                            (p0x * ratio_x, p0y * ratio_y)
                        };

                        let (px_w, py_n) = merc_to_lv_px(mx_w, my_n);
                        let (px_e, py_s) = merc_to_lv_px(mx_e, my_s);

                        let tile_f = ozf::TILE as f64;
                        // Сколько реально «нужных» тайлов (исключаем пустой padding-тайл).
                        let need_tx = (((lv_w as f64) / tile_f).ceil() as u32).max(1).min(lv_tx);
                        let need_ty = (((lv_h as f64) / tile_f).ceil() as u32).max(1).min(lv_ty);

                        let px_lo = px_w.min(px_e).max(0.0);
                        let px_hi = px_w.max(px_e).min(lv_w as f64);
                        let py_lo = py_n.min(py_s).max(0.0);
                        let py_hi = py_n.max(py_s).min(lv_h as f64);

                        if px_hi > px_lo && py_hi > py_lo {
                            let tx0 = (px_lo / tile_f).floor() as u32;
                            let ty0 = (py_lo / tile_f).floor() as u32;
                            let tx1 = (((px_hi / tile_f).floor() as u32) + 1).min(need_tx);
                            let ty1 = (((py_hi / tile_f).floor() as u32) + 1).min(need_ty);

                            for ty in ty0..ty1 {
                                for tx in tx0..tx1 {
                                    // Экранный прямоугольник тайла по двум противоположным углам.
                                    let (px_l_tl, py_l_tl) =
                                        (tx as f64 * tile_f, ty as f64 * tile_f);
                                    let (px_l_br, py_l_br) =
                                        ((tx as f64 + 1.0) * tile_f, (ty as f64 + 1.0) * tile_f);

                                    // Уровень L → уровень 0.
                                    let (px0_tl, py0_tl) = (px_l_tl / ratio_x, py_l_tl / ratio_y);
                                    let (px0_br, py0_br) = (px_l_br / ratio_x, py_l_br / ratio_y);

                                    // Пиксель → lon/lat → меркатор.
                                    let (lon_tl, lat_tl) = g.pixel_to_lonlat(px0_tl, py0_tl);
                                    let (lon_br, lat_br) = g.pixel_to_lonlat(px0_br, py0_br);
                                    let (mx_tl, my_tl) = merc_of(lon_tl, lat_tl);
                                    let (mx_br, my_br) = merc_of(lon_br, lat_br);

                                    // Меркатор → экран.
                                    let sx_tl =
                                        (bounds.width as f64 / 2.0 + (mx_tl - self.cx) / self.scale)
                                            as f32;
                                    let sy_tl =
                                        (bounds.height as f64 / 2.0 - (my_tl - self.cy) / self.scale)
                                            as f32;
                                    let sx_br =
                                        (bounds.width as f64 / 2.0 + (mx_br - self.cx) / self.scale)
                                            as f32;
                                    let sy_br =
                                        (bounds.height as f64 / 2.0 - (my_br - self.cy) / self.scale)
                                            as f32;

                                    let rect = Rectangle {
                                        x: sx_tl.min(sx_br),
                                        y: sy_tl.min(sy_br),
                                        width: (sx_br - sx_tl).abs(),
                                        height: (sy_br - sy_tl).abs(),
                                    };

                                    // Отсечение невидимых тайлов.
                                    if rect.width <= 0.0
                                        || rect.height <= 0.0
                                        || rect.x + rect.width < 0.0
                                        || rect.y + rect.height < 0.0
                                        || rect.x > bounds.width
                                        || rect.y > bounds.height
                                    {
                                        continue;
                                    }

                                    // Достаём Handle из кэша / декодируем один раз.
                                    let key = (self.active, ozf_lv, tx, ty);
                                    let cached = self.tile_cache.borrow().get(&key).cloned();
                                    let handle_opt = match cached {
                                        // Уже в кэше (успех или известный битый).
                                        Some(opt) => opt,
                                        None => {
                                            // Пробуем декодировать.
                                            let decoded = layer
                                                .ozf
                                                .borrow_mut()
                                                .tile_rgba(ozf_lv, tx, ty)
                                                .ok()
                                                .map(|rgba| {
                                                    ImageHandle::from_rgba(
                                                        ozf::TILE as u32,
                                                        ozf::TILE as u32,
                                                        rgba,
                                                    )
                                                });
                                            self.tile_cache
                                                .borrow_mut()
                                                .insert(key, decoded.clone());
                                            decoded
                                        }
                                    };

                                    if let Some(handle) = handle_opt {
                                        frame.draw_image(rect, canvas::Image::new(handle));
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Оверлей GPX поверх карты.
            if self.show_grid {
                if let Some((_, lines)) = &self.grid {
                    for seg in lines {
                        let path = Path::new(|p| {
                            let mut first = true;
                            for (mx, my) in seg {
                                let sx = bounds.width / 2.0
                                    + (mx - self.cx) as f32 / self.scale as f32;
                                let sy = bounds.height / 2.0
                                    - (my - self.cy) as f32 / self.scale as f32;
                                if first {
                                    p.move_to(Point::new(sx, sy));
                                    first = false;
                                } else {
                                    p.line_to(Point::new(sx, sy));
                                }
                            }
                        });
                        frame.stroke(
                            &path,
                            Stroke::default().with_color(LINE_COLOR).with_width(2.0),
                        );
                    }
                }
            }
        });
        vec![geom]
    }
}

pub fn main() -> iced::Result {
    let dir = std::env::args().nth(1).unwrap_or_default();

    iced::application(
        move || App::new(dir.clone()),
        App::update,
        App::view,
    )
    .title(App::title)
    .run()
}
