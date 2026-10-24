//! Рисунок из фигур (SmartArt): контуры с заливкой и линией и текст
//! в фигурах. Здесь рисунок превращается в элементы сцены, а ещё
//! здесь разбиение многоугольника на треугольники для заливки.

use letter_layout::{FontLibrary, ShapedRun, TextStyle};
use letter_model::Drawing;
use letter_model::drawing::{ShapeText, TextAlign, TextAnchor};

use crate::glyphs::{GlyphCache, GlyphKey, SUBPIXEL_STEPS};
use crate::scene::DrawItem;

/// Шрифт текста фигуры, если в рисунке он не задан.
const DEFAULT_FONT: &str = "Calibri";

/// Рамка рисунка на странице, пт от левого верхнего угла листа.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DrawingFrame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Добавляет в `out` фигуры рисунка, растянутого на `frame`, при
/// масштабе `scale` пикселей на пункт.
pub(crate) fn draw_shapes(
    out: &mut Vec<DrawItem>,
    drawing: &Drawing,
    frame: DrawingFrame,
    scale: f32,
    library: &FontLibrary,
    cache: &mut GlyphCache,
) {
    if drawing.width <= 0 || drawing.height <= 0 {
        return;
    }
    // Пунктов страницы на EMU рисунка.
    let kx = frame.width / drawing.width as f32;
    let ky = frame.height / drawing.height as f32;
    let point = |(x, y): (i64, i64)| {
        (
            (frame.x + x as f32 * kx) * scale,
            (frame.y + y as f32 * ky) * scale,
        )
    };
    // Во сколько раз рисунок растянут против исходного размера: так же
    // меняются толщина линий и кегль.
    let stretch = kx.min(ky) * letter_model::EMU_PER_POINT as f32;
    for shape in &drawing.shapes {
        let stroke = shape.line.map(|line| {
            let width = line.width as f32 / letter_model::EMU_PER_POINT as f32 * stretch;
            (line.color, (width * scale).max(0.5))
        });
        for contour in &shape.contours {
            let points: Vec<(f32, f32)> = contour.points.iter().copied().map(point).collect();
            let fill = shape.fill.filter(|_| contour.closed);
            if fill.is_none() && stroke.is_none() {
                continue;
            }
            out.push(DrawItem::Path {
                points,
                closed: contour.closed,
                fill,
                stroke,
            });
        }
        if let Some(text) = &shape.text {
            let area = TextArea {
                x: frame.x + text.x as f32 * kx,
                y: frame.y + text.y as f32 * ky,
                width: text.width as f32 * kx,
                height: text.height as f32 * ky,
                stretch,
            };
            draw_text(out, text, area, scale, library, cache);
        }
    }
}

/// Область текста на странице, пт.
struct TextArea {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    /// Множитель кегля.
    stretch: f32,
}

/// Слово или пробелы с глифами.
struct Piece {
    runs: Vec<ShapedRun>,
    width: f32,
    space: bool,
    color: [u8; 4],
}

/// Строка текста фигуры.
#[derive(Default)]
struct TextLine {
    pieces: Vec<Piece>,
    ascent: f32,
    descent: f32,
    align: TextAlign,
}

impl TextLine {
    /// Ширина без пробелов в конце.
    fn width(&self) -> f32 {
        let end = self
            .pieces
            .iter()
            .rposition(|p| !p.space)
            .map_or(0, |i| i + 1);
        self.pieces[..end].iter().map(|p| p.width).sum()
    }
}

fn draw_text(
    out: &mut Vec<DrawItem>,
    text: &ShapeText,
    area: TextArea,
    scale: f32,
    library: &FontLibrary,
    cache: &mut GlyphCache,
) {
    let mut lines: Vec<TextLine> = Vec::new();
    for paragraph in &text.paragraphs {
        let mut line = TextLine {
            align: paragraph.align,
            ..TextLine::default()
        };
        let mut x = 0.0;
        for run in &paragraph.runs {
            let size = run.size as f32 / 100.0 * area.stretch;
            let mut style =
                TextStyle::new(run.font.as_deref().unwrap_or(DEFAULT_FONT), size.max(0.5));
            style.bold = run.bold;
            style.italic = run.italic;
            for word in words(&run.text) {
                let newline = word == "\n";
                let runs = if newline {
                    Vec::new()
                } else {
                    cache.shaper().shape(library, word, &style)
                };
                let width: f32 = runs.iter().map(ShapedRun::width).sum();
                let (ascent, descent) = runs
                    .first()
                    .and_then(|r| cache.metrics(library, r.font.id))
                    .map_or((0.8 * size, 0.2 * size), |m| {
                        (m.ascent * size, m.descent * size)
                    });
                let piece = Piece {
                    runs,
                    width,
                    space: word.chars().all(char::is_whitespace),
                    color: run.color,
                };
                let overflow = x + width > area.width && !piece.space && !line.pieces.is_empty();
                if newline || overflow {
                    let next = TextLine {
                        align: paragraph.align,
                        ..TextLine::default()
                    };
                    let mut done = std::mem::replace(&mut line, next);
                    if newline {
                        done.ascent = done.ascent.max(ascent);
                        done.descent = done.descent.max(descent);
                    }
                    lines.push(done);
                    x = 0.0;
                    if newline || piece.space {
                        continue;
                    }
                }
                x += width;
                line.ascent = line.ascent.max(ascent);
                line.descent = line.descent.max(descent);
                line.pieces.push(piece);
            }
        }
        if line.pieces.is_empty() && line.ascent == 0.0 {
            // Пустой абзац — строка высотой в его кегль.
            let size = paragraph.size as f32 / 100.0 * area.stretch;
            line.ascent = 0.8 * size;
            line.descent = 0.2 * size;
        }
        lines.push(line);
    }
    let total: f32 = lines.iter().map(|l| l.ascent + l.descent).sum();
    let mut top = match text.anchor {
        TextAnchor::Top => area.y,
        TextAnchor::Middle => area.y + (area.height - total) / 2.0,
        TextAnchor::Bottom => area.y + area.height - total,
    };
    for line in &lines {
        let baseline = top + line.ascent;
        let width = line.width();
        let mut pen = match line.align {
            TextAlign::Left => area.x,
            TextAlign::Center => area.x + (area.width - width) / 2.0,
            TextAlign::Right => area.x + area.width - width,
        };
        for piece in &line.pieces {
            if !piece.space {
                let mut x = pen;
                for run in &piece.runs {
                    push_run(out, run, x, baseline, scale, piece.color);
                    x += run.width();
                }
            }
            pen += piece.width;
        }
        top += line.ascent + line.descent;
    }
}

/// Делит текст на слова, пробелы и переводы строки.
fn words(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut kind = None;
    for (i, c) in text.char_indices() {
        let k = if c == '\n' {
            2
        } else if c.is_whitespace() {
            1
        } else {
            0
        };
        if kind.is_some_and(|prev| prev != k || k == 2) {
            out.push(&text[start..i]);
            start = i;
        }
        kind = Some(k);
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// Глифы куска от точки `(x, baseline)`, пт.
fn push_run(
    out: &mut Vec<DrawItem>,
    run: &ShapedRun,
    x: f32,
    baseline: f32,
    scale: f32,
    color: [u8; 4],
) {
    let size_64 = (run.size * scale * 64.0).round().max(1.0) as u32;
    let mut pen = x;
    for glyph in &run.glyphs {
        let gx = (pen + glyph.x_offset) * scale;
        let gy = (baseline - glyph.y_offset) * scale;
        let whole = gx.floor();
        let subpixel =
            (((gx - whole) * f32::from(SUBPIXEL_STEPS)).floor() as u8).min(SUBPIXEL_STEPS - 1);
        out.push(DrawItem::Glyph {
            key: GlyphKey {
                font: run.font.id,
                glyph_id: glyph.glyph_id,
                size_64,
                subpixel,
                bold: run.font.synthetic_bold,
                italic: run.font.synthetic_italic,
                width_percent: (run.width_scale * 100.0).round().clamp(1.0, 600.0) as u16,
                rotation: 0,
            },
            x: whole as i32,
            y: gy.round() as i32,
            color,
        });
        pen += glyph.x_advance;
    }
}

/// Разбивает многоугольник на треугольники («отрезание ушей»); работает
/// и для невыпуклых фигур (шеврон, стрелка). Возвращает тройки номеров
/// точек. Повторяющиеся подряд точки пропускаются.
pub fn triangulate(points: &[(f32, f32)]) -> Vec<[usize; 3]> {
    let mut index: Vec<usize> = Vec::with_capacity(points.len());
    for (i, p) in points.iter().enumerate() {
        let same = index
            .last()
            .is_some_and(|&j| (points[j].0 - p.0).abs() < 1e-3 && (points[j].1 - p.1).abs() < 1e-3);
        if !same {
            index.push(i);
        }
    }
    if index.len() > 1 {
        let (first, last) = (points[index[0]], points[index[index.len() - 1]]);
        if (first.0 - last.0).abs() < 1e-3 && (first.1 - last.1).abs() < 1e-3 {
            index.pop();
        }
    }
    let mut triangles = Vec::new();
    if index.len() < 3 {
        return triangles;
    }
    // Обход против или по часовой стрелке — по знаку площади.
    let area: f32 = (0..index.len())
        .map(|i| {
            let a = points[index[i]];
            let b = points[index[(i + 1) % index.len()]];
            a.0 * b.1 - b.0 * a.1
        })
        .sum();
    let sign = if area >= 0.0 { 1.0 } else { -1.0 };
    let cross = |a: (f32, f32), b: (f32, f32), c: (f32, f32)| {
        ((b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)) * sign
    };
    let mut guard = 0;
    while index.len() > 3 && guard < 4 * points.len() + 16 {
        guard += 1;
        let n = index.len();
        let mut cut = None;
        for i in 0..n {
            let (ia, ib, ic) = (index[(i + n - 1) % n], index[i], index[(i + 1) % n]);
            let (a, b, c) = (points[ia], points[ib], points[ic]);
            if cross(a, b, c) <= 0.0 {
                continue;
            }
            let inside = index.iter().any(|&j| {
                if j == ia || j == ib || j == ic {
                    return false;
                }
                let p = points[j];
                cross(a, b, p) > 0.0 && cross(b, c, p) > 0.0 && cross(c, a, p) > 0.0
            });
            if !inside {
                cut = Some(i);
                break;
            }
        }
        // Нет «уха» (контур с самопересечением) — отрезаем любой угол.
        let i = cut.unwrap_or(0);
        let n = index.len();
        triangles.push([index[(i + n - 1) % n], index[i], index[(i + 1) % n]]);
        index.remove(i);
    }
    if index.len() == 3 {
        triangles.push([index[0], index[1], index[2]]);
    }
    triangles
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(points: &[(f32, f32)], triangles: &[[usize; 3]]) -> f32 {
        triangles
            .iter()
            .map(|t| {
                let (a, b, c) = (points[t[0]], points[t[1]], points[t[2]]);
                ((b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)).abs() / 2.0
            })
            .sum()
    }

    #[test]
    fn square_is_two_triangles() {
        let square = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let triangles = triangulate(&square);
        assert_eq!(triangles.len(), 2);
        assert!((area(&square, &triangles) - 100.0).abs() < 1e-3);
    }

    #[test]
    fn chevron_keeps_its_notch() {
        // Шеврон невыпуклый: площадь — без выреза слева.
        let chevron = [
            (0.0, 0.0),
            (6.0, 0.0),
            (10.0, 5.0),
            (6.0, 10.0),
            (0.0, 10.0),
            (4.0, 5.0),
        ];
        let triangles = triangulate(&chevron);
        assert_eq!(triangles.len(), 4);
        assert!((area(&chevron, &triangles) - 60.0).abs() < 1e-3);
        // Обход в другую сторону — то же самое.
        let mut reversed = chevron;
        reversed.reverse();
        assert!((area(&reversed, &triangulate(&reversed)) - 60.0).abs() < 1e-3);
    }

    #[test]
    fn words_split_spaces_and_breaks() {
        assert_eq!(words("ab  c\nd"), vec!["ab", "  ", "c", "\n", "d"]);
        assert_eq!(words("\n\n"), vec!["\n", "\n"]);
    }
}
