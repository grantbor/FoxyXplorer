use iced::widget::{button, container, svg, text, tooltip, Button};
use iced::{Alignment, Color, Element, Length, Theme};
use crate::{App, Message};

/// Оборачивает элемент в тултип с подсказкой снизу и системным стилем.
pub fn with_tooltip<'a>(
    content: impl Into<Element<'a, Message>>,
    tooltip_text: impl Into<String>,
) -> Element<'a, Message> {
    tooltip(
        content,
        container(text(tooltip_text.into()))
            .padding([4, 8])
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                    background: Some(palette.background.weak.color.into()),
                    text_color: Some(palette.background.weak.text),
                    border: iced::Border {
                        color: palette.background.strong.color,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            }),
        iced::widget::tooltip::Position::Bottom,
    )
    .into()
}

impl App {
    // =========================================================
    // Основные кнопки
    // =========================================================
    pub fn btn<'a>(&'a self, label: impl Into<String>) -> Button<'a, Message> {
        button(text(label.into()))
            .padding(native_theme_iced::button_padding(&self.resolved_theme))
            .style(native_theme_iced::styles::button_primary(&self.resolved_theme))
    }

    pub fn btn_secondary<'a>(&'a self, label: impl Into<String>) -> Button<'a, Message> {
        button(text(label.into()))
            .padding(native_theme_iced::button_padding(&self.resolved_theme))
            .style(native_theme_iced::styles::button(&self.resolved_theme))
    }

    // =========================================================
    // Кнопки с иконками Lucide
    // =========================================================
    pub fn icon_button<'a>(&'a self, icon: &'static [u8], msg: Message) -> Button<'a, Message> {
        button(
            container(lucide_iced::themed_icon(icon, 20.0))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .width(Length::Fixed(32.0))
        .height(Length::Fixed(32.0))
        .padding(0)
        .style(native_theme_iced::styles::button(&self.resolved_theme))
        .on_press(msg)
    }

    pub fn icon_button_disabled<'a>(&'a self, icon: &'static [u8]) -> Button<'a, Message> {
        button(
            container(lucide_iced::themed_icon(icon, 20.0))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .width(Length::Fixed(32.0))
        .height(Length::Fixed(32.0))
        .padding(0)
        .style(native_theme_iced::styles::button(&self.resolved_theme))
    }

    // =========================================================
    // Кнопка-переключатель (красная при активном, серая при неактивном)
    // =========================================================
    pub fn icon_toggle_button<'a>(
        &'a self,
        icon: &'static [u8],
        active: bool,
        msg: Message,
    ) -> Button<'a, Message> {
        let color = if active {
            Color::from_rgb(1.0, 0.25, 0.25) // красный — включено
        } else {
            Color::from_rgb(0.55, 0.55, 0.55) // серый — выключено
        };

        let svg_icon = svg(svg::Handle::from_memory(icon))
            .width(Length::Fixed(20.0))
            .height(Length::Fixed(20.0))
            .style(move |_, _| svg::Style {
                color: Some(color),
                ..Default::default()
            });

        button(
            container(svg_icon)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .width(Length::Fixed(32.0))
        .height(Length::Fixed(32.0))
        .padding(0)
        .style(native_theme_iced::styles::button(&self.resolved_theme))
        .on_press(msg)
    }

    // =========================================================
    // Кнопка-действие (иконка без состояния)
    // =========================================================
    pub fn icon_action_button<'a>(
        &'a self,
        icon: &'static [u8],
        msg: Message,
    ) -> Button<'a, Message> {
        let svg_icon = svg(svg::Handle::from_memory(icon))
            .width(Length::Fixed(20.0))
            .height(Length::Fixed(20.0));

        button(
            container(svg_icon)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .width(Length::Fixed(32.0))
        .height(Length::Fixed(32.0))
        .padding(0)
        .style(native_theme_iced::styles::button(&self.resolved_theme))
        .on_press(msg)
    }

    // =========================================================
    // Кнопка видимости (глаз / перечёркнутый глаз)
    // =========================================================
    pub fn visibility_button<'a>(
        &'a self,
        is_visible: bool,
        msg: Message,
    ) -> Button<'a, Message> {
        let icon = if is_visible {
            lucide_iced::bytes::EYE
        } else {
            lucide_iced::bytes::EYE_CLOSED
        };

        button(
            container(lucide_iced::themed_icon(icon, 16.0))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .width(Length::Fixed(24.0))
        .height(Length::Fixed(24.0))
        .padding(0)
        .style(native_theme_iced::styles::button(&self.resolved_theme))
        .on_press(msg)
    }

    // =========================================================
    // Элемент списка (строка, которая подсвечивается при выделении)
    // =========================================================
    pub fn list_item<'a>(
        &'a self,
        label: &str,
        is_selected: bool,
        msg: Message,
    ) -> Element<'a, Message> {
        let content = label.to_string();

        let style = move |theme: &Theme, status| {
            let palette = theme.extended_palette();

            let (bg, text_color) = if is_selected {
                (Some(palette.primary.weak.color.into()), palette.primary.weak.text)
            } else {
                match status {
                    iced::widget::button::Status::Hovered => (
                        Some(palette.background.weak.color.into()),
                        palette.background.weak.text,
                    ),
                    _ => (None, palette.background.base.text),
                }
            };

            iced::widget::button::Style {
                background: bg,
                text_color,
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        };

        button(
            container(
                text(content)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .size(14),
            )
            .width(Length::Fill)
            .align_x(Alignment::Start),
        )
        .on_press(msg)
        .width(Length::Fill)
        .padding([4, 8])
        .style(style)
        .into()
    }
}
