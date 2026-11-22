//! FoxyXplorer widget styling based solely on iced::Theme (iced 0.14).
//! Avoid storing a second independent theme state in App.
use iced::{Padding, Theme};


pub(crate) const fn border_radius() -> f32 { 4.0 }
pub(crate) fn button_padding() -> Padding { Padding::from([5, 10]) }
pub(crate) fn input_padding() -> Padding { Padding::from([6, 10]) }

pub(crate) fn button(theme: &Theme, status: iced::widget::button::Status) -> iced::widget::button::Style {
    let mut style = iced::widget::button::secondary(theme, status);
    style.border.radius = border_radius().into();
    style
}
pub(crate) fn button_primary(theme: &Theme, status: iced::widget::button::Status) -> iced::widget::button::Style {
    let mut style = iced::widget::button::primary(theme, status);
    style.border.radius = border_radius().into();
    style
}
pub(crate) fn text_input(theme: &Theme, status: iced::widget::text_input::Status) -> iced::widget::text_input::Style {
    let mut style = iced::widget::text_input::default(theme, status);
    style.border.radius = border_radius().into();
    style
}
pub(crate) fn pick_list(theme: &Theme, status: iced::widget::pick_list::Status) -> iced::widget::pick_list::Style {
    let mut style = iced::widget::pick_list::default(theme, status);
    style.border.radius = border_radius().into();
    style
}
