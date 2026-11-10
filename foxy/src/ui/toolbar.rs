use iced::widget::{button, column, container, row, text, tooltip};
use iced::{Alignment, Element, Length, Theme};
use crate::{App, Message};

impl App {
    pub(crate) fn view_top_toolbar(&self) -> Element<'_, Message> {
        // Переменная source_label полностью удалена

        row![
            self.btn("Карты").on_press(Message::PickKit),
            self.btn("Данные").on_press(Message::PickTrack),
            // Удален разделитель и текст с названием карты
            self.btn_secondary(if self.show_grid { "Сетка: вкл" } else { "Сетка: выкл" })
                .on_press(Message::ToggleGrid),
            self.btn_secondary(if self.show_labels { "Подписи: вкл" } else { "Подписи: выкл" })
                .on_press(Message::ToggleLabels),
            self.btn_secondary("Fit").on_press(Message::Fit),
            self.btn_secondary(self.format_name()).on_press(Message::CycleFormat),
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
                container(lucide_iced::themed_icon(icon, 32.0))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill) 
                    .center_y(Length::Fill) 
            )
            .width(Length::Fixed(48.0))
            .height(Length::Fixed(48.0))
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
            make_tool_btn(crate::Tool::Pan, lucide_iced::bytes::HAND, "Pan (Q / Space)"),
            make_tool_btn(crate::Tool::SelectObjects, lucide_iced::bytes::MOUSE_POINTER, "Select Objects (S)"),
            make_tool_btn(crate::Tool::SelectPoints, lucide_iced::bytes::MOUSE_POINTER_2, "Select Points (D)"),
            make_tool_btn(crate::Tool::DrawTrack, lucide_iced::bytes::PEN_TOOL, "Draw Track (T)"),
            make_tool_btn(crate::Tool::Freehand, lucide_iced::bytes::PENCIL, "Freehand (F)"),
            make_tool_btn(crate::Tool::AddWaypoint, lucide_iced::bytes::CIRCLE_PLUS, "Add Waypoint (W)"),
            make_tool_btn(crate::Tool::Ruler, lucide_iced::bytes::RULER, "Ruler (R)"),
        ]
        .spacing(4)
        .padding([10, 0])
        .width(Length::Fixed(56.0))
        .into()
    }
}
