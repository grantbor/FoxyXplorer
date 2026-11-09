use iced::widget::{button, container, text};
use iced::{Alignment, Element, Length, Theme};
use crate::{App, Message};

impl App {
    pub fn btn<'a>(&'a self, label: impl Into<String>) -> iced::widget::Button<'a, Message> {
        button(text(label.into()))
            .padding(native_theme_iced::button_padding(&self.resolved_theme))
            .style(native_theme_iced::styles::button_primary(&self.resolved_theme))
    }

    pub fn btn_secondary<'a>(&'a self, label: impl Into<String>) -> iced::widget::Button<'a, Message> {
        button(text(label.into()))
            .padding(native_theme_iced::button_padding(&self.resolved_theme))
            .style(native_theme_iced::styles::button(&self.resolved_theme))
    }

    pub fn icon_button<'a>(&'a self, icon: &'static [u8], msg: Message) -> iced::widget::Button<'a, Message> {
        button(lucide_iced::themed_icon(icon, 16.0))
            .padding(native_theme_iced::button_padding(&self.resolved_theme))
            .style(native_theme_iced::styles::button(&self.resolved_theme))
            .on_press(msg)
    }

    pub fn icon_button_disabled<'a>(&'a self, icon: &'static [u8]) -> iced::widget::Button<'a, Message> {
        button(lucide_iced::themed_icon(icon, 16.0))
            .padding(native_theme_iced::button_padding(&self.resolved_theme))
            .style(native_theme_iced::styles::button(&self.resolved_theme))
    }

    pub fn visibility_button<'a>(&'a self, is_visible: bool, msg: Message) -> iced::widget::Button<'a, Message> {
        let icon = if is_visible {
            lucide_iced::bytes::EYE
        } else {
            lucide_iced::bytes::EYE_CLOSED
        };

        button(lucide_iced::themed_icon(icon, 16.0))
            .width(16)
            .height(16)
            .padding(0)
            .style(native_theme_iced::styles::button(&self.resolved_theme))
            .on_press(msg)
    }

    pub fn list_item<'a>(&'a self, label: &str, is_selected: bool, msg: Message) -> Element<'a, Message> {
        let content = label.to_string();

        let style = move |theme: &Theme, status| {
            let palette = theme.extended_palette();
            
            let (bg, text_color) = if is_selected {
                (Some(palette.primary.weak.color.into()), palette.primary.weak.text)
            } else {
                match status {
                    iced::widget::button::Status::Hovered => (
                        Some(palette.background.weak.color.into()), 
                        palette.background.weak.text
                    ),
                    _ => (None, palette.background.base.text),
                }
            };

            iced::widget::button::Style {
                background: bg,
                text_color,
                border: iced::Border { radius: 4.0.into(), ..Default::default() },
                ..Default::default()
            }
        };

        button(
            container(
                text(content)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .size(14)
            )
            .width(Length::Fill)
            .align_x(Alignment::Start)
        )
        .on_press(msg)
        .width(Length::Fill)
        .padding([4, 8])
        .style(style)
        .into()
    }
}
