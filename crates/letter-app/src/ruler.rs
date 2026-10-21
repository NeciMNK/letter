//! Линейки над документом и слева от него, как в Word: белая часть —
//! место текста между полями, серая — поля; деления через 0,25 см,
//! числа — сантиметры от края поля в обе стороны. Перетаскивание
//! отступов и табуляций — задача F4.6.

use eframe::egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, pos2, vec2};
use letter_layout::Margins;

/// Толщина линейки, точки экрана.
pub const RULER_SIZE: f32 = 18.0;

/// Пунктов в сантиметре.
const POINTS_PER_CM: f32 = 72.0 / 2.54;

const BACKGROUND: Color32 = Color32::from_gray(232);
const MARGIN: Color32 = Color32::from_gray(205);
const TEXT_AREA: Color32 = Color32::WHITE;
const MARKS: Color32 = Color32::from_gray(90);

/// Вид деления.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickKind {
    /// Целый сантиметр: число.
    Number(u32),
    /// Полсантиметра: длинная чёрточка.
    Half,
    /// Четверть: короткая чёрточка.
    Quarter,
}

/// Деление линейки.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tick {
    /// Место, пт от начала листа.
    pub at: f32,
    /// Вид.
    pub kind: TickKind,
}

/// Деления на листе длиной `length` пт; отсчёт ведётся от `origin` пт
/// (край поля) в обе стороны. Сам край поля не отмечается.
pub fn ticks(length: f32, origin: f32) -> Vec<Tick> {
    let step = POINTS_PER_CM / 4.0;
    if length <= 0.0 || !origin.is_finite() {
        return Vec::new();
    }
    let first = -(origin / step).floor() as i64;
    let last = ((length - origin) / step).floor() as i64;
    (first..=last)
        .filter(|&k| k != 0)
        .map(|k| Tick {
            at: origin + k as f32 * step,
            kind: if k % 4 == 0 {
                TickKind::Number((k.unsigned_abs() / 4) as u32)
            } else if k % 2 == 0 {
                TickKind::Half
            } else {
                TickKind::Quarter
            },
        })
        .filter(|t| t.at >= 0.0 && t.at <= length)
        .collect()
}

/// Рисует горизонтальную линейку в полосе `strip` для листа, который
/// на экране занимает по горизонтали `page_left..` (точки экрана);
/// `width` — ширина листа, пт; `scale` — точек экрана на пункт.
pub fn paint_horizontal(
    painter: &Painter,
    strip: Rect,
    page_left: f32,
    width: f32,
    margins: Margins,
    scale: f32,
) {
    painter.rect_filled(strip, 0.0, BACKGROUND);
    let x = |pt: f32| page_left + pt * scale;
    let band = strip.shrink2(vec2(0.0, 2.0));
    let span = |a: f32, b: f32| Rect::from_x_y_ranges(x(a)..=x(b), band.y_range());
    painter.rect_filled(span(0.0, width), 0.0, MARGIN);
    painter.rect_filled(span(margins.left, width - margins.right), 0.0, TEXT_AREA);
    for tick in ticks(width, margins.left) {
        paint_tick(painter, tick, scale, |offset| {
            pos2(x(tick.at), band.center().y + offset)
        });
    }
}

/// Рисует вертикальную линейку в полосе `strip` для листа, верх
/// которого на экране — `page_top`; `height` — высота листа, пт.
pub fn paint_vertical(
    painter: &Painter,
    strip: Rect,
    page_top: f32,
    height: f32,
    margins: Margins,
    scale: f32,
) {
    painter.rect_filled(strip, 0.0, BACKGROUND);
    let y = |pt: f32| page_top + pt * scale;
    let band = strip.shrink2(vec2(2.0, 0.0));
    let span = |a: f32, b: f32| Rect::from_x_y_ranges(band.x_range(), y(a)..=y(b));
    painter.rect_filled(span(0.0, height), 0.0, MARGIN);
    painter.rect_filled(span(margins.top, height - margins.bottom), 0.0, TEXT_AREA);
    for tick in ticks(height, margins.top) {
        paint_tick(painter, tick, scale, |offset| {
            pos2(band.center().x + offset, y(tick.at))
        });
    }
}

/// Одно деление. `at(сдвиг)` — точка экрана со сдвигом поперёк линейки.
fn paint_tick(painter: &Painter, tick: Tick, scale: f32, at: impl Fn(f32) -> Pos2) {
    // При мелком масштабе мелкие деления сливаются — их не рисуем.
    let quarter = POINTS_PER_CM / 4.0 * scale;
    let line = |half: f32| {
        painter.line_segment([at(-half), at(half)], Stroke::new(1.0, MARKS));
    };
    match tick.kind {
        TickKind::Number(n) => {
            // Числа не должны налезать друг на друга.
            if quarter * 4.0 >= 14.0 || n % 2 == 0 {
                painter.text(
                    at(0.0),
                    Align2::CENTER_CENTER,
                    n.to_string(),
                    FontId::proportional(10.0),
                    MARKS,
                );
            }
        }
        TickKind::Half if quarter >= 2.0 => line(3.0),
        TickKind::Quarter if quarter >= 4.0 => line(1.0),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_count_from_the_margin_both_ways() {
        // Лист 10 см, поле 2 см.
        let ticks = ticks(10.0 * POINTS_PER_CM, 2.0 * POINTS_PER_CM);
        let numbers: Vec<(f32, u32)> = ticks
            .iter()
            .filter_map(|t| match t.kind {
                TickKind::Number(n) => Some(((t.at / POINTS_PER_CM * 100.0).round() / 100.0, n)),
                _ => None,
            })
            .collect();
        assert_eq!(
            numbers,
            vec![
                (0.0, 2),
                (1.0, 1),
                (3.0, 1),
                (4.0, 2),
                (5.0, 3),
                (6.0, 4),
                (7.0, 5),
                (8.0, 6),
                (9.0, 7),
                (10.0, 8)
            ]
        );
        // Край поля не отмечен, между сантиметрами — половина и четверти.
        let near_margin: Vec<TickKind> = ticks
            .iter()
            .filter(|t| (t.at - 2.0 * POINTS_PER_CM).abs() < POINTS_PER_CM * 0.9)
            .map(|t| t.kind)
            .collect();
        assert_eq!(
            near_margin,
            vec![
                TickKind::Quarter,
                TickKind::Half,
                TickKind::Quarter,
                TickKind::Quarter,
                TickKind::Half,
                TickKind::Quarter
            ]
        );
        assert!(super::ticks(0.0, 0.0).is_empty());
    }
}
