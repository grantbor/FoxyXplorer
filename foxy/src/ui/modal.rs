use iced::widget::{
    button, column, container, pick_list, row, text, text_input,
};
use iced::{Alignment, Color, Element, Theme};
use crate::domain;
use crate::util::coords::build_template;
use crate::widgets::palette::get_extended_palette;
use crate::{App, Message};

impl App {
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
        
        let text_color_row = row(text_colors.iter().map(|&c| make_color_btn(c, Message::DraftWpTextColorChanged(c))).collect::<Vec<_>>()).spacing(2);
        let bg_color_row = row(bg_colors.iter().map(|&c| make_color_btn(c, Message::DraftWpBgColorChanged(c))).collect::<Vec<_>>()).spacing(2);
        
        let input_style = native_theme_iced::styles::text_input(&self.resolved_theme);
        let input_pad = native_theme_iced::input_padding(&self.resolved_theme);
        
        let content = column![
            text("Свойства точки").size(20),
            
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
            row![text("Цвет фона:").width(100), bg_color_row].align_y(Alignment::Center),
            
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
        .into()
    }

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
                // ВОТ ЗДЕСЬ ОБЯЗАТЕЛЬНО ДОЛЖНО БЫТЬ .into()
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
        // СТАЛО:
        color_btns.push(
            button(lucide_iced::themed_icon(lucide_iced::bytes::CHEVRON_DOWN, 16.0))
            .width(24)
            .height(24)
            .padding(0) // Убираем лишние отступы, чтобы иконка была по центру
            .on_press(Message::ToggleColorPicker)
            .style(native_theme_iced::styles::button(&self.resolved_theme))
            .into()
        );
        let color_row = row(color_btns).spacing(2);
        
        let mut fill_color_btns: Vec<Element<'a, Message>> = Vec::new();
        for &c in &self.palette {
            fill_color_btns.push(make_color_btn(c, Message::DraftFillColorChanged(c)));
        }
        let fill_color_row = row(fill_color_btns).spacing(2);
        
        let color_picker_ui: Element<'a, Message> = if self.show_color_picker {
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
        
        let fill_fields: Element<'a, Message> = if draft.poly_type == domain::PolyType::Polygon {
            column![
                row![text("Fill Color:").width(120), fill_color_row].align_y(Alignment::Center),
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
            text("Свойства трека").size(20),
            
            row![
                text("Описание:").width(120),
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
            
            row![text("Цвет трека:").width(120), color_row].align_y(Alignment::Center),
            color_picker_ui,
            
            row![
                text("Толщина:").width(120),
                text_input("", &draft.line_width.to_string())
                .on_input(Message::DraftWidthChanged).width(50)
                .padding(input_pad).style(input_style.clone()),
                container(text("Стиль:")).padding([0, 10]),
                pick_list(line_styles, Some(draft.line_style.clone()), Message::DraftStyleChanged)
                .padding(pick_pad).style(pick_style.clone()),
            ].align_y(Alignment::Center),
            
            row![
                text("Тип:").width(120),
                pick_list(poly_types, Some(draft.poly_type.clone()), Message::DraftTypeChanged)
                .padding(pick_pad).style(pick_style.clone()),
            ].align_y(Alignment::Center),
            
            fill_fields,
            
            row![
                text("Шаблон:").width(120),
                text(template).color(Color::from_rgb8(draft.line_color[0], draft.line_color[1], draft.line_color[2])).size(16)
            ].align_y(Alignment::Center),
            
            // Кнопки (должны быть через self.btn и self.btn_secondary)
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
                radius: native_theme_iced::border_radius(&self.resolved_theme).into(), // <-- Добавили .into()
            },
            ..Default::default()
            }
        })
        .into()
    }
}
