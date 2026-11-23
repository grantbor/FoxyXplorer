//! Категории путевых точек из CSV, вшитого при компиляции.
//! Для обновления справочника измените `foxy/assets/waypoints.csv` и пересоберите FoxyXplorer.

use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct WaypointCategory {
    pub code: String,
    pub name: String,
    pub color: Option<[u8; 3]>,
}

/// Чтение CSV с поддержкой кавычек и запятых внутри ячеек.
fn csv_rows(csv: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut chars = csv.trim_start_matches('\u{feff}').chars().peekable();
    let mut quoted = false;

    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    cell.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                cell.push(ch);
            }
        } else {
            match ch {
                '"' => quoted = true,
                ',' => row.push(std::mem::take(&mut cell)),
                '\n' => {
                    row.push(std::mem::take(&mut cell));
                    if row.iter().any(|c| !c.trim().is_empty()) {
                        rows.push(std::mem::take(&mut row));
                    } else {
                        row.clear();
                    }
                }
                '\r' => {},
                _ => cell.push(ch),
            }
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        if row.iter().any(|c| !c.trim().is_empty()) { rows.push(row); }
    }
    rows
}

fn color_from_csv(cell: &str) -> Option<[u8; 3]> {
    let start = cell.find('#')? + 1;
    let hex = cell.get(start..start + 6)?;
    Some([
        u8::from_str_radix(&hex[0..2], 16).ok()?,
        u8::from_str_radix(&hex[2..4], 16).ok()?,
        u8::from_str_radix(&hex[4..6], 16).ok()?,
    ])
}

fn parse_categories(csv: &str) -> Vec<WaypointCategory> {
    let rows = csv_rows(csv);
    let Some(header) = rows.first().cloned() else { return Vec::new(); };
    let column = |name: &str| header.iter().position(|col| col.trim() == name);
    let (Some(code_col), Some(name_col), Some(color_col)) =
        (column("code"), column("name"), column("color"))
    else { return Vec::new(); };

    rows.into_iter().skip(1).filter_map(|row| {
        let code = row.get(code_col)?.trim();
        let name = row.get(name_col)?.trim();
        if code.is_empty() || name.is_empty() { return None; }
        Some(WaypointCategory {
            code: code.to_string(),
            name: name.to_string(),
            color: row.get(color_col).and_then(|s| color_from_csv(s)),
        })
    }).collect()
}

pub fn all() -> &'static [WaypointCategory] {
    static CATEGORIES: OnceLock<Vec<WaypointCategory>> = OnceLock::new();
    CATEGORIES.get_or_init(|| parse_categories(include_str!("../../assets/waypoints.csv"))).as_slice()
}

pub fn background_colors() -> Vec<[u8; 3]> {
    let mut colors = Vec::new();
    for cat in all() {
        if let Some(color) = cat.color {
            if !colors.contains(&color) { colors.push(color); }
        }
    }
    colors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supplied_csv_parses() {
        let categories = parse_categories("code,name,group,color\ncellar,Подвалы и чердаки,g_terrain,[b:#FF00FF]\nimportant,Важное,g_artifact,\"[b:#FF0000, f:]\"\n");
        let cellar = categories.iter().find(|c| c.code == "cellar").unwrap();
        assert_eq!(cellar.name, "Подвалы и чердаки");
        assert_eq!(cellar.color, Some([255, 0, 255]));
        let important = categories.iter().find(|c| c.code == "important").unwrap();
        assert_eq!(important.color, Some([255, 0, 0]));
    }

    #[test]
    fn quoted_and_lowercase_colors() {
        let csv = "code,name,group,color\nA,\"Имя, с запятой\",g,\"[b:#aBcDeF, f:]\"\n";
        let categories = parse_categories(csv);
        assert_eq!(categories[0].name, "Имя, с запятой");
        assert_eq!(categories[0].color, Some([0xab, 0xcd, 0xef]));
    }
}
