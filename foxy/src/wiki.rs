//! Неинтерактивный слой точек Wiki из WPT, соседнего с GPX-сеткой комплекта.
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct WikiPoint {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
}

/// Для `2026-11-24_Vyuny_500m.gpx` ищем два допустимых файла в той же папке.
/// Нечувствительно к регистру расширения и служебных суффиксов.
fn candidate_paths(grid_gpx: &Path) -> Result<Vec<PathBuf>, String> {
    let Some(stem) = grid_gpx.file_stem().and_then(|s| s.to_str()) else {
        return Ok(Vec::new());
    };
    let Some((kit_prefix, size)) = stem.rsplit_once('_') else {
        return Ok(Vec::new());
    };
    let distance = size.to_ascii_lowercase();
    let distance = distance.strip_suffix("km").or_else(|| distance.strip_suffix('m'));
    if !distance.is_some_and(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())) {
        return Ok(Vec::new());
    }
    let parent = grid_gpx.parent().ok_or("У GPX-сетки нет родительской папки")?;
    let entries = std::fs::read_dir(parent)
        .map_err(|e| format!("{}: {e}", parent.display()))?;
    let names: Vec<(String, PathBuf)> = entries.filter_map(|entry| {
        let p = entry.ok()?.path();
        if !p.is_file() { return None; }
        Some((p.file_name()?.to_str()?.to_lowercase(), p))
    }).collect();
    let mut matches = Vec::new();
    // Оба варианта содержат одни и те же объекты: приоритет Satell.
    for variant in ["Satell", "Topo"] {
        let wanted = format!("{kit_prefix}_Wiki_{variant}_4ozi.wpt").to_lowercase();
        if let Some((_, path)) = names.iter().find(|(name, _)| name.as_str() == wanted.as_str()) {
            matches.push(path.clone());
        }
    }
    Ok(matches)
}

/// Не смешиваем Wiki с обычными пользовательскими Waypoint: только имя и геопозиция.
/// При отсутствии подходящего WPT получаем пустой список (не ошибка).
pub(crate) fn load_for_grid(grid_gpx: &Path) -> Result<Vec<WikiPoint>, String> {
    let candidates = candidate_paths(grid_gpx)?;
    let mut failures = Vec::new();
    for path in candidates {
        let loaded = std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| geoio::wpt::read(&bytes).map_err(|e| e.to_string()));
        match loaded {
            Ok(data) => {
                let points: Vec<_> = data.waypoints.into_iter()
                    .filter(|w| !w.name.trim().is_empty())
                    .map(|w| WikiPoint { name: w.name, lat: w.lat, lon: w.lon })
                    .collect();
                if !points.is_empty() { return Ok(points); }
                failures.push(format!("{}: нет именованных точек", path.display()));
            }
            Err(err) => failures.push(format!("{}: {err}", path.display())),
        }
    }
    if failures.is_empty() { Ok(Vec::new()) } else { Err(failures.join("; ")) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("foxy-wiki-{}-{id}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn path(&self) -> &Path { &self.0 }
    }
    impl Drop for TempDir {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }

    fn wpt(name: &str) -> String {
        format!("OziExplorer Waypoint File Version 1.1\r\nWGS 84\r\nReserved 2\r\ngarmin\r\n1,{name},55.123,83.456,0,0,1,3,0,0,,,,,\r\n")
    }

    #[test]
    fn finds_matching_satell_and_reads_point() {
        let dir = TempDir::new();
        let gpx = dir.path().join("2026-11-24_Vyuny_500m.gpx");
        std::fs::write(dir.path().join("2026-11-24_Vyuny_Wiki_Satell_4ozi.WPT"), wpt("Дом")).unwrap();
        std::fs::write(dir.path().join("2026-11-24_Vyuny_Wiki_Topo_4ozi.wpt"), wpt("Лес")).unwrap();
        let points = load_for_grid(&gpx).unwrap();
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].name, "Дом");
        assert_eq!(points[0].lat, 55.123);
        assert_eq!(points[0].lon, 83.456);
    }

    #[test]
    fn falls_back_to_topo_and_ignores_unrelated_files() {
        let dir = TempDir::new();
        let gpx = dir.path().join("2026-11-24_Vyuny_1km.gpx");
        std::fs::write(dir.path().join("2026-11-24_Other_Wiki_Satell_4ozi.wpt"), wpt("Чужой")).unwrap();
        std::fs::write(dir.path().join("2026-11-24_Vyuny_Wiki_Topo_4ozi.wpt"), wpt("Узел")).unwrap();
        assert_eq!(load_for_grid(&gpx).unwrap()[0].name, "Узел");
    }

    #[test]
    fn absence_is_not_an_error_and_broken_satell_falls_back() {
        let dir = TempDir::new();
        let gpx = dir.path().join("2026-11-24_Vyuny_500m.gpx");
        assert!(load_for_grid(&gpx).unwrap().is_empty());
        std::fs::write(dir.path().join("2026-11-24_Vyuny_Wiki_Satell_4ozi.wpt"), "bad").unwrap();
        std::fs::write(dir.path().join("2026-11-24_Vyuny_Wiki_Topo_4ozi.wpt"), wpt("Запасной")).unwrap();
        assert_eq!(load_for_grid(&gpx).unwrap()[0].name, "Запасной");
    }
}
