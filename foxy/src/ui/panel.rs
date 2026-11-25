use iced::widget::{
    column, container, mouse_area, row, scrollable, text, text_input, Space, stack,
};
use iced::{Alignment, Color, Element, Length};
use crate::{App, Message, ContextMenuItem};
use crate::ui::controls::with_tooltip;
use crate::t;

/// Show the waypoint's wall-clock time in its recording region, not the
/// timezone of the computer where FoxyXplorer is running.
fn waypoint_panel_date_time(w: &crate::domain::Waypoint) -> (String, String) {
    let Some(utc) = w.time.filter(|t| t.is_finite()) else {
        return ("-".to_owned(), "-".to_owned());
    };

    // Imported WPT offsets must survive Save/Append. For GPX, timezone metadata
    // is set from the geographic location and timestamp during import. Legacy
    // sessions can still resolve an offset using the same offline lookup.
    let offset_minutes = w.utc_offset_minutes
        .or_else(|| w.timezone_id.as_deref()
            .and_then(|zone| crate::util::timezones::offset_for_zone(zone, utc)))
        .or_else(|| crate::util::timezones::at(w.lon, w.lat, utc)
            .map(|(_, offset)| offset))
        .unwrap_or(0); // If location cannot be resolved, show UTC rather than host local time.

    let local = utc + f64::from(offset_minutes) * 60.0;
    let (year, month, day, hour, minute, _, _) = geoio::time::parts(local);
    (
        format!("{year:04}-{month:02}-{day:02}"),
        format!("{hour:02}:{minute:02}"),
    )
}

impl App {
    fn search_input<'a>(
        &'a self,
        placeholder: &'a str,
        value: &'a str,
        on_input: fn(String) -> Message,
        on_clear: Message,
    ) -> Element<'a, Message> {
        let mut pad = crate::ui::styles::input_padding();
        pad.right += 22.0;

        let input = text_input(placeholder, value)
            .padding(pad)
            .style(crate::ui::styles::text_input)
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
            .style(crate::ui::styles::button)
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
            text(format!("{} ({})", t!("msg_0180"), self.layers.len() + 3)).size(14),
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
        
        // OSM (было)
        let osm_idx = self.layers.len();
        list = list.push(
            self.list_item("Openstreetmap (HOT)", self.osm_active() && self.active == osm_idx, Message::Layer(osm_idx))
        );
        
        // Sentinel-2 (новое)
        let s2_idx = self.layers.len() + 1;
        list = list.push(
            self.list_item("Sentinel-2 cloudless", self.active == s2_idx, Message::Layer(s2_idx))
        );
        
        let esri_idx = self.layers.len() + 2;
        list = list.push(
            self.list_item("ESRI World Imagery", self.active == esri_idx, Message::Layer(esri_idx))
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
        let header = text(format!("{} ({})", t!("msg_0204"), self.tracks.len())).size(14);

        let all_visible = self.tracks.iter().all(|t| t.visible);
        let top_bar = row![
            with_tooltip(
                self.icon_button(lucide_iced::bytes::EYE, Message::ToggleAllTracksVisible),
                if all_visible { t!("msg_0080") } else { t!("msg_0081") }
            ),
            self.search_input(
                t!("msg_0082"),
                &self.search_tracks,
                Message::SearchTracksChanged,
                Message::SearchTracksChanged(String::new()),
            ),
        ]
        .spacing(4);

        let cols = row![
            Space::new().width(24.0),
            Space::new().width(16.0),
            container(text(t!("msg_0083")).size(11)).width(Length::Fill),
            container(text(t!("msg_0084")).size(11)).width(Length::Fixed(50.0)).align_x(Alignment::End),
            container(text(t!("msg_0085")).size(11)).width(Length::Fixed(70.0)).align_x(Alignment::End),
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
                        self.list_item(&t.name, selected, Message::SelectTrack(t.id))
                    ).on_right_press(Message::ShowContextMenu(
                        self.global_cursor,
                        vec![
                            ContextMenuItem::Label(t.name.clone()),
                            ContextMenuItem::Action(t!("msg_0074").to_string(), Message::OpenTrackProps(track_id)),
                            ContextMenuItem::Action(t!("msg_0076").to_string(), Message::OpenTrackList(track_id)),
                            ContextMenuItem::Action(t!("msg_0086").to_string(), Message::FitTrackById(track_id)),
                            ContextMenuItem::Separator,
                            ContextMenuItem::Action(t!("msg_0075").to_string(), Message::DeleteTrackById(track_id)),
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
        let multi_seg_selected = self.selection.tracks.len() == 1
            && self.tracks.iter()
                .find(|t| self.selection.tracks.contains(&t.id))
                .map_or(false, |t| t.segments.len() > 1);
        let mergeable = self.selection.tracks.len() >= 2;
        
        let row_normal = row![
            with_tooltip(
                if one_track {
                    self.icon_button(lucide_iced::bytes::SETTINGS, Message::OpenTrackProps(selected_id.unwrap()))
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::SETTINGS)
                },
                t!("msg_0087")
            ),
            with_tooltip(
                if has_tracks {
                    self.icon_button(lucide_iced::bytes::FUNNEL, Message::SimplifyTracks)
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::FUNNEL)
                },
                t!("msg_0088")
            ),
            with_tooltip(
                if one_track {
                    self.icon_button(lucide_iced::bytes::LIST, Message::OpenTrackList(selected_id.unwrap()))
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::LIST)
                },
                t!("msg_0076")
            ),
            with_tooltip(
                if mergeable {
                    self.icon_button(lucide_iced::bytes::GIT_MERGE, Message::MergeTracks)
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::GIT_MERGE)
                },
                t!("msg_0089")
            ),
            with_tooltip(
                if multi_seg_selected {
                    self.icon_button(lucide_iced::bytes::GIT_GRAPH, Message::SplitTrackBySegments)
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::GIT_GRAPH)
                },
                t!("msg_0090")
            ),
            with_tooltip(
                if has_tracks {
                    self.icon_button(lucide_iced::bytes::SCAN_EYE, Message::FitTrackToScreen)
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::SCAN_EYE)
                },
                t!("msg_0091")
            ),
            with_tooltip(
                if has_tracks {
                    self.icon_button(lucide_iced::bytes::SAVE, Message::ToggleTrackSaveMenu)
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::SAVE)
                },
                t!("msg_0092")
            ),
            if self.ftp_enabled {
                with_tooltip(
                    if has_tracks && self.last_kit.is_some() {
                        self.icon_button(lucide_iced::bytes::CLOUD_UPLOAD, Message::FtpTracks)
                    } else { self.icon_button_disabled(lucide_iced::bytes::CLOUD_UPLOAD) },
                    "Отправить выбранные PLT по FTP"
                )
            } else { Space::new().width(Length::Fixed(0.0)).into() },
            Space::new().width(Length::Fill),
            with_tooltip(
                if has_tracks {
                    self.icon_button(lucide_iced::bytes::TRASH, Message::DeleteSelectedTracks)
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::TRASH)
                },
                t!("msg_0093")
            ),
        ].spacing(4).align_y(Alignment::Center);

        let row_save = row![
            with_tooltip(
                if has_tracks && !has_wps && self.selection.tracks.len() == 1 {
                    self.btn_secondary("PLT").on_press(Message::SaveTracksPlt)
                } else {
                    self.btn_secondary("PLT")
                },
                t!("msg_0094")
            ),
            with_tooltip(
                if has_tracks || has_wps {
                    self.btn_secondary("GPX").on_press(Message::SaveTracksGpx)
                } else {
                    self.btn_secondary("GPX")
                },
                t!("msg_0095")
            ),
            with_tooltip(
                if has_tracks || has_wps {
                    self.btn_secondary("MS").on_press(Message::SaveTracksMs)
                } else {
                    self.btn_secondary("MS")
                },
                t!("msg_0096")
            ),
             with_tooltip(
                 if has_tracks || has_wps {
                     self.btn_secondary("KML").on_press(Message::SaveTracksKml)
                 } else {
                     self.btn_secondary("KML")
                 },
                 t!("msg_0097")
             ),
            Space::new().width(Length::Fill),
            with_tooltip(
                self.icon_button(lucide_iced::bytes::X, Message::ToggleTrackSaveMenu),
                t!("msg_0098")
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
        let header = text(format!("{} ({})", t!("msg_0205"),  self.waypoints.len())).size(14);

        let all_visible = self.waypoints.iter().all(|w| w.visible);
        let top_bar = row![
            with_tooltip(
                self.icon_button(lucide_iced::bytes::EYE, Message::ToggleAllWaypointsVisible),
                if all_visible { t!("msg_0099") } else { t!("msg_0100") }
            ),
            self.search_input(
                t!("msg_0101"),
                &self.search_waypoints,
                Message::SearchWaypointsChanged,
                Message::SearchWaypointsChanged(String::new()),
            ),
        ].spacing(4);

        let cols = row![
            Space::new().width(24.0),
            Space::new().width(14.0),
            container(text(t!("msg_0083")).size(11)).width(Length::Fill),
            container(text(t!("msg_0102")).size(11)).width(Length::Fixed(120.0)).align_x(Alignment::End),
            container(text(if crate::i18n::current_lang() == "en" { "Date/Time" } else { "Дат/Вр" }).size(11))
                .width(Length::Fixed(86.0)).align_x(Alignment::End),
        ].spacing(4);

        let query = self.search_waypoints.to_lowercase();
        let mut list = column![].spacing(2).padding([0.0, 12.0]);

        for w in &self.waypoints {
            if !query.is_empty() && !w.name.to_lowercase().contains(&query) { continue; }

            let selected = self.selection.waypoints.contains(&w.id);
            let icon = container(text("●").size(11).color(Color::from_rgb8(w.bg_color[0], w.bg_color[1], w.bg_color[2]))).width(14);
            let coord = coord::format_pair(w.lat, w.lon, self.format);
            let (date, time) = waypoint_panel_date_time(w);

            // Only waypoint descriptions need a tooltip. Empty descriptions
            // leave the row without a tooltip, just like track names.
            let waypoint_label: Element<'_, Message> = {
                let name_button = self.list_item(&w.name, selected, Message::SelectWaypoint(w.id));
                match w.description.as_deref().filter(|desc| !desc.trim().is_empty()) {
                    // Hide description tooltip as soon as any context menu opens.
                    Some(description) if self.context_menu.is_none() => with_tooltip(name_button, description),
                    _ => name_button,
                }
            };

            let wp_id = w.id;
            let r = row![
                self.visibility_button(w.visible, Message::ToggleWaypointVisible(w.id, !w.visible)),
                icon,
                container(
                    mouse_area(
                        waypoint_label
                    ).on_right_press(Message::ShowContextMenu(
                        self.global_cursor,
                        vec![
                            ContextMenuItem::Label(w.name.clone()),
                            ContextMenuItem::Action(t!("msg_0073").to_string(), Message::CopyCoordinates(w.lat, w.lon)),
                            ContextMenuItem::Action(t!("msg_0074").to_string(), Message::OpenWaypointProps(wp_id)),
                            ContextMenuItem::Action(t!("msg_0103").to_string(), Message::CenterOnWaypoint(wp_id)),
                            ContextMenuItem::Action(t!("msg_0062").to_string(), Message::OpenWaypointProjection(wp_id)),
                            ContextMenuItem::Separator,
                            ContextMenuItem::Action(t!("msg_0075").to_string(), Message::DeleteWaypointById(wp_id)),
                        ],
                    ))
                ).width(Length::Fill).clip(true),
                container(text(coord).size(11).font(iced::Font::MONOSPACE)).width(Length::Fixed(120.0)).align_x(Alignment::End),
                container(column![
                    text(date).size(11).font(iced::Font::MONOSPACE),
                    text(time).size(11).font(iced::Font::MONOSPACE),
                ].spacing(0).align_x(Alignment::End))
                    .width(Length::Fixed(86.0)).align_x(Alignment::End),
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
                t!("msg_0104")
            ),
            with_tooltip(
                if one_wp {
                    self.icon_button(lucide_iced::bytes::SETTINGS, Message::OpenWaypointProps(selected_id.unwrap()))
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::SETTINGS)
                },
                t!("msg_0105") 
            ),
            with_tooltip(
                if one_wp {
                    self.icon_button(lucide_iced::bytes::DIAMOND_PLUS, Message::CenterOnWaypoint(selected_id.unwrap()))
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::DIAMOND_PLUS)
                },
                t!("msg_0106")
            ),
            with_tooltip(
                if one_wp {
                    self.icon_button(lucide_iced::bytes::CIRCLE_ARROW_UP_RIGHT_EXIT, Message::OpenWaypointProjection(selected_id.unwrap()))
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::CIRCLE_ARROW_UP_RIGHT_EXIT)
                },
                t!("msg_0062")
            ),
            with_tooltip(
                if two_wps {
                    self.icon_button(lucide_iced::bytes::RULER, Message::MeasureWaypoints)
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::RULER)
                },
                t!("msg_0107")
            ),
            with_tooltip(
                if has_wps {
                    self.icon_button(lucide_iced::bytes::SAVE, Message::ToggleWpSaveMenu)
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::SAVE)
                },
                t!("msg_0108")
            ),
            if self.ftp_enabled {
                with_tooltip(
                    if has_wps && self.last_kit.is_some() {
                        self.icon_button(lucide_iced::bytes::CLOUD_UPLOAD, Message::FtpWaypointNew)
                    } else { self.icon_button_disabled(lucide_iced::bytes::CLOUD_UPLOAD) },
                    "FTP: выбрать WPT / WPT+"
                )
            } else { Space::new().width(Length::Fixed(0.0)).into() },
            Space::new().width(Length::Fill),
            with_tooltip(
                if has_wps {
                    self.icon_button(lucide_iced::bytes::TRASH, Message::DeleteSelectedWaypoints)
                } else {
                    self.icon_button_disabled(lucide_iced::bytes::TRASH)
                },
                t!("msg_0049")
            ),
        ].spacing(4).align_y(Alignment::Center);

        let row_save = row![
            with_tooltip(
                if has_wps && !has_tracks {
                    self.btn_secondary("WPT").on_press(Message::SaveWaypointsWptNew)
                } else {
                    self.btn_secondary("WPT")
                },
                t!("msg_0109")
            ),
            with_tooltip(
                if has_wps && !has_tracks {
                    self.btn_secondary("WPT+").on_press(Message::SaveWaypointsWptAppend)
                } else {
                    self.btn_secondary("WPT+")
                },
                t!("msg_0110")
            ),
            with_tooltip(
                if has_wps || has_tracks {
                    self.btn_secondary("GPX").on_press(Message::SaveTracksGpx)
                } else {
                    self.btn_secondary("GPX")
                },
                t!("msg_0095")
            ),
            with_tooltip(
                if has_wps || has_tracks {
                    self.btn_secondary("MS").on_press(Message::SaveTracksMs)
                } else {
                    self.btn_secondary("MS")
                },
                t!("msg_0096")
            ),
             with_tooltip(
                 if has_tracks || has_wps {
                     self.btn_secondary("KML").on_press(Message::SaveTracksKml)
                 } else {
                     self.btn_secondary("KML")
                 },
                 t!("msg_0097")
             ),
            Space::new().width(Length::Fill),
            with_tooltip(
                self.icon_button(lucide_iced::bytes::X, Message::ToggleWpSaveMenu),
                t!("msg_0098")
            ),
        ].spacing(4).align_y(Alignment::Center);

        let row_ftp = row![
            with_tooltip(self.btn_secondary("WPT").on_press(Message::FtpNewDialog), "Создать WPT, затем отправить"),
            with_tooltip(self.btn_secondary("WPT+").on_press(Message::FtpAppendDialog), "Добавить в WPT, затем отправить"),
            Space::new().width(Length::Fill),
            self.icon_button(lucide_iced::bytes::X, Message::FtpWaypointNew),
        ].spacing(4).align_y(Alignment::Center);
        let bottom_bar = if self.ftp_enabled && self.show_wp_ftp_menu { row_ftp } else if self.show_wp_save_menu { row_save } else { row_normal };

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
