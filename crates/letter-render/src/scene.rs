//! Что нарисовать на странице: список глифов и прямоугольников
//! (подчёркивания, выделение маркером, заполнители табуляции)
//! в пикселях. Список одинаков для экрана и для картинки.

use letter_layout::{
    Float, FontLibrary, LineItem, Margins, MathLayout, Page, PageLine, PageStory, PageWatermark,
    ShapeKind, ShapedRun, TabItem, TabLeader, TextItem, baseline_rise,
};
use std::sync::Arc;

use letter_model::{
    Color, Drawing, HeaderFooterKind, ImageData, ResolvedRunProps, Shape, Underline,
};

use crate::drawing::{DrawingFrame, draw_shapes};
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
    /// Отрезок (линии знака корня в формулах).
    Line {
        /// Начало по горизонтали.
        x1: f32,
        /// Начало по вертикали.
        y1: f32,
        /// Конец по горизонтали.
        x2: f32,
        /// Конец по вертикали.
        y2: f32,
        /// Толщина.
        width: f32,
        /// Цвет.
        color: Rgba,
    },
    /// Залитый выпуклый многоугольник (части диаграмм).
    Polygon {
        /// Вершины по порядку.
        points: Vec<(f32, f32)>,
        /// Цвет.
        color: Rgba,
    },
    /// Рисунок: картинка, растянутая на прямоугольник.
    Picture {
        /// Левый край.
        x: f32,
        /// Верхний край.
        y: f32,
        /// Ширина.
        width: f32,
        /// Высота.
        height: f32,
        /// Картинка; `None` — нарисовать заглушку.
        image: Option<Arc<ImageData>>,
        /// Фигура: рисуется она, а не картинка.
        shape: Option<Arc<Shape>>,
    },
    /// Контур фигуры (SmartArt): многоугольник с заливкой и линией
    /// или ломаная.
    Path {
        /// Точки.
        points: Vec<(f32, f32)>,
        /// Замкнутый: от последней точки линия идёт к первой.
        closed: bool,
        /// Заливка (только у замкнутого контура).
        fill: Option<Rgba>,
        /// Линия: цвет и толщина.
        stroke: Option<(Rgba, f32)>,
    },
}

/// Список того, что нарисовать на странице при масштабе `scale`
/// пикселей на пункт. Сначала идут фоны (заливка ячеек, выделение
/// маркером) и плавающие рисунки за текстом, потом рисунки в строке,
/// глифы и линии (подчёркивания, границы таблиц), последними — плавающие
/// рисунки перед текстом.
pub fn page_scene(
    page: &Page,
    scale: f32,
    library: &FontLibrary,
    cache: &mut GlyphCache,
) -> Vec<DrawItem> {
    let mut backgrounds = Vec::new();
    let mut pictures = Vec::new();
    let mut glyphs = Vec::new();
    let mut lines = Vec::new();
    let stories: Vec<&PageStory> = [&page.header, &page.footer]
        .into_iter()
        .chain(page.notes.iter().map(|n| &n.story))
        .collect();
    let shapes = stories.iter().flat_map(|s| &s.shapes).chain(&page.shapes);
    for shape in shapes {
        let (r, g, b) = shape.color;
        let item = rect(
            scale,
            shape.x,
            shape.y,
            shape.width,
            shape.height,
            [r, g, b, 255],
        );
        match shape.kind {
            ShapeKind::Fill => backgrounds.push(item),
            ShapeKind::Rule => lines.push(item),
        }
    }
    if let Some(mark) = &page.watermark {
        watermark(&mut backgrounds, mark, scale);
    }
    let all_lines = stories.iter().flat_map(|s| &s.lines).chain(&page.lines);
    for line in all_lines {
        for item in &line.line.items {
            match item {
                LineItem::Text(text) => {
                    let Some(props) = line.spans.get(text.span) else {
                        continue;
                    };
                    if text.run.hidden {
                        continue;
                    }
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
                LineItem::Object(object) if object.math.is_some() => {
                    let color = line.spans.get(object.span).map_or(BLACK, text_color);
                    if let Some(math) = &object.math {
                        formula(&mut glyphs, &mut lines, math, line, object.x, scale, color);
                    }
                }
                // Знак привязки плавающего рисунка места не занимает.
                LineItem::Object(object) if object.width == 0.0 && object.height == 0.0 => {}
                LineItem::Object(object) => picture(
                    &mut pictures,
                    (line.x + object.x, line.baseline - object.height),
                    (object.width, object.height),
                    (
                        object.image.as_ref(),
                        object.drawing.as_deref(),
                        object.shape.as_ref(),
                        object.text.as_deref(),
                    ),
                    scale,
                    library,
                    cache,
                ),
            }
        }
        // Номер или маркер пункта списка.
        if let Some(label) = &line.label {
            let props = &label.props;
            let color = text_color(props);
            for item in &label.items {
                let LineItem::Text(text) = item else {
                    continue;
                };
                if text.run.hidden {
                    continue;
                }
                push_glyphs(&mut glyphs, line, text, baseline_rise(props), scale, color);
                let (x, width) = (line.x + text.x, text.run.width());
                decorate(
                    &mut lines, library, cache, line, text, props, x, width, scale, color,
                );
            }
        }
    }
    let mut float = |out: &mut Vec<DrawItem>, f: &Float| {
        let (at, size) = ((f.x, f.y), (f.width, f.height));
        let content = (
            f.image.as_ref(),
            f.drawing.as_deref(),
            f.shape.as_ref(),
            f.text.as_deref(),
        );
        picture(out, at, size, content, scale, library, cache);
    };
    let floats = || stories.iter().flat_map(|s| &s.floats).chain(&page.floats);
    for f in floats().filter(|f| f.behind) {
        float(&mut backgrounds, f);
    }
    backgrounds.extend(pictures);
    backgrounds.extend(glyphs);
    backgrounds.extend(lines);
    for f in floats().filter(|f| !f.behind) {
        float(&mut backgrounds, f);
    }
    backgrounds
}

/// Что нарисовать в рамке рисунка: картинка, рисунок из фигур
/// (SmartArt), фигура и текст её надписей.
type Content<'a> = (
    Option<&'a Arc<ImageData>>,
    Option<&'a Drawing>,
    Option<&'a Arc<Shape>>,
    Option<&'a PageStory>,
);

/// Рисунок в точке `at` размера `size` (пт): картинка, диаграмма,
/// рисунок из фигур (SmartArt) или фигура с текстом надписи.
fn picture(
    out: &mut Vec<DrawItem>,
    (x, y): (f32, f32),
    (width, height): (f32, f32),
    (image, drawing, shape, text): Content<'_>,
    scale: f32,
    library: &FontLibrary,
    cache: &mut GlyphCache,
) {
    if let Some(drawing) = drawing {
        let frame = DrawingFrame {
            x,
            y,
            width,
            height,
        };
        draw_shapes(out, drawing, frame, scale, library, cache);
        return;
    }
    if let Some(chart) = image.and_then(|i| i.chart.as_ref()) {
        out.extend(crate::chart::chart_items(
            chart, x, y, width, height, scale, library, cache,
        ));
        return;
    }
    out.push(DrawItem::Picture {
        x: x * scale,
        y: y * scale,
        width: width * scale,
        height: height * scale,
        image: image.cloned(),
        shape: shape.cloned(),
    });
    if let Some(text) = text {
        text_box(out, text, x, y, scale, library, cache);
    }
}

/// Текст надписей фигуры, левый верхний угол которой — в точке `x`,
/// `y` пт: рисуется сразу после самой фигуры, поверх неё.
fn text_box(
    out: &mut Vec<DrawItem>,
    text: &PageStory,
    x: f32,
    y: f32,
    scale: f32,
    library: &FontLibrary,
    cache: &mut GlyphCache,
) {
    let story = text.clone().shifted(x, y);
    let page = Page {
        width: 0.0,
        height: 0.0,
        margins: Margins {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        },
        lines: story.lines,
        shapes: story.shapes,
        floats: story.floats,
        section: 0,
        kind: HeaderFooterKind::Default,
        header: PageStory::default(),
        footer: PageStory::default(),
        notes: Vec::new(),
        watermark: None,
    };
    out.extend(page_scene(&page, scale, library, cache));
}

/// Глифы подложки вдоль повёрнутой базовой линии.
fn watermark(out: &mut Vec<DrawItem>, mark: &PageWatermark, scale: f32) {
    let (sin, cos) = mark.rotation.to_radians().sin_cos();
    // Поворот глифа против часовой стрелки, целые градусы.
    let rotation = (360.0 - mark.rotation).rem_euclid(360.0).round() as u16 % 360;
    let (r, g, b) = mark.color;
    let color = [r, g, b, mark.alpha];
    let mut pen = 0.0;
    for run in &mark.runs {
        let size_64 = (run.size * scale * 64.0).round().max(1.0) as u32;
        for glyph in &run.glyphs {
            // Вдоль строки и вверх от неё, потом поворот.
            let (along, up) = (pen + glyph.x_offset, glyph.y_offset);
            let x = (mark.x + along * cos + up * sin) * scale;
            let y = (mark.y + along * sin - up * cos) * scale;
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
                    width_percent: (run.width_scale * 100.0).round().clamp(1.0, 600.0) as u16,
                    rotation,
                },
                x: whole as i32,
                y: y.round() as i32,
                color,
            });
            pen += glyph.x_advance;
        }
    }
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

/// Формула: её глифы и линии. `x` — левый край в строке, пт.
#[allow(clippy::too_many_arguments)]
fn formula(
    glyphs: &mut Vec<DrawItem>,
    lines: &mut Vec<DrawItem>,
    math: &MathLayout,
    line: &PageLine,
    x: f32,
    scale: f32,
    color: Rgba,
) {
    let left = line.x + x;
    for piece in &math.glyphs {
        push_run(
            glyphs,
            &piece.run,
            left + piece.x,
            line.baseline - piece.rise,
            scale,
            color,
        );
    }
    for stroke in &math.strokes {
        let (x1, y1) = (left + stroke.x1, line.baseline - stroke.y1);
        let (x2, y2) = (left + stroke.x2, line.baseline - stroke.y2);
        // Горизонтальная черта — прямоугольником, чтобы была чёткой.
        if y1 == y2 {
            let (from, to) = (x1.min(x2), x1.max(x2));
            let top = y1 - stroke.width / 2.0;
            lines.push(rect(scale, from, top, to - from, stroke.width, color));
        } else {
            lines.push(DrawItem::Line {
                x1: x1 * scale,
                y1: y1 * scale,
                x2: x2 * scale,
                y2: y2 * scale,
                width: stroke.width * scale,
                color,
            });
        }
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
    push_run(
        out,
        &text.run,
        line.x + text.x,
        line.baseline - rise,
        scale,
        color,
    );
}

/// Глифы куска с левым краем `left` на базовой линии `baseline`
/// (пт от угла листа).
fn push_run(
    out: &mut Vec<DrawItem>,
    run: &ShapedRun,
    left: f32,
    baseline: f32,
    scale: f32,
    color: Rgba,
) {
    if run.hidden {
        return;
    }
    let size_64 = (run.size * scale * 64.0).round().max(1.0) as u32;
    let mut pen = left;
    for glyph in &run.glyphs {
        let x = (pen + glyph.x_offset) * scale;
        let y = (baseline - glyph.y_offset) * scale;
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
                width_percent: (run.width_scale * 100.0).round().clamp(1.0, 600.0) as u16,
                rotation: 0,
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
    let underline_color = match props.underline_color {
        Some(Color::Rgb(r, g, b)) => [r, g, b, 255],
        _ => color,
    };
    let mut bar = |offset: f32, thickness: f32, color: Rgba| {
        let y = line.baseline - offset - thickness / 2.0;
        out.push(rect(scale, x, y, width, thickness, color));
    };
    let mut underline = |offset: f32, thickness: f32| bar(offset, thickness, underline_color);
    match props.underline {
        Underline::None => {}
        Underline::Double | Underline::WavyDouble => {
            let t = thin(underline_thickness);
            underline(underline_at * size, t);
            underline(underline_at * size - 2.0 * t, t);
        }
        Underline::Thick
        | Underline::DottedHeavy
        | Underline::DashedHeavy
        | Underline::DashLongHeavy
        | Underline::DashDotHeavy
        | Underline::DashDotDotHeavy
        | Underline::WavyHeavy => underline(underline_at * size, 2.0 * thin(underline_thickness)),
        // Точки, штрихи и волны пока рисуются сплошной линией.
        _ => underline(underline_at * size, thin(underline_thickness)),
    }
    if props.strike {
        bar(strike_at * size, thin(strike_thickness), color);
    }
    if props.double_strike {
        let t = thin(strike_thickness);
        bar(strike_at * size + t, t, color);
        bar(strike_at * size - t, t, color);
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
