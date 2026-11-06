//! ms — XML customMapSource с блоком <geojson> (формат Guru Maps).
//! Чтение терпимое: без CDATA, с запятыми перед «]» и «}», с лишним текстом после JSON.
//! Запись как в вашем конвертере GPX2MS: валидный JSON в CDATA, координаты [lon, lat].

use serde_json::{json, Map, Value};

use crate::time;
use crate::{text, GeoData, GeoError, Track, TrackPoint, Waypoint};

/// Иконка точки из файла вашего конвертера (SVG, base64).
const ICON_B64: &str = "PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSI4IiBoZWlnaHQ9IjgiIHZpZXdCb3g9IjAgMCA4IDgiPjxjaXJjbGUgY3g9IjQiIGN5PSI0IiByPSIzIiBmaWxsPSIjODg4Ii8+PC9zdmc+";

#[derive(Debug, Clone, PartialEq)]
pub struct MsStyle {
    /// «#RRGGBB»
    pub line_color: String,
    pub line_width_px: u32,
    pub point_color: String,
    pub icon_scale: f64,
}

impl Default for MsStyle {
    fn default() -> Self {
        MsStyle {
            line_color: "#00FFFF".into(),
            line_width_px: 3,
            point_color: "#00FFFF".into(),
            icon_scale: 1.0,
        }
    }
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Содержимое элемента <tag>…</tag>: с CDATA или без.
fn extract(text: &str, tag: &str) -> Option<String> {
    let start = text.find(&format!("<{tag}"))?;
    let gt = start + text[start..].find('>')? + 1;
    let end = gt + text[gt..].find(&format!("</{tag}>"))?;
    let body = text[gt..end].trim();
    if let Some(inner) = body.strip_prefix("<![CDATA[") {
        Some(inner.strip_suffix("]]>").unwrap_or(inner).to_string())
    } else {
        Some(unescape(body))
    }
}

/// Убирает запятые перед «]» и «}» вне строк.
fn strip_trailing_commas(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let (mut in_str, mut esc) = (false, false);
    for (i, &c) in chars.iter().enumerate() {
        if in_str {
            out.push(c);
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
            }
            ',' => {
                let next = chars[i + 1..].iter().find(|ch| !ch.is_whitespace());
                if matches!(next, Some(']') | Some('}')) {
                    continue;
                }
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// [lon, lat, ele?] -> (lat, lon, ele). Третья координата 0.0 считается «нет высоты»:
/// в файлах Liza Alert это заполнитель.
fn position(v: &Value) -> Option<(f64, f64, Option<f64>)> {
    let a = v.as_array()?;
    let lon = a.first()?.as_f64()?;
    let lat = a.get(1)?.as_f64()?;
    if !(lat.abs() <= 90.0 && lon.abs() <= 180.0) {
        return None;
    }
    let ele = a.get(2).and_then(Value::as_f64).filter(|z| *z != 0.0);
    Some((lat, lon, ele))
}

fn line(coords: &Value, times: Option<&Value>) -> Option<Vec<TrackPoint>> {
    let arr = coords.as_array()?;
    let ts: Option<Vec<Option<f64>>> = times
        .and_then(Value::as_array)
        .filter(|t| t.len() == arr.len())
        .map(|t| t.iter().map(|v| v.as_str().and_then(time::parse_iso)).collect());
    let mut out = Vec::with_capacity(arr.len());
    for (i, p) in arr.iter().enumerate() {
        let (lat, lon, ele_m) = position(p)?;
        out.push(TrackPoint { lat, lon, ele_m, time: ts.as_ref().and_then(|t| t[i]) });
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn read_feature(f: &Value, index: usize, data: &mut GeoData) {
    let n = index + 1;
    let props = f.get("properties");
    let name = props
        .and_then(|p| p.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let Some(geom) = f.get("geometry") else {
        data.warnings.push(format!("объект {n}: нет geometry"));
        return;
    };
    let Some(coords) = geom.get("coordinates") else {
        data.warnings.push(format!("объект {n}: нет coordinates"));
        return;
    };
    match geom.get("type").and_then(Value::as_str) {
        Some("Point") => match position(coords) {
            Some((lat, lon, ele_m)) => data.waypoints.push(Waypoint {
                name,
                lat,
                lon,
                ele_m,
                time: props
                    .and_then(|p| p.get("time"))
                    .and_then(Value::as_str)
                    .and_then(time::parse_iso),
                description: String::new(),
            }),
            None => data.warnings.push(format!("объект {n}: некорректная точка")),
        },
        Some("LineString") => match line(coords, props.and_then(|p| p.get("times"))) {
            Some(seg) => data.tracks.push(Track { name, segments: vec![seg], ..Track::default() }),
            None => data.warnings.push(format!("объект {n}: некорректная линия")),
        },
        Some("MultiLineString") => {
            let times = props.and_then(|p| p.get("times"));
            let mut segments = Vec::new();
            for (k, c) in coords.as_array().into_iter().flatten().enumerate() {
                let t = times.and_then(|t| t.get(k));
                match line(c, t) {
                    Some(seg) => segments.push(seg),
                    None => data.warnings.push(format!("объект {n}: сегмент {} пропущен", k + 1)),
                }
            }
            if !segments.is_empty() {
                data.tracks.push(Track { name, segments, ..Track::default() });
            }
        }
        Some(other) => data.warnings.push(format!("объект {n}: тип «{other}» не поддерживается")),
        None => data.warnings.push(format!("объект {n}: нет type")),
    }
}

pub fn read(bytes: &[u8]) -> Result<GeoData, GeoError> {
    let text = text::decode(bytes);
    if !text.contains("<customMapSource") {
        return Err(GeoError::WrongFormat("нет элемента <customMapSource>".into()));
    }
    let body = extract(&text, "geojson")
        .ok_or_else(|| GeoError::WrongFormat("нет блока <geojson>".into()))?;
    let cleaned = strip_trailing_commas(&body);
    let mut data = GeoData::default();
    let mut stream = serde_json::Deserializer::from_str(&cleaned).into_iter::<Value>();
    let root = match stream.next() {
        Some(Ok(v)) => v,
        Some(Err(e)) => return Err(GeoError::WrongFormat(format!("JSON: {e}"))),
        None => return Err(GeoError::WrongFormat("блок <geojson> пуст".into())),
    };
    let rest = cleaned[stream.byte_offset()..].trim();
    if !rest.is_empty() {
        data.warnings.push("после GeoJSON найден лишний текст, он пропущен".into());
    }
    let features: Vec<&Value> = match root.get("type").and_then(Value::as_str) {
        Some("FeatureCollection") => root
            .get("features")
            .and_then(Value::as_array)
            .map(|a| a.iter().collect())
            .unwrap_or_default(),
        Some("Feature") => vec![&root],
        _ => return Err(GeoError::WrongFormat("в <geojson> не FeatureCollection".into())),
    };
    for (i, f) in features.into_iter().enumerate() {
        read_feature(f, i, &mut data);
    }
    Ok(data)
}

fn color_or(c: &str, default: &str) -> String {
    let ok = c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit());
    if ok { c.to_string() } else { default.to_string() }
}

/// Запись. Высоты не пишутся (как в вашем конвертере), время пишется, если оно есть у всех точек трека.
pub fn write(data: &GeoData, map_name: &str, style: &MsStyle) -> String {
    let r7 = |x: f64| (x * 1e7).round() / 1e7;
    let mut features: Vec<Value> = Vec::new();

    for w in &data.waypoints {
        let mut props = Map::new();
        props.insert("name".into(), json!(w.name));
        if let Some(t) = w.time {
            props.insert("time".into(), json!(time::format_iso(t)));
        }
        features.push(json!({
            "type": "Feature",
            "geometry": { "type": "Point", "coordinates": [r7(w.lon), r7(w.lat)] },
            "properties": Value::Object(props)
        }));
    }
    for t in &data.tracks {
        let segs: Vec<Value> = t
            .segments
            .iter()
            .map(|s| Value::Array(s.iter().map(|p| json!([r7(p.lon), r7(p.lat)])).collect()))
            .collect();
        let mut props = Map::new();
        props.insert("name".into(), json!(t.name));
        if !t.segments.is_empty() && t.segments.iter().flatten().all(|p| p.time.is_some()) {
            let times: Vec<Value> = t
                .segments
                .iter()
                .map(|s| {
                    Value::Array(
                        s.iter().map(|p| json!(time::format_iso(p.time.unwrap_or(0.0)))).collect(),
                    )
                })
                .collect();
            props.insert("times".into(), Value::Array(times));
        }
        features.push(json!({
            "type": "Feature",
            "geometry": { "type": "MultiLineString", "coordinates": segs },
            "properties": Value::Object(props)
        }));
    }
    let root = json!({ "type": "FeatureCollection", "features": features });
    // «]]>» может встретиться только внутри строки; «>» в ней заменяем на \u003e
    let json_text = serde_json::to_string_pretty(&root)
        .unwrap_or_default()
        .replace("]]>", "]]\\u003e");

    let default = MsStyle::default();
    let lc = color_or(&style.line_color, &default.line_color);
    let pc = color_or(&style.point_color, &default.point_color);
    let width = style.line_width_px.clamp(1, 30);
    let scale = if style.icon_scale.is_finite() && style.icon_scale > 0.0 {
        style.icon_scale
    } else {
        1.0
    };

    let mut css = String::new();
    css.push_str(
        "node {\n    text: eval(tag(\"name\"));\n    details-text: eval(tag(\"name\"));\n    \
         details-description: eval(tag(\"name\"));\n    text-color: black;\n    \
         font-stroke-width: 5px;\n    font-stroke-color: yellow;\n",
    );
    css.push_str(&format!("    icon-image: eval(data(\"{}\"));\n", ICON_B64));
    css.push_str(&format!("    icon-scale: {};\n    icon-tint: {};\n}}\n", scale, pc));
    css.push_str(&format!("line {{\n    color: {};\n    width: {}px;\n}}\n", lc, width));

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<customMapSource overlay=\"true\">\n  \
         <name>{}</name>\n  <style><![CDATA[\n{}  ]]></style>\n  \
         <geojson><![CDATA[\n{}\n  ]]></geojson>\n</customMapSource>\n",
        xml_escape(map_name),
        css,
        json_text
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIZA: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<customMapSource overlay="true">
<name>2023-09-21_Novosibirsk</name>
<geojson>{
       "type": "FeatureCollection",
       "generator": "Setka500",
       "features": [
{ "type": "Feature", "properties": { "name": "Setka_500" }, "geometry": { "type": "MultiLineString", "coordinates":[ [
[ 83.058272,54.970978, 0.0 ] , 
[ 83.120715,54.969904, 0.0 ] , 
  ]] }},
{ "type": "Feature", "properties": { "name": "A1" }, "geometry": { "type": "Point", "coordinates": [ 83.058272,54.970978, 0.0 ] } },
{ "type": "Feature", "properties": { "name": "B1" }, "geometry": { "type": "Point", "coordinates": [ 83.062175,54.970911, 0.0 ] } },
]}</geojson>
<style>
node { text: eval(tag("name")); icon-image:"circle.svgpb"; } line { color: #00FFFF ; width: 3px; }
</style>
</customMapSource>"#;

    fn tp(lat: f64, lon: f64, t: f64) -> TrackPoint {
        TrackPoint { lat, lon, ele_m: None, time: Some(t) }
    }

    #[test]
    fn reads_liza_alert_style_file() {
        let d = read(LIZA.as_bytes()).unwrap();
        assert!(d.warnings.is_empty(), "{:?}", d.warnings);
        assert_eq!(d.waypoints.len(), 2);
        assert_eq!(d.waypoints[0].name, "A1");
        assert!((d.waypoints[0].lat - 54.970978).abs() < 1e-9);
        assert!((d.waypoints[0].lon - 83.058272).abs() < 1e-9);
        assert!(d.waypoints[0].ele_m.is_none());
        assert_eq!(d.tracks.len(), 1);
        assert_eq!(d.tracks[0].name, "Setka_500");
        assert_eq!(d.tracks[0].segments.len(), 1);
        assert_eq!(d.tracks[0].segments[0].len(), 2);
    }

    #[test]
    fn roundtrip_with_cdata_and_special_chars() {
        let mut d = GeoData::default();
        d.waypoints.push(Waypoint {
            name: "Лагерь & «база»".into(),
            lat: 54.946425,
            lon: 83.08296,
            ele_m: None,
            time: Some(1_792_822_157.0),
            description: String::new(),
        });
        d.waypoints.push(Waypoint {
            name: "x]]>y".into(),
            lat: -33.5,
            lon: -70.25,
            ele_m: None,
            time: None,
            description: String::new(),
        });
        d.tracks.push(Track {
            name: "Трек 1".into(),
            segments: vec![
                vec![tp(54.1, 83.1, 1_792_822_157.0), tp(54.2, 83.2, 1_792_822_182.0)],
                vec![tp(54.3, 83.3, 1_792_822_215.0)],
            ],
            ..Track::default()
        });
        let xml = write(&d, "тест & <карта>", &MsStyle::default());
        assert!(xml.contains("<![CDATA["));
        let b = read(xml.as_bytes()).unwrap();
        assert!(b.warnings.is_empty(), "{:?}", b.warnings);
        assert_eq!(b.waypoints.len(), 2);
        assert_eq!(b.waypoints[0].name, d.waypoints[0].name);
        assert_eq!(b.waypoints[0].time, d.waypoints[0].time);
        assert_eq!(b.waypoints[1].name, "x]]>y");
        assert!(b.waypoints[1].time.is_none());
        assert!((b.waypoints[1].lat + 33.5).abs() < 1e-9);
        assert_eq!(b.tracks.len(), 1);
        assert_eq!(b.tracks[0].name, "Трек 1");
        assert_eq!(b.tracks[0].segments.len(), 2);
        assert_eq!(b.tracks[0].segments[0].len(), 2);
        assert_eq!(b.tracks[0].segments[1][0].time, Some(1_792_822_215.0));
        assert!((b.tracks[0].segments[0][1].lat - 54.2).abs() < 1e-9);
    }

    #[test]
    fn tolerates_trailing_text_and_commas_in_strings() {
        let xml = "<customMapSource><geojson>{\"type\":\"FeatureCollection\",\"features\":[]}\n]</geojson></customMapSource>";
        let d = read(xml.as_bytes()).unwrap();
        assert_eq!(d.warnings.len(), 1);
        assert_eq!(
            strip_trailing_commas(r#"{"a":"x,]","b":[1,2,],}"#),
            r#"{"a":"x,]","b":[1,2]}"#
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(read(b"<kml/>").is_err());
        assert!(read(b"<customMapSource></customMapSource>").is_err());
        assert!(read(b"<customMapSource><geojson>{oops</geojson></customMapSource>").is_err());
        let not_fc = "<customMapSource><geojson>{\"type\":\"Banana\"}</geojson></customMapSource>";
        assert!(read(not_fc.as_bytes()).is_err());
    }

    #[test]
    fn bad_colors_fall_back_to_default() {
        let st = MsStyle { line_color: "red".into(), point_color: "#12345".into(), ..MsStyle::default() };
        let xml = write(&GeoData::default(), "x", &st);
        assert!(xml.contains("color: #00FFFF;"));
    }
}
