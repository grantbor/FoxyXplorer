use iced::widget::{
    button, column, container, mouse_area, pick_list, row, scrollable, text, text_input, tooltip, Space,
};
use iced::{Alignment, Color, Element, Length, Theme};
use crate::domain;
use crate::widgets::palette::get_extended_palette;
use crate::{App, Message, ContextMenuState, ContextMenuItem, ColorPickerKind};
use crate::t;
use crate::SettingsDraft;

// =========================================================
// Хелпер для форматирования даты/времени в таблице точек
// =========================================================
fn format_date_time(unix: Option<f64>) -> (String, String) {
    if let Some(t) = unix {
        let (y, m, d, h, min, sec, _) = geoio::time::parts_local(t);
        
        // Добавлено ключевое слово `let` и явная типизация
        let months: [&str; 12] = [
            "янв", "фев", "мар", "апр", "май", "июн", 
            "июл", "авг", "сен", "окт", "ноя", "дек"
        ];
        
        // .copied() превращает Option<&&str> в Option<&str>, 
        // а unwrap_or("???") корректно подставляет срез строки
        let month_str = months.get((m - 1) as usize).copied().unwrap_or("???");
        
        let date = format!("{:02}-{}-{:02}", d, month_str, y % 100);
        let time = format!("{:02}:{:02}:{:02}", h, min, sec);
        (date, time)
    } else {
        ("-".into(), "-".into())
    }
}

impl App {
    // =========================================================
    // Таблица точек (Track List)
    // =========================================================
    pub(crate) fn view_track_list_modal<'a>(
        &'a self,
        state: &'a crate::TrackListState,
    ) -> Element<'a, Message> {
        let track_name = self.tracks.iter().find(|t| t.id == state.track_id).map(|t| t.name.as_str()).unwrap_or("Unknown");
        
        let header = container(
            row![
                text("#").width(Length::FillPortion(40)),
                text(t!("msg_0123")).width(Length::FillPortion(40)),
                text(t!("msg_0200")).width(Length::FillPortion(90)),
                text(t!("msg_0201")).width(Length::FillPortion(90)),
                text(t!("msg_0123")).width(Length::FillPortion(50)),
                text(t!("msg_0125")).width(Length::FillPortion(80)),
                text(t!("msg_0126")).width(Length::FillPortion(60)),
                text(t!("msg_0127")).width(Length::FillPortion(60)),
                text(t!("msg_0128")).width(Length::FillPortion(50)),
                // Space::new().width(Length::Fill) УБРАТЬ
            ].spacing(4).padding(4)
        )
        // ...
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            container::Style { 
                background: Some(palette.background.strong.color.into()), 
                ..Default::default() 
            }
        })
        .width(Length::Fill);

        let mut list = column![].spacing(crate::TRACK_LIST_ROW_SPACING).width(Length::Fill);
        
        for r in &state.rows {
            let selected = state.selected_points.contains(&(r.sect, r.pt_idx));
            let (d_str, t_str) = format_date_time(r.time);
            
            let row_style = if selected {
                move |theme: &Theme| {
                    let palette = theme.extended_palette();
                    container::Style { 
                        background: Some(palette.primary.weak.color.into()), 
                        ..Default::default() 
                    }
                }
            } else {
                move |_: &Theme| container::Style::default()
            };

            let r_ui = container(
                mouse_area(
                    row![
                        text(r.pid.to_string()).width(Length::FillPortion(40)),
                        text(r.sect.to_string()).width(Length::FillPortion(40)),
                        text(format!("{:.6}", r.lat)).width(Length::FillPortion(90)),
                        text(format!("{:.6}", r.lon)).width(Length::FillPortion(90)),
                        text(r.ele.map(|e| format!("{:.1}", e)).unwrap_or_default()).width(Length::FillPortion(50)),
                        text(d_str).width(Length::FillPortion(80)),
                        text(t_str).width(Length::FillPortion(60)),
                        text(format!("{:.1}", r.dist_m)).width(Length::FillPortion(60)),
                        text(format!("{:.1}", r.kph)).width(Length::FillPortion(50)),
                        // Space::new().width(Length::Fill) УБРАТЬ
                    ].spacing(4).padding([2, 4])
                ).on_press(Message::SelectTrackPoint(r.sect, r.pt_idx))
            )
            .style(row_style)
            .width(Length::Fill)
            .height(Length::Fixed(crate::TRACK_LIST_ROW_HEIGHT));

            list = list.push(r_ui);
        }

        let content = column![
            // Заголовок с кнопкой закрытия
            row![
                mouse_area(
                    container(text(format!("{} - {}", t!("msg_0207"), track_name)).size(18))
                        .width(Length::Fill)
                        .padding(4)
                        .style(|theme: &Theme| {
                            let palette = theme.extended_palette();
                            container::Style {
                                background: Some(palette.background.weak.color.into()),
                                ..Default::default()
                            }
                        })
                ).on_press(Message::ModalDragStart),
                container(
                    self.icon_button(lucide_iced::bytes::X, Message::CloseTrackList)
                ).padding(4)
            ].align_y(Alignment::Center).width(Length::Fill),
            
            container(header).padding(2).width(Length::Fill),
            container(
                scrollable(list)
                    .id(crate::TRACK_LIST_SCROLL_ID)
                    .height(Length::Fixed(crate::TRACK_LIST_VIEW_HEIGHT))
            ).padding(2).width(Length::Fill),

            row![
                self.btn_secondary(t!("msg_0129")).on_press_maybe(
                    state.last_clicked.map(|(s, p)| Message::CenterOnTrackPoint(s, p))
                ),
                self.btn_secondary(t!("msg_0130")).on_press_maybe(
                    state.last_clicked.map(|(s, p)| Message::SplitTrackAtPoint(s, p))
                ),
                self.btn_secondary(t!("msg_0131")).on_press_maybe(
                    if state.selected_points.len() == 2 {
                        Some(Message::StitchSegments)
                    } else {
                        None
                    }
                ),                
                self.btn_secondary(t!("msg_0132")).on_press(Message::SelectPointsOutsideGrid),
                self.btn_secondary(t!("msg_0075")).on_press(Message::DeleteSelectedTrackPoints),
                Space::new().width(Length::Fill),
                self.btn_secondary(t!("msg_0133")).on_press(Message::CloseTrackList),
            ].spacing(8).padding([10, 0])
        ]
        .spacing(10)
        .padding(20)
        .width(Length::Fill);

        container(content)
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                    background: Some(palette.background.base.color.into()),
                    border: iced::Border { 
                        color: palette.background.strong.color, 
                        width: 1.0, 
                        radius: crate::ui::styles::border_radius().into() 
                    },
                    ..Default::default()
                }
            })
            .width(Length::Fixed(750.0))
            .into()
    }

    // =========================================================
    // Свойства трека
    // =========================================================
    pub(crate) fn view_track_props_modal<'a>(&'a self, id: u64, draft: &'a domain::TrackProps) -> Element<'a, Message> {
        let callsigns = vec![
            "Lisa".to_string(), "Veter".to_string(), "Kinolog".to_string(), 
            "BPLA".to_string(), "Bort".to_string(), "Zadacha".to_string(), 
            "Avtonom".to_string(), "Pegas".to_string()
        ];
        
        let line_styles = vec![
            domain::LineStyle::Solid, domain::LineStyle::Dash, domain::LineStyle::Dot, 
            domain::LineStyle::DashDot, domain::LineStyle::DashDotDot
        ];
        let poly_types = vec![domain::PolyType::Line, domain::PolyType::Polygon];
        let fill_types = vec![
            domain::FillType::None,
            domain::FillType::Solid,
            domain::FillType::Diagonal1,
            domain::FillType::Diagonal2,
            domain::FillType::CrossHatch,
            domain::FillType::DiagCross,
            domain::FillType::Horizontal,
            domain::FillType::Vertical,
        ];

        let template = draft.description.as_str();
        
        // Извлекаем исходный путь из Track, а не из TrackProps.
        // draft — это копия props, в ней нет source. Track ищем по id.
        let file_source: Option<&'a std::path::Path> = self.tracks.iter()
            .find(|t| t.id == id)
            .and_then(|t| match &t.source {
                domain::FileSource::File { path, .. } => Some(path.as_path()),
                _ => None,
            });

        let file_name: &'a str = file_source
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or(t!("msg_0134"));

        let file_path: Option<&'a std::path::Path> = file_source;

        let extra_fields: Element<'a, Message> = if draft.callsign == "Zadacha" {
            row![].into()
        } else {
            row![
                container(text("#_#")).padding([0, 4]),
                text_input("", &draft.num2.map(|n| n.to_string()).unwrap_or_default())
                    .padding(crate::ui::styles::input_padding())
                    .style(crate::ui::styles::text_input)
                    .on_input(Message::DraftNum2Changed).width(50),
                container(text(t!("msg_0135"))).padding([0, 4]),
                text_input("", &draft.suffix)
                    .padding(crate::ui::styles::input_padding())
                    .style(crate::ui::styles::text_input)
                    .on_input(Message::DraftSuffixChanged).width(80),
            ].align_y(Alignment::Center).spacing(4).into()
        };

        let make_color_btn = |c: [u8; 3], selected: bool, msg: Message| -> Element<'a, Message> {
            button(text("")).width(24).height(24)
                .style(move |_: &Theme, _| button::Style {
                    background: Some(iced::Background::Color(Color::from_rgb8(c[0], c[1], c[2]))),
                    border: iced::Border { 
                        color: if selected { Color::WHITE } else { Color::from_rgb(0.5, 0.5, 0.5) },
                        width: if selected { 2.0 } else { 1.0 },
                        radius: crate::ui::styles::border_radius().into()
                    },
                    ..Default::default()
                })
                .on_press(msg)
                .into()
        };

        let mut color_btns: Vec<Element<'a, Message>> = Vec::new();
        for &c in &self.palette {
            color_btns.push(make_color_btn(c, c == draft.line_color, Message::DraftColorChanged(c)));
        }
        
        color_btns.push(self.chevron_toggle(ColorPickerKind::LineColor));
        
        let color_row = row(color_btns).spacing(2);

        let mut fill_color_btns: Vec<Element<'a, Message>> = Vec::new();
        for &c in &self.palette {
            fill_color_btns.push(make_color_btn(c, c == draft.fill_color, Message::DraftFillColorChanged(c)));
        }
        fill_color_btns.push(self.chevron_toggle(ColorPickerKind::FillColor));
        
        let fill_color_row = row(fill_color_btns).spacing(2);

        let line_picker_ui: Element<'a, Message> = if self.show_color_picker == Some(ColorPickerKind::LineColor) {
            let ext_palette = get_extended_palette();
            let mut grid_rows: Vec<Element<'a, Message>> = Vec::new();
            for chunk in ext_palette.chunks(8) {
                let mut row_btns: Vec<Element<'a, Message>> = Vec::new();
                for &c in chunk {
                    row_btns.push(make_color_btn(c, c == draft.line_color, Message::DraftColorChanged(c)));
                }
                grid_rows.push(row(row_btns).spacing(2).into());
            }
            container(column(grid_rows).spacing(2)).padding([0, 120]).into()
        } else {
            column![].into()
        };

        let fill_picker_ui: Element<'a, Message> = if self.show_color_picker == Some(ColorPickerKind::FillColor) {
            let ext_palette = get_extended_palette();
            let mut grid_rows: Vec<Element<'a, Message>> = Vec::new();
            for chunk in ext_palette.chunks(8) {
                let mut row_btns: Vec<Element<'a, Message>> = Vec::new();
                for &c in chunk {
                    row_btns.push(make_color_btn(c, c == draft.fill_color, Message::DraftFillColorChanged(c)));
                }
                grid_rows.push(row(row_btns).spacing(2).into());
            }
            container(column(grid_rows).spacing(2)).padding([0, 120]).into()
        } else {
            column![].into()
        };

        let fill_fields: Element<'a, Message> = if draft.poly_type == domain::PolyType::Polygon {
            column![
                row![text(t!("msg_0136")).width(120), fill_color_row].align_y(Alignment::Center),
                fill_picker_ui,   // ← добавить
                row![
                    text(t!("msg_0137")).width(120),
                    pick_list(fill_types, Some(draft.fill_type.clone()), Message::DraftFillTypeChanged)
                        .padding(crate::ui::styles::button_padding())
                        .style(crate::ui::styles::pick_list),
                ].align_y(Alignment::Center),
            ].spacing(10).into()
        } else {
            column![].into()
        };

        let input_style = crate::ui::styles::text_input;
        let input_pad = crate::ui::styles::input_padding();
        let pick_pad = crate::ui::styles::button_padding();
        let pick_style = crate::ui::styles::pick_list;

        let content = column![
            // Заголовок с кнопкой закрытия
            row![
                mouse_area(
                    container(text(t!("msg_0087")).size(20))
                        .width(Length::Fill)
                        .padding(4)
                        .style(|theme: &Theme| {
                            let palette = theme.extended_palette();
                            container::Style {
                                background: Some(palette.background.weak.color.into()),
                                ..Default::default()
                            }
                        })
                ).on_press(Message::ModalDragStart),
                container(
                    self.icon_button(lucide_iced::bytes::X, Message::CloseTrackProps)
                ).padding(4)
            ].align_y(Alignment::Center).width(Length::Fill),
            
            row![
                text(t!("msg_0138")).width(120),
                text_input(t!("msg_0139"), &draft.description)
                    .on_input(Message::DraftDescChanged).width(280)
                    .padding(input_pad).style(input_style.clone())
            ].align_y(Alignment::Center),
            
            // Исходный файл (read-only + tooltip с путём)
            {
            let file_field: Element<'a, Message> = if let Some(path) = file_path {
                container(
                    tooltip(
                        text_input("", file_name)
                            .padding(input_pad)
                            .width(Length::Fill)
                            .style(|theme: &Theme, _status| {
                                let palette = theme.extended_palette();
                                iced::widget::text_input::Style {
                                    background: palette.background.weak.color.into(),
                                    border: iced::Border {
                                        color: palette.background.strong.color,
                                        width: 1.0,
                                        radius: 4.0.into(),
                                    },
                                    icon: palette.background.weak.text,
                                    placeholder: palette.background.weak.text,
                                    value: palette.background.weak.text,
                                    selection: palette.primary.weak.color,
                                }
                            }),
                        container(text(path.display().to_string()))
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
                )
                .width(Length::Fill)
                .into()
            } else {
                container(text(file_name).size(14))
                    .padding([6, 8])
                    .width(Length::Fill)
                    .into()
            };

                row![
                    text(t!("msg_0140")).width(120),
                    file_field,
                ].align_y(Alignment::Center)
            },

            row![
                text(t!("msg_0141")).width(120),
                self.btn_secondary(t!("msg_0142")).on_press(Message::DraftDateYesterday),
                self.btn_secondary(t!("msg_0143")).on_press(Message::DraftDateToday),
                text_input("", &draft.date_str)
                    .on_input(Message::DraftDateChanged).width(120)
                    .padding(input_pad).style(input_style.clone()),
            ].align_y(Alignment::Center).spacing(4),

            row![
                text(t!("msg_0144")).width(120),
                pick_list(callsigns, Some(draft.callsign.clone()), Message::DraftCallsignChanged)
                    .padding(pick_pad).style(pick_style.clone()),
                container(text("#")).padding([0, 4]),
                text_input("", &draft.num1.to_string())
                    .on_input(Message::DraftNum1Changed).width(50)
                    .padding(input_pad).style(input_style.clone()),
                extra_fields
            ].align_y(Alignment::Center).spacing(4),

            row![text(t!("msg_0145")).width(120), color_row].align_y(Alignment::Center),
            line_picker_ui,

            row![
                text(t!("msg_0146")).width(120),
                text_input("", &draft.line_width.to_string())
                    .on_input(Message::DraftWidthChanged).width(50)
                    .padding(input_pad).style(input_style.clone()),
                container(text(t!("msg_0147"))).padding([0, 10]),
                pick_list(line_styles, Some(draft.line_style.clone()), Message::DraftStyleChanged)
                    .padding(pick_pad).style(pick_style.clone()),
            ].align_y(Alignment::Center),

            row![
                text(t!("msg_0148")).width(120),
                pick_list(poly_types, Some(draft.poly_type.clone()), Message::DraftTypeChanged)
                    .padding(pick_pad).style(pick_style.clone()),
            ].align_y(Alignment::Center),

            fill_fields,

            row![
                text(t!("msg_0149")).width(120),
                text(template).color(Color::from_rgb8(draft.line_color[0], draft.line_color[1], draft.line_color[2])).size(16)
            ].align_y(Alignment::Center),

            row![
                self.btn_secondary(t!("msg_0150")).on_press(Message::SaveTrackProps),
                self.btn_secondary(t!("msg_0151")).on_press(Message::ApplyTrackProps),
                self.btn_secondary(t!("msg_0152")).on_press(Message::CloseTrackProps),
            ].spacing(10)
        ]
        .spacing(12)
        .padding(20);

        container(content)
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                    background: Some(palette.background.base.color.into()),
                    border: iced::Border { 
                        color: palette.background.strong.color, 
                        width: 1.0, 
                        radius: crate::ui::styles::border_radius().into() 
                    },
                    ..Default::default()
                }
            })
            .width(Length::Fixed(600.0))
            .into()
    }

    // =========================================================
    // Свойства точки
    // =========================================================
    pub(crate) fn view_waypoint_props_modal<'a>(
        &'a self, 
        _id: u64, 
        draft: &'a domain::Waypoint,
        coord_input: &'a str,
    ) -> Element<'a, Message> {
        let text_colors = vec![[0, 0, 0], [255, 255, 255]];
        let bg_colors = vec![[255, 0, 0], [0, 255, 255], [0, 128, 0], [255, 255, 0]];

        let make_color_btn = |c: [u8; 3], selected: bool, msg: Message| -> Element<'a, Message> {
            button(text("")).width(24).height(24)
                .style(move |_: &Theme, _| button::Style {
                    background: Some(iced::Background::Color(Color::from_rgb8(c[0], c[1], c[2]))),
                    border: iced::Border { 
                        color: if selected { Color::WHITE } else { Color::from_rgb(0.5, 0.5, 0.5) },
                        width: if selected { 2.0 } else { 1.0 },
                        radius: crate::ui::styles::border_radius().into()
                    },
                    ..Default::default()
                })
                .on_press(msg)
                .into()
        };

        let mut text_color_btns: Vec<Element<'a, Message>> = text_colors
            .iter()
            .map(|&c| make_color_btn(c, c == draft.text_color, Message::DraftWpTextColorChanged(c)))
            .collect();
            
        text_color_btns.push(self.chevron_toggle(ColorPickerKind::WpTextColor));
        
        let text_color_row = row(text_color_btns).spacing(2);

        let mut bg_color_btns: Vec<Element<'a, Message>> = bg_colors
            .iter()
            .map(|&c| make_color_btn(c, c == draft.bg_color, Message::DraftWpBgColorChanged(c)))
            .collect();
            
        bg_color_btns.push(self.chevron_toggle(ColorPickerKind::WpBgColor));
        
        let bg_color_row = row(bg_color_btns).spacing(2);
        
        let text_picker_ui: Element<'a, Message> = if self.show_color_picker == Some(ColorPickerKind::WpTextColor) {
            let ext_palette = get_extended_palette();
            let mut grid_rows: Vec<Element<'a, Message>> = Vec::new();
            for chunk in ext_palette.chunks(8) {
                let mut row_btns: Vec<Element<'a, Message>> = Vec::new();
                for &c in chunk {
                    row_btns.push(make_color_btn(c, c == draft.text_color, Message::DraftWpTextColorChanged(c)));
                }
                grid_rows.push(row(row_btns).spacing(2).into());
            }
            container(column(grid_rows).spacing(2)).padding([0, 100]).into()
        } else {
            column![].into()
        };

        let bg_picker_ui: Element<'a, Message> = if self.show_color_picker == Some(ColorPickerKind::WpBgColor) {
            let ext_palette = get_extended_palette();
            let mut grid_rows: Vec<Element<'a, Message>> = Vec::new();
            for chunk in ext_palette.chunks(8) {
                let mut row_btns: Vec<Element<'a, Message>> = Vec::new();
                for &c in chunk {
                    row_btns.push(make_color_btn(c, c == draft.bg_color, Message::DraftWpBgColorChanged(c)));
                }
                grid_rows.push(row(row_btns).spacing(2).into());
            }
            container(column(grid_rows).spacing(2)).padding([0, 100]).into()
        } else {
            column![].into()
        };

        let input_style = crate::ui::styles::text_input;
        let input_pad = crate::ui::styles::input_padding();

        let content = column![
            // Заголовок с кнопкой закрытия
            row![
                mouse_area(
                    container(text(t!("msg_0105")).size(20))
                        .width(Length::Fill)
                        .padding(4)
                        .style(|theme: &Theme| {
                            let palette = theme.extended_palette();
                            container::Style {
                                background: Some(palette.background.weak.color.into()),
                                ..Default::default()
                            }
                        })
                ).on_press(Message::ModalDragStart),
                container(
                    self.icon_button(lucide_iced::bytes::X, Message::CloseWaypointProps)
                ).padding(4)
            ].align_y(Alignment::Center).width(Length::Fill),
            
            row![
                text(t!("msg_0138")).width(100), 
                text_input(t!("msg_0083"), &draft.name)
                    .on_input(Message::DraftWpNameChanged).width(250)
                    .padding(input_pad).style(input_style.clone())
            ],
            row![
                text(t!("msg_0153")).width(100), 
                text_input(t!("msg_0154"), draft.description.as_deref().unwrap_or(""))
                    .on_input(Message::DraftWpDescChanged).width(250)
                    .padding(input_pad).style(input_style.clone())
            ],
            
            row![
                text(t!("msg_0155")).width(100),
                text_input("55.7558 37.6173", coord_input)
                    .on_input(Message::DraftWpCoordsChanged).width(250)
                    .padding(input_pad).style(input_style.clone()),
            ].align_y(Alignment::Center),
            
            container(
                text(t!("msg_0156"))
                    .size(11)
                    .color(Color::from_rgb(0.5, 0.5, 0.5))
            ).padding([0, 100]),

            row![
                text(t!("msg_0157")).width(100),
                text_input("", &draft.ele.map(|e| e.to_string()).unwrap_or_default())
                    .on_input(Message::DraftWpEleChanged).width(100)
                    .padding(input_pad).style(input_style.clone()),
            ],
            row![text(t!("msg_0158")).width(100), text_color_row].align_y(Alignment::Center),
            text_picker_ui,
            row![text(t!("msg_0159")).width(100), bg_color_row].align_y(Alignment::Center),
            bg_picker_ui,
            
            row![
                self.btn_secondary(t!("msg_0160")).on_press(Message::None),
                self.btn_secondary(t!("msg_0151")).on_press(Message::ApplyWaypointProps),
                self.btn_secondary(t!("msg_0152")).on_press(Message::CloseWaypointProps),
            ].spacing(10)
        ].spacing(12).padding(20);

        container(content)
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                    background: Some(palette.background.base.color.into()),
                    border: iced::Border { 
                        color: palette.background.strong.color, 
                        width: 1.0, 
                        radius: crate::ui::styles::border_radius().into() 
                    },
                    ..Default::default()
                }
            })
            .width(Length::Fixed(550.0))
            .into()
    }
    
    
    // =========================================================
    // Измерение между точками (линейка)
    // =========================================================
    pub(crate) fn view_waypoint_measure_modal<'a>(
        &'a self,
        from_name: &'a str, from_lat: f64, from_lon: f64,
        to_name: &'a str, to_lat: f64, to_lon: f64,
        dist_m: f64, mag_az: f64,
    ) -> Element<'a, Message> {
        let dist_text = if dist_m >= 1000.0 {
            format!("{:.2} {}", dist_m / 1000.0, t!("msg_0220"))
        } else {
            format!("{:.0} {}", dist_m, t!("msg_0221"))
        };

        let content = column![
            text(t!("msg_0161")).size(20),
            
            row![
                text(t!("msg_0162")).width(40),
                text(format!("{} ({:.6}, {:.6})", from_name, from_lat, from_lon))
            ],
            row![
                text(t!("msg_0163")).width(40),
                text(format!("{} ({:.6}, {:.6})", to_name, to_lat, to_lon))
            ],
            Space::new().height(10),
            row![
                text(t!("msg_0164")).width(120).size(16),
                text(dist_text).size(20).color(Color::from_rgb8(0x2a, 0x7f, 0xff))
            ],
            row![
                text(t!("msg_0165")).width(120).size(16),
                text(format!("{:.0}°", mag_az)).size(20).color(Color::from_rgb8(255, 140, 0))
            ],
            Space::new().height(10),
            
            row![
                Space::new().width(Length::Fill),
                self.btn(t!("msg_0133")).on_press(Message::CloseWaypointMeasure),
            ]
        ].spacing(12).padding(20);

        container(content)
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                    background: Some(palette.background.base.color.into()),
                    border: iced::Border { 
                        color: palette.background.strong.color, 
                        width: 1.0, 
                        radius: crate::ui::styles::border_radius().into() 
                    },
                    ..Default::default()
                }
            })
            .width(Length::Fixed(550.0))
            .into()
    }

    fn chevron_toggle<'a>(&'a self, kind: ColorPickerKind) -> Element<'a, Message> {
        let icon = if self.show_color_picker == Some(kind) {
            lucide_iced::bytes::CHEVRON_UP
        } else {
            lucide_iced::bytes::CHEVRON_DOWN
        };
        button(lucide_iced::themed_icon(icon, 16.0))
            .width(24).height(24).padding(0)
            .on_press(Message::ToggleColorPicker(kind))
            .style(crate::ui::styles::button)
            .into()
    }
    
    // =========================================================
    // Подтверждение Clear All
    // =========================================================
    pub(crate) fn view_confirm_clear_all_modal<'a>(&'a self) -> Element<'a, Message> {
        let content = column![
            text(t!("msg_0166")).size(20),
            text(t!("msg_0167")).size(14),
            Space::new().height(10),
            row![
                self.btn(t!("msg_0168")).on_press(Message::ExecuteClearAll),
                self.btn_secondary(t!("msg_0152")).on_press(Message::CancelClearAll),
            ].spacing(10)
        ]
        .spacing(12)
        .padding(20);

        container(content)
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                    background: Some(palette.background.base.color.into()),
                    border: iced::Border {
                        color: palette.background.strong.color,
                        width: 1.0,
                        radius: crate::ui::styles::border_radius().into(),
                    },
                    ..Default::default()
                }
            })
            .width(Length::Fixed(400.0))
            .into()
    }

    // =========================================================
    // Подтверждение загрузки сессии
    // =========================================================
    pub(crate) fn view_confirm_load_session_modal<'a>(&'a self) -> Element<'a, Message> {
        let content = column![
            text(t!("msg_0169")).size(20),
            text(t!("msg_0170"))
                .size(14),
            Space::new().height(10),
            
            self.btn(t!("msg_0171"))
                .on_press(Message::SaveAndLoadSession)
                .width(Length::Fill),
            self.btn_secondary(t!("msg_0172"))
                .on_press(Message::LoadSessionNoPrompt)
                .width(Length::Fill),
            self.btn_secondary(t!("msg_0152"))
                .on_press(Message::CancelClearAll)
                .width(Length::Fill),
        ]
        .spacing(8)
        .padding(20);

        container(content)
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                    background: Some(palette.background.base.color.into()),
                    border: iced::Border {
                        color: palette.background.strong.color,
                        width: 1.0,
                        radius: crate::ui::styles::border_radius().into(),
                    },
                    ..Default::default()
                }
            })
            .width(Length::Fixed(400.0))
            .into()
    }
    
    pub(crate) fn view_context_menu<'a>(&'a self, state: &'a ContextMenuState) -> Element<'a, Message> {
        let mut menu = column![].spacing(2).padding(4);

        for (idx, item) in state.items.iter().enumerate() {
            match item {
                ContextMenuItem::Action(label, _) => {
                    menu = menu.push(
                        button(text(label.clone()).size(14))
                            .width(Length::Fill)
                            .padding([4, 12])
                            .on_press(Message::ContextMenuSelect(idx))
                            .style(crate::ui::styles::button),
                    );
                }
                ContextMenuItem::Label(label) => {
                    menu = menu.push(
                        container(text(label.clone()).size(13))
                            .width(Length::Fill)
                            .padding([4, 12])
                            .style(|theme: &Theme| {
                                let palette = theme.extended_palette();
                                container::Style {
                                    background: Some(palette.background.weak.color.into()),
                                    text_color: Some(palette.background.weak.text),
                                    ..Default::default()
                                }
                            }),
                    );
                }
                ContextMenuItem::Separator => {
                    menu = menu.push(
                        container(text("")).width(Length::Fill).height(Length::Fixed(1.0))
                            .style(|theme: &Theme| {
                                let palette = theme.extended_palette();
                                container::Style {
                                    background: Some(palette.background.strong.color.into()),
                                    ..Default::default()
                                }
                            }),
                    );
                }
            }
        }

        container(menu)
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                    background: Some(palette.background.base.color.into()),
                    border: iced::Border {
                        color: palette.background.strong.color,
                        width: 1.0,
                        radius: crate::ui::styles::border_radius().into(),
                    },
                    ..Default::default()
                }
            })
            .width(Length::Fixed(220.0))
            .into()
    }
    pub(crate) fn view_waypoint_projection_modal<'a>(
        &'a self,
        _from_id: u64,
        azimuth_input: &'a str,
        distance_input: &'a str,
    ) -> Element<'a, Message> {
        let input_style = crate::ui::styles::text_input;
        let input_pad = crate::ui::styles::input_padding();

        let content = column![
            text(t!("msg_0062")).size(20),
            text(t!("msg_0173")).size(13),
            Space::new().height(8),
            row![
                text(t!("msg_0174")).width(120),
                text_input("0", azimuth_input)
                    .on_input(Message::ProjectAzimuthChanged)
                    .width(120).padding(input_pad).style(input_style.clone()),
            ].align_y(Alignment::Center),
            row![
                text(t!("msg_0175")).width(120),
                text_input("100", distance_input)
                    .on_input(Message::ProjectDistanceChanged)
                    .width(120).padding(input_pad).style(input_style.clone()),
            ].align_y(Alignment::Center),
            Space::new().height(8),
            row![
                self.btn(t!("msg_0176")).on_press(Message::ExecuteProjection),
                self.btn_secondary(t!("msg_0152")).on_press(Message::CloseWaypointProjection),
            ].spacing(10)
        ].spacing(12).padding(20);

        container(content)
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                    background: Some(palette.background.base.color.into()),
                    border: iced::Border {
                        color: palette.background.strong.color,
                        width: 1.0,
                        radius: crate::ui::styles::border_radius().into(),
                    },
                    ..Default::default()
                }
            })
            .width(Length::Fixed(450.0))
            .into()
    }
    pub(crate) fn view_about_modal<'a>(&'a self) -> Element<'a, Message> {
        let version = env!("CARGO_PKG_VERSION");

        // Стиль для тонких горизонтальных линий
        let separator = || {
            container(text(""))
                .width(Length::Fill)
                .height(Length::Fixed(1.0))
                .style(|theme: &Theme| {
                    let palette = theme.extended_palette();
                    container::Style {
                        background: Some(palette.background.strong.color.into()),
                        ..Default::default()
                    }
                })
        };

        // Стиль для кнопок-ссылок (синий текст без фона)
        let link_style = |theme: &Theme, status: button::Status| {
            let palette = theme.extended_palette();
            let color = palette.primary.strong.color;
            let text_color = match status {
                button::Status::Hovered => palette.primary.base.color,
                _ => color,
            };
            button::Style {
                background: Some(Color::TRANSPARENT.into()),
                text_color,
                border: iced::Border::default(),
                ..Default::default()
            }
        };

        // Собираем правую колонку (Текст)
        let right_column = column![
            text(format!("FoxyXplorer v{}", version)).size(24),
            separator(),
            text(t!("msg_0177"))
                .size(16)
                .align_x(Alignment::Center), // центрируем текст
            separator(),
            row![
                button(text("Github").size(16))
                    .padding(0)
                    .style(link_style)
                    .on_press(Message::OpenUrl("https://github.com/grantbor/FoxyXplorer".to_string())),
                
                text("|").size(16).color(Color::from_rgb(0.5, 0.5, 0.5)),
                
                button(text(t!("msg_0178")).size(16))
                    .padding(0)
                    .style(link_style)
                    .on_press(Message::OpenUrl("https://github.com/grantbor/FoxyXplorer/blob/main/docs/user-guide.md".to_string())),
            ]
            .spacing(20)
            .align_y(Alignment::Center),
        ]
        .spacing(16)
        .width(Length::Fill)
        .align_x(Alignment::Center); // центрируем всё содержимое колонки

        // Верхняя часть: Логотип + Правая колонка
        let top_content = row![
            // Логотип
            container(
                iced::widget::svg(iced::widget::svg::Handle::from_memory(crate::ICON_SVG))
                    .width(Length::Fixed(140.0))
                    .height(Length::Fixed(140.0))
            ).padding([0, 20]), // отступ справа от логотипа

            right_column
        ]
        .align_y(Alignment::Center)
        .spacing(10);

        // Нижняя часть: Лицензия + Кнопка закрытия
        let bottom_content = row![
            text("GPL-3.0").size(16).color(Color::from_rgb(0.6, 0.6, 0.6)),
            Space::new().width(Length::Fill),
            self.btn(t!("msg_0133")).on_press(Message::CloseAbout),
        ].align_y(Alignment::Center);

        // Общая композиция
        let content = column![
            top_content,
            bottom_content
        ]
        .spacing(24)
        .padding(24);

        // Фрейм модалки
        container(content)
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                    background: Some(palette.background.base.color.into()),
                    border: iced::Border {
                        color: palette.background.strong.color,
                        width: 1.0,
                        radius: crate::ui::styles::border_radius().into(),
                    },
                    ..Default::default()
                }
            })
            .width(Length::Fixed(540.0)) // Ширина подобрана под макет
            .into()
    }

    pub(crate) fn view_foxydrop_modal<'a>(&'a self) -> Element<'a, Message> {
        use crate::foxydrop::Phase;

        let en = crate::i18n::current_lang() == "en";

        // Локализация подписей
        let tr = |ru: &'static str, en_text: &'static str| -> &'static str {
            if en { en_text } else { ru }
        };

        let status: Element<'a, Message> = match self.drop_phase {
            Phase::Stopped => text(tr("Сервер остановлен", "Server Down"))
                .size(18)
                .color(Color::from_rgb8(180, 75, 65))
                .into(),

            Phase::Starting => text(tr("Запуск сервера...", "Starting..."))
                .size(18)
                .into(),

            Phase::Stopping => text(tr("Остановка сервера...", "Stopping..."))
                .size(18)
                .into(),

            Phase::Running => text(tr("Сервер запущен", "Server Running"))
                .size(18)
                .color(Color::from_rgb8(45, 175, 96))
                .into(),
        };

        let mut body = column![status]
            .spacing(12)
            .align_x(Alignment::Center);

        // Сообщения об ошибках
        if let Some(error) = &self.drop_error {
            let error_text = if en {
                let translations = [
                    (
                        "Не найден IPv4-адрес локальной сети",
                        "No local IPv4 address found",
                    ),
                    (
                        "Сервер неожиданно остановился",
                        "The server stopped unexpectedly",
                    ),
                    (
                        "FoxyDrop уже запущен",
                        "FoxyDrop is already running",
                    ),
                    (
                        "Не найдена папка загрузки: ",
                        "Upload folder not found: ",
                    ),
                    (
                        "Нет доступа для записи в папку: ",
                        "Upload folder is not writable: ",
                    ),
                    (
                        "Невозможно открыть порт: ",
                        "Could not open port: ",
                    ),
                    (
                        "Невозможно запустить FoxyDrop: ",
                        "Could not start FoxyDrop: ",
                    ),
                ];

                translations
                    .iter()
                    .find_map(|(ru, english)| {
                        error.strip_prefix(ru)
                            .map(|rest| format!("{english}{rest}"))
                    })
                    .unwrap_or_else(|| error.clone())
            } else {
                error.clone()
            };

            body = body.push(
                text(error_text)
                    .size(13)
                    .color(Color::from_rgb8(200, 70, 60))
            );
        }

        // QR-код и URL работающего сервера
        if self.drop_phase == Phase::Running {
            if let Some(addr) = self.drop_address {
                let url = crate::foxydrop::url(addr);

                body = body.push(
                    iced::widget::canvas::Canvas::new(
                        crate::foxydrop::qr::QrProgram {
                            url: url.clone(),
                        }
                    )
                    .width(Length::Fixed(220.0))
                    .height(Length::Fixed(220.0))
                );

                body = body.push(
                    row![
                        text(url).size(15),
                        self.btn_secondary(
                            tr("Копировать", "Copy")
                        )
                        .on_press(Message::FoxyDropCopyUrl),
                    ]
                    .align_y(Alignment::Center)
                    .spacing(12)
                );
            }
        }

        // Выбор IP-адреса
        body = body.push(
            row![
                text("IP:").width(40),
                pick_list(
                    self.drop_ips.clone(),
                    self.drop_selected_ip,
                    Message::FoxyDropIpSelected
                )
                .padding(
                    crate::ui::styles::button_padding()
                )
                .style(
                    crate::ui::styles::pick_list
                ),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
        );

        // Папка загрузки
        let current_dir = self.drop_service
            .directory()
            .display()
            .to_string();

        let folder_text = if current_dir.is_empty() {
            tr(
                "Выберите папку в настройках",
                "Choose a folder in Settings"
            ).to_string()
        } else {
            format!(
                "{}: {}",
                tr("Папка", "Folder"),
                current_dir
            )
        };

        body = body.push(text(folder_text).size(12));

        // Запуск и остановка
        if self.drop_phase == Phase::Stopped {
            body = body.push(
                self.btn(tr("Запустить", "Start"))
                    .on_press(Message::FoxyDropStart)
            );
        } else if self.drop_phase == Phase::Running {
            body = body.push(
                self.btn_secondary(tr("Остановить", "Stop"))
                    .on_press(Message::FoxyDropStop)
            );
        }

        // Основное содержимое окна
        let content = column![
            row![
                mouse_area(
                    container(text("FoxyDrop").size(20))
                        .width(Length::Fill)
                        .padding(4)
                )
                .on_press(Message::ModalDragStart),

                self.icon_button(
                    lucide_iced::bytes::X,
                    Message::CloseFoxyDrop
                ),
            ]
            .align_y(Alignment::Center),

            body.width(Length::Fill),

            row![
                Space::new().width(Length::Fill),
                self.btn_secondary(t!("msg_0133"))
                    .on_press(Message::CloseFoxyDrop),
            ]
        ]
        .spacing(16)
        .padding(20);

        container(content)
            .style(|theme: &Theme| {
                let pal = theme.extended_palette();

                container::Style {
                    background: Some(
                        pal.background.base.color.into()
                    ),
                    border: iced::Border {
                        color: pal.background.strong.color,
                        width: 1.0,
                        radius: crate::ui::styles::border_radius().into(),
                    },
                    ..Default::default()
                }
            })
            .width(Length::Fixed(460.0))
            .into()
    }


    pub(crate) fn view_settings_modal<'a>(&'a self, draft: &'a SettingsDraft) -> Element<'a, Message> {
        let separator = || {
            container(text(""))
                .width(Length::Fill)
                .height(Length::Fixed(1.0))
                .style(|theme: &Theme| {
                    let palette = theme.extended_palette();
                    container::Style {
                        background: Some(palette.background.strong.color.into()),
                        ..Default::default()
                    }
                })
        };

        let input_style = crate::ui::styles::text_input;
        let input_pad = crate::ui::styles::input_padding();
        let pick_pad = crate::ui::styles::button_padding();
        let pick_style = crate::ui::styles::pick_list;

        // Основной ряд цветов и расширенная палитра 8×8, как в Track Properties.
        let make_grid_color_btn = |color: [u8; 3]| -> Element<'a, Message> {
            let selected = color == draft.grid_color;
            button(text("")).width(24).height(24)
                .style(move |_: &Theme, _| button::Style {
                    background: Some(iced::Background::Color(Color::from_rgb8(
                        color[0], color[1], color[2],
                    ))),
                    border: iced::Border {
                        color: if selected { Color::WHITE } else { Color::from_rgb(0.5, 0.5, 0.5) },
                        width: if selected { 2.0 } else { 1.0 },
                        radius: crate::ui::styles::border_radius().into(),
                    },
                    ..Default::default()
                })
                .on_press(Message::SettingsGridColorChanged(color))
                .into()
        };

        let mut grid_color_buttons: Vec<Element<'a, Message>> = self.palette.iter()
            .copied()
            .map(make_grid_color_btn)
            .collect();
        grid_color_buttons.push(self.chevron_toggle(ColorPickerKind::GridColor));
        let grid_color_row = row(grid_color_buttons).spacing(2);

        let grid_color_picker: Element<'a, Message> = if self.show_color_picker == Some(ColorPickerKind::GridColor) {
            let mut palette_rows: Vec<Element<'a, Message>> = Vec::new();
            for chunk in get_extended_palette().chunks(8) {
                let buttons: Vec<Element<'a, Message>> = chunk.iter()
                    .copied()
                    .map(make_grid_color_btn)
                    .collect();
                palette_rows.push(row(buttons).spacing(2).into());
            }
            container(column(palette_rows).spacing(2))
                .padding(iced::Padding::new(0.0).left(166.0))
                .into()
        } else {
            column![].into()
        };

        let current_lang_label = match draft.lang.as_str() {
            "en" => "English",
            _ => "Русский",
        };

        // Подсветка: зелёная рамка если координаты валидны, обычная если нет
        let is_valid = draft.parsed_lat.is_some() && draft.parsed_lon.is_some();
        
        let coord_style = move |theme: &Theme, status: iced::widget::text_input::Status| {
            // Берём стандартный стиль iced
            let mut style = input_style(theme, status);
            
            // Если координаты распарсились корректно, меняем цвет рамки на зелёный
            if is_valid {
                style.border = iced::Border {
                    color: Color::from_rgb(0.2, 0.7, 0.3), // зелёный цвет
                    width: 1.0,
                    radius: 4.0.into(),
                };
            }
            style
        };

        let content = column![
            // Заголовок
            row![
                mouse_area(
                    container(text(t!("msg_0202")).size(20))
                        .width(Length::Fill)
                        .padding(4)
                        .style(|theme: &Theme| {
                            let palette = theme.extended_palette();
                            container::Style {
                                background: Some(palette.background.weak.color.into()),
                                ..Default::default()
                            }
                        })
                ).on_press(Message::ModalDragStart),
                container(
                    self.icon_button(lucide_iced::bytes::X, Message::CloseSettings)
                ).padding(4)
            ].align_y(Alignment::Center).width(Length::Fill),

            separator(),

            // Прокручивается только содержимое настроек; кнопки остаются видимыми.
            scrollable(column![
            // Стартовая локация — единое поле ввода
            row![
                text(t!("msg_0236")).width(160),
                text_input("55.016359 82.945971", &draft.coord_input)
                    .on_input(Message::SettingsCoordChanged)
                    .width(240)
                    .padding(input_pad)
                    .style(coord_style),
            ].align_y(Alignment::Center).spacing(6),

            // Подсказка по форматам
            container(
                text(t!("msg_0235"))
                    .size(11)
                    .color(Color::from_rgb(0.5, 0.5, 0.5))
            ).padding([0, 160]),

            // Язык
            row![
                text(t!("msg_0237")).width(160),
                pick_list(
                    ["Русский", "English"],
                    Some(current_lang_label),
                    |label: &str| Message::SettingsLangChanged(label.to_string())
                ).padding(pick_pad).style(pick_style.clone()),
            ].align_y(Alignment::Center),

            // Допуск упрощения
            // Глобальная тема: встроенные варианты iced::Theme.
        row![
            text(if crate::i18n::current_lang() == "en" { "Theme" } else { "Тема" }).width(160),
            pick_list(
                crate::THEME_FAMILY_LABELS.to_vec(),
                Some(crate::theme_family_name(&draft.ui_theme)),
                |family| Message::SettingsThemeChanged(crate::theme_from_family(family)),
            )
            .padding(pick_pad)
            .style(pick_style.clone()),
        ].align_y(Alignment::Center).spacing(6),

        row![
                text(t!("msg_0238")).width(160),
                iced::widget::slider(0.1..=10.0, draft.simplify_tolerance, Message::SettingsToleranceChanged)
                    .step(0.1)
                    .width(200),
                text(format!("{:.1} {}", draft.simplify_tolerance, t!("msg_0221"))).width(50),
            ].align_y(Alignment::Center).spacing(6),

            separator(),
            row![
                text(if crate::i18n::current_lang() == "en" { "FoxyDrop folder" } else { "Папка FoxyDrop" }).width(160),
                text_input("", &draft.drop_folder)
                    .width(Length::Fill)
                    .padding(input_pad)
                    // Поле без on_input (только чтение): не используем бледный disabled-стиль.
                    .style(|theme: &Theme, _status| {
                        let pal = theme.extended_palette();
                        iced::widget::text_input::Style {
                            background: pal.background.weak.color.into(),
                            border: iced::Border {
                                color: pal.background.strong.color,
                                width: 1.0,
                                radius: crate::ui::styles::border_radius().into(),
                            },
                            icon: pal.background.base.text,
                            placeholder: pal.background.base.text,
                            value: pal.background.base.text,
                            selection: pal.primary.weak.color,
                        }
                    }),
                self.btn_secondary("...").on_press(Message::SettingsChooseDropFolder),
            ].align_y(Alignment::Center).spacing(6),

            // Настраиваемый цвет линий и узлов сетки.
            row![
                text(if crate::i18n::current_lang() == "en" { "Grid color" } else { "Цвет сетки" }).width(160),
                grid_color_row,
            ].align_y(Alignment::Center).spacing(6),
            grid_color_picker,
            ].spacing(12).width(Length::Fill)).height(Length::Fill),

            // Кнопки всегда видны, даже если палитра раскрыта.
            row![
                Space::new().width(Length::Fill),
                self.btn_secondary(t!("msg_0152")).on_press(Message::CloseSettings),
                self.btn(t!("msg_0151")).on_press(Message::ApplySettings),
            ].spacing(10)
        ]
        .spacing(12)
        .padding(20);

        container(content)
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                    background: Some(palette.background.base.color.into()),
                    border: iced::Border {
                        color: palette.background.strong.color,
                        width: 1.0,
                        radius: crate::ui::styles::border_radius().into(),
                    },
                    ..Default::default()
                }
            })
            .width(Length::Fixed(520.0))
            // Высота ограничена реальным размером окна, основная часть прокручивается.
            .height(Length::Fixed(
                (if self.show_color_picker == Some(ColorPickerKind::GridColor) {
                    690.0_f32
                } else {
                    480.0_f32
                }).min((self.window_size.height - 64.0).max(240.0))
            ))
            .into()
    }
}
