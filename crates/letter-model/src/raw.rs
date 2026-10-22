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

    /// Значение атрибута корневого элемента по имени без префикса:
    /// у `<w:lang w:val="ru-RU"/>` атрибут `"val"` — `"ru-RU"`.
    /// Ссылки на сущности (`&amp;`) не раскрываются.
    pub fn attribute(&self, local: &str) -> Option<&str> {
        let start = self.0.trim_start();
        let mut rest = start.get(1 + self.name().len()..)?;
        loop {
            rest = rest.trim_start();
            if rest.is_empty() || rest.starts_with(['/', '>']) {
                return None;
            }
            let eq = rest.find('=')?;
            let name = rest[..eq].trim();
            let value = rest[eq + 1..].trim_start();
            let quote = value.chars().next().filter(|q| matches!(q, '"' | '\''))?;
            let end = value[1..].find(quote)? + 1;
            if name.rsplit_once(':').map_or(name, |(_, l)| l) == local {
                return Some(&value[1..end]);
            }
            rest = &value[end + 1..];
        }
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

    #[test]
    fn attributes() {
        let raw = RawXml::new(r#"<w:lang xmlns:w="x" w:val='ru-RU' w:bidi = "ar-SA"/>"#);
        assert_eq!(raw.attribute("val"), Some("ru-RU"));
        assert_eq!(raw.attribute("bidi"), Some("ar-SA"));
        assert_eq!(raw.attribute("eastAsia"), None);
        let nested = RawXml::new(r#"<a x="1"><b y="2"/></a>"#);
        assert_eq!(nested.attribute("y"), None);
        assert_eq!(RawXml::new("<broken x=1>").attribute("x"), None);
        assert_eq!(RawXml::new("").attribute("x"), None);
    }
}
