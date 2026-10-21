//! Тема документа (`word/theme/theme1.xml`): пока только шрифты.

use crate::ThemeFont;

/// Шрифты темы. Стили Word обычно ссылаются не на конкретный шрифт,
/// а на «шрифт заголовков» и «шрифт текста» темы.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Theme {
    /// Основной набор (`a:majorFont`) — для заголовков.
    pub major: ThemeFontSet,
    /// Дополнительный набор (`a:minorFont`) — для основного текста.
    pub minor: ThemeFontSet,
}

/// Набор шрифтов темы для разных письменностей. Пустая строка в файле
/// означает «не задан» и хранится как `None`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ThemeFontSet {
    /// Латиница и кириллица (`a:latin`).
    pub latin: Option<String>,
    /// Восточноазиатское письмо (`a:ea`).
    pub east_asia: Option<String>,
    /// Сложное письмо (`a:cs`).
    pub complex_script: Option<String>,
}

impl Theme {
    /// Имя шрифта, на который указывает ссылка, если оно задано в теме.
    pub fn font(&self, font: ThemeFont) -> Option<&str> {
        let set = if font.is_major() {
            &self.major
        } else {
            &self.minor
        };
        match font {
            ThemeFont::MajorAscii
            | ThemeFont::MajorHAnsi
            | ThemeFont::MinorAscii
            | ThemeFont::MinorHAnsi => set.latin.as_deref(),
            ThemeFont::MajorEastAsia | ThemeFont::MinorEastAsia => set.east_asia.as_deref(),
            ThemeFont::MajorBidi | ThemeFont::MinorBidi => set.complex_script.as_deref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_font_lookup() {
        let theme = Theme {
            major: ThemeFontSet {
                latin: Some("Cambria".into()),
                ..Default::default()
            },
            minor: ThemeFontSet {
                latin: Some("Calibri".into()),
                east_asia: None,
                complex_script: Some("Arial".into()),
            },
        };
        assert_eq!(theme.font(ThemeFont::MajorHAnsi), Some("Cambria"));
        assert_eq!(theme.font(ThemeFont::MinorAscii), Some("Calibri"));
        assert_eq!(theme.font(ThemeFont::MinorBidi), Some("Arial"));
        assert_eq!(theme.font(ThemeFont::MinorEastAsia), None);
    }
}
