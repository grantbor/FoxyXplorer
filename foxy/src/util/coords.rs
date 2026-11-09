use std::time::{SystemTime, UNIX_EPOCH};
use crate::domain::TrackProps;
use grid::Grid;

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
        format!("{}_{}_{}{}{}", d.date_str.replace("-", ""), d.callsign, d.num1, n2, sfx)
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
