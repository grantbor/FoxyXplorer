use iced::widget::canvas::{self, Geometry};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use crate::{App, Message};

pub struct DemProgram<'a> { pub app: &'a App }
pub struct DemLegendProgram<'a> { pub app: &'a App }

impl<'a> canvas::Program<Message> for DemProgram<'a> {
    type State = ();

    fn draw(
        &self, _: &Self::State, renderer: &Renderer, _: &Theme,
        bounds: Rectangle, _: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let view = self.app.current_dem_view();
        if let Some(overlay) = self.app.dem.visible(view) {
            frame.draw_image(
                Rectangle { x: 0.0, y: 0.0, width: bounds.width, height: bounds.height },
                canvas::Image::new(overlay.image.clone()),
            );
        }
        vec![frame.into_geometry()]
    }
}

impl<'a> canvas::Program<Message> for DemLegendProgram<'a> {
    type State = ();

    fn draw(
        &self, _: &Self::State, renderer: &Renderer, _: &Theme,
        bounds: Rectangle, _: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let view = self.app.current_dem_view();
        if let Some(overlay) = self.app.dem.visible(view) {
            let height = (bounds.height - 100.0).clamp(60.0, 260.0);
            let top = 28.0;
            let right = bounds.width - 12.0;
            let left = (right - 96.0).max(0.0);
            frame.fill_rectangle(
                Point::new(left - 8.0, top - 22.0),
                Size::new(104.0, height + 62.0),
                Color::from_rgba(0.05, 0.06, 0.08, 0.78),
            );
            frame.fill_text(canvas::Text {
                content: "DEM (m)".into(),
                position: Point::new(left, top - 7.0),
                size: iced::Pixels(12.0), color: Color::WHITE,
                ..Default::default()
            });
            let bar_left = left + 3.0;
            let bar_top = top + 7.0;
            let steps = height.round() as u32;
            for i in 0..steps {
                let t = 1.0 - i as f32 / (steps.saturating_sub(1).max(1)) as f32;
                let [r, g, b] = crate::dem::palette(t);
                frame.fill_rectangle(
                    Point::new(bar_left, bar_top + i as f32),
                    Size::new(18.0, 1.0),
                    Color::from_rgb8(r, g, b),
                );
            }
            for i in 0..=4 {
                let t = 1.0 - i as f32 / 4.0;
                let elevation = overlay.lower + (overlay.upper - overlay.lower) * t;
                frame.fill_text(canvas::Text {
                    content: format!("{elevation:.0}"),
                    position: Point::new(bar_left + 25.0, bar_top + height * (i as f32 / 4.0) - 7.0),
                    size: iced::Pixels(12.0), color: Color::WHITE,
                    ..Default::default()
                });
            }
            // The base map's existing attribution remains in its own canvas.
            frame.fill_text(canvas::Text {
                content: "Copernicus DEM".into(),
                position: Point::new(left, bar_top + height + 20.0),
                size: iced::Pixels(10.0), color: Color::WHITE,
                ..Default::default()
            });
        }
        vec![frame.into_geometry()]
    }
}
