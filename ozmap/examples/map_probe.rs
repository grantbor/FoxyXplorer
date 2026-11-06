use ozmap::MapFile;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 2 {
        eprintln!("использование: map_probe файл.map [широта долгота]");
        std::process::exit(2);
    }
    let m = MapFile::open(&a[1]).unwrap_or_else(|e| {
        eprintln!("ошибка: {e}");
        std::process::exit(1)
    });
    println!("растр: {}", m.image_file);
    println!("размер: {:?} x {:?}, проекция: {}", m.width, m.height, m.projection);
    println!("опорных точек: {}", m.points.len());
    let g = m.georef().unwrap_or_else(|e| {
        eprintln!("привязка не удалась: {e}");
        std::process::exit(1)
    });
    for (i, r) in g.residuals(&m.points).iter().enumerate() {
        println!("  точка {}: невязка {:.3} px", i + 1, r);
    }
    if let (Some(lat), Some(mm)) = (m.points.first().map(|p| p.lat), m.mm1b) {
        println!("м/пиксель: расчёт {:?}, MM1B {mm}", g.meters_per_pixel(lat));
    }
    for (i, lon, lat) in &m.mmp_ll {
        if let Some((_, x, y)) = m.mmp_xy.iter().find(|(j, _, _)| j == i) {
            let (px, py) = g.lonlat_to_pixel(*lon, *lat);
            println!("  MMP {i}: ожидалось ({x}, {y}), получено ({px:.2}, {py:.2})");
        }
    }
    if a.len() >= 4 {
        let lat: f64 = a[2].parse().expect("широта");
        let lon: f64 = a[3].parse().expect("долгота");
        let (x, y) = g.lonlat_to_pixel(lon, lat);
        println!("({lat}, {lon}) -> пиксель ({x:.1}, {y:.1})");
    }
}
