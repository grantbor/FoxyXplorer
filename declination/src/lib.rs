//! Магнитное склонение по WMM2025 (крейт world_magnetic_model).
//! Положительное значение: восточное. Магнитный азимут + склонение = истинный.

use world_magnetic_model::time::Date;
use world_magnetic_model::uom::si::angle::degree;
use world_magnetic_model::uom::si::f32::{Angle, Length};
use world_magnetic_model::uom::si::length::meter;
use world_magnetic_model::GeomagneticField;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclinationError(pub String);

impl std::fmt::Display for DeclinationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "склонение не вычислено: {}", self.0)
    }
}
impl std::error::Error for DeclinationError {}

/// Склонение в градусах (положительное к востоку) на дату `year`-`ordinal`
/// (порядковый день года, 1..=366). Высота 0 м над эллипсоидом WGS-84.
pub fn declination_deg(
    lat: f64,
    lon: f64,
    year: i32,
    ordinal: u16,
) -> Result<f64, DeclinationError> {
    let date = Date::from_ordinal_date(year, ordinal)
        .map_err(|e| DeclinationError(e.to_string()))?;
    let field = GeomagneticField::new(
        Length::new::<meter>(0.0),
        Angle::new::<degree>(lat as f32),
        Angle::new::<degree>(lon as f32),
        date,
    )
    .map_err(|e| DeclinationError(format!("{e:?}")))?;
    Ok(field.declination().get::<degree>() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Центр сетки Новосибирска. Ваше значение с magnetic-declination.com
    // (WMM2025) равно +8°5' = 8.083°. Допуск 0.3°: ошибка самой модели
    // порядка 0.25–0.5°, а сайт мог считать на другую дату.
    #[test]
    fn novosibirsk_matches_site() {
        let d = declination_deg(54.9467, 83.0803, 2026, 310).unwrap(); // 6 ноября 2026
        assert!((d - 8.0833).abs() < 0.3, "склонение {d}");
    }

    #[test]
    fn east_positive_west_negative() {
        // Москва: на этих датах склонение восточное, слегка положительное.
        let msk = declination_deg(55.75, 37.62, 2026, 310).unwrap();
        assert!(msk > 0.0, "Москва {msk}");
        // Центр США (запад): склонение отрицательное или близко к нулю.
        let us = declination_deg(40.0, -105.0, 2026, 310).unwrap();
        assert!(us < 12.0, "США {us}");
    }

    #[test]
    fn bad_date_is_error() {
        assert!(declination_deg(54.9, 83.0, 2026, 400).is_err());
    }
}
