//! Единое оформление обычных WPT и информационного Wiki-WPT.
use iced::widget::canvas::{self, Path as CanvasPath};
use iced::{Color, Point, Size};
use iced::advanced::text::{Paragraph, Text, LineHeight, Shaping};

pub(super) fn draw_waypoint_label(
    frame: &mut canvas::Frame,
    pt: Point,
    name: &str,
    bg_rgb: [u8; 3],
    fg_rgb: [u8; 3],
    selected: bool,
) {
    type Para = <iced::Renderer as iced::advanced::text::Renderer>::Paragraph;

    let paragraph = Para::with_text(Text {
        content: name,
        bounds: iced::Size::INFINITE,
        size: iced::Pixels(14.0),
        line_height: LineHeight::default(),
        font: iced::Font::default(),
        shaping: Shaping::Basic,
        align_x: iced::widget::text::Alignment::Left,
        align_y: iced::alignment::Vertical::Top,
        wrapping: iced::advanced::text::Wrapping::None,
    });
    let text_w = paragraph.min_bounds().width;

    let padding_h = 8.0;
    let box_w = (text_w + 2.0 * padding_h).max(40.0);
    let box_h = 24.0;
    let box_x = pt.x - box_w / 2.0;
    let box_y = pt.y - 40.0;

    frame.stroke(&CanvasPath::line(Point::new(pt.x, box_y + box_h), pt), canvas::Stroke::default().with_color(Color::BLACK).with_width(1.5));

    let bg_color = Color::from_rgb8(bg_rgb[0], bg_rgb[1], bg_rgb[2]);
    frame.fill_rectangle(Point::new(box_x, box_y), Size::new(box_w, box_h), bg_color);

    let border_color = if selected { Color::from_rgb8(0x2a, 0x7f, 0xff) } else { Color::BLACK };
    let border_width = if selected { 2.5 } else { 1.5 };

    let border = CanvasPath::rectangle(Point::new(box_x, box_y), Size::new(box_w, box_h));
    frame.stroke(&border, canvas::Stroke::default().with_color(border_color).with_width(border_width));

    let fg_color = Color::from_rgb8(fg_rgb[0], fg_rgb[1], fg_rgb[2]);
    frame.fill_text(canvas::Text {
        content: name.to_string(),
        position: Point::new(box_x + box_w / 2.0, box_y + box_h / 2.0),
        color: fg_color,
        size: iced::Pixels(14.0),
        align_x: iced::widget::text::Alignment::Center,
        align_y: iced::alignment::Vertical::Center,
        ..Default::default()
    });

    let circle = CanvasPath::circle(pt, 5.0);
    frame.fill(&circle, Color::WHITE);
    frame.stroke(&circle, canvas::Stroke::default().with_color(Color::BLACK).with_width(1.5));

    let dot = CanvasPath::circle(pt, 2.0);
    frame.fill(&dot, Color::BLACK);
}
