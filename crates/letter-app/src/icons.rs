//! Простые значки, нарисованные линиями (свои, не из Word): сохранить,
//! отменить, повторить, вырезать, копировать, вставить, выравнивание.
//! Шрифтов со значками в egui по умолчанию нет. Полный набор значков —
//! задача F4.2.

use std::f32::consts::PI;

use eframe::egui::{
    Color32, CornerRadius, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui,
    Vec2, WidgetInfo, pos2, vec2,
};
use letter_model::Alignment;

/// Значок.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// Дискета.
    Save,
    /// Стрелка назад.
    Undo,
    /// Стрелка вперёд.
    Redo,
    /// Ножницы.
    Cut,
    /// Два листа.
    Copy,
    /// Планшет с листом.
    Paste,
    /// Строки с выравниванием.
    Align(Alignment),
}

/// Кнопка-значок размера `size`. `name` — название кнопки для подсказки
/// и для программ чтения с экрана (и тестов).
pub fn icon_button(
    ui: &mut Ui,
    icon: Icon,
    name: &str,
    size: Vec2,
    enabled: bool,
    selected: bool,
) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response
        .widget_info(|| WidgetInfo::selected(eframe::egui::Role::Button, enabled, selected, name));
    let visuals = ui.style().interact_selectable(&response, selected);
    if enabled && (selected || response.hovered()) {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(2), visuals.weak_bg_fill);
    }
    let color = if enabled {
        visuals.fg_stroke.color
    } else {
        Color32::from_gray(170)
    };
    let side = rect.height().min(rect.width()).min(24.0) * 0.7;
    let area = Rect::from_center_size(rect.center(), Vec2::splat(side));
    paint(ui.painter(), area, icon, color);
    response
}

/// Кнопка «значок и надпись» в одну строку.
pub fn icon_text_button(ui: &mut Ui, icon: Icon, text: &str, enabled: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        eframe::egui::TextStyle::Body.resolve(ui.style()),
        Color32::PLACEHOLDER,
    );
    let height = 20.0_f32.max(galley.size().y + 4.0);
    let size = vec2(height + 4.0 + galley.size().x + 4.0, height);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| WidgetInfo::labeled(eframe::egui::Role::Button, enabled, text));
    let visuals = ui.style().interact(&response);
    if enabled && response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(2), visuals.weak_bg_fill);
    }
    let color = if enabled {
        visuals.fg_stroke.color
    } else {
        Color32::from_gray(170)
    };
    let icon_rect = Rect::from_min_size(rect.min, Vec2::splat(height));
    let side = height * 0.7;
    paint(
        ui.painter(),
        Rect::from_center_size(icon_rect.center(), Vec2::splat(side)),
        icon,
        color,
    );
    let text_pos = pos2(
        icon_rect.right() + 4.0,
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(text_pos, galley, color);
    response
}

/// Рисует значок в квадрате `r`.
pub fn paint(painter: &Painter, r: Rect, icon: Icon, color: Color32) {
    let stroke = Stroke::new((r.width() / 12.0).max(1.2), color);
    let at = |x: f32, y: f32| pos2(r.left() + x * r.width(), r.top() + y * r.height());
    match icon {
        Icon::Save => {
            let body = Rect::from_min_max(at(0.08, 0.08), at(0.92, 0.92));
            painter.rect_stroke(body, CornerRadius::same(1), stroke, StrokeKind::Middle);
            // Шторка сверху и наклейка снизу.
            painter.rect_filled(
                Rect::from_min_max(at(0.28, 0.08), at(0.68, 0.36)),
                0.0,
                color,
            );
            painter.rect_stroke(
                Rect::from_min_max(at(0.24, 0.56), at(0.76, 0.92)),
                0.0,
                stroke,
                StrokeKind::Middle,
            );
        }
        Icon::Undo | Icon::Redo => {
            let mirror = icon == Icon::Redo;
            let x = |v: f32| if mirror { 1.0 - v } else { v };
            // Дуга сверху вниз направо и наконечник слева.
            let center = at(x(0.55), 0.6);
            let radius = r.width() * 0.32;
            let points: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let a = PI * (1.0 + i as f32 / 12.0 * 0.9);
                    let dx = radius * a.cos();
                    pos2(
                        center.x + if mirror { -dx } else { dx },
                        center.y + radius * a.sin(),
                    )
                })
                .collect();
            let tip = points[0];
            painter.add(Shape::line(points, stroke));
            let head = r.width() * 0.22;
            let side = if mirror { -1.0 } else { 1.0 };
            painter.line_segment([tip, tip + vec2(-side * head * 0.2, -head)], stroke);
            painter.line_segment([tip, tip + vec2(side * head, -head * 0.15)], stroke);
        }
        Icon::Cut => {
            painter.circle_stroke(at(0.28, 0.78), r.width() * 0.14, stroke);
            painter.circle_stroke(at(0.72, 0.78), r.width() * 0.14, stroke);
            painter.line_segment([at(0.36, 0.66), at(0.72, 0.06)], stroke);
            painter.line_segment([at(0.64, 0.66), at(0.28, 0.06)], stroke);
        }
        Icon::Copy => {
            let back = Rect::from_min_max(at(0.08, 0.08), at(0.62, 0.7));
            let front = Rect::from_min_max(at(0.38, 0.3), at(0.92, 0.92));
            painter.rect_stroke(back, 0.0, stroke, StrokeKind::Middle);
            painter.rect_filled(front, 0.0, Color32::WHITE);
            painter.rect_stroke(front, 0.0, stroke, StrokeKind::Middle);
        }
        Icon::Paste => {
            let board = Rect::from_min_max(at(0.12, 0.14), at(0.88, 0.94));
            painter.rect_stroke(board, CornerRadius::same(1), stroke, StrokeKind::Middle);
            painter.rect_filled(
                Rect::from_min_max(at(0.34, 0.04), at(0.66, 0.24)),
                0.0,
                color,
            );
            for y in [0.46, 0.62, 0.78] {
                painter.line_segment([at(0.28, y), at(0.72, y)], Stroke::new(1.0, color));
            }
        }
        Icon::Align(alignment) => {
            for (i, long) in [true, false, true, false].into_iter().enumerate() {
                let width = if long || alignment == Alignment::Justify {
                    1.0
                } else {
                    0.6
                };
                let (left, right) = match alignment {
                    Alignment::Center => (0.5 - width / 2.0, 0.5 + width / 2.0),
                    Alignment::Right => (1.0 - width, 1.0),
                    _ => (0.0, width),
                };
                let y = 0.1 + i as f32 * 0.8 / 3.0;
                painter.line_segment([at(left, y), at(right, y)], stroke);
            }
        }
    }
}
