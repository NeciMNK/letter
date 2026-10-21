//! Соответствие значений OOXML и типов модели. Таблицы общие для чтения
//! и записи, чтобы оба направления не разошлись.

use letter_model::{Alignment, Highlight, ThemeFont, Underline};

/// Значения `w:u/@w:val`.
pub(crate) const UNDERLINES: &[(&str, Underline)] = &[
    ("none", Underline::None),
    ("single", Underline::Single),
    ("words", Underline::Words),
    ("double", Underline::Double),
    ("thick", Underline::Thick),
    ("dotted", Underline::Dotted),
    ("dottedHeavy", Underline::DottedHeavy),
    ("dash", Underline::Dash),
    ("dashedHeavy", Underline::DashedHeavy),
    ("dashLong", Underline::DashLong),
    ("dashLongHeavy", Underline::DashLongHeavy),
    ("dotDash", Underline::DotDash),
    ("dashDotHeavy", Underline::DashDotHeavy),
    ("dotDotDash", Underline::DotDotDash),
    ("dashDotDotHeavy", Underline::DashDotDotHeavy),
    ("wave", Underline::Wave),
    ("wavyHeavy", Underline::WavyHeavy),
    ("wavyDouble", Underline::WavyDouble),
];

/// Значения `w:highlight/@w:val`.
pub(crate) const HIGHLIGHTS: &[(&str, Highlight)] = &[
    ("none", Highlight::None),
    ("black", Highlight::Black),
    ("blue", Highlight::Blue),
    ("cyan", Highlight::Cyan),
    ("green", Highlight::Green),
    ("magenta", Highlight::Magenta),
    ("red", Highlight::Red),
    ("yellow", Highlight::Yellow),
    ("white", Highlight::White),
    ("darkBlue", Highlight::DarkBlue),
    ("darkCyan", Highlight::DarkCyan),
    ("darkGreen", Highlight::DarkGreen),
    ("darkMagenta", Highlight::DarkMagenta),
    ("darkRed", Highlight::DarkRed),
    ("darkYellow", Highlight::DarkYellow),
    ("darkGray", Highlight::DarkGray),
    ("lightGray", Highlight::LightGray),
];

/// Значения `w:jc/@w:val`. Первое имя для каждого значения — то, что
/// пишет Word; `start`/`end` — синонимы из новой редакции стандарта.
pub(crate) const ALIGNMENTS: &[(&str, Alignment)] = &[
    ("left", Alignment::Left),
    ("center", Alignment::Center),
    ("right", Alignment::Right),
    ("both", Alignment::Justify),
    ("distribute", Alignment::Distribute),
    ("start", Alignment::Left),
    ("end", Alignment::Right),
];

/// Значения `w:asciiTheme` и других ссылок на шрифты темы.
pub(crate) const THEME_FONTS: &[(&str, ThemeFont)] = &[
    ("majorAscii", ThemeFont::MajorAscii),
    ("majorHAnsi", ThemeFont::MajorHAnsi),
    ("majorEastAsia", ThemeFont::MajorEastAsia),
    ("majorBidi", ThemeFont::MajorBidi),
    ("minorAscii", ThemeFont::MinorAscii),
    ("minorHAnsi", ThemeFont::MinorHAnsi),
    ("minorEastAsia", ThemeFont::MinorEastAsia),
    ("minorBidi", ThemeFont::MinorBidi),
];

/// Ищет значение по имени.
pub(crate) fn lookup<T: Copy>(table: &[(&str, T)], name: &str) -> Option<T> {
    table.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_have_unique_names() {
        fn check<T>(table: &[(&str, T)]) {
            for (i, (a, _)) in table.iter().enumerate() {
                assert!(table[i + 1..].iter().all(|(b, _)| a != b), "duplicate {a}");
            }
        }
        check(UNDERLINES);
        check(HIGHLIGHTS);
        check(ALIGNMENTS);
        check(THEME_FONTS);
        assert_eq!(lookup(ALIGNMENTS, "both"), Some(Alignment::Justify));
        assert_eq!(lookup(UNDERLINES, "bogus"), None);
    }
}
