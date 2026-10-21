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
//!   налево.

pub mod fonts;
pub mod lines;
pub mod shaping;

pub use fonts::{FontId, FontLibrary, FontMatch};
pub use lines::{
    Align, Line, LineEnd, LineItem, ParagraphFormat, ParagraphLines, TabItem, TabKind, TabLeader,
    TabStop, TextItem, TextSpan, break_lines,
};
pub use shaping::{FontMetrics, ShapedGlyph, ShapedRun, Shaper, TextStyle};
