//! Расчёты просмотрщика без окна: масштаб, где на экране стоят листы,
//! куда прокрутить после изменения масштаба.
//!
//! Все размеры здесь — в точках egui. Размеры листов — в пунктах
//! документа; при масштабе 100 % пункт занимает
//! [`SCREEN_PIXELS_PER_POINT`] точек, как в Word.

use eframe::egui::{Pos2, Rect, Vec2, vec2};
use letter_render::SCREEN_PIXELS_PER_POINT;

/// Наименьший масштаб, 10 %.
pub const MIN_ZOOM: f32 = 0.10;

/// Наибольший масштаб, 500 %.
pub const MAX_ZOOM: f32 = 5.0;

/// Шаг кнопок «увеличить» и «уменьшить», как у ползунка масштаба в Word.
pub const ZOOM_STEP: f32 = 0.10;

/// Промежуток между листами и от листов до краёв окна, точки.
pub const PAGE_GAP: f32 = 16.0;

/// Как выбирается масштаб.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Zoom {
    /// Заданный масштаб: 1.0 — 100 %.
    Fixed(f32),
    /// «По ширине страницы»: самый широкий лист занимает всю ширину
    /// окна; пересчитывается при изменении размера окна.
    PageWidth,
}

impl Default for Zoom {
    fn default() -> Zoom {
        Zoom::Fixed(1.0)
    }
}

/// Масштаб в пределах 10–500 %.
pub fn clamp_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() {
        zoom.clamp(MIN_ZOOM, MAX_ZOOM)
    } else {
        1.0
    }
}

/// Масштаб, при котором лист шириной `page_width` пт вместе
/// с промежутками по бокам занимает `view_width` точек.
pub fn page_width_zoom(page_width: f32, view_width: f32) -> f32 {
    if page_width <= 0.0 {
        return 1.0;
    }
    clamp_zoom((view_width - 2.0 * PAGE_GAP) / (page_width * SCREEN_PIXELS_PER_POINT))
}

/// Следующий масштаб при нажатии «+» (`up`) или «−»: ближайшее
/// круглое значение с шагом [`ZOOM_STEP`].
pub fn step_zoom(zoom: f32, up: bool) -> f32 {
    let steps = zoom / ZOOM_STEP;
    // Небольшой допуск, чтобы 0.3 / 0.1 = 2.9999… считалось ровно тремя.
    let next = if up {
        (steps + 1e-3).floor() + 1.0
    } else {
        (steps - 1e-3).ceil() - 1.0
    };
    clamp_zoom(next * ZOOM_STEP)
}

/// Расположение листов в прокручиваемой области.
#[derive(Debug, Clone, PartialEq)]
pub struct PagesPlacement {
    /// Прямоугольник каждого листа; (0, 0) — левый верхний угол
    /// содержимого области.
    pub rects: Vec<Rect>,
    /// Размер всего содержимого.
    pub size: Vec2,
}

/// Ставит листы размерами `pages` (ширина и высота, пт) друг под другом
/// по центру области шириной `view_width` точек.
pub fn place_pages(pages: &[(f32, f32)], zoom: f32, view_width: f32) -> PagesPlacement {
    let scale = zoom * SCREEN_PIXELS_PER_POINT;
    let widest = pages.iter().map(|&(w, _)| w).fold(0.0, f32::max) * scale;
    let width = view_width.max(widest + 2.0 * PAGE_GAP);
    let mut y = PAGE_GAP;
    let rects = pages
        .iter()
        .map(|&(w, h)| {
            let size = vec2(w * scale, h * scale);
            let rect = Rect::from_min_size(Pos2::new((width - size.x) / 2.0, y), size);
            y += size.y + PAGE_GAP;
            rect
        })
        .collect();
    PagesPlacement {
        rects,
        size: vec2(width, y),
    }
}

/// Номер листа (с нуля), который виден в середине области, если её
/// верхний край прокручен до `top`, а высота — `height`.
pub fn current_page(placement: &PagesPlacement, top: f32, height: f32) -> usize {
    let middle = top + height / 2.0;
    placement
        .rects
        .iter()
        .position(|r| middle < r.max.y + PAGE_GAP / 2.0)
        .unwrap_or_else(|| placement.rects.len().saturating_sub(1))
}

/// Новая прокрутка после изменения масштаба в `ratio` раз, чтобы
/// точка `anchor` (от левого верхнего угла видимой части) осталась
/// под тем же местом документа.
pub fn anchored_offset(offset: Vec2, anchor: Vec2, ratio: f32) -> Vec2 {
    let new = (offset + anchor) * ratio - anchor;
    vec2(new.x.max(0.0), new.y.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LETTER: (f32, f32) = (612.0, 792.0);

    #[test]
    fn zoom_is_limited_to_10_500_percent() {
        assert_eq!(clamp_zoom(0.01), MIN_ZOOM);
        assert_eq!(clamp_zoom(9.0), MAX_ZOOM);
        assert_eq!(clamp_zoom(1.5), 1.5);
        assert_eq!(clamp_zoom(f32::NAN), 1.0);
    }

    #[test]
    fn zoom_steps_go_to_round_values() {
        assert!((step_zoom(1.0, true) - 1.1).abs() < 1e-5);
        assert!((step_zoom(1.0, false) - 0.9).abs() < 1e-5);
        assert!((step_zoom(1.23, true) - 1.3).abs() < 1e-5);
        assert!((step_zoom(1.23, false) - 1.2).abs() < 1e-5);
        assert!((step_zoom(0.3, false) - 0.2).abs() < 1e-5);
        assert_eq!(step_zoom(MAX_ZOOM, true), MAX_ZOOM);
        assert_eq!(step_zoom(MIN_ZOOM, false), MIN_ZOOM);
    }

    #[test]
    fn page_width_fills_the_view() {
        let zoom = page_width_zoom(612.0, 1000.0);
        let placement = place_pages(&[LETTER], zoom, 1000.0);
        let rect = placement.rects[0];
        assert!((rect.min.x - PAGE_GAP).abs() < 1e-3, "{rect:?}");
        assert!((rect.max.x - (1000.0 - PAGE_GAP)).abs() < 1e-3, "{rect:?}");
        assert_eq!(page_width_zoom(612.0, 10.0), MIN_ZOOM);
        assert_eq!(page_width_zoom(0.0, 1000.0), 1.0);
    }

    #[test]
    fn pages_are_stacked_and_centered() {
        // 100 %: лист Letter — 816 × 1056 точек.
        let placement = place_pages(&[LETTER, LETTER], 1.0, 1000.0);
        let [a, b] = [placement.rects[0], placement.rects[1]];
        assert!((a.width() - 816.0).abs() < 1e-3);
        assert!((a.height() - 1056.0).abs() < 1e-3);
        assert!((a.min.x - 92.0).abs() < 1e-3);
        assert_eq!(a.min.y, PAGE_GAP);
        assert!((b.min.y - (a.max.y + PAGE_GAP)).abs() < 1e-3);
        assert_eq!(placement.size.x, 1000.0);
        assert!((placement.size.y - (b.max.y + PAGE_GAP)).abs() < 1e-3);

        // Окно уже листа: появляется прокрутка по горизонтали.
        let placement = place_pages(&[LETTER], 2.0, 500.0);
        assert_eq!(placement.rects[0].min.x, PAGE_GAP);
        assert!(placement.size.x > 500.0);
    }

    #[test]
    fn current_page_is_the_one_in_the_middle() {
        let placement = place_pages(&[LETTER; 3], 1.0, 1000.0);
        assert_eq!(current_page(&placement, 0.0, 600.0), 0);
        let second = placement.rects[1];
        assert_eq!(current_page(&placement, second.min.y, 600.0), 1);
        assert_eq!(current_page(&placement, 1e6, 600.0), 2);
        assert_eq!(current_page(&place_pages(&[], 1.0, 10.0), 0.0, 10.0), 0);
    }

    #[test]
    fn zoom_keeps_the_point_under_the_mouse() {
        let offset = vec2(0.0, 1000.0);
        let anchor = vec2(300.0, 200.0);
        let new = anchored_offset(offset, anchor, 2.0);
        // Точка документа под мышью была на 1200, стала на 2400.
        assert_eq!(new, vec2(300.0, 2200.0));
        // Прокрутка не уходит в минус.
        assert_eq!(anchored_offset(Vec2::ZERO, anchor, 0.5), Vec2::ZERO);
    }
}
