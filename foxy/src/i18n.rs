//! Минимальная локализация на основе встроенного translations.json.
//! Формат: { "msg_0001": { "ru": "...", "en": "..." }, ... }

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// Текущий язык. `ru` по умолчанию, меняется через `set_lang`.
static LANG: RwLock<&'static str> = RwLock::new("ru");

/// Разобранные переводы: ключ → (язык → текст).
/// Встроены в бинарник через `include_str!`, поэтому работают и в AppImage,
/// и в портативном exe, и без доступа к файловой системе.
static TRANSLATIONS: LazyLock<HashMap<&'static str, HashMap<&'static str, &'static str>>> =
    LazyLock::new(|| {
        let json = include_str!("../assets/translations.json");
        let parsed: serde_json::Value = serde_json::from_str(json)
            .expect("translations.json: некорректный JSON");
        let mut map = HashMap::new();
        for (key, val) in parsed.as_object().expect("translations.json: ожидался объект") {
            let mut lang_map = HashMap::new();
            for (lang, text) in val.as_object().expect("значение ключа — объект") {
                let text = text.as_str().expect("перевод — строка");
                lang_map.insert(lang.as_str(), text);
            }
            map.insert(key.as_str(), lang_map);
        }
        map
    });

/// Установить текущий язык. Допустимые значения — `"ru"`, `"en"`.
/// В будущем можно хранить в `App.lang` и восстанавливать из сессии.
pub fn set_lang(lang: &'static str) {
    *LANG.write().unwrap() = lang;
}

/// Текущий язык (для UI-переключателя, когда он появится).
pub fn current_lang() -> &'static str {
    *LANG.read().unwrap()
}

/// Перевод по ключу. Если ключа или языка нет — возвращает сам ключ,
/// чтобы в UI было видно, что именно не переведено (а не пустая строка).
pub fn translate(key: &'static str) -> &'static str {
    let lang = *LANG.read().unwrap();
    TRANSLATIONS
        .get(key)
        .and_then(|m| m.get(lang))
        .copied()
        .unwrap_or(key)
}

/// Макрос для краткости: `t!("msg_0001")`.
#[macro_export]
macro_rules! t {
    ($key:literal) => {
        $crate::i18n::translate($key).to_string()
    };
}
