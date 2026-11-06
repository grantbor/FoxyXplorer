use iced::mouse::{self, ScrollDelta};
use iced::widget::canvas::{self, Cache, Canvas, Geometry, Path, Stroke};
use iced::widget::{column, mouse_area, text, Row};
use iced::{Application, Color, Command, Element, Length, Point, Rectangle, Renderer, Settings, Size, Theme};
use std::collections::HashMap;
use std::path::{Path as StdPath};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use coord; use declination; use geoio; use grid; use ozf; use ozmap;

const VIEW_W: f32 = 1100.0;
const VIEW_H: f32 = 720.0;
const R: f64 = 6_378_137.0;
const BG: Color = Color::from_rgb(0.15, 0.15, 0.16);
const LINE_COLOR: Color = Color::from_rgb(0.0, 0.8, 1.0);
const NODE_COLOR: Color = Color::from_rgb(1.0, 0.2, 0.2);

fn merc_of(lon: f64, lat: f64) -> (f64, f64) {
    (R * lon.to_radians(), R * (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.0).tan().ln())
}
fn lonlat_of(x: f64, y: f64) -> (f64, f64) {
    ((x / R).to_degrees(), (2.0 * (y / R).exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees())
}

struct Affine { ax: f64, ay: f64, bx: f64, by: f64 }
fn affine(g: &ozmap::Georef, cx: f64, cy: f64) -> Affine {
    const D: f64 = 1000.0;
    let at = |mx: f64, my: f64| { let (lon, lat) = lonlat_of(mx, my); g.lonlat_to_pixel(lon, lat) };
    let (x0, y0) = at(cx, cy); let (x1, y1) = at(cx + D, cy); let (x2, y2) = at(cx, cy + D);
    Affine { ax: (x1 - x0) / D, ay: (y1 - y0) / D, bx: (x2 - x0) / D, by: (y2 - y0) / D }
}

struct Layer { name: String, ozf: ozf::Ozf, georef: ozmap::Georef, levels: Vec<(usize, f64, f64)> }

fn today() -> (i32, u16) {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    let (y, m, d, ..) = geoio::time::parts(secs);
    let ord = geoio::time::days_from_civil(y, m, d) - geoio::time::days_from_civil(y, 1, 1) + 1;
    (y as i32, ord as u16)
}

fn load_kit(dir: &StdPath) -> Result<(Vec<Layer>, Option<(grid::Grid, Vec<Vec<(f64, f64)>>)>, Vec<String>), String> {
    let mut maps = Vec::new(); let mut gpxs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            match p.extension().and_then(|s| s.to_str()).map(|s| s.to_lowercase()).as_deref() {
                Some("map") => maps.push(p), Some("gpx") => gpxs.push(p), _ => {}
            }
        }
    }
    maps.sort(); let mut notes = Vec::new(); let mut layers = Vec::new();
    for p in &maps {
        let Ok(map) = ozmap::MapFile::open(p) else { continue };
        let Ok(georef) = map.georef() else { continue };
        let Ok(ozf) = ozf::Ozf::open(dir.join(&map.image_file)) else { continue };
        let (bw, bh) = (ozf.levels.iter().map(|l| l.width).max().unwrap_or(1) as f64, ozf.levels.iter().map(|l| l.height).max().unwrap_or(1) as f64);
        let mut lv: Vec<_> = ozf.levels.iter().enumerate().map(|(i, l)| (i, l.width as f64 / bw, l.height as f64 / bh)).collect();
        lv.sort_by(|a, b| b.1.total_cmp(&a.1));
        layers.push(Layer { name: p.file_stem().unwrap().to_string_lossy().into(), ozf, georef, levels: lv });
    }
    let mut best = None;
    for p in &gpxs {
        let Ok(bytes) = std::fs::read(p) else { continue };
        let Ok(g) = grid::Grid::from_gpx(&geoio::text::decode(&bytes)) else { continue };
        if best.as_ref().map_or(true, |(b, _)| g.nodes.len() > b.nodes.len()) {
            let mut lines = Vec::new();
            if let Ok(data) = geoio::gpx::read(&bytes) {
                for t in &data.tracks { for seg in &t.segments { lines.push(seg.iter().map(|q| merc_of(q.lon, q.lat)).collect()); } }
            }
            best = Some((g, lines));
        }
    }
    Ok((layers, best, notes))
}

#[derive(Debug, Clone)]
enum Message { Layer(usize), ToggleGrid, CycleFormat, Decl(f64), DeclReset, Pressed, Released, Moved(Point), Scrolled(ScrollDelta) }

struct App {
    layers: Vec<Layer>, active: usize, grid: Option<(grid::Grid, Vec<Vec<(f64, f64)>>)>,
    show_grid: bool, decl: f64, decl_off: f64, format: coord::CoordFormat,
    cx: f64, cy: f64, scale: f64, canvas_cache: Cache, dragging: bool, cursor: Option<Point>, status: String,
    tile_cache: HashMap<(usize, usize, u32, u32), iced::widget::image::Handle>,
}

impl Application for App {
    type Executor = iced::executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = String;

    fn new(dir: String) -> (Self, Command<Message>) {
        let mut app = Self {
            layers: Vec::new(), active: 0, grid: None, show_grid: true,
            decl: 0.0, decl_off: 0.0, format: coord::CoordFormat::Decimal,
            cx: 0.0, cy: 0.0, scale: 2.0, canvas_cache: Cache::default(), dragging: false, cursor: None,
            status: String::new(), tile_cache: HashMap::new(),
        };
        if let Ok((layers, gd, notes)) = load_kit(StdPath::new(&dir)) {
            app.layers = layers; app.grid = gd; app.status = notes.join("; ");
        }
        if let Some((g, _)) = &app.grid {
            let (lat, lon) = g.center();
            let (y, ord) = today();
            if let Ok(d) = declination::declination_deg(lat, lon, y, ord) { app.decl = d; }
        }
        if !app.layers.is_empty() {
            let l = &app.layers[0];
            let (lon, lat) = l.georef.pixel_to_lonlat(l.ozf.levels[0].width as f64 / 2.0, l.ozf.levels[0].height as f64 / 2.0);
            (app.cx, app.cy) = merc_of(lon, lat);
        }
        (app, Command::none())
    }

    fn title(&self) -> String { String::from("FoxyXplorer") }

    fn update(&mut self, m: Message) -> Command<Message> {
        match m {
            Message::Layer(i) => { if i < self.layers.len() { self.active = i; self.canvas_cache.clear(); self.tile_cache.clear(); } }
            Message::ToggleGrid => { self.show_grid = !self.show_grid; self.canvas_cache.clear(); }
            Message::CycleFormat => self.format = if self.format == coord::CoordFormat::Decimal { coord::CoordFormat::DegMin } else { coord::CoordFormat::Decimal },
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
                let steps = match d { ScrollDelta::Lines { y, .. } => y, ScrollDelta::Pixels { y, .. } => y / 40.0 };
                self.scale = (self.scale * 0.8f64.powf(steps as f64)).clamp(0.05, 500.0);
                self.canvas_cache.clear();
            }
        }
        Command::none()
    }

    fn view(&self) -> Element<Message> {
        let top = Row::new().push(text("FoxyXplorer")).spacing(10);
        let map = Canvas::new(self).width(Length::Fixed(VIEW_W)).height(Length::Fixed(VIEW_H));
        column![
            top, 
            mouse_area(map).on_press(Message::Pressed).on_release(Message::Released).on_move(Message::Moved).on_scroll(Message::Scrolled), 
            text(&self.status)
        ].into()
    }
}

impl canvas::Program<Message> for App {
    type State = ();
    fn draw(&self, _state: &Self::State, renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry> {
        let geom = self.canvas_cache.draw(renderer, bounds.size(), |frame| {
            if let Some(l) = self.layers.get(self.active) {
                let a = affine(&l.georef, self.cx, self.cy);
                let ppm = (a.ax * a.ax + a.ay * a.ay).sqrt();
                let r = self.scale * ppm;
                let pick = *l.levels.iter().find(|lv| lv.1 * 1000.0 / r >= 0.9).unwrap_or(&l.levels[0]);
                let (lidx, _, _) = pick;
                
                let min_mx = self.cx - bounds.width as f64 / 2.0 * self.scale;
                let max_mx = self.cx + bounds.width as f64 / 2.0 * self.scale;
                let min_my = self.cy - bounds.height as f64 / 2.0 * self.scale;
                let max_my = self.cy + bounds.height as f64 / 2.0 * self.scale;

                // Отрисовка видимых тайлов (упрощенно)
                // В реальном коде тут нужно перебрать индексы тайлов, попадающих в [min_mx, max_mx]
                // и вызвать frame.draw_image(...)
            }
            if self.show_grid, let Some((_, lines)) = &self.grid {
                for seg in lines {
                    let path = Path::new(|p| {
                        let mut first = true;
                        for (mx, my) in seg {
                            let sx = bounds.width / 2.0 + (mx - self.cx) as f32 / self.scale as f32;
                            let sy = bounds.height / 2.0 - (my - self.cy) as f32 / self.scale as f32;
                            if first { p.move_to(Point::new(sx, sy)); first = false; } else { p.line_to(Point::new(sx, sy)); }
                        }
                    });
                    frame.stroke(&path, Stroke::default().with_color(LINE_COLOR).with_width(2.0));
                }
            }
        });
        vec![geom]
    }
}

pub fn main() -> iced::Result {
    App::run(Settings::with_flags(std::env::args().nth(1).unwrap_or_default()))
}
