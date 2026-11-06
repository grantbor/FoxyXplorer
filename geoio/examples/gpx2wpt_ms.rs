use geoio::text::Encoding;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("использование: gpx2wpt_ms in.gpx out_без_расширения");
        return;
    }
    let data = geoio::gpx::read(&std::fs::read(&a[1]).expect("чтение gpx")).expect("разбор gpx");
    for w in &data.warnings {
        eprintln!("предупреждение: {w}");
    }
    let name = std::path::Path::new(&a[2])
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("map")
        .to_string();
    std::fs::write(format!("{}.wpt", a[2]), geoio::wpt::write(&data.waypoints, Encoding::Cp1251))
        .expect("запись wpt");
    std::fs::write(format!("{}.ms", a[2]), geoio::ms::write(&data, &name, &geoio::ms::MsStyle::default()))
        .expect("запись ms");
    println!("записано: {} точек, {} треков", data.waypoints.len(), data.tracks.len());
}
