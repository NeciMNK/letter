//! Названия языков для строки состояния: код из документа (`ru-RU`,
//! `en-US`) → «русский», «английский (США)». Сами названия — в файлах
//! перевода (`lang-…`, `region-…`).

use crate::i18n::{has, tr, tr_args};

/// Языки, у которых страну не пишут (одна основная).
const SINGLE: [&str; 6] = ["ru", "uk", "be", "kk", "pl", "ja"];

/// Название языка по коду: `ru-RU` → «русский», `en-US` → «английский
/// (США)». Незнакомый код показывается как есть.
pub fn display_name(code: &str) -> String {
    let (language, region) = code.split_once(['-', '_']).unwrap_or((code, ""));
    let lower = language.to_ascii_lowercase();
    let id = format!("lang-{lower}");
    if lower.is_empty() || !has(&id) {
        return code.to_owned();
    }
    let name = tr(&id);
    if region.is_empty() || SINGLE.contains(&lower.as_str()) {
        return name.to_owned();
    }
    let upper = region.to_ascii_uppercase();
    let id = format!("region-{upper}");
    let country = if has(&id) { tr(&id) } else { region };
    tr_args(
        "language-with-region",
        &[("language", name.into()), ("region", country.into())],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Language, set_language};

    #[test]
    fn names() {
        assert_eq!(display_name("ru-RU"), "русский");
        assert_eq!(display_name("en-US"), "английский (США)");
        assert_eq!(display_name("en_gb"), "английский (Великобритания)");
        assert_eq!(display_name("de"), "немецкий");
        assert_eq!(display_name("fr-CI"), "французский (CI)");
        assert_eq!(display_name("x-none"), "x-none");
        assert_eq!(display_name(""), "");
        assert_eq!(display_name("-US"), "-US");
        set_language(Language::English);
        assert_eq!(display_name("en-US"), "English (United States)");
        assert_eq!(display_name("ru-RU"), "Russian");
        set_language(Language::Russian);
    }
}
