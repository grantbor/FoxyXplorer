use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FileSource {
    File { path: std::path::PathBuf, read_only: bool },
    New,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Waypoint {
    pub id: u64,
    pub name: String,
    pub lon: f64,
    pub lat: f64,
    pub ele: Option<f64>,
    pub time: Option<f64>,
    pub symbol: Option<String>,
    pub description: Option<String>,
    pub visible: bool,
    pub source: FileSource,
    
    #[serde(default = "default_text_color")]
    pub text_color: [u8; 3],
    #[serde(default = "default_bg_color")]
    pub bg_color: [u8; 3],
    /// Код категории из waypoints.csv; WPT: поле 23 (индекс 22).
    #[serde(default)]
    pub category_code: Option<String>,
    /// Азимут в градусах 0..359; WPT: поле 24 (индекс 23), например :35.
    #[serde(default)]
    pub azimuth: Option<u16>,
}

fn default_text_color() -> [u8; 3] { [0, 0, 0] }
fn default_bg_color() -> [u8; 3] { [0, 255, 255] }
