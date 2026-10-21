//! Стили документа и значения по умолчанию.
//!
//! Здесь только хранение. Вычисление итоговых свойств с учётом наследования
//! (`basedOn`) появится в задаче F1.4.

use crate::{ParagraphProps, RawXml, RunProps};

/// Все стили документа (содержимое `word/styles.xml`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Styles {
    /// Оформление по умолчанию для всего документа.
    pub defaults: DocDefaults,
    /// Стили в порядке, в котором они записаны в файле.
    pub styles: Vec<Style>,
    /// Непонятые элементы `w:styles` (например, `w:latentStyles`).
    pub unknown: Vec<RawXml>,
}

impl Styles {
    /// Стиль по идентификатору (`w:styleId`), например `"Heading1"`.
    ///
    /// Если в файле несколько стилей с одним идентификатором, берётся первый.
    pub fn get(&self, id: &str) -> Option<&Style> {
        self.styles.iter().find(|s| s.id == id)
    }

    /// Стиль по умолчанию для данного вида (например, «Обычный» для абзацев).
    ///
    /// Если таких стилей несколько, берётся первый.
    pub fn default_for(&self, kind: StyleKind) -> Option<&Style> {
        self.styles.iter().find(|s| s.kind == kind && s.is_default)
    }
}

/// Оформление по умолчанию (`w:docDefaults`): база, на которую
/// накладываются все стили.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocDefaults {
    /// Оформление символов по умолчанию.
    pub run: RunProps,
    /// Оформление абзацев по умолчанию.
    pub paragraph: ParagraphProps,
}

/// Один стиль.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Style {
    /// Внутренний идентификатор (`w:styleId`), на него ссылаются абзацы и фрагменты.
    pub id: String,
    /// Имя, которое видит пользователь (например, `"heading 1"`).
    pub name: Option<String>,
    /// К чему применяется стиль.
    pub kind: StyleKind,
    /// Стиль по умолчанию для своего вида.
    pub is_default: bool,
    /// Родительский стиль (`w:basedOn`), от которого наследуются свойства.
    pub based_on: Option<String>,
    /// Стиль следующего абзаца после нажатия Enter (`w:next`).
    pub next: Option<String>,
    /// Оформление абзаца, заданное стилем.
    pub paragraph: ParagraphProps,
    /// Оформление символов, заданное стилем.
    pub run: RunProps,
    /// Стиль создан пользователем, а не встроен в Word (`w:customStyle`).
    pub custom: bool,
    /// Непонятые элементы стиля (`w:uiPriority`, `w:qFormat`, свойства
    /// таблиц и т. п.) в порядке из файла.
    pub unknown: Vec<RawXml>,
}

impl Style {
    /// Новый стиль без оформления и без родителя.
    pub fn new(id: impl Into<String>, kind: StyleKind) -> Style {
        Style {
            id: id.into(),
            name: None,
            kind,
            is_default: false,
            based_on: None,
            next: None,
            paragraph: ParagraphProps::default(),
            run: RunProps::default(),
            custom: false,
            unknown: Vec::new(),
        }
    }
}

/// Вид стиля.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StyleKind {
    /// Стиль абзаца.
    Paragraph,
    /// Стиль символов.
    Character,
    /// Стиль таблицы.
    Table,
    /// Стиль списка.
    Numbering,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_styles() {
        let mut normal = Style::new("Normal", StyleKind::Paragraph);
        normal.is_default = true;
        let mut heading = Style::new("Heading1", StyleKind::Paragraph);
        heading.based_on = Some("Normal".into());
        let styles = Styles {
            defaults: DocDefaults::default(),
            styles: vec![normal, heading],
            unknown: Vec::new(),
        };
        assert_eq!(
            styles.get("Heading1").and_then(|s| s.based_on.as_deref()),
            Some("Normal")
        );
        assert_eq!(styles.get("Missing"), None);
        assert_eq!(
            styles
                .default_for(StyleKind::Paragraph)
                .map(|s| s.id.as_str()),
            Some("Normal")
        );
        assert_eq!(styles.default_for(StyleKind::Character), None);
    }
}
