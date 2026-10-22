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
//!   по вертикали.

pub mod fonts;
pub mod lines;
pub mod pages;
pub mod shaping;
pub mod spacing;

pub use fonts::{FontId, FontLibrary, FontMatch};
pub use lines::{
    Align, Line, LineEnd, LineItem, ParagraphFormat, ParagraphLines, TabItem, TabKind, TabLeader,
    TabStop, TextItem, TextSpan, break_lines,
};
pub use pages::{
    DocumentLayout, LayoutCache, Margins, Page, PageLine, baseline_rise, layout_document,
    layout_document_cached,
};
pub use shaping::{FontMetrics, ShapedGlyph, ShapedRun, Shaper, TextStyle};
pub use spacing::{
    LineBox, LineRule, ParagraphBlock, ParagraphSpacing, PlacedParagraph, effective_spacing,
    gap_between, place_lines, stack_paragraphs,
};
