use iced::widget::canvas::{self, Geometry};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

pub struct QrProgram { pub url: String }
impl canvas::Program<crate::Message> for QrProgram {
    type State = ();
    fn draw(&self, _: &Self::State, renderer: &Renderer, _: &Theme,
            bounds: Rectangle, _: iced::mouse::Cursor) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), Color::WHITE);
        if let Ok(qr) = qrcode::QrCode::new(self.url.as_bytes()) {
            let modules = qr.width();
            let side = (modules + 8) as f32; // quiet zone: 4 modules on each side
            let cell = bounds.width.min(bounds.height) / side;
            let ox = (bounds.width - side * cell) / 2.0;
            let oy = (bounds.height - side * cell) / 2.0;
            for (i, c) in qr.to_colors().into_iter().enumerate() {
                if c == qrcode::Color::Dark {
                    let x = (i % modules) as f32;
                    let y = (i / modules) as f32;
                    frame.fill_rectangle(Point::new(ox + (x + 4.0) * cell, oy + (y + 4.0) * cell),
                                         Size::new(cell + 0.05, cell + 0.05), Color::BLACK);
                }
            }
        }
        vec![frame.into_geometry()]
    }
}
