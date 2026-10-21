//! Модель документа: абзацы, фрагменты текста, стили, таблицы.
//!
//! Модель повторяет устройство документа Word (OOXML), но ничего не знает
//! о файлах: чтением и записью занимаются другие крейты (`letter-docx` и т. д.).
//!
//! Устройство:
//!
//! - [`Document`] — весь документ: разделы, стили, нумерация, настройки.
//! - [`Section`] — раздел со своими размерами страницы и полями.
//! - [`Block`] — элемент раздела: абзац ([`Paragraph`]) или таблица ([`Table`]).
//! - [`Run`] — фрагмент абзаца с одинаковым оформлением символов.
//!
//! Все свойства оформления необязательны (`Option`): `None` значит
//! «не задано здесь, взять из стиля или значений по умолчанию».
//! Расстояния хранятся в твипах ([`Twips`]), как в Word.
//!
//! То, что программа пока не понимает, хранится как [`RawXml`] и при
//! сохранении записывается обратно без изменений.

mod document;
mod paragraph;
mod raw;
mod run;
mod section;
mod style;
mod table;
mod units;

pub use document::{Document, Numbering, Settings};
pub use paragraph::{
    Alignment, FirstLineIndent, Indentation, Inline, LineSpacing, Paragraph, ParagraphProps,
    Spacing,
};
pub use raw::RawXml;
pub use run::{
    BreakKind, Color, Highlight, Run, RunContent, RunFonts, RunProps, Underline, VerticalAlign,
};
pub use section::{Orientation, PageMargins, PageSize, Section, SectionProps};
pub use style::{DocDefaults, Style, StyleKind, Styles};
pub use table::{Block, Table, TableCell, TableRow};
pub use units::{HalfPoints, Twips};
