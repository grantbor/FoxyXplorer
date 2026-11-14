use iced::widget::{
    column, container, mouse_area, row, scrollable, text, text_input, tooltip, Space, stack,
};
use iced::{Alignment, Color, Element, Length, Theme};
use crate::{App, Message, ContextMenuItem};
use crate::ui::controls::with_tooltip;


impl App {
    fn search_input<'a>(
        &'a self,
        placeholder: &'a str,
        value: &'a str,
        on_input: fn(String) -> Message,
        on_clear: Message,
    ) -> Element<'a, Message> {
        let mut pad = native_theme_iced::input_padding(&self.resolved_theme);
        pad.right += 22.0;

        let input = text_input(placeholder, value)
            .padding(pad)
            .style(native_theme_iced::styles::text_input(&self.resolved_theme))
            .on_input(on_input)
            .size(12);

        // Второй слой stack! присутствует всегда — либо кнопка, либо пустышка.
        // Это стабилизирует дерево виджетов, и text_input не пересоздаётся.
        let trailing: Element<'a, Message> = if value.is_empty() {
            Space::new()
                .width(Length::Fixed(22.0))
                .height(Length::Fixed(22.0))
                .into()
        } else {
            iced::widget::button(
                iced::widget::container(
                    lucide_iced::themed_icon(lucide_iced::bytes::CIRCLE_X, 14.0)
                )
                .center_x(Length::Fill)
                .center_y(Length::Fill)
            )
            .width(Length::Fixed(22.0))
            .height(Length::Fixed(22.0))
            .padding(0)
            .style(native_theme_iced::styles::button(&self.resolved_theme))
            .on_press(on_clear)
            .into()
        };

        stack![
            input,
            container(trailing)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::End)
                .align_y(Alignment::Center)
                .padding([0, 3]),
        ]
        .into()
    }
    
    pub(crate) fn view_right_panel(&self) -> Element<'_, Message> {
        column![
            self.view_maps_section(),
            self.view_tracks_section(),
            self.view_waypoints_section(),
        ]
        .spacing(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    pub(crate) fn view_maps_section(&self) -> Element<'_, Message> {
        let kit_name = self.layers.first().map(|l| l.kit_name.as_str()).unwrap_or("");
        
        let header = row![
            text(format!("Карты ({})", self.layers.len() + 1)).size(14),
            Space::new().width(Length::Fill),
            if !kit_name.is_empty() {
                text(kit_name).size(11).color(Color::from_rgb(0.6, 0.6, 0.6))
            } else {
                text("").size(11)
            }
        ]
        .align_y(Alignment::Center);
        
        let mut list = column![].spacing(2).padding([0.0, 12.0]);
        
        // Список локальных карт
        for (i, l) in self.layers.iter().enumerate() {
            list = list.push(
                self.list_item(&l.name, self.active == i, Message::Layer(i))
            );
        }
        
        // Карта OSM
        let osm_idx = self.layers.len();
        list = list.push(
            self.list_item("Openstreetmap (HOT)", self.osm_active(), Message::Layer(osm_idx))
        );
        
        column![
            header,
            container(scrollable(list).height(Length::Fixed(150.0))).padding(2),
        ]
        .spacing(4)
        .width(Length::Fill)
        .into()
    }

    fn view_tracks_section(&self) -> Element<'_, Message> {
        let header = text(format!("Треки ({})", self.tracks.len())).size(14);

        let all_visible = self.tracks.iter().all(|t| t.visible);
        let top_bar = row![
            with_tooltip(
                self.icon_button(lucide_iced::bytes::EYE, Message::ToggleAllTracksVisible),
                if all_visible { "Скрыть все треки" } else { "Показать все треки" }
            ),
            self.search_input(
                "Поиск треков...",
                &self.search_tracks,
                Message::SearchTracksChanged,
                Message::SearchTracksChanged(String::new()),
            ),
        ]
        .spacing(4);

        let cols = row![
            Space::new().width(24.0),
            Space::new().width(16.0),
            container(text("Имя").size(11)).width(Length::Fill),
            container(text("Точек").size(11)).width(Length::Fixed(50.0)).align_x(Alignment::End),
            container(text("Расст.").size(11)).width(Length::Fixed(70.0)).align_x(Alignment::End),
        ].spacing(4);

        let query = self.search_tracks.to_lowercase();
        let mut list = column![].spacing(2).padding([0.0, 12.0]);

        for t in &self.tracks {
            if !query.is_empty() && !t.name.to_lowercase().contains(&query) { continue; }

            let selected = self.selection.tracks.contains(&t.id);
            let color_box = container(text(""))
                .width(12).height(12)
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(Color::from_rgb8(t.color[0], t.color[1], t.color[2]))),
                    border: iced::Border { color: Color::from_rgb8(60, 60, 60), width: 1.0, radius: 2.0.into() },
                    ..Default::default()
                });

            let dist = if t.cached_distance_m >= 1000.0 {
                format!("{:.1}км", t.cached_distance_m / 1000.0)
            } else {
                format!("{:.0}м", t.cached_distance_m)
            };

            let track_id = t.id;
            let r = row![
                self.visibility_button(t.visible, Message::ToggleTrackVisible(t.id, !t.visible)),
                color_box,
                container(
                    mouse_area(
                        tooltip(
                            self.list_item(&t.name, selected, Message::SelectTrack(t.id)),
                            container(text(t.name.clone())).padding([2, 6]).style(|theme: &Theme| {
                                let palette = theme.extended_palette();
                                container::Style {
                                    background: Some(palette.background.weak.color.into()),
                                    text_color: Some(palette.background.weak.text),
                                    border: iced::Border { color: palette.background.strong.color, width: 1.0, radius: 4.0.into() },
                                    ..Default::default()
                                }
                            }),
                            tooltip::Position::Bottom,
                        )
                    ).on_right_press(Message::ShowContextMenu(
                        self.global_cursor,
                        vec![
                            ContextMenuItem::Label(t.name.clone()),
                            ContextMenuItem::Action("Свойства".to_string(), Message::OpenTrackProps(track_id)),
                            ContextMenuItem::Action("Таблица точек".to_string(), Message::OpenTrackList(track_id)),
                            ContextMenuItem::Action("Вместить в экран".to_string(), Message::FitTrackById(track_id)),
                            ContextMenuItem::Separator,
                            ContextMenuItem::Action("Удалить".to_string(), Message::DeleteTrackById(track_id)),
                        ],
                    ))
                ).width(Length::Fill).clip(true),
                container(text(t.cached_points.to_string()).size(12)).width(Length::Fixed(50.0)).align_x(Alignment::End),
                container(text(dist).size(12)).width(Length::Fixed(70.0)).align_x(Alignment::End),
            ].spacing(4).align_y(Alignment::Center);

            list = list.push(r);
        }

        let has_tracks = !self.selection.tracks.is_empty();
        let has_wps = !self.selection.waypoints.is_empty();
        let one_track = self.selection.tracks.len() == 1;
        let selected_id = self.selection.tracks.iter().next().copied();

        let row_normal = row![
            with_tooltip(
                if one_track {
                    self.icon_button(lucide_iced::bytes::SETTINGS, Message::OpenTrackProps(selected_id.unwrap()))
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::SETTINGS)
                },
                "Свойства трека"
            ),
            with_tooltip(
                if has_tracks {
                    self.icon_button(lucide_iced::bytes::FUNNEL, Message::SimplifyTracks)
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::FUNNEL)
                },
                "Упростить (фильтр Дугласа-Пекера)"
            ),
            with_tooltip(
                if one_track {
                    self.icon_button(lucide_iced::bytes::LIST, Message::OpenTrackList(selected_id.unwrap()))
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::LIST)
                },
                "Таблица точек"
            ),
            with_tooltip(
                if has_tracks {
                    self.icon_button(lucide_iced::bytes::SCAN_EYE, Message::FitTrackToScreen)
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::SCAN_EYE)
                },
                "Вместить треки в экран"
            ),
            with_tooltip(
                self.icon_button(lucide_iced::bytes::SAVE, Message::ToggleTrackSaveMenu),
                "Сохранить треки..."
            ),
            Space::new().width(Length::Fill),
            with_tooltip(
                if has_tracks {
                    self.icon_button(lucide_iced::bytes::TRASH, Message::DeleteSelectedTracks)
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::TRASH)
                },
                "Удалить выделенные треки"
            ),
        ].spacing(4).align_y(Alignment::Center);

        let row_save = row![
            with_tooltip(
                if has_tracks && !has_wps && self.selection.tracks.len() == 1 {
                    self.btn_secondary("PLT").on_press(Message::SaveTracksPlt)
                } else {
                    self.btn_secondary("PLT")
                },
                "Сохранить в PLT"
            ),
            with_tooltip(
                if has_tracks || has_wps {
                    self.btn_secondary("GPX").on_press(Message::SaveTracksGpx)
                } else {
                    self.btn_secondary("GPX")
                },
                "Сохранить в GPX"
            ),
            with_tooltip(
                if has_tracks || has_wps {
                    self.btn_secondary("MS").on_press(Message::SaveTracksMs)
                } else {
                    self.btn_secondary("MS")
                },
                "Сохранить в MS"
            ),
             with_tooltip(
                 if has_tracks || has_wps {
                     self.btn_secondary("KML").on_press(Message::SaveTracksKml)
                 } else {
                     self.btn_secondary("KML")
                 },
                 "Сохранить в KML (Google Earth)"
             ),
            Space::new().width(Length::Fill),
            with_tooltip(
                self.icon_button(lucide_iced::bytes::X, Message::ToggleTrackSaveMenu),
                "Закрыть меню"
            ),
        ].spacing(4).align_y(Alignment::Center);

        let bottom_bar = if self.show_track_save_menu { row_save } else { row_normal };

        column![
            header,
            container(top_bar).padding(2),
            container(cols).padding(2),
            container(scrollable(list).height(Length::FillPortion(1))).padding(2),
            container(bottom_bar).padding(2),
        ]
        .spacing(4)
        .width(Length::Fill)
        .height(Length::FillPortion(1))
        .into()
    }

    fn view_waypoints_section(&self) -> Element<'_, Message> {
        let header = text(format!("Точки ({})", self.waypoints.len())).size(14);

        let all_visible = self.waypoints.iter().all(|w| w.visible);
        let top_bar = row![
            with_tooltip(
                self.icon_button(lucide_iced::bytes::EYE, Message::ToggleAllWaypointsVisible),
                if all_visible { "Скрыть все точки" } else { "Показать все точки" }
            ),
            self.search_input(
                "Поиск точек...",
                &self.search_waypoints,
                Message::SearchWaypointsChanged,
                Message::SearchWaypointsChanged(String::new()),
            ),
        ].spacing(4);

        let cols = row![
            Space::new().width(24.0),
            Space::new().width(14.0),
            container(text("Имя").size(11)).width(Length::Fill),
            container(text("Координаты").size(11)).width(Length::Fixed(120.0)).align_x(Alignment::End),
        ].spacing(4);

        let query = self.search_waypoints.to_lowercase();
        let mut list = column![].spacing(2).padding([0.0, 12.0]);

        for w in &self.waypoints {
            if !query.is_empty() && !w.name.to_lowercase().contains(&query) { continue; }

            let selected = self.selection.waypoints.contains(&w.id);
            let icon = container(text("●").size(11).color(Color::from_rgb8(w.bg_color[0], w.bg_color[1], w.bg_color[2]))).width(14);
            let coord = coord::format_pair(w.lat, w.lon, self.format);

            let wp_id = w.id;
            let r = row![
                self.visibility_button(w.visible, Message::ToggleWaypointVisible(w.id, !w.visible)),
                icon,
                container(
                    mouse_area(
                        tooltip(
                            self.list_item(&w.name, selected, Message::SelectWaypoint(w.id)),
                            container(text(w.name.clone())).padding([2, 6]).style(|theme: &Theme| {
                                let palette = theme.extended_palette();
                                container::Style {
                                    background: Some(palette.background.weak.color.into()),
                                    text_color: Some(palette.background.weak.text),
                                    border: iced::Border { color: palette.background.strong.color, width: 1.0, radius: 4.0.into() },
                                    ..Default::default()
                                }
                            }),
                            tooltip::Position::Bottom,
                        )
                    ).on_right_press(Message::ShowContextMenu(
                        self.global_cursor,
                        vec![
                            ContextMenuItem::Label(w.name.clone()),
                            ContextMenuItem::Action("Копировать координаты".to_string(), Message::CopyCoordinates(w.lat, w.lon)),
                            ContextMenuItem::Action("Свойства".to_string(), Message::OpenWaypointProps(wp_id)),
                            ContextMenuItem::Action("Центрировать".to_string(), Message::CenterOnWaypoint(wp_id)),
                            ContextMenuItem::Action("Проекция точки".to_string(), Message::OpenWaypointProjection(wp_id)),
                            ContextMenuItem::Separator,
                            ContextMenuItem::Action("Удалить".to_string(), Message::DeleteWaypointById(wp_id)),
                        ],
                    ))
                ).width(Length::Fill).clip(true),
                container(text(coord).size(11).font(iced::Font::MONOSPACE)).width(Length::Fixed(120.0)).align_x(Alignment::End),
            ].spacing(4).align_y(Alignment::Center);

            list = list.push(r);
        }

        let has_wps = !self.selection.waypoints.is_empty();
        let has_tracks = !self.selection.tracks.is_empty();
        let one_wp = self.selection.waypoints.len() == 1;
        let two_wps = self.selection.waypoints.len() == 2;
        let selected_id = self.selection.waypoints.iter().next().copied();

        let row_normal = row![
            with_tooltip(
                self.icon_button(lucide_iced::bytes::PLUS, Message::AddNewWaypoint),
                "Добавить точку в центре экрана"
            ),
            with_tooltip(
                if one_wp {
                    self.icon_button(lucide_iced::bytes::SETTINGS, Message::OpenWaypointProps(selected_id.unwrap()))
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::SETTINGS)
                },
                "Свойства точки" 
            ),
            with_tooltip(
                if one_wp {
                    self.icon_button(lucide_iced::bytes::DIAMOND_PLUS, Message::CenterOnWaypoint(selected_id.unwrap()))
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::DIAMOND_PLUS)
                },
                "Центрировать на точке"
            ),
            with_tooltip(
                if one_wp {
                    self.icon_button(lucide_iced::bytes::CIRCLE_ARROW_UP_RIGHT_EXIT, Message::OpenWaypointProjection(selected_id.unwrap()))
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::CIRCLE_ARROW_UP_RIGHT_EXIT)
                },
                "Проекция точки"
            ),
            with_tooltip(
                if two_wps {
                    self.icon_button(lucide_iced::bytes::RULER, Message::MeasureWaypoints)
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::RULER)
                },
                "Измерить расстояние  между двумя точками и азимут с первой на вторую"
            ),
            with_tooltip(
                self.icon_button(lucide_iced::bytes::SAVE, Message::ToggleWpSaveMenu),
                "Сохранить точки..."
            ),
            Space::new().width(Length::Fill),
            with_tooltip(
                if has_wps {
                    self.icon_button(lucide_iced::bytes::TRASH, Message::DeleteSelectedWaypoints)
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::TRASH)
                },
                "Удалить выделенные точки"
            ),
        ].spacing(4).align_y(Alignment::Center);

        let row_save = row![
            with_tooltip(
                if has_wps && !has_tracks {
                    self.btn_secondary("WPT").on_press(Message::SaveWaypointsWptNew)
                } else {
                    self.btn_secondary("WPT")
                },
                "Сохранить точки в новый WPT"
            ),
            with_tooltip(
                if has_wps && !has_tracks {
                    self.btn_secondary("WPT+").on_press(Message::SaveWaypointsWptAppend)
                } else {
                    self.btn_secondary("WPT+")
                },
                "Добавить точки в существующий WPT"
            ),
            with_tooltip(
                if has_wps || has_tracks {
                    self.btn_secondary("GPX").on_press(Message::SaveTracksGpx)
                } else {
                    self.btn_secondary("GPX")
                },
                "Сохранить в GPX"
            ),
            with_tooltip(
                if has_wps || has_tracks {
                    self.btn_secondary("MS").on_press(Message::SaveTracksMs)
                } else {
                    self.btn_secondary("MS")
                },
                "Сохранить в MS"
            ),
             with_tooltip(
                 if has_tracks || has_wps {
                     self.btn_secondary("KML").on_press(Message::SaveTracksKml)
                 } else {
                     self.btn_secondary("KML")
                 },
                 "Сохранить в KML (Google Earth)"
             ),
            Space::new().width(Length::Fill),
            with_tooltip(
                self.icon_button(lucide_iced::bytes::X, Message::ToggleWpSaveMenu),
                "Закрыть меню"
            ),
        ].spacing(4).align_y(Alignment::Center);

        let bottom_bar = if self.show_wp_save_menu { row_save } else { row_normal };

        column![
            header,
            container(top_bar).padding(2),
            container(cols).padding(2),
            container(scrollable(list).height(Length::FillPortion(1))).padding(2),
            container(bottom_bar).padding(2),
        ]
        .spacing(4)
        .width(Length::Fill)
        .height(Length::FillPortion(1))
        .into()
    }
}
