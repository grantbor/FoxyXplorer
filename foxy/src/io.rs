#![allow(dead_code)]
use std::path::Path;
use crate::domain::{
    Track, TrackPoint, TrackSegment, Waypoint, FileSource, TrackProps,
    PolyType, FillType, LineStyle, recompute_track_stats,
};
use crate::t;


pub struct LoadedContent {
    pub tracks: Vec<Track>,
    pub waypoints: Vec<Waypoint>,
    pub notes: Vec<String>,
}



fn next_color(idx: usize) -> [u8; 3] {
    const PALETTE: [[u8; 3]; 8] = [
        [220, 60, 60],   // red
        [60, 120, 220],  // blue
        [60, 180, 80],   // green
        [220, 160, 40],  // orange
        [160, 60, 200],  // purple
        [40, 180, 180],  // teal
        [220, 100, 160], // pink
        [130, 130, 130], // gray
    ];
    PALETTE[idx % PALETTE.len()]
}

fn parse_gpx_color(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().to_lowercase();
    match s.as_str() {
        "red" => Some([255, 0, 0]),
        "green" => Some([0, 255, 0]),
        "blue" => Some([0, 0, 255]),
        "magenta" => Some([255, 0, 255]),
        "cyan" => Some([0, 255, 255]),
        "yellow" => Some([255, 255, 0]),
        "black" => Some([0, 0, 0]),
        "white" => Some([255, 255, 255]),
        _ => {
            if s.starts_with('#') && s.len() == 7 {
                let r = u8::from_str_radix(&s[1..3], 16).ok()?;
                let g = u8::from_str_radix(&s[3..5], 16).ok()?;
                let b = u8::from_str_radix(&s[5..7], 16).ok()?;
                Some([r, g, b])
            } else {
                None
            }
        }
    }
}

fn detect_callsign(name: &str) -> Option<String> {
    let upper = name.to_uppercase();
    let table = [
        ("Lisa",    t!("msg_0001")),
        ("Veter",   t!("msg_0002")),
        ("Kinolog", t!("msg_0003")),
        ("BPLA",    t!("msg_0004")),
        ("Bort",    t!("msg_0005")),
        ("Zadacha", t!("msg_0006")),
        ("Avtonom", t!("msg_0007")),
        ("Pegas",   t!("msg_0008")),
    ];
    for (canonical, cyrillic) in table {
        if upper.contains(&canonical.to_uppercase()) || upper.contains(&cyrillic) {
            return Some(canonical.to_string());
        }
    }
    None
}

/// Ozi BGR -> RGB
fn ozi_to_rgb(c: u32) -> [u8; 3] {
    [(c & 0xFF) as u8, ((c >> 8) & 0xFF) as u8, ((c >> 16) & 0xFF) as u8]
}

/// RGB -> Ozi BGR
fn rgb_to_ozi(c: [u8; 3]) -> u32 {
    (c[0] as u32) | ((c[1] as u32) << 8) | ((c[2] as u32) << 16)
}



// Ozi WPT field 5 is a UTC instant. Field 25 stores the offset (minutes)
// of the RECORDING LOCATION, not the local timezone of this computer.
// For GPX with Z timestamps, coordinates are used to infer the IANA region.
// Explicit field-25 offsets from imported WPTs are retained unchanged.

pub fn load_user_file(path: &Path, next_id: &mut u64) -> Result<LoadedContent, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let ext = path.extension().and_then(|s| s.to_str()).map(|s| s.to_lowercase()).unwrap_or_default();

    
    let (is_read_only, data) = match ext.as_str() {
        "gpx" => (true, geoio::gpx::read(&bytes)),
        "plt" => (false, geoio::plt::read(&bytes)),
        "ms" => (false, geoio::ms::read(&bytes)),
        "wpt" => (false, geoio::wpt::read(&bytes)),
        "kml" | "klm" => (false, geoio::kml::read(&bytes)),
        "fit" => (true, geoio::fit::read(&bytes)),
        _ => return Err(crate::tf!("msg_0402", path = path.display())),
    };
    
    let gd = data.map_err(|e| format!("{}: {e}", path.display()))?;
    
    let mut tracks = Vec::new();
    for (i, t) in gd.tracks.into_iter().enumerate() {
        
        let mut segments = Vec::new();
        for seg in t.segments {
            let points = seg.into_iter().map(|p| TrackPoint {
                lon: p.lon,
                lat: p.lat,
                ele: p.ele_m,
                time: p.time,
                hdop: p.hdop,
                speed_m_s: p.speed_m_s,
            }).collect();
            segments.push(TrackSegment { points });
        }
        
        let line_style = match t.style.kind {
            0 => LineStyle::Solid,
            1 => LineStyle::Dash,
            2 => LineStyle::Dot,
            3 => LineStyle::DashDot,
            4 => LineStyle::DashDotDot,
            _ => LineStyle::Solid,
        };

        let fill_type = match t.style.fill_style {
            0 => FillType::Solid,
            1 => FillType::None,
            2 => FillType::Diagonal1,
            3 => FillType::Diagonal2,
            4 => FillType::CrossHatch,
            5 => FillType::DiagCross,
            6 => FillType::Horizontal,
            7 => FillType::Vertical,
            _ => FillType::None,
        };

        let poly_type = match t.style.poly_flag {
            10 | 20 => PolyType::Polygon,
            _ => PolyType::Line,
        };
        
        let fill_color = ozi_to_rgb(t.style.fill_color);
        
        let name = if t.name.is_empty() {
            format!("{}: trk#{}", path.file_stem().unwrap_or_default().to_string_lossy(), i + 1)
        } else {
            t.name
        };
        
        let callsign = detect_callsign(&name).unwrap_or_default();
        
        let color_from_callsign: Option<[u8; 3]> = match callsign.as_str() {
            "Veter"   => Some([255, 0, 255]),
            "BPLA"    => Some([0, 255, 255]),
            "Bort"    => Some([255, 255, 255]),
            "Zadacha" => Some([0, 0, 0]),
            _         => None,
        };

        let color = if let Some(c) = color_from_callsign {
            c
        } else if let Some(c) = t.gpx_color.as_deref().and_then(parse_gpx_color) {
            c
        } else if t.style.color != 0 {
            ozi_to_rgb(t.style.color)
        } else {
            next_color(i)
        };
        
        let mut track = Track {
            id: *next_id,
            name: name.clone(), 
            color,
            visible: true,
            segments,
            source: FileSource::File { path: path.to_path_buf(), read_only: is_read_only },
            cached_points: 0,
            cached_distance_m: 0.0,
            props: TrackProps {
                description: name,
                callsign, 
                line_color: color,
                line_style,
                fill_type,
                fill_color,
                line_width: t.style.width as f32,
                poly_type, 
                ..Default::default()
            },
        };
        // Enforce naming-template rules even when the track was never renamed here.
        crate::util::coords::enforce_named_track_style(&mut track.props);
        track.color = track.props.line_color;
        recompute_track_stats(&mut track);
        tracks.push(track);
        *next_id += 1;
    }
    
    // geoio читает стандартные WPT-поля. Специальные поля 22/23 (category/:azimuth)
    // восстанавливаем отдельно из исходных байтов, не декодируя CP1251 как UTF-8.
    let wpt_extras = if ext == "wpt" { read_wpt_extras(&bytes) } else { Vec::new() };
    let mut waypoints = Vec::new();
    for (i, w) in gd.waypoints.into_iter().enumerate() {
        let name = if w.name.is_empty() {
            format!("{}: wp#{}", path.file_stem().unwrap_or_default().to_string_lossy(), i + 1)
        } else {
            w.name
        };
        
        // Region is inferred at the instant of recording, including historical DST.
        // A WPT-supplied field 25 takes precedence over inferred offsets to
        // preserve existing Ozi records across Save and Append.
        let inferred = w.time.and_then(|utc| crate::util::timezones::at(w.lon, w.lat, utc));
        let stored_offset = if ext == "wpt" {
            wpt_extras.get(i).and_then(|(_, _, offset)| *offset)
        } else { None };
        waypoints.push(Waypoint {
            id: *next_id,
            name,
            lon: w.lon,
            lat: w.lat,
            ele: w.ele_m,
            // geoio reads the numeric WPT timestamp as the original instant.
            // Ozi's timezone offset in field 25 is for display, not a time shift.
            time: w.time,
            symbol: None,
            description: if w.description.is_empty() { None } else { Some(w.description) },
            visible: true,
            source: FileSource::File { path: path.to_path_buf(), read_only: is_read_only },
            text_color: w.text_color, // <-- из geoio
            bg_color: w.bg_color,     // <-- из geoio
            category_code: wpt_extras.get(i).and_then(|(category, _, _)| category.clone()),
            azimuth: wpt_extras.get(i).and_then(|(_, azimuth, _)| *azimuth),
            timezone_id: inferred.as_ref().map(|(name, _)| name.clone()),
            utc_offset_minutes: stored_offset.or_else(|| inferred.map(|(_, offset)| offset)),
        });
        *next_id += 1;
    }
    
    Ok(LoadedContent {
        tracks,
        waypoints,
        notes: gd.warnings,
    })
}

pub fn save_gpx(tracks: &[&Track], waypoints: &[&Waypoint], path: &Path) -> Result<(), String> {
    let mut gd = geoio::GeoData::default();
    
    for t in tracks {
        let segments = t.segments.iter().map(|seg| {
            seg.points.iter().map(|p| geoio::TrackPoint {
                lat: p.lat,
                lon: p.lon,
                ele_m: p.ele,
                time: p.time,
                hdop: p.hdop,
                speed_m_s: p.speed_m_s,
            }).collect()
        }).collect();
        
        let color_str = if t.color != [0,0,0] {
            Some(format!("#{:02X}{:02X}{:02X}", t.color[0], t.color[1], t.color[2]))
        } else { None };
        
        gd.tracks.push(geoio::Track {
            name: t.name.clone(),
            segments,
            gpx_color: color_str,
            ..Default::default()
        });
    }
    
    for w in waypoints {
        gd.waypoints.push(geoio::Waypoint {
            name: w.name.clone(),
            lon: w.lon,
            lat: w.lat,
            ele_m: w.ele,
            time: w.time,
            description: w.description.clone().unwrap_or_default(),
            text_color: w.text_color,
            bg_color: w.bg_color,
        });
    }
    
    let xml = geoio::gpx::write(&gd);
    std::fs::write(path, xml).map_err(|e| format!("{}: {e}", path.display()))
}

fn ozi_hack(s: &str) -> String {
    s.replace('С', "C").replace('с', "c")
}

pub fn save_plt(track: &Track, path: &Path) -> Result<(), String> {
    let segments = track.segments.iter().map(|seg| {
        seg.points.iter().map(|p| geoio::TrackPoint {
            lat: p.lat,
            lon: p.lon,
            ele_m: p.ele,
            time: p.time,
            hdop: p.hdop,
            speed_m_s: p.speed_m_s,
        }).collect()
    }).collect();

    let mut geo_track = geoio::Track {
        name: ozi_hack(&track.props.description),
        segments,
        ..Default::default()
    };

    geo_track.style.kind = match track.props.line_style {
        LineStyle::Solid      => 0,
        LineStyle::Dash       => 1,
        LineStyle::Dot        => 2,
        LineStyle::DashDot    => 3,
        LineStyle::DashDotDot => 4,
    };
    
    geo_track.style.poly_flag = if track.props.poly_type == PolyType::Polygon {
        10
    } else {
        0
    };

    if track.props.poly_type == PolyType::Polygon {
        geo_track.style.fill_style = match track.props.fill_type {
            FillType::Solid       => 0,
            FillType::None        => 1,
            FillType::Diagonal1   => 2,
            FillType::Diagonal2   => 3,
            FillType::CrossHatch  => 4,
            FillType::DiagCross   => 5,
            FillType::Horizontal  => 6,
            FillType::Vertical    => 7,
        };
    } else {
        geo_track.style.fill_style = 1;
    }
    geo_track.style.fill_color = rgb_to_ozi(track.props.fill_color);
    geo_track.style.color = rgb_to_ozi(track.props.line_color);
    geo_track.style.width = track.props.line_width as u32;

    let bytes = geoio::plt::write(&geo_track, geoio::text::Encoding::Cp1251);
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn save_kml(tracks: &[&Track], waypoints: &[&Waypoint], path: &Path) -> Result<(), String> {
    let mut gd = geoio::GeoData::default();

    for t in tracks {
        let segments = t.segments.iter().map(|seg| {
            seg.points.iter().map(|p| geoio::TrackPoint {
                lat: p.lat,
                lon: p.lon,
                ele_m: p.ele,
                time: p.time,
                hdop: p.hdop,
                speed_m_s: p.speed_m_s,
            }).collect()
        }).collect();

        let color_str = if t.color != [0, 0, 0] {
            Some(format!("#{:02X}{:02X}{:02X}", t.color[0], t.color[1], t.color[2]))
        } else {
            None
        };

        gd.tracks.push(geoio::Track {
            name: t.name.clone(),
            segments,
            gpx_color: color_str,
            ..Default::default()
        });
    }

    for w in waypoints {
        gd.waypoints.push(geoio::Waypoint {
            name: w.name.clone(),
            lon: w.lon,
            lat: w.lat,
            ele_m: w.ele,
            time: w.time,
            description: w.description.clone().unwrap_or_default(),
            text_color: w.text_color,
            bg_color: w.bg_color,
        });
    }

    let xml = geoio::kml::write(&gd);
    std::fs::write(path, xml).map_err(|e| format!("{}: {e}", path.display()))
}


// Дополнительные поля WPT (индексы начинаются с нуля):
// 22 — category code; 23 — :azimuth; 24 — UTC offset in minutes.
// Field 24 is metadata for display and MUST NOT shift timestamp in field 4.
fn read_wpt_extras(bytes: &[u8]) -> Vec<(Option<String>, Option<u16>, Option<i32>)> {
    bytes.split(|b| *b == b'\n')
        .skip(4) // стандартный заголовок OziExplorer
        .filter_map(|line| {
            let fields: Vec<&[u8]> = line.split(|b| *b == b',').collect();
            let number = std::str::from_utf8(fields.first()?).ok()?.trim();
            if number.parse::<usize>().is_err() { return None; }
            let ascii = |index: usize| fields.get(index)
                .and_then(|s| std::str::from_utf8(s).ok())
                .map(str::trim);
            let category = ascii(22).filter(|s| !s.is_empty()).map(str::to_string);
            let azimuth = ascii(23)
                .and_then(|s| s.strip_prefix(':'))
                .and_then(|s| s.parse::<u16>().ok())
                .filter(|v| *v <= 359);
            let offset = ascii(24)
                .and_then(|s| s.parse::<i32>().ok())
                .filter(|minutes| (-840..=840).contains(minutes));
            Some((category, azimuth, offset))
        })
        .collect()
}

/// Дополняем результат стандартного CP1251-экспорта geoio без перекодирования.
/// Не переписываем базовые поля, чтобы не нарушать сохранение WPT и Append.
fn add_wpt_extras(bytes: Vec<u8>, waypoints: &[&Waypoint]) -> Result<Vec<u8>, String> {
    // Offset in field 25 must be written even when there is no category/azimuth.
    if waypoints.is_empty() { return Ok(bytes); }

    let mut output = Vec::with_capacity(bytes.len() + waypoints.len() * 20);
    let mut count = 0usize;
    for (line_index, raw_line) in bytes.split_inclusive(|b| *b == b'\n').enumerate() {
        if line_index < 4 {
            output.extend_from_slice(raw_line);
            continue;
        }
        let (content, ending): (&[u8], &[u8]) = if let Some(s) = raw_line.strip_suffix(b"\r\n") {
            (s, b"\r\n")
        } else if let Some(s) = raw_line.strip_suffix(b"\n") {
            (s, b"\n")
        } else {
            (raw_line, b"")
        };

        let mut fields: Vec<Vec<u8>> = content.split(|b| *b == b',').map(|s| s.to_vec()).collect();
        let has_record_number = fields.first().and_then(|s| std::str::from_utf8(s).ok())
            .and_then(|s| s.trim().parse::<usize>().ok()).is_some();
        if !has_record_number {
            output.extend_from_slice(raw_line);
            continue;
        }
        let waypoint = waypoints.get(count).ok_or(crate::t!("msg_0403"))?;
        // Standard WPT field 25 holds the timezone offset in minutes.
        // If geoio omitted category/azimuth columns, insert them before field 25.
        if fields.len() < 25 {
            let final_field = fields.pop().unwrap_or_default();
            fields.resize(24, Vec::new());
            fields.push(final_field);
        }
        let category = waypoint.category_code.as_deref().unwrap_or("");
        if !category.is_ascii() || category.contains(',') || category.contains('\r') || category.contains('\n') {
            return Err(crate::tf!("msg_0404", category = category));
        }
        fields[22] = category.as_bytes().to_vec();
        fields[23] = waypoint.azimuth.map(|v| format!(":{v}").into_bytes()).unwrap_or_default();
        // Keep field 5 (UTC datetime) intact. Field 25 is *recording* time zone.
        // Prefer imported metadata (especially when appending to an existing WPT).
        if let Some(utc) = waypoint.time.filter(|t| t.is_finite()) {
            let offset = waypoint.utc_offset_minutes
                .or_else(|| waypoint.timezone_id.as_deref()
                    .and_then(|name| crate::util::timezones::offset_for_zone(name, utc)))
                .or_else(|| crate::util::timezones::at(waypoint.lon, waypoint.lat, utc)
                    .map(|(_, minutes)| minutes));
            // Never substitute this computer's local time zone on lookup failure.
            fields[24] = offset.map(|minutes| minutes.to_string().into_bytes())
                .unwrap_or_default();
        }
        for (index, field) in fields.iter().enumerate() {
            if index > 0 { output.push(b','); }
            output.extend_from_slice(field);
        }
        output.extend_from_slice(ending);
        count += 1;
    }
    if count != waypoints.len() {
        return Err(crate::tf!("msg_0405", expected = waypoints.len(), actual = count));
    }
    Ok(output)
}

#[cfg(test)]
mod wpt_timezone_tests {
    use super::*;

    fn sample_waypoint(time: Option<f64>, category_code: Option<&str>) -> Waypoint {
        Waypoint {
            id: 1, name: "Test".to_string(), lon: 39.0, lat: 51.0,
            ele: None, time, symbol: None, description: None,
            visible: true, source: FileSource::New,
            text_color: [0, 0, 0], bg_color: [0, 255, 255],
            category_code: category_code.map(str::to_owned), azimuth: None,
            timezone_id: None, utc_offset_minutes: None,
        }
    }

    fn data_fields(bytes: &[u8]) -> Vec<&[u8]> {
        let record = bytes.split(|&b| b == b'\n')
            .skip(4)
            .find(|line| line.first().map_or(false, |c| c.is_ascii_digit()))
            .expect("WPT data row");
        record.split(|&b| b == b',').collect()
    }

    #[test]
    fn timestamp_unchanged_and_offset_written_without_category() {
        let timestamp = 1_764_000_000.625;
        let w = sample_waypoint(Some(timestamp), None);
        let raw = geoio::wpt::write(&[geoio::Waypoint {
            name: w.name.clone(), lon: w.lon, lat: w.lat,
            ele_m: w.ele, time: w.time, description: String::new(),
            text_color: w.text_color, bg_color: w.bg_color,
        }], geoio::text::Encoding::Cp1251);
        let result = add_wpt_extras(raw.clone(), &[&w]).unwrap();
        let before = data_fields(&raw);
        let after = data_fields(&result);
        assert_eq!(before[4], after[4], "WPT field 5 must not shift");
        assert_eq!(after.len(), 25);
        let expected = crate::util::timezones::at(w.lon, w.lat, timestamp).unwrap().1;
        assert_eq!(std::str::from_utf8(after[24]).unwrap().trim(), expected.to_string());
        let parsed = geoio::wpt::read(&result).unwrap();
        let got = parsed.waypoints[0].time.unwrap();
        assert!((got - timestamp).abs() < 0.02, "WPT round-trip changed UTC time");
    }

    #[test]
    fn category_and_azimuth_do_not_displace_offset_field() {
        let mut w = sample_waypoint(Some(1_764_000_000.0), Some("cellar"));
        w.azimuth = Some(35);
        let raw = geoio::wpt::write(&[geoio::Waypoint {
            name: w.name.clone(), lon: w.lon, lat: w.lat,
            ele_m: w.ele, time: w.time, description: String::new(),
            text_color: w.text_color, bg_color: w.bg_color,
        }], geoio::text::Encoding::Cp1251);
        let result = add_wpt_extras(raw, &[&w]).unwrap();
        let fields = data_fields(&result);
        assert_eq!(fields[22], b"cellar");
        assert_eq!(fields[23], b":35");
        let expected = crate::util::timezones::at(w.lon, w.lat, w.time.unwrap()).unwrap().1;
        assert_eq!(std::str::from_utf8(fields[24]).unwrap().trim(), expected.to_string());
    }

    #[test]
    fn imported_wpt_offset_is_preserved_over_geographic_guess() {
        let mut w = sample_waypoint(Some(1_764_000_000.0), Some("cellar"));
        w.azimuth = Some(35);
        // A pre-existing WPT says UTC+5, even though coordinates lie in UTC+7.
        w.lon = 82.922;
        w.lat = 55.507;
        w.utc_offset_minutes = Some(300);
        let raw = geoio::wpt::write(&[geoio::Waypoint {
            name: w.name.clone(), lon: w.lon, lat: w.lat,
            ele_m: w.ele, time: w.time, description: String::new(),
            text_color: w.text_color, bg_color: w.bg_color,
        }], geoio::text::Encoding::Cp1251);
        let result = add_wpt_extras(raw, &[&w]).unwrap();
        let fields = data_fields(&result);
        assert_eq!(std::str::from_utf8(fields[24]).unwrap().trim(), "300");
        assert_eq!(read_wpt_extras(&result)[0].2, Some(300));
    }

    #[test]
    fn no_timestamp_keeps_empty_offset() {
        let w = sample_waypoint(None, None);
        let raw = geoio::wpt::write(&[geoio::Waypoint {
            name: w.name.clone(), lon: w.lon, lat: w.lat,
            ele_m: w.ele, time: None, description: String::new(),
            text_color: w.text_color, bg_color: w.bg_color,
        }], geoio::text::Encoding::Cp1251);
        let result = add_wpt_extras(raw.clone(), &[&w]).unwrap();
        assert_eq!(data_fields(&raw)[4], data_fields(&result)[4]);
    }
}

#[cfg(test)]
mod wpt_extra_tests {
    use super::*;

    #[test]
    fn sample_has_category_and_azimuth() {
        let sample = b"OziExplorer Waypoint File Version 1.1\r\nWGS 84\r\nReserved 2\r\ngarmin\r\n1,Test,  51.669144,  39.099740,46349.7377440,  0, 0, 3,         0,     65535,, 0, 0,    0,   -777, 6, 0,17,0,10.0,2,,cellar,:35,420\r\n";
        assert_eq!(read_wpt_extras(sample), vec![(Some("cellar".to_string()), Some(35), Some(420))]);
    }

    #[test]
    fn save_and_append_metadata_roundtrip() {
        let ordinary = Waypoint {
            id: 1, name: "Обычная".to_string(), lon: 39.0, lat: 51.0,
            ele: None, time: None, symbol: None, description: None,
            visible: true, source: FileSource::New,
            text_color: [0, 0, 0], bg_color: [0, 255, 255],
            category_code: None, azimuth: None,
            timezone_id: None, utc_offset_minutes: None,
        };
        let annotated = Waypoint {
            id: 2, name: "Тест".to_string(), lon: 39.1, lat: 51.1,
            category_code: Some("cellar".to_string()), azimuth: Some(35),
            ..ordinary.clone()
        };
        let refs = [&ordinary, &annotated];
        let geo_wps: Vec<_> = refs.iter().map(|w| geoio::Waypoint {
            name: ozi_hack(&w.name), lon: w.lon, lat: w.lat,
            ele_m: w.ele, time: w.time,
            description: w.description.clone().unwrap_or_default(),
            text_color: w.text_color, bg_color: w.bg_color,
        }).collect();
        let raw = geoio::wpt::write(&geo_wps, geoio::text::Encoding::Cp1251);
        let output = add_wpt_extras(raw, &refs).unwrap();
        let decoded = read_wpt_extras(&output);
        assert_eq!(decoded[0].0, None);
        assert_eq!(decoded[0].1, None);
        assert_eq!(decoded[1].0.as_deref(), Some("cellar"));
        assert_eq!(decoded[1].1, Some(35));
        assert!(output.starts_with(b"OziExplorer Waypoint File Version"));
    }

    #[test]
    fn sample_with_only_category() {
        let sample = b"OziExplorer Waypoint File Version 1.1\r\nWGS 84\r\nReserved 2\r\ngarmin\r\n1,Test,  51.669144,  39.099740,46349.7377440,  0, 0, 3,         0,     65535,, 0, 0,    0,   -777, 6, 0,17,0,10.0,2,,cellar,,420\r\n";
        assert_eq!(read_wpt_extras(sample), vec![(Some("cellar".to_string()), None, Some(420))]);
    }

    #[test]
    fn sample_with_only_azimuth() {
        let sample = b"OziExplorer Waypoint File Version 1.1\r\nWGS 84\r\nReserved 2\r\ngarmin\r\n1,Test,  51.667969,  39.098366,46349.6094044,  0, 0, 3,         0,     65535,, 0, 0,    0,   -777, 6, 0,17,0,10.0,2,,,:222,420\r\n";
        assert_eq!(read_wpt_extras(sample), vec![(None, Some(222), Some(420))]);
    }
}

pub fn save_wpt(waypoints: &[&Waypoint], path: &Path) -> Result<(), String> {
    let geo_wps: Vec<geoio::Waypoint> = waypoints.iter().map(|w| geoio::Waypoint {
        name: ozi_hack(&w.name),
        lon: w.lon,
        lat: w.lat,
        ele_m: w.ele,
        // Preserve the original timestamp. Ozi's local UTC offset is field 25.
        time: w.time,
        description: ozi_hack(w.description.as_deref().unwrap_or("")),
        text_color: w.text_color,
        bg_color: w.bg_color,                                                             
    }).collect();
    
    let bytes = geoio::wpt::write(&geo_wps, geoio::text::Encoding::Cp1251);
    // Сохраняем оригинальную CP1251-кодировку и остальные поля geoio без изменений.
    let bytes = add_wpt_extras(bytes, waypoints)?;
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn save_ms(tracks: &[&Track], waypoints: &[&Waypoint], path: &Path) -> Result<(), String> {
    let mut gd = geoio::GeoData::default();
    
    for t in tracks {
        let segments = t.segments.iter().map(|seg| {
            seg.points.iter().map(|p| geoio::TrackPoint {
                lat: p.lat,
                lon: p.lon,
                ele_m: p.ele,
                time: p.time,
                hdop: p.hdop,
                speed_m_s: p.speed_m_s,
            }).collect()
        }).collect();
        
        gd.tracks.push(geoio::Track {
            name: t.name.clone(),
            segments,
            ..Default::default()
        });
    }
    
    for w in waypoints {
        gd.waypoints.push(geoio::Waypoint {
            name: w.name.clone(),
            lon: w.lon,
            lat: w.lat,
            ele_m: w.ele,
            time: w.time,
            description: w.description.clone().unwrap_or_default(),
            text_color: w.text_color,
            bg_color: w.bg_color,
        });
    }
    
    let map_name = path.file_stem().unwrap_or_default().to_string_lossy();
    let xml = geoio::ms::write(&gd, &map_name, &geoio::ms::MsStyle::default());
    std::fs::write(path, xml).map_err(|e| format!("{}: {e}", path.display()))
}

/// Garmin Course FIT: one track and optional selected waypoints as Course Points.
/// The binary format is handled by the independent geoio::fit module.
pub fn save_fit(tracks: &[&Track], waypoints: &[&Waypoint], path: &Path) -> Result<(), String> {
    if tracks.len() != 1 { return Err(crate::t!("msg_0628").to_owned()); }
    let mut data = geoio::GeoData::default();
    let t = tracks[0];
    data.tracks.push(geoio::Track {
        name: t.name.clone(),
        segments: t.segments.iter().map(|seg| seg.points.iter().map(|p| geoio::TrackPoint {
            lat: p.lat,
            lon: p.lon,
            ele_m: p.ele,
            time: p.time,
            hdop: p.hdop,
            speed_m_s: p.speed_m_s,
        }).collect()).collect(),
        ..Default::default()
    });
    data.waypoints = waypoints.iter().map(|w| geoio::Waypoint {
        name: w.name.clone(),
        lon: w.lon,
        lat: w.lat,
        ele_m: w.ele,
        time: w.time,
        description: w.description.clone().unwrap_or_default(),
        text_color: w.text_color,
        bg_color: w.bg_color,
    }).collect();
    let bytes = geoio::fit::write(&data).map_err(|e| e.to_string())?;
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn save_any(tracks: &[&Track], waypoints: &[&Waypoint], path: &Path) -> Result<(), String> {
    let ext = path.extension().and_then(|s| s.to_str()).map(|s| s.to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "gpx" => save_gpx(tracks, waypoints, path),
        "plt" => {
            if !waypoints.is_empty() { return Err(t!("msg_0009").into()); }
            if tracks.len() != 1 { return Err(t!("msg_0010").into()); }
            save_plt(tracks[0], path)
        },
        "wpt" => {
            if !tracks.is_empty() { return Err(t!("msg_0011").into()); }
            save_wpt(waypoints, path)
        },
        "ms" => save_ms(tracks, waypoints, path),
        "kml" => save_kml(tracks, waypoints, path),
        "fit" => save_fit(tracks, waypoints, path),
        _ => Err(crate::tf!("msg_0406", format = ext)),
    }
}
