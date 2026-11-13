use iced::widget::{
    button, column, container, mouse_area, pick_list, row, scrollable, text, text_input, Space,
};
use iced::{Alignment, Color, Element, Length, Theme};
use crate::domain;
use crate::util::coords::build_template;
use crate::widgets::palette::get_extended_palette;
use crate::{App, Message, ContextMenuState, ContextMenuItem, ColorPickerKind};

// =========================================================
// Хелпер для форматирования даты/времени в таблице точек
// =========================================================
fn format_date_time(unix: Option<f64>) -> (String, String) {
    if let Some(t) = unix {
        let (y, m, d, h, min, sec, _) = geoio::time::parts_local(t);
        let months = ["янв", "фев", "мар", "апр", "май", "июн", "июл", "авг", "сен", "окт", "ноя", "дек"];
        let month_str = months.get((m - 1) as usize).unwrap_or(&"???");
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
    pub(crate) fn view_track_list_modal<'a>(&'a self, state: &'a crate::TrackListState) -> Element<'a, Message> {
        let track_name = self.tracks.iter().find(|t| t.id == state.track_id).map(|t| t.name.as_str()).unwrap_or("Unknown");
        
        let header = container(
            row![
                text("#").width(Length::FillPortion(40)),
                text("Сег").width(Length::FillPortion(40)),
                text("Latitude").width(Length::FillPortion(90)),
                text("Longitude").width(Length::FillPortion(90)),
                text("Alt(м)").width(Length::FillPortion(50)),
                text("Дата").width(Length::FillPortion(80)),
                text("Время").width(Length::FillPortion(60)),
                text("Расст(м)").width(Length::FillPortion(60)),
                text("Км/ч").width(Length::FillPortion(50)),
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

        let mut list = column![].spacing(1).width(Length::Fill);
        
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
            .width(Length::Fill);

            list = list.push(r_ui);
        }

        let content = column![
            // Заголовок с кнопкой закрытия
            row![
                mouse_area(
                    container(text(format!("Track List - {}", track_name)).size(18))
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
            container(scrollable(list).height(Length::Fixed(400.0))).padding(2).width(Length::Fill),

            row![
                self.btn_secondary("В центр").on_press_maybe(
                    state.last_clicked.map(|(s, p)| Message::CenterOnTrackPoint(s, p))
                ),
                self.btn_secondary("Разделить").on_press_maybe(
                    state.last_clicked.map(|(s, p)| Message::SplitTrackAtPoint(s, p))
                ),
                self.btn_secondary("Вне сетки").on_press(Message::SelectPointsOutsideGrid),
                self.btn_secondary("Удалить").on_press(Message::DeleteSelectedTrackPoints),
                Space::new().width(Length::Fill),
                self.btn_secondary("Закрыть").on_press(Message::CloseTrackList),
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
                        radius: native_theme_iced::border_radius(&self.resolved_theme).into() 
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
    pub(crate) fn view_track_props_modal<'a>(&'a self, _id: u64, draft: &'a domain::TrackProps) -> Element<'a, Message> {
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
            domain::FillType::None, domain::FillType::Diagonal1, domain::FillType::Diagonal2, 
            domain::FillType::CrossHatch, domain::FillType::Solid
        ];

        let template = build_template(draft);

        let extra_fields: Element<'a, Message> = if draft.callsign == "Zadacha" {
            row![].into()
        } else {
            row![
                container(text("#_#")).padding([0, 4]),
                text_input("", &draft.num2.map(|n| n.to_string()).unwrap_or_default())
                    .padding(native_theme_iced::input_padding(&self.resolved_theme))
                    .style(native_theme_iced::styles::text_input(&self.resolved_theme))
                    .on_input(Message::DraftNum2Changed).width(50),
                container(text("Суффикс")).padding([0, 4]),
                text_input("", &draft.suffix)
                    .padding(native_theme_iced::input_padding(&self.resolved_theme))
                    .style(native_theme_iced::styles::text_input(&self.resolved_theme))
                    .on_input(Message::DraftSuffixChanged).width(80),
            ].align_y(Alignment::Center).spacing(4).into()
        };

        let make_color_btn = |c: [u8; 3], msg: Message| -> Element<'a, Message> {
            button(text("")).width(24).height(24)
                .style(move |_: &Theme, _| button::Style {
                    background: Some(iced::Background::Color(Color::from_rgb8(c[0], c[1], c[2]))),
                    border: iced::Border { 
                        color: Color::from_rgb(0.5, 0.5, 0.5), 
                        width: 1.0, 
                        radius: native_theme_iced::border_radius(&self.resolved_theme).into()
                    },
                    ..Default::default()
                })
                .on_press(msg)
                .into()
        };

        let mut color_btns: Vec<Element<'a, Message>> = Vec::new();
        for &c in &self.palette {
            color_btns.push(make_color_btn(c, Message::DraftColorChanged(c)));
        }
        
        color_btns.push(self.chevron_toggle(ColorPickerKind::LineColor));
        
        let color_row = row(color_btns).spacing(2);

        let mut fill_color_btns: Vec<Element<'a, Message>> = Vec::new();
        for &c in &self.palette {
            fill_color_btns.push(make_color_btn(c, Message::DraftFillColorChanged(c)));
        }
        fill_color_btns.push(self.chevron_toggle(ColorPickerKind::FillColor));
        
        let fill_color_row = row(fill_color_btns).spacing(2);

        let line_picker_ui: Element<'a, Message> = if self.show_color_picker == Some(ColorPickerKind::LineColor) {
            let ext_palette = get_extended_palette();
            let mut grid_rows: Vec<Element<'a, Message>> = Vec::new();
            for chunk in ext_palette.chunks(8) {
                let mut row_btns: Vec<Element<'a, Message>> = Vec::new();
                for &c in chunk {
                    row_btns.push(make_color_btn(c, Message::DraftColorChanged(c)));
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
                    row_btns.push(make_color_btn(c, Message::DraftFillColorChanged(c)));
                }
                grid_rows.push(row(row_btns).spacing(2).into());
            }
            container(column(grid_rows).spacing(2)).padding([0, 120]).into()
        } else {
            column![].into()
        };

        let fill_fields: Element<'a, Message> = if draft.poly_type == domain::PolyType::Polygon {
            column![
                row![text("Fill Color:").width(120), fill_color_row].align_y(Alignment::Center),
                fill_picker_ui,   // ← добавить
                row![
                    text("Fill Type:").width(120),
                    pick_list(fill_types, Some(draft.fill_type.clone()), Message::DraftFillTypeChanged)
                        .padding(native_theme_iced::button_padding(&self.resolved_theme))
                        .style(native_theme_iced::styles::pick_list(&self.resolved_theme)),
                ].align_y(Alignment::Center),
            ].spacing(10).into()
        } else {
            column![].into()
        };

        let input_style = native_theme_iced::styles::text_input(&self.resolved_theme);
        let input_pad = native_theme_iced::input_padding(&self.resolved_theme);
        let pick_pad = native_theme_iced::button_padding(&self.resolved_theme);
        let pick_style = native_theme_iced::styles::pick_list(&self.resolved_theme);

        let content = column![
            // Заголовок с кнопкой закрытия
            row![
                mouse_area(
                    container(text("Свойства трека").size(20))
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
                text("Description:").width(120),
                text_input("Введите описание...", &draft.description)
                    .on_input(Message::DraftDescChanged).width(280)
                    .padding(input_pad).style(input_style.clone())
            ].align_y(Alignment::Center),

            row![
                text("Дата:").width(120),
                self.btn_secondary("Вчера").on_press(Message::DraftDateYesterday),
                self.btn_secondary("Сегодня").on_press(Message::DraftDateToday),
                text_input("YYYY-MM-DD", &draft.date_str)
                    .on_input(Message::DraftDateChanged).width(120)
                    .padding(input_pad).style(input_style.clone()),
            ].align_y(Alignment::Center).spacing(4),

            row![
                text("Позывной:").width(120),
                pick_list(callsigns, Some(draft.callsign.clone()), Message::DraftCallsignChanged)
                    .padding(pick_pad).style(pick_style.clone()),
                container(text("#")).padding([0, 4]),
                text_input("", &draft.num1.to_string())
                    .on_input(Message::DraftNum1Changed).width(50)
                    .padding(input_pad).style(input_style.clone()),
                extra_fields
            ].align_y(Alignment::Center).spacing(4),

            row![text("Line Color:").width(120), color_row].align_y(Alignment::Center),
            line_picker_ui,

            row![
                text("Width:").width(120),
                text_input("", &draft.line_width.to_string())
                    .on_input(Message::DraftWidthChanged).width(50)
                    .padding(input_pad).style(input_style.clone()),
                container(text("Line Style:")).padding([0, 10]),
                pick_list(line_styles, Some(draft.line_style.clone()), Message::DraftStyleChanged)
                    .padding(pick_pad).style(pick_style.clone()),
            ].align_y(Alignment::Center),

            row![
                text("Type:").width(120),
                pick_list(poly_types, Some(draft.poly_type.clone()), Message::DraftTypeChanged)
                    .padding(pick_pad).style(pick_style.clone()),
            ].align_y(Alignment::Center),

            fill_fields,

            row![
                text("Шаблон:").width(120),
                text(template).color(Color::from_rgb8(draft.line_color[0], draft.line_color[1], draft.line_color[2])).size(16)
            ].align_y(Alignment::Center),

            row![
                self.btn_secondary("Сохранить").on_press(Message::SaveTrackProps),
                self.btn_secondary("Применить").on_press(Message::ApplyTrackProps),
                self.btn_secondary("Отмена").on_press(Message::CloseTrackProps),
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
                        radius: native_theme_iced::border_radius(&self.resolved_theme).into() 
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

        let make_color_btn = |c: [u8; 3], msg: Message| -> Element<'a, Message> {
            button(text("")).width(24).height(24)
                .style(move |_: &Theme, _| button::Style {
                    background: Some(iced::Background::Color(Color::from_rgb8(c[0], c[1], c[2]))),
                    border: iced::Border { 
                        color: Color::from_rgb(0.5, 0.5, 0.5), 
                        width: 1.0, 
                        radius: native_theme_iced::border_radius(&self.resolved_theme).into()
                    },
                    ..Default::default()
                })
                .on_press(msg)
                .into()
        };

        let mut text_color_btns: Vec<Element<'a, Message>> = text_colors
            .iter()
            .map(|&c| make_color_btn(c, Message::DraftWpTextColorChanged(c)))
            .collect();
            
        text_color_btns.push(self.chevron_toggle(ColorPickerKind::WpTextColor));
        
        let text_color_row = row(text_color_btns).spacing(2);

        let mut bg_color_btns: Vec<Element<'a, Message>> = bg_colors
            .iter()
            .map(|&c| make_color_btn(c, Message::DraftWpBgColorChanged(c)))
            .collect();
            
        bg_color_btns.push(self.chevron_toggle(ColorPickerKind::WpBgColor));
        
        let bg_color_row = row(bg_color_btns).spacing(2);
        
        let text_picker_ui: Element<'a, Message> = if self.show_color_picker == Some(ColorPickerKind::WpTextColor) {
            let ext_palette = get_extended_palette();
            let mut grid_rows: Vec<Element<'a, Message>> = Vec::new();
            for chunk in ext_palette.chunks(8) {
                let mut row_btns: Vec<Element<'a, Message>> = Vec::new();
                for &c in chunk {
                    row_btns.push(make_color_btn(c, Message::DraftWpTextColorChanged(c)));
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
                    row_btns.push(make_color_btn(c, Message::DraftWpBgColorChanged(c)));
                }
                grid_rows.push(row(row_btns).spacing(2).into());
            }
            container(column(grid_rows).spacing(2)).padding([0, 100]).into()
        } else {
            column![].into()
        };

        let input_style = native_theme_iced::styles::text_input(&self.resolved_theme);
        let input_pad = native_theme_iced::input_padding(&self.resolved_theme);

        let content = column![
            // Заголовок с кнопкой закрытия
            row![
                mouse_area(
                    container(text("Свойства точки").size(20))
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
                text("Имя:").width(100), 
                text_input("Имя", &draft.name)
                    .on_input(Message::DraftWpNameChanged).width(250)
                    .padding(input_pad).style(input_style.clone())
            ],
            row![
                text("Описание:").width(100), 
                text_input("Описание", draft.description.as_deref().unwrap_or(""))
                    .on_input(Message::DraftWpDescChanged).width(250)
                    .padding(input_pad).style(input_style.clone())
            ],
            
            row![
                text("Координаты:").width(100),
                text_input("55.7558 37.6173", coord_input)
                    .on_input(Message::DraftWpCoordsChanged).width(250)
                    .padding(input_pad).style(input_style.clone()),
            ].align_y(Alignment::Center),
            
            container(
                text("Форматы: dd.dddddd dd.dddddd | dd°mm.mmmm' dd°mm.mmmm' | dd°mm'ss.ss'' dd°mm'ss.ss'' | Узел (A1) | Узел + азимут + метры (A1 120 110)")
                    .size(11)
                    .color(Color::from_rgb(0.5, 0.5, 0.5))
            ).padding([0, 100]),

            row![
                text("Высота (м):").width(100),
                text_input("", &draft.ele.map(|e| e.to_string()).unwrap_or_default())
                    .on_input(Message::DraftWpEleChanged).width(100)
                    .padding(input_pad).style(input_style.clone()),
            ],
            row![text("Цвет текста:").width(100), text_color_row].align_y(Alignment::Center),
            text_picker_ui,
            row![text("Цвет фона:").width(100), bg_color_row].align_y(Alignment::Center),
            bg_picker_ui,
            
            row![
                self.btn_secondary("Вложение").on_press(Message::None),
                self.btn_secondary("Применить").on_press(Message::ApplyWaypointProps),
                self.btn_secondary("Отмена").on_press(Message::CloseWaypointProps),
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
                        radius: native_theme_iced::border_radius(&self.resolved_theme).into() 
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
            format!("{:.2} км", dist_m / 1000.0)
        } else {
            format!("{:.0} м", dist_m)
        };

        let content = column![
            text("Измерение между точками").size(20),
            
            row![
                text("От:").width(40),
                text(format!("{} ({:.6}, {:.6})", from_name, from_lat, from_lon))
            ],
            row![
                text("До:").width(40),
                text(format!("{} ({:.6}, {:.6})", to_name, to_lat, to_lon))
            ],
            Space::new().height(10),
            row![
                text("Расстояние:").width(120).size(16),
                text(dist_text).size(20).color(Color::from_rgb8(0x2a, 0x7f, 0xff))
            ],
            row![
                text("Магн. азимут:").width(120).size(16),
                text(format!("{:.0}°", mag_az)).size(20).color(Color::from_rgb8(255, 140, 0))
            ],
            Space::new().height(10),
            
            row![
                Space::new().width(Length::Fill),
                self.btn("Закрыть").on_press(Message::CloseWaypointMeasure),
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
                        radius: native_theme_iced::border_radius(&self.resolved_theme).into() 
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
            .style(native_theme_iced::styles::button(&self.resolved_theme))
            .into()
    }
    
    // =========================================================
    // Подтверждение Clear All
    // =========================================================
    pub(crate) fn view_confirm_clear_all_modal<'a>(&'a self) -> Element<'a, Message> {
        let content = column![
            text("Очистить сессию?").size(20),
            text("Все загруженные карты, треки и точки будут удалены.\nЭто действие нельзя отменить.").size(14),
            Space::new().height(10),
            row![
                self.btn("Да, очистить").on_press(Message::ExecuteClearAll),
                self.btn_secondary("Отмена").on_press(Message::CancelClearAll),
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
                        radius: native_theme_iced::border_radius(&self.resolved_theme).into(),
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
            text("Загрузить сессию из файла?").size(20),
            text("Текущее рабочее состояние будет заменено.\nЕсли оно важно — сохраните его в файл .foxy.")
                .size(14),
            Space::new().height(10),
            
            self.btn("Сохранить текущее и загрузить")
                .on_press(Message::SaveAndLoadSession)
                .width(Length::Fill),
            self.btn_secondary("Загрузить без сохранения")
                .on_press(Message::LoadSessionNoPrompt)
                .width(Length::Fill),
            self.btn_secondary("Отмена")
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
                        radius: native_theme_iced::border_radius(&self.resolved_theme).into(),
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
                            .style(native_theme_iced::styles::button(&self.resolved_theme)),
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
                        radius: native_theme_iced::border_radius(&self.resolved_theme).into(),
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
        let input_style = native_theme_iced::styles::text_input(&self.resolved_theme);
        let input_pad = native_theme_iced::input_padding(&self.resolved_theme);

        let content = column![
            text("Проекция точки").size(20),
            text("Создать новую точку по азимуту и расстоянию от исходной.").size(13),
            Space::new().height(8),
            row![
                text("Азимут (°):").width(120),
                text_input("0", azimuth_input)
                    .on_input(Message::ProjectAzimuthChanged)
                    .width(120).padding(input_pad).style(input_style.clone()),
            ].align_y(Alignment::Center),
            row![
                text("Расстояние (м):").width(120),
                text_input("100", distance_input)
                    .on_input(Message::ProjectDistanceChanged)
                    .width(120).padding(input_pad).style(input_style.clone()),
            ].align_y(Alignment::Center),
            Space::new().height(8),
            row![
                self.btn("Создать").on_press(Message::ExecuteProjection),
                self.btn_secondary("Отмена").on_press(Message::CloseWaypointProjection),
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
                        radius: native_theme_iced::border_radius(&self.resolved_theme).into(),
                    },
                    ..Default::default()
                }
            })
            .width(Length::Fixed(450.0))
            .into()
    }
}
