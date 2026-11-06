//! OziExplorer Waypoint File Version 1.1 (WPT).
//! Символ, цвета и шрифт при чтении не сохраняются, при записи берутся
//! значения по умолчанию из файлов сеток Liza Alert.

use crate::text::{self, Encoding};
use crate::time;
use crate::{GeoData, GeoError, Waypoint, FEET};

const NO_ALT: f64 = -777.0;

fn parse_line(line: &str) -> Option<Waypoint> {
    let f: Vec<&str> = line.split(',').map(str::trim).collect();
    if f.len() < 5 {
        return None;
    }
    let lat: f64 = f[2].parse().ok()?;
    let lon: f64 = f[3].parse().ok()?;
    if !(lat.abs() <= 90.0 && lon.abs() <= 180.0) {
        return None;
    }
    let ole: f64 = f[4].parse().unwrap_or(0.0);
    let ele_m = f
        .get(14)
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|a| a.is_finite() && (*a - NO_ALT).abs() >= 0.5)
        .map(|a| a * FEET);
    Some(Waypoint {
        name: f[1].to_string(),
        lat,
        lon,
        ele_m,
        time: if ole > 1.0 { Some(time::ole_to_unix(ole)) } else { None },
        description: f.get(10).map(|s| (*s).to_string()).unwrap_or_default(),
    })
}

pub fn read(bytes: &[u8]) -> Result<GeoData, GeoError> {
    let text = text::decode(bytes);
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("");
    if !first.starts_with("OziExplorer Waypoint File") {
        return Err(GeoError::WrongFormat("нет заголовка «OziExplorer Waypoint File»".into()));
    }
    let mut data = GeoData::default();
    let datum = lines.next().unwrap_or("").trim();
    if !datum.eq_ignore_ascii_case("WGS 84") {
        data.warnings
            .push(format!("датум «{datum}»: поддерживается только WGS 84, координаты не пересчитаны"));
    }
    lines.next(); // Reserved 2
    lines.next(); // garmin
    for (n, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match parse_line(line) {
            Some(w) => data.waypoints.push(w),
            None => data.warnings.push(format!("строка {}: точка не разобрана", n + 5)),
        }
    }
    Ok(data)
}

fn clean(s: &str) -> String {
    s.chars()
        .map(|c| if c == ',' { ';' } else if c.is_control() { ' ' } else { c })
        .collect()
}

/// Запятые в имени и описании заменяются на «;», потому что запятая разделяет поля.
pub fn write(wps: &[Waypoint], enc: Encoding) -> Vec<u8> {
    let mut out = String::from("OziExplorer Waypoint File Version 1.1\r\nWGS 84\r\nReserved 2\r\ngarmin\r\n");
    for (i, w) in wps.iter().enumerate() {
        let alt = match w.ele_m {
            Some(m) => format!("{:.1}", m / FEET),
            None => "-777".to_string(),
        };
        let ole = w.time.map_or(0.0, time::unix_to_ole);
        out.push_str(&format!(
            "{},{},{:>11.6},{:>10.6},{:.10}, 0, 1, 3,{:>10},{:>10},{}, 0, 0,0,{:>7}, 6, 0,17,0,10.0,2,,,,\r\n",
            i + 1,
            clean(&w.name),
            w.lat,
            w.lon,
            ole,
            0,
            65535,
            clean(&w.description),
            alt
        ));
    }
    text::encode(&out, enc)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "OziExplorer Waypoint File Version 1.1\r\nWGS 84\r\nReserved 2\r\ngarmin\r\n\
1,A1,  54.970978, 83.058272,45190.4801043519, 0, 1, 3,         0,     65535,, 0, 0,0,   -777, 6, 0,17,0,10.0,2,,,,\r\n\
4,D1,  54.970779, 83.06998,45190.4801045718, 0, 1, 3,         0,     65535,, 0, 0,0,   -777, 6, 0,17,0,10.0,2,,,,\r\n";

    #[test]
    fn reads_ozi_sample() {
        let d = read(SAMPLE.as_bytes()).unwrap();
        assert!(d.warnings.is_empty(), "{:?}", d.warnings);
        assert_eq!(d.waypoints.len(), 2);
        let a = &d.waypoints[0];
        assert_eq!(a.name, "A1");
        assert!((a.lat - 54.970978).abs() < 1e-9 && (a.lon - 83.058272).abs() < 1e-9);
        assert!(a.ele_m.is_none());
        let want = time::parse_iso("2023-09-21T11:31:21Z").unwrap();
        // дробная часть секунды в файле Ozi — артефакт, допускаем секунду
        assert!((a.time.unwrap() - want).abs() < 1.0);
        assert_eq!(d.waypoints[1].name, "D1");
    }

    #[test]
    fn written_line_has_same_layout_as_ozi() {
        let w = Waypoint {
            name: "A1".into(),
            lat: 54.970978,
            lon: 83.058272,
            ele_m: None,
            time: time::parse_iso("2023-09-21T11:31:21Z"),
            description: String::new(),
        };
        let s = String::from_utf8(write(&[w], Encoding::Utf8)).unwrap();
        let line = s.lines().nth(4).unwrap();
        let ozi = SAMPLE.lines().nth(4).unwrap();
        // всё, кроме поля даты (5-е), совпадает с Ozi посимвольно
        let (a, b): (Vec<&str>, Vec<&str>) = (line.split(',').collect(), ozi.split(',').collect());
        assert_eq!(a.len(), b.len());
        for i in (0..a.len()).filter(|i| *i != 4) {
            assert_eq!(a[i], b[i], "поле {}", i + 1);
        }
    }

    #[test]
    fn roundtrip_cp1251_and_commas() {
        let w = Waypoint {
            name: "Лагерь, №1".into(),
            lat: -54.123456,
            lon: -83.654321,
            ele_m: Some(156.35),
            time: Some(1_792_822_157.0),
            description: "у ручья, слева".into(),
        };
        let bytes = write(&[w], Encoding::Cp1251);
        assert!(std::str::from_utf8(&bytes).is_err());
        let d = read(&bytes).unwrap();
        assert!(d.warnings.is_empty(), "{:?}", d.warnings);
        let b = &d.waypoints[0];
        assert_eq!(b.name, "Лагерь; №1");
        assert_eq!(b.description, "у ручья; слева");
        assert!((b.lat + 54.123456).abs() < 1e-7 && (b.lon + 83.654321).abs() < 1e-7);
        assert!((b.ele_m.unwrap() - 156.35).abs() < 0.05);
        assert!((b.time.unwrap() - 1_792_822_157.0).abs() < 0.02);
    }

    #[test]
    fn rejects_other_files() {
        assert!(read(b"<?xml version=\"1.0\"?>").is_err());
    }

    #[test]
    fn bad_line_becomes_warning() {
        let text = format!("{SAMPLE}мусор\r\n");
        let d = read(text.as_bytes()).unwrap();
        assert_eq!(d.waypoints.len(), 2);
        assert_eq!(d.warnings.len(), 1);
    }
}
