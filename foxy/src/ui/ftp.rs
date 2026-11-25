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
                    FtpKind::Tracks => format!("Треки: {} PLT", self.selection.tracks.len()),
                    FtpKind::WaypointsNew(path) => format!("Новый WPT: {}", path.file_name().unwrap_or_default().to_string_lossy()),
                    FtpKind::WaypointsAppend(path) => format!("WPT+: {}", path.file_name().unwrap_or_default().to_string_lossy()),
                };
                body = body.push(text(operation));
                body = body.push(text("Локальная папка: 10-Tracks"));
                body = body.push(row![
                    text("Папка FTP:").width(105),
                    text_input("Имя папки на сервере", folder)
                        .on_input(Message::FtpFolderChanged)
                        .padding(crate::ui::styles::input_padding())
                        .style(crate::ui::styles::text_input)
                        .width(Length::Fill),
                ].align_y(Alignment::Center).spacing(8));
                body = body.push(text("Сетка загружается, только если её нет на сервере. Одноимённые PLT/WPT будут удалены и заменены.").size(12));
                body = body.push(text("FTP без шифрования: пароль и файлы передаются открытым текстом.").size(12));
                body = body.push(row![
                    Space::new().width(Length::Fill),
                    self.btn("Сохранить и отправить").on_press(Message::FtpStart),
                    self.btn_secondary("Отмена").on_press(Message::FtpClose),
                ].spacing(8));
            }
            Some(Modal::FtpProgress) => {
                body = body.push(text("Файлы сохранены локально. Выполняется FTP-загрузка (до 5 попыток на файл)…"));
                body = body.push(text("Дождитесь итогового сообщения перед повторной отправкой.").size(12));
            }
            Some(Modal::FtpResult(report)) => {
                body = body.push(text(format!("Загружено файлов: {}", report.uploaded)));
                if report.skipped_grid {
                    body = body.push(text("Существующая сетка на сервере не изменена."));
                }
                if report.failures.is_empty() {
                    body = body.push(text("Передача завершена успешно."));
                } else {
                    body = body.push(text(format!("Не удалось передать: {}. Требуется вмешательство пользователя.", report.failures.len())));
                    let mut errors = column![].spacing(6);
                    for f in &report.failures {
                        errors = errors.push(text(format!("{}: {}", f.file.display(), f.error)).size(12));
                    }
                    body = body.push(scrollable(errors).height(Length::Fixed(180.0)));
                    body = body.push(text("Файлы остались в 10-Tracks. Загрузите их вручную после устранения ошибки.").size(12));
                }
                body = body.push(row![
                    Space::new().width(Length::Fill),
                    self.btn_secondary("Закрыть").on_press(Message::FtpClose),
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
