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
}

fn default_text_color() -> [u8; 3] { [0, 0, 0] }
fn default_bg_color() -> [u8; 3] { [0, 255, 255] }
