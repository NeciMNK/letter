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
    /// Видимая часть каждого листа: в разметке страницы — весь лист,
    /// в черновике — только место текста между полями.
    pub clips: Vec<Rect>,
    /// Размер всего содержимого.
    pub size: Vec2,
}

/// Режим просмотра, как в Word («Вид → Режимы»).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// Разметка страницы: листы с полями друг под другом.
    #[default]
    Print,
    /// Режим чтения: листы по два рядом, без ленты, масштаб по высоте.
    Read,
    /// Веб-документ: текст по ширине окна, без листов.
    Web,
    /// Черновик: только текст, без полей и колонтитулов, листы
    /// разделены пунктирной линией.
    Draft,
    /// Структура: абзацы с отступом по уровню заголовка, у каждого — значок.
    Outline,
}

impl ViewMode {
    /// Все режимы в порядке кнопок на ленте.
    pub const ALL: [ViewMode; 5] = [
        ViewMode::Read,
        ViewMode::Print,
        ViewMode::Web,
        ViewMode::Outline,
        ViewMode::Draft,
    ];

    /// Имя в переводах (`view-…`).
    pub fn key(self) -> &'static str {
        match self {
            ViewMode::Print => "view-print",
            ViewMode::Read => "view-read",
            ViewMode::Web => "view-web",
            ViewMode::Draft => "view-draft",
            ViewMode::Outline => "view-outline",
        }
    }

    /// Листы идут сплошной лентой без промежутков (черновик, веб,
    /// структура).
    pub fn continuous(self) -> bool {
        matches!(self, ViewMode::Web | ViewMode::Draft | ViewMode::Outline)
    }
}

/// Лист для расстановки: ширина и высота, поля сверху и снизу, пт.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageBox {
    /// Ширина листа.
    pub width: f32,
    /// Высота листа.
    pub height: f32,
    /// Поле сверху.
    pub top: f32,
    /// Поле снизу.
    pub bottom: f32,
}

/// Ставит листы для режима `mode`: в разметке страницы и веб-документе —
/// как [`place_pages`]; в черновике и структуре — только полосы текста
/// между полями, вплотную друг к другу; в режиме чтения — по два листа
/// рядом.
pub fn place_view(pages: &[PageBox], zoom: f32, view_width: f32, mode: ViewMode) -> PagesPlacement {
    let scale = zoom * SCREEN_PIXELS_PER_POINT;
    match mode {
        ViewMode::Print => {
            let sizes: Vec<(f32, f32)> = pages.iter().map(|p| (p.width, p.height)).collect();
            place_pages(&sizes, zoom, view_width)
        }
        ViewMode::Read => {
            let widest = pages.iter().map(|p| p.width).fold(0.0, f32::max) * scale;
            // Один лист стоит посередине, а не слева от пустого места.
            let columns = pages.len().clamp(1, 2) as f32;
            let row = columns * widest + (columns + 1.0) * PAGE_GAP;
            let width = view_width.max(row);
            let left = (width - row) / 2.0 + PAGE_GAP;
            let mut rects = Vec::with_capacity(pages.len());
            let mut y = PAGE_GAP;
            for pair in pages.chunks(2) {
                let mut tallest: f32 = 0.0;
                for (i, p) in pair.iter().enumerate() {
                    let size = vec2(p.width * scale, p.height * scale);
                    let x = left + i as f32 * (widest + PAGE_GAP);
                    rects.push(Rect::from_min_size(Pos2::new(x, y), size));
                    tallest = tallest.max(size.y);
                }
                y += tallest + PAGE_GAP;
            }
            PagesPlacement {
                clips: rects.clone(),
                rects,
                size: vec2(width, y),
            }
        }
        ViewMode::Web | ViewMode::Draft | ViewMode::Outline => {
            let widest = pages.iter().map(|p| p.width).fold(0.0, f32::max) * scale;
            let width = view_width.max(widest + 2.0 * PAGE_GAP);
            let mut y = PAGE_GAP;
            let mut rects = Vec::with_capacity(pages.len());
            let mut clips = Vec::with_capacity(pages.len());
            for p in pages {
                let size = vec2(p.width * scale, p.height * scale);
                let band = (p.height - p.top - p.bottom).max(0.0) * scale;
                let x = (width - size.x) / 2.0;
                rects.push(Rect::from_min_size(Pos2::new(x, y - p.top * scale), size));
                clips.push(Rect::from_min_size(Pos2::new(x, y), vec2(size.x, band)));
                y += band;
            }
            PagesPlacement {
                rects,
                clips,
                size: vec2(width, y + PAGE_GAP),
            }
        }
    }
}

/// Масштаб для режима чтения: два листа рядом целиком помещаются
/// в окно `view` (точки).
pub fn read_zoom(page: (f32, f32), view: Vec2) -> f32 {
    let (width, height) = page;
    if width <= 0.0 || height <= 0.0 {
        return 1.0;
    }
    let across = (view.x - 3.0 * PAGE_GAP) / (2.0 * width * SCREEN_PIXELS_PER_POINT);
    let down = (view.y - 2.0 * PAGE_GAP) / (height * SCREEN_PIXELS_PER_POINT);
    clamp_zoom(across.min(down))
}

/// Ставит листы размерами `pages` (ширина и высота, пт) друг под другом
/// по центру области шириной `view_width` точек.
pub fn place_pages(pages: &[(f32, f32)], zoom: f32, view_width: f32) -> PagesPlacement {
    let scale = zoom * SCREEN_PIXELS_PER_POINT;
    let widest = pages.iter().map(|&(w, _)| w).fold(0.0, f32::max) * scale;
    let width = view_width.max(widest + 2.0 * PAGE_GAP);
    let mut y = PAGE_GAP;
    let rects: Vec<Rect> = pages
        .iter()
        .map(|&(w, h)| {
            let size = vec2(w * scale, h * scale);
            let rect = Rect::from_min_size(Pos2::new((width - size.x) / 2.0, y), size);
            y += size.y + PAGE_GAP;
            rect
        })
        .collect();
    PagesPlacement {
        clips: rects.clone(),
        rects,
        size: vec2(width, y),
    }
}

/// Номер листа (с нуля), который виден в середине области, если её
/// верхний край прокручен до `top`, а высота — `height`.
pub fn current_page(placement: &PagesPlacement, top: f32, height: f32) -> usize {
    let middle = top + height / 2.0;
    let clips = &placement.clips;
    // Граница между листами — посередине промежутка между ними.
    (0..clips.len())
        .position(|i| {
            let end = clips[i].max.y;
            let next = clips.get(i + 1).map_or(end + PAGE_GAP, |r| r.min.y);
            middle < (end + next) / 2.0
        })
        .unwrap_or_else(|| clips.len().saturating_sub(1))
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

    fn boxes(n: usize) -> Vec<PageBox> {
        vec![
            PageBox {
                width: 612.0,
                height: 792.0,
                top: 72.0,
                bottom: 72.0,
            };
            n
        ]
    }

    #[test]
    fn draft_shows_text_bands_without_gaps() {
        let placement = place_view(&boxes(2), 1.0, 1000.0, ViewMode::Draft);
        let [a, b] = [placement.clips[0], placement.clips[1]];
        // Полоса — лист без полей; следующая сразу под ней.
        assert!((a.height() - 648.0 * SCREEN_PIXELS_PER_POINT).abs() < 1e-3);
        assert!((b.min.y - a.max.y).abs() < 1e-3);
        // Лист сдвинут вверх на поле: текст начинается в начале полосы.
        let r = placement.rects[0];
        assert!((a.min.y - (r.min.y + 72.0 * SCREEN_PIXELS_PER_POINT)).abs() < 1e-3);
        assert_eq!(current_page(&placement, b.min.y, 10.0), 1);
    }

    #[test]
    fn reading_puts_two_pages_side_by_side() {
        let placement = place_view(&boxes(3), 0.5, 1000.0, ViewMode::Read);
        let [a, b, c] = [placement.rects[0], placement.rects[1], placement.rects[2]];
        assert_eq!(a.min.y, b.min.y);
        assert!(b.min.x > a.max.x);
        assert!(c.min.y > a.max.y);
        assert_eq!(c.min.x, a.min.x);
        // Масштаб чтения: оба листа помещаются по ширине и высоте.
        let zoom = read_zoom((612.0, 792.0), vec2(1000.0, 700.0));
        let scale = zoom * SCREEN_PIXELS_PER_POINT;
        assert!(2.0 * 612.0 * scale + 3.0 * PAGE_GAP <= 1000.0 + 1e-3);
        assert!(792.0 * scale + 2.0 * PAGE_GAP <= 700.0 + 1e-3);
        // Разметка страницы — как раньше.
        let print = place_view(&boxes(2), 1.0, 1000.0, ViewMode::Print);
        assert_eq!(print.rects, place_pages(&[LETTER; 2], 1.0, 1000.0).rects);
    }
}
