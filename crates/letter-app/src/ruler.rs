//! Линейки над документом и слева от него, как в Word: белая часть —
//! место текста между полями, серая — поля; деления через 0,25 см,
//! числа — сантиметры от края поля в обе стороны. На горизонтальной
//! линейке — отступы и позиции табуляции абзаца у курсора: их можно
//! перетаскивать мышью, щелчок ставит новую позицию табуляции, а
//! стащенная с линейки позиция убирается. Вид новой позиции выбирается
//! кнопкой в углу между линейками.

use crate::i18n::tr;
use eframe::egui::{
    Align2, Color32, CursorIcon, FontId, Painter, Pos2, Rect, Role, Sense, Shape, Stroke, Ui,
    WidgetInfo, pos2, vec2,
};
use letter_editor::ParagraphFormat;
use letter_layout::Margins;

use crate::theme::Palette;
use letter_model::{
    FirstLineIndent, ResolvedParagraphProps, TabLeader, TabStop, TabStopKind, Twips,
};

/// Толщина линейки, точки экрана.
pub const RULER_SIZE: f32 = 18.0;

/// Пунктов в сантиметре.
const POINTS_PER_CM: f32 = 72.0 / 2.54;

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
    let colors = Palette::of(painter.ctx());
    painter.rect_filled(strip, 0.0, colors.ruler);
    let x = |pt: f32| page_left + pt * scale;
    let band = strip.shrink2(vec2(0.0, 2.0));
    let span = |a: f32, b: f32| Rect::from_x_y_ranges(x(a)..=x(b), band.y_range());
    painter.rect_filled(span(0.0, width), 0.0, colors.ruler_margin);
    painter.rect_filled(
        span(margins.left, width - margins.right),
        0.0,
        colors.ruler_text,
    );
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
    let colors = Palette::of(painter.ctx());
    painter.rect_filled(strip, 0.0, colors.ruler);
    let y = |pt: f32| page_top + pt * scale;
    let band = strip.shrink2(vec2(2.0, 0.0));
    let span = |a: f32, b: f32| Rect::from_x_y_ranges(band.x_range(), y(a)..=y(b));
    painter.rect_filled(span(0.0, height), 0.0, colors.ruler_margin);
    painter.rect_filled(
        span(margins.top, height - margins.bottom),
        0.0,
        colors.ruler_text,
    );
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
    let marks = Palette::of(painter.ctx()).ruler_marks;
    let line = |half: f32| {
        painter.line_segment([at(-half), at(half)], Stroke::new(1.0, marks));
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
                    marks,
                );
            }
        }
        TickKind::Half if quarter >= 2.0 => line(3.0),
        TickKind::Quarter if quarter >= 4.0 => line(1.0),
        _ => {}
    }
}

/// Отступы и позиции табуляции абзаца у курсора, пт от левого поля.
#[derive(Debug, Clone, PartialEq)]
pub struct RulerParagraph {
    /// Отступ слева.
    pub left: f32,
    /// Отступ справа (от правого поля).
    pub right: f32,
    /// Сдвиг первой строки от отступа слева: больше нуля — отступ,
    /// меньше — выступ.
    pub first_line: f32,
    /// Задан ли у абзаца особый отступ первой строки.
    pub has_first_line: bool,
    /// Позиции табуляции.
    pub tabs: Vec<TabStop>,
    /// Шаг табуляции по умолчанию.
    pub default_tab: f32,
}

impl RulerParagraph {
    /// Значения для линейки из итогового оформления абзаца.
    pub fn new(props: &ResolvedParagraphProps, default_tab: Twips) -> RulerParagraph {
        let first_line = match props.first_line {
            Some(FirstLineIndent::FirstLine(t)) => points(t),
            Some(FirstLineIndent::Hanging(t)) => -points(t),
            None => 0.0,
        };
        RulerParagraph {
            left: points(props.indent_left),
            right: points(props.indent_right),
            first_line,
            has_first_line: props.first_line.is_some(),
            tabs: props.tabs.clone(),
            default_tab: points(default_tab),
        }
    }
}

fn points(t: Twips) -> f32 {
    t.0 as f32 / 20.0
}

fn twips(points: f32) -> Twips {
    Twips((points * 20.0).round() as i32)
}

/// Метка на линейке.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Marker {
    /// Отступ первой строки: треугольник сверху.
    FirstLine,
    /// Выступ: треугольник снизу (двигает отступ слева, первая строка
    /// остаётся на месте).
    Hanging,
    /// Отступ слева: прямоугольник под выступом (двигает и первую строку).
    Left,
    /// Отступ справа.
    Right,
    /// Позиция табуляции в этом месте.
    Tab(Twips),
}

impl Marker {
    /// Название для подсказки.
    pub fn name(self) -> &'static str {
        match self {
            Marker::FirstLine => tr("ruler-first-line"),
            Marker::Hanging => tr("ruler-hanging"),
            Marker::Left => tr("ruler-left"),
            Marker::Right => tr("ruler-right"),
            Marker::Tab(_) => tr("ruler-tab"),
        }
    }
}

/// Виды новой позиции табуляции по порядку кнопки в углу.
pub const TAB_KINDS: [TabStopKind; 5] = [
    TabStopKind::Left,
    TabStopKind::Center,
    TabStopKind::Right,
    TabStopKind::Decimal,
    TabStopKind::Bar,
];

/// Название вида табуляции.
pub fn tab_kind_name(kind: TabStopKind) -> &'static str {
    match kind {
        TabStopKind::Left => tr("tab-stop-left"),
        TabStopKind::Center => tr("tab-stop-center"),
        TabStopKind::Right => tr("tab-stop-right"),
        TabStopKind::Decimal => tr("tab-stop-decimal"),
        TabStopKind::Bar => tr("tab-stop-bar"),
        TabStopKind::Clear => tr("tab-stop-clear"),
    }
}

/// Состояние горизонтальной линейки между кадрами.
#[derive(Debug, Clone, Default)]
pub struct RulerState {
    /// Вид новой позиции табуляции: номер в [`TAB_KINDS`].
    pub tab_kind: usize,
    /// Что перетаскивается и куда (пт от левого поля), и не стащено ли
    /// за пределы линейки.
    drag: Option<(Marker, f32, bool)>,
}

impl RulerState {
    /// Вид новой позиции табуляции.
    pub fn tab_kind(&self) -> TabStopKind {
        TAB_KINDS[self.tab_kind % TAB_KINDS.len()]
    }
}

/// Где метки: середина по горизонтали (пт от левого поля) и полоса
/// по вертикали (доли толщины линейки, сверху вниз). Табуляции первые:
/// они мельче и лежат поверх отступов.
fn marker_places(para: &RulerParagraph, text_width: f32) -> Vec<(Marker, f32, (f32, f32))> {
    let mut places: Vec<(Marker, f32, (f32, f32))> = para
        .tabs
        .iter()
        .map(|t| (Marker::Tab(t.position), points(t.position), (0.4, 1.0)))
        .collect();
    places.extend([
        (Marker::FirstLine, para.left + para.first_line, (0.0, 0.45)),
        (Marker::Hanging, para.left, (0.45, 0.72)),
        (Marker::Left, para.left, (0.72, 1.0)),
        (Marker::Right, text_width - para.right, (0.45, 1.0)),
    ]);
    places
}

/// Метка под точкой `at` (смещение от левого поля, пт; доля толщины
/// линейки сверху); `tolerance` — допуск по горизонтали, пт.
pub fn marker_at(
    para: &RulerParagraph,
    text_width: f32,
    at: f32,
    depth: f32,
    tolerance: f32,
) -> Option<Marker> {
    marker_places(para, text_width)
        .into_iter()
        .filter(|(_, x, (top, bottom))| {
            (at - x).abs() <= tolerance && depth >= *top && depth <= *bottom
        })
        .min_by(|a, b| (at - a.1).abs().total_cmp(&(at - b.1).abs()))
        .map(|(marker, _, _)| marker)
}

/// Привязка к делениям линейки (0,25 см).
pub fn snap(points: f32) -> f32 {
    let step = POINTS_PER_CM / 4.0;
    (points / step).round() * step
}

/// Что меняет перенос метки `marker` в точку `to` (пт от левого поля).
/// `off` — позицию табуляции стащили с линейки. `None` — ничего.
pub fn drag_result(
    para: &RulerParagraph,
    marker: Marker,
    to: f32,
    off: bool,
    text_width: f32,
) -> Option<ParagraphFormat> {
    let first_line = |offset: Twips| match offset.0 {
        o if o > 0 => Some(FirstLineIndent::FirstLine(offset)),
        o if o < 0 => Some(FirstLineIndent::Hanging(Twips(-o))),
        _ if para.has_first_line => Some(FirstLineIndent::FirstLine(Twips(0))),
        _ => None,
    };
    let indents = |left: f32, right: f32, first: f32| ParagraphFormat::Indents {
        left: twips(left),
        right: twips(right),
        first_line: first_line(twips(first)),
    };
    let result = match marker {
        Marker::FirstLine => indents(para.left, para.right, to - para.left),
        Marker::Hanging => indents(to, para.right, para.left + para.first_line - to),
        Marker::Left => indents(to, para.right, para.first_line),
        Marker::Right => indents(para.left, text_width - to, para.first_line),
        Marker::Tab(at) if off => ParagraphFormat::RemoveTab(at),
        Marker::Tab(at) if twips(to) == at => return None,
        Marker::Tab(at) => ParagraphFormat::MoveTab {
            from: at,
            to: twips(to),
        },
    };
    let unchanged = indents(para.left, para.right, para.first_line);
    (result != unchanged).then_some(result)
}

/// Горизонтальная линейка с метками абзаца `para`: рисует её
/// в полосе `strip` и обрабатывает мышь. Возвращает правку абзаца
/// и, пока метку тащат, положение направляющей (x экрана).
#[allow(clippy::too_many_arguments)]
pub fn horizontal_ui(
    ui: &mut Ui,
    strip: Rect,
    page_left: f32,
    width: f32,
    margins: Margins,
    scale: f32,
    para: &RulerParagraph,
    state: &mut RulerState,
) -> (Option<ParagraphFormat>, Option<f32>) {
    let painter = ui.painter_at(strip);
    paint_horizontal(&painter, strip, page_left, width, margins, scale);
    let text_width = width - margins.left - margins.right;
    let origin = page_left + margins.left * scale;
    let to_points = |x: f32| (x - origin) / scale;
    // Метки можно увести до краёв листа.
    let clamp = |pt: f32| pt.clamp(-margins.left, text_width + margins.right);

    let depth = |p: Pos2| (p.y - strip.top()) / strip.height();
    let tolerance = 5.0 / scale;
    let response = ui.interact(
        strip,
        ui.scope_id().with("ruler-horizontal"),
        Sense::click_and_drag(),
    );
    response.widget_info(|| WidgetInfo::labeled(Role::Group, true, tr("ruler")));
    let hovered = response
        .hover_pos()
        .and_then(|p| marker_at(para, text_width, to_points(p.x), depth(p), tolerance));
    let response = match hovered {
        Some(marker) => {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
            response.on_hover_text(marker.name())
        }
        None => response.on_hover_text(tab_kind_name(state.tab_kind())),
    };

    let free = ui.input(|i| i.modifiers.alt);
    let place = |p: Pos2| {
        let pt = clamp(to_points(p.x));
        if free { pt } else { snap(pt) }
    };
    let mut edit = None;
    if response.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
        && let Some(marker) = marker_at(para, text_width, to_points(p.x), depth(p), tolerance)
    {
        state.drag = Some((marker, place(p), false));
    }
    if let Some((marker, _, _)) = state.drag
        && let Some(p) = response.interact_pointer_pos()
    {
        let off = matches!(marker, Marker::Tab(_))
            && (p.y - strip.center().y).abs() > strip.height() * 1.5;
        state.drag = Some((marker, place(p), off));
    }
    if response.drag_stopped()
        && let Some((marker, to, off)) = state.drag.take()
    {
        edit = drag_result(para, marker, to, off, text_width);
    }
    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        let at = place(p);
        if marker_at(para, text_width, to_points(p.x), depth(p), tolerance).is_none()
            && at > 0.0
            && at < text_width
        {
            edit = Some(ParagraphFormat::AddTab(TabStop {
                position: twips(at),
                kind: state.tab_kind(),
                leader: TabLeader::None,
            }));
        }
    }

    // Табуляции по умолчанию — серые чёрточки после последней своей.
    let last = para
        .tabs
        .iter()
        .map(|t| points(t.position))
        .fold(0.0, f32::max);
    if para.default_tab > 1.0 {
        let mut at = ((last / para.default_tab).floor() + 1.0) * para.default_tab;
        while at < text_width {
            let x = origin + at * scale;
            painter.line_segment(
                [pos2(x, strip.bottom() - 4.0), pos2(x, strip.bottom() - 1.0)],
                Stroke::new(1.0, Color32::from_gray(150)),
            );
            at += para.default_tab;
        }
    }

    // Перетаскиваемая метка рисуется на новом месте.
    let mut shown = para.clone();
    let mut guide = None;
    if let Some((marker, to, off)) = state.drag {
        guide = (!off).then_some(origin + to * scale);
        match marker {
            Marker::FirstLine => shown.first_line = to - shown.left,
            Marker::Hanging => {
                shown.first_line += shown.left - to;
                shown.left = to;
            }
            Marker::Left => shown.left = to,
            Marker::Right => shown.right = text_width - to,
            Marker::Tab(at) => {
                for tab in &mut shown.tabs {
                    if tab.position == at {
                        tab.position = twips(to);
                    }
                }
                if off {
                    shown.tabs.retain(|t| t.position != twips(to));
                }
            }
        }
    }
    paint_markers(&painter, strip, origin, scale, &shown, text_width);
    (edit, guide)
}

/// Рисует отступы и позиции табуляции.
fn paint_markers(
    painter: &Painter,
    strip: Rect,
    origin: f32,
    scale: f32,
    para: &RulerParagraph,
    text_width: f32,
) {
    let x = |pt: f32| (origin + pt * scale).round() + 0.5;
    let (top, h) = (strip.top(), strip.height());
    let colors = Palette::of(painter.ctx());
    let fill = colors.marker;
    let stroke = Stroke::new(1.0, Color32::from_gray(if colors.dark { 30 } else { 70 }));
    let triangle = |points: Vec<Pos2>| {
        painter.add(Shape::convex_polygon(points, fill, stroke));
    };
    let first = x(para.left + para.first_line);
    triangle(vec![
        pos2(first - 4.5, top + 1.0),
        pos2(first + 4.5, top + 1.0),
        pos2(first, top + h * 0.45),
    ]);
    let left = x(para.left);
    triangle(vec![
        pos2(left, top + h * 0.45),
        pos2(left + 4.5, top + h * 0.72),
        pos2(left - 4.5, top + h * 0.72),
    ]);
    painter.rect(
        Rect::from_min_max(
            pos2(left - 4.5, top + h * 0.72),
            pos2(left + 4.5, strip.bottom() - 1.0),
        ),
        0.0,
        fill,
        stroke,
        eframe::egui::StrokeKind::Middle,
    );
    let right = x(text_width - para.right);
    triangle(vec![
        pos2(right, top + h * 0.45),
        pos2(right + 4.5, strip.bottom() - 1.0),
        pos2(right - 4.5, strip.bottom() - 1.0),
    ]);
    for tab in &para.tabs {
        paint_tab(
            painter,
            tab.kind,
            x(points(tab.position)),
            strip.bottom() - 3.0,
            h * 0.4,
        );
    }
}

/// Значок позиции табуляции: угол или крестик из линий, низ — `bottom`.
pub fn paint_tab(painter: &Painter, kind: TabStopKind, x: f32, bottom: f32, height: f32) {
    let stroke = Stroke::new(1.5, Palette::of(painter.ctx()).text);
    let line = |a: Pos2, b: Pos2| {
        painter.line_segment([a, b], stroke);
    };
    let top = bottom - height;
    line(pos2(x, top), pos2(x, bottom));
    let arm = height * 0.8;
    match kind {
        TabStopKind::Left => line(pos2(x, bottom), pos2(x + arm, bottom)),
        TabStopKind::Right => line(pos2(x - arm, bottom), pos2(x, bottom)),
        TabStopKind::Center => line(pos2(x - arm, bottom), pos2(x + arm, bottom)),
        TabStopKind::Decimal => {
            line(pos2(x - arm, bottom), pos2(x + arm, bottom));
            painter.circle_filled(pos2(x + arm * 0.6, top + height * 0.4), 1.2, stroke.color);
        }
        TabStopKind::Bar | TabStopKind::Clear => {}
    }
}

/// Кнопка в углу между линейками: какой вид табуляции ставит щелчок
/// по линейке. Щелчок переключает вид.
pub fn tab_kind_ui(ui: &mut Ui, corner: Rect, state: &mut RulerState) {
    let kind = state.tab_kind();
    let response = ui
        .interact(corner, ui.scope_id().with("ruler-tab-kind"), Sense::click())
        .on_hover_text(tab_kind_name(kind));
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, tab_kind_name(kind)));
    if response.clicked() {
        state.tab_kind = (state.tab_kind + 1) % TAB_KINDS.len();
    }
    let painter = ui.painter_at(corner);
    painter.rect_filled(corner, 0.0, Palette::of(ui.ctx()).ruler);
    let inner = corner.shrink(3.0);
    painter.rect_stroke(
        inner,
        1.0,
        Stroke::new(1.0, Color32::from_gray(160)),
        eframe::egui::StrokeKind::Inside,
    );
    paint_tab(
        &painter,
        state.tab_kind(),
        inner.center().x - 1.0,
        inner.bottom() - 3.0,
        inner.height() - 6.0,
    );
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

    fn paragraph() -> RulerParagraph {
        RulerParagraph {
            left: 36.0,
            right: 0.0,
            first_line: -18.0,
            has_first_line: true,
            tabs: vec![TabStop {
                position: Twips(2880),
                kind: TabStopKind::Right,
                leader: TabLeader::None,
            }],
            default_tab: 36.0,
        }
    }

    #[test]
    fn markers_are_found_by_place() {
        let p = paragraph();
        let at = |x, depth| marker_at(&p, 400.0, x, depth, 3.0);
        assert_eq!(at(18.0, 0.2), Some(Marker::FirstLine));
        assert_eq!(at(37.0, 0.6), Some(Marker::Hanging));
        assert_eq!(at(35.0, 0.9), Some(Marker::Left));
        assert_eq!(at(400.0, 0.8), Some(Marker::Right));
        assert_eq!(at(144.0, 0.8), Some(Marker::Tab(Twips(2880))));
        assert_eq!(at(100.0, 0.8), None);
        assert_eq!(at(36.0, 0.2), None);
    }

    #[test]
    fn dragging_changes_indents_and_tabs() {
        let p = paragraph();
        let indents = |left, right, first_line| {
            Some(ParagraphFormat::Indents {
                left: Twips(left),
                right: Twips(right),
                first_line,
            })
        };
        // Первая строка: только её сдвиг.
        assert_eq!(
            drag_result(&p, Marker::FirstLine, 54.0, false, 400.0),
            indents(720, 0, Some(FirstLineIndent::FirstLine(Twips(360))))
        );
        // Выступ: первая строка остаётся на 18 пт.
        assert_eq!(
            drag_result(&p, Marker::Hanging, 72.0, false, 400.0),
            indents(1440, 0, Some(FirstLineIndent::Hanging(Twips(1080))))
        );
        // Отступ слева двигает всё.
        assert_eq!(
            drag_result(&p, Marker::Left, 72.0, false, 400.0),
            indents(1440, 0, Some(FirstLineIndent::Hanging(Twips(360))))
        );
        assert_eq!(
            drag_result(&p, Marker::Right, 360.0, false, 400.0),
            indents(720, 800, Some(FirstLineIndent::Hanging(Twips(360))))
        );
        // Первая строка вровень с отступом — ноль, а не «нет».
        assert_eq!(
            drag_result(&p, Marker::FirstLine, 36.0, false, 400.0),
            indents(720, 0, Some(FirstLineIndent::FirstLine(Twips(0))))
        );
        assert_eq!(drag_result(&p, Marker::Left, 36.0, false, 400.0), None);
        let tab = Marker::Tab(Twips(2880));
        assert_eq!(
            drag_result(&p, tab, 150.0, false, 400.0),
            Some(ParagraphFormat::MoveTab {
                from: Twips(2880),
                to: Twips(3000)
            })
        );
        assert_eq!(drag_result(&p, tab, 144.0, false, 400.0), None);
        assert_eq!(
            drag_result(&p, tab, 150.0, true, 400.0),
            Some(ParagraphFormat::RemoveTab(Twips(2880)))
        );
        assert!((snap(15.0) - POINTS_PER_CM / 2.0).abs() < 1e-3);
    }

    #[test]
    fn clicks_and_drags_in_the_ruler() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;
        // Лист 400 пт без полей, масштаб 1: x экрана = пт + 10.
        let margins = Margins {
            top: 0.0,
            bottom: 0.0,
            left: 0.0,
            right: 0.0,
        };
        type State = (RulerState, Vec<ParagraphFormat>);
        let mut h = Harness::new_ui_state(
            move |ui, state: &mut State| {
                let corner = Rect::from_min_size(pos2(0.0, 0.0), vec2(18.0, 18.0));
                tab_kind_ui(ui, corner, &mut state.0);
                let strip = Rect::from_min_size(pos2(10.0, 30.0), vec2(400.0, 18.0));
                let (edit, _) = horizontal_ui(
                    ui,
                    strip,
                    10.0,
                    400.0,
                    margins,
                    1.0,
                    &paragraph(),
                    &mut state.0,
                );
                state.1.extend(edit);
            },
            (RulerState::default(), Vec::new()),
        );
        h.run();
        h.get_by_label("Табуляция по левому краю").click();
        h.run();
        assert_eq!(h.state().0.tab_kind(), TabStopKind::Center);
        // Щелчок по пустому месту — новая позиция по центру, к делению.
        h.get_by_label("Линейка");
        let x = 10.0 + 7.0 * POINTS_PER_CM;
        h.hover_at(pos2(x + 2.0, 45.0));
        h.run();
        h.event(eframe::egui::Event::PointerButton {
            pos: pos2(x + 2.0, 45.0),
            button: eframe::egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.run();
        h.event(eframe::egui::Event::PointerButton {
            pos: pos2(x + 2.0, 45.0),
            button: eframe::egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.run();
        assert_eq!(
            h.state().1,
            [ParagraphFormat::AddTab(TabStop {
                position: twips(7.0 * POINTS_PER_CM),
                kind: TabStopKind::Center,
                leader: TabLeader::None,
            })]
        );
        // Перетаскивание отступа слева на 2 см.
        h.state_mut().1.clear();
        let start = pos2(10.0 + 36.0, 30.0 + 16.0);
        let end = pos2(10.0 + 2.0 * POINTS_PER_CM, 30.0 + 16.0);
        h.hover_at(start);
        h.run();
        h.drag_at(start);
        h.run();
        h.hover_at(pos2(start.x + 20.0, start.y));
        h.run();
        h.hover_at(end);
        h.run();
        h.drop_at(end);
        h.run();
        assert_eq!(
            h.state().1,
            [ParagraphFormat::Indents {
                left: twips(2.0 * POINTS_PER_CM),
                right: Twips(0),
                first_line: Some(FirstLineIndent::Hanging(Twips(360))),
            }]
        );
    }
}
