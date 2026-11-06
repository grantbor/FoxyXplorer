use geoio::text::Encoding;
use geoio::{gpx, ms, plt, wpt};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("использование: gpx2all вход.gpx выходная_папка [--utf8]");
        return;
    }
    let enc = if a.iter().any(|x| x == "--utf8") { Encoding::Utf8 } else { Encoding::Cp1251 };
    let data = gpx::read(&std::fs::read(&a[1]).expect("чтение GPX")).expect("разбор GPX");
    for w in &data.warnings {
        eprintln!("предупреждение: {w}");
    }
    std::fs::create_dir_all(&a[2]).expect("создание папки");
    let stem = Path::new(&a[1]).file_stem().unwrap().to_string_lossy().to_string();
    let out = |name: String| Path::new(&a[2]).join(name);

    if !data.waypoints.is_empty() {
        std::fs::write(out(format!("{stem}.wpt")), wpt::write(&data.waypoints, enc)).unwrap();
        println!("{stem}.wpt: {} точек", data.waypoints.len());
    }
    for (i, t) in data.tracks.iter().enumerate() {
        let file = if data.tracks.len() == 1 { format!("{stem}.plt") } else { format!("{stem}_{}.plt", i + 1) };
        std::fs::write(out(file.clone()), plt::write(t, enc)).unwrap();
        println!("{file}: «{}», {} точек", t.name, t.segments.iter().map(Vec::len).sum::<usize>());
    }
    std::fs::write(out(format!("{stem}.ms")), ms::write(&data, &stem, &ms::MsStyle::default())).unwrap();
    println!("{stem}.ms записан");
}
