use iced::widget::canvas::{self, Geometry, Path as CanvasPath};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

use crate::util::geo::merc_of;
use crate::{App, Message};

pub struct TrackProgram<'a> {
    pub app: &'a App,
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
                let width = t.props.line_width;

                for seg in &t.segments {
                    if seg.points.len() < 2 {
                        continue;
                    }

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
                        if t.props.poly_type == crate::domain::PolyType::Polygon {
                            p.close();
                        }
                    });

                    if t.props.poly_type == crate::domain::PolyType::Polygon
                        && t.props.fill_type != crate::domain::FillType::None
                    {
                        let alpha = if t.props.fill_type == crate::domain::FillType::Solid {
                            1.0
                        } else {
                            0.3
                        };
                        let fill_c = Color::from_rgba8(
                            t.props.fill_color[0],
                            t.props.fill_color[1],
                            t.props.fill_color[2],
                            alpha,
                        );
                        frame.fill(&path, fill_c);
                    }

                    frame.stroke(
                        &path,
                        canvas::Stroke::default().with_color(color).with_width(width),
                    );
                }
            }

            for wp in &app.waypoints {
                if !wp.visible {
                    continue;
                }
                let (mx, my) = merc_of(wp.lon, wp.lat);
                let pt = to_screen(mx, my);

                let text_len = wp.name.chars().count() as f32;
                let box_w = (text_len * 7.0).max(30.0) + 6.0;
                let box_h = 16.0;
                let box_x = pt.x - box_w / 2.0;
                let box_y = pt.y - 24.0;

                frame.stroke(
                    &CanvasPath::line(Point::new(pt.x, box_y + box_h), pt),
                    canvas::Stroke::default().with_color(Color::BLACK).with_width(1.0),
                );

                let bg_color =
                    Color::from_rgb8(wp.bg_color[0], wp.bg_color[1], wp.bg_color[2]);
                frame.fill_rectangle(
                    Point::new(box_x, box_y),
                    Size::new(box_w, box_h),
                    bg_color,
                );

                let border =
                    CanvasPath::rectangle(Point::new(box_x, box_y), Size::new(box_w, box_h));
                frame.stroke(
                    &border,
                    canvas::Stroke::default().with_color(Color::BLACK).with_width(1.0),
                );

                let fg_color =
                    Color::from_rgb8(wp.text_color[0], wp.text_color[1], wp.text_color[2]);
                frame.fill_text(canvas::Text {
                    content: wp.name.clone(),
                    position: Point::new(box_x + box_w / 2.0, box_y + box_h / 2.0),
                    color: fg_color,
                    size: iced::Pixels(11.0),
                    align_x: iced::widget::text::Alignment::Center,
                    align_y: iced::alignment::Vertical::Center,
                    ..Default::default()
                });

                let circle = CanvasPath::circle(pt, 4.0);
                frame.fill(&circle, Color::WHITE);
                frame.stroke(
                    &circle,
                    canvas::Stroke::default().with_color(Color::BLACK).with_width(1.5),
                );

                let dot = CanvasPath::circle(pt, 1.5);
                frame.fill(&dot, Color::BLACK);
            }
        });
        vec![geom]
    }
}
