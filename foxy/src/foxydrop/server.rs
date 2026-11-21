use super::upload::{self, UploadState};
use axum::{extract::DefaultBodyLimit, routing::{get, post}, Router, response::Html};
use std::{net::{IpAddr, Ipv4Addr, SocketAddr}, path::PathBuf, sync::{Arc, Mutex, RwLock}, time::Duration};
use tokio::{net::TcpListener, sync::{oneshot, Semaphore}, task::JoinHandle};

struct LiveServer {
    shutdown: oneshot::Sender<()>,
    join: JoinHandle<()>,
}

/// Потокобезопасное управление сервером. Срок жизни не связан с модальным окном.
/// При уничтожении App сигнал остановки отправляется автоматически.
pub struct DropService {
    live: Mutex<Option<LiveServer>>,
    directory: Arc<RwLock<PathBuf>>,
}

impl DropService {
    pub fn new(directory: PathBuf) -> Self {
        Self { live: Mutex::new(None), directory: Arc::new(RwLock::new(directory)) }
    }

    pub fn set_directory(&self, directory: PathBuf) {
        if let Ok(mut d) = self.directory.write() { *d = directory; }
    }

    pub fn directory(&self) -> PathBuf {
        self.directory.read().map(|d| d.clone()).unwrap_or_default()
    }

    pub fn is_running(&self) -> bool {
        self.live.lock().map(|l| l.as_ref().is_some_and(|s| !s.join.is_finished())).unwrap_or(false)
    }

    pub async fn start(&self, ip: Ipv4Addr, preferred_port: u16) -> Result<SocketAddr, String> {
        if self.is_running() { return Err("FoxyDrop уже запущен".into()); }
        let folder = self.directory.read().map_err(|e| e.to_string())?.clone();
        if !folder.is_dir() { return Err(format!("Не найдена папка загрузки: {}", folder.display())); }
        // Проверяем право записи до открытия сокета.
        let probe = folder.join(format!(".foxydrop-check-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default().as_nanos()));
        let file = tokio::fs::OpenOptions::new().write(true).create_new(true).open(&probe)
            .await.map_err(|e| format!("Нет доступа для записи в папку: {e}"))?;
        drop(file);
        let _ = tokio::fs::remove_file(probe).await;

        let requested = SocketAddr::new(IpAddr::V4(ip), preferred_port);
        let listener = match TcpListener::bind(requested).await {
            Ok(listener) => listener,
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                TcpListener::bind(SocketAddr::new(IpAddr::V4(ip), 0)).await
                    .map_err(|e| format!("Невозможно открыть порт: {e}"))?
            }
            Err(e) => return Err(format!("Невозможно запустить FoxyDrop: {e}")),
        };
        let address = listener.local_addr().map_err(|e| e.to_string())?;
        let state = Arc::new(UploadState {
            directory: Arc::clone(&self.directory),
            permits: Arc::new(Semaphore::new(2)),
        });
        let router = Router::new()
            .route("/", get(|| async { Html(super::page::INDEX) }))
            .route("/upload", post(upload::handle))
            .layer(DefaultBodyLimit::max(upload::MAX_SIZE as usize + 1024 * 1024))
            .with_state(state);
        let (tx, rx) = oneshot::channel::<()>();
        let join = tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, router)
                .with_graceful_shutdown(async move { let _ = rx.await; })
                .await
            { eprintln!("FoxyDrop: {e}"); }
        });
        self.live.lock().map_err(|e| e.to_string())?.replace(LiveServer { shutdown: tx, join });
        Ok(address)
    }

    /// Инициирует shutdown сразу; возвращает задачу ожидания, не блокируя интерфейс.
    pub fn stop(&self) -> Option<JoinHandle<()>> {
        let running = self.live.lock().ok()?.take()?;
        let _ = running.shutdown.send(());
        Some(running.join)
    }

    pub async fn finish_stop(mut join: JoinHandle<()>) {
        if tokio::time::timeout(Duration::from_secs(15), &mut join).await.is_err() {
            join.abort();
            let _ = join.await;
        }
    }
}

impl Drop for DropService {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.live.lock() {
            if let Some(running) = guard.take() {
                let _ = running.shutdown.send(());
            }
        }
    }
}
