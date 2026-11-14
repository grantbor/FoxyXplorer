use iced::widget::{button, column, container, row, text, tooltip, Space};
use iced::{Alignment, Element, Length, Theme};
use crate::{App, Message};
use crate::ui::controls::with_tooltip;
use crate::AppTheme;

impl App {
    pub(crate) fn view_top_toolbar(&self) -> Element<'_, Message> {
        row![
            with_tooltip(
                self.btn_secondary("Карты").on_press(Message::PickKit),
                "Загрузить комплект карт или карту"
            ),
            with_tooltip(
                self.btn_secondary("Геоданные").on_press(Message::PickTrack),
                "Загрузить треки / точки (GPX, PLT, WPT, MS)"
            ),
            with_tooltip(
                self.icon_toggle_button(
                    lucide_iced::bytes::GRID_3X3,
                    self.show_grid,
                    Message::ToggleGrid,
                ),
                if self.show_grid { "Выключить сетку привязки" } else { "Включить сетку привязки" }
            ),
            with_tooltip(
                self.icon_toggle_button(
                    lucide_iced::bytes::GRIP,
                    self.show_labels,
                    Message::ToggleLabels,
                ),
                if self.show_labels { "Выключить имена узлов" } else { "Включить имена узлов" }
            ),
            with_tooltip(
                self.icon_action_button(
                    lucide_iced::bytes::FULLSCREEN,
                    Message::Fit,
                ),
                "Вместить карту в экран"
            ),
            with_tooltip(
                self.btn_secondary(self.format_short_name()).on_press(Message::CycleFormat),
                format!("Формат координат (сейчас {})", self.format_name())
            ),
            with_tooltip(
                self.icon_button(
                    if self.current_theme == AppTheme::Dark {
                        lucide_iced::bytes::SUN
                    } else {
                        lucide_iced::bytes::MOON
                    },
                    Message::ToggleTheme,
                ),
                "Переключить тему",
            ),

            Space::new().width(Length::Fill),

            with_tooltip(
                self.icon_button(lucide_iced::bytes::FOLDER_OPEN, Message::LoadSessionPrompt),
                "Загрузить сессию"
            ),
            with_tooltip(
                self.icon_button(lucide_iced::bytes::SAVE, Message::SaveSessionAs),
                "Сохранить сессию"
            ),
            text("|").size(18),
            with_tooltip(
                self.icon_button(lucide_iced::bytes::ERASER, Message::ConfirmClearAll),
                "Очистить сессию"
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
            make_tool_btn(crate::Tool::Pan, lucide_iced::bytes::HAND, "Панормаирование (Q / Пробел (нажать и удерживать, временное переключение))"),
            make_tool_btn(crate::Tool::SelectObjects, lucide_iced::bytes::MOUSE_POINTER, "Выбор треков или путевых точек (S)"),
            make_tool_btn(crate::Tool::SelectPoints, lucide_iced::bytes::MOUSE_POINTER_2, "Выбор точек трека(D)"),
            make_tool_btn(crate::Tool::DrawTrack, lucide_iced::bytes::PEN_TOOL, "Рисование трек поточечно (T)"),
            make_tool_btn(crate::Tool::Freehand, lucide_iced::bytes::PENCIL, "Свободное рисование трека (F)"),
            make_tool_btn(crate::Tool::AddWaypoint, lucide_iced::bytes::CIRCLE_PLUS, "Добавление путевой точки (W)"),
            make_tool_btn(crate::Tool::Ruler, lucide_iced::bytes::RULER, "Линейка (R)"),
        ]
        .spacing(4)
        .padding([4, 4])
        .width(Length::Fixed(56.0))
        .into()
    }
}
