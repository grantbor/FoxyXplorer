use iced::Point;

pub const R: f64 = 6_378_137.0;

pub fn merc_of(lon: f64, lat: f64) -> (f64, f64) {
    (
        R * lon.to_radians(),
        R * (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.0).tan().ln(),
    )
}

pub fn lonlat_of(x: f64, y: f64) -> (f64, f64) {
    (
        (x / R).to_degrees(),
        (2.0 * (y / R).exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees(),
    )
}

pub struct Affine {
    pub x0: f64,
    pub y0: f64,
    pub ax: f64,
    pub ay: f64,
    pub bx: f64,
    pub by: f64,
}

pub fn affine(g: &ozmap::Georef, cx: f64, cy: f64) -> Affine {
    const D: f64 = 1000.0;
    let at = |mx: f64, my: f64| {
        let (lon, lat) = lonlat_of(mx, my);
        g.lonlat_to_pixel(lon, lat)
    };
    let (x0, y0) = at(cx, cy);
    let (x1, y1) = at(cx + D, cy);
    let (x2, y2) = at(cx, cy + D);
    Affine {
        x0,
        y0,
        ax: (x1 - x0) / D,
        ay: (y1 - y0) / D,
        bx: (x2 - x0) / D,
        by: (y2 - y0) / D,
    }
}

pub fn true_bearing(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat1 = lat1.to_radians();
    let lat2 = lat2.to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let y = dlon.sin() * lat2.cos();
    let x = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * dlon.cos();
    y.atan2(x).to_degrees()
}

pub (crate) fn dist_to_segment(p: Point, a: Point, b: Point) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let l2 = dx * dx + dy * dy;
    if l2 == 0.0 { return (p.x - a.x).hypot(p.y - a.y); }
    let t = ((p.x - a.x) * dx + (p.y - a.y) * dy) / l2;
    let t = t.clamp(0.0, 1.0);
    let proj_x = a.x + t * dx;
    let proj_y = a.y + t * dy;
    (p.x - proj_x).hypot(p.y - proj_y)
}
