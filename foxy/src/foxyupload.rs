//! FoxyUpload: a small passive-mode plain FTP client, with no temporary remote filenames.
//! The complete PLT/WPT/GPX files are prepared by the UI before the network job starts.
//! WARNING: standard FTP sends credentials and data without encryption.
use serde::{Deserialize, Serialize};
use crate::i18n::tr;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

const SERVICE: &str = "FoxyXplorer.FoxyUpload";
const ACCOUNT: &str = "ftp-connection";
const RETRIES: usize = 5;
const IO_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

impl Profile {
    pub fn validate(&self) -> Result<(), String> {
        if self.host.trim().is_empty() || self.username.trim().is_empty() || self.password.is_empty() || self.port == 0 {
            return Err(tr("Заполните FTP-сервер, порт, логин и пароль", "Enter FTP host, port, username and password").into());
        }
        for s in [&self.host, &self.username, &self.password] {
            if s.contains('\n') || s.contains('\r') { return Err(tr("Недопустимый символ в параметрах FTP", "Invalid character in FTP settings").into()); }
        }
        if self.host.contains('/') || self.host.contains('\\') || self.host.contains(':') {
            return Err(tr("Укажите имя FTP-сервера без ftp:// и без порта (порт — отдельно)", "Enter the FTP host without ftp:// or port (port is a separate field)").into());
        }
        Ok(())
    }
}

fn credential_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| format!("{}: {e}", tr("Хранилище паролей недоступно", "Credential store unavailable")))
}

/// All connection fields (including hostname) are stored ONLY in the OS keyring.
/// Nothing is written to config.json or session files.
pub fn load_profile() -> Result<Option<Profile>, String> {
    let entry = credential_entry()?;
    match entry.get_password() {
        Ok(json) => {
            let profile: Profile = serde_json::from_str(&json).map_err(|e| format!("{}: {e}", tr("Повреждён FTP-профиль", "FTP profile is corrupted")))?;
            profile.validate()?;
            Ok(Some(profile))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("{}: {e}", tr("Не удалось прочитать FTP-профиль", "Could not read FTP profile"))),
    }
}

pub fn save_profile(profile: &Profile) -> Result<(), String> {
    profile.validate()?;
    let json = serde_json::to_string(profile).map_err(|e| e.to_string())?;
    credential_entry()?.set_password(&json).map_err(|e| format!("{}: {e}", tr("Не удалось сохранить FTP-профиль", "Could not save FTP profile")))
}

/// Local filesystem-safe basename; all track exports remain inside 10-Tracks.
pub fn safe_filename(name: &str) -> String {
    let clean: String = name.trim().chars().take(150).map(|c| {
        if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c }
    }).collect();
    let clean = clean.trim_matches(|c: char| c == '.' || c == ' ');
    if clean.is_empty() { "track".to_string() } else { clean.to_string() }
}

/// A single folder name under the FTP login's home directory.
pub fn validate_folder(folder: &str) -> Result<&str, String> {
    let name = folder.trim();
    if name.is_empty() || name == "." || name == ".." || name.len() > 180
        || name.chars().any(|c| c.is_control() || c == '/' || c == '\\') {
        return Err(tr("Укажите простое имя папки FTP (без /, \\ и управляющих символов)", "Enter a simple FTP folder name (no /, \\ or control characters)").into());
    }
    Ok(name)
}

#[derive(Debug, Clone)]
pub struct FailedFile {
    pub file: PathBuf,
    pub error: String,
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub uploaded: usize,
    pub skipped_grid: bool,
    pub failures: Vec<FailedFile>,
}

#[derive(Clone)]
pub struct Plan {
    pub profile: Profile,
    pub remote_folder: String,
    pub grid: Option<PathBuf>,
    pub files: Vec<PathBuf>,
}

#[derive(Debug)]
struct FtpError {
    message: String,
    permanent: bool,
}
impl FtpError {
    fn temp(message: impl Into<String>) -> Self { Self { message: message.into(), permanent: false } }
    fn permanent(message: impl Into<String>) -> Self { Self { message: message.into(), permanent: true } }
}
type FtpResult<T> = Result<T, FtpError>;

struct Reply { code: u16, body: String }
struct Client {
    control: BufReader<TcpStream>,
    writer: TcpStream,
}
impl Client {
    fn connect(profile: &Profile, folder: &str) -> FtpResult<Self> {
        let address = format!("{}:{}", profile.host, profile.port);
        let addrs = address.to_socket_addrs().map_err(|e| FtpError::temp(format!("DNS: {e}")))?;
        let mut stream = None;
        let mut last_error = tr("Нет доступных адресов FTP", "No FTP addresses available").to_string();
        for addr in addrs {
            match TcpStream::connect_timeout(&addr, Duration::from_secs(10)) {
                Ok(s) => { stream = Some(s); break; }
                Err(e) => last_error = format!("{}: {e}", tr("Подключение", "Connection")),
            }
        }
        let writer = stream.ok_or_else(|| FtpError::temp(last_error))?;
        writer.set_read_timeout(Some(IO_TIMEOUT)).map_err(|e| FtpError::temp(e.to_string()))?;
        writer.set_write_timeout(Some(IO_TIMEOUT)).map_err(|e| FtpError::temp(e.to_string()))?;
        let control = BufReader::new(writer.try_clone().map_err(|e| FtpError::temp(e.to_string()))?);
        let mut ftp = Self { control, writer };
        ftp.expect(220, tr("Приветствие FTP", "FTP greeting"))?;
        let r = ftp.command(&format!("USER {}", profile.username))?;
        match r.code {
            230 => {},
            331 | 332 => { ftp.command_expect(&format!("PASS {}", profile.password), 230, tr("Авторизация", "Authentication"))?; }
            530 => return Err(FtpError::permanent(tr("Неверный логин или пароль FTP", "Invalid FTP username or password"))),
            _ => return Err(FtpError::permanent(format!("FTP USER: {}", r.code))),
        }
        ftp.command_expect("TYPE I", 200, tr("Бинарный режим", "Binary mode"))?;
        let reply = ftp.command(&format!("CWD {}", folder))?;
        if reply.code != 250 {
            // Create only if missing. If another client creates it concurrently, CWD still succeeds.
            let create = ftp.command(&format!("MKD {}", folder))?;
            if create.code != 257 && create.code != 250 {
                ftp.command_expect(&format!("CWD {}", folder), 250, tr("Папка FTP", "FTP folder"))?;
            } else {
                ftp.command_expect(&format!("CWD {}", folder), 250, tr("Папка FTP", "FTP folder"))?;
            }
        }
        Ok(ftp)
    }

    fn read_reply(&mut self) -> FtpResult<Reply> {
        let mut first_code = None;
        let mut body = String::new();
        for _ in 0..100 {
            let mut bytes = Vec::new();
            let count = self.control.read_until(b'\n', &mut bytes).map_err(|e| FtpError::temp(format!("{}: {e}", tr("Ответ FTP", "FTP response"))))?;
            if count == 0 { return Err(FtpError::temp(tr("FTP закрыл соединение", "FTP closed the connection"))); }
            let line = String::from_utf8_lossy(&bytes);
            let line = line.trim_end_matches(|c: char| c == '\r' || c == '\n');
            if body.len() < 4096 { body.push_str(line); body.push('\n'); }
            let b = line.as_bytes();
            if b.len() >= 4 && b[0..3].iter().all(|c| c.is_ascii_digit()) {
                let code = line[0..3].parse::<u16>().unwrap_or(0);
                if first_code.is_none() { first_code = Some(code); }
                if b[3] == b' ' && first_code == Some(code) { return Ok(Reply { code, body }); }
            }
        }
        Err(FtpError::temp(tr("Слишком длинный ответ FTP", "FTP response too long")))
    }
    fn expect(&mut self, code: u16, label: &str) -> FtpResult<Reply> {
        let r = self.read_reply()?;
        if r.code == code { Ok(r) }
        else if r.code >= 400 && r.code < 500 { Err(FtpError::temp(format!("{label}: {} {}", tr("ответ FTP", "FTP reply"), r.code))) }
        else { Err(FtpError::permanent(format!("{label}: {} {}", tr("ответ FTP", "FTP reply"), r.code))) }
    }
    fn command(&mut self, command: &str) -> FtpResult<Reply> {
        self.writer.write_all(command.as_bytes()).and_then(|_| self.writer.write_all(b"\r\n"))
            .map_err(|e| FtpError::temp(format!("{}: {e}", tr("Команда FTP", "FTP command"))))?;
        self.read_reply()
    }
    fn command_expect(&mut self, command: &str, code: u16, label: &str) -> FtpResult<Reply> {
        let reply = self.command(command)?;
        if reply.code == code { Ok(reply) }
        else if reply.code >= 400 && reply.code < 500 { Err(FtpError::temp(format!("{label}: {} {}", tr("ответ FTP", "FTP reply"), reply.code))) }
        else { Err(FtpError::permanent(format!("{label}: {} {}", tr("ответ FTP", "FTP reply"), reply.code))) }
    }
    fn remote_size(&mut self, name: &str) -> FtpResult<Option<u64>> {
        let r = self.command(&format!("SIZE {}", name))?;
        match r.code {
            213 => {
                let size = r.body.lines().last().unwrap_or("").split_whitespace().nth(1)
                    .and_then(|s| s.parse().ok());
                Ok(size)
            }
            550 => Ok(None),
            500 | 502 | 504 => Ok(None), // optional verification unsupported
            n => Err(FtpError::temp(format!("SIZE: {} {n}", tr("ответ FTP", "FTP reply")))),
        }
    }
    fn grid_exists(&mut self, name: &str) -> FtpResult<bool> {
        let r = self.command(&format!("SIZE {}", name))?;
        match r.code {
            213 => Ok(true),
            550 => Ok(false),
            500 | 502 | 504 => Err(FtpError::permanent(tr("FTP-сервер не поддерживает SIZE; существующую сетку нельзя безопасно проверить",
                        "FTP server does not support SIZE; cannot safely check the existing grid"))),
            n => Err(FtpError::temp(format!("{}: FTP {n}", tr("Проверка сетки", "Grid check")))),
        }
    }
    /// In a dedicated kit folder any .gpx is a grid. Check different filenames
    /// too, so an existing grid is never overwritten or duplicated.
    fn any_grid_exists(&mut self, candidate: &str) -> FtpResult<bool> {
        let mut data = self.passive_socket()?;
        let reply = self.command("NLST")?;
        match reply.code {
            125 | 150 => {
                let mut listing = Vec::new();
                data.read_to_end(&mut listing).map_err(|e| FtpError::temp(format!("NLST: {e}")))?;
                drop(data);
                let finish = self.read_reply()?;
                if finish.code != 226 && finish.code != 250 {
                    return Err(FtpError::temp(format!("NLST: {} {}", tr("ответ FTP", "FTP reply"), finish.code)));
                }
                let names = String::from_utf8_lossy(&listing);
                Ok(names.lines().any(|n| n.trim().to_ascii_lowercase().ends_with(".gpx")))
            }
            550 => Ok(false), // empty directory
            500 | 502 | 504 => self.grid_exists(candidate), // NLST unsupported
            n => Err(FtpError::temp(format!("NLST: {} {n}", tr("ответ FTP", "FTP reply")))),
        }
    }
    fn delete_existing(&mut self, name: &str) -> FtpResult<()> {
        // DELE 550 can mean 'not found' (or no permission); STOR will reveal permission errors.
        let r = self.command(&format!("DELE {}", name))?;
        match r.code {
            250 | 550 => Ok(()),
            n if n >= 500 => Err(FtpError::permanent(format!("DELE: {} {n}", tr("ответ FTP", "FTP reply")))),
            n => Err(FtpError::temp(format!("DELE: {} {n}", tr("ответ FTP", "FTP reply")))),
        }
    }
    fn passive_socket(&mut self) -> FtpResult<TcpStream> {
        let peer = self.writer.peer_addr().map_err(|e| FtpError::temp(e.to_string()))?;
        let r = self.command("EPSV")?;
        let port = if r.code == 229 {
            let start = r.body.find('(').ok_or_else(|| FtpError::temp(tr("Некорректный EPSV", "Invalid EPSV response")))?;
            let end = r.body[start..].find(')').map(|p| start + p).ok_or_else(|| FtpError::temp(tr("Некорректный EPSV", "Invalid EPSV response")))?;
            let inner = &r.body[start+1..end];
            inner.trim_matches(|c: char| !c.is_ascii_digit()).parse::<u16>()
                .map_err(|e| FtpError::temp(format!("{}: {e}", tr("Некорректный порт EPSV", "Invalid EPSV port"))))?
        } else {
            let r = self.command("PASV")?;
            if r.code != 227 { return Err(FtpError::temp(format!("PASV: {} {}", tr("ответ FTP", "FTP reply"), r.code))); }
            let start = r.body.find('(').ok_or_else(|| FtpError::temp(tr("Некорректный PASV", "Invalid PASV response")))?;
            let end = r.body[start..].find(')').map(|p| start + p).ok_or_else(|| FtpError::temp(tr("Некорректный PASV", "Invalid PASV response")))?;
            let nums: Vec<u16> = r.body[start+1..end].split(',').map(str::trim).filter_map(|s| s.parse().ok()).collect();
            if nums.len() != 6 || nums[4] > 255 || nums[5] > 255 { return Err(FtpError::temp(tr("Некорректный адрес PASV", "Invalid PASV address"))); }
            nums[4] * 256 + nums[5]
        };
        // Use the control-connection IP: some FTP servers return private addresses in PASV.
        let addr = std::net::SocketAddr::new(peer.ip(), port);
        let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))
            .map_err(|e| FtpError::temp(format!("FTP data: {e}")))?;
        stream.set_read_timeout(Some(IO_TIMEOUT)).map_err(|e| FtpError::temp(e.to_string()))?;
        stream.set_write_timeout(Some(IO_TIMEOUT)).map_err(|e| FtpError::temp(e.to_string()))?;
        Ok(stream)
    }
    fn send_file(&mut self, path: &Path) -> FtpResult<()> {
        let name = path.file_name().and_then(|f| f.to_str())
            .ok_or_else(|| FtpError::permanent(tr("Недопустимое имя файла", "Invalid filename")))?;
        if name.chars().any(|c| c == '\n' || c == '\r' || c == '/') {
            return Err(FtpError::permanent(tr("Недопустимое имя FTP-файла", "Invalid FTP filename")));
        }
        let mut local = std::fs::File::open(path).map_err(|e| FtpError::permanent(format!("{}: {e}", path.display())))?;
        let length = local.metadata().map_err(|e| FtpError::permanent(e.to_string()))?.len();
        self.delete_existing(name)?;
        let mut data = self.passive_socket()?;
        let response = self.command(&format!("STOR {}", name))?;
        if response.code != 125 && response.code != 150 {
            let message = format!("STOR {}: {} {}", name, tr("ответ FTP", "FTP reply"), response.code);
            return Err(if response.code >= 400 && response.code < 500 { FtpError::temp(message) } else { FtpError::permanent(message) });
        }
        io::copy(&mut local, &mut data).map_err(|e| FtpError::temp(format!("{} {}: {e}", tr("Передача", "Transfer"), name)))?;
        data.shutdown(Shutdown::Write).map_err(|e| FtpError::temp(format!("{}: {e}", tr("Завершение передачи", "Finishing transfer"))))?;
        drop(data);
        let final_reply = self.read_reply()?;
        if final_reply.code != 226 && final_reply.code != 250 {
            return Err(FtpError::temp(format!("{} STOR {}: {}", tr("Завершение", "Finish"), name, final_reply.code)));
        }
        if let Some(remote_len) = self.remote_size(name)? {
            if length != remote_len { return Err(FtpError::temp(format!("{}: {} {} {} {}", name, tr("размер", "size"), remote_len,
                        tr("вместо", "instead of"), length))); }
        }
        Ok(())
    }
}

fn send_with_retry(plan: &Plan, file: &Path, grid: bool) -> Result<bool, String> {
    let mut last_error = String::new();
    // Once we have proved the grid is absent, retries may replace OUR partial upload.
    // If the probe itself fails, try the probe again and never delete an old grid.
    let mut grid_absence_confirmed = false;
    for attempt in 1..=RETRIES {
        if attempt > 1 { std::thread::sleep(Duration::from_secs(1 << (attempt - 2))); }
        let result: FtpResult<bool> = (|| {
            let mut ftp = Client::connect(&plan.profile, &plan.remote_folder)?;
            if grid && !grid_absence_confirmed {
                let name = file.file_name().and_then(|n| n.to_str()).ok_or_else(|| FtpError::permanent(tr("Имя сетки не UTF-8", "Grid filename is not UTF-8")))?;
                if ftp.any_grid_exists(name)? { return Ok(true); }
                grid_absence_confirmed = true;
            }
            ftp.send_file(file)?;
            let _ = ftp.command("QUIT");
            Ok(false)
        })();
        match result {
            Ok(skipped) => return Ok(skipped),
            Err(e) => {
                last_error = e.message;
                if e.permanent { break; }
            }
        }
    }
    Err(if crate::i18n::current_lang() == "en" {
        format!("{last_error} (failed after up to {RETRIES} attempts or permanent error)")
    } else {
        format!("{last_error} (не удалось за {RETRIES} попыток или ошибка постоянная)")
    })
}

pub fn upload_batch(plan: Plan) -> Report {
    let mut report = Report::default();
    if let Err(err) = plan.profile.validate().and_then(|_| validate_folder(&plan.remote_folder).map(|_| ())) {
        report.failures.push(FailedFile { file: PathBuf::from("FTP"), error: err });
        return report;
    }
    if let Some(grid) = &plan.grid {
        match send_with_retry(&plan, grid, true) {
            Ok(skipped) => { report.skipped_grid = skipped; if !skipped { report.uploaded += 1; } }
            Err(error) => report.failures.push(FailedFile { file: grid.clone(), error }),
        }
    }
    for file in &plan.files {
        match send_with_retry(&plan, file, false) {
            Ok(_) => report.uploaded += 1,
            Err(error) => report.failures.push(FailedFile { file: file.clone(), error }),
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ftp_folder_rejects_traversal() {
        for bad in ["", "..", "foo/bar", "foo\\bar", "hello\nworld"] {
            assert!(validate_folder(bad).is_err());
        }
        assert_eq!(validate_folder("Search_2026").unwrap(), "Search_2026");
    }
    #[test]
    fn filename_cannot_escape_tracks_directory() {
        assert_eq!(safe_filename("../Test:Track"), "_Test_Track");
        assert_eq!(safe_filename(""), "track");
        assert_eq!(safe_filename("Track_01"), "Track_01");
    }
    #[test]
    fn credentials_reject_ftp_url_and_newline() {
        let good = Profile { host: "ftp.example.com".into(), port: 21, username: "user".into(), password: "secret".into() };
        assert!(good.validate().is_ok());
        let mut bad = good.clone(); bad.host = "ftp://example.com".into();
        assert!(bad.validate().is_err());
        bad.host = "example.com".into(); bad.password = "p\nWORD".into();
        assert!(bad.validate().is_err());
    }
}
