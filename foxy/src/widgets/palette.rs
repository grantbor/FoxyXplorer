use std::path::PathBuf;

pub fn load_palette() -> Vec<[u8; 3]> {
    let path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("foxy")
        .join("palette.json");

    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(arr) = serde_json::from_str::<Vec<[u8; 3]>>(&content) {
            return arr;
        }
    }

    let default = vec![
        [255, 0, 0],
        [0, 0, 255],
        [0, 128, 0],
        [255, 255, 255],
        [0, 255, 255],
        [255, 255, 0],
        [255, 0, 255],
        [0, 255, 0],
        [0, 0, 0],
    ];

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, serde_json::to_string_pretty(&default).unwrap_or_default());

    default
}

/// Расширенная палитра 8×8, сгруппированная по семействам оттенков.
/// Строки (по 8 цветов):
/// 1) белый/жёлтый/оранжевый  2) красный/розовый/магента
/// 3) персиковый/розовый/фиолетовый  4) синий
/// 5) бирюзовый/зелёный  6) зелёный/жёлто-зелёный
/// 7) коричневый/тёмно-красный  8) тёмно-коричневый/серый/чёрный
const EXTENDED_PALETTE: [[u8; 3]; 64] = [
    // 1. Белый → жёлтый → оранжевый
    [255, 255, 255], // white
    [255, 248, 220], // ivory
    [255, 235,   0], // yellow
    [255, 200,   0], // gold
    [240, 220, 130], // khaki
    [255, 210, 170], // peach
    [255, 165,   0], // orange
    [255, 140,   0], // dark orange

    // 2. Красный → розовый → магента
    [255, 100,  30], // orange-red
    [220,  40,  30], // red
    [180,  30,  20], // firebrick
    [140,  25,  15], // dark red
    [110,  20,  15], // maroon
    [220,  20,  90], // crimson
    [220,  20, 140], // magenta
    [240, 130, 120], // light coral

    // 3. Персиковый → розовый → фиолетовый
    [255, 210, 180], // light peach
    [255, 170, 200], // light pink
    [200, 120, 190], // orchid
    [140,  20, 140], // purple
    [ 90,  20,  90], // dark purple
    [ 60,  15,  60], // very dark purple
    [ 40,  30, 100], // indigo
    [ 80,  40, 180], // blue-violet

    // 4. Синий
    [ 30,  90, 180], // blue
    [ 20,  50, 120], // dark blue
    [ 30,  50,  80], // dark slate
    [ 20,  30,  50], // very dark slate
    [ 15,  20,  35], // almost-black blue
    [  5,   5,  20], // midnight
    [ 30, 140, 220], // dodger blue
    [ 30, 170, 210], // sky blue

    // 5. Бирюзовый → зелёный
    [170, 200, 220], // light blue-gray
    [140, 200, 200], // pale turquoise
    [110, 140, 160], // gray-blue
    [ 20, 140, 150], // teal
    [ 15, 110, 100], // dark teal
    [ 20, 140, 100], // sea green
    [ 30, 150,  80], // green
    [ 20,  80,  50], // forest green

    // 6. Зелёный → жёлто-зелёный
    [ 25, 100,  60], // dark forest green
    [ 50, 160,  70], // medium green
    [120, 190,  70], // yellow-green
    [170, 190,  50], // olive-yellow
    [190, 220,  40], // lime
    [210, 200,  60], // pale yellow
    [ 90, 110,  60], // olive
    [230, 230, 140], // light yellow

    // 7. Коричневый → тёмно-красный
    [180, 140,  20], // goldenrod
    [200, 130,  40], // orange-brown
    [180, 150, 110], // tan
    [120,  80,  60], // brown
    [ 70,  45,  35], // dark brown
    [130,  30,  70], // maroon-pink
    [ 90,  25,  55], // dark maroon
    [ 90,  15,  15], // very dark red

    // 8. Тёмно-коричневый → серый → чёрный
    [ 50,  25,  15], // very dark brown
    [ 85,  50,  35], // dark reddish brown
    [130,  85,  65], // medium brown
    [220, 130, 100], // salmon
    [240, 180, 150], // light salmon
    [245, 210, 190], // very pale peach
    [190, 190, 190], // light gray
    [  0,   0,   0], // black
];

pub fn get_extended_palette() -> Vec<[u8; 3]> {
    EXTENDED_PALETTE.to_vec()
}
