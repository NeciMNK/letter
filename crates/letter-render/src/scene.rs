//! Что нарисовать на странице: список глифов и прямоугольников
//! (подчёркивания, выделение маркером, заполнители табуляции)
//! в пикселях. Список одинаков для экрана и для картинки.

use letter_layout::{
    FontLibrary, LineItem, Page, PageLine, TabItem, TabLeader, TextItem, baseline_rise,
};
use letter_model::{Color, ResolvedRunProps, Underline};

use crate::glyphs::{GlyphCache, GlyphKey, SUBPIXEL_STEPS};

/// Цвет RGBA без предумножения.
pub type Rgba = [u8; 4];

/// Чёрный — цвет текста «Авто» на белом листе.
pub const BLACK: Rgba = [0, 0, 0, 255];

/// Что нарисовать. Координаты — пиксели от левого верхнего угла листа.
#[derive(Debug, Clone, PartialEq)]
pub enum DrawItem {
    /// Залитый прямоугольник.
    Rect {
        /// Левый край.
        x: f32,
        /// Верхний край.
        y: f32,
        /// Ширина.
        width: f32,
        /// Высота.
        height: f32,
        /// Цвет.
        color: Rgba,
    },
    /// Глиф.
    Glyph {
        /// Какой глиф и в каком размере.
        key: GlyphKey,
        /// Целая часть положения точки глифа по горизонтали (дробная —
        /// в `key.subpixel`).
        x: i32,
        /// Базовая линия, округлённая до пикселя.
        y: i32,
        /// Цвет.
        color: Rgba,
    },
}

/// Список того, что нарисовать на странице при масштабе `scale`
/// пикселей на пункт. Сначала идут фоны (выделение маркером), потом
/// глифы, потом линии.
pub fn page_scene(
    page: &Page,
    scale: f32,
    library: &FontLibrary,
    cache: &mut GlyphCache,
) -> Vec<DrawItem> {
    let mut backgrounds = Vec::new();
    let mut glyphs = Vec::new();
    let mut lines = Vec::new();
    for line in &page.lines {
        for item in &line.line.items {
            match item {
                LineItem::Text(text) => {
                    let Some(props) = line.spans.get(text.span) else {
                        continue;
                    };
                    let x = line.x + text.x;
                    let width = text.run.width();
                    if let Some((r, g, b)) = props.highlight.rgb() {
                        backgrounds.push(rect(
                            scale,
                            x,
                            line.baseline - line.line.ascent,
                            width,
                            line.line.ascent + line.line.descent,
                            [r, g, b, 255],
                        ));
                    }
                    let color = text_color(props);
                    push_glyphs(&mut glyphs, line, text, baseline_rise(props), scale, color);
                    decorate(
                        &mut lines, library, cache, line, text, props, x, width, scale, color,
                    );
                }
                LineItem::Tab(tab) => {
                    if let Some(props) = line.spans.get(tab.span) {
                        leader(&mut lines, line, tab, props, scale);
                    }
                }
            }
        }
    }
    backgrounds.extend(glyphs);
    backgrounds.extend(lines);
    backgrounds
}

fn rect(scale: f32, x: f32, y: f32, width: f32, height: f32, color: Rgba) -> DrawItem {
    DrawItem::Rect {
        x: x * scale,
        y: y * scale,
        width: width * scale,
        height: height * scale,
        color,
    }
}

/// Цвет текста. «Авто» на белом листе — чёрный.
pub fn text_color(props: &ResolvedRunProps) -> Rgba {
    match props.color {
        Color::Auto => BLACK,
        Color::Rgb(r, g, b) => [r, g, b, 255],
    }
}

/// Глифы куска; `rise` — подъём над базовой линией (индексы), пт.
fn push_glyphs(
    out: &mut Vec<DrawItem>,
    line: &PageLine,
    text: &TextItem,
    rise: f32,
    scale: f32,
    color: Rgba,
) {
    let run = &text.run;
    let size_64 = (run.size * scale * 64.0).round().max(1.0) as u32;
    let mut pen = line.x + text.x;
    for glyph in &run.glyphs {
        let x = (pen + glyph.x_offset) * scale;
        let y = (line.baseline - rise - glyph.y_offset) * scale;
        let whole = x.floor();
        let subpixel =
            (((x - whole) * f32::from(SUBPIXEL_STEPS)).floor() as u8).min(SUBPIXEL_STEPS - 1);
        out.push(DrawItem::Glyph {
            key: GlyphKey {
                font: run.font.id,
                glyph_id: glyph.glyph_id,
                size_64,
                subpixel,
                bold: run.font.synthetic_bold,
                italic: run.font.synthetic_italic,
            },
            x: whole as i32,
            y: y.round() as i32,
            color,
        });
        pen += glyph.x_advance;
    }
}

/// Подчёркивание и зачёркивание.
#[allow(clippy::too_many_arguments)]
fn decorate(
    out: &mut Vec<DrawItem>,
    library: &FontLibrary,
    cache: &mut GlyphCache,
    line: &PageLine,
    text: &TextItem,
    props: &ResolvedRunProps,
    x: f32,
    width: f32,
    scale: f32,
    color: Rgba,
) {
    let wants_lines = props.underline != Underline::None || props.strike || props.double_strike;
    if !wants_lines {
        return;
    }
    let size = text.run.size;
    let metrics = cache.metrics(library, text.run.font.id);
    let (underline_at, underline_thickness, strike_at, strike_thickness) =
        metrics.map_or((-0.1, 0.05, 0.3, 0.05), |m| {
            (
                m.underline_position,
                m.underline_thickness,
                m.strikeout_position,
                m.strikeout_thickness,
            )
        });
    // Линия не тоньше половины пиксельной точки при 100 %.
    let thin = |t: f32| (t * size).max(0.5);
    let mut bar = |offset: f32, thickness: f32| {
        let y = line.baseline - offset - thickness / 2.0;
        out.push(rect(scale, x, y, width, thickness, color));
    };
    match props.underline {
        Underline::None => {}
        Underline::Double | Underline::WavyDouble => {
            let t = thin(underline_thickness);
            bar(underline_at * size, t);
            bar(underline_at * size - 2.0 * t, t);
        }
        Underline::Thick
        | Underline::DottedHeavy
        | Underline::DashedHeavy
        | Underline::DashLongHeavy
        | Underline::DashDotHeavy
        | Underline::DashDotDotHeavy
        | Underline::WavyHeavy => bar(underline_at * size, 2.0 * thin(underline_thickness)),
        // Точки, штрихи и волны пока рисуются сплошной линией.
        _ => bar(underline_at * size, thin(underline_thickness)),
    }
    if props.strike {
        bar(strike_at * size, thin(strike_thickness));
    }
    if props.double_strike {
        let t = thin(strike_thickness);
        bar(strike_at * size + t, t);
        bar(strike_at * size - t, t);
    }
}

/// Заполнитель табуляции: точки, дефисы или линия.
fn leader(
    out: &mut Vec<DrawItem>,
    line: &PageLine,
    tab: &TabItem,
    props: &ResolvedRunProps,
    scale: f32,
) {
    let size = props.size.0 as f32 / 2.0;
    let color = text_color(props);
    let x = line.x + tab.x;
    let end = x + tab.width;
    let (step, mark, height, raise) = match tab.leader {
        TabLeader::None => return,
        TabLeader::Dot => (size * 0.25, size * 0.08, size * 0.08, 0.0),
        TabLeader::MiddleDot => (size * 0.25, size * 0.08, size * 0.08, size * 0.3),
        TabLeader::Hyphen => (size * 0.5, size * 0.3, size * 0.06, size * 0.25),
        TabLeader::Underscore => (0.0, 0.0, size * 0.05, -size * 0.1),
        TabLeader::Heavy => (0.0, 0.0, size * 0.1, -size * 0.1),
    };
    let y = line.baseline - raise - height;
    if step <= 0.0 {
        out.push(rect(scale, x, y, tab.width, height, color));
        return;
    }
    // Точки выровнены по сетке от левого поля, как в Word: соседние
    // строки с заполнителем выглядят ровными столбиками.
    let mut at = (x / step).ceil() * step;
    while at + mark <= end {
        out.push(rect(scale, at, y, mark, height, color));
        at += step;
    }
}
