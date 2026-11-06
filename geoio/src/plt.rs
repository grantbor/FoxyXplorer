//! OziExplorer Track Point File Version 2.1 (PLT).

use crate::text::{self, Encoding};
use crate::time;
use crate::{GeoData, GeoError, PltStyle, Track, TrackPoint, FEET};

const NO_ALT: f64 = -777.0;

fn parse_point(line: &str) -> Option<(TrackPoint, bool)> {
    let f: Vec<&str> = line.split(',').map(str::trim).collect();
    if f.len() < 5 {
        return None;
    }
    let lat: f64 = f[0].parse().ok()?;
    let lon: f64 = f[1].parse().ok()?;
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    let alt: f64 = f[3].parse().ok()?;
    let ele_m = if (alt - NO_ALT).abs() < 0.5 { None } else { Some(alt * FEET) };
    let ole: f64 = f[4].parse().unwrap_or(0.0);
    let time = if ole > 0.0 { Some(time::ole_to_unix(ole)) } else { None };
    Some((TrackPoint { lat, lon, ele_m, time }, f[2] == "1"))
}

/// Читает один трек. Строковые дата и время игнорируются: они зависят от
/// локали, а числовое поле (дни OLE, UTC) однозначно.
pub fn read(bytes: &[u8]) -> Result<GeoData, GeoError> {
    let text = text::decode(bytes);
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("");
    if !first.trim_start_matches('\u{feff}').starts_with("OziExplorer Track Point File") {
        return Err(GeoError::WrongFormat("нет заголовка «OziExplorer Track Point File»".into()));
    }
    let mut data = GeoData::default();
    let datum = lines.next().unwrap_or("").trim();
    if !datum.eq_ignore_ascii_case("WGS 84") {
        data.warnings.push(format!(
            "датум «{datum}»: поддерживается только WGS 84, координаты не пересчитаны"
        ));
    }
    lines.next(); // «Altitude is in Feet»
    lines.next(); // «Reserved 3»
    let header = lines
        .next()
        .ok_or_else(|| GeoError::WrongFormat("нет строки параметров трека".into()))?;
    let f: Vec<&str> = header.split(',').collect();
    if f.len() < 4 {
        return Err(GeoError::WrongFormat("строка параметров трека слишком короткая".into()));
    }
    let d = PltStyle::default();
    let num = |i: usize, default: u32| -> u32 {
        f.get(i).and_then(|s| s.trim().parse().ok()).unwrap_or(default)
    };
    let style = PltStyle {
        width: num(1, d.width),
        color: num(2, d.color),
        skip: num(4, d.skip),
        kind: num(5, d.kind),
        fill_style: num(6, d.fill_style),
        fill_color: num(7, d.fill_color),
        tail: if f.len() > 8 { f[8..].join(",").trim().to_string() } else { d.tail.clone() },
    };
    let name = f[3].trim().to_string();
    let declared: Option<usize> = lines.next().and_then(|l| l.trim().parse().ok());

    let mut segments: Vec<Vec<TrackPoint>> = Vec::new();
    let mut current: Vec<TrackPoint> = Vec::new();
    let mut total = 0usize;
    for (n, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match parse_point(line) {
            Some((p, new_segment)) => {
                if new_segment && !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
                current.push(p);
                total += 1;
            }
            None => data.warnings.push(format!("строка {}: точка не разобрана", n + 7)),
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }
    if let Some(c) = declared {
        if c != total {
            data.warnings.push(format!("в заголовке {c} точек, прочитано {total}"));
        }
    }
    data.tracks.push(Track { name, segments, style, gpx_color: None });
    Ok(data)
}

/// Записывает один трек. Сегменты склеиваются, первая точка каждого
/// получает флаг 1. Имя и стиль берутся из `track`.
pub fn write(track: &Track, enc: Encoding) -> Vec<u8> {
    let s = &track.style;
    let name: String = track
        .name
        .chars()
        .map(|c| if c == ',' { ';' } else if c.is_control() { ' ' } else { c })
        .collect();
    let count: usize = track.segments.iter().map(Vec::len).sum();
    let mut out = String::new();
    out.push_str("OziExplorer Track Point File Version 2.1\r\nWGS 84\r\nAltitude is in Feet\r\nReserved 3\r\n");
    out.push_str(&format!(
        "0,{},{},{},{},{},{},{},{}\r\n",
        s.width, s.color, name, s.skip, s.kind, s.fill_style, s.fill_color, s.tail
    ));
    out.push_str(&format!("{count}\r\n"));
    for seg in &track.segments {
        for (i, p) in seg.iter().enumerate() {
            let alt = p.ele_m.map_or(NO_ALT, |m| m / FEET);
            let ole = p.time.map_or(0.0, time::unix_to_ole);
            let (y, mo, d, h, mi, sec, _) =
                time::parts(p.time.unwrap_or_else(|| time::ole_to_unix(0.0)));
            out.push_str(&format!(
                "{:>11.6},{:>11.6},{},{:>7.1},{:.7}, {:02}-{:02}-{:04}, {:02}:{:02}:{:02}\r\n",
                p.lat,
                p.lon,
                if i == 0 { 1 } else { 0 },
                alt,
                ole,
                d,
                mo,
                y,
                h,
                mi,
                sec
            ));
        }
    }
    text::encode(&out, enc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ozi_sample() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(
            b"OziExplorer Track Point File Version 2.1\r\nWGS 84\r\nAltitude is in Feet\r\nReserved 3\r\n\
0,2,255,20261024_Lisa4                     ,0,0,2,8421376,-1,0\r\n3\r\n",
        );
        for (flag, alt, ole, clock) in [
            ("1", "  513.0", "46319.2564468", "06:09:17"),
            ("0", "  509.8", "46319.2567361", "06:09:42"),
            ("0", " -777.0", "46319.2571181", "06:10:15"),
        ] {
            b.extend_from_slice(format!("  54.845306,  83.090073,{flag},{alt},{ole}, 24-").as_bytes());
            b.extend_from_slice(&[0xEE, 0xEA, 0xF2]); // «окт» в CP1251
            b.extend_from_slice(format!(".-26, {clock}\r\n").as_bytes());
        }
        b
    }

    #[test]
    fn reads_ozi_style_file() {
        let d = read(&ozi_sample()).unwrap();
        assert!(d.warnings.is_empty(), "{:?}", d.warnings);
        let t = &d.tracks[0];
        assert_eq!(t.name, "20261024_Lisa4");
        assert_eq!(t.style.width, 2);
        assert_eq!(t.style.color, 255);
        assert_eq!(t.segments.len(), 1);
        assert_eq!(t.segments[0].len(), 3);
        let p = &t.segments[0][0];
        assert!((p.ele_m.unwrap() - 513.0 * FEET).abs() < 1e-9);
        assert_eq!(time::format_iso(p.time.unwrap()), "2026-10-24T06:09:17Z");
        assert!(t.segments[0][2].ele_m.is_none());
    }

    #[test]
    fn write_read_roundtrip_cp1251() {
        let mut t = Track::default();
        t.name = "Лиса, трек №4".into();
        t.segments = vec![
            vec![
                TrackPoint {
                    lat: 54.845306,
                    lon: 83.090073,
                    ele_m: Some(156.35),
                    time: time::parse_iso("2026-10-24T06:09:17Z"),
                },
                TrackPoint { lat: 54.845290, lon: 83.090037, ele_m: None, time: None },
            ],
            vec![TrackPoint { lat: 54.8, lon: 83.0, ele_m: Some(100.0), time: None }],
        ];
        let bytes = write(&t, Encoding::Cp1251);
        assert!(bytes.contains(&0xCB), "«Л» должна быть записана байтом 0xCB");
        let back = read(&bytes).unwrap();
        assert!(back.warnings.is_empty(), "{:?}", back.warnings);
        let b = &back.tracks[0];
        assert_eq!(b.name, "Лиса; трек №4");
        assert_eq!(b.segments.len(), 2);
        assert_eq!(b.segments[0].len(), 2);
        let (p, q) = (&b.segments[0][0], &t.segments[0][0]);
        assert!((p.lat - q.lat).abs() < 1e-6 && (p.lon - q.lon).abs() < 1e-6);
        assert!((p.ele_m.unwrap() - 156.35).abs() < 0.05);
        assert!((p.time.unwrap() - q.time.unwrap()).abs() < 0.011);
        assert!(b.segments[0][1].ele_m.is_none() && b.segments[0][1].time.is_none());
    }

    #[test]
    fn rejects_other_files() {
        assert!(read(b"<?xml version=\"1.0\"?>").is_err());
    }
}
