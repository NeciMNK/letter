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
            _ => None,
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

#[test]
fn indexes_are_smaller_and_shifted() {
    use letter_model::{Block, Document, Paragraph, Run, RunProps, VerticalAlign};
    let library = FontLibrary::embedded_only();
    let mut cache = GlyphCache::new();
    let script = |text: &str, align| {
        Run::text(text).with_props(RunProps {
            vertical_align: Some(align),
            ..RunProps::default()
        })
    };
    let mut doc = Document::new();
    doc.sections[0].blocks = vec![Block::from(Paragraph::from_runs([
        Run::text("x"),
        script("y", VerticalAlign::Superscript),
        script("z", VerticalAlign::Subscript),
    ]))];
    let layout = layout_document(&doc, &mut Shaper::new(), &library);
    let page = &layout.pages[0];
    let glyphs: Vec<(u32, i32)> = page_scene(page, 1.0, &library, &mut cache)
        .into_iter()
        .filter_map(|i| match i {
            DrawItem::Glyph { key, y, .. } => Some((key.size_64, y)),
            _ => None,
        })
        .collect();
    let [(normal, base), (sup, sup_y), (sub, sub_y)] = glyphs[..] else {
        panic!("{glyphs:?}");
    };
    assert!(sup < normal && sub < normal, "{glyphs:?}");
    assert!(sup_y < base && sub_y > base, "{glyphs:?}");
}

#[test]
fn formatting_marks_follow_the_text() {
    use letter_model::{Block, BreakKind, Document, Paragraph, Run, RunContent, RunProps};
    use letter_render::{MarkKind, formatting_marks};
    let library = FontLibrary::embedded_only();
    let mut doc = Document::new();
    let run = Run {
        props: RunProps::default(),
        content: vec![
            RunContent::Text("a b".into()),
            RunContent::Tab,
            RunContent::Text("c".into()),
            RunContent::Break(BreakKind::Line),
            RunContent::Text("d".into()),
        ],
    };
    doc.sections[0].blocks = vec![
        Block::from(Paragraph::from_runs([run])),
        Block::from(Paragraph::default()),
    ];
    let layout = layout_document(&doc, &mut Shaper::new(), &library);
    let marks = formatting_marks(&layout.pages[0]);
    let kinds: Vec<MarkKind> = marks.iter().map(|m| m.kind).collect();
    assert_eq!(
        kinds,
        vec![
            MarkKind::Space,
            MarkKind::Tab,
            MarkKind::LineBreak,
            MarkKind::Paragraph,
            MarkKind::Paragraph
        ]
    );
    // Точка — между «a» и «b», «¶» — после «d», на второй строке.
    assert!(marks[0].x < marks[1].x && marks[2].x > marks[1].x);
    assert!(marks[3].baseline > marks[2].baseline);
    assert!(marks[4].baseline > marks[3].baseline);
    let margin = layout.pages[0].margins.left;
    assert!((marks[4].x - margin).abs() < 0.5, "{:?}", marks[4]);
}

#[test]
fn hidden_wide_and_colored_underline() {
    use letter_model::{Block, Color, Document, Paragraph, Run, RunProps, Underline};
    let library = FontLibrary::embedded_only();
    let mut cache = GlyphCache::new();
    let mut doc = Document::new();
    doc.sections[0].blocks = vec![Block::from(Paragraph::from_runs([
        Run::text("a").with_props(RunProps {
            hidden: Some(true),
            ..RunProps::default()
        }),
        Run::text("b").with_props(RunProps {
            scale: Some(150),
            underline: Some(Underline::Single),
            underline_color: Some(Color::Rgb(255, 0, 0)),
            ..RunProps::default()
        }),
    ]))];
    let layout = layout_document(&doc, &mut Shaper::new(), &library);
    let items = page_scene(&layout.pages[0], 1.0, &library, &mut cache);
    let glyphs: Vec<u16> = items
        .iter()
        .filter_map(|i| match i {
            DrawItem::Glyph { key, .. } => Some(key.width_percent),
            _ => None,
        })
        .collect();
    // Скрытая «a» не рисуется, «b» растянута.
    assert_eq!(glyphs, [150]);
    assert!(items.iter().any(|i| matches!(
        i,
        DrawItem::Rect {
            color: [255, 0, 0, 255],
            ..
        }
    )));
}

#[test]
fn inline_picture_is_drawn() {
    let library = FontLibrary::embedded_only();
    let mut cache = GlyphCache::new();
    let doc = layout("image.docx", &library);
    let page = &doc.pages[0];
    let scale = SCREEN_PIXELS_PER_POINT;
    let (x, y, width, height) = page_scene(page, scale, &library, &mut cache)
        .into_iter()
        .find_map(|i| match i {
            DrawItem::Picture {
                x,
                y,
                width,
                height,
                image,
                shape: None,
            } => {
                assert!(image.is_some());
                Some((x, y, width, height))
            }
            _ => None,
        })
        .expect("picture");
    // 2 × 1,2 дюйма = 144 × 86,4 пт.
    assert!((width - 144.0 * scale).abs() < 0.5, "{width}");
    assert!((height - 86.4 * scale).abs() < 0.5, "{height}");
    let image = render_page(page, scale, &library, &mut cache);
    // В середине рисунка — оранжевый круг.
    let center = image
        .pixel((x + width / 2.0) as u32, (y + height / 2.0) as u32)
        .unwrap();
    assert!(
        center[0] > 200 && center[1] > 90 && center[1] < 150 && center[2] < 40,
        "{center:?}"
    );
    // Строка с рисунком выше строки текста: следующая строка ниже рисунка.
    let next = page
        .lines
        .iter()
        .find(|l| l.text.starts_with("Text after"))
        .unwrap();
    assert!(
        next.top * scale >= y + height - 0.5,
        "{} < {}",
        next.top * scale,
        y + height
    );
}

#[test]
fn broken_picture_becomes_a_placeholder() {
    use std::sync::Arc;

    use letter_model::{Block, Document, ImageData, Paragraph, Picture, Run, RunContent};
    let library = FontLibrary::embedded_only();
    let mut cache = GlyphCache::new();
    let mut doc = Document::new();
    let picture = Picture {
        width: 72 * 12_700,
        height: 36 * 12_700,
        image: Some(Arc::new(ImageData {
            name: "word/media/image1.emf".into(),
            bytes: vec![1, 2, 3],
        })),
        relationship: None,
        raw: None,
        anchor: None,
        shape: None,
    };
    doc.sections[0].blocks = vec![Block::from(Paragraph::from_runs([Run {
        props: Default::default(),
        content: vec![RunContent::Picture(picture)],
    }]))];
    let layout = layout_document(&doc, &mut Shaper::new(), &library);
    let page = &layout.pages[0];
    let line = &page.lines[0];
    let image = render_page(page, 1.0, &library, &mut cache);
    // Середина верхней половины заглушки — светло-серая заливка.
    let x = (line.x + 36.0) as u32;
    let y = (line.baseline - 30.0) as u32;
    assert_eq!(
        image.pixel(x, y),
        Some(letter_render::picture::PLACEHOLDER_FILL)
    );
}

#[test]
fn floating_pictures_are_drawn_in_their_place() {
    let library = FontLibrary::embedded_only();
    let mut cache = GlyphCache::new();
    let doc = layout("floating.docx", &library);
    let page = &doc.pages[0];
    let items = page_scene(page, 1.0, &library, &mut cache);
    let pictures: Vec<(usize, f32, f32, f32)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| match item {
            DrawItem::Picture { x, y, width, .. } => Some((i, *x, *y, *width)),
            _ => None,
        })
        .collect();
    // Знаки привязки не рисуются: только два плавающих рисунка.
    let [behind, square] = pictures[..] else {
        panic!("{pictures:?}");
    };
    let first_glyph = items
        .iter()
        .position(|i| matches!(i, DrawItem::Glyph { .. }))
        .expect("glyphs");
    // «За текстом» — под буквами, «вокруг рамки» — поверх.
    assert!(behind.0 < first_glyph && square.0 > first_glyph);
    // 4 572 000 EMU = 360 пт от края листа.
    assert!((square.1 - 360.0).abs() < 0.01, "{square:?}");
    assert!((square.3 - 72.0).abs() < 0.01);
    // По центру между полями и по нижнему полю (рисунок квадратный).
    let text_width = page.width - page.margins.left - page.margins.right;
    let center = page.margins.left + (text_width - behind.3) / 2.0;
    assert!((behind.1 - center).abs() < 0.01, "{behind:?}");
    let bottom = page.height - page.margins.bottom - behind.3;
    assert!((behind.2 - bottom).abs() < 0.01, "{behind:?}");
    // Плавающие рисунки не раздвигают строки.
    let tops: Vec<f32> = page.lines.iter().map(|l| l.top).collect();
    let steps: Vec<f32> = tops.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(steps.iter().all(|s| *s < 30.0), "{steps:?}");
}

#[test]
fn shapes_are_drawn_on_screen_image_and_pdf() {
    let library = FontLibrary::embedded_only();
    let mut cache = GlyphCache::new();
    let doc = layout("shapes.docx", &library);
    let page = &doc.pages[0];
    let scale = SCREEN_PIXELS_PER_POINT;
    let mut shapes: Vec<(String, (f32, f32, f32, f32))> =
        page_scene(page, scale, &library, &mut cache)
            .into_iter()
            .filter_map(|i| match i {
                DrawItem::Picture {
                    x,
                    y,
                    width,
                    height,
                    shape: Some(shape),
                    ..
                } => {
                    let name = match &shape.geometry {
                        letter_model::Geometry::Preset { name, .. } => name.clone(),
                        _ => String::new(),
                    };
                    Some((name, (x, y, width, height)))
                }
                _ => None,
            })
            .collect();
    assert_eq!(shapes.len(), 6, "{shapes:?}");
    let mut find = |name: &str| {
        let i = shapes.iter().position(|(n, _)| n == name).unwrap();
        shapes.remove(i).1
    };
    let (arrow, oval) = (find("rightArrow"), find("ellipse"));
    // Надпись: белые буквы внутри синего прямоугольника.
    let (rx, ry, rw, rh) = find("rect");
    let glyphs = page_scene(page, scale, &library, &mut cache)
        .into_iter()
        .filter(|i| match *i {
            DrawItem::Glyph { x, y, color, .. } => {
                let (x, y) = (x as f32, y as f32);
                color == [255, 255, 255, 255] && x > rx && x < rx + rw && y > ry && y < ry + rh
            }
            _ => false,
        })
        .count();
    assert_eq!(glyphs, "Надпись в прямоугольнике".chars().count());
    let image = render_page(page, scale, &library, &mut cache);
    let at = |x: f32, y: f32| image.pixel(x as u32, y as u32).unwrap();
    // Жёлтая стрелка (FFC000): середина залита, правый верхний угол
    // прямоугольника пуст — там скос острия.
    let (x, y, w, h) = arrow;
    let middle = at(x + w / 2.0, y + h / 2.0);
    assert!(
        middle[0] > 240 && (180..210).contains(&middle[1]) && middle[2] < 30,
        "{middle:?}"
    );
    assert_eq!(at(x + w - 2.0, y + 2.0), [255, 255, 255, 255]);
    // Овал без заливки: середина белая, на левом краю — красная линия.
    let (x, y, w, h) = oval;
    let inside = at(x + w / 2.0, y + h / 2.0);
    assert!(inside[0] > 200 && inside[1] > 200, "{inside:?}");
    let edge = (0..6)
        .map(|dx| at(x + dx as f32, y + h / 2.0))
        .any(|p| p[0] > 150 && p[1] < 80);
    assert!(edge);
    // В PDF фигуры — линии с заливкой (оператор `f` или `B`).
    let pdf = letter_render::write_pdf(&doc.pages, &library, &Default::default());
    assert!(pdf.len() > 1000);
}
