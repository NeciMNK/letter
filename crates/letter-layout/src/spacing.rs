//! Межстрочный и межабзацный интервалы по правилам Word.
//!
//! - Высота строки при одинарном интервале — высота самого крупного
//!   шрифта строки по размерам Windows плюс внешний интерлиньяж
//!   ([`Line::natural_height`]).
//! - «Множитель» (1; 1,15; 1,5; 2 …) умножает эту высоту, «точно»
//!   задаёт высоту, «минимум» не даёт строке стать ниже заданного.
//! - При «множителе» лишнее место (или нехватка места) приходится на
//!   низ строки: буквы стоят там же, где при одинарном интервале. Так
//!   делает Word (проверено сверкой с PDF из Word 2024). При «точно» и
//!   «минимум» — на верх строки: низ букв у низа строки.
//! - Между абзацами Word по умолчанию берёт больший из интервалов
//!   «после» предыдущего и «перед» следующим (как в HTML). Складывает
//!   их, только если в документе включена настройка совместимости
//!   `w:doNotUseHTMLParagraphAutoSpacing`.
//! - «Не добавлять интервал между абзацами одного стиля»: абзац с этим
//!   флагом не получает свой интервал перед, если предыдущий абзац того
//!   же стиля, и свой интервал после, если следующий того же стиля.
//!
//! Все размеры — в пунктах, ось Y направлена вниз.

use crate::lines::Line;

/// Правило межстрочного интервала.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineRule {
    /// Множитель одинарного интервала: 1 — одинарный, 1,15, 1,5, 2 —
    /// двойной. В файле `w:line="276" w:lineRule="auto"` — это 276/240.
    Multiple(f32),
    /// Высота строки ровно столько пунктов.
    Exact(f32),
    /// Высота строки не меньше стольких пунктов.
    AtLeast(f32),
}

impl Default for LineRule {
    fn default() -> LineRule {
        LineRule::Multiple(1.0)
    }
}

impl LineRule {
    /// Высота строки по этому правилу, пт.
    pub fn height(self, line: &Line) -> f32 {
        let natural = line.natural_height();
        match self {
            LineRule::Multiple(m) if m.is_finite() && m > 0.0 => natural * m,
            LineRule::Multiple(_) => natural,
            LineRule::Exact(h) if h.is_finite() && h >= 0.0 => h,
            LineRule::Exact(_) => natural,
            LineRule::AtLeast(h) if h.is_finite() => natural.max(h),
            LineRule::AtLeast(_) => natural,
        }
    }

    /// Расстояние от верха строки высотой `height` (из [`Self::height`])
    /// до базовой линии, пт.
    pub fn baseline_offset(self, line: &Line, height: f32) -> f32 {
        match self {
            LineRule::Multiple(_) => line.natural_height() - line.descent,
            LineRule::Exact(_) | LineRule::AtLeast(_) => height - line.descent,
        }
    }
}

/// Интервалы абзаца.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ParagraphSpacing {
    /// Интервал перед абзацем, пт.
    pub before: f32,
    /// Интервал после абзаца, пт.
    pub after: f32,
    /// Межстрочный интервал.
    pub line: LineRule,
    /// «Не добавлять интервал между абзацами одного стиля».
    pub contextual: bool,
}

/// Абзац для расстановки по вертикали: строки и интервалы.
#[derive(Debug, Clone, Copy)]
pub struct ParagraphBlock<'a> {
    /// Строки абзаца (из [`crate::break_lines`]).
    pub lines: &'a [Line],
    /// Интервалы.
    pub spacing: ParagraphSpacing,
    /// Стиль абзаца (для правила «одного стиля»). `None` — стиль по
    /// умолчанию; два абзаца без стиля — одного стиля.
    pub style: Option<&'a str>,
}

/// Строка на своём месте по вертикали.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineBox {
    /// Верхний край строки, пт.
    pub top: f32,
    /// Высота строки, пт.
    pub height: f32,
    /// Базовая линия (на ней стоят буквы), пт.
    pub baseline: f32,
}

/// Абзац на своём месте по вертикали.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedParagraph {
    /// Интервал перед абзацем с учётом правила «одного стиля», пт.
    pub space_before: f32,
    /// Верх первой строки, пт.
    pub top: f32,
    /// Строки сверху вниз.
    pub lines: Vec<LineBox>,
    /// Низ последней строки, пт.
    pub bottom: f32,
    /// Интервал после абзаца с учётом правила «одного стиля», пт.
    pub space_after: f32,
}

/// Расставляет строки абзаца по вертикали, начиная с `top`.
pub fn place_lines(lines: &[Line], rule: LineRule, top: f32) -> Vec<LineBox> {
    let mut y = top;
    lines
        .iter()
        .map(|line| {
            let height = rule.height(line);
            let placed = LineBox {
                top: y,
                height,
                baseline: y + rule.baseline_offset(line, height),
            };
            y += height;
            placed
        })
        .collect()
}

/// Интервалы перед и после абзаца `blocks[index]` с учётом правила
/// «не добавлять интервал между абзацами одного стиля».
pub fn effective_spacing(blocks: &[ParagraphBlock<'_>], index: usize) -> (f32, f32) {
    let Some(block) = blocks.get(index) else {
        return (0.0, 0.0);
    };
    let same_as = |other: Option<&ParagraphBlock<'_>>| {
        block.spacing.contextual && other.is_some_and(|o| o.style == block.style)
    };
    let previous = index.checked_sub(1).and_then(|p| blocks.get(p));
    let before = if same_as(previous) {
        0.0
    } else {
        block.spacing.before.max(0.0)
    };
    let after = if same_as(blocks.get(index + 1)) {
        0.0
    } else {
        block.spacing.after.max(0.0)
    };
    (before, after)
}

/// Расстояние между низом предыдущего абзаца (с интервалом после
/// `previous_after`) и верхом следующего (с интервалом перед `before`):
/// сумма, если `add_spacing`, иначе больший из двух.
pub fn gap_between(previous_after: f32, before: f32, add_spacing: bool) -> f32 {
    if add_spacing {
        previous_after + before
    } else {
        previous_after.max(before)
    }
}

/// Расставляет абзацы друг под другом, начиная с `top` (верх интервала
/// перед первым абзацем). `add_spacing` — складывать ли интервалы
/// между абзацами (см. [`gap_between`]).
pub fn stack_paragraphs(
    blocks: &[ParagraphBlock<'_>],
    top: f32,
    add_spacing: bool,
) -> Vec<PlacedParagraph> {
    let mut y = top;
    let mut previous_after = 0.0;
    let mut placed = Vec::with_capacity(blocks.len());
    for (i, block) in blocks.iter().enumerate() {
        let (space_before, space_after) = effective_spacing(blocks, i);
        let first_top = y + gap_between(previous_after, space_before, add_spacing);
        let lines = place_lines(block.lines, block.spacing.line, first_top);
        let bottom = lines.last().map_or(first_top, |l| l.top + l.height);
        y = bottom;
        previous_after = space_after;
        placed.push(PlacedParagraph {
            space_before,
            top: first_top,
            lines,
            bottom,
            space_after,
        });
    }
    placed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lines::LineEnd;

    fn line(ascent: f32, descent: f32, leading: f32) -> Line {
        Line {
            range: 0..0,
            x: 0.0,
            width: 0.0,
            ascent,
            descent,
            leading,
            items: Vec::new(),
            end: LineEnd::Paragraph,
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn single_line_of_real_fonts() {
        use crate::{FontLibrary, ParagraphFormat, Shaper, TextSpan, TextStyle, break_lines};
        let library = FontLibrary::embedded_only();
        let mut shaper = Shaper::new();
        let mut height = |family: &str, size: f32| {
            let style = TextStyle::new(family, size);
            let p = break_lines(
                &mut shaper,
                &library,
                &[TextSpan::new("Text", style.clone())],
                &style,
                &ParagraphFormat::new(400.0),
            );
            LineRule::default().height(&p.lines[0])
        };
        // Как в Word: Times New Roman 12 пт — 13,8 пт, Calibri 11 пт —
        // 13,43 пт (через совместимые по размерам Liberation и Carlito).
        assert!((height("Times New Roman", 12.0) - 13.799).abs() < 1e-3);
        assert!((height("Calibri", 11.0) - 13.428).abs() < 1e-3);
        // Cambria 11 пт — 12,89 пт (через Caladea с теми же размерами).
        assert!((height("Cambria", 11.0) - 12.892).abs() < 1e-3);
    }

    #[test]
    fn line_rules() {
        let l = line(10.0, 3.0, 1.0);
        assert!(close(LineRule::default().height(&l), 14.0));
        assert!(close(LineRule::Multiple(1.15).height(&l), 16.1));
        assert!(close(LineRule::Multiple(2.0).height(&l), 28.0));
        assert!(close(LineRule::Exact(9.0).height(&l), 9.0));
        assert!(close(LineRule::AtLeast(9.0).height(&l), 14.0));
        assert!(close(LineRule::AtLeast(20.0).height(&l), 20.0));
        // Испорченные значения — как одинарный.
        assert!(close(LineRule::Multiple(f32::NAN).height(&l), 14.0));
        assert!(close(LineRule::Exact(-1.0).height(&l), 14.0));
    }

    #[test]
    fn multiple_puts_extra_space_below_the_text() {
        let lines = [line(10.0, 3.0, 1.0), line(10.0, 3.0, 1.0)];
        let boxes = place_lines(&lines, LineRule::Multiple(2.0), 100.0);
        assert_eq!(
            boxes,
            [
                LineBox {
                    top: 100.0,
                    height: 28.0,
                    baseline: 111.0
                },
                LineBox {
                    top: 128.0,
                    height: 28.0,
                    baseline: 139.0
                },
            ]
        );
        // При «минимум» лишнее место над текстом.
        let boxes = place_lines(&lines[..1], LineRule::AtLeast(20.0), 0.0);
        assert!(close(boxes[0].baseline, 17.0));
        // Точная высота меньше шрифта: обрезается верх, низ на месте.
        let boxes = place_lines(&lines[..1], LineRule::Exact(8.0), 0.0);
        assert!(close(boxes[0].baseline, 5.0));
    }

    #[test]
    fn paragraph_spacing_adds_up() {
        let a = [line(10.0, 3.0, 1.0)];
        let b = [line(10.0, 3.0, 1.0), line(10.0, 3.0, 1.0)];
        let spacing = ParagraphSpacing {
            before: 6.0,
            after: 10.0,
            ..ParagraphSpacing::default()
        };
        let blocks = [
            ParagraphBlock {
                lines: &a,
                spacing,
                style: None,
            },
            ParagraphBlock {
                lines: &b,
                spacing,
                style: None,
            },
        ];
        let placed = stack_paragraphs(&blocks, 0.0, true);
        assert!(close(placed[0].top, 6.0) && close(placed[0].bottom, 20.0));
        // 10 после первого + 6 перед вторым.
        assert!(close(placed[1].top, 36.0) && close(placed[1].bottom, 64.0));
        assert!(close(placed[1].lines[1].top, 50.0));
        assert!(stack_paragraphs(&[], 0.0, true).is_empty());
        // По умолчанию в Word — больший из двух: 10.
        let placed = stack_paragraphs(&blocks, 0.0, false);
        assert!(close(placed[0].top, 6.0) && close(placed[1].top, 30.0));
        assert_eq!(gap_between(10.0, 6.0, false), 10.0);
        assert_eq!(gap_between(4.0, 6.0, false), 6.0);
        assert_eq!(gap_between(4.0, 6.0, true), 10.0);
    }

    #[test]
    fn contextual_spacing_between_paragraphs_of_one_style() {
        let one = [line(10.0, 3.0, 1.0)];
        let list = ParagraphSpacing {
            before: 6.0,
            after: 10.0,
            contextual: true,
            ..ParagraphSpacing::default()
        };
        let block = |style, spacing| ParagraphBlock {
            lines: &one,
            spacing,
            style,
        };
        let blocks = [
            block(Some("List"), list),
            block(Some("List"), list),
            block(None, list),
            block(None, ParagraphSpacing::default()),
        ];
        let placed = stack_paragraphs(&blocks, 0.0, true);
        let spaces: Vec<_> = placed
            .iter()
            .map(|p| (p.space_before, p.space_after))
            .collect();
        // Между двумя «List» интервалов нет; третий абзац другого стиля
        // получает свой интервал перед, а четвёртый без флага — свои 0.
        assert_eq!(spaces, [(6.0, 0.0), (0.0, 10.0), (6.0, 0.0), (0.0, 0.0)]);
        // Третий и четвёртый — оба без стиля, поэтому у третьего нет
        // интервала после.
        assert!(close(placed[3].top, placed[2].bottom));
    }
}
