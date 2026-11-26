//! Минимальная локализация на основе встроенного translations.json.
//! Формат: { "msg_0001": { "ru": "...", "en": "..." }, ... }

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// Текущий язык. `ru` по умолчанию, меняется через `set_lang`.
/// Используем `&'static str`, чтобы можно было инициализировать static без LazyLock.
static LANG: RwLock<&'static str> = RwLock::new("ru");

/// Разобранные переводы: ключ → (язык → текст).
static TRANSLATIONS: LazyLock<HashMap<&'static str, HashMap<&'static str, &'static str>>> =
    LazyLock::new(|| {
        let json = include_str!("../assets/translations.json");
        // Box::leak превращает локальный Value в &'static Value
        let parsed: &'static serde_json::Value = Box::leak(Box::new(
            serde_json::from_str::<serde_json::Value>(json)
                .expect("translations.json: некорректный JSON"),
        ));
        let mut map = HashMap::new();
        for (key, val) in parsed.as_object().expect("translations.json: ожидался объект") {
            let mut lang_map = HashMap::new();
            for (lang, text) in val.as_object().expect("значение ключа — объект") {
                let raw = text.as_str().expect("перевод — строка");
                // Декодируем escape-последовательности и делаем &'static str
                let decoded: &'static str = Box::leak(decode_escapes(raw).into_boxed_str());
                lang_map.insert(lang.as_str(), decoded);
            }
            map.insert(key.as_str(), lang_map);
        }
        map
    });

/// Установить текущий язык. Принимает `&'static str`.
/// Если у вас есть динамический `String` (например, из `Config`), используйте
/// `set_lang(Box::leak(s.into_boxed_str()))` — это однократная «утечка»,
/// которая приемлема для редко меняющихся настроек.
pub fn set_lang(lang: &'static str) {
    *LANG.write().unwrap() = lang;
}

/// Текущий язык.
pub fn current_lang() -> &'static str {
    *LANG.read().unwrap()
}

/// Перевод по ключу. Если ключа или языка нет — возвращает сам ключ.
pub fn translate(key: &'static str) -> &'static str {
    let lang = *LANG.read().unwrap();
    TRANSLATIONS
        .get(key)
        .and_then(|m| m.get(lang))
        .copied()
        .unwrap_or(key)
}

/// Format a localized template by replacing its named {placeholders}.
/// Each language can use its own word order and punctuation.
pub fn interpolate(key: &'static str, vars: &[(&str, String)]) -> String {
    let mut result = translate(key).to_string();
    for (name, value) in vars {
        result = result.replace(&format!("{{{name}}}"), value);
    }
    result
}

/// Декодирует escape-последовательности: \n, \t, \r, \\, \", \', \0, \xNN, \u{NNNN}.
fn decode_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('\'') => out.push('\''),
                Some('0') => out.push('\0'),
                Some('x') => {
                    let mut hex = String::new();
                    for _ in 0..2 {
                        if let Some(&h) = chars.peek() {
                            if h.is_ascii_hexdigit() { hex.push(h); chars.next(); } else { break; }
                        }
                    }
                    if let Ok(code) = u32::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(code) { out.push(ch); }
                    }
                }
                Some('u') => {
                    if chars.peek() == Some(&'{') {
                        chars.next();
                        let mut hex = String::new();
                        while let Some(&h) = chars.peek() {
                            if h == '}' { chars.next(); break; }
                            if h.is_ascii_hexdigit() { hex.push(h); chars.next(); } else { break; }
                        }
                        if let Ok(code) = u32::from_str_radix(&hex, 16) {
                            if let Some(ch) = char::from_u32(code) { out.push(ch); }
                        }
                    }
                }
                Some(other) => { out.push('\\'); out.push(other); }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Макрос для краткости: `t!("msg_0001")`.
#[macro_export]
macro_rules! t {
    ($key:literal) => {
        $crate::i18n::translate($key)  // БЕЗ .to_string()
    };
}

/// Interpolate a translation with named arguments: tf!("msg_NNNN", count = n).
#[macro_export]
macro_rules! tf {
    ($key:literal $(, $name:ident = $value:expr)* $(,)?) => {{
        $crate::i18n::interpolate($key, &[
            $((stringify!($name), ($value).to_string()),)*
        ])
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_translation_keys_have_both_languages() {
        for (key, languages) in TRANSLATIONS.iter() {
            for lang in ["ru", "en"] {
                assert!(languages.get(lang).is_some_and(|text| !text.is_empty()),
                    "missing {lang} translation for {key}");
            }
        }
    }

    #[test]
    fn interpolated_strings_resolve_named_parameters() {
        let result = interpolate("msg_0374", &[
            ("tracks", "10".into()),
            ("waypoints", "24".into()),
        ]);
        assert!(result.contains("10"));
        assert!(result.contains("24"));
        assert!(!result.contains("{tracks}"));
        assert!(!result.contains("{waypoints}"));
    }
}
