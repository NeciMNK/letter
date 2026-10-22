//! Названия языков для строки состояния: код из документа (`ru-RU`,
//! `en-US`) → «русский», «английский (США)».

/// Языки по первой части кода.
const LANGUAGES: [(&str, &str); 24] = [
    ("ru", "русский"),
    ("en", "английский"),
    ("uk", "украинский"),
    ("be", "белорусский"),
    ("kk", "казахский"),
    ("de", "немецкий"),
    ("fr", "французский"),
    ("es", "испанский"),
    ("it", "итальянский"),
    ("pt", "португальский"),
    ("nl", "нидерландский"),
    ("pl", "польский"),
    ("cs", "чешский"),
    ("sk", "словацкий"),
    ("bg", "болгарский"),
    ("sr", "сербский"),
    ("tr", "турецкий"),
    ("el", "греческий"),
    ("he", "иврит"),
    ("ar", "арабский"),
    ("zh", "китайский"),
    ("ja", "японский"),
    ("ko", "корейский"),
    ("fi", "финский"),
];

/// Страны по второй части кода, если у языка их несколько.
const REGIONS: [(&str, &str); 14] = [
    ("US", "США"),
    ("GB", "Великобритания"),
    ("AU", "Австралия"),
    ("CA", "Канада"),
    ("DE", "Германия"),
    ("AT", "Австрия"),
    ("CH", "Швейцария"),
    ("FR", "Франция"),
    ("BE", "Бельгия"),
    ("ES", "Испания"),
    ("MX", "Мексика"),
    ("PT", "Португалия"),
    ("BR", "Бразилия"),
    ("CN", "КНР"),
];

/// Языки, у которых страну не пишут (одна основная).
const SINGLE: [&str; 6] = ["ru", "uk", "be", "kk", "pl", "ja"];

/// Название языка по коду: `ru-RU` → «русский», `en-US` → «английский
/// (США)». Незнакомый код показывается как есть.
pub fn display_name(code: &str) -> String {
    let (language, region) = code.split_once(['-', '_']).unwrap_or((code, ""));
    let lower = language.to_ascii_lowercase();
    let Some((_, name)) = LANGUAGES.iter().find(|(l, _)| *l == lower) else {
        return code.to_owned();
    };
    if region.is_empty() || SINGLE.contains(&lower.as_str()) {
        return (*name).to_owned();
    }
    let upper = region.to_ascii_uppercase();
    match REGIONS.iter().find(|(r, _)| *r == upper) {
        Some((_, country)) => format!("{name} ({country})"),
        None => format!("{name} ({region})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(display_name("ru-RU"), "русский");
        assert_eq!(display_name("en-US"), "английский (США)");
        assert_eq!(display_name("en_gb"), "английский (Великобритания)");
        assert_eq!(display_name("de"), "немецкий");
        assert_eq!(display_name("fr-CI"), "французский (CI)");
        assert_eq!(display_name("x-none"), "x-none");
        assert_eq!(display_name(""), "");
    }
}
