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
