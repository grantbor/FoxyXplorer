use axum::{extract::{Multipart, State}, http::StatusCode, Json};
use serde::Serialize;
use std::{path::{Path, PathBuf}, sync::{Arc, RwLock}, sync::atomic::{AtomicU64, Ordering}, time::{SystemTime, UNIX_EPOCH}};
use tokio::{fs::{self, OpenOptions}, io::AsyncWriteExt, sync::Semaphore};

pub const MAX_SIZE: u64 = 100 * 1024 * 1024;
static UPLOAD_ID: AtomicU64 = AtomicU64::new(0);

pub struct UploadState {
    pub directory: Arc<RwLock<PathBuf>>,
    pub permits: Arc<Semaphore>,
}

#[derive(Serialize)]
pub struct UploadReply { message: &'static str }

type Reply = (StatusCode, Json<UploadReply>);
fn reply(code: StatusCode, message: &'static str) -> Reply {
    (code, Json(UploadReply { message }))
}

/// Удаляет временный файл даже при отмене HTTP-задачи во время приёма данных.
struct TempFile(PathBuf);
impl Drop for TempFile {
    fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); }
}

fn safe_name(raw: &str) -> Option<(String, String)> {
    // Браузеры могут прислать как Unix-, так и Windows-путь.
    let name = raw.rsplit(|ch| ch == '/' || ch == '\\').next()?.trim();
    let (stem, ext) = name.rsplit_once('.')?;
    let ext = ext.to_ascii_lowercase();
    if !matches!(ext.as_str(), "gpx" | "ms" | "plt" | "kml" | "wpt") { return None; }

    let mut clean = String::new();
    for ch in stem.trim().trim_matches('.').chars() {
        let ch = if ch.is_control() || "<>:\"/\\|?*".contains(ch) { '_' } else { ch };
        if clean.len() + ch.len_utf8() > 160 { break; }
        clean.push(ch);
    }
    clean = clean.trim_end_matches(|ch| ch == ' ' || ch == '.').to_string();
    if clean.trim().is_empty() { clean = "upload".to_string(); }
    // Windows: зарезервированные имена устройств (CON, PRN, NUL, COM1, ...).
    let upper = clean.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4 && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.as_bytes()[3].is_ascii_digit() && upper.as_bytes()[3] != b'0') {
        clean.insert(0, '_');
    }
    Some((clean, ext))
}

async fn persist_file(tmp: &Path, dir: &Path, stem: &str, ext: &str) -> std::io::Result<()> {
    for i in 0..10_000u32 {
        let name = if i == 0 { format!("{stem}.{ext}") } else { format!("{stem} ({i}).{ext}") };
        let target = dir.join(name);
        // hard_link — атомарная операция "создать, только если ещё нет".
        // Работает на ext4, NTFS, APFS; не перезаписывает существующие файлы.
        match fs::hard_link(tmp, &target).await {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "Слишком много совпадающих имён"))
}

pub async fn handle(State(state): State<Arc<UploadState>>, mut multipart: Multipart) -> Reply {
    let _permit = match Arc::clone(&state.permits).try_acquire_owned() {
        Ok(p) => p,
        Err(_) => return reply(StatusCode::TOO_MANY_REQUESTS, "Сбой загрузки"),
    };
    let mut field = match multipart.next_field().await {
        Ok(Some(f)) => f,
        _ => return reply(StatusCode::BAD_REQUEST, "Сбой загрузки"),
    };
    let (stem, ext) = match field.file_name().and_then(safe_name) {
        Some(parts) => parts,
        None => return reply(StatusCode::UNSUPPORTED_MEDIA_TYPE, "Неподдерживаемый тип"),
    };
    let dir = match state.directory.read() {
        Ok(g) => g.clone(),
        Err(_) => return reply(StatusCode::INTERNAL_SERVER_ERROR, "Сбой загрузки"),
    };
    if !dir.is_dir() { return reply(StatusCode::INTERNAL_SERVER_ERROR, "Сбой загрузки"); }

    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let id = UPLOAD_ID.fetch_add(1, Ordering::Relaxed);
    let temp_path = dir.join(format!(".foxydrop-{}-{nonce}-{id}.part", std::process::id()));
    let mut out = match OpenOptions::new().write(true).create_new(true).open(&temp_path).await {
        Ok(f) => f,
        Err(_) => return reply(StatusCode::INTERNAL_SERVER_ERROR, "Сбой загрузки"),
    };
    let tempfile = TempFile(temp_path);
    let mut received: u64 = 0;
    loop {
        match field.chunk().await {
            Ok(Some(data)) => {
                received = received.saturating_add(data.len() as u64);
                if received > MAX_SIZE || out.write_all(&data).await.is_err() {
                    return reply(StatusCode::PAYLOAD_TOO_LARGE, "Сбой загрузки");
                }
            }
            Ok(None) => break,
            Err(_) => return reply(StatusCode::BAD_REQUEST, "Сбой загрузки"),
        }
    }
    if out.flush().await.is_err() { return reply(StatusCode::INTERNAL_SERVER_ERROR, "Сбой загрузки"); }
    drop(out);
    if persist_file(&tempfile.0, &dir, &stem, &ext).await.is_err() {
        return reply(StatusCode::INTERNAL_SERVER_ERROR, "Сбой загрузки");
    }
    reply(StatusCode::OK, "Успешно загружено")
}

#[cfg(test)]
mod tests {
    use super::{safe_name, persist_file};
    #[test]
    fn extensions_and_path_traversal() {
        assert_eq!(safe_name("../photo.jpg"), None);
        assert_eq!(safe_name("C:\\temp\\track.GPX"), Some(("track".into(), "gpx".into())));
        assert_eq!(safe_name("../../very?bad.wpt"), Some(("very_bad".into(), "wpt".into())));
    }
    #[test]
    fn windows_reserved_names() {
        assert_eq!(safe_name("CON.gpx"), Some(("_CON".into(), "gpx".into())));
        assert_eq!(safe_name("con .plt"), Some(("_con".into(), "plt".into())));
    }

    #[tokio::test]
    async fn file_names_do_not_overwrite() {
        let dir = std::env::temp_dir().join(format!("foxydrop-test-{}-{}",
            std::process::id(), super::UPLOAD_ID.fetch_add(1, super::Ordering::Relaxed)));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let source = dir.join(".part");
        tokio::fs::write(&source, b"new data").await.unwrap();
        tokio::fs::write(dir.join("track.gpx"), b"existing").await.unwrap();
        persist_file(&source, &dir, "track", "gpx").await.unwrap();
        assert_eq!(tokio::fs::read(dir.join("track.gpx")).await.unwrap(), b"existing");
        assert_eq!(tokio::fs::read(dir.join("track (1).gpx")).await.unwrap(), b"new data");
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }
}
