//! Garmin FIT (Flexible and Interoperable Data Transfer) interoperability.
//!
//! Based on Garmin's published FIT Protocol and Course/Activity profiles:
//! https://developer.garmin.com/fit/protocol/
//! https://developer.garmin.com/fit/file-types/course/
//!
//! Import Activity and Course GPS records and Course Point POIs. Export one
//! navigable Course containing one track and optional Course Point POIs.
//! FIT exercise sensor measurements and unknown vendor/developer fields are
//! intentionally not represented in GeoData. No Garmin SDK code is copied.

use crate::{GeoData, GeoError, Track, TrackPoint, Waypoint};

const FIT_UNIX_EPOCH: i64 = 631_065_600; // 1989-12-31 00:00:00 UTC
const SC_PER_DEGREE: f64 = 2_147_483_648.0 / 180.0;
const MAX_FILE_BYTES: usize = 128 * 1024 * 1024;

fn invalid(reason: impl Into<String>) -> GeoError {
    GeoError::WrongFormat(format!("FIT: {}", reason.into()))
}

fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &byte in bytes {
        crc ^= byte as u16;
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xA001 } else { 0 };
        }
    }
    crc
}

fn num(bytes: &[u8], little: bool) -> u64 {
    if little {
        bytes.iter().rev().fold(0, |x, b| (x << 8) | *b as u64)
    } else {
        bytes.iter().fold(0, |x, b| (x << 8) | *b as u64)
    }
}

#[derive(Clone)]
struct FieldDef {
    number: u8,
    size: usize,
    base_type: u8,
}

#[derive(Clone)]
struct Definition {
    global: u16,
    little: bool,
    fields: Vec<FieldDef>,
    developer_sizes: Vec<usize>,
}

struct Value<'a> {
    data: &'a [u8],
    base: u8,
    little: bool,
}

impl Value<'_> {
    fn integer(&self) -> Option<i64> {
        let kind = self.base & 0x1f;
        let width = match kind {
            0 | 1 | 2 | 10 | 13 => 1,
            3 | 4 | 11 => 2,
            5 | 6 | 12 => 4,
            14 | 15 | 16 => 8,
            _ => return None,
        };
        if self.data.len() != width { return None; }
        let raw = num(self.data, self.little);
        match kind {
            0 | 2 if raw == 0xff => None,
            4 if raw == 0xffff => None,
            6 if raw == 0xffff_ffff => None,
            10 | 11 | 12 | 16 if raw == 0 => None,
            1 if raw == 0x7f => None,
            3 if raw == 0x7fff => None,
            5 if raw == 0x7fff_ffff => None,
            14 if raw == 0x7fff_ffff_ffff_ffff => None,
            15 if raw == 0xffff_ffff_ffff_ffff => None,
            1 => Some((raw as u8 as i8) as i64),
            3 => Some((raw as u16 as i16) as i64),
            5 => Some((raw as u32 as i32) as i64),
            14 => Some(raw as i64),
            15 | 16 if raw > i64::MAX as u64 => None,
            _ => Some(raw as i64),
        }
    }

    fn text(&self) -> Option<String> {
        if self.base & 0x1f != 7 { return None; }
        let stop = self.data.iter().position(|&b| b == 0).unwrap_or(self.data.len());
        let s = std::str::from_utf8(&self.data[..stop]).ok()?.trim();
        (!s.is_empty()).then(|| s.to_owned())
    }
}

fn field<'a, 'b>(fields: &'b [(u8, Value<'a>)], id: u8) -> Option<&'b Value<'a>> {
    fields.iter().find(|(n, _)| *n == id).map(|(_, value)| value)
}

fn int(fields: &[(u8, Value<'_>)], id: u8) -> Option<i64> {
    field(fields, id)?.integer()
}

fn time_from_fit(value: i64) -> Option<f64> {
    (value >= 0 && value <= u32::MAX as i64)
        .then_some((value + FIT_UNIX_EPOCH) as f64)
}

fn position(value: i64, latitude: bool) -> Option<f64> {
    if !(i32::MIN as i64..=i32::MAX as i64).contains(&value) { return None; }
    let degrees = value as f64 / SC_PER_DEGREE;
    if degrees.is_finite() && (if latitude { degrees.abs() <= 90.0 } else { degrees.abs() <= 180.0 }) {
        Some(degrees)
    } else {
        None
    }
}

fn point(fields: &[(u8, Value<'_>)], compressed_time: Option<u32>) -> Option<TrackPoint> {
    let lat = position(int(fields, 0)?, true)?;
    let lon = position(int(fields, 1)?, false)?;
    let ele_m = int(fields, 78).or_else(|| int(fields, 2))
        .map(|v| v as f64 / 5.0 - 500.0);
    let timestamp = int(fields, 253).or(compressed_time.map(i64::from));
    let time = timestamp.and_then(time_from_fit);
    let speed_m_s = int(fields, 73).or_else(|| int(fields, 6))
        .map(|v| v as f64 / 1000.0);
    Some(TrackPoint { lat, lon, ele_m, time, hdop: None, speed_m_s })
}

fn waypoint(name: String, lat: f64, lon: f64, time: Option<f64>) -> Waypoint {
    Waypoint {
        name, lat, lon, ele_m: None, time,
        description: String::new(), bg_color: [0, 255, 255], text_color: [0, 0, 0],
    }
}

/// Read a FIT Activity or Course file, including compressed timestamps and
/// developer-field payloads (which are skipped, not interpreted).
/// Corrupted CRC, truncated messages or unsupported layouts return an error.
pub fn read(bytes: &[u8]) -> Result<GeoData, GeoError> {
    if bytes.len() < 14 || bytes.len() > MAX_FILE_BYTES { return Err(invalid("invalid file length")); }
    let header = bytes[0] as usize;
    if header != 12 && header != 14 { return Err(invalid("unsupported header length")); }
    if &bytes[8..12] != b".FIT" { return Err(invalid("missing .FIT signature")); }
    if header == 14 && crc16(&bytes[..12]) != u16::from_le_bytes([bytes[12], bytes[13]]) {
        return Err(invalid("header CRC mismatch"));
    }
    let data_len = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let end = header.checked_add(data_len).ok_or_else(|| invalid("oversized data"))?;
    if end.checked_add(2) != Some(bytes.len()) { return Err(invalid("data length mismatch")); }
    if crc16(&bytes[..end]) != u16::from_le_bytes([bytes[end], bytes[end + 1]]) {
        return Err(invalid("file CRC mismatch"));
    }
    let mut definitions: [Option<Definition>; 16] = std::array::from_fn(|_| None);
    let mut index = header;
    let mut last_timestamp: Option<u32> = None;
    let mut result = GeoData::default();
    let mut track = Track::default();
    let mut saw_file_id = false;
    let mut fit_type = None;
    let mut pending_pois: Vec<(String, Option<f64>)> = Vec::new();
    let mut lap_ranges: Vec<(f64, f64)> = Vec::new();
    while index < end {
        let rec = bytes[index]; index += 1;
        if rec & 0x80 == 0 && rec & 0x40 != 0 {
            let local = (rec & 0x0f) as usize;
            if end - index < 5 { return Err(invalid("truncated definition")); }
            let architecture = bytes[index + 1];
            if architecture > 1 { return Err(invalid("unsupported architecture")); }
            let little = architecture == 0;
            let global = num(&bytes[index + 2..index + 4], little) as u16;
            let count = bytes[index + 4] as usize;
            index += 5;
            let needed = count.checked_mul(3).ok_or_else(|| invalid("invalid field count"))?;
            if end - index < needed { return Err(invalid("truncated field definition")); }
            let mut fields = Vec::with_capacity(count);
            for _ in 0..count {
                fields.push(FieldDef {
                    number: bytes[index], size: bytes[index + 1] as usize,
                    base_type: bytes[index + 2],
                });
                index += 3;
            }
            let mut developer_sizes = Vec::new();
            if rec & 0x20 != 0 {
                if index >= end { return Err(invalid("missing developer field count")); }
                let dev_count = bytes[index] as usize;
                index += 1;
                let needed = dev_count.checked_mul(3).ok_or_else(|| invalid("invalid developer fields"))?;
                if end - index < needed { return Err(invalid("truncated developer fields")); }
                for _ in 0..dev_count {
                    developer_sizes.push(bytes[index + 1] as usize);
                    index += 3;
                }
            }
            definitions[local] = Some(Definition { global, little, fields, developer_sizes });
            continue;
        }
        let compressed = rec & 0x80 != 0;
        let local = if compressed { (rec >> 5 & 0x03) as usize } else { (rec & 0x0f) as usize };
        let def = definitions[local].as_ref().ok_or_else(|| invalid("data without definition"))?;
        let mut compressed_time = None;
        if compressed {
            if let Some(old) = last_timestamp {
                let low = u32::from(rec & 0x1f);
                let mut next = (old & !0x1f) | low;
                if next < old { next = next.wrapping_add(32); }
                compressed_time = Some(next);
            }
        }
        let mut values = Vec::with_capacity(def.fields.len());
        for f in &def.fields {
            if f.size > end - index { return Err(invalid("truncated message")); }
            values.push((f.number, Value { data: &bytes[index..index + f.size], base: f.base_type, little: def.little }));
            index += f.size;
        }
        for &size in &def.developer_sizes {
            if size > end - index { return Err(invalid("truncated developer message")); }
            index += size;
        }
        if let Some(ts) = int(&values, 253) {
            if let Ok(ts) = u32::try_from(ts) { last_timestamp = Some(ts); }
        } else if let Some(ts) = compressed_time {
            last_timestamp = Some(ts);
        }
        match def.global {
            0 => {
                saw_file_id = true;
                fit_type = int(&values, 0);
            }
            20 => {
                if let Some(p) = point(&values, compressed_time) {
                    if track.segments.is_empty() { track.segments.push(Vec::new()); }
                    track.segments[0].push(p);
                }
            }
            19 => {
                if let Some((start, end)) = int(&values, 2).and_then(time_from_fit)
                    .zip(int(&values, 253).and_then(time_from_fit)) {
                    if end >= start { lap_ranges.push((start, end)); }
                }
            }
            31 => {
                if let Some(n) = field(&values, 5).and_then(Value::text) { track.name = n; }
            }
            32 => {
                let name = field(&values, 6).and_then(Value::text).unwrap_or_default();
                let stamp = int(&values, 1).and_then(time_from_fit);
                let location = int(&values, 2).and_then(|v| position(v, true))
                    .zip(int(&values, 3).and_then(|v| position(v, false)));
                if let Some((lat, lon)) = location {
                    result.waypoints.push(waypoint(name, lat, lon, stamp));
                } else if !name.is_empty() {
                    pending_pois.push((name, stamp));
                }
            }
            _ => {}
        }
    }
    if !saw_file_id { return Err(invalid("missing file_id message")); }
    if !matches!(fit_type, Some(4 | 6)) {
        return Err(invalid("only Activity and Course files are supported"));
    }
    let samples = track.segments.first().map(Vec::as_slice).unwrap_or_default();
    for (name, time) in pending_pois {
        if let Some(t) = time {
            if let Some(point) = samples.iter().find(|p| p.time == Some(t)) {
                result.waypoints.push(waypoint(name, point.lat, point.lon, Some(t)));
                continue;
            }
        }
        result.warnings.push(format!("FIT course point '{name}' without coordinates was skipped"));
    }
    // Multiple laps often represent FIT Course segments (or Activity laps).
    // Retain these boundaries when their timestamps actually partition records.
    if lap_ranges.len() > 1 && !track.segments.is_empty() {
        lap_ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
        let all_points = std::mem::take(&mut track.segments[0]);
        let mut rebuilt: Vec<Vec<TrackPoint>> = vec![Vec::new(); lap_ranges.len()];
        let mut remainder = Vec::new();
        for p in all_points {
            if let Some(t) = p.time {
                if let Some(idx) = lap_ranges.iter().position(|&(a, b)| t >= a && t <= b) {
                    rebuilt[idx].push(p);
                    continue;
                }
            }
            remainder.push(p);
        }
        track.segments = rebuilt.into_iter().filter(|s| !s.is_empty()).collect();
        if !remainder.is_empty() { track.segments.push(remainder); }
    }
    if track.segments.iter().any(|s| !s.is_empty()) { result.tracks.push(track); }
    if result.tracks.is_empty() && result.waypoints.is_empty() {
        return Err(invalid("no geographic records"));
    }
    result.warnings.push("FIT: sensor data and vendor-specific fields are not imported".to_owned());
    Ok(result)
}

fn semicircles(degrees: f64, lat: bool) -> Result<i32, GeoError> {
    if !degrees.is_finite() || degrees.abs() > if lat { 90.0 } else { 180.0 } {
        return Err(invalid("invalid geographic coordinate"));
    }
    // +180 degrees cannot be expressed as an i32 semicircle; normalize to -180.
    let degrees = if !lat && degrees == 180.0 { -180.0 } else { degrees };
    let scaled = (degrees * SC_PER_DEGREE).round();
    if !(i32::MIN as f64..=i32::MAX as f64).contains(&scaled) {
        return Err(invalid("coordinate outside FIT semicircle range"));
    }
    Ok(scaled as i32)
}

fn fit_time(unix: f64) -> Option<u32> {
    let secs = unix.round();
    if !secs.is_finite() || secs < FIT_UNIX_EPOCH as f64 || secs - FIT_UNIX_EPOCH as f64 >= u32::MAX as f64 {
        None
    } else {
        Some((secs as i64 - FIT_UNIX_EPOCH) as u32)
    }
}

fn name_bytes(text: &str, size: usize) -> Vec<u8> {
    let mut v = vec![0u8; size];
    let mut n = 0;
    for ch in text.chars() {
        if ch == '\0' || n + ch.len_utf8() >= size { break; }
        n += ch.encode_utf8(&mut v[n..]).len();
    }
    v
}

struct Writer { body: Vec<u8> }

impl Writer {
    fn definition(&mut self, local: u8, global: u16, fields: &[(u8, u8, u8)]) {
        self.body.push(0x40 | local);
        self.body.push(0); // reserved
        self.body.push(0); // little endian
        self.body.extend_from_slice(&global.to_le_bytes());
        self.body.push(fields.len() as u8);
        for &(number, size, base) in fields { self.body.extend_from_slice(&[number, size, base]); }
    }
    fn data(&mut self, local: u8, bytes: &[u8]) {
        self.body.push(local);
        self.body.extend_from_slice(bytes);
    }
    fn finish(self) -> Result<Vec<u8>, GeoError> {
        let data_size = u32::try_from(self.body.len()).map_err(|_| invalid("course too large"))?;
        let mut out = vec![14, 0x20, 0x43, 0x08]; // protocol 2.0, FIT profile 21.15
        out.extend_from_slice(&data_size.to_le_bytes());
        out.extend_from_slice(b".FIT");
        let hdr_crc = crc16(&out);
        out.extend_from_slice(&hdr_crc.to_le_bytes());
        out.extend_from_slice(&self.body);
        let file_crc = crc16(&out);
        out.extend_from_slice(&file_crc.to_le_bytes());
        Ok(out)
    }
}

fn distance_m(a: &TrackPoint, b: &TrackPoint) -> f64 {
    let lat1 = a.lat.to_radians();
    let lat2 = b.lat.to_radians();
    let dlat = lat2 - lat1;
    let dlon = (b.lon - a.lon).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    12_742_000.0 * h.min(1.0).sqrt().asin()
}

/// Export a single selected track as Garmin Course FIT, optionally with POIs.
/// Course FIT requires timestamps: when absent, a synthetic relative timeline
/// beginning at 2020-01-01 UTC is used. There is no fake heart-rate or sensor data.
/// FIT cannot hold an arbitrary collection of independent tracks/waypoints.
pub fn write(data: &GeoData) -> Result<Vec<u8>, GeoError> {
    if data.tracks.len() != 1 { return Err(invalid("Course export requires exactly one track")); }
    let track = &data.tracks[0];
    let mut records: Vec<&TrackPoint> = Vec::new();
    let mut segment_bounds = Vec::new();
    for seg in &track.segments {
        if seg.is_empty() { continue; }
        let start = records.len();
        records.extend(seg.iter());
        segment_bounds.push((start, records.len() - 1));
    }
    if records.is_empty() { return Err(invalid("cannot export an empty track")); }
    // Validate before writing any data; don't silently skip invalid points.
    for point in &records {
        semicircles(point.lat, true)?;
        semicircles(point.lon, false)?;
    }
    for point in &data.waypoints {
        semicircles(point.lat, true)?;
        semicircles(point.lon, false)?;
    }
    let base = records.iter().filter_map(|p| p.time.and_then(fit_time)).next()
        .unwrap_or(946_771_200); // 2020-01-01 00:00:00 UTC, FIT seconds
    let mut times = Vec::with_capacity(records.len());
    let mut previous = base.saturating_sub(1);
    for p in &records {
        let chosen = p.time.and_then(fit_time).unwrap_or_else(|| previous.saturating_add(1));
        let now = chosen.max(previous.saturating_add(1));
        times.push(now);
        previous = now;
    }
    if times.last() == Some(&u32::MAX) { return Err(invalid("timestamp overflow")); }
    let mut distances = Vec::with_capacity(records.len());
    let mut length = 0.0;
    for (i, point) in records.iter().enumerate() {
        if i > 0 && !segment_bounds.iter().any(|&(start, _)| start == i) {
            length += distance_m(records[i - 1], point);
        }
        if !length.is_finite() || length * 100.0 >= u32::MAX as f64 {
            return Err(invalid("track exceeds FIT distance range"));
        }
        distances.push((length * 100.0).round() as u32);
    }
    let start_time = times[0];
    let end_time = *times.last().unwrap();
    let mut w = Writer { body: Vec::new() };
    // file_id, manufacturer 255 = development, product 1 (local application).
    w.definition(0, 0, &[(0, 1, 0x00), (1, 2, 0x84), (2, 2, 0x84), (4, 4, 0x86)]);
    let mut file_id = vec![6]; // FIT file type: Course
    file_id.extend_from_slice(&255u16.to_le_bytes());
    file_id.extend_from_slice(&1u16.to_le_bytes());
    file_id.extend_from_slice(&start_time.to_le_bytes());
    w.data(0, &file_id);
    w.definition(1, 31, &[(5, 64, 0x07), (4, 1, 0x00)]);
    let title = if track.name.trim().is_empty() { "FoxyXplorer Course" } else { &track.name };
    let mut course = name_bytes(title, 64);
    course.push(0); // generic sport
    w.data(1, &course);
    // Minimum Lap summary fields; positioned before Records as per Course spec.
    w.definition(2, 19, &[(2, 4, 0x86), (253, 4, 0x86),
        (3, 4, 0x85), (4, 4, 0x85), (5, 4, 0x85), (6, 4, 0x85)]);
    for &(start, end) in &segment_bounds {
        let first = records[start];
        let last = records[end];
        let mut lap = Vec::with_capacity(24);
        lap.extend_from_slice(&times[start].to_le_bytes());
        lap.extend_from_slice(&times[end].to_le_bytes());
        for v in [semicircles(first.lat, true)?, semicircles(first.lon, false)?,
            semicircles(last.lat, true)?, semicircles(last.lon, false)?] {
            lap.extend_from_slice(&v.to_le_bytes());
        }
        w.data(2, &lap);
    }
    w.definition(3, 21, &[(253, 4, 0x86), (0, 1, 0x00), (1, 1, 0x00)]);
    let mut event = start_time.to_le_bytes().to_vec();
    event.extend_from_slice(&[0, 0]); // Timer, Start
    w.data(3, &event);
    w.definition(4, 20, &[(253, 4, 0x86), (0, 4, 0x85), (1, 4, 0x85),
        (2, 2, 0x84), (5, 4, 0x86), (6, 2, 0x84)]);
    for (i, p) in records.iter().enumerate() {
        let mut record = Vec::with_capacity(20);
        record.extend_from_slice(&times[i].to_le_bytes());
        record.extend_from_slice(&semicircles(p.lat, true)?.to_le_bytes());
        record.extend_from_slice(&semicircles(p.lon, false)?.to_le_bytes());
        let altitude = p.ele_m.filter(|v| v.is_finite())
            .map(|v| ((v + 500.0) * 5.0).round())
            .filter(|v| (0.0..65535.0).contains(v))
            .map(|v| v as u16).unwrap_or(0xffff);
        record.extend_from_slice(&altitude.to_le_bytes());
        record.extend_from_slice(&distances[i].to_le_bytes());
        let speed = p.speed_m_s.filter(|v| v.is_finite() && *v >= 0.0)
            .map(|v| (v * 1000.0).round())
            .filter(|v| (0.0..65535.0).contains(v))
            .map(|v| v as u16).unwrap_or(0xffff);
        record.extend_from_slice(&speed.to_le_bytes());
        w.data(4, &record);
    }
    if !data.waypoints.is_empty() {
        w.definition(5, 32, &[(1, 4, 0x86), (2, 4, 0x85), (3, 4, 0x85),
            (6, 64, 0x07), (5, 1, 0x00)]);
        for wp in &data.waypoints {
            // Course Point timestamps need to correspond to a course fix.
            let near = records.iter().enumerate().min_by(|(_, a), (_, b)| {
                let da = (a.lat - wp.lat).powi(2) + (a.lon - wp.lon).powi(2);
                let db = (b.lat - wp.lat).powi(2) + (b.lon - wp.lon).powi(2);
                da.total_cmp(&db)
            }).map(|(i, _)| i).unwrap_or(0);
            let mut cp = Vec::with_capacity(77);
            cp.extend_from_slice(&times[near].to_le_bytes());
            cp.extend_from_slice(&semicircles(wp.lat, true)?.to_le_bytes());
            cp.extend_from_slice(&semicircles(wp.lon, false)?.to_le_bytes());
            cp.extend_from_slice(&name_bytes(&wp.name, 64));
            cp.push(0); // generic course point
            w.data(5, &cp);
        }
    }
    let mut event = end_time.to_le_bytes().to_vec();
    event.extend_from_slice(&[0, 4]); // Timer, StopDisableAll
    w.data(3, &event);
    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> GeoData {
        let points = vec![
            TrackPoint { lat: 47.2692, lon: 11.4041, ele_m: Some(620.4),
                time: Some(1_782_400_000.0), speed_m_s: Some(2.75), hdop: None },
            TrackPoint { lat: 47.2700, lon: 11.4060, ele_m: Some(635.2),
                time: Some(1_782_400_015.0), speed_m_s: None, hdop: None },
        ];
        GeoData {
            tracks: vec![Track { name: "Nordkette".into(), segments: vec![points], ..Default::default() }],
            waypoints: vec![waypoint("Aussicht".into(), 47.2700, 11.4060, None)],
            ..Default::default()
        }
    }

    #[test]
    fn crc_matches_garmin_documented_header() {
        // Header example from Garmin's FIT integrity documentation.
        let header = [0x0e, 0x10, 0x43, 0x08, 0x78, 0x06, 0x09, 0x00,
            0x2e, 0x46, 0x49, 0x54];
        assert_eq!(crc16(&header), 0x8596);
    }

    #[test]
    fn accepts_compressed_timestamps_and_unknown_developer_fields() {
        let mut w = Writer { body: Vec::new() };
        w.definition(0, 0, &[(0, 1, 0x00)]);
        w.data(0, &[4]); // activity
        w.definition(1, 20, &[(253, 4, 0x86), (0, 4, 0x85), (1, 4, 0x85)]);
        let first_time = 900_000_000u32;
        let mut first = first_time.to_le_bytes().to_vec();
        first.extend_from_slice(&semicircles(47.0, true).unwrap().to_le_bytes());
        first.extend_from_slice(&semicircles(11.0, false).unwrap().to_le_bytes());
        w.data(1, &first);
        // Redefine the same local type without a timestamp field; compressed
        // headers then reconstruct it from the preceding full timestamp.
        w.definition(1, 20, &[(0, 4, 0x85), (1, 4, 0x85)]);
        w.body.push(0xa0 | ((first_time.wrapping_add(1) & 0x1f) as u8));
        w.body.extend_from_slice(&semicircles(47.1, true).unwrap().to_le_bytes());
        w.body.extend_from_slice(&semicircles(11.1, false).unwrap().to_le_bytes());
        // Definition and message with an unknown developer field.
        w.body.extend_from_slice(&[0x62, 0, 0]); // local=2, definition, dev flag, LE
        w.body.extend_from_slice(&20u16.to_le_bytes());
        w.body.extend_from_slice(&[2, 0, 4, 0x85, 1, 4, 0x85]);
        w.body.extend_from_slice(&[1, 7, 3, 0]); // 3-byte developer data field
        let mut next = semicircles(47.2, true).unwrap().to_le_bytes().to_vec();
        next.extend_from_slice(&semicircles(11.2, false).unwrap().to_le_bytes());
        next.extend_from_slice(&[11, 12, 13]);
        w.data(2, &next);
        let back = read(&w.finish().unwrap()).unwrap();
        let points = &back.tracks[0].segments[0];
        assert_eq!(points.len(), 3);
        assert_eq!(points[1].time.unwrap() - points[0].time.unwrap(), 1.0);
        assert!((points[2].lat - 47.2).abs() < 1e-6);
    }

    #[test]
    fn accepts_big_endian_message_definitions() {
        let mut w = Writer { body: Vec::new() };
        w.definition(0, 0, &[(0, 1, 0)]);
        w.data(0, &[6]); // course
        w.body.extend_from_slice(&[0x41, 0, 1, 0, 20, 3]);
        w.body.extend_from_slice(&[253, 4, 0x86, 0, 4, 0x85, 1, 4, 0x85]);
        let mut sample = 900_000_000u32.to_be_bytes().to_vec();
        sample.extend_from_slice(&semicircles(47.0, true).unwrap().to_be_bytes());
        sample.extend_from_slice(&semicircles(11.0, false).unwrap().to_be_bytes());
        w.data(1, &sample);
        let data = read(&w.finish().unwrap()).unwrap();
        assert_eq!(data.tracks[0].segments[0][0].time, Some((900_000_000 + FIT_UNIX_EPOCH) as f64));
        assert!((data.tracks[0].segments[0][0].lon - 11.0).abs() < 1e-6);
    }

    #[test]
    fn course_roundtrip_coordinates_height_speed_time_and_poi() {
        let original = sample();
        let bytes = write(&original).unwrap();
        assert_eq!(&bytes[8..12], b".FIT");
        let read_back = read(&bytes).unwrap();
        assert_eq!(read_back.tracks[0].name, "Nordkette");
        let points = &read_back.tracks[0].segments[0];
        assert_eq!(points.len(), 2);
        assert!((points[0].lat - 47.2692).abs() < 1e-6);
        assert!((points[1].lon - 11.4060).abs() < 1e-6);
        assert!((points[0].ele_m.unwrap() - 620.4).abs() < 0.21);
        assert_eq!(points[0].time, Some(1_782_400_000.0));
        assert_eq!(points[0].speed_m_s, Some(2.75));
        assert_eq!(read_back.waypoints[0].name, "Aussicht");
    }

    #[test]
    fn multiple_segments_use_multiple_laps_without_bridging_distance() {
        let mut original = sample();
        let second = original.tracks[0].segments[0].split_off(1);
        original.tracks[0].segments.push(second);
        let back = read(&write(&original).unwrap()).unwrap();
        assert_eq!(back.tracks[0].segments.len(), 2);
        assert_eq!(back.tracks[0].segments[0].len(), 1);
        assert_eq!(back.tracks[0].segments[1].len(), 1);
    }

    #[test]
    fn checksum_and_truncation_are_detected() {
        let mut bytes = write(&sample()).unwrap();
        bytes[29] ^= 1;
        assert!(read(&bytes).is_err());
        let mut bytes = write(&sample()).unwrap();
        bytes.pop();
        assert!(read(&bytes).is_err());
    }

    #[test]
    fn requires_one_nonempty_track() {
        assert!(write(&GeoData::default()).is_err());
        let mut data = sample();
        data.tracks.push(Track::default());
        assert!(write(&data).is_err());
    }

    #[test]
    fn missing_altitude_is_not_invented() {
        let mut data = sample();
        data.tracks[0].segments[0][0].ele_m = None;
        assert_eq!(read(&write(&data).unwrap()).unwrap().tracks[0].segments[0][0].ele_m, None);
    }

    #[test]
    fn normalizes_missing_timestamp_with_relative_course_clock() {
        let mut data = sample();
        for p in &mut data.tracks[0].segments[0] { p.time = None; }
        let back = read(&write(&data).unwrap()).unwrap();
        let points = &back.tracks[0].segments[0];
        assert_eq!(points[1].time.unwrap() - points[0].time.unwrap(), 1.0);
    }
}
