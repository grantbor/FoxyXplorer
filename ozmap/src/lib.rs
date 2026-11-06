//! Парсер .map (OziExplorer) и привязка пикселей к координатам.
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};
use std::path::Path;

const R: f64 = 6_378_137.0;

#[derive(Debug, thiserror::Error)]
pub enum MapError {
    #[error("ошибка чтения файла: {0}")]
    Io(#[from] std::io::Error),
    #[error("неподдерживаемая проекция: {0}")]
    Projection(String),
    #[error("мало опорных точек: {0} (нужно не менее 3)")]
    FewPoints(usize),
    #[error("опорные точки лежат на одной линии")]
    Degenerate,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalPoint {
    pub x: f64,
    pub y: f64,
    pub lon: f64,
    pub lat: f64,
}

#[derive(Debug, Clone, Default)]
pub struct MapFile {
    /// Имя файла растра без пути (3-я строка .map).
    pub image_file: String,
    pub projection: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Метры на пиксель (MM1B), если указано.
    pub mm1b: Option<f64>,
    pub points: Vec<CalPoint>,
    /// MMPXY: (номер, x, y)
    pub mmp_xy: Vec<(usize, f64, f64)>,
    /// MMPLL: (номер, lon, lat)
    pub mmp_ll: Vec<(usize, f64, f64)>,
}

fn cp1251(c: u8) -> char {
    match c {
        0..=0x7f => c as char,
        0xA8 => 'Ё',
        0xB8 => 'ё',
        0xC0..=0xFF => char::from_u32(0x410 + (c as u32 - 0xC0)).unwrap_or('?'),
        _ => '?',
    }
}

fn decode(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => b.iter().map(|&c| cp1251(c)).collect(),
    }
}

fn parse_point(line: &str) -> Option<CalPoint> {
    let f: Vec<&str> = line.split(',').map(str::trim).collect();
    if f.len() < 12 {
        return None;
    }
    let x: f64 = f[2].parse().ok()?;
    let y: f64 = f[3].parse().ok()?;
    let lat_d: f64 = f[6].parse().ok()?;
    let lat_m: f64 = f[7].parse().ok()?;
    let lon_d: f64 = f[9].parse().ok()?;
    let lon_m: f64 = f[10].parse().ok()?;
    let ls = if f[8].eq_ignore_ascii_case("S") { -1.0 } else { 1.0 };
    let os = if f[11].eq_ignore_ascii_case("W") { -1.0 } else { 1.0 };
    Some(CalPoint {
        x,
        y,
        lat: ls * (lat_d.abs() + lat_m / 60.0),
        lon: os * (lon_d.abs() + lon_m / 60.0),
    })
}

impl MapFile {
    pub fn open(path: impl AsRef<Path>) -> Result<MapFile, MapError> {
        let bytes = std::fs::read(path)?;
        Ok(MapFile::parse(&decode(&bytes)))
    }

    pub fn parse(text: &str) -> MapFile {
        let text = text.trim_start_matches('\u{feff}');
        let lines: Vec<&str> = text.lines().collect();
        let mut m = MapFile::default();
        if let Some(l) = lines.get(2) {
            m.image_file = l.trim().rsplit(['\\', '/']).next().unwrap_or("").to_string();
        }
        for line in &lines {
            let line = line.trim();
            let f: Vec<&str> = line.split(',').map(str::trim).collect();
            let head = f[0];
            if head.len() > 5 && head.starts_with("Point") && head[5..].chars().all(|c| c.is_ascii_digit()) {
                if let Some(p) = parse_point(line) {
                    m.points.push(p);
                }
            } else if head == "Map Projection" {
                m.projection = f.get(1).unwrap_or(&"").to_string();
            } else if head == "IWH" && f.len() >= 4 {
                m.width = f[2].parse().ok();
                m.height = f[3].parse().ok();
            } else if head == "MM1B" && f.len() >= 2 {
                m.mm1b = f[1].parse().ok();
            } else if (head == "MMPXY" || head == "MMPLL") && f.len() >= 4 {
                if let (Ok(i), Ok(a), Ok(b)) =
                    (f[1].parse::<usize>(), f[2].parse::<f64>(), f[3].parse::<f64>())
                {
                    if head == "MMPXY" {
                        m.mmp_xy.push((i, a, b));
                    } else {
                        m.mmp_ll.push((i, a, b));
                    }
                }
            }
        }
        m
    }

    pub fn georef(&self) -> Result<Georef, MapError> {
        Georef::fit(&self.projection, &self.points)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Mercator,
    LatLon,
}

fn project(k: Kind, lon: f64, lat: f64) -> (f64, f64) {
    match k {
        Kind::Mercator => (
            R * lon.to_radians(),
            R * (FRAC_PI_4 + lat.to_radians() / 2.0).tan().ln(),
        ),
        Kind::LatLon => (lon, lat),
    }
}

fn unproject(k: Kind, x: f64, y: f64) -> (f64, f64) {
    match k {
        Kind::Mercator => (
            (x / R).to_degrees(),
            (2.0 * (y / R).exp().atan() - FRAC_PI_2).to_degrees(),
        ),
        Kind::LatLon => (x, y),
    }
}

/// Аффинная привязка: пиксель = A * (проекция - центр) + центр_пикселей.
#[derive(Debug, Clone)]
pub struct Georef {
    kind: Kind,
    mx: f64,
    my: f64,
    mpx: f64,
    mpy: f64,
    a: f64,
    b: f64,
    d: f64,
    e: f64,
}

impl Georef {
    pub fn fit(projection: &str, pts: &[CalPoint]) -> Result<Georef, MapError> {
        let p = projection.to_lowercase();
        let kind = if p.contains("transverse") {
            return Err(MapError::Projection(projection.to_string()));
        } else if p.contains("mercator") {
            Kind::Mercator
        } else if p.contains("latitude") {
            Kind::LatLon
        } else {
            return Err(MapError::Projection(projection.to_string()));
        };
        if pts.len() < 3 {
            return Err(MapError::FewPoints(pts.len()));
        }
        let n = pts.len() as f64;
        let xy: Vec<(f64, f64)> = pts.iter().map(|q| project(kind, q.lon, q.lat)).collect();
        let mx = xy.iter().map(|v| v.0).sum::<f64>() / n;
        let my = xy.iter().map(|v| v.1).sum::<f64>() / n;
        let mpx = pts.iter().map(|q| q.x).sum::<f64>() / n;
        let mpy = pts.iter().map(|q| q.y).sum::<f64>() / n;

        let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
        let (mut sxp, mut syp, mut sxq, mut syq) = (0.0, 0.0, 0.0, 0.0);
        for (q, (x, y)) in pts.iter().zip(&xy) {
            let (dx, dy) = (x - mx, y - my);
            let (dp, dq) = (q.x - mpx, q.y - mpy);
            sxx += dx * dx;
            sxy += dx * dy;
            syy += dy * dy;
            sxp += dx * dp;
            syp += dy * dp;
            sxq += dx * dq;
            syq += dy * dq;
        }
        let det = sxx * syy - sxy * sxy;
        if !det.is_finite() || det.abs() <= 1e-12 * sxx * syy {
            return Err(MapError::Degenerate);
        }
        Ok(Georef {
            kind,
            mx,
            my,
            mpx,
            mpy,
            a: (sxp * syy - sxy * syp) / det,
            b: (sxx * syp - sxy * sxp) / det,
            d: (sxq * syy - sxy * syq) / det,
            e: (sxx * syq - sxy * sxq) / det,
        })
    }

    pub fn lonlat_to_pixel(&self, lon: f64, lat: f64) -> (f64, f64) {
        let (x, y) = project(self.kind, lon, lat);
        let (dx, dy) = (x - self.mx, y - self.my);
        (
            self.a * dx + self.b * dy + self.mpx,
            self.d * dx + self.e * dy + self.mpy,
        )
    }

    pub fn pixel_to_lonlat(&self, px: f64, py: f64) -> (f64, f64) {
        let det = self.a * self.e - self.b * self.d;
        let (u, v) = (px - self.mpx, py - self.mpy);
        let dx = (self.e * u - self.b * v) / det;
        let dy = (-self.d * u + self.a * v) / det;
        unproject(self.kind, dx + self.mx, dy + self.my)
    }

    /// Невязка в пикселях по каждой опорной точке.
    pub fn residuals(&self, pts: &[CalPoint]) -> Vec<f64> {
        pts.iter()
            .map(|q| {
                let (x, y) = self.lonlat_to_pixel(q.lon, q.lat);
                ((x - q.x).powi(2) + (y - q.y).powi(2)).sqrt()
            })
            .collect()
    }

    /// Метры на пиксель на данной широте (только для Mercator).
    pub fn meters_per_pixel(&self, lat: f64) -> Option<f64> {
        if self.kind != Kind::Mercator {
            return None;
        }
        let det = (self.a * self.e - self.b * self.d).abs();
        Some(lat.to_radians().cos() / det.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP: &str = "OziExplorer Map Data File Version 2.2\r
2023-09-21_Novosibirsk_Topo_OTM_z18.png\r
D:\\LA\\MAPS\\2023-09-21_Novosibirsk_Topo_OTM_z18.ozf2\r
1 ,Map Code,\r
WGS 84,,   0.0000,   0.0000,WGS 84\r
Reserved 1\r
Reserved 2\r
Magnetic Variation,,,E\r
Map Projection,Mercator,PolyCal,No,AutoCalOnly,No,BSBUseWPX,No\r
Point01,xy,    0, 0,in, deg, 54, 58.4041,N, 83, 3.3695,E, grid,   ,           ,           ,N\r
Point02,xy,    3176, 0,in, deg, 54, 58.4041,N, 83, 5.4146,E, grid,   ,           ,           ,N\r
Point03,xy,    6353, 0,in, deg, 54, 58.4041,N, 83, 7.4591,E, grid,   ,           ,           ,N\r
Point04,xy,    0, 4298,in, deg, 54, 56.8156,N, 83, 3.3695,E, grid,   ,           ,           ,N\r
Point05,xy,    3176, 4298,in, deg, 54, 56.8156,N, 83, 5.4146,E, grid,   ,           ,           ,N\r
Point06,xy,    6353, 4298,in, deg, 54, 56.8156,N, 83, 7.4591,E, grid,   ,           ,           ,N\r
Point07,xy,    0, 8596,in, deg, 54, 55.2260,N, 83, 3.3695,E, grid,   ,           ,           ,N\r
Point08,xy,    3176, 8596,in, deg, 54, 55.2260,N, 83, 5.4146,E, grid,   ,           ,           ,N\r
Point09,xy,    6353, 8596,in, deg, 54, 55.2260,N, 83, 7.4591,E, grid,   ,           ,           ,N\r
Point10,xy,     ,     ,in, deg,    ,        ,N,    ,        ,W, grid,   ,           ,           ,N\r
MMPXY,1,0,0\r
MMPLL,1, 83.056158, 54.973402\r
MM1B,0.685945\r
IWH,Map Image Width/Height,6353,8596\r
";

    #[test]
    fn parses_header_and_points() {
        let m = MapFile::parse(MAP);
        assert_eq!(m.image_file, "2023-09-21_Novosibirsk_Topo_OTM_z18.ozf2");
        assert_eq!(m.projection, "Mercator");
        assert_eq!((m.width, m.height), (Some(6353), Some(8596)));
        assert_eq!(m.points.len(), 9);
        assert_eq!(m.mm1b, Some(0.685945));
        assert!((m.points[0].lat - 54.973402).abs() < 1e-6);
        assert!((m.points[0].lon - 83.056158).abs() < 1e-6);
    }

    #[test]
    fn fit_is_sub_pixel() {
        let m = MapFile::parse(MAP);
        let g = m.georef().unwrap();
        let worst = g.residuals(&m.points).into_iter().fold(0.0, f64::max);
        assert!(worst < 1.5, "наибольшая невязка {worst} px");
    }

    #[test]
    fn corners_and_roundtrip() {
        let m = MapFile::parse(MAP);
        let g = m.georef().unwrap();
        let (lon, lat) = g.pixel_to_lonlat(0.0, 0.0);
        assert!((lon - 83.056158).abs() < 1e-5 && (lat - 54.973402).abs() < 1e-5);
        let (x, y) = g.lonlat_to_pixel(lon, lat);
        assert!(x.abs() < 1e-6 && y.abs() < 1e-6);
    }

    #[test]
    fn grid_node_a1_lands_where_expected() {
        // Узел A1 сетки (54.970978, 83.058272); ожидание (197, 393) px
        // получено ручной оценкой, допуск 3 px.
        let m = MapFile::parse(MAP);
        let g = m.georef().unwrap();
        let (x, y) = g.lonlat_to_pixel(83.058272, 54.970978);
        assert!((x - 197.0).abs() < 3.0 && (y - 393.0).abs() < 3.0, "({x}, {y})");
    }
}
