use iced::widget::{column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Element, Length, Theme};
use crate::{App, FtpKind, Message, Modal};
use crate::i18n::tr;

impl App {
    pub(crate) fn view_ftp_modal(&self) -> Element<'_, Message> {
        let header = text("FoxyUpload · FTP").size(20);
        let mut body = column![header].spacing(12);
        match self.modal.as_ref() {
            Some(Modal::FtpConfirm { kind, folder }) => {
                let operation = match kind {
                    FtpKind::Tracks => format!("{}: {} PLT", tr("Треки", "Tracks"), self.selection.tracks.len()),
                    FtpKind::WaypointsNew(path) => format!("{}: {}", tr("Новый WPT", "New WPT"), path.file_name().unwrap_or_default().to_string_lossy()),
                    FtpKind::WaypointsAppend(path) => format!("WPT+: {}", path.file_name().unwrap_or_default().to_string_lossy()),
                };
                body = body.push(text(operation));
                body = body.push(text(tr("Локальная папка: 10-Tracks", "Local folder: 10-Tracks")));
                body = body.push(row![
                    text(tr("Папка FTP:", "FTP folder:")).width(105),
                    text_input(tr("Имя папки на сервере", "Folder name on server"), folder)
                        .on_input(Message::FtpFolderChanged)
                        .padding(crate::ui::styles::input_padding())
                        .style(crate::ui::styles::text_input)
                        .width(Length::Fill),
                ].align_y(Alignment::Center).spacing(8));
                body = body.push(text(tr("Сетка загружается, только если её нет на сервере. Одноимённые PLT/WPT будут удалены и заменены.",
                    "The grid is uploaded only if absent on the server. Existing PLT/WPT files with matching names will be deleted and replaced.")).size(12));
                body = body.push(text(tr("FTP без шифрования: пароль и файлы передаются открытым текстом.",
                    "Unencrypted FTP: passwords and files are sent in plain text.")).size(12));
                body = body.push(row![
                    Space::new().width(Length::Fill),
                    self.btn(tr("Сохранить и отправить", "Save and upload")).on_press(Message::FtpStart),
                    self.btn_secondary(tr("Отмена", "Cancel")).on_press(Message::FtpClose),
                ].spacing(8));
            }
            Some(Modal::FtpProgress) => {
                body = body.push(text(tr("Файлы сохранены локально. Выполняется FTP-загрузка (до 5 попыток на файл)…",
                    "Files saved locally. Uploading via FTP (up to 5 attempts per file)…")));
                body = body.push(text(tr("Дождитесь итогового сообщения перед повторной отправкой.",
                    "Wait for the final result before uploading again.")).size(12));
            }
            Some(Modal::FtpResult(report)) => {
                body = body.push(text(format!("{}: {}", tr("Загружено файлов", "Files uploaded"), report.uploaded)));
                if report.skipped_grid {
                    body = body.push(text(tr("Существующая сетка на сервере не изменена.", "Existing server grid was left unchanged.")));
                }
                if report.failures.is_empty() {
                    body = body.push(text(tr("Передача завершена успешно.", "Upload completed successfully.")));
                } else {
                    body = body.push(text(format!("{}: {}. {}",
                        tr("Не удалось передать", "Failed to upload"), report.failures.len(),
                        tr("Требуется вмешательство пользователя.", "Manual intervention is required."))));
                    let mut errors = column![].spacing(6);
                    for f in &report.failures {
                        errors = errors.push(text(format!("{}: {}", f.file.display(), f.error)).size(12));
                    }
                    body = body.push(scrollable(errors).height(Length::Fixed(180.0)));
                    body = body.push(text(tr("Файлы остались в 10-Tracks. Загрузите их вручную после устранения ошибки.",
                        "The files remain in 10-Tracks. Upload them manually after resolving the problem.")).size(12));
                }
                body = body.push(row![
                    Space::new().width(Length::Fill),
                    self.btn_secondary(tr("Закрыть", "Close")).on_press(Message::FtpClose),
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
