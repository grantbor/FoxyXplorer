//! Неинтерактивный слой точек Wiki из WPT, соседнего с GPX-сеткой комплекта.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct WikiPoint {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
}

/// Ищем WPT рядом с GPX. Встречаются оба варианта префикса:
/// `DATE_Kit_500m_Wiki_Topo.wpt` (полное имя GPX-сетки) и
/// `DATE_Kit_Wiki_Topo.wpt` (имя комплекта без размера), а также `Wiki_*.wpt`.
/// Сначала рассматриваем файлы с суффиксом `_4ozi`, затем обычные WPT.
/// Satell и Topo равноценны: оформление точек задаётся самим Wiki-слоем.
/// Имя файла и расширение сравниваются без учёта регистра.
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

    let parent = grid_gpx.parent().ok_or(crate::t!("msg_0248"))?;
    let entries = std::fs::read_dir(parent)
        .map_err(|e| format!("{}: {e}", parent.display()))?;
    let grid_prefix = format!("{}_wiki_", stem.to_lowercase());
    let kit_prefix = format!("{}_wiki_", kit_prefix.to_lowercase());
    let mut matches = Vec::new();

    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if !path.is_file() || !path.extension().and_then(|s| s.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("wpt")) {
            continue;
        }
        let Some(filename_stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        let lower = filename_stem.to_lowercase();
        let (suffix, source_priority) = if let Some(s) = lower.strip_prefix(&grid_prefix) {
            (s, 0u8) // имя именно этой GPX-сетки, включая 500m/1km
        } else if let Some(s) = lower.strip_prefix(&kit_prefix) {
            (s, 1u8) // старый вариант без размера сетки
        } else if let Some(s) = lower.strip_prefix("wiki_") {
            (s, 2u8) // общий файл Wiki_*.wpt
        } else {
            continue;
        };
        if suffix.is_empty() { continue; }

        let file_priority = if suffix.ends_with("_4ozi") { 0u8 } else { 1u8 };
        matches.push((file_priority, source_priority, lower, path));
    }
    // Нет предпочтения Satell/Topo: стабильный порядок по имени файла.
    matches.sort_by(|a, b| (&a.0, &a.1, &a.2).cmp(&(&b.0, &b.1, &b.2)));
    Ok(matches.into_iter().map(|(_, _, _, path)| path).collect())
}

/// Типичное имя узла: A3, H12, AA3, AE30 (регистр неважен).
/// На очень больших сетках допускаются также более длинные буквенные префиксы.
fn looks_like_grid_node(name: &str) -> bool {
    let bytes = name.trim().as_bytes();
    let letters = bytes.iter().take_while(|c| c.is_ascii_alphabetic()).count();
    letters > 0 && letters < bytes.len() && bytes[letters..].iter().all(u8::is_ascii_digit)
}

/// Удаляем только узлы, действительно присутствующие в выбранной GPX-сетке.
/// Так не теряются Wiki-объекты с похожим коротким именем из другой области.
fn grid_node_names(grid: &grid::Grid) -> HashSet<String> {
    grid.nodes.iter()
        .map(|n| n.name.trim())
        .filter(|name| looks_like_grid_node(name))
        .map(str::to_ascii_uppercase)
        .collect()
}

/// Не смешиваем Wiki с обычными пользовательскими Waypoint: только имя и геопозиция.
/// При отсутствии подходящего WPT (или при наличии одних узлов сетки) получаем
/// пустой список, а не ошибку. Повреждённый файл не мешает загрузить запасной.
pub(crate) fn load_for_grid(grid_gpx: &Path) -> Result<Vec<WikiPoint>, String> {
    let candidates = candidate_paths(grid_gpx)?;
    if candidates.is_empty() { return Ok(Vec::new()); }

    // GPX уже был проверен в load_kit(); повторное чтение позволяет сохранить
    // неизменным интерфейс load_for_grid и не затрагивать другие модули.
    let bytes = std::fs::read(grid_gpx)
        .map_err(|e| format!("{}: {e}", grid_gpx.display()))?;
    let grid = grid::Grid::from_gpx(&geoio::text::decode(&bytes))
        .map_err(|e| format!("{}: {e}", grid_gpx.display()))?;
    let node_names = grid_node_names(&grid);

    let mut failures = Vec::new();
    let mut parsed_any = false;
    for path in candidates {
        let loaded = std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| geoio::wpt::read(&bytes).map_err(|e| e.to_string()));
        match loaded {
            Ok(data) => {
                parsed_any = true;
                let points: Vec<_> = data.waypoints.into_iter()
                    .filter_map(|w| {
                        let name = w.name.trim();
                        if name.is_empty() || node_names.contains(&name.to_ascii_uppercase()) {
                            return None;
                        }
                        Some(WikiPoint { name: name.to_string(), lat: w.lat, lon: w.lon })
                    })
                    .collect();
                if !points.is_empty() { return Ok(points); }
            }
            Err(err) => failures.push(format!("{}: {err}", path.display())),
        }
    }
    if parsed_any { Ok(Vec::new()) } else { Err(failures.join("; ")) }
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

    fn grid_gpx(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, "<?xml version=\"1.0\"?><gpx>\
            <wpt lat=\"55.0\" lon=\"83.0\"><name>A3</name></wpt>\
            <wpt lat=\"55.1\" lon=\"83.1\"><name>AB12</name></wpt>\
            <wpt lat=\"55.2\" lon=\"83.2\"><name>AE30</name></wpt>\
            </gpx>").unwrap();
        path
    }

    fn wpt(names: &[&str]) -> String {
        let mut s = String::from("OziExplorer Waypoint File Version 1.1\r\nWGS 84\r\nReserved 2\r\ngarmin\r\n");
        for (i, name) in names.iter().enumerate() {
            s.push_str(&format!("{},{name},55.123,83.456,0,0,1,3,0,0,,,,,\r\n", i + 1));
        }
        s
    }

    #[test]
    fn loads_prefixed_4ozi_file_and_strips_actual_nodes() {
        let dir = TempDir::new();
        let gpx = grid_gpx(dir.path(), "2026-11-24_Vyuny_500m.gpx");
        std::fs::write(dir.path().join("2026-11-24_Vyuny_Wiki_Satell_4ozi.WPT"),
            wpt(&["A3", "ab12", "AE30", "Дом", "A99"])).unwrap();
        let points = load_for_grid(&gpx).unwrap();
        assert_eq!(points.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Дом", "A99"]);
        assert_eq!(points[0].lat, 55.123);
        assert_eq!(points[0].lon, 83.456);
    }

    #[test]
    fn falls_back_to_topo_and_ignores_other_kits() {
        let dir = TempDir::new();
        let gpx = grid_gpx(dir.path(), "2026-11-24_Vyuny_1km.gpx");
        std::fs::write(dir.path().join("2026-11-24_Other_Wiki_Satell_4ozi.wpt"), wpt(&["Чужой"])).unwrap();
        std::fs::write(dir.path().join("2026-11-24_Vyuny_Wiki_Topo_4ozi.wpt"), wpt(&["Узел"])).unwrap();
        assert_eq!(load_for_grid(&gpx).unwrap()[0].name, "Узел");
    }

    #[test]
    fn plain_wiki_file_without_4ozi_is_loaded_and_filtered() {
        let dir = TempDir::new();
        let gpx = grid_gpx(dir.path(), "2026-10-31_Nizhniy_Novgorod_250m.gpx");
        std::fs::write(dir.path().join("Wiki_Nizhniy-Novgorod.WpT"),
            wpt(&["a3", "AB12", "ae30", "Мост", "Лагерь"])).unwrap();
        let points = load_for_grid(&gpx).unwrap();
        assert_eq!(points.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Мост", "Лагерь"]);
    }

    #[test]
    fn prefers_4ozi_to_plain_wpt_even_if_unprefixed() {
        let dir = TempDir::new();
        let gpx = grid_gpx(dir.path(), "2026-11-24_Vyuny_500m.gpx");
        std::fs::write(dir.path().join("Wiki_Topo_4ozi.wpt"), wpt(&["Приоритет"])).unwrap();
        std::fs::write(dir.path().join("2026-11-24_Vyuny_Wiki_Satell.wpt"), wpt(&["Обычный"])).unwrap();
        assert_eq!(load_for_grid(&gpx).unwrap()[0].name, "Приоритет");
    }

    #[test]
    fn only_nodes_are_not_an_error_and_allow_fallback() {
        let dir = TempDir::new();
        let gpx = grid_gpx(dir.path(), "2026-11-24_Vyuny_500m.gpx");
        std::fs::write(dir.path().join("2026-11-24_Vyuny_Wiki_Satell_4ozi.wpt"),
            wpt(&["A3", "ab12", "AE30"])).unwrap();
        std::fs::write(dir.path().join("Wiki_other.wpt"), wpt(&["Новая точка"])).unwrap();
        assert_eq!(load_for_grid(&gpx).unwrap()[0].name, "Новая точка");
        std::fs::remove_file(dir.path().join("Wiki_other.wpt")).unwrap();
        assert!(load_for_grid(&gpx).unwrap().is_empty());
    }

    #[test]
    fn absence_is_not_error_and_broken_primary_falls_back() {
        let dir = TempDir::new();
        let gpx = grid_gpx(dir.path(), "2026-11-24_Vyuny_500m.gpx");
        assert!(load_for_grid(&gpx).unwrap().is_empty());
        std::fs::write(dir.path().join("2026-11-24_Vyuny_Wiki_Satell_4ozi.wpt"), "bad").unwrap();
        std::fs::write(dir.path().join("2026-11-24_Vyuny_Wiki_Topo_4ozi.wpt"), wpt(&["Запасной"])).unwrap();
        assert_eq!(load_for_grid(&gpx).unwrap()[0].name, "Запасной");
    }

    #[test]
    fn recognizes_full_grid_filename_without_4ozi_and_filters_nodes() {
        // Real kit naming: size (`500m`) is part of the WPT prefix, not omitted.
        let dir = TempDir::new();
        let gpx = grid_gpx(dir.path(), "2026-07-27_Petropavlovka_500m.gpx");
        std::fs::write(dir.path().join("2026-07-27_Petropavlovka_500m_Wiki_Topo.wpt"),
            wpt(&["A3", "ab12", "AE30", "Петропавловка", "Пруд"])).unwrap();
        let points = load_for_grid(&gpx).unwrap();
        assert_eq!(points.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["Петропавловка", "Пруд"]);
    }

    #[test]
    fn full_grid_4ozi_is_preferred_to_plain_wpt_and_other_grid_sizes() {
        let dir = TempDir::new();
        let gpx = grid_gpx(dir.path(), "2026-07-27_Petropavlovka_500m.gpx");
        std::fs::write(dir.path().join("2026-07-27_Petropavlovka_500m_Wiki_Topo.wpt"),
            wpt(&["Обычный"])).unwrap();
        std::fs::write(dir.path().join("2026-07-27_Petropavlovka_1km_Wiki_Topo_4ozi.wpt"),
            wpt(&["Чужая сетка"])).unwrap();
        std::fs::write(dir.path().join("2026-07-27_Petropavlovka_500m_Wiki_Satell_4ozi.WPT"),
            wpt(&["Правильный"])).unwrap();
        assert_eq!(load_for_grid(&gpx).unwrap()[0].name, "Правильный");
    }

    #[test]
    fn node_shape_supports_double_letter_and_future_longer_prefixes() {
        for name in ["A3", "AB12", "ae30", "AAA7"] {
            assert!(looks_like_grid_node(name), "{name}");
        }
        for name in ["Дерево", "A", "123", "A-3", "A3test"] {
            assert!(!looks_like_grid_node(name), "{name}");
        }
    }
}
