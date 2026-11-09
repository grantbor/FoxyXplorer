use iced::widget::canvas::{self, Geometry, Path as CanvasPath};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

use crate::util::geo::{lonlat_of, merc_of};
use crate::{App, Message};

pub struct GridProgram<'a> {
    pub app: &'a App,
}

fn draw_scale_bar(
    frame: &mut canvas::Frame,
    bounds: Rectangle,
    scale_m_per_px: f64,
    cx: f64,
    cy: f64,
) {
    let (_, lat_c) = lonlat_of(cx, cy);
    let cos_lat = lat_c.to_radians().cos().abs().max(1e-6);
    let ground_m_per_px = scale_m_per_px * cos_lat;

    let target_px = 140.0;
    let raw = ground_m_per_px * target_px;
    let exp = raw.log10().floor();
    let mag = 10f64.powf(exp);
    let norm = raw / mag;
    let nice = if norm < 1.5 {
        1.0
    } else if norm < 3.5 {
        2.0
    } else if norm < 7.5 {
        5.0
    } else {
        10.0
    };
    let distance_m = nice * mag;
    let bar_w = (distance_m / ground_m_per_px) as f32;

    let label = if distance_m >= 1000.0 {
        format!("{:.0} км", distance_m / 1000.0)
    } else if distance_m >= 1.0 {
        format!("{:.0} м", distance_m)
    } else {
        format!("{:.0} см", distance_m * 100.0)
    };

    let margin = 16.0;
    let bar_h = 8.0;
    let label_h = 18.0;
    let bg_pad = 6.0;
    let x0 = margin;
    let y0 = bounds.height - margin - label_h - bar_h;
    let bar_y = y0 + label_h;

    frame.fill_rectangle(
        Point::new(x0 - bg_pad, y0 - bg_pad),
        Size::new(bar_w + 2.0 * bg_pad, label_h + bar_h + 2.0 * bg_pad),
        Color::from_rgba8(0, 0, 0, 0.55),
    );

    let n_seg = 4;
    let seg_w = bar_w / n_seg as f32;
    let color_a = Color::from_rgba8(240, 240, 240, 1.0);
    let color_b = Color::from_rgba8(60, 60, 60, 1.0);
    for i in 0..n_seg {
        let color = if i % 2 == 0 { color_a } else { color_b };
        frame.fill_rectangle(
            Point::new(x0 + i as f32 * seg_w, bar_y),
            Size::new(seg_w, bar_h),
            color,
        );
    }

    let border = CanvasPath::rectangle(Point::new(x0, bar_y), Size::new(bar_w, bar_h));
    frame.stroke(
        &border,
        canvas::Stroke::default()
            .with_color(Color::WHITE)
            .with_width(1.0),
    );

    frame.fill_text(canvas::Text {
        content: "0".to_string(),
        position: Point::new(x0, bar_y - 2.0),
        color: Color::WHITE,
        size: iced::Pixels(11.0),
        align_x: iced::widget::text::Alignment::Left,
        align_y: iced::alignment::Vertical::Bottom,
        ..Default::default()
    });
    frame.fill_text(canvas::Text {
        content: label,
        position: Point::new(x0 + bar_w, bar_y - 2.0),
        color: Color::WHITE,
        size: iced::Pixels(11.0),
        align_x: iced::widget::text::Alignment::Right,
        align_y: iced::alignment::Vertical::Bottom,
        ..Default::default()
    });
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

            if app.show_grid {
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
                        canvas::Stroke::default()
                            .with_color(line_color)
                            .with_width(1.5),
                    );
                }

                if let Some(g) = &app.grid {
                    let node_color = Color::from_rgba8(255, 40, 40, 1.0);
                    for n in &g.nodes {
                        let (mx, my) = merc_of(n.lon, n.lat);
                        let pt = to_screen(mx, my);
                        frame.fill_rectangle(
                            Point::new(pt.x - 1.5, pt.y - 1.5),
                            Size::new(3.0, 3.0),
                            node_color,
                        );
                        if app.show_labels && !n.name.is_empty() {
                            let size_f: f32 = 13.0;
                            let pad: f32 = 3.0;
                            let bg_color = Color::from_rgba8(0, 0, 0, 0.7);
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
            }

            draw_scale_bar(frame, bounds, app.scale, app.cx, app.cy);
        });
        vec![geom]
    }
}
