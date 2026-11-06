//! Кодировки: UTF-8 и CP1251 (чтение с автоопределением, запись по выбору).

const CP1251_HIGH: [char; 64] = [
    '\u{0402}', '\u{0403}', '\u{201A}', '\u{0453}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{20AC}', '\u{2030}', '\u{0409}', '\u{2039}', '\u{040A}', '\u{040C}', '\u{040B}', '\u{040F}',
    '\u{0452}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{FFFD}', '\u{2122}', '\u{0459}', '\u{203A}', '\u{045A}', '\u{045C}', '\u{045B}', '\u{045F}',
    '\u{00A0}', '\u{040E}', '\u{045E}', '\u{0408}', '\u{00A4}', '\u{0490}', '\u{00A6}', '\u{00A7}',
    '\u{0401}', '\u{00A9}', '\u{0404}', '\u{00AB}', '\u{00AC}', '\u{00AD}', '\u{00AE}', '\u{0407}',
    '\u{00B0}', '\u{00B1}', '\u{0406}', '\u{0456}', '\u{0491}', '\u{00B5}', '\u{00B6}', '\u{00B7}',
    '\u{0451}', '\u{2116}', '\u{0454}', '\u{00BB}', '\u{0458}', '\u{0405}', '\u{0455}', '\u{0457}',
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Cp1251,
}

pub fn decode_cp1251(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0..=0x7F => b as char,
            0x80..=0xBF => CP1251_HIGH[(b - 0x80) as usize],
            _ => char::from_u32(0x0410 + (b - 0xC0) as u32).unwrap_or('?'),
        })
        .collect()
}

/// UTF-8 (с BOM или без); если байты не UTF-8 — считаем, что это CP1251.
pub fn decode(bytes: &[u8]) -> String {
    let bytes = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) { &bytes[3..] } else { bytes };
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => decode_cp1251(bytes),
    }
}

fn encode_cp1251_char(c: char) -> u8 {
    let u = c as u32;
    if u < 0x80 {
        return u as u8;
    }
    if (0x0410..=0x044F).contains(&u) {
        return (u - 0x0410) as u8 + 0xC0;
    }
    match CP1251_HIGH.iter().position(|&h| h == c && h != '\u{FFFD}') {
        Some(i) => 0x80 + i as u8,
        None => b'?',
    }
}

/// Символы, которых нет в CP1251, превращаются в «?».
pub fn encode(s: &str, enc: Encoding) -> Vec<u8> {
    match enc {
        Encoding::Utf8 => s.as_bytes().to_vec(),
        Encoding::Cp1251 => s.chars().map(encode_cp1251_char).collect(),
    }
}

/// Можно ли записать строку в CP1251 без потерь.
pub fn fits_cp1251(s: &str) -> bool {
    s.chars().all(|c| c == '?' || encode_cp1251_char(c) != b'?')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cp1251_roundtrip() {
        let s = "Лиса №4 ёЁ «трек»";
        assert!(fits_cp1251(s));
        assert_eq!(decode(&encode(s, Encoding::Cp1251)), s);
        assert!(!fits_cp1251("日本"));
    }

    #[test]
    fn month_abbreviation_bytes() {
        assert_eq!(decode(&[0xEE, 0xEA, 0xF2]), "окт");
    }

    #[test]
    fn utf8_with_bom() {
        let mut b = vec![0xEF, 0xBB, 0xBF];
        b.extend_from_slice("Лиса".as_bytes());
        assert_eq!(decode(&b), "Лиса");
    }
}
