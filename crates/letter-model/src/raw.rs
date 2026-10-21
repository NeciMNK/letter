//! Содержимое, которое `letter` пока не понимает, но обязан сохранить.

use std::fmt;

/// Фрагмент XML из файла, сохранённый «как есть».
///
/// Это главный механизм совместимости: всё, что программа ещё не умеет
/// показывать или редактировать (рисунки, поля, закладки, исправления…),
/// не выбрасывается, а хранится здесь и при сохранении записывается обратно.
///
/// Строка — один XML-элемент целиком, вместе с вложенными элементами.
/// Фрагмент самодостаточен: в его корневом элементе объявлены все
/// пространства имён (`xmlns:…`), префиксы которых встречаются внутри.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RawXml(String);

impl RawXml {
    /// Оборачивает готовую строку XML. Корректность XML не проверяется.
    pub fn new(xml: impl Into<String>) -> RawXml {
        RawXml(xml.into())
    }

    /// Текст фрагмента.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Полное имя корневого элемента с префиксом, например `"w:drawing"`.
    pub fn name(&self) -> &str {
        let rest = self.0.trim_start().trim_start_matches('<');
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(rest.len());
        &rest[..end]
    }

    /// Имя корневого элемента без префикса, например `"drawing"`.
    pub fn local_name(&self) -> &str {
        let name = self.name();
        name.rsplit_once(':').map_or(name, |(_, local)| local)
    }
}

impl fmt::Display for RawXml {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_name() {
        let raw = RawXml::new(r#"<w:bookmarkStart xmlns:w="x" w:id="0"/>"#);
        assert_eq!(raw.name(), "w:bookmarkStart");
        assert_eq!(raw.local_name(), "bookmarkStart");
        assert_eq!(RawXml::new("<plain>t</plain>").local_name(), "plain");
        assert_eq!(RawXml::new("").name(), "");
    }
}
