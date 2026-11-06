//! Время: секунды Unix (UTC) <-> ISO 8601 и числовая дата OLE (дни с 1899-12-30).

pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// (год, месяц, день, час, минута, секунда, миллисекунды)
pub fn parts(unix: f64) -> (i64, i64, i64, i64, i64, i64, i64) {
    let total_ms = (unix * 1000.0).round() as i64;
    let secs = total_ms.div_euclid(1000);
    let ms = total_ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    (y, m, d, sod / 3600, sod % 3600 / 60, sod % 60, ms)
}

fn parse_offset(s: &str) -> Option<i64> {
    let sign = match s.chars().next()? {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let digits: String = s[1..].chars().filter(|c| c.is_ascii_digit()).collect();
    let (h, m) = match digits.len() {
        2 => (digits.parse::<i64>().ok()?, 0),
        4 => (digits[..2].parse::<i64>().ok()?, digits[2..].parse::<i64>().ok()?),
        _ => return None,
    };
    Some(sign * (h * 60 + m))
}

/// Терпимый разбор ISO 8601: «2026-10-24T06:09:17Z», «2023-07-09T6:15:47Z»
/// (час без нуля, как у Ozi), доли секунды, смещение ±HH:MM.
pub fn parse_iso(s: &str) -> Option<f64> {
    let s = s.trim();
    let (date, rest) = s.split_once(|c: char| c == 'T' || c == 't' || c == ' ')?;
    let mut dp = date.split('-');
    let y: i64 = dp.next()?.parse().ok()?;
    let m: i64 = dp.next()?.parse().ok()?;
    let d: i64 = dp.next()?.parse().ok()?;
    if dp.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let (clock, offset_min) = if let Some(c) = rest.strip_suffix(|c: char| c == 'Z' || c == 'z') {
        (c, 0)
    } else if let Some(pos) = rest.rfind(|c: char| c == '+' || c == '-') {
        (&rest[..pos], parse_offset(&rest[pos..])?)
    } else {
        (rest, 0)
    };
    let mut cp = clock.split(':');
    let h: i64 = cp.next()?.trim().parse().ok()?;
    let mi: i64 = cp.next()?.trim().parse().ok()?;
    let sec: f64 = match cp.next() {
        Some(x) => x.trim().parse().ok()?,
        None => 0.0,
    };
    if cp.next().is_some()
        || !(0..24).contains(&h)
        || !(0..60).contains(&mi)
        || !(0.0..61.0).contains(&sec)
    {
        return None;
    }
    let days = days_from_civil(y, m, d);
    Some((days * 86_400 + h * 3600 + mi * 60 - offset_min * 60) as f64 + sec)
}

pub fn format_iso(unix: f64) -> String {
    let (y, m, d, h, mi, s, ms) = parts(unix);
    if ms == 0 {
        format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
    } else {
        format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{ms:03}Z")
    }
}

pub fn unix_to_ole(unix: f64) -> f64 {
    unix / 86_400.0 + 25_569.0
}

/// Ozi хранит дни с 7 знаками (около 8,6 мс), поэтому округляем до 10 мс,
/// чтобы не плодить артефакты вида «17.004 с».
pub fn ole_to_unix(ole: f64) -> f64 {
    ((ole - 25_569.0) * 86_400.0 * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2026, 10, 24), 20_750);
        assert_eq!(civil_from_days(20_750), (2026, 10, 24));
        assert_eq!(civil_from_days(-25_569), (1899, 12, 30));
    }

    #[test]
    fn ozi_serial_matches_real_file() {
        let u = parse_iso("2026-10-24T06:09:17Z").unwrap();
        assert_eq!(format!("{:.7}", unix_to_ole(u)), "46319.2564468");
        assert_eq!(format_iso(u), "2026-10-24T06:09:17Z");
        assert_eq!(format_iso(ole_to_unix(46319.2564468)), "2026-10-24T06:09:17Z");
    }

    #[test]
    fn lenient_parsing() {
        assert!(parse_iso("2023-07-09T6:15:47Z").is_some());
        let a = parse_iso("2026-10-24T13:09:17+07:00").unwrap();
        assert_eq!(a, parse_iso("2026-10-24T06:09:17Z").unwrap());
        assert!(parse_iso("мусор").is_none());
        assert!(parse_iso("2026-13-01T00:00:00Z").is_none());
    }
}
