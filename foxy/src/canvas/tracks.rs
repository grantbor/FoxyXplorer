use iced::widget::canvas::{self, Geometry, Path as CanvasPath};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use crate::util::geo::merc_of;
use crate::{App, Message};

pub struct TrackProgram<'a> {
    pub app: &'a App,
}

// Вспомогательная функция для создания пунктирной линии
fn create_dashed_path(points: &[Point], dash_len: f32, gap_len: f32) -> CanvasPath {
    CanvasPath::new(|p| {
        for w in points.windows(2) {
            let p1 = w[0];
            let p2 = w[1];
            let dx = p2.x - p1.x;
            let dy = p2.y - p1.y;
            let dist = (dx * dx + dy * dy).sqrt();
            
            if dist == 0.0 { continue; }
            
            let dir_x = dx / dist;
            let dir_y = dy / dist;
            let mut d = 0.0;
            
            while d < dist {
                let d_end = (d + dash_len).min(dist);
                p.move_to(Point::new(p1.x + dir_x * d, p1.y + dir_y * d));
                p.line_to(Point::new(p1.x + dir_x * d_end, p1.y + dir_y * d_end));
                d += dash_len + gap_len;
            }
        }
    })
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

            for t in &app.tracks {
                if !t.visible {
                    continue;
                }
                let color = Color::from_rgb8(
                    t.props.line_color[0],
                    t.props.line_color[1],
                    t.props.line_color[2],
                );
                // Берем толщину из настроек (минимум 1.0)
                let width = t.props.line_width.max(1.0);
                let is_dashed = t.props.line_style == crate::domain::LineStyle::Dash;
                let is_polygon = t.props.poly_type == crate::domain::PolyType::Polygon;

                for seg in &t.segments {
                    if seg.points.len() < 2 {
                        continue;
                    }

                    // Сначала переводим все точки в экранные координаты
                    let screen_pts: Vec<Point> = seg.points.iter().map(|pt| {
                        let (mx, my) = merc_of(pt.lon, pt.lat);
                        to_screen(mx, my)
                    }).collect();

                    // Заливка для полигона
                    if is_polygon && t.props.fill_type != crate::domain::FillType::None {
                        let path = CanvasPath::new(|p| {
                            let mut first = true;
                            for &pt in &screen_pts {
                                if first { p.move_to(pt); first = false; }
                                else { p.line_to(pt); }
                            }
                            p.close();
                        });
                        let alpha = if t.props.fill_type == crate::domain::FillType::Solid { 1.0 } else { 0.3 };
                        let fill_c = Color::from_rgba8(
                            t.props.fill_color[0],
                            t.props.fill_color[1],
                            t.props.fill_color[2],
                            alpha,
                        );
                        frame.fill(&path, fill_c);
                    }

                    // Отрисовка линии (пунктир или сплошная)
                    if is_dashed && !is_polygon {
                        // 10 px штрих, 8 px пробел
                        let dashed_path = create_dashed_path(&screen_pts, 10.0, 8.0);
                        frame.stroke(
                            &dashed_path,
                            canvas::Stroke::default().with_color(color).with_width(width),
                        );
                    } else {
                        let path = CanvasPath::new(|p| {
                            let mut first = true;
                            for &pt in &screen_pts {
                                if first { p.move_to(pt); first = false; }
                                else { p.line_to(pt); }
                            }
                            if is_polygon { p.close(); }
                        });
                        frame.stroke(
                            &path,
                            canvas::Stroke::default().with_color(color).with_width(width),
                        );
                    }
                }
            }

            // Отрисовка точек (Waypoints)
            for wp in &app.waypoints {
                if !wp.visible {
                    continue;
                }
                let (mx, my) = merc_of(wp.lon, wp.lat);
                let pt = to_screen(mx, my);

                // Увеличенные размеры плашки
                let text_len = wp.name.chars().count() as f32;
                let box_w = (text_len * 12.0).max(40.0) + 10.0; // Шире и больше
                let box_h = 28.0; // Было 16.0
                let box_x = pt.x - box_w / 2.0;
                let box_y = pt.y - 40.0; // Подняли выше (было -24.0)

                // Линия от плашки к точке
                frame.stroke(
                    &CanvasPath::line(Point::new(pt.x, box_y + box_h), pt),
                    canvas::Stroke::default().with_color(Color::BLACK).with_width(1.5),
                );

                // Фон плашки
                let bg_color = Color::from_rgb8(wp.bg_color[0], wp.bg_color[1], wp.bg_color[2]);
                frame.fill_rectangle(
                    Point::new(box_x, box_y),
                    Size::new(box_w, box_h),
                    bg_color,
                );

                // Рамка плашки
                let border = CanvasPath::rectangle(Point::new(box_x, box_y), Size::new(box_w, box_h));
                frame.stroke(
                    &border,
                    canvas::Stroke::default().with_color(Color::BLACK).with_width(1.5),
                );

                // Текст (увеличенный шрифт)
                let fg_color = Color::from_rgb8(wp.text_color[0], wp.text_color[1], wp.text_color[2]);
                frame.fill_text(canvas::Text {
                    content: wp.name.clone(),
                    position: Point::new(box_x + box_w / 2.0, box_y + box_h / 2.0),
                    color: fg_color,
                    size: iced::Pixels(18.0), // Было 11.0
                    align_x: iced::widget::text::Alignment::Center,
                    align_y: iced::alignment::Vertical::Center,
                    ..Default::default()
                });

                // Сама точка на карте (кружок)
                let circle = CanvasPath::circle(pt, 5.0); // Чуть больше
                frame.fill(&circle, Color::WHITE);
                frame.stroke(
                    &circle,
                    canvas::Stroke::default().with_color(Color::BLACK).with_width(1.5),
                );

                let dot = CanvasPath::circle(pt, 2.0);
                frame.fill(&dot, Color::BLACK);
            }

            // Рисуем прицел, если открыта таблица точек (ваш код)
            if let Some(crate::Modal::TrackList(state)) = &app.modal {
                if let Some((sect, pt_idx)) = state.last_clicked {
                    if let Some(t) = app.tracks.iter().find(|t| t.id == state.track_id) {
                        if sect > 0 && sect <= t.segments.len() {
                            let seg = &t.segments[sect - 1];
                            if pt_idx < seg.points.len() {
                                let pt = &seg.points[pt_idx];
                                let (mx, my) = merc_of(pt.lon, pt.lat);
                                let sp = to_screen(mx, my);

                                let circle = CanvasPath::circle(sp, 16.0);
                                frame.stroke(&circle, canvas::Stroke::default().with_color(Color::WHITE).with_width(3.0));
                                frame.stroke(&circle, canvas::Stroke::default().with_color(Color::from_rgb(1.0, 0.2, 0.2)).with_width(2.0));
                                
                                let c1 = CanvasPath::line(Point::new(sp.x - 24.0, sp.y), Point::new(sp.x + 24.0, sp.y));
                                let c2 = CanvasPath::line(Point::new(sp.x, sp.y - 24.0), Point::new(sp.x, sp.y + 24.0));
                                
                                let white_cross = canvas::Stroke::default().with_color(Color::WHITE).with_width(3.0);
                                frame.stroke(&c1, white_cross);
                                frame.stroke(&c2, white_cross);

                                let red_cross = canvas::Stroke::default().with_color(Color::from_rgb(1.0, 0.2, 0.2)).with_width(1.5);
                                frame.stroke(&c1, red_cross);
                                frame.stroke(&c2, red_cross);
                            }
                        }
                    }
                }
            }
        });
        vec![geom]
    }
}
