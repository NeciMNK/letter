//! Вёрстка документа: шрифты, разбиение на строки и страницы.
//!
//! Сейчас готово:
//!
//! - [`fonts`] — поиск шрифтов (системных и встроенных) и замена
//!   отсутствующих похожими;
//! - [`shaping`] — превращение текста в глифы с размерами (лигатуры,
//!   кернинг, письмо справа налево, подбор шрифта для отдельных букв).

pub mod fonts;
pub mod shaping;

pub use fonts::{FontId, FontLibrary, FontMatch};
pub use shaping::{FontMetrics, ShapedGlyph, ShapedRun, Shaper, TextStyle};
