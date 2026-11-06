use geoio::text::Encoding;
use geoio::{gpx, ms, plt, wpt, GeoData, TrackPoint, Waypoint};
use std::path::PathBuf;

fn dir() -> Option<PathBuf> {
    std::env::var_os("GEOIO_TEST_DIR").map(PathBuf::from)
}

fn line_of(d: &GeoData) -> Vec<TrackPoint> {
    d.tracks.iter().flat_map(|t| t.segments.iter().flatten().cloned()).collect()
}

fn same_nodes(a: &[Waypoint], b: &[Waypoint], what: &str) {
    assert_eq!(a.len(), b.len(), "{what}: число узлов");
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        assert_eq!(x.name, y.name, "{what}: имя узла №{i}");
        assert!(
            (x.lat - y.lat).abs() < 2e-6 && (x.lon - y.lon).abs() < 2e-6,
            "{what}: узел {} ({}, {}) и ({}, {})", x.name, x.lat, x.lon, y.lat, y.lon
        );
    }
}

fn same_line(a: &[TrackPoint], b: &[TrackPoint], what: &str) {
    assert_eq!(a.len(), b.len(), "{what}: число вершин линии");
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        assert!(
            (x.lat - y.lat).abs() < 2e-6 && (x.lon - y.lon).abs() < 2e-6,
            "{what}: вершина {i}: ({}, {}) и ({}, {})", x.lat, x.lon, y.lat, y.lon
        );
    }
}

// Времена в файлах сетки служебные (GPX и PLT расходятся на секунды),
// поэтому сравниваются только имена и координаты.
fn check(stem: &str, nodes: usize, line: usize) {
    let Some(d) = dir() else {
        eprintln!("GEOIO_TEST_DIR не задан, тест {stem} пропущен");
        return;
    };
    let load = |ext: &str| {
        std::fs::read(d.join(format!("{stem}{ext}"))).unwrap_or_else(|e| panic!("{stem}{ext}: {e}"))
    };

    let g = gpx::read(&load(".gpx")).expect("GPX");
    assert!(g.warnings.is_empty(), "GPX: {:?}", g.warnings);
    assert_eq!(g.waypoints.len(), nodes, "узлов в GPX");
    let gline = line_of(&g);
    assert_eq!(gline.len(), line, "вершин линии в GPX");

    let w = wpt::read(&load(".wpt")).expect("WPT");
    assert!(w.warnings.is_empty(), "WPT: {:?}", w.warnings);
    same_nodes(&g.waypoints, &w.waypoints, "GPX и WPT");

    let p = plt::read(&load(".plt")).expect("PLT");
    assert!(p.warnings.is_empty(), "PLT: {:?}", p.warnings);
    same_line(&gline, &line_of(&p), "GPX и PLT");

    for suffix in [".ms", "_gpx2ms.ms"] {
        let path = d.join(format!("{stem}{suffix}"));
        if !path.exists() {
            println!("{stem}{suffix}: файла нет, пропущено");
            continue;
        }
        let m = ms::read(&std::fs::read(&path).unwrap()).expect("ms");
        assert!(m.warnings.is_empty(), "{suffix}: {:?}", m.warnings);
        same_nodes(&g.waypoints, &m.waypoints, &format!("GPX и {suffix}"));
        same_line(&gline, &line_of(&m), &format!("GPX и {suffix}"));
        println!("{stem}{suffix}: совпало");
    }

    // наши писатели: запись и чтение обратно
    let out = ms::write(&g, stem, &ms::MsStyle::default());
    let back = ms::read(out.as_bytes()).expect("наш ms");
    same_nodes(&g.waypoints, &back.waypoints, "GPX и наш ms");
    same_line(&gline, &line_of(&back), "GPX и наш ms");
    let w2 = wpt::read(&wpt::write(&g.waypoints, Encoding::Cp1251)).expect("наш wpt");
    same_nodes(&g.waypoints, &w2.waypoints, "GPX и наш wpt");

    println!("{stem}: узлов {nodes}, вершин линии {line}, все форматы совпали");
}

#[test]
fn novosibirsk_250m() {
    check("2023-09-21_Novosibirsk_250m", 374, 77);
}

#[test]
fn tatchikha_500m() {
    check("2023-07-08_Tatchikha_500m", 900, 121);
}
