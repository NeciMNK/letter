//! Вёрстка документа: шрифты, разбиение на строки и страницы.
//!
//! Сейчас готово:
//!
//! - [`fonts`] — поиск шрифтов (системных и встроенных) и замена
//!   отсутствующих похожими;
//! - [`shaping`] — превращение текста в глифы с размерами (лигатуры,
//!   кернинг, письмо справа налево, подбор шрифта для отдельных букв);
//! - [`lines`] — разбиение абзаца на строки: перенос по словам,
//!   выравнивание, отступы, табуляции, порядок показа текста справа
//!   налево;
//! - [`pages`] — документ целиком: перевод модели в типы вёрстки
//!   и раскладка строк по страницам;
//! - [`spacing`] — межстрочный и межабзацный интервалы: строки и абзацы
//!   по вертикали;
//! - `table` — таблицы: столбцы, ячейки, границы и заливка.

mod fields;
pub mod fonts;
pub mod lines;
mod notes;
pub mod pages;
mod revisions;
pub mod shaping;
pub mod spacing;
mod table;

pub use fonts::{FontId, FontLibrary, FontMatch};
pub use lines::{
    Align, Hyphenation, InlineObject, Line, LineEnd, LineItem, LineRoom, ObjectItem,
    ParagraphFormat, ParagraphLines, TabItem, TabKind, TabLeader, TabStop, TextItem, TextSpan,
    break_lines, break_lines_around,
};
pub use pages::{
    CellArea, DocumentLayout, Float, LayoutCache, Margins, Page, PageLine, PageNote, PageStory,
    PageWatermark, Shape, ShapeKind, baseline_rise, layout_document, layout_document_cached,
};
pub use revisions::{mark_revision, revision_color};
pub use shaping::{FontMetrics, ShapedGlyph, ShapedRun, Shaper, TextStyle};
pub use spacing::{
    LineBox, LineRule, ParagraphBlock, ParagraphSpacing, PlacedParagraph, effective_spacing,
    gap_between, place_lines, stack_paragraphs,
};
