use std::time::Instant;

use iced::widget::canvas::{self, Geometry};
use iced::widget::image::Handle as ImageHandle;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

use crate::osm;
use crate::util::geo::affine;
use crate::{App, Message};

pub struct MapProgram<'a> {
    pub app: &'a App,
}

impl<'a> canvas::Program<Message> for MapProgram<'a> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<iced::widget::Action<Message>> {
        if let iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) = event {
            if let Some(pos) = cursor.position_in(bounds) {
                return Some(iced::widget::Action::publish(Message::MapPressed(pos)));
            }
        }
        if let iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) = event {
            if let Some(pos) = cursor.position_in(bounds) {
                return Some(iced::widget::Action::publish(Message::MapReleased(pos)));
            }
        }
        None
    }
    
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
                        Color::from_rgba8(38, 38, 42, 1.0),
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
                        Color::from_rgba8(38, 38, 42, 1.0),
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
