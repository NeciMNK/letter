//! Отрисовка свёрстанных страниц.
//!
//! - [`glyphs`] — растровые изображения букв (`swash`) с кешем;
//! - [`scene`] — что нарисовать на странице: глифы, подчёркивания,
//!   выделение маркером, заполнители табуляции;
//! - [`drawing`] — рисунки из фигур (SmartArt);
//! - [`marks`] — знаки форматирования (¶, пробелы, табуляции);
//! - [`picture`] — распаковка картинок рисунков;
//! - [`image`] — отрисовка страницы в картинку в памяти (тесты, сверка);
//! - [`screen`] — отрисовка на экране через egui: серый фон, белые листы
//!   с тенью, текст.
//!
//! Масштаб задаётся в пикселях на пункт: при 100 % на обычном экране
//! это 96 / 72 (см. [`SCREEN_PIXELS_PER_POINT`]).

pub mod drawing;
pub mod glyphs;
pub mod image;
pub mod marks;
pub mod pdf;
pub mod picture;
pub mod scene;
pub mod screen;

pub use glyphs::{GlyphCache, GlyphImage, GlyphKey, GlyphPixels};
pub use image::{RgbaImage, render_page};
pub use marks::{Mark, MarkKind, formatting_marks};
pub use pdf::{OutlineItem, PdfInfo, write_pdf};
pub use scene::{DrawItem, Rgba, page_scene};
pub use screen::{DESK_COLOR, PagePainter};

/// Сколько точек экрана (egui) приходится на пункт документа при
/// масштабе 100 %: экран считается в 96 точек на дюйм, пункт — 1/72
/// дюйма, как в Word.
pub const SCREEN_PIXELS_PER_POINT: f32 = 96.0 / 72.0;
