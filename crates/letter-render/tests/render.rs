//! Отрисовка настоящих документов из `tests/fixtures/docx`.

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use letter_layout::{DocumentLayout, FontLibrary, Shaper, layout_document};
use letter_render::{DrawItem, GlyphCache, SCREEN_PIXELS_PER_POINT, page_scene, render_page};

fn layout(name: &str, library: &FontLibrary) -> DocumentLayout {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/docx")
        .join(name);
    let doc = letter_docx::read_file(&path).unwrap();
    layout_document(&doc, &mut Shaper::new(), library)
}

fn is_dark(p: [u8; 4]) -> bool {
    u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 300
}

#[test]
fn scene_has_glyphs_and_decorations() {
    let library = FontLibrary::embedded_only();
    let mut cache = GlyphCache::new();
    let doc = layout("bold-italic.docx", &library);
    let page = &doc.pages[0];
    let items = page_scene(page, 1.0, &library, &mut cache);
    let glyphs: Vec<_> = items
        .iter()
        .filter_map(|i| match i {
            DrawItem::Glyph { color, .. } => Some(*color),
            DrawItem::Rect { .. } => None,
        })
        .collect();
    // Глифов не меньше, чем непробельных букв в строке (лигатуры
    // могут склеить пару букв).
    let letters = page.lines[0]
        .text
        .chars()
        .filter(|c| !c.is_whitespace())
        .count();
    assert!(glyphs.len() + 2 >= letters, "{} < {letters}", glyphs.len());
    // «red 16pt» — красным.
    assert!(glyphs.iter().any(|c| c[0] > 150 && c[1] < 50));
    // Подчёркивание и зачёркивание — прямоугольники после глифов.
    let rects = items
        .iter()
        .skip_while(|i| matches!(i, DrawItem::Rect { .. }))
        .filter(|i| matches!(i, DrawItem::Rect { .. }))
        .count();
    assert!(rects >= 2, "{rects}");
}

#[test]
fn pages_are_drawn_where_the_text_is() {
    let library = FontLibrary::embedded_only();
    let mut cache = GlyphCache::new();
    let doc = layout("headings.docx", &library);
    let page = &doc.pages[0];
    let scale = SCREEN_PIXELS_PER_POINT;
    let image = render_page(page, scale, &library, &mut cache);
    assert_eq!((image.width, image.height), (816, 1056));
    // Поля пустые: левый верхний угол белый.
    assert_eq!(image.pixel(10, 10), Some([255, 255, 255, 255]));
    // В полосе первой строки есть тёмные пиксели, над ней — нет.
    let band = |top: f32, bottom: f32| {
        let mut dark = 0;
        for y in (top * scale) as u32..(bottom * scale) as u32 {
            for x in 0..image.width {
                if image.pixel(x, y).is_some_and(is_dark) {
                    dark += 1;
                }
            }
        }
        dark
    };
    let first = &page.lines[0];
    assert!(band(first.top, first.top + first.height) > 100);
    assert_eq!(band(0.0, first.top - 1.0), 0);
    // Последняя строка тоже нарисована.
    let last = page.lines.last().unwrap();
    assert!(band(last.top, last.top + last.height) > 100);
}
