//! Линии границ разных видов (`w:val` у `w:top`, `w:bottom` и т. п.):
//! сплошная, двойная и тройная, точки, штрихи и их сочетания. Каждая
//! линия — несколько прямоугольников [`ShapeKind::Rule`], поэтому рисование
//! на экране и в PDF ничего не знает о видах линий.

use letter_model::{Border, Color};

use crate::pages::{Shape, ShapeKind};

/// Прямоугольники линии `border`, занимающей полосу `x`, `y`, `width` ×
/// `height` пт (линия идёт вдоль длинной стороны, толщина — короткая).
pub(crate) fn border_shapes(
    border: &Border,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> Vec<Shape> {
    let color = match border.color {
        Some(Color::Rgb(r, g, b)) => (r, g, b),
        _ => (0, 0, 0),
    };
    let piece = |x: f32, y: f32, width: f32, height: f32| Shape {
        x,
        y,
        width,
        height,
        color,
        kind: ShapeKind::Rule,
    };
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let along = width >= height;
    let (length, thickness) = if along {
        (width, height)
    } else {
        (height, width)
    };
    // Кусок полосы: от `a` до `b` по длине и от `c` до `d` по толщине
    // (доли толщины).
    let part = |a: f32, b: f32, c: f32, d: f32| {
        if along {
            piece(x + a, y + c * height, b - a, (d - c) * height)
        } else {
            piece(x + c * width, y + a, (d - c) * width, b - a)
        }
    };
    // Несколько параллельных линий: полосы толщины `stripes` (доли).
    let stripes = |bands: &[(f32, f32)]| {
        bands
            .iter()
            .map(|&(c, d)| part(0.0, length, c, d))
            .collect::<Vec<_>>()
    };
    // Узор вдоль линии: длины штрихов и промежутков по очереди, в
    // толщинах линии (но не мельче 0,5 пт, иначе точки сливаются).
    let pattern = |steps: &[f32]| {
        let unit = thickness.max(0.5);
        let mut out = Vec::new();
        let mut at = 0.0;
        let mut i = 0;
        while at < length && !steps.is_empty() {
            let step = steps[i % steps.len()] * unit;
            if i % 2 == 0 {
                out.push(part(at, (at + step).min(length), 0.0, 1.0));
            }
            at += step;
            i += 1;
        }
        out
    };
    match border.style.as_str() {
        "double" => stripes(&[(0.0, 1.0 / 3.0), (2.0 / 3.0, 1.0)]),
        "triple" => stripes(&[(0.0, 0.2), (0.4, 0.6), (0.8, 1.0)]),
        "dotted" => pattern(&[1.0, 1.0]),
        "dashed" => pattern(&[4.0, 3.0]),
        "dashSmallGap" => pattern(&[4.0, 1.0]),
        "dotDash" => pattern(&[4.0, 2.0, 1.0, 2.0]),
        "dotDotDash" => pattern(&[4.0, 2.0, 1.0, 2.0, 1.0, 2.0]),
        _ => vec![piece(x, y, width, height)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(style: &str, eighths: u32) -> Border {
        Border {
            style: style.into(),
            ..Border::single(eighths)
        }
    }

    #[test]
    fn kinds_of_lines() {
        // Сплошная — один прямоугольник во всю полосу.
        let solid = border_shapes(&line("single", 8), 0.0, 0.0, 100.0, 1.0);
        assert_eq!(solid.len(), 1);
        assert_eq!(solid[0].width, 100.0);
        // Двойная — две линии по трети толщины, между ними промежуток.
        let double = border_shapes(&line("double", 8), 0.0, 10.0, 100.0, 3.0);
        assert_eq!(double.len(), 2);
        assert!((double[0].height - 1.0).abs() < 1e-4);
        assert!((double[1].y - 12.0).abs() < 1e-4);
        // Вертикальная двойная — по ширине.
        let upright = border_shapes(&line("double", 8), 5.0, 0.0, 3.0, 100.0);
        assert!((upright[1].x - 7.0).abs() < 1e-4);
        assert_eq!(upright[1].height, 100.0);
        // Точки толщиной 1 пт через 1 пт: 50 точек на 100 пт.
        let dots = border_shapes(&line("dotted", 8), 0.0, 0.0, 100.0, 1.0);
        assert_eq!(dots.len(), 50);
        assert!(dots.iter().all(|d| (d.width - 1.0).abs() < 1e-4));
        // Штрихи не выходят за конец линии.
        let dashes = border_shapes(&line("dashed", 8), 0.0, 0.0, 10.0, 1.0);
        let end = dashes.iter().map(|d| d.x + d.width).fold(0.0, f32::max);
        assert!(end <= 10.0 + 1e-4);
        assert_eq!(dashes.len(), 2);
    }
}
