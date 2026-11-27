use std::time::Instant;

use iced::widget::canvas::{self, Geometry};
use iced::widget::image::Handle as ImageHandle;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

use crate::osm;
use crate::util::geo::{affine};
use crate::{App, Message, BasemapSource};
use crate::util::geo::dist_to_segment;
use crate::t;

pub struct MapProgram<'a> {
    pub app: &'a App,
}

/// Hit-test the small grid-node marker and, when enabled, its adjacent label.
fn grid_node_hit_test(click: Point, pt: Point, name: &str, show_label: bool) -> bool {
    if (click.x - pt.x).hypot(click.y - pt.y) <= 10.0 { return true; }
    if !show_label || name.is_empty() { return false; }
    // The geometry matches GridProgram's node label layout.
    let size = 13.0_f32;
    let pad = 3.0_f32;
    let width = name.chars().count() as f32 * size * 0.55 + pad * 2.0;
    let height = size + pad * 2.0;
    let left = pt.x + 6.0;
    let top = pt.y - 6.0 - height;
    click.x >= left && click.x <= left + width
        && click.y >= top && click.y <= top + height
}

fn reference_point_menu(name: &str, lat: f64, lon: f64) -> Vec<crate::ContextMenuItem> {
    let add_label = t!("msg_0246");
    vec![
        crate::ContextMenuItem::Label(name.to_string()),
        crate::ContextMenuItem::Action(
            t!("msg_0073").to_string(),
            Message::CopyCoordinates(lat, lon),
        ),
        crate::ContextMenuItem::Action(
            add_label.to_string(),
            Message::AddNamedWaypoint(name.to_string(), lon, lat),
        ),
    ]
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
        // Never deliver map gestures to tools while any modal is open,
        // even if a future overlay stops capturing a particular event.
        if self.app.modal.is_some() {
            return None;
        }

        // In review mode a click on a red suspicious interval focuses it in
        // the sidebar. The usual map context menu is intentionally disabled.
        if let Some(inspect) = &self.app.track_inspection {
            if let iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Right)) = event {
                return None;
            }
            if inspect.preview_segments.is_none() {
                if let iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) = event {
                    if let Some(pos) = cursor.position_in(bounds) {
                        if let Some(track) = self.app.tracks.iter().find(|t| t.id == inspect.track_id) {
                            let to_screen = |p: &crate::domain::TrackPoint| {
                                let (x, y) = crate::util::geo::merc_of(p.lon, p.lat);
                                Point::new(
                                    bounds.width / 2.0 + ((x - self.app.cx) / self.app.scale) as f32,
                                    bounds.height / 2.0 - ((y - self.app.cy) / self.app.scale) as f32,
                                )
                            };
                            for (index, region) in inspect.report.ranges.iter().enumerate() {
                                let Some(segment) = track.segments.get(region.segment) else { continue };
                                let pts = &segment.points[region.start..=region.end];
                                if pts.windows(2).any(|w| dist_to_segment(pos, to_screen(&w[0]), to_screen(&w[1])) < 11.0)
                                    || pts.first().is_some_and(|p| { let q = to_screen(p); (pos.x - q.x).hypot(pos.y - q.y) < 11.0 })
                                {
                                    return Some(iced::widget::Action::publish(Message::InspectionFocus(index)));
                                }
                            }
                        }
                    }
                }
            }
        }

        // ============================================================
        // ПРАВАЯ КНОПКА — КОНТЕКСТНОЕ МЕНЮ
        // ============================================================
        if let iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Right)) = event {
            if let Some(pos) = cursor.position_in(bounds) {
                let hw = bounds.width / 2.0;
                let hh = bounds.height / 2.0;
                let mx = self.app.cx + (pos.x as f64 - hw as f64) * self.app.scale;
                let my = self.app.cy - (pos.y as f64 - hh as f64) * self.app.scale;
                let (lon, lat) = crate::util::geo::lonlat_of(mx, my);

                // --- Hit-test: waypoints (радиус 20 px) ---
                let mut hit_wp = None;
                let mut min_wp = 20.0_f32;
                for wp in &self.app.waypoints {
                    if !wp.visible { continue; }
                    let (wmx, wmy) = crate::util::geo::merc_of(wp.lon, wp.lat);
                    let sp = Point::new(
                        hw + ((wmx - self.app.cx) / self.app.scale) as f32,
                        hh - ((wmy - self.app.cy) / self.app.scale) as f32,
                    );
                    let d = (pos.x - sp.x).hypot(pos.y - sp.y);
                    if d < min_wp { min_wp = d; hit_wp = Some(wp.id); }
                }

                // --- Hit-test: треки (радиус 15 px до линии или до точки) ---
                let mut hit_track = None;
                if hit_wp.is_none() {
                    let mut min_t = 15.0_f32;
                    for t in &self.app.tracks {
                        if !t.visible { continue; }
                        for seg in &t.segments {
                            // проверяем расстояния до сегментов линии
                            for w in seg.points.windows(2) {
                                let (m1, n1) = crate::util::geo::merc_of(w[0].lon, w[0].lat);
                                let (m2, n2) = crate::util::geo::merc_of(w[1].lon, w[1].lat);
                                let p1 = Point::new(
                                    hw + ((m1 - self.app.cx) / self.app.scale) as f32,
                                    hh - ((n1 - self.app.cy) / self.app.scale) as f32,
                                );
                                let p2 = Point::new(
                                    hw + ((m2 - self.app.cx) / self.app.scale) as f32,
                                    hh - ((n2 - self.app.cy) / self.app.scale) as f32,
                                );
                                let d = dist_to_segment(pos, p1, p2);
                                if d < min_t { min_t = d; hit_track = Some(t.id); }
                            }
                        }
                    }
                }

                // ============================================================
                // ПРИОРИТЕТ 1: под курсором waypoint
                // ============================================================
                if let Some(id) = hit_wp {
                    if let Some(w) = self.app.waypoints.iter().find(|w| w.id == id) {
                        let items = vec![
                            crate::ContextMenuItem::Label(w.name.clone()),
                            crate::ContextMenuItem::Action(
                                t!("msg_0073").to_string(),
                                Message::CopyCoordinates(w.lat, w.lon),
                            ),
                            crate::ContextMenuItem::Action(
                                t!("msg_0074").to_string(),
                                Message::OpenWaypointProps(id),
                            ),
                            crate::ContextMenuItem::Action(
                                t!("msg_0062").to_string(),
                                Message::OpenWaypointProjection(id),
                            ),
                            crate::ContextMenuItem::Separator,
                            crate::ContextMenuItem::Action(
                                t!("msg_0075").to_string(),
                                Message::DeleteWaypointById(id),
                            ),
                        ];
                        return Some(iced::widget::Action::publish(
                            Message::ShowContextMenu(self.app.global_cursor, items),
                        ));
                    }
                }

                // ============================================================
                // ПРИОРИТЕТ 2: видимый узел сетки (выше Wiki и треков)
                // ============================================================
                if self.app.show_grid {
                    if let Some(grid) = &self.app.grid {
                        // Later nodes are painted last, so test them first.
                        for node in grid.nodes.iter().rev() {
                            let (nx, ny) = crate::util::geo::merc_of(node.lon, node.lat);
                            let pt = Point::new(
                                hw + ((nx - self.app.cx) / self.app.scale) as f32,
                                hh - ((ny - self.app.cy) / self.app.scale) as f32,
                            );
                            if grid_node_hit_test(pos, pt, &node.name, self.app.show_labels) {
                                return Some(iced::widget::Action::publish(
                                    Message::ShowContextMenu(
                                        self.app.global_cursor,
                                        reference_point_menu(&node.name, node.lat, node.lon),
                                    ),
                                ));
                            }
                        }
                    }
                }

                // ============================================================
                // ПРИОРИТЕТ 3: видимая точка Wiki-WPT или её подпись
                // ============================================================
                if self.app.show_wiki {
                    for wiki in self.app.wiki_points.iter().rev() {
                        let (wx, wy) = crate::util::geo::merc_of(wiki.lon, wiki.lat);
                        let pt = Point::new(
                            hw + ((wx - self.app.cx) / self.app.scale) as f32,
                            hh - ((wy - self.app.cy) / self.app.scale) as f32,
                        );
                        // Match the culling in WikiProgram, including its margin.
                        if pt.x < -200.0 || pt.x > bounds.width + 200.0
                            || pt.y < -60.0 || pt.y > bounds.height + 60.0 {
                            continue;
                        }
                        if super::waypoint_label::waypoint_label_hit_test(pt, &wiki.name, pos) {
                            return Some(iced::widget::Action::publish(
                                Message::ShowContextMenu(
                                    self.app.global_cursor,
                                    reference_point_menu(&wiki.name, wiki.lat, wiki.lon),
                                ),
                            ));
                        }
                    }
                }

                // ============================================================
                // ПРИОРИТЕТ 4: под курсором трек
                // ============================================================
                if let Some(id) = hit_track {
                    if let Some(t) = self.app.tracks.iter().find(|t| t.id == id) {
                        // Ищем ближайшую точку этого трека к клику (для "Разделить здесь")
                        let mut best_seg = 0usize;
                        let mut best_pt = 0usize;
                        let mut best_d = f32::MAX;
                        for (seg_idx, seg) in t.segments.iter().enumerate() {
                            for (pt_idx, p) in seg.points.iter().enumerate() {
                                let (m, n) = crate::util::geo::merc_of(p.lon, p.lat);
                                let sp = Point::new(
                                    hw + ((m - self.app.cx) / self.app.scale) as f32,
                                    hh - ((n - self.app.cy) / self.app.scale) as f32,
                                );
                                let d = (pos.x - sp.x).hypot(pos.y - sp.y);
                                if d < best_d {
                                    best_d = d;
                                    best_seg = seg_idx;
                                    best_pt = pt_idx;
                                }
                            }
                        }

                        let items = vec![
                            crate::ContextMenuItem::Label(t.name.clone()),
                            crate::ContextMenuItem::Action(
                                t!("msg_0073").to_string(),
                                Message::CopyCoordinates(lat, lon),
                            ),
                            crate::ContextMenuItem::Action(
                                t!("msg_0074").to_string(),
                                Message::OpenTrackProps(id),
                            ),
                            crate::ContextMenuItem::Action(
                                t!("msg_0076").to_string(),
                                Message::OpenTrackList(id),
                            ),
                            crate::ContextMenuItem::Action(
                                t!("msg_0431").to_string(),
                                Message::InspectTrack(id),
                            ),
                            crate::ContextMenuItem::Action(
                                t!("msg_0077").to_string(),
                                Message::FitTrackById(id),
                            ),
                            crate::ContextMenuItem::Action(
                                t!("msg_0078").to_string(),
                                Message::SplitTrackAt(id, best_seg, best_pt),
                            ),
                            crate::ContextMenuItem::Separator,
                            crate::ContextMenuItem::Action(
                                t!("msg_0075").to_string(),
                                Message::DeleteTrackById(id),
                            ),
                        ];
                        return Some(iced::widget::Action::publish(
                            Message::ShowContextMenu(self.app.global_cursor, items),
                        ));
                    }
                }

                // ============================================================
                // ПРИОРИТЕТ 5: пустое место
                // ============================================================
                let items = vec![
                    crate::ContextMenuItem::Action(
                        t!("msg_0073").to_string(),
                        Message::CopyCoordinates(lat, lon),
                    ),
                    crate::ContextMenuItem::Action(
                        t!("msg_0079").to_string(),
                        Message::AddWaypointAt(lon, lat),
                    ),
                ];
                return Some(iced::widget::Action::publish(
                    Message::ShowContextMenu(self.app.global_cursor, items),
                ));
            }
        }

        // ============================================================
        // ЛЕВАЯ КНОПКА
        // ============================================================
        // MMB: pan from any tool without generating MapPressed (LMB).
        if let iced::Event::Mouse(
            iced::mouse::Event::ButtonPressed(iced::mouse::Button::Middle)
        ) = event {
            if let Some(pos) = cursor.position_in(bounds) {
                return Some(iced::widget::Action::publish(
                    Message::MapMiddlePressed(pos)
                ));
            }
        }

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
                    let max_z = match app.online_basemap() {
                        Some(crate::BasemapSource::Sentinel2) => Some(14u8),
                        Some(BasemapSource::Esri) => Some(19),
                        _ => None,
                    };
                    let (_z, tiles) = osm::visible_tiles(
                        app.cx, app.cy, app.scale,
                        sz.width as f64, sz.height as f64,
                        max_z,
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

                    let (attr_text, attr_w_target): (&str, f32) = match app.online_basemap() {
                        Some(crate::BasemapSource::Sentinel2) => (
                            "Sentinel-2 cloudless by EOX IT Services GmbH \
                            (Contains modified Copernicus Sentinel data 2024) — CC BY 4.0",
                            720.0,
                        ),
                        Some(crate::BasemapSource::Esri) => (
                            "Sources: Esri, DigitalGlobe, GeoEye, i-cubed, USDA FSA, USGS, \
                            AEX, Getmapping, Aerogrid, IGN, IGP, swisstopo, and the GIS User Community",
                            900.0,
                        ),
                        _ => (
                            "© OpenStreetMap contributors, Tiles courtesy of Humanitarian OpenStreetMap Team",
                            470.0,
                        ),
                    };
                    let attr_w = attr_w_target.min(bounds.width);
                    let attr_h = 20.0_f32;
                    frame.fill_rectangle(
                        Point::new(bounds.width - attr_w - 4.0, bounds.height - attr_h - 4.0),
                        Size::new(attr_w, attr_h),
                        Color::from_rgba8(0, 0, 0, 0.6),
                    );
                    frame.fill_text(canvas::Text {
                        content: attr_text.to_string(),
                        position: Point::new(bounds.width - 8.0, bounds.height - attr_h / 2.0 - 4.0),
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

#[cfg(test)]
mod reference_point_context_tests {
    use super::*;

    #[test]
    fn grid_node_and_label_can_be_hit_separately() {
        let anchor = Point::new(150.0, 120.0);
        assert!(grid_node_hit_test(Point::new(158.0, 120.0), anchor, "H12", false));
        assert!(grid_node_hit_test(Point::new(170.0, 100.0), anchor, "H12", true));
        assert!(!grid_node_hit_test(Point::new(170.0, 100.0), anchor, "H12", false));
    }

    #[test]
    fn wiki_marker_and_label_are_both_clickable() {
        let anchor = Point::new(150.0, 120.0);
        assert!(super::super::waypoint_label::waypoint_label_hit_test(
            anchor, "Wiki place", Point::new(150.0, 126.0),
        ));
        let rect = super::super::waypoint_label::waypoint_label_bounds(anchor, "Wiki place");
        assert!(super::super::waypoint_label::waypoint_label_hit_test(
            anchor, "Wiki place", Point::new(rect.x + 2.0, rect.y + 2.0),
        ));
        assert!(!super::super::waypoint_label::waypoint_label_hit_test(
            anchor, "Wiki place", Point::new(0.0, 0.0),
        ));
    }

    #[test]
    fn context_menu_preserves_reference_point_name_and_coordinates() {
        let items = reference_point_menu("H12", 54.945816, 83.084312);
        assert_eq!(items.len(), 3);
        match &items[2] {
            crate::ContextMenuItem::Action(_, Message::AddNamedWaypoint(name, lon, lat)) => {
                assert_eq!(name, "H12");
                assert_eq!((*lat, *lon), (54.945816, 83.084312));
            }
            other => panic!("Wrong menu action: {other:?}"),
        }
    }
}
