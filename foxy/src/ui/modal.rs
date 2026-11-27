use iced::widget::{
    button, checkbox, column, container, mouse_area, pick_list, row, scrollable, text, text_input, tooltip, Space,
};
use iced::{Alignment, Color, Element, Length, Theme};
use crate::domain;
use crate::widgets::palette::get_extended_palette;
use crate::{App, Message, ContextMenuState, ContextMenuItem, ColorPickerKind};
use crate::t;
use crate::SettingsDraft;

// Include the repository README at compile time: the Windows EXE and other
// standalone builds do not need a separate README file at runtime.
const EMBEDDED_README: &str = include_str!("../../../README.md");

fn extract_readme_legal_sections(readme: &str) -> Option<(&str, &str)> {
    let license_start = readme.find("This program is free software:")?;
    let (license, _) = readme[license_start..]
        .split_once("------------------------------------------------------")?;
    let (_, sources) = readme.split_once("## Источники подложки")?;
    let (sources, _) = sources.split_once("### Кэширование и нагрузка")?;
    Some((license.trim(), sources.trim()))
}

fn readme_legal_sections() -> Option<(&'static str, &'static str)> {
    extract_readme_legal_sections(EMBEDDED_README)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadmeLineKind {
    Heading,
    Paragraph,
    Bullet,
    Quote,
}

// Strip only Markdown presentation markers. Keep the wording and mandatory
// legal notices from README intact, including their attribution and URLs.
fn readme_paragraphs(source: &str) -> Vec<(ReadmeLineKind, String)> {
    let mut paragraphs: Vec<(ReadmeLineKind, String)> = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() {
            // A paragraph break, including between consecutive quote blocks.
            if !paragraphs.is_empty() && paragraphs.last().is_some_and(|(_, text)| !text.is_empty()) {
                paragraphs.push((ReadmeLineKind::Paragraph, String::new()));
            }
            continue;
        }
        let (kind, content, starts_new) = if let Some(t) = line.strip_prefix("### ") {
            (ReadmeLineKind::Heading, t, true)
        } else if let Some(t) = line.strip_prefix("#### ") {
            (ReadmeLineKind::Heading, t, true)
        } else if let Some(t) = line.strip_prefix("- ") {
            (ReadmeLineKind::Bullet, t, true)
        } else if let Some(t) = line.strip_prefix("> ") {
            (ReadmeLineKind::Quote, t, false)
        } else {
            (ReadmeLineKind::Paragraph, line, false)
        };
        let content = content.replace("**", "").replace('`', "");
        if !starts_new {
            if let Some((last_kind, last_text)) = paragraphs.last_mut() {
                // Continuation lines in README are hard-wrapped; join them
                // into paragraphs instead of treating each as a UI row.
                if !last_text.is_empty() && (*last_kind == kind
                    || (kind == ReadmeLineKind::Paragraph
                        && *last_kind == ReadmeLineKind::Bullet)) {
                    last_text.push(' ');
                    last_text.push_str(&content);
                    continue;
                }
            }
        }
        paragraphs.push((kind, content));
    }
    paragraphs.into_iter().filter(|(_, text)| !text.is_empty()).collect()
}

#[cfg(test)]
mod readme_legal_tests {
    use super::*;

    #[test]
    fn embedded_readme_has_program_license_and_all_map_sources() {
        let (license, sources) = readme_legal_sections().expect("README legal section markers");
        assert!(license.contains("GNU General Public License"));
        for source in ["OpenStreetMap", "Sentinel-2", "ESRI", "Copernicus DEM"] {
            assert!(sources.contains(source), "missing {source} in embedded notices");
        }
        assert!(sources.contains("produced using Copernicus WorldDEM-30"));
        assert!(sources.contains("do not incur any liability"));
        assert!(!sources.contains("### Кэширование и нагрузка"));
        assert!(!sources.contains("### Manual point deletion"));
    }

    #[test]
    fn embedded_legal_sections_support_windows_line_endings() {
        let windows_readme = EMBEDDED_README.replace('\n', "\r\n");
        let (license, sources) = extract_readme_legal_sections(&windows_readme)
            .expect("README sections with CRLF line endings");
        assert!(license.starts_with("This program is free software:"));
        assert!(sources.contains("Copernicus DEM"));
    }

    #[test]
    fn legal_text_preserves_multiline_required_copernicus_notices() {
        let (_, sources) = readme_legal_sections().expect("README legal section markers");
        let text = readme_paragraphs(sources)
            .into_iter().map(|(_, s)| s).collect::<Vec<_>>().join("\n");
        assert!(text.contains("produced using Copernicus WorldDEM-30 © DLR e.V. 2010-2014 and © Airbus"));
        assert!(text.contains("provided under COPERNICUS by the European Union and ESA; all rights reserved"));
        assert!(text.contains("do not incur any liability for any use of the Copernicus WorldDEM-30"));
    }
}

// =========================================================
// Хелпер для форматирования даты/времени в таблице точек
// =========================================================
fn format_date_time(unix: Option<f64>) -> (String, String) {
    if let Some(t) = unix {
        let (y, m, d, h, min, sec, _) = geoio::time::parts_local(t);
        
        // Добавлено ключевое слово `let` и явная типизация
        let months: [&str; 12] = [
            crate::t!("msg_0339"), crate::t!("msg_0340"), crate::t!("msg_0341"), crate::t!("msg_0342"), crate::t!("msg_0343"), crate::t!("msg_0344"), crate::t!("msg_0345"), crate::t!("msg_0346"), crate::t!("msg_0347"), crate::t!("msg_0348"), crate::t!("msg_0349"), crate::t!("msg_0350")
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
        let track_name = self.tracks.iter().find(|t| t.id == state.track_id).map(|t| t.name.as_str()).unwrap_or(crate::t!("msg_0420"));
        let modal_width = (self.window_size.width - 32.0).min(1000.0).max(1.0);
        
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

            // Keep all commands on one line and pin Close to the right edge.
            // A minimum content width allows horizontal scrolling on narrow windows.
            scrollable(
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
                    self.btn_secondary(t!("msg_0075")).on_press_maybe(
                        (!state.selected_points.is_empty()).then_some(
                            Message::DeleteSelectedTrackPoints { leave_gap: false }
                        )
                    ),
                    self.btn_secondary(t!("msg_0484")).on_press_maybe(
                        (!state.selected_points.is_empty()).then_some(
                            Message::DeleteSelectedTrackPoints { leave_gap: true }
                        )
                    ),
                    Space::new().width(Length::Fill),
                    self.btn_secondary(t!("msg_0133")).on_press(Message::CloseTrackList),
                ]
                .spacing(8)
                .padding([10, 0])
                .width(Length::Fixed((modal_width - 40.0).max(820.0)))
            )
            .horizontal()
            .width(Length::Fill)
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
            // Wider than the old 750px dialog, but never wider than the app window.
            .width(Length::Fixed(modal_width))
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
            for chunk in ext_palette.chunks(10) {
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
            for chunk in ext_palette.chunks(10) {
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
                Space::new().width(Length::Fill),
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
        // Waypoint text is deliberately limited to black or white.
        let text_colors = [[0, 0, 0], [255, 255, 255]];
        let mut bg_colors = crate::util::waypoint_categories::background_colors();
        // The category palette has eight distinct colors; add black as the ninth.
        if !bg_colors.contains(&[0, 0, 0]) {
            bg_colors.push([0, 0, 0]);
        }
        let categories = crate::util::waypoint_categories::all();
        let empty_category = crate::t!("msg_0351");
        let category_selected = draft.category_code.as_ref()
            .and_then(|code| categories.iter().find(|c| &c.code == code))
            .map(|c| c.name.clone())
            .unwrap_or_else(|| empty_category.to_string());
        let mut category_names = vec![empty_category.to_string()];
        category_names.extend(categories.iter().map(|c| c.name.clone()));

        // The waypoint's original file (same read-only field as Track Properties).
        let file_path: Option<&'a std::path::Path> = match &draft.source {
            domain::FileSource::File { path, .. } => Some(path.as_path()),
            domain::FileSource::New => None,
        };
        let file_name: &'a str = file_path
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or(t!("msg_0134"));

        let file_field: Element<'a, Message> = if let Some(path) = file_path {
            container(
                tooltip(
                    text_input("", file_name)
                        .padding(crate::ui::styles::input_padding())
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

        let text_color_btns: Vec<Element<'a, Message>> = text_colors
            .iter()
            .map(|&c| make_color_btn(c, c == draft.text_color, Message::DraftWpTextColorChanged(c)))
            .collect();

        let text_color_row = row(text_color_btns).spacing(2);

        let mut bg_color_btns: Vec<Element<'a, Message>> = bg_colors
            .iter()
            .map(|&c| make_color_btn(c, c == draft.bg_color, Message::DraftWpBgColorChanged(c)))
            .collect();
            
        bg_color_btns.push(self.chevron_toggle(ColorPickerKind::WpBgColor));
        
        let bg_color_row = row(bg_color_btns).spacing(2);
        
        let bg_picker_ui: Element<'a, Message> = if self.show_color_picker == Some(ColorPickerKind::WpBgColor) {
            let mut grid_rows: Vec<Element<'a, Message>> = Vec::new();
            for chunk in get_extended_palette().chunks(10) {
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
                text(t!("msg_0140")).width(100),
                file_field,
            ].align_y(Alignment::Center),
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
            row![
                text(crate::t!("msg_0352")).width(100),
                pick_list(category_names, Some(category_selected), Message::DraftWpCategoryChanged)
                    .width(Length::Fixed(290.0))
                    .padding(crate::ui::styles::button_padding())
                    .style(crate::ui::styles::pick_list),
            ].align_y(Alignment::Center),
            row![
                text(crate::t!("msg_0353")).width(100),
                text_input("0–359", &draft.azimuth.map(|v| v.to_string()).unwrap_or_default())
                    .on_input(Message::DraftWpAzimuthChanged)
                    .width(Length::Fixed(90.0))
                    .padding(input_pad).style(input_style.clone()),
            ].align_y(Alignment::Center),
            row![text(t!("msg_0158")).width(100), text_color_row].align_y(Alignment::Center),
            row![text(t!("msg_0159")).width(100), bg_color_row].align_y(Alignment::Center),
            bg_picker_ui,
            
            row![
                Space::new().width(Length::Fill),
               // self.btn_secondary(t!("msg_0160")).on_press(Message::None),
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
                Space::new().width(Length::Fill),
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
                Space::new().width(Length::Fill),
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
    pub(crate) fn view_data_licenses_modal<'a>(&'a self) -> Element<'a, Message> {
        let width = (self.window_size.width - 48.0).min(780.0).max(1.0);
        let height = (self.window_size.height - 64.0).min(690.0).max(1.0);

        let mut content = column![].spacing(9).width(Length::Fill);
        if let Some((license, sources)) = readme_legal_sections() {
            content = content.push(text(t!("msg_0624")).size(18));
            for (kind, paragraph) in readme_paragraphs(license) {
                content = content.push(match kind {
                    ReadmeLineKind::Heading => text(paragraph).size(17),
                    ReadmeLineKind::Bullet => text(format!("• {paragraph}")).size(14),
                    ReadmeLineKind::Quote | ReadmeLineKind::Paragraph => text(paragraph).size(14),
                });
            }
            content = content.push(Space::new().height(12));
            content = content.push(text(t!("msg_0623")).size(18));
            for (kind, paragraph) in readme_paragraphs(sources) {
                content = content.push(match kind {
                    ReadmeLineKind::Heading => text(paragraph).size(17),
                    ReadmeLineKind::Bullet => text(format!("• {paragraph}")).size(14),
                    ReadmeLineKind::Quote | ReadmeLineKind::Paragraph => text(paragraph).size(14),
                });
            }
        } else {
            content = content.push(text(t!("msg_0626")).size(14));
        }

        let layout = column![
            text(t!("msg_0623")).size(21),
            scrollable(container(content).padding([8, 4]).width(Length::Fill))
                .height(Length::Fill),
            row![
                Space::new().width(Length::Fill),
                self.btn(t!("msg_0625")).on_press(Message::CloseDataLicenses),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(12)
        .padding(20)
        .width(Length::Fill)
        .height(Length::Fixed(height));

        container(layout)
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
            .width(Length::Fixed(width))
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

        // The release check is started only on request, never at application startup.
        let check_button = self.btn_secondary(t!("msg_0424"));
        let check_button = if matches!(&self.update_check, crate::update_check::Status::Checking) {
            check_button
        } else {
            check_button.on_press(Message::CheckForUpdates)
        };
        let mut check_area = column![check_button]
            .spacing(8)
            .align_x(Alignment::Center);
        match &self.update_check {
            crate::update_check::Status::NotChecked => {}
            crate::update_check::Status::Checking => {
                check_area = check_area.push(text(t!("msg_0425")).size(13));
            }
            crate::update_check::Status::UpToDate => {
                check_area = check_area.push(text(t!("msg_0426")).size(13));
            }
            crate::update_check::Status::Available(version) => {
                check_area = check_area.push(
                    text(crate::tf!("msg_0427", version = version)).size(13)
                );
                check_area = check_area.push(
                    button(text(t!("msg_0428")).size(14))
                        .padding(0)
                        .style(link_style)
                        .on_press(Message::OpenUrl(crate::update_check::RELEASES_URL.to_owned()))
                );
            }
            crate::update_check::Status::NoReleases => {
                check_area = check_area.push(text(t!("msg_0429")).size(13));
            }
            crate::update_check::Status::Failed => {
                check_area = check_area.push(text(t!("msg_0430")).size(13));
            }
        }

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
            separator(),
            check_area,
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
            self.btn_secondary(t!("msg_0623")).on_press(Message::OpenDataLicenses),
            self.btn(t!("msg_0133")).on_press(Message::CloseAbout),
        ].spacing(10).align_y(Alignment::Center);

        // Общая композиция
        let content = column![
            top_content,
            separator(),
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

        let status: Element<'a, Message> = match self.drop_phase {
            Phase::Stopped => text(crate::t!("msg_0312"))
                .size(18)
                .color(Color::from_rgb8(180, 75, 65))
                .into(),

            Phase::Starting => text(crate::t!("msg_0313"))
                .size(18)
                .into(),

            Phase::Stopping => text(crate::t!("msg_0314"))
                .size(18)
                .into(),

            Phase::Running => text(crate::t!("msg_0315"))
                .size(18)
                .color(Color::from_rgb8(45, 175, 96))
                .into(),
        };

        let mut body = column![status]
            .spacing(12)
            .align_x(Alignment::Center);

        // Сообщения об ошибках
        if let Some(error) = &self.drop_error {
            // Errors are localized at their source using translations.json.
            body = body.push(
                text(error.as_str())
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
                            crate::t!("msg_0316")
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
            crate::t!("msg_0317").to_string()
        } else {
            format!(
                "{}: {}",
                crate::t!("msg_0318"),
                current_dir
            )
        };

        body = body.push(text(folder_text).size(12));

        // Запуск и остановка
        if self.drop_phase == Phase::Stopped {
            body = body.push(
                self.btn(crate::t!("msg_0319"))
                    .on_press(Message::FoxyDropStart)
            );
        } else if self.drop_phase == Phase::Running {
            body = body.push(
                self.btn_secondary(crate::t!("msg_0320"))
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

        // Основной ряд цветов и расширенная палитра 10×6, как в Track Properties.
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
            for chunk in get_extended_palette().chunks(10) {
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

        // A single checkbox when disabled; the full settings block when enabled.
        // Fields survive toggling and the OS credential store is never cleared here.
        let ftp_settings: Element<'a, Message> = if draft.ftp_enabled {
            column![
                checkbox(true)
                    .label(crate::t!("msg_0354"))
                    .on_toggle(Message::SettingsFtpEnabledChanged),
                row![
                    text(crate::t!("msg_0357")).width(160),
                    text_input("ftp.example.com", &draft.ftp_host)
                        .on_input(Message::SettingsFtpHostChanged)
                        .padding(input_pad).style(input_style.clone()).width(Length::Fill),
                ].align_y(Alignment::Center),
                row![
                    text(crate::t!("msg_0358")).width(160),
                    text_input("21", &draft.ftp_port)
                        .on_input(Message::SettingsFtpPortChanged)
                        .padding(input_pad).style(input_style.clone()).width(80),
                ].align_y(Alignment::Center),
                row![
                    text(crate::t!("msg_0359")).width(160),
                    text_input("", &draft.ftp_username)
                        .on_input(Message::SettingsFtpUserChanged)
                        .padding(input_pad).style(input_style.clone()).width(Length::Fill),
                ].align_y(Alignment::Center),
                row![
                    text(crate::t!("msg_0360")).width(160),
                    text_input("", &draft.ftp_password)
                        .secure(true)
                        .on_input(Message::SettingsFtpPasswordChanged)
                        .padding(input_pad).style(input_style.clone()).width(Length::Fill),
                ].align_y(Alignment::Center),
                text(crate::t!("msg_0355")).size(11),
            ].spacing(12).into()
        } else {
            checkbox(false)
                .label(crate::t!("msg_0356"))
                .on_toggle(Message::SettingsFtpEnabledChanged)
                .into()
        };

        let current_lang_label = match draft.lang.as_str() {
            "en" => crate::t!("msg_0365"),
            _ => crate::t!("msg_0364"),
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
                    [crate::t!("msg_0364"), crate::t!("msg_0365")],
                    Some(current_lang_label),
                    |label: &str| Message::SettingsLangChanged(label.to_string())
                ).padding(pick_pad).style(pick_style.clone()),
            ].align_y(Alignment::Center),

            // Допуск упрощения
            // Глобальная тема: встроенные варианты iced::Theme.
        row![
            text(crate::t!("msg_0361")).width(160),
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
                text(crate::t!("msg_0362")).width(160),
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

            separator(),
            ftp_settings,
            separator(),
            // Настраиваемый цвет линий и узлов сетки.
            row![
                text(crate::t!("msg_0363")).width(160),
                grid_color_row,
            ].align_y(Alignment::Center).spacing(6),
            grid_color_picker,
            ].spacing(12).width(Length::Fill)).height(Length::Fill),

            // Кнопки всегда видны, даже если палитра раскрыта.
            row![
                Space::new().width(Length::Fill),
                self.btn(t!("msg_0151")).on_press(Message::ApplySettings),
                self.btn_secondary(t!("msg_0152")).on_press(Message::CloseSettings),
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
                    if draft.ftp_enabled { 900.0_f32 } else { 730.0_f32 }
                } else if draft.ftp_enabled {
                    740.0_f32
                } else {
                    540.0_f32
                }).min((self.window_size.height - 64.0).max(240.0))
            ))
            .into()
    }
}
