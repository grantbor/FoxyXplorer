use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use crate::domain::{Track, Waypoint};

#[derive(Serialize, Deserialize)]
pub struct SessionView {
    pub cx: f64,
    pub cy: f64,
    pub scale: f64,
    pub active: usize,
    pub show_grid: bool,
    pub show_labels: bool,
    pub format_str: String,
    pub window_size: Option<(f32, f32)>,
    pub window_pos: Option<(f32, f32)>,
}

#[derive(Serialize, Deserialize)]
pub struct Session {
    pub version: u32,
    pub last_kit: Option<PathBuf>,
    pub view: SessionView,
    pub tracks: Vec<Track>,
    pub waypoints: Vec<Waypoint>,
    pub next_id: u64,
}

pub fn foxy_config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("foxy")
}

pub fn last_kit_path() -> PathBuf {
    foxy_config_dir().join("last_kit")
}

pub fn load_last_kit() -> Option<PathBuf> {
    std::fs::read_to_string(last_kit_path())
        .ok()
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| p.is_dir())
}

pub fn save_last_kit(path: &Path) {
    let _ = std::fs::create_dir_all(foxy_config_dir());
    let _ = std::fs::write(last_kit_path(), path.to_string_lossy().as_bytes());
}

pub fn session_path() -> PathBuf {
    foxy_config_dir().join("session.json")
}

pub fn load_session() -> Option<Session> {
    let path = session_path();
    let content = std::fs::read_to_string(path).ok()?;
    let session: Session = serde_json::from_str(&content).ok()?;
    if session.version == 1 {
        Some(session)
    } else {
        None
    }
}

pub fn write_session(session: &Session) {
    if let Ok(json) = serde_json::to_string_pretty(session) {
        let path = session_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, json);
    }
}
