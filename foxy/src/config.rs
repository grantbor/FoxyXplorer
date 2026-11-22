//! Пользовательские настройки: ~/.foxy/config.json
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub default_lon: f64,
    pub default_lat: f64,
    pub lang: String,
    pub simplify_tolerance: f64,
    #[serde(default = "default_grid_color")]
    pub grid_color: [u8; 3],
    #[serde(default)]
    pub drop_folder: String,
    #[serde(default = "default_drop_port")]
    pub drop_port: u16,
    #[serde(default = "default_ui_theme")]
    pub ui_theme: String,
}

fn default_drop_port() -> u16 { 8070 }
fn default_ui_theme() -> String { "Catppuccin Frappé".to_string() }

fn default_grid_color() -> [u8; 3] {
    [255, 70, 70]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_lon: crate::DEFAULT_CENTER_LON,
            default_lat: crate::DEFAULT_CENTER_LAT,
            lang: "ru".into(),
            simplify_tolerance: 2.2,
            grid_color: default_grid_color(),
            drop_folder: String::new(),
            drop_port: default_drop_port(),
            ui_theme: default_ui_theme(),
        }
    }
}
fn config_dir() -> PathBuf {
    let mut p = dirs::config_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));

    p.push("foxy");
    p
}
pub fn path() -> PathBuf {
    config_dir().join("config.json")
}
pub fn load() -> Config {
    let p = path();
    match fs::read_to_string(&p) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
            eprintln!("config.json повреждён ({e}), использую дефолты");
            Config::default()
        }),
        Err(_) => Config::default(),
    }
}
pub fn save(cfg: &Config) -> Result<(), String> {
    let dir = config_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    let s = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    fs::write(path(), s).map_err(|e| format!("write: {e}"))
}
