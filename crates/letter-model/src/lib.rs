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

mod border;
mod document;
mod numbering;
mod package;
mod paragraph;
mod picture;
mod properties;
mod raw;
mod resolve;
mod run;
mod section;
mod style;
mod table;
mod theme;
mod units;

pub use border::{Border, Borders, Shading};
pub use document::{Document, Settings};
pub use numbering::{
    AbstractNumbering, LevelOverride, LevelSuffix, ListCounters, ListLabel, ListLevel,
    NumberFormat, Numbering, NumberingInstance, NumberingRef,
};
pub use package::{SourcePackage, SourcePart};
pub use paragraph::{
    Alignment, FirstLineIndent, Indentation, Inline, LineSpacing, Paragraph, ParagraphProps,
    Spacing, TabLeader, TabStop, TabStopKind,
};
pub use picture::{
    Anchor, AnchorAlign, AnchorPosition, Distance, EMU_PER_POINT, HorizontalFrom, ImageData,
    Picture, Placement, VerticalFrom, Wrap,
};
pub use properties::{AppProperties, CoreProperties, Properties, Statistics};
pub use raw::RawXml;
pub use resolve::{ResolvedFonts, ResolvedParagraphProps, ResolvedRunProps, overlay_borders};
pub use run::{
    BreakKind, Color, Highlight, OBJECT_CHAR, Run, RunContent, RunFonts, RunProps, ThemeColor,
    ThemeFont, Underline, VerticalAlign,
};
pub use section::{Orientation, PageMargins, PageSize, Section, SectionProps};
pub use style::{DocDefaults, Style, StyleKind, Styles};
pub use table::{
    Block, CellMargins, CellProps, CellVerticalAlign, HeightRule, RowHeight, RowProps, Table,
    TableCell, TableLayout, TableProps, TableRow, TableWidth, VerticalMerge,
};
pub use theme::{Theme, ThemeFontSet};
pub use units::{HalfPoints, Twips};
