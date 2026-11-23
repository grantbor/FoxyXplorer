use std::time::{SystemTime, UNIX_EPOCH};
use crate::domain::TrackProps;
use grid::Grid;


/// Components of a valid FoxyXplorer track name, including tracks loaded from files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NamedTrack {
    pub date_str: String,
    pub callsign: String,
    pub num1: u32,
    pub num2: Option<u32>,
    pub suffix: String,
}

/// Recognize YYYYMMDD_CallsignN[_N][_suffix] and ZadachaN_YYYYMMDD.
/// The former YYYYMMDD_Callsign_N form is also accepted for older tracks.
pub(crate) fn parse_named_track(name: &str) -> Option<NamedTrack> {
    fn valid_date(s: &str) -> bool {
        s.len() == 8 && s.bytes().all(|b| b.is_ascii_digit())
    }
    fn display_date(s: &str) -> String {
        format!("{}-{}-{}", &s[..4], &s[4..6], &s[6..8])
    }

    let name = name.trim();
    if let Some(rest) = name.strip_prefix("Zadacha") {
        let (number, date) = rest.split_once('_')?;
        if !valid_date(date) || number.is_empty()
            || !number.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        return Some(NamedTrack {
            date_str: display_date(date),
            callsign: "Zadacha".to_string(),
            num1: number.parse().ok()?,
            num2: None,
            suffix: String::new(),
        });
    }

    let (date, rest) = name.split_once('_')?;
    if !valid_date(date) { return None; }
    let parts: Vec<&str> = rest.split('_').collect();
    let head = *parts.first()?;
    let (callsign, number, extra_start) = if let Some(i) = head.find(|c: char| c.is_ascii_digit()) {
        let (cs, n) = head.split_at(i);
        (cs, n, 1)
    } else {
        (head, *parts.get(1)?, 2)
    };
    if callsign.is_empty() || callsign == "Zadacha"
        || !callsign.chars().all(|c| c.is_alphabetic())
        || number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let num1 = number.parse::<u32>().ok()?;
    let extras = &parts[extra_start..];
    let (num2, suffix) = if let Some(first) = extras.first() {
        if !first.is_empty() && first.bytes().all(|b| b.is_ascii_digit()) {
            (Some(first.parse::<u32>().ok()?), extras[1..].join("_"))
        } else {
            (None, extras.join("_"))
        }
    } else {
        (None, String::new())
    };
    Some(NamedTrack {
        date_str: display_date(date),
        callsign: callsign.to_string(),
        num1,
        num2,
        suffix,
    })
}

pub(crate) fn exclusive_track_color(color: [u8; 3]) -> bool {
    matches!(color,
        [0, 255, 255] | [255, 255, 255] | [255, 0, 255] | [0, 0, 0])
}

pub(crate) fn fixed_track_color(callsign: &str) -> Option<[u8; 3]> {
    match callsign {
        "BPLA" => Some([0, 255, 255]),
        "Bort" => Some([255, 255, 255]),
        "Veter" => Some([255, 0, 255]),
        "Zadacha" => Some([0, 0, 0]),
        _ => None,
    }
}

/// Keep the existing color if the user clicks an exclusive color for
/// an ordinary, correctly named track. Never replace that click with red.
pub(crate) fn requested_track_color(
    description: &str,
    current: [u8; 3],
    requested: [u8; 3],
) -> [u8; 3] {
    let Some(named) = parse_named_track(description) else { return requested; };
    if let Some(fixed) = fixed_track_color(&named.callsign) {
        fixed
    } else if exclusive_track_color(requested) {
        current
    } else {
        requested
    }
}

/// Normalize imported and edited tracks whose names match the template.
/// An already-invalid exclusive color on an ordinary imported track has no
/// valid prior color to restore, so it receives the normal default red.
pub(crate) fn enforce_named_track_style(props: &mut TrackProps) {
    let Some(named) = parse_named_track(&props.description) else { return; };
    props.line_width = if named.callsign == "BPLA" { 1.0 } else { 2.0 };
    if let Some(fixed) = fixed_track_color(&named.callsign) {
        props.line_color = fixed;
    } else if exclusive_track_color(props.line_color) {
        props.line_color = [255, 0, 0];
    }
}

#[cfg(test)]
mod named_track_tests {
    use super::*;

    #[test]
    fn recognizes_external_and_legacy_names() {
        let n = parse_named_track("20261122_Lisa7_3_A").unwrap();
        assert_eq!(n.callsign, "Lisa");
        assert_eq!(n.date_str, "2026-11-22");
        assert_eq!(n.num1, 7);
        assert_eq!(n.num2, Some(3));
        assert_eq!(n.suffix, "A");
        assert_eq!(parse_named_track("20261122_Lisa_7_3_A"), Some(n));
        assert_eq!(parse_named_track("Zadacha9_20261122").unwrap().callsign, "Zadacha");
        assert!(parse_named_track("ordinary imported track").is_none());
        assert!(parse_named_track("20261122_Zadacha9").is_none());
    }

    #[test]
    fn enforces_colors_and_line_widths() {
        for (name, color, width) in [
            ("20261122_BPLA7", [0, 255, 255], 1.0),
            ("20261122_Bort7", [255, 255, 255], 2.0),
            ("20261122_Veter7", [255, 0, 255], 2.0),
            ("Zadacha7_20261122", [0, 0, 0], 2.0),
            ("20261122_Lisa7", [255, 0, 0], 2.0),
        ] {
            let mut props = TrackProps::default();
            props.description = name.into();
            props.line_color = [0, 0, 0];
            props.line_width = 7.0;
            enforce_named_track_style(&mut props);
            assert_eq!(props.line_color, color);
            assert_eq!(props.line_width, width);
        }
    }

    #[test]
    fn forbidden_click_keeps_the_current_color() {
        let name = "20261122_Lisa7";
        let current = [12, 34, 56];
        assert_eq!(requested_track_color(name, current, [0, 255, 255]), current);
        assert_eq!(requested_track_color(name, current, [0, 0, 0]), current);
        assert_eq!(requested_track_color(name, current, [6, 7, 8]), [6, 7, 8]);
        assert_eq!(requested_track_color("20261122_BPLA7", current, [6, 7, 8]), [0, 255, 255]);
    }
}

pub fn today() -> (i32, u16) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let (y, m, d, ..) = geoio::time::parts(secs);
    let ordinal = geoio::time::days_from_civil(y, m, d) - geoio::time::days_from_civil(y, 1, 1) + 1;
    (y as i32, ordinal as u16)
}

pub fn date_offset(days: i64) -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        + days * 86400;
    let (y, m, d, ..) = geoio::time::parts(secs as f64);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

pub fn build_template(d: &TrackProps) -> String {
    if d.callsign == "Zadacha" {
        format!("Zadacha{}_{}", d.num1, d.date_str.replace("-", ""))
    } else {
        let n2 = d.num2.map(|n| format!("_{}", n)).unwrap_or_default();
        let sfx = if d.suffix.is_empty() { String::new() } else { format!("_{}", d.suffix) };
        format!("{}_{}{}{}{}", d.date_str.replace("-", ""), d.callsign, d.num1, n2, sfx)
    }
}

pub fn parse_coords(input: &str, grid: Option<&Grid>, declination: f64) -> Option<(f64, f64)> {
    let trimmed = input.trim();
    if trimmed.is_empty() { return None; }

    if let Some(c) = trimmed.chars().next() {
        if c.is_alphabetic() {
            if let Some(g) = grid {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.is_empty() { return None; }
                
                let node_name = parts[0];
                if let Some(node) = g.nodes.iter().find(|n| n.name == node_name) {
                    if parts.len() == 1 {
                        return Some((node.lat, node.lon));
                    }
                    if parts.len() >= 3 {
                        if let (Ok(az), Ok(dist)) = (parts[1].parse::<f64>(), parts[2].parse::<f64>()) {
                            return Some(grid::locate_observer(node, az, dist, declination));
                        }
                    }
                }
            }
            return None;
        }
    }

    let normalized = trimmed
        .replace(',', ".")
        .replace('°', " ")
        .replace('\'', " ")
        .replace('"', " ")
        .replace('″', " ")
        .replace('′', " ");

    let mut nums = Vec::new();
    for part in normalized.split_whitespace() {
        for sub in part.split(|c| c == ' ' || c == '°' || c == '\'' || c == '"') {
            if sub.is_empty() { continue; }
            if let Ok(v) = sub.parse::<f64>() {
                nums.push(v);
            }
        }
    }

    match nums.len() {
        2 => Some((nums[0], nums[1])),
        4 => {
            let lat = nums[0].signum() * (nums[0].abs() + nums[1] / 60.0);
            let lon = nums[2].signum() * (nums[2].abs() + nums[3] / 60.0);
            Some((lat, lon))
        }
        6 => {
            let lat = nums[0].signum() * (nums[0].abs() + nums[1] / 60.0 + nums[2] / 3600.0);
            let lon = nums[3].signum() * (nums[3].abs() + nums[4] / 60.0 + nums[5] / 3600.0);
            Some((lat, lon))
        }
        _ => None,
    }
}
