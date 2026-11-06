//! Чтение и запись треков и точек: GPX и PLT (WPT и ms — следующий этап).
//! Внутри всё хранится в метрах и в UTC (секунды Unix, f64).

pub mod gpx;
pub mod plt;
pub mod text;
pub mod time;
pub mod ms;
pub mod wpt;

pub const FEET: f64 = 0.3048;

#[derive(Debug, thiserror::Error)]
pub enum GeoError {
    #[error("неверный формат файла: {0}")]
    WrongFormat(String),
    #[error("ошибка XML: {0}")]
    Xml(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackPoint {
    pub lat: f64,
    pub lon: f64,
    /// Высота в метрах; None — нет данных.
    pub ele_m: Option<f64>,
    /// Секунды Unix (UTC); None — нет данных.
    pub time: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Waypoint {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub ele_m: Option<f64>,
    pub time: Option<f64>,
    pub description: String,
}

/// Оформление трека в PLT. Меняется перед сохранением.
#[derive(Debug, Clone, PartialEq)]
pub struct PltStyle {
    pub width: u32,
    /// Цвет как целое BGR, как в Ozi (255 = красный).
    pub color: u32,
    pub skip: u32,
    pub kind: u32,
    pub fill_style: u32,
    pub fill_color: u32,
    /// Два последних поля заголовка, смысл не выяснен; хранятся как есть.
    pub tail: String,
}

impl Default for PltStyle {
    fn default() -> Self {
        PltStyle {
            width: 2,
            color: 255,
            skip: 0,
            kind: 0,
            fill_style: 2,
            fill_color: 8_421_376,
            tail: "-1,0".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Track {
    pub name: String,
    pub segments: Vec<Vec<TrackPoint>>,
    pub style: PltStyle,
    /// Название цвета из GPX (Garmin DisplayColor), как есть.
    pub gpx_color: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct GeoData {
    pub waypoints: Vec<Waypoint>,
    pub tracks: Vec<Track>,
    /// Всё, что пропущено или разобрано с оговорками.
    pub warnings: Vec<String>,
}
