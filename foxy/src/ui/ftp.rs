use iced::widget::{column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Element, Length, Theme};
use crate::{App, FtpKind, Message, Modal};

impl App {
    pub(crate) fn view_ftp_modal(&self) -> Element<'_, Message> {
        let header = text("FoxyUpload · FTP").size(20);
        let mut body = column![header].spacing(12);
        match self.modal.as_ref() {
            Some(Modal::FtpConfirm { kind, folder }) => {
                let operation = match kind {
                    FtpKind::Tracks => format!("{}: {} PLT", crate::t!("msg_0204"), self.selection.tracks.len()),
                    FtpKind::WaypointsNew(path) => format!("{}: {}", crate::t!("msg_0297"), path.file_name().unwrap_or_default().to_string_lossy()),
                    FtpKind::WaypointsAppend(path) => format!("WPT+: {}", path.file_name().unwrap_or_default().to_string_lossy()),
                };
                body = body.push(text(operation));
                body = body.push(text(crate::t!("msg_0298")));
                body = body.push(row![
                    text(crate::t!("msg_0299")).width(105),
                    text_input(crate::t!("msg_0300"), folder)
                        .on_input(Message::FtpFolderChanged)
                        .padding(crate::ui::styles::input_padding())
                        .style(crate::ui::styles::text_input)
                        .width(Length::Fill),
                ].align_y(Alignment::Center).spacing(8));
                body = body.push(text(crate::t!("msg_0301")).size(12));
                body = body.push(text(crate::t!("msg_0302")).size(12));
                body = body.push(row![
                    Space::new().width(Length::Fill),
                    self.btn(crate::t!("msg_0303")).on_press(Message::FtpStart),
                    self.btn_secondary(crate::t!("msg_0152")).on_press(Message::FtpClose),
                ].spacing(8));
            }
            Some(Modal::FtpProgress) => {
                body = body.push(text(crate::t!("msg_0304")));
                body = body.push(text(crate::t!("msg_0305")).size(12));
            }
            Some(Modal::FtpResult(report)) => {
                body = body.push(text(format!("{}: {}", crate::t!("msg_0306"), report.uploaded)));
                if report.skipped_grid {
                    body = body.push(text(crate::t!("msg_0307")));
                }
                if report.failures.is_empty() {
                    body = body.push(text(crate::t!("msg_0308")));
                } else {
                    body = body.push(text(format!("{}: {}. {}",
                        crate::t!("msg_0309"), report.failures.len(),
                        crate::t!("msg_0310"))));
                    let mut errors = column![].spacing(6);
                    for f in &report.failures {
                        errors = errors.push(text(format!("{}: {}", f.file.display(), f.error)).size(12));
                    }
                    body = body.push(scrollable(errors).height(Length::Fixed(180.0)));
                    body = body.push(text(crate::t!("msg_0311")).size(12));
                }
                body = body.push(row![
                    Space::new().width(Length::Fill),
                    self.btn_secondary(crate::t!("msg_0133")).on_press(Message::FtpClose),
                ]);
            }
            _ => {}
        }
        container(body.padding(20))
            .width(Length::Fixed(510.0))
            .style(|theme: &Theme| {
                let pal = theme.extended_palette();
                container::Style {
                    background: Some(pal.background.base.color.into()),
                    border: iced::Border { color: pal.background.strong.color, width: 1.0, radius: 6.0.into() },
                    ..Default::default()
                }
            })
            .into()
    }
}
