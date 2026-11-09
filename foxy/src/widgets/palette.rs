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

pub fn get_extended_palette() -> Vec<[u8; 3]> {
    let mut colors = Vec::new();
    colors.extend_from_slice(&[[0,0,0], [85,85,85], [170,170,170], [255,255,255]]);
    for &(r, g, b) in &[(255,0,0), (0,255,0), (0,0,255), (255,255,0), (0,255,255), (255,0,255), (128,0,0), (0,128,0), (0,0,128), (128,128,0), (0,128,128), (128,0,128)] {
        colors.push([r, g, b]);
        colors.push([r/2, g/2, b/2]);
        colors.push([(r as u16 * 3 / 4) as u8, (g as u16 * 3 / 4) as u8, (b as u16 * 3 / 4) as u8]);
    }
    colors
}
