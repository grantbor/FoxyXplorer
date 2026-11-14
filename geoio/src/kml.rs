//! KML 2.2: чтение и запись треков и путевых точек.
//!
//! Поддерживается:
//! - `<Point>` → путевая точка
//! - `<LineString>` → сегмент трека (без времён — в KML нет per-point time)
//! - `<gx:Track>` → сегмент трека с временами (расширение Google)
//! - `<MultiGeometry>` с несколькими `<LineString>`/`<gx:Track>` → multi-segment трек
//! - `<TimeStamp><when>` у Placemark → время путевой точки
//! - `<Style><LineStyle><color>` → цвет линии (AABBGGRR → #RRGGBB)
//!
//! Не поддерживается: `.kmz` (ZIP), `<styleUrl>`-ссылки (только inline `<Style>`).

use crate::time;
use crate::{text, GeoData, GeoError, Track, TrackPoint, Waypoint};
use roxmltree::{Document, Node};

fn is(n: &Node<'_, '_>, name: &str) -> bool {
    n.is_element() && n.tag_name().name() == name
}

fn child_text(n: &Node<'_, '_>, name: &str) -> Option<String> {
    n.children()
        .find(|c| is(c, name))
        .and_then(|c| c.text())
        .map(|t| t.trim().to_string())
}

/// KML: "lon,lat[,alt]" → (lat, lon, ele_m). Порядок меняется!
fn parse_coord(s: &str) -> Option<(f64, f64, Option<f64>)> {
    let mut it = s.trim().split(',');
    let lon: f64 = it.next()?.trim().parse().ok()?;
    let lat: f64 = it.next()?.trim().parse().ok()?;
    let alt: Option<f64> = it.next().and_then(|a| a.trim().parse().ok());
    if (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) {
        Some((lat, lon, alt))
    } else {
        None
    }
}

fn parse_coordinates(text: &str) -> Vec<(f64, f64, Option<f64>)> {
    text.split_whitespace().filter_map(parse_coord).collect()
}

/// AABBGGRR → "#RRGGBB".
fn parse_color(s: &str) -> Option<String> {
    let s = s.trim();
    if s.len() != 8 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{}{}{}", &s[6..8], &s[4..6], &s[2..4]))
}

/// "#RRGGBB" → AABBGGRR (alpha=FF).
fn write_color(rgb: &str) -> Option<String> {
    let s = rgb.trim_start_matches('#');
    if s.len() != 6 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("ff{}{}{}", &s[4..6], &s[2..4], &s[0..2]))
}

fn read_point(n: &Node<'_, '_>, _warnings: &mut Vec<String>) -> Option<TrackPoint> {
    let coords = n
        .children()
        .find(|c| is(c, "coordinates"))
        .and_then(|c| c.text())?;
    let (lat, lon, alt) = parse_coordinates(coords).into_iter().next()?;
    let t = n
        .children()
        .find(|c| is(c, "TimeStamp"))
        .and_then(|ts| ts.children().find(|c| is(c, "when")))
        .and_then(|w| w.text())
        .and_then(time::parse_iso);
    Some(TrackPoint { lat, lon, ele_m: alt, time: t })
}

fn read_line_string(n: &Node<'_, '_>, warnings: &mut Vec<String>) -> Vec<TrackPoint> {
    let Some(coords) = n
        .children()
        .find(|c| is(c, "coordinates"))
        .and_then(|c| c.text())
    else {
        warnings.push("LineString без coordinates пропущен".into());
        return Vec::new();
    };
    parse_coordinates(coords)
        .into_iter()
        .map(|(lat, lon, ele_m)| TrackPoint { lat, lon, ele_m, time: None })
        .collect()
}

fn read_gx_track(n: &Node<'_, '_>, _warnings: &mut Vec<String>) -> Vec<TrackPoint> {
    let mut whens: Vec<Option<f64>> = Vec::new();
    let mut coords: Vec<(f64, f64, Option<f64>)> = Vec::new();
    for child in n.children() {
        if is(&child, "when") {
            whens.push(child.text().and_then(time::parse_iso));
        } else if is(&child, "coord") {
            // gx:coord: "lon lat alt" через пробелы (не запятые!)
            if let Some(t) = child.text() {
                let parts: Vec<&str> = t.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let (Ok(lon), Ok(lat)) =
                        (parts[0].parse::<f64>(), parts[1].parse::<f64>())
                    {
                        let alt = parts.get(2).and_then(|s| s.parse().ok());
                        if (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) {
                            coords.push((lat, lon, alt));
                        }
                    }
                }
            }
        }
    }
    let n = coords.len().max(whens.len());
    let mut pts = Vec::with_capacity(n);
    for i in 0..n {
        let (lat, lon, ele_m) = coords.get(i).copied().unwrap_or((0.0, 0.0, None));
        let t = whens.get(i).copied().flatten();
        pts.push(TrackPoint { lat, lon, ele_m, time: t });
    }
    pts
}

fn read_placemark(n: &Node<'_, '_>, data: &mut GeoData) {
    let name = child_text(n, "name").unwrap_or_default();
    let description = child_text(n, "description").unwrap_or_default();
    let color = n
        .descendants()
        .find(|d| is(d, "LineStyle"))
        .and_then(|ls| ls.children().find(|c| is(c, "color")))
        .and_then(|c| c.text())
        .and_then(parse_color);

    // Point
    if let Some(p) = n.children().find(|c| is(c, "Point")) {
        if let Some(pt) = read_point(&p, &mut data.warnings) {
            data.waypoints.push(Waypoint {
                name,
                lat: pt.lat,
                lon: pt.lon,
                ele_m: pt.ele_m,
                time: pt.time,
                description,
                bg_color: [0, 255, 255],
                text_color: [0, 0, 0],
            });
        }
        return;
    }

    // LineString
    if let Some(ls) = n.children().find(|c| is(c, "LineString")) {
        let pts = read_line_string(&ls, &mut data.warnings);
        if !pts.is_empty() {
            let mut t = Track::default();
            t.name = name;
            t.gpx_color = color;
            t.segments.push(pts);
            data.tracks.push(t);
        }
        return;
    }

    // gx:Track
    if let Some(gx) = n.children().find(|c| is(c, "Track")) {
        let pts = read_gx_track(&gx, &mut data.warnings);
        if !pts.is_empty() {
            let mut t = Track::default();
            t.name = name;
            t.gpx_color = color;
            t.segments.push(pts);
            data.tracks.push(t);
        }
        return;
    }

    // MultiGeometry
    if let Some(mg) = n.children().find(|c| is(c, "MultiGeometry")) {
        let mut t = Track::default();
        t.name = name;
        t.gpx_color = color;
        for child in mg.children() {
            if is(&child, "LineString") {
                let pts = read_line_string(&child, &mut data.warnings);
                if !pts.is_empty() {
                    t.segments.push(pts);
                }
            } else if is(&child, "Track") {
                let pts = read_gx_track(&child, &mut data.warnings);
                if !pts.is_empty() {
                    t.segments.push(pts);
                }
            }
        }
        if !t.segments.is_empty() {
            data.tracks.push(t);
        }
    }
}

pub fn read(bytes: &[u8]) -> Result<GeoData, GeoError> {
    let text = text::decode(bytes);
    let doc = Document::parse(&text).map_err(|e| GeoError::Xml(e.to_string()))?;
    let root = doc.root_element();
    if root.tag_name().name() != "kml" {
        return Err(GeoError::WrongFormat("корневой элемент не <kml>".into()));
    }
    let mut data = GeoData::default();
    for pm in root.descendants().filter(|n| is(n, "Placemark")) {
        read_placemark(&pm, &mut data);
    }
    Ok(data)
}

fn esc(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t' | '\r'))
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn fmt_coord(lat: f64, lon: f64, ele: Option<f64>) -> String {
    match ele {
        Some(e) => format!("{:.7},{:.7},{:.3}", lon, lat, e),
        None => format!("{:.7},{:.7},0", lon, lat),
    }
}

fn has_time(seg: &[TrackPoint]) -> bool {
    seg.len() > 1 && seg.iter().all(|p| p.time.is_some())
}

fn write_linestring(o: &mut String, seg: &[TrackPoint]) {
    o.push_str("      <LineString><coordinates>");
    for (i, p) in seg.iter().enumerate() {
        if i > 0 {
            o.push(' ');
        }
        o.push_str(&fmt_coord(p.lat, p.lon, p.ele_m));
    }
    o.push_str("</coordinates></LineString>\n");
}

fn write_gx_track(o: &mut String, seg: &[TrackPoint]) {
    o.push_str("      <gx:Track>\n");
    for p in seg {
        o.push_str(&format!(
            "        <when>{}</when>\n",
            time::format_iso(p.time.unwrap())
        ));
    }
    for p in seg {
        let alt = p.ele_m.unwrap_or(0.0);
        o.push_str(&format!(
            "        <gx:coord>{:.7} {:.7} {:.3}</gx:coord>\n",
            p.lon, p.lat, alt
        ));
    }
    o.push_str("      </gx:Track>\n");
}

pub fn write(data: &GeoData) -> String {
    let mut o = String::new();
    o.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    o.push_str(
        "<kml xmlns=\"http://www.opengis.net/kml/2.2\" \
xmlns:gx=\"http://www.google.com/kml/ext/2.2\">\n<Document>\n",
    );

    for w in &data.waypoints {
        o.push_str("  <Placemark>\n");
        o.push_str(&format!("    <name>{}</name>\n", esc(&w.name)));
        if !w.description.is_empty() {
            o.push_str(&format!(
                "    <description>{}</description>\n",
                esc(&w.description)
            ));
        }
        if let Some(t) = w.time {
            o.push_str(&format!(
                "    <TimeStamp><when>{}</when></TimeStamp>\n",
                time::format_iso(t)
            ));
        }
        o.push_str(&format!(
            "    <Point><coordinates>{}</coordinates></Point>\n",
            fmt_coord(w.lat, w.lon, w.ele_m)
        ));
        o.push_str("  </Placemark>\n");
    }

    for t in &data.tracks {
        o.push_str("  <Placemark>\n");
        o.push_str(&format!("    <name>{}</name>\n", esc(&t.name)));

        if let Some(color) = t.gpx_color.as_deref().and_then(write_color) {
            o.push_str(&format!(
                "    <Style><LineStyle><color>{color}</color><width>2</width></LineStyle></Style>\n"
            ));
        }

        let use_gx = t.segments.iter().all(|s| has_time(s));

        if t.segments.len() == 1 {
            let seg = &t.segments[0];
            if use_gx {
                write_gx_track(&mut o, seg);
            } else {
                write_linestring(&mut o, seg);
            }
        } else {
            o.push_str("    <MultiGeometry>\n");
            for seg in &t.segments {
                if use_gx {
                    write_gx_track(&mut o, seg);
                } else {
                    write_linestring(&mut o, seg);
                }
            }
            o.push_str("    </MultiGeometry>\n");
        }
        o.push_str("  </Placemark>\n");
    }

    o.push_str("</Document>\n</kml>\n");
    o
}
