//! Сноски и концевые сноски. В тексте документа — ссылка на сноску
//! ([`crate::RunContent::NoteReference`]), которая показывается номером;
//! сам текст сноски лежит в [`crate::Document::footnotes`] или
//! [`crate::Document::endnotes`] и начинается с того же номера
//! ([`crate::RunContent::NoteMark`]).

use crate::Block;

/// Вид сноски.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoteKind {
    /// Обычная сноска внизу страницы (`w:footnote`).
    Footnote,
    /// Концевая сноска в конце документа (`w:endnote`).
    Endnote,
}

/// Ссылка на сноску в тексте (`w:footnoteReference`, `w:endnoteReference`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteReference {
    /// Вид сноски.
    pub kind: NoteKind,
    /// Номер сноски в части сносок (`w:id`), не тот номер, что виден.
    pub id: String,
    /// Свой знак сноски (`w:customMarkFollows`): номер не показывается,
    /// знак — текст следующего фрагмента.
    pub custom_mark: bool,
}

/// Сноска: её текст.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Note {
    /// Номер (`w:id`), по нему на сноску ссылаются из текста.
    pub id: String,
    /// Служебная сноска (`w:type`): `separator` — черта над сносками,
    /// `continuationSeparator` — черта над продолжением сноски,
    /// `continuationNotice`; у обычной — `None`.
    pub special: Option<String>,
    /// Текст сноски.
    pub blocks: Vec<Block>,
}

impl Note {
    /// Обычная (не служебная) сноска.
    pub fn is_regular(&self) -> bool {
        self.special.is_none()
    }
}
