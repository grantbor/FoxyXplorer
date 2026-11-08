//! GPX 1.1: путевые точки (<wpt>) и треки (<trk>). Маршруты (<rte>) пропускаются.

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

fn latlon(n: &Node<'_, '_>) -> Option<(f64, f64)> {
    let lat: f64 = n.attribute("lat")?.trim().parse().ok()?;
    let lon: f64 = n.attribute("lon")?.trim().parse().ok()?;
    if (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) {
        Some((lat, lon))
    } else {
        None
    }
}

fn read_point(n: &Node<'_, '_>, warnings: &mut Vec<String>) -> Option<TrackPoint> {
    let Some((lat, lon)) = latlon(n) else {
        warnings.push("точка без корректных lat/lon пропущена".into());
        return None;
    };
    let ele_m = child_text(n, "ele").and_then(|s| s.parse::<f64>().ok());
    let time = match child_text(n, "time") {
        Some(t) => {
            let v = time::parse_iso(&t);
            if v.is_none() {
                warnings.push(format!("не удалось разобрать время «{t}»"));
            }
            v
        }
        None => None,
    };
    Some(TrackPoint { lat, lon, ele_m, time })
}

fn read_wpt(n: &Node<'_, '_>, warnings: &mut Vec<String>) -> Option<Waypoint> {
    let p = read_point(n, warnings)?;
    Some(Waypoint {
        name: child_text(n, "name").unwrap_or_default(),
        lat: p.lat,
        lon: p.lon,
        ele_m: p.ele_m,
        time: p.time,
        description: child_text(n, "desc")
            .or_else(|| child_text(n, "cmt"))
            .unwrap_or_default(),
        bg_color: [0, 255, 255], 
        text_color: [0, 0, 0],   
    })
}

fn read_trk(n: &Node<'_, '_>, warnings: &mut Vec<String>) -> Track {
    let mut t = Track::default();
    t.name = child_text(n, "name").unwrap_or_default();
    t.gpx_color = n
        .descendants()
        .find(|d| is(d, "DisplayColor"))
        .and_then(|d| d.text())
        .map(|s| s.trim().to_string());
    for seg in n.children().filter(|c| is(c, "trkseg")) {
        let pts: Vec<TrackPoint> = seg
            .children()
            .filter(|c| is(c, "trkpt"))
            .filter_map(|p| read_point(&p, warnings))
            .collect();
        if !pts.is_empty() {
            t.segments.push(pts);
        }
    }
    t
}

pub fn read(bytes: &[u8]) -> Result<GeoData, GeoError> {
    let text = text::decode(bytes);
    let doc = Document::parse(&text).map_err(|e| GeoError::Xml(e.to_string()))?;
    let root = doc.root_element();
    if root.tag_name().name() != "gpx" {
        return Err(GeoError::WrongFormat("корневой элемент не <gpx>".into()));
    }
    let mut data = GeoData::default();
    for n in root.children().filter(|n| n.is_element()) {
        match n.tag_name().name() {
            "wpt" => {
                if let Some(w) = read_wpt(&n, &mut data.warnings) {
                    data.waypoints.push(w);
                }
            }
            "trk" => {
                let t = read_trk(&n, &mut data.warnings);
                data.tracks.push(t);
            }
            "rte" => data.warnings.push("маршруты <rte> пропущены (пока не поддерживаются)".into()),
            _ => {}
        }
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

/// Статистика Garmin (TrackStatsExtension) не переносится: после правок
/// трека она всё равно неверна.
pub fn write(data: &GeoData) -> String {
    let mut o = String::new();
    o.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    o.push_str(
        "<gpx version=\"1.1\" creator=\"FoxyXplorer\" xmlns=\"http://www.topografix.com/GPX/1/1\" \
xmlns:gpxx=\"http://www.garmin.com/xmlschemas/GpxExtensions/v3\">\n",
    );
    for w in &data.waypoints {
        o.push_str(&format!("  <wpt lat=\"{:.7}\" lon=\"{:.7}\">\n", w.lat, w.lon));
        if let Some(e) = w.ele_m {
            o.push_str(&format!("    <ele>{e:.2}</ele>\n"));
        }
        if let Some(t) = w.time {
            o.push_str(&format!("    <time>{}</time>\n", time::format_iso(t)));
        }
        o.push_str(&format!("    <name>{}</name>\n", esc(&w.name)));
        if !w.description.is_empty() {
            o.push_str(&format!("    <desc>{}</desc>\n", esc(&w.description)));
        }
        o.push_str("  </wpt>\n");
    }
    for t in &data.tracks {
        o.push_str("  <trk>\n");
        o.push_str(&format!("    <name>{}</name>\n", esc(&t.name)));
        if let Some(c) = &t.gpx_color {
            o.push_str(&format!(
                "    <extensions><gpxx:TrackExtension><gpxx:DisplayColor>{}</gpxx:DisplayColor></gpxx:TrackExtension></extensions>\n",
                esc(c)
            ));
        }
        for seg in &t.segments {
            o.push_str("    <trkseg>\n");
            for p in seg {
                o.push_str(&format!("      <trkpt lat=\"{:.7}\" lon=\"{:.7}\">", p.lat, p.lon));
                if let Some(e) = p.ele_m {
                    o.push_str(&format!("<ele>{e:.2}</ele>"));
                }
                if let Some(tm) = p.time {
                    o.push_str(&format!("<time>{}</time>", time::format_iso(tm)));
                }
                o.push_str("</trkpt>\n");
            }
            o.push_str("    </trkseg>\n");
        }
        o.push_str("  </trk>\n");
    }
    o.push_str("</gpx>\n");
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    const GARMIN: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="no" ?><gpx xmlns="http://www.topografix.com/GPX/1/1" xmlns:gpxx="http://www.garmin.com/xmlschemas/GpxExtensions/v3" creator="eTrex 20x" version="1.1"><trk><name>2026-10-24-ЛИСА4</name><extensions><gpxx:TrackExtension><gpxx:DisplayColor>Magenta</gpxx:DisplayColor></gpxx:TrackExtension></extensions><trkseg><trkpt lat="54.8453057650" lon="83.0900727678"><ele>156.35</ele><time>2026-10-24T06:09:17Z</time></trkpt><trkpt lat="54.8452904262" lon="83.0900366418"><ele>155.39</ele><time>2026-10-24T06:09:42Z</time></trkpt></trkseg></trk></gpx>"#;

    #[test]
    fn reads_garmin_track() {
        let d = read(GARMIN.as_bytes()).unwrap();
        assert!(d.warnings.is_empty(), "{:?}", d.warnings);
        assert!(d.waypoints.is_empty());
        let t = &d.tracks[0];
        assert_eq!(t.name, "2026-10-24-ЛИСА4");
        assert_eq!(t.gpx_color.as_deref(), Some("Magenta"));
        assert_eq!(t.segments.len(), 1);
        assert_eq!(t.segments[0].len(), 2);
        assert_eq!(t.segments[0][0].ele_m, Some(156.35));
        assert_eq!(time::format_iso(t.segments[0][0].time.unwrap()), "2026-10-24T06:09:17Z");
    }

    #[test]
    fn write_read_roundtrip() {
        let mut d = read(GARMIN.as_bytes()).unwrap();
        d.waypoints.push(Waypoint {
            name: "A & B <1>".into(),
            lat: 54.97,
            lon: 83.05,
            ele_m: None,
            time: None,
            description: String::new(),
        });
        let b = read(write(&d).as_bytes()).unwrap();
        assert!(b.warnings.is_empty(), "{:?}", b.warnings);
        assert_eq!(b.waypoints[0].name, "A & B <1>");
        assert_eq!(b.tracks[0].name, d.tracks[0].name);
        assert_eq!(b.tracks[0].gpx_color, d.tracks[0].gpx_color);
        let (p, q) = (&b.tracks[0].segments[0][0], &d.tracks[0].segments[0][0]);
        assert!((p.lat - q.lat).abs() < 1e-7 && (p.lon - q.lon).abs() < 1e-7);
    }

    #[test]
    fn rejects_non_gpx() {
        assert!(read(b"<kml/>").is_err());
        assert!(read(b"not xml").is_err());
    }
}
