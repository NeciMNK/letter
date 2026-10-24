//! Вёрстка формул ([`Math`]): дроби, степени, корни, скобки, суммы.
//!
//! Формула собирается из «коробок», как в TeX: у каждой части есть
//! ширина, высота над базовой линией и глубина под ней. Части ставятся
//! рядом или друг над другом; в итоге получается одна коробка —
//! [`MathLayout`] — со списком глифов и линий (черта дроби, знак корня).
//! В строке абзаца формула стоит как рисунок.
//!
//! Размеры — в долях кегля и подобраны на глаз так, чтобы формулы были
//! похожи на формулы Word со шрифтом Cambria Math.

use letter_model::{FractionKind, Math, MathAlign, MathNode};

use crate::fonts::FontLibrary;
use crate::shaping::{ShapedRun, Shaper, TextStyle};

/// Шрифт формул Word по умолчанию.
pub const MATH_FONT: &str = "Cambria Math";

/// Свёрстанная формула или её часть. Координаты — пункты от левого
/// края и от базовой линии коробки, вверх — плюс.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MathLayout {
    /// Ширина, пт.
    pub width: f32,
    /// Высота над базовой линией, пт.
    pub ascent: f32,
    /// Глубина под базовой линией, пт.
    pub descent: f32,
    /// Куски текста.
    pub glyphs: Vec<MathGlyphs>,
    /// Линии: черты дробей, знаки корня.
    pub strokes: Vec<MathStroke>,
}

/// Кусок текста формулы на своём месте.
#[derive(Debug, Clone, PartialEq)]
pub struct MathGlyphs {
    /// Левый край, пт от левого края формулы.
    pub x: f32,
    /// Базовая линия куска над базовой линией формулы, пт.
    pub rise: f32,
    /// Глифы.
    pub run: ShapedRun,
}

/// Отрезок толщиной `width` от точки 1 до точки 2 (пт, вверх — плюс).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MathStroke {
    /// Начало по горизонтали.
    pub x1: f32,
    /// Начало по вертикали.
    pub y1: f32,
    /// Конец по горизонтали.
    pub x2: f32,
    /// Конец по вертикали.
    pub y2: f32,
    /// Толщина.
    pub width: f32,
}

impl MathLayout {
    /// Ставит `other` внутрь этой коробки: левый край на `dx`, базовую
    /// линию на `rise` выше своей. Ширину не меняет.
    fn put(&mut self, other: MathLayout, dx: f32, rise: f32) {
        self.ascent = self.ascent.max(other.ascent + rise);
        self.descent = self.descent.max(other.descent - rise);
        self.glyphs.extend(other.glyphs.into_iter().map(|mut g| {
            g.x += dx;
            g.rise += rise;
            g
        }));
        self.strokes.extend(other.strokes.into_iter().map(|mut s| {
            s.x1 += dx;
            s.x2 += dx;
            s.y1 += rise;
            s.y2 += rise;
            s
        }));
    }

    /// Добавляет `other` справа.
    fn push(&mut self, other: MathLayout) {
        let dx = self.width;
        self.width += other.width;
        self.put(other, dx, 0.0);
    }

    /// Пустое место шириной `width`.
    fn space(width: f32) -> MathLayout {
        MathLayout {
            width,
            ..MathLayout::default()
        }
    }

    /// Горизонтальная линия на высоте `y` от `x1` до `x2`.
    fn rule(&mut self, x1: f32, x2: f32, y: f32, width: f32) {
        self.strokes.push(MathStroke {
            x1,
            y1: y,
            x2,
            y2: y,
            width,
        });
        self.ascent = self.ascent.max(y + width / 2.0);
        self.descent = self.descent.max(-y + width / 2.0);
    }
}

/// Верстает формулу кеглем `size` пт.
pub fn layout_math(
    math: &Math,
    size: f32,
    shaper: &mut Shaper,
    library: &FontLibrary,
) -> MathLayout {
    let mut ctx = Ctx { shaper, library };
    let style = Style {
        size,
        min: size * 0.5,
        display: math.display,
        script: false,
        upright: false,
    };
    let lines: Vec<MathLayout> = math
        .lines
        .iter()
        .map(|line| ctx.list(line, style))
        .collect();
    let width = lines.iter().map(|l| l.width).fold(0.0, f32::max);
    let mut out = MathLayout::space(width);
    let mut rise = 0.0;
    for (i, line) in lines.into_iter().enumerate() {
        if i > 0 {
            rise = -(out.descent + size * 0.3 + line.ascent);
        }
        let dx = match math.align {
            MathAlign::Left => 0.0,
            MathAlign::Center => (width - line.width) / 2.0,
            MathAlign::Right => width - line.width,
        };
        out.put(line, dx, rise);
    }
    out
}

/// Как верстать часть формулы.
#[derive(Debug, Clone, Copy)]
struct Style {
    /// Кегль, пт.
    size: f32,
    /// Самый мелкий кегль для индексов, пт.
    min: f32,
    /// Отдельная формула: крупные суммы и интегралы, пределы над и под.
    display: bool,
    /// Индекс или предел: без пробелов вокруг знаков действий.
    script: bool,
    /// Все буквы прямые (имя функции).
    upright: bool,
}

impl Style {
    /// Стиль индекса: мельче, без пробелов.
    fn script(self) -> Style {
        Style {
            size: (self.size * 0.7).max(self.min),
            display: false,
            script: true,
            ..self
        }
    }

    /// Средняя линия формулы над базовой (на ней черта дроби и знак
    /// минуса), пт.
    fn axis(self) -> f32 {
        self.size * 0.25
    }

    /// Толщина черты дроби, пт.
    fn rule(self) -> f32 {
        (self.size * 0.05).max(0.4)
    }
}

/// Знаки действий: пробел вокруг них в долях кегля.
fn operator_space(c: char) -> Option<f32> {
    match c {
        '+' | '−' | '±' | '∓' | '×' | '÷' | '·' | '∙' | '∘' | '∪' | '∩' | '∧' | '∨' | '⊕' | '⊗' => {
            Some(0.22)
        }
        '=' | '≠' | '<' | '>' | '≤' | '≥' | '≈' | '≡' | '∼' | '≅' | '∝' | '→' | '←' | '↔' | '⇒'
        | '⇐' | '⇔' | '∈' | '∉' | '⊂' | '⊃' | '⊆' | '⊇' | '∣' | '≪' | '≫' => {
            Some(0.28)
        }
        ',' | ';' => Some(0.0),
        _ => None,
    }
}

/// Буквы формулы пишутся курсивом, кроме прописных греческих.
fn is_italic(c: char) -> bool {
    c.is_alphabetic() && !('\u{391}'..='\u{3a9}').contains(&c) && c != '∞'
}

struct Ctx<'a> {
    shaper: &'a mut Shaper,
    library: &'a FontLibrary,
}

impl Ctx<'_> {
    /// Узлы подряд.
    fn list(&mut self, nodes: &[MathNode], style: Style) -> MathLayout {
        let mut out = MathLayout::default();
        for (i, node) in nodes.iter().enumerate() {
            let node = self.node(node, style, i == 0);
            out.push(node);
        }
        out
    }

    fn node(&mut self, node: &MathNode, style: Style, first: bool) -> MathLayout {
        match node {
            MathNode::Text { text, plain } => {
                self.text(text, *plain || style.upright, style, first)
            }
            MathNode::Fraction { kind, num, den } => self.fraction(*kind, num, den, style),
            MathNode::Scripts {
                base,
                sub,
                sup,
                pre,
            } => {
                let base = self.list(base, style);
                self.scripts(base, sub.as_deref(), sup.as_deref(), *pre, style)
            }
            MathNode::Radical { degree, base } => self.radical(degree.as_deref(), base, style),
            MathNode::Delimiter {
                open,
                close,
                separator,
                items,
            } => self.delimiter(open, close, separator, items, style),
            MathNode::Nary {
                op,
                limits_over,
                sub,
                sup,
                base,
            } => self.nary(
                op,
                *limits_over,
                sub.as_deref(),
                sup.as_deref(),
                base,
                style,
            ),
            MathNode::Function { name, base } => {
                let mut out = self.list(
                    name,
                    Style {
                        upright: true,
                        ..style
                    },
                );
                out.push(MathLayout::space(style.size * 0.17));
                out.push(self.list(base, style));
                out
            }
            MathNode::Accent { mark, base } => self.accent(mark, base, style),
            MathNode::Bar { top, base } => {
                let mut out = self.list(base, style);
                let gap = style.size * 0.08;
                let y = if *top {
                    out.ascent + gap
                } else {
                    -out.descent - gap
                };
                let width = out.width;
                out.rule(0.0, width, y, style.rule());
                out
            }
            MathNode::Limit { upper, base, limit } => {
                let base = self.list(base, style);
                let limit = self.list(limit, style.script());
                self.over_under(
                    base,
                    (!*upper).then_some(limit.clone()),
                    upper.then_some(limit),
                    style,
                )
            }
            MathNode::GroupChar { mark, top, base } => self.group_char(mark, *top, base, style),
            MathNode::Matrix(rows) => self.grid(rows, true, style),
            MathNode::EqArray(rows) => {
                let rows: Vec<Vec<_>> = rows.iter().map(|r| vec![r.clone()]).collect();
                self.grid(&rows, false, style)
            }
            MathNode::Group(list) => self.list(list, style),
        }
    }

    /// Кусок текста одним шрифтом.
    fn glyphs(&mut self, text: &str, size: f32, italic: bool, width_scale: f32) -> MathLayout {
        let mut text_style = TextStyle::new(MATH_FONT, size);
        text_style.italic = italic;
        text_style.width_scale = width_scale;
        let mut out = MathLayout::default();
        // Настоящие границы глифов: у знака «ˆ» низ выше базовой линии,
        // и глубина тогда отрицательная.
        let (mut low, mut high) = (f32::MAX, f32::MIN);
        for run in self.shaper.shape(self.library, text, &text_style) {
            for glyph in &run.glyphs {
                if let Some((bottom, top)) =
                    self.shaper
                        .glyph_bounds(self.library, run.font.id, glyph.glyph_id)
                {
                    low = low.min(bottom * size);
                    high = high.max(top * size);
                }
            }
            let width = run.width();
            out.glyphs.push(MathGlyphs {
                x: out.width,
                rise: 0.0,
                run,
            });
            out.width += width;
        }
        if low <= high {
            out.ascent = high;
            out.descent = -low;
        }
        out
    }

    /// Текст: буквы курсивом, знаки действий с пробелами.
    fn text(&mut self, text: &str, plain: bool, style: Style, first: bool) -> MathLayout {
        let mut out = MathLayout::default();
        let mut piece = String::new();
        let mut piece_italic = false;
        let flush = |ctx: &mut Ctx, out: &mut MathLayout, piece: &mut String, italic: bool| {
            if !piece.is_empty() {
                let glyphs = ctx.glyphs(piece, style.size, italic, 1.0);
                out.push(glyphs);
                piece.clear();
            }
        };
        for (i, c) in text.chars().enumerate() {
            // Word показывает дефис в формуле знаком минуса.
            let c = if c == '-' { '−' } else { c };
            if let Some(space) = operator_space(c).filter(|_| !plain) {
                flush(self, &mut out, &mut piece, piece_italic);
                // Знак в начале — унарный (−x): без пробела.
                let unary = first && i == 0 && space < 0.25;
                let space = if style.script || unary {
                    0.0
                } else {
                    space * style.size
                };
                out.push(MathLayout::space(if c == ',' || c == ';' {
                    0.0
                } else {
                    space
                }));
                let glyph = self.glyphs(&c.to_string(), style.size, false, 1.0);
                out.push(glyph);
                let after = if c == ',' || c == ';' {
                    style.size * 0.17
                } else {
                    space
                };
                out.push(MathLayout::space(after));
                continue;
            }
            let italic = !plain && is_italic(c);
            if italic != piece_italic {
                flush(self, &mut out, &mut piece, piece_italic);
                piece_italic = italic;
            }
            piece.push(c);
        }
        flush(self, &mut out, &mut piece, piece_italic);
        out
    }

    /// Знак, растянутый по высоте до `height` пт и стоящий посередине
    /// на средней линии (скобки, разделители, большие операторы).
    fn tall_glyph(&mut self, c: &str, height: f32, style: Style) -> MathLayout {
        if c.is_empty() {
            return MathLayout::default();
        }
        let natural = self.glyphs(c, style.size, false, 1.0);
        let natural_height = natural.ascent + natural.descent;
        if natural_height <= 0.0 || height <= natural_height {
            return natural;
        }
        let scale = height / natural_height;
        // Высокие скобки не делаем такими же широкими.
        let width_scale = scale.powf(-0.6).clamp(0.3, 1.0);
        let mut out = self.glyphs(c, style.size * scale, false, width_scale);
        let middle = (out.ascent - out.descent) / 2.0;
        let rise = style.axis() - middle;
        let mut placed = MathLayout::space(out.width);
        out.width = 0.0;
        placed.put(out, 0.0, rise);
        placed
    }

    fn fraction(
        &mut self,
        kind: FractionKind,
        num: &[MathNode],
        den: &[MathNode],
        style: Style,
    ) -> MathLayout {
        let inner = if style.display {
            Style {
                display: false,
                ..style
            }
        } else {
            style.script()
        };
        let size = style.size;
        match kind {
            FractionKind::Linear => {
                let mut out = self.list(num, style);
                out.push(self.glyphs("/", size, false, 1.0));
                out.push(self.list(den, style));
                out
            }
            FractionKind::Skewed => {
                let num = self.list(num, style.script());
                let den = self.list(den, style.script());
                let mut out = MathLayout::space(num.width);
                out.put(num, 0.0, size * 0.35);
                out.push(self.glyphs("/", size, false, 1.0));
                let dx = out.width;
                out.width += den.width;
                out.put(den, dx, -size * 0.1);
                out
            }
            FractionKind::Bar | FractionKind::NoBar => {
                let num = self.list(num, inner);
                let den = self.list(den, inner);
                let thick = style.rule();
                let gap = size * if style.display { 0.15 } else { 0.1 };
                let pad = size * 0.1;
                let width = num.width.max(den.width) + 2.0 * pad;
                let axis = style.axis();
                let num_rise = axis + thick / 2.0 + gap + num.descent;
                let den_rise = axis - thick / 2.0 - gap - den.ascent;
                let mut out = MathLayout::space(width);
                let (nw, dw) = (num.width, den.width);
                out.put(num, (width - nw) / 2.0, num_rise);
                out.put(den, (width - dw) / 2.0, den_rise);
                if kind == FractionKind::Bar {
                    out.rule(pad / 2.0, width - pad / 2.0, axis, thick);
                }
                out
            }
        }
    }

    fn scripts(
        &mut self,
        base: MathLayout,
        sub: Option<&[MathNode]>,
        sup: Option<&[MathNode]>,
        pre: bool,
        style: Style,
    ) -> MathLayout {
        let size = style.size;
        let sub = sub.map(|s| self.list(s, style.script()));
        let sup = sup.map(|s| self.list(s, style.script()));
        let mut sup_rise = (size * 0.4).max(base.ascent - size * 0.35);
        let mut sub_rise = -(size * 0.18).max(base.descent - size * 0.1);
        if let Some(sup) = &sup {
            sup_rise = sup_rise.max(sup.descent + size * 0.2);
        }
        if let (Some(sub), Some(sup)) = (&sub, &sup) {
            let gap = (sup_rise - sup.descent) - (sub_rise + sub.ascent);
            if gap < size * 0.15 {
                sub_rise -= size * 0.15 - gap;
            }
        }
        let scripts_width = sub
            .as_ref()
            .map_or(0.0, |s| s.width)
            .max(sup.as_ref().map_or(0.0, |s| s.width));
        let kern = size * 0.04;
        let mut out = MathLayout::space(base.width + kern + scripts_width);
        let (base_x, scripts_x) = if pre {
            (scripts_width + kern, 0.0)
        } else {
            (0.0, base.width + kern)
        };
        out.put(base, base_x, 0.0);
        for (script, rise) in [(sub, sub_rise), (sup, sup_rise)] {
            if let Some(script) = script {
                // Индексы слева прижаты к основанию.
                let x = if pre {
                    scripts_width - script.width
                } else {
                    scripts_x
                };
                out.put(script, x, rise);
            }
        }
        out
    }

    fn radical(
        &mut self,
        degree: Option<&[MathNode]>,
        base: &[MathNode],
        style: Style,
    ) -> MathLayout {
        let size = style.size;
        let inner = self.list(base, style);
        let thick = style.rule();
        let gap = size * if style.display { 0.15 } else { 0.1 };
        let top = inner.ascent.max(size * 0.5) + gap + thick;
        let bottom = -inner.descent.max(size * 0.05) - size * 0.05;
        let height = top - bottom;
        let sign = size * 0.45 + height * 0.1;
        let degree = degree.map(|d| {
            self.list(
                d,
                Style {
                    size: (style.size * 0.5).max(style.min),
                    ..style.script()
                },
            )
        });
        let shift = degree
            .as_ref()
            .map_or(0.0, |d| (d.width - sign * 0.45).max(0.0));
        let mut out = MathLayout::space(shift + sign + inner.width + size * 0.1);
        let y = top - thick / 2.0;
        let p0 = (shift, bottom + height * 0.42);
        let p1 = (shift + sign * 0.22, bottom + height * 0.5);
        let p2 = (shift + sign * 0.55, bottom);
        let p3 = (shift + sign, y);
        for ((x1, y1), (x2, y2), width) in [(p0, p1, thick), (p1, p2, thick * 1.8), (p2, p3, thick)]
        {
            out.strokes.push(MathStroke {
                x1,
                y1,
                x2,
                y2,
                width,
            });
        }
        let end = out.width;
        out.rule(shift + sign, end, y, thick);
        out.descent = out.descent.max(-bottom);
        let inner_x = shift + sign + size * 0.05;
        out.put(inner, inner_x, 0.0);
        if let Some(degree) = degree {
            let rise = bottom + height * 0.55 + degree.descent;
            let x = shift + sign * 0.45 - degree.width;
            out.put(degree, x.max(0.0), rise);
        }
        out.ascent += size * 0.05;
        out
    }

    fn delimiter(
        &mut self,
        open: &str,
        close: &str,
        separator: &str,
        items: &[Vec<MathNode>],
        style: Style,
    ) -> MathLayout {
        let mut inner = MathLayout::default();
        let parts: Vec<MathLayout> = items.iter().map(|i| self.list(i, style)).collect();
        for part in &parts {
            inner.ascent = inner.ascent.max(part.ascent);
            inner.descent = inner.descent.max(part.descent);
        }
        let axis = style.axis();
        let half = (inner.ascent - axis).max(inner.descent + axis);
        let height = 2.0 * half + style.size * 0.1;
        let mut out = self.tall_glyph(open, height, style);
        for (i, part) in parts.into_iter().enumerate() {
            if i > 0 {
                let separator = self.tall_glyph(separator, height, style);
                out.push(separator);
            }
            out.push(part);
        }
        let close = self.tall_glyph(close, height, style);
        out.push(close);
        out
    }

    fn nary(
        &mut self,
        op: &str,
        limits_over: Option<bool>,
        sub: Option<&[MathNode]>,
        sup: Option<&[MathNode]>,
        base: &[MathNode],
        style: Style,
    ) -> MathLayout {
        let size = style.size;
        let integral = op.chars().any(|c| ('∫'..='∳').contains(&c) || c == '⨌');
        let scale = match (style.display, integral) {
            (true, true) => 2.2,
            (true, false) => 1.8,
            (false, _) => 1.2,
        };
        // Большой знак — крупнее текста, посередине на средней линии.
        let glyph = self.glyphs(op, size * scale, false, 1.0);
        let middle = (glyph.ascent - glyph.descent) / 2.0;
        let mut op_box = MathLayout::space(glyph.width);
        op_box.put(
            MathLayout {
                width: 0.0,
                ..glyph
            },
            0.0,
            style.axis() - middle,
        );
        let over = limits_over.unwrap_or(style.display && !integral);
        let mut out = if over {
            let sub = sub.map(|s| self.list(s, style.script()));
            let sup = sup.map(|s| self.list(s, style.script()));
            self.over_under(op_box, sub, sup, style)
        } else {
            self.scripts(op_box, sub, sup, false, style)
        };
        out.push(MathLayout::space(size * 0.1));
        out.push(self.list(base, style));
        out
    }

    /// Основание с частями над ним и под ним (пределы), по центру.
    fn over_under(
        &mut self,
        base: MathLayout,
        under: Option<MathLayout>,
        over: Option<MathLayout>,
        style: Style,
    ) -> MathLayout {
        let gap = style.size * 0.1;
        let width = base
            .width
            .max(under.as_ref().map_or(0.0, |u| u.width))
            .max(over.as_ref().map_or(0.0, |o| o.width));
        let mut out = MathLayout::space(width);
        let (base_ascent, base_descent) = (base.ascent, base.descent);
        let bw = base.width;
        out.put(base, (width - bw) / 2.0, 0.0);
        if let Some(under) = under {
            let rise = -base_descent - gap - under.ascent;
            let uw = under.width;
            out.put(under, (width - uw) / 2.0, rise);
        }
        if let Some(over) = over {
            let rise = base_ascent + gap + over.descent;
            let ow = over.width;
            out.put(over, (width - ow) / 2.0, rise);
        }
        out
    }

    fn accent(&mut self, mark: &str, base: &[MathNode], style: Style) -> MathLayout {
        let mut out = self.list(base, style);
        let gap = style.size * 0.05;
        // Черта над буквой — линией во всю ширину.
        if matches!(mark, "\u{304}" | "\u{305}" | "\u{af}") {
            let y = out.ascent + gap + style.rule();
            let width = out.width;
            out.rule(0.0, width, y, style.rule());
            return out;
        }
        let shown = match mark {
            "\u{302}" => "ˆ",
            "\u{303}" => "˜",
            "\u{307}" => "˙",
            "\u{308}" => "¨",
            "\u{30c}" => "ˇ",
            "\u{301}" => "´",
            "\u{300}" => "`",
            "\u{306}" => "˘",
            "\u{20d7}" | "\u{20d1}" => "→",
            "\u{20d6}" => "←",
            other => other,
        };
        let glyph = self.glyphs(shown, style.size, false, 1.0);
        // Низ знака (над базовой линией у знаков вроде «ˆ») — чуть выше
        // буквы.
        let rise = out.ascent + gap + glyph.descent;
        let (gw, width) = (glyph.width, out.width);
        out.put(glyph, (width - gw) / 2.0, rise);
        out
    }

    fn group_char(&mut self, mark: &str, top: bool, base: &[MathNode], style: Style) -> MathLayout {
        let base = self.list(base, style);
        let natural = self.glyphs(mark, style.size, false, 1.0);
        let scale = if natural.width > 0.0 {
            (base.width / natural.width).clamp(0.5, 6.0)
        } else {
            1.0
        };
        let mark = self.glyphs(mark, style.size, false, scale);
        if top {
            self.over_under(base, None, Some(mark), style)
        } else {
            self.over_under(base, Some(mark), None, style)
        }
    }

    /// Таблица ячеек по центру средней линии: матрица (`centered` —
    /// ячейки по центру столбца) или система уравнений (по левому краю).
    fn grid(&mut self, rows: &[Vec<Vec<MathNode>>], centered: bool, style: Style) -> MathLayout {
        let size = style.size;
        let cells: Vec<Vec<MathLayout>> = rows
            .iter()
            .map(|row| row.iter().map(|c| self.list(c, style)).collect())
            .collect();
        let columns = cells.iter().map(Vec::len).max().unwrap_or(0);
        let widths: Vec<f32> = (0..columns)
            .map(|c| {
                cells
                    .iter()
                    .filter_map(|row| row.get(c))
                    .map(|cell| cell.width)
                    .fold(0.0, f32::max)
            })
            .collect();
        let column_gap = size * 0.6;
        let row_gap = size * 0.25;
        let width = widths.iter().sum::<f32>() + column_gap * columns.saturating_sub(1) as f32;
        let heights: Vec<(f32, f32)> = cells
            .iter()
            .map(|row| {
                let ascent = row.iter().map(|c| c.ascent).fold(size * 0.7, f32::max);
                let descent = row.iter().map(|c| c.descent).fold(size * 0.2, f32::max);
                (ascent, descent)
            })
            .collect();
        let total: f32 = heights.iter().map(|(a, d)| a + d).sum::<f32>()
            + row_gap * heights.len().saturating_sub(1) as f32;
        let mut out = MathLayout::space(width);
        let mut y = style.axis() + total / 2.0;
        for (row, (ascent, descent)) in cells.into_iter().zip(heights) {
            let rise = y - ascent;
            let mut x = 0.0;
            for (cell, column_width) in row.into_iter().zip(&widths) {
                let dx = if centered {
                    (column_width - cell.width) / 2.0
                } else {
                    0.0
                };
                out.put(cell, x + dx, rise);
                x += column_width + column_gap;
            }
            y = rise - descent - row_gap;
        }
        out.ascent = out.ascent.max(style.axis() + total / 2.0);
        out.descent = out.descent.max(total / 2.0 - style.axis());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::MathList;

    fn t(s: &str) -> MathList {
        vec![MathNode::text(s)]
    }

    fn lay(nodes: MathList, display: bool) -> MathLayout {
        let library = FontLibrary::embedded_only();
        let mut shaper = Shaper::new();
        let mut math = Math::inline(nodes);
        math.display = display;
        layout_math(&math, 12.0, &mut shaper, &library)
    }

    #[test]
    fn text_has_glyphs_and_size() {
        let out = lay(t("x+1"), false);
        assert!(out.width > 12.0, "ширина {}", out.width);
        assert!(out.ascent > 5.0 && out.ascent < 12.0);
        // «x» курсивом, «+» и «1» — прямо: три куска.
        assert_eq!(out.glyphs.len(), 3);
    }

    #[test]
    fn fraction_is_taller_than_text() {
        let text = lay(t("a"), true);
        let frac = lay(
            vec![MathNode::Fraction {
                kind: FractionKind::Bar,
                num: t("a"),
                den: t("b"),
            }],
            true,
        );
        assert!(frac.ascent > text.ascent * 1.5);
        assert!(frac.descent > 3.0);
        // Черта дроби горизонтальна и на средней линии.
        let bar = frac.strokes[0];
        assert_eq!(bar.y1, bar.y2);
        assert!((bar.y1 - 3.0).abs() < 0.01);
        // Числитель над чертой, знаменатель под ней.
        let rises: Vec<f32> = frac.glyphs.iter().map(|g| g.rise).collect();
        assert!(rises[0] > bar.y1 && rises[1] < bar.y1);
    }

    #[test]
    fn superscript_is_smaller_and_raised() {
        let out = lay(
            vec![MathNode::Scripts {
                base: t("x"),
                sub: None,
                sup: Some(t("2")),
                pre: false,
            }],
            false,
        );
        let [base, sup] = out.glyphs.as_slice() else {
            panic!("ждали два куска: {:?}", out.glyphs.len());
        };
        assert!(sup.rise > 3.0);
        assert!(sup.run.size < base.run.size);
        assert!(sup.x >= base.run.width());
    }

    #[test]
    fn radical_draws_sign_and_bar() {
        let out = lay(
            vec![MathNode::Radical {
                degree: None,
                base: t("x"),
            }],
            false,
        );
        assert_eq!(out.strokes.len(), 4);
        // Подкоренное выражение правее знака.
        assert!(out.glyphs[0].x > 4.0);
    }

    #[test]
    fn display_sum_puts_limits_over_and_under() {
        let sum = |display| {
            lay(
                vec![MathNode::Nary {
                    op: "∑".into(),
                    limits_over: None,
                    sub: Some(t("i")),
                    sup: Some(t("n")),
                    base: t("i"),
                }],
                display,
            )
        };
        let display = sum(true);
        let inline = sum(false);
        assert!(display.ascent > inline.ascent);
        // В отдельной формуле пределы над и под знаком: «n» выше знака.
        let n = display
            .glyphs
            .iter()
            .map(|g| g.rise)
            .fold(f32::MIN, f32::max);
        assert!(n > 8.0, "подъём предела {n}");
    }

    #[test]
    fn tall_parentheses_around_fraction() {
        let out = lay(
            vec![MathNode::Delimiter {
                open: "(".into(),
                close: ")".into(),
                separator: "|".into(),
                items: vec![vec![MathNode::Fraction {
                    kind: FractionKind::Bar,
                    num: t("a"),
                    den: t("b"),
                }]],
            }],
            true,
        );
        let paren = &out.glyphs[0];
        assert!(paren.run.size > 12.0, "скобка {}", paren.run.size);
    }

    #[test]
    fn several_display_lines_stack_down() {
        let library = FontLibrary::embedded_only();
        let mut shaper = Shaper::new();
        let math = Math {
            display: true,
            lines: vec![t("a"), t("b")],
            ..Math::default()
        };
        let out = layout_math(&math, 12.0, &mut shaper, &library);
        assert!(out.glyphs[1].rise < -10.0);
        assert!(out.descent > 10.0);
    }
}
