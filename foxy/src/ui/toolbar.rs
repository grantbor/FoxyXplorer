use iced::widget::{button, column, container, row, text, tooltip, Space};
use iced::{Alignment, Element, Length, Theme};
use crate::{App, Message};
use crate::ui::controls::with_tooltip;
use crate::AppTheme;
use crate::t;

impl App {
    pub(crate) fn view_top_toolbar(&self) -> Element<'_, Message> {
        row![
            with_tooltip(
                self.btn(t!("msg_0180")).on_press(Message::PickKit),
                t!("msg_0181")
            ),
            with_tooltip(
                self.btn_secondary(t!("msg_0182")).on_press(Message::PickTrack),
                t!("msg_0183")
            ),
            with_tooltip(
                self.icon_toggle_button(
                    lucide_iced::bytes::GRID_3X3,
                    self.show_grid,
                    Message::ToggleGrid,
                ),
                if self.show_grid { t!("msg_0184") } else { t!("msg_0185") }
            ),
            with_tooltip(
                self.icon_toggle_button(
                    lucide_iced::bytes::GRIP,
                    self.show_labels,
                    Message::ToggleLabels,
                ),
                if self.show_labels { t!("msg_0186") } else { t!("msg_0187") }
            ),
            with_tooltip(
                self.icon_action_button(
                    lucide_iced::bytes::FULLSCREEN,
                    Message::Fit,
                ),
                t!("msg_0188")
            ),
            with_tooltip(
                self.btn_secondary(self.format_short_name()).on_press(Message::CycleFormat),
                format!("{} {})", t!("msg_0206"), self.format_name())
            ),

            Space::new().width(Length::Fill),

            with_tooltip(
                self.icon_button(lucide_iced::bytes::FOLDER_OPEN, Message::LoadSessionPrompt),
                t!("msg_0190")
            ),
            with_tooltip(
                self.icon_button(lucide_iced::bytes::SAVE, Message::SaveSessionAs),
                t!("msg_0191")
            ),
            container(Space::new().width(Length::Fixed(8.0))),   // отступ
            with_tooltip(
                self.icon_button(lucide_iced::bytes::ERASER, Message::ConfirmClearAll),
                t!("msg_0192")
            ),
            text(" | ").size(18),     
            with_tooltip(
                self.icon_button(
                    if self.current_theme == AppTheme::Dark {
                        lucide_iced::bytes::SUN
                    } else {
                        lucide_iced::bytes::MOON
                    },
                    Message::ToggleTheme,
                ),
                t!("msg_0189"),
            ),
            with_tooltip(
                self.icon_button(lucide_iced::bytes::SETTINGS, Message::OpenSettings),
                         t!("msg_0202")
            ),
            with_tooltip(
                button(
                    container(
                        iced::widget::svg(iced::widget::svg::Handle::from_memory(crate::ICON_SVG))
                            .width(Length::Fixed(22.0))
                            .height(Length::Fixed(22.0))
                    )
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                )
                .width(Length::Fixed(32.0))
                .height(Length::Fixed(32.0))
                .padding(0)
                .style(native_theme_iced::styles::button(&self.resolved_theme))
                .on_press(Message::OpenAbout),
                t!("msg_0179"),
            ),
            
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
    }

    pub(crate) fn view_tool_palette(&self) -> Element<'_, Message> {
        // Теперь функция принимает текст подсказки
        let make_tool_btn = |tool: crate::Tool, icon: &'static [u8], tooltip_text: &'static str| -> Element<'_, Message> {
            let is_active = self.active_tool == tool;
            
            let btn = button(
                container(lucide_iced::themed_icon(icon, 26.0))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill) 
                    .center_y(Length::Fill) 
            )
            .width(Length::Fixed(40.0))
            .height(Length::Fixed(40.0))
            .padding(0)
            .on_press(Message::SelectTool(tool));

            let styled_btn = if is_active {
                btn.style(native_theme_iced::styles::button_primary(&self.resolved_theme))
            } else {
                btn.style(native_theme_iced::styles::button(&self.resolved_theme))
            };

            // Оборачиваем в тултип (появляется справа)
            tooltip(
                styled_btn,
                container(text(tooltip_text)).padding([4, 8]).style(|theme: &Theme| {
                    let palette = theme.extended_palette();
                    container::Style {
                        background: Some(palette.background.weak.color.into()),
                        text_color: Some(palette.background.weak.text),
                        border: iced::Border { color: palette.background.strong.color, width: 1.0, radius: 4.0.into() },
                        ..Default::default()
                    }
                }),
                iced::widget::tooltip::Position::Right,
            ).into()
        };

        column![
            make_tool_btn(crate::Tool::Pan, lucide_iced::bytes::HAND, t!("msg_0193")),
            make_tool_btn(crate::Tool::SelectObjects, lucide_iced::bytes::MOUSE_POINTER, t!("msg_0194")),
            make_tool_btn(crate::Tool::SelectPoints, lucide_iced::bytes::MOUSE_POINTER_2, t!("msg_0195")),
            make_tool_btn(crate::Tool::DrawTrack, lucide_iced::bytes::PEN_TOOL, t!("msg_0196")),
            make_tool_btn(crate::Tool::Freehand, lucide_iced::bytes::PENCIL, t!("msg_0197")),
            make_tool_btn(crate::Tool::AddWaypoint, lucide_iced::bytes::CIRCLE_PLUS, t!("msg_0198")),
            make_tool_btn(crate::Tool::Ruler, lucide_iced::bytes::RULER, t!("msg_0199")),
        ]
        .spacing(4)
        .padding([4, 4])
        .width(Length::Fixed(56.0))
        .into()
    }
}
