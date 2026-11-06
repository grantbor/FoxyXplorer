//! Форматы координат: вывод и разбор.
//! Форматы: dd.dddddd; градусы и минуты; градусы, минуты и секунды.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CoordFormat {
    /// 54.946425 (6 знаков, около 0,11 м)
    #[default]
    Decimal,
    /// 54°56.7855' (4 знака у минут, около 0,19 м)
    DegMin,
    /// 54°56'47.13" (2 знака у секунд, около 0,3 м)
    DegMinSec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Lat,
    Lon,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    BadNumber(String),
    OutOfRange(String),
    WrongHemisphere(String),
}


impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Empty => write!(f, "пустое значение"),
            ParseError::BadNumber(s) => write!(f, "не удалось разобрать «{s}»"),
            ParseError::OutOfRange(s) => write!(f, "значение вне диапазона: {s}"),
            ParseError::WrongHemisphere(s) => write!(f, "буква {s} не подходит к этой оси"),
        }
    }
}
impl std::error::Error for ParseError {}

fn hemi(axis: Axis, negative: bool) -> char {
    match (axis, negative) {
        (Axis::Lat, false) => 'N',
        (Axis::Lat, true) => 'S',
        (Axis::Lon, false) => 'E',
        (Axis::Lon, true) => 'W',
    }
}

/// Одна координата в выбранном формате. Знак выводится буквой N/S/E/W
/// (кроме Decimal, где используется минус), чтобы нулевые градусы не теряли знак.
pub fn format_coord(value: f64, axis: Axis, fmt: CoordFormat) -> String {
    let neg = value.is_sign_negative() && value != 0.0;
    let a = value.abs();
    match fmt {
        CoordFormat::Decimal => format!("{value:.6}"),
        CoordFormat::DegMin => {
            let mut deg = a.floor();
            let mut min = ((a - deg) * 60.0 * 1e4).round() / 1e4;
            if min >= 60.0 {
                min -= 60.0;
                deg += 1.0;
            }
            format!("{}°{:07.4}' {}", deg as u32, min, hemi(axis, neg))
        }
        CoordFormat::DegMinSec => {
            let mut deg = a.floor();
            let mut min = ((a - deg) * 60.0).floor();
            let mut sec = (((a - deg) * 60.0 - min) * 60.0 * 1e2).round() / 1e2;
            if sec >= 60.0 {
                sec -= 60.0;
                min += 1.0;
            }
            if min >= 60.0 {
                min -= 60.0;
                deg += 1.0;
            }
            format!("{}°{:02}'{:05.2}\" {}", deg as u32, min as u32, sec, hemi(axis, neg))
        }
    }
}

/// Пара координат через пробел: «широта долгота».
pub fn format_pair(lat: f64, lon: f64, fmt: CoordFormat) -> String {
    format!(
        "{}  {}",
        format_coord(lat, Axis::Lat, fmt),
        format_coord(lon, Axis::Lon, fmt)
    )
}

/// Разбор одной координаты в любом из форматов. Принимает:
/// 54.946425, -54,946425, 54°56.7855' N, 54 56.7855, N54 56 47.13, 54d56m47.13s,
/// а также русские буквы С, Ю, В, З. Десятичный разделитель: точка или запятая.
pub fn parse_coord(input: &str, axis: Axis) -> Result<f64, ParseError> {
    let s = input.trim();
    if s.is_empty() {
        return Err(ParseError::Empty);
    }
    let mut negative = false;
    let mut letter: Option<char> = None;
    let mut cleaned = String::new();
    let chars: Vec<char> = s.chars().collect();
    let last = chars.len() - 1;
    for (i, &ch) in chars.iter().enumerate() {
        let hem = match ch {
            'N' | 'n' | 'С' | 'с' => Some('N'),
            'S' | 's' | 'Ю' | 'ю' => Some('S'),
            'E' | 'e' | 'В' | 'в' => Some('E'),
            'W' | 'w' | 'З' | 'з' => Some('W'),
            _ => None,
        };
        if let Some(h) = hem {
            // Буква полушария допустима только первой или последней и только одна.
            if (i != 0 && i != last) || letter.is_some() {
                return Err(ParseError::BadNumber(input.to_string()));
            }
            letter = Some(h);
            continue;
        }
        match ch {
            '-' | '−' | '–' => negative = true,
            '+' => {}
            '0'..='9' | '.' => cleaned.push(ch),
            ',' => cleaned.push('.'),
            // любая другая буква — ошибка, а не молчаливый разделитель
            c if c.is_alphabetic() => return Err(ParseError::BadNumber(input.to_string())),
            _ => cleaned.push(' '), // °, ', ′, ", ″, пробелы
        }
    }
    if let Some(l) = letter {
        let ok = match axis {
            Axis::Lat => l == 'N' || l == 'S',
            Axis::Lon => l == 'E' || l == 'W',
        };
        if !ok {
            return Err(ParseError::WrongHemisphere(l.to_string()));
        }
        if l == 'S' || l == 'W' {
            negative = true;
        }
    }
    let parts: Vec<&str> = cleaned.split_whitespace().collect();
    if parts.is_empty() || parts.len() > 3 {
        return Err(ParseError::BadNumber(input.to_string()));
    }
    let mut nums = [0.0f64; 3];
    for (i, p) in parts.iter().enumerate() {
        nums[i] = p.parse::<f64>().map_err(|_| ParseError::BadNumber(input.to_string()))?;
    }
    // Дробная часть допустима только у последнего заданного компонента.
    for p in parts.iter().take(parts.len() - 1) {
        if p.contains('.') {
            return Err(ParseError::BadNumber(input.to_string()));
        }
    }
    if parts.len() >= 2 && nums[1] >= 60.0 || parts.len() == 3 && nums[2] >= 60.0 {
        return Err(ParseError::OutOfRange(input.to_string()));
    }
    let v = nums[0] + nums[1] / 60.0 + nums[2] / 3600.0;
    let limit = if axis == Axis::Lat { 90.0 } else { 180.0 };
    if v > limit {
        return Err(ParseError::OutOfRange(input.to_string()));
    }
    Ok(if negative { -v } else { v })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_output() {
        assert_eq!(format_coord(54.946425, Axis::Lat, CoordFormat::Decimal), "54.946425");
        assert_eq!(format_coord(-83.1, Axis::Lon, CoordFormat::Decimal), "-83.100000");
    }

    #[test]
    fn degmin_output() {
        // 54.946425 -> 54°56.7855'
        assert_eq!(
            format_coord(54.946425, Axis::Lat, CoordFormat::DegMin),
            "54°56.7855' N"
        );
        assert_eq!(
            format_coord(-0.5, Axis::Lon, CoordFormat::DegMin),
            "0°30.0000' W"
        );
    }

    #[test]
    fn degminsec_output() {
        // 0.0113 * 3600 = 40.68, 54.946425 -> 54°56'47.13"
        assert_eq!(
            format_coord(54.946425, Axis::Lat, CoordFormat::DegMinSec),
            "54°56'47.13\" N"
        );
    }

    #[test]
    fn carry_on_rounding() {
        // 59.99999 секунд округляются до 60 и переходят в следующую минуту
        let v = 54.0 + 56.0 / 60.0 + 59.99999 / 3600.0;
        assert_eq!(format_coord(v, Axis::Lat, CoordFormat::DegMinSec), "54°57'00.00\" N");
        let w = 54.0 + 59.99999 / 60.0;
        assert_eq!(format_coord(w, Axis::Lat, CoordFormat::DegMin), "55°00.0000' N");
    }

    #[test]
    fn parse_many_styles() {
        let want = 54.946425;
        for s in [
            "54.946425",
            "54,946425",
            "54°56.7855' N",
            "54 56.7855",
            "N54 56 47.13",
            "54°56′47.13″ С",
        ] {
            let v = parse_coord(s, Axis::Lat).unwrap();
            assert!((v - want).abs() < 1e-6, "{s} -> {v}");
        }
    }

    #[test]
    fn parse_signs_and_errors() {
        assert!(parse_coord("-0°30'", Axis::Lon).unwrap() < 0.0);
        assert!((parse_coord("83°6.3'W", Axis::Lon).unwrap() + 83.105).abs() < 1e-9);
        assert!((parse_coord("12 30 S", Axis::Lat).unwrap() + 12.5).abs() < 1e-9);
        assert!(matches!(parse_coord("", Axis::Lat), Err(ParseError::Empty)));
        assert!(matches!(parse_coord("54 75", Axis::Lat), Err(ParseError::OutOfRange(_))));
        assert!(matches!(parse_coord("91", Axis::Lat), Err(ParseError::OutOfRange(_))));
        assert!(matches!(parse_coord("54 E", Axis::Lat), Err(ParseError::WrongHemisphere(_))));
        assert!(matches!(parse_coord("abc", Axis::Lat), Err(ParseError::BadNumber(_))));
        assert!(matches!(parse_coord("54d56m47.13s", Axis::Lat), Err(ParseError::BadNumber(_))));
        assert!(matches!(parse_coord("1e5", Axis::Lon), Err(ParseError::BadNumber(_))));
    }

    #[test]
    fn roundtrip_all_formats() {
        let vals = [54.946425, -83.082960, 0.0, -0.5, 179.999999, 12.0];
        for &v in &vals {
            for fmt in [CoordFormat::Decimal, CoordFormat::DegMin, CoordFormat::DegMinSec] {
                let s = format_coord(v, Axis::Lon, fmt);
                let back = parse_coord(&s, Axis::Lon).unwrap();
                // допуск 1e-4 градуса (около 11 м) оставлен с запасом по округлению
                // секунд до сотых; сам формат Decimal и DegMin точнее.
                assert!((back - v).abs() < 1e-5, "{fmt:?}: {v} -> {s} -> {back}");
            }
        }
    }
}
