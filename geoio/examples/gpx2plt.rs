use geoio::text::Encoding;
use geoio::{gpx, plt};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("использование: gpx2plt in.gpx out.plt [новое_имя] [--utf8]");
        return;
    }
    let data = gpx::read(&std::fs::read(&a[1]).expect("чтение файла")).expect("разбор GPX");
    for w in &data.warnings {
        eprintln!("предупреждение: {w}");
    }
    let enc = if a.iter().any(|x| x == "--utf8") { Encoding::Utf8 } else { Encoding::Cp1251 };
    let name = a.get(3).filter(|s| !s.starts_with("--"));
    for (i, t) in data.tracks.iter().enumerate() {
        let mut t = t.clone();
        if let Some(n) = name {
            t.name = n.clone();
        }
        let path = if data.tracks.len() == 1 { a[2].clone() } else { format!("{}.{}", a[2], i + 1) };
        std::fs::write(&path, plt::write(&t, enc)).expect("запись файла");
        let n: usize = t.segments.iter().map(Vec::len).sum();
        println!("записан {path}: «{}», {n} точек", t.name);
    }
}
