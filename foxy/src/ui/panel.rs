use iced::widget::{
    column, container, row, scrollable, text, text_input, tooltip, Space,
};
use iced::{Alignment, Color, Element, Length, Theme};
use crate::{App, Message};


impl App {
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

    pub(crate) fn view_tracks_section(&self) -> Element<'_, Message> {
        let header = text(format!("Треки ({})", self.tracks.len())).size(14);
        
        let top_bar = row![
            self.icon_button(lucide_iced::bytes::EYE, Message::ToggleAllTracksVisible),
            text_input("Поиск треков...", &self.search_tracks)
            .padding(native_theme_iced::input_padding(&self.resolved_theme))
            .style(native_theme_iced::styles::text_input(&self.resolved_theme))
            .on_input(Message::SearchTracksChanged)
            .size(12),
        ]
        .spacing(4);
        
        let cols = row![
            Space::new().width(24.0),
            Space::new().width(16.0),
            container(text("Имя").size(11)).width(Length::Fill),
            container(text("Точек").size(11)).width(Length::Fixed(50.0)).align_x(Alignment::End),
            container(text("Расст.").size(11)).width(Length::Fixed(70.0)).align_x(Alignment::End),
        ]
        .spacing(4);
        
        let query = self.search_tracks.to_lowercase();
        let mut list = column![].spacing(2).padding([0.0, 12.0]);
        
        for t in &self.tracks {
            if !query.is_empty() && !t.name.to_lowercase().contains(&query) {
                continue;
            }
            
            let selected = self.selection.tracks.contains(&t.id);
            
            let color_box = container(text(""))
            .width(12)
            .height(12)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(Color::from_rgb8(
                    t.color[0], t.color[1], t.color[2],
                ))),
                border: iced::Border {
                    color: Color::from_rgb8(60, 60, 60),
                width: 1.0,
                radius: 2.0.into(),
                },
                ..Default::default()
            });
            
            let dist = if t.cached_distance_m >= 1000.0 {
                format!("{:.1}км", t.cached_distance_m / 1000.0)
            } else {
                format!("{:.0}м", t.cached_distance_m)
            };
            
            let r = row![
                self.visibility_button(t.visible, Message::ToggleTrackVisible(t.id, !t.visible)),
                color_box,
                container(
                    tooltip(
                        self.list_item(&t.name, selected, Message::SelectTrack(t.id)),
                            container(text(t.name.clone()))
                            .padding([2, 6])
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
                            tooltip::Position::Bottom,
                    )
                ).width(Length::Fill).clip(true),
                container(text(t.cached_points.to_string()).size(12)).width(Length::Fixed(50.0)).align_x(Alignment::End),
                container(text(dist).size(12)).width(Length::Fixed(70.0)).align_x(Alignment::End),
            ]
            .spacing(4)
            .align_y(Alignment::Center);
            
            list = list.push(r);
        }
        
        let has_tracks = !self.selection.tracks.is_empty();
        let has_wps = !self.selection.waypoints.is_empty();
        let one_track = self.selection.tracks.len() == 1;
        let selected_id = self.selection.tracks.iter().next().copied();
        
        // Создаем список пресетов для допуска
//         let tolerances = vec![1.0, 2.0, 3.0, 4.0, 5.0, 7.5, 10.0];
//         let tol_options: Vec<String> = tolerances.iter().map(|t| format!("{:.1} м", t)).collect();
//         let current_tol = format!("{:.1} м", self.simplify_tolerance);
//         
//         let tol_picker = pick_list(tol_options, Some(current_tol), move |s: String| {
//             let val = s.trim_end_matches(" м").parse::<f64>().unwrap_or(2.0);
//             Message::ChangeSimplifyTolerance(val)
//         })
        // .padding(native_theme_iced::button_padding(&self.resolved_theme))
        // .style(native_theme_iced::styles::pick_list(&self.resolved_theme));
        
        let row_normal = row![
            if one_track { 
                self.icon_button(lucide_iced::bytes::SETTINGS, Message::OpenTrackProps(selected_id.unwrap())) 
            } else { 
                self.icon_button_disabled(lucide_iced::bytes::SETTINGS) 
            },
            if has_tracks { 
                self.icon_button(lucide_iced::bytes::TRASH, Message::DeleteSelectedTracks) 
            } else { 
                self.icon_button_disabled(lucide_iced::bytes::TRASH) 
            },
            if has_tracks {
                self.icon_button(lucide_iced::bytes::FUNNEL, Message::SimplifyTracks)
            } else {
                self.icon_button_disabled(lucide_iced::bytes::FUNNEL)
            },
            //tol_picker, 
            self.icon_button(lucide_iced::bytes::SAVE, Message::ToggleTrackSaveMenu)
        ].spacing(4).align_y(Alignment::Center);
        
        let row_save = row![
            if has_tracks && !has_wps && self.selection.tracks.len() == 1 { 
                self.btn_secondary("PLT").on_press(Message::SaveTracksPlt) 
            } else { 
                self.btn_secondary("PLT") 
            },
            if has_tracks || has_wps { 
                self.btn_secondary("GPX").on_press(Message::SaveTracksGpx) 
            } else { 
                self.btn_secondary("GPX") 
            },
            if has_tracks || has_wps { 
                self.btn_secondary("MS").on_press(Message::SaveTracksMs) 
            } else { 
                self.btn_secondary("MS") 
            },
            self.icon_button(lucide_iced::bytes::X, Message::ToggleTrackSaveMenu)
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

    pub(crate) fn view_waypoints_section(&self) -> Element<'_, Message> {
        let header = text(format!("Точки ({})", self.waypoints.len())).size(14);
        
        let top_bar = row![
            self.icon_button(lucide_iced::bytes::EYE, Message::ToggleAllWaypointsVisible),
            text_input("Поиск точек...", &self.search_waypoints)
            .padding(native_theme_iced::input_padding(&self.resolved_theme))
            .style(native_theme_iced::styles::text_input(&self.resolved_theme))
            .on_input(Message::SearchWaypointsChanged)
            .size(12),
        ]
        .spacing(4);
        
        let cols = row![
            Space::new().width(24.0),
            Space::new().width(14.0),
            container(text("Имя").size(11)).width(Length::Fill),
            container(text("Координаты").size(11)).width(Length::Fixed(120.0)).align_x(Alignment::End),
        ]
        .spacing(4);
        
        let query = self.search_waypoints.to_lowercase();
        let mut list = column![].spacing(2).padding([0.0, 12.0]);
        
        for w in &self.waypoints {
            if !query.is_empty() && !w.name.to_lowercase().contains(&query) {
                continue;
            }
            
            let selected = self.selection.waypoints.contains(&w.id);
            let icon = container(text("●").size(11).color(Color::from_rgb8(w.bg_color[0], w.bg_color[1], w.bg_color[2]))).width(14);
            let coord = coord::format_pair(w.lat, w.lon, self.format);
            
            let r = row![
                self.visibility_button(w.visible, Message::ToggleWaypointVisible(w.id, !w.visible)),
                icon,
                container(
                    tooltip(
                        self.list_item(&w.name, selected, Message::SelectWaypoint(w.id)),
                            container(text(w.name.clone()))
                            .padding([2, 6])
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
                            tooltip::Position::Bottom,
                    )
                ).width(Length::Fill).clip(true),
                container(text(coord).size(11).font(iced::Font::MONOSPACE)).width(Length::Fixed(120.0)).align_x(Alignment::End),
            ]
            .spacing(4)
            .align_y(Alignment::Center);
            
            list = list.push(r);
        }
        
        let has_wps = !self.selection.waypoints.is_empty();
        let has_tracks = !self.selection.tracks.is_empty();
        let one_wp = self.selection.waypoints.len() == 1;
        let selected_id = self.selection.waypoints.iter().next().copied();
        
        let row_normal = row![
            self.icon_button(lucide_iced::bytes::PLUS, Message::AddNewWaypoint),
            if one_wp { 
                self.icon_button(lucide_iced::bytes::SETTINGS, Message::OpenWaypointProps(selected_id.unwrap())) 
            } else { 
                self.icon_button_disabled(lucide_iced::bytes::SETTINGS) 
            },
            if has_wps { 
                self.icon_button(lucide_iced::bytes::TRASH, Message::DeleteSelectedWaypoints) 
            } else { 
                self.icon_button_disabled(lucide_iced::bytes::TRASH) 
            },
            self.icon_button(lucide_iced::bytes::SAVE, Message::ToggleWpSaveMenu)
        ].spacing(4).align_y(Alignment::Center);
        
        let row_save = row![
            if has_wps && !has_tracks { 
                self.btn_secondary("WPT").on_press(Message::SaveWaypointsWptNew) 
            } else { 
                self.btn_secondary("WPT") 
            },
            if has_wps && !has_tracks { 
                self.btn_secondary("WPT+").on_press(Message::SaveWaypointsWptAppend) 
            } else { 
                self.btn_secondary("WPT+") 
            },
            if has_wps || has_tracks { 
                self.btn_secondary("GPX").on_press(Message::SaveTracksGpx) 
            } else { 
                self.btn_secondary("GPX") 
            },
            if has_wps || has_tracks { 
                self.btn_secondary("MS").on_press(Message::SaveTracksMs) 
            } else { 
                self.btn_secondary("MS") 
            },
            self.icon_button(lucide_iced::bytes::X, Message::ToggleWpSaveMenu)
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
