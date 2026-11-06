use geoio::text::Encoding;
use geoio::{gpx, plt, TrackPoint};
use std::path::PathBuf;

fn dir() -> Option<PathBuf> {
    std::env::var_os("GEOIO_TEST_DIR").map(PathBuf::from)
}

fn close(a: &TrackPoint, b: &TrackPoint, what: &str, i: usize) {
    assert!((a.lat - b.lat).abs() < 1e-6, "{what}, точка {i}: lat {} и {}", a.lat, b.lat);
    assert!((a.lon - b.lon).abs() < 1e-6, "{what}, точка {i}: lon {} и {}", a.lon, b.lon);
    match (a.ele_m, b.ele_m) {
        (Some(x), Some(y)) => assert!((x - y).abs() < 0.06, "{what}, точка {i}: высота {x} и {y}"),
        (None, None) => {}
        _ => panic!("{what}, точка {i}: высота есть только в одном файле"),
    }
    match (a.time, b.time) {
        (Some(x), Some(y)) => assert!((x - y).abs() < 0.5, "{what}, точка {i}: время {x} и {y}"),
        (None, None) => {}
        _ => panic!("{what}, точка {i}: время есть только в одном файле"),
    }
}

#[test]
fn garmin_gpx_vs_ozi_plt() {
    let Some(d) = dir() else {
        eprintln!("GEOIO_TEST_DIR не задан — тест пропущен");
        return;
    };
    let g = gpx::read(&std::fs::read(d.join("Трек_2026-10-24-ЛИСА4.gpx")).unwrap()).unwrap();
    let o = plt::read(&std::fs::read(d.join("20261024_Lisa4.plt")).unwrap()).unwrap();
    assert!(g.warnings.is_empty(), "GPX: {:?}", g.warnings);
    assert!(o.warnings.is_empty(), "PLT: {:?}", o.warnings);

    let (tg, to) = (&g.tracks[0], &o.tracks[0]);
    assert_eq!(tg.name, "2026-10-24-ЛИСА4");
    assert_eq!(to.name, "20261024_Lisa4");
    assert_eq!((to.style.width, to.style.color), (2, 255));
    assert_eq!((tg.segments.len(), to.segments.len()), (1, 1));
    assert_eq!(tg.segments[0].len(), 313);
    assert_eq!(to.segments[0].len(), 313);

    for (i, (a, b)) in tg.segments[0].iter().zip(&to.segments[0]).enumerate() {
        close(a, b, "GPX и PLT из Ozi", i);
    }

    // наша запись PLT, прочитанная обратно, совпадает с PLT из Ozi
    let back = plt::read(&plt::write(tg, Encoding::Cp1251)).unwrap();
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    let bt = &back.tracks[0];
    assert_eq!(bt.name, tg.name);
    assert_eq!(bt.segments.len(), 1);
    for (i, (a, b)) in bt.segments[0].iter().zip(&to.segments[0]).enumerate() {
        close(a, b, "наш PLT и PLT из Ozi", i);
    }
}
