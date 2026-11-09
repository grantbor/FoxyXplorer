use iced::widget::{button, column, row, text};
use iced::{Alignment, Element, Length};
use crate::{App, Message};

impl App {
    pub(crate) fn view_top_toolbar(&self) -> Element<'_, Message> {
        let source_label = if self.osm_active() {
            "Карта: OSM".to_string()
        } else {
            self.layers
                .get(self.active)
                .map(|l| format!("Карта: {}", l.name))
                .unwrap_or_else(|| "Карта: —".into())
        };
        
        row![
            self.btn("Карты").on_press(Message::PickKit),
            self.btn("Данные").on_press(Message::PickTrack),
            text("|"),
            text(source_label).size(13),
            text("|"),
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
        column![
            button(text("✋")).on_press(Message::Pressed),
            button(text("▲")).on_press(Message::Pressed),
            button(text("⊕")).on_press(Message::Pressed),
            button(text("●")).on_press(Message::Pressed),
            button(text("✎")).on_press(Message::Pressed),
            button(text("✕")).on_press(Message::Pressed),
        ]
        .spacing(4)
        .width(Length::Fill)
        .into()
    }
}
