//! Окно «Абзац»: вкладки «Отступы и интервалы» и «Положение на
//! странице», образец. Окно собирает только то, что поменяли: у
//! нескольких выделенных абзацев остальные значения остаются своими.

use crate::i18n::tr;
use eframe::egui::{self, Color32, ComboBox, Context, Key, Rect, Sense, Stroke, Ui, pos2, vec2};
use letter_editor::ParagraphChanges;
use letter_model::{Alignment, FirstLineIndent, LineSpacing, ResolvedParagraphProps, Twips};

use crate::units::{TWIPS_PER_CM, cm_field, format_number, pt_field};

/// Первая строка.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstLine {
    /// «(нет)».
    None,
    /// «Отступ».
    Indent,
    /// «Выступ».
    Hanging,
}

/// Вид межстрочного интервала, как в списке Word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// «Одинарный».
    Single,
    /// «1,5 строки».
    OneAndHalf,
    /// «Двойной».
    Double,
    /// «Минимум».
    AtLeast,
    /// «Точно».
    Exact,
    /// «Множитель».
    Multiple,
}

impl LineKind {
    const ALL: [LineKind; 6] = [
        LineKind::Single,
        LineKind::OneAndHalf,
        LineKind::Double,
        LineKind::AtLeast,
        LineKind::Exact,
        LineKind::Multiple,
    ];

    fn name(self) -> &'static str {
        match self {
            LineKind::Single => tr("line-single"),
            LineKind::OneAndHalf => tr("line-one-half"),
            LineKind::Double => tr("line-double"),
            LineKind::AtLeast => tr("line-at-least"),
            LineKind::Exact => tr("line-exactly"),
            LineKind::Multiple => tr("line-multiple"),
        }
    }
}

const ALIGNMENTS: [(Alignment, &str); 5] = [
    (Alignment::Left, "align-left"),
    (Alignment::Center, "align-center"),
    (Alignment::Right, "align-right"),
    (Alignment::Justify, "align-justify"),
    (Alignment::Distribute, "align-distribute"),
];

/// Значения полей окна.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParagraphValues {
    /// Выравнивание.
    pub alignment: Alignment,
    /// Отступ слева, твипы.
    pub left: i32,
    /// Отступ справа, твипы.
    pub right: i32,
    /// Первая строка.
    pub first_line: FirstLine,
    /// «на»: величина отступа или выступа первой строки, твипы.
    pub first_by: i32,
    /// Интервал перед, твипы.
    pub before: i32,
    /// Интервал после, твипы.
    pub after: i32,
    /// Межстрочный интервал.
    pub line: LineKind,
    /// «Значение»: у «Множителя» — во сколько раз (1,15), у «Минимум»
    /// и «Точно» — твипы.
    pub line_value: f64,
    /// «Не добавлять интервал между абзацами одного стиля».
    pub contextual_spacing: bool,
    /// «Запрет висячих строк».
    pub widow_control: bool,
    /// «Не отрывать от следующего».
    pub keep_next: bool,
    /// «Не разрывать абзац».
    pub keep_lines: bool,
    /// «С новой страницы».
    pub page_break_before: bool,
    /// «Запретить автоматический перенос слов».
    pub suppress_auto_hyphens: bool,
}

impl ParagraphValues {
    /// Значения абзаца.
    pub fn new(props: &ResolvedParagraphProps) -> ParagraphValues {
        let (first_line, first_by) = match props.first_line {
            Some(FirstLineIndent::FirstLine(t)) if t.0 > 0 => (FirstLine::Indent, t.0),
            Some(FirstLineIndent::Hanging(t)) if t.0 > 0 => (FirstLine::Hanging, t.0),
            _ => (FirstLine::None, 0),
        };
        let (line, line_value) = match props.line_spacing {
            LineSpacing::Multiple(240) => (LineKind::Single, 1.0),
            LineSpacing::Multiple(360) => (LineKind::OneAndHalf, 1.5),
            LineSpacing::Multiple(480) => (LineKind::Double, 2.0),
            LineSpacing::Multiple(m) => (LineKind::Multiple, f64::from(m) / 240.0),
            LineSpacing::AtLeast(t) => (LineKind::AtLeast, f64::from(t.0)),
            LineSpacing::Exact(t) => (LineKind::Exact, f64::from(t.0)),
        };
        ParagraphValues {
            alignment: props.alignment,
            left: props.indent_left.0,
            right: props.indent_right.0,
            first_line,
            first_by,
            before: props.space_before.0,
            after: props.space_after.0,
            line,
            line_value,
            contextual_spacing: props.contextual_spacing,
            widow_control: props.widow_control,
            keep_next: props.keep_next,
            keep_lines: props.keep_lines,
            page_break_before: props.page_break_before,
            suppress_auto_hyphens: props.suppress_auto_hyphens,
        }
    }

    /// Первая строка для модели.
    pub fn first_line_indent(&self) -> Option<FirstLineIndent> {
        match self.first_line {
            FirstLine::None => None,
            FirstLine::Indent => Some(FirstLineIndent::FirstLine(Twips(self.first_by))),
            FirstLine::Hanging => Some(FirstLineIndent::Hanging(Twips(self.first_by))),
        }
    }

    /// Межстрочный интервал для модели.
    pub fn line_spacing(&self) -> LineSpacing {
        match self.line {
            LineKind::Single => LineSpacing::SINGLE,
            LineKind::OneAndHalf => LineSpacing::ONE_AND_HALF,
            LineKind::Double => LineSpacing::DOUBLE,
            LineKind::Multiple => LineSpacing::multiple(self.line_value),
            LineKind::AtLeast => LineSpacing::AtLeast(Twips(self.line_value.round() as i32)),
            LineKind::Exact => LineSpacing::Exact(Twips(self.line_value.round() as i32)),
        }
    }

    /// Что изменилось по сравнению с `initial`.
    pub fn changes(&self, initial: &ParagraphValues) -> ParagraphChanges {
        fn changed<T: PartialEq>(new: T, old: T) -> Option<T> {
            (new != old).then_some(new)
        }
        ParagraphChanges {
            alignment: changed(self.alignment, initial.alignment),
            left: changed(Twips(self.left), Twips(initial.left)),
            right: changed(Twips(self.right), Twips(initial.right)),
            first_line: changed(self.first_line_indent(), initial.first_line_indent()),
            before: changed(Twips(self.before), Twips(initial.before)),
            after: changed(Twips(self.after), Twips(initial.after)),
            line: changed(self.line_spacing(), initial.line_spacing()),
            contextual_spacing: changed(self.contextual_spacing, initial.contextual_spacing),
            widow_control: changed(self.widow_control, initial.widow_control),
            keep_next: changed(self.keep_next, initial.keep_next),
            keep_lines: changed(self.keep_lines, initial.keep_lines),
            page_break_before: changed(self.page_break_before, initial.page_break_before),
            suppress_auto_hyphens: changed(
                self.suppress_auto_hyphens,
                initial.suppress_auto_hyphens,
            ),
        }
    }
}

/// Окно «Абзац».
#[derive(Debug, Clone, Default)]
pub struct ParagraphDialog {
    /// Окно открыто.
    pub open: bool,
    /// Открытая вкладка: 0 — «Отступы и интервалы», 1 — «Положение
    /// на странице».
    pub tab: usize,
    /// Значения при открытии.
    initial: Option<ParagraphValues>,
    /// Значения в полях.
    pub values: Option<ParagraphValues>,
}

impl ParagraphDialog {
    /// Открывает окно со значениями абзаца у курсора.
    pub fn show(&mut self, values: ParagraphValues) {
        self.open = true;
        self.initial = Some(values);
        self.values = Some(values);
    }

    /// Рисует окно. По «ОК» возвращает изменения (пустые не
    /// возвращаются).
    pub fn ui(&mut self, ctx: &Context) -> Option<ParagraphChanges> {
        if !self.open {
            return None;
        }
        let (Some(initial), Some(mut values)) = (self.initial, self.values) else {
            self.open = false;
            return None;
        };
        let mut open = true;
        let mut result = None;
        let mut close = false;
        egui::Window::new(tr("paragraph"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, 0, tr("paragraph-indents-tab"));
                    ui.selectable_value(&mut self.tab, 1, tr("paragraph-breaks-tab"));
                });
                ui.separator();
                if self.tab == 0 {
                    indents_tab(ui, &mut values);
                } else {
                    pagination_tab(ui, &mut values);
                }
                ui.separator();
                ui.label(tr("preview"));
                paint_sample(ui, &values);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(tr("ok")).clicked() {
                        let changes = values.changes(&initial);
                        result = (!changes.is_empty()).then_some(changes);
                        close = true;
                    }
                    if ui.button(tr("cancel")).clicked() {
                        close = true;
                    }
                });
            });
        self.values = Some(values);
        if !open || close || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.open = false;
        }
        result
    }
}

/// Вкладка «Отступы и интервалы».
fn indents_tab(ui: &mut Ui, v: &mut ParagraphValues) {
    ui.strong(tr("paragraph-general"));
    egui::Grid::new("paragraph-general")
        .num_columns(2)
        .spacing(vec2(8.0, 6.0))
        .show(ui, |ui| {
            let label = ui.label(tr("paragraph-alignment"));
            let name = ALIGNMENTS
                .iter()
                .find(|(a, _)| *a == v.alignment)
                .map_or("", |(_, n)| tr(n));
            ComboBox::from_id_salt("paragraph-alignment")
                .selected_text(name)
                .width(160.0)
                .show_ui(ui, |ui| {
                    for (alignment, name) in ALIGNMENTS {
                        ui.selectable_value(&mut v.alignment, alignment, tr(name));
                    }
                })
                .response
                .labelled_by(label.id);
            ui.end_row();
        });
    ui.add_space(4.0);
    ui.strong(tr("paragraph-indentation"));
    egui::Grid::new("paragraph-indents")
        .num_columns(4)
        .spacing(vec2(8.0, 6.0))
        .show(ui, |ui| {
            let label = ui.label(tr("paragraph-left"));
            cm_field(ui, label.id, &mut v.left, -55.87, 55.87);
            let label = ui.label(tr("paragraph-special"));
            let name = |f| match f {
                FirstLine::None => tr("none-in-parens"),
                FirstLine::Indent => tr("paragraph-first-line"),
                FirstLine::Hanging => tr("paragraph-hanging"),
            };
            let before = v.first_line;
            ComboBox::from_id_salt("paragraph-first-line")
                .selected_text(name(v.first_line))
                .width(100.0)
                .show_ui(ui, |ui| {
                    for f in [FirstLine::None, FirstLine::Indent, FirstLine::Hanging] {
                        ui.selectable_value(&mut v.first_line, f, name(f));
                    }
                })
                .response
                .labelled_by(label.id);
            if before != v.first_line {
                // Word ставит 1,25 см, когда выбирают отступ или выступ.
                v.first_by = match v.first_line {
                    FirstLine::None => 0,
                    _ if v.first_by > 0 => v.first_by,
                    _ => (1.25 * TWIPS_PER_CM).round() as i32,
                };
            }
            ui.end_row();
            let label = ui.label(tr("paragraph-right"));
            cm_field(ui, label.id, &mut v.right, -55.87, 55.87);
            let label = ui.label(tr("by"));
            ui.add_enabled_ui(v.first_line != FirstLine::None, |ui| {
                cm_field(ui, label.id, &mut v.first_by, 0.0, 55.87);
            });
            ui.end_row();
        });
    ui.add_space(4.0);
    ui.strong(tr("paragraph-spacing"));
    egui::Grid::new("paragraph-spacing")
        .num_columns(4)
        .spacing(vec2(8.0, 6.0))
        .show(ui, |ui| {
            let label = ui.label(tr("paragraph-before"));
            pt_field(ui, label.id, &mut v.before, 0.0, 1584.0, 1.0);
            let label = ui.label(tr("paragraph-line-spacing"));
            let before = v.line;
            ComboBox::from_id_salt("paragraph-line")
                .selected_text(v.line.name())
                .width(110.0)
                .show_ui(ui, |ui| {
                    for kind in LineKind::ALL {
                        ui.selectable_value(&mut v.line, kind, kind.name());
                    }
                })
                .response
                .labelled_by(label.id);
            if before != v.line {
                v.line_value = match v.line {
                    LineKind::Single => 1.0,
                    LineKind::OneAndHalf => 1.5,
                    LineKind::Double => 2.0,
                    LineKind::Multiple => 3.0,
                    LineKind::AtLeast | LineKind::Exact => 240.0,
                };
            }
            ui.end_row();
            let label = ui.label(tr("paragraph-after"));
            pt_field(ui, label.id, &mut v.after, 0.0, 1584.0, 1.0);
            let label = ui.label(tr("paragraph-at"));
            match v.line {
                LineKind::Multiple => {
                    ui.add_sized(
                        crate::units::FIELD_SIZE,
                        egui::DragValue::new(&mut v.line_value)
                            .speed(0.05)
                            .range(0.06..=132.0)
                            .custom_formatter(|n, _| format_number(n, 2))
                            .custom_parser(crate::units::parse_number),
                    )
                    .labelled_by(label.id);
                }
                LineKind::AtLeast | LineKind::Exact => {
                    let mut twips = v.line_value.round() as i32;
                    if pt_field(ui, label.id, &mut twips, 0.7, 1584.0, 0.5).changed() {
                        v.line_value = f64::from(twips);
                    }
                }
                _ => {
                    ui.add_enabled_ui(false, |ui| {
                        let mut shown = String::new();
                        ui.add_sized(
                            crate::units::FIELD_SIZE,
                            egui::TextEdit::singleline(&mut shown),
                        )
                        .labelled_by(label.id);
                    });
                }
            }
            ui.end_row();
        });
    ui.checkbox(&mut v.contextual_spacing, tr("paragraph-contextual"));
}

/// Вкладка «Положение на странице».
fn pagination_tab(ui: &mut Ui, v: &mut ParagraphValues) {
    ui.strong(tr("paragraph-pagination"));
    ui.checkbox(&mut v.widow_control, tr("paragraph-widow"));
    ui.checkbox(&mut v.keep_next, tr("paragraph-keep-next"));
    ui.checkbox(&mut v.keep_lines, tr("paragraph-keep-lines"));
    ui.checkbox(&mut v.page_break_before, tr("paragraph-page-break"));
    ui.separator();
    ui.checkbox(&mut v.suppress_auto_hyphens, tr("paragraph-no-hyphens"));
}

/// Образец: соседние абзацы серыми полосками, этот — тёмными, с его
/// отступами, выравниванием и интервалами.
fn paint_sample(ui: &mut Ui, v: &ParagraphValues) {
    let (rect, _) = ui.allocate_exact_size(vec2(360.0, 120.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect(
        rect,
        0.0,
        Color32::WHITE,
        Stroke::new(1.0, Color32::from_gray(180)),
        egui::StrokeKind::Inside,
    );
    // Ширина текста образца — 16 см.
    let inner = rect.shrink2(vec2(12.0, 8.0));
    let per_twip = inner.width() / (16.0 * TWIPS_PER_CM as f32);
    let x = |twips: i32| (twips as f32 * per_twip).clamp(-inner.width() / 2.0, inner.width());
    let line_height = 4.0;
    let gap = match v.line_spacing() {
        LineSpacing::Multiple(m) => 3.0 * m as f32 / 240.0,
        LineSpacing::AtLeast(t) | LineSpacing::Exact(t) => (t.0 as f32 / 240.0 * 3.0).max(0.5),
    };
    let mut y = inner.top();
    let gray = Color32::from_gray(200);
    for _ in 0..2 {
        painter.rect_filled(
            Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), line_height)),
            0.0,
            gray,
        );
        y += line_height + 3.0;
    }
    y += (v.before as f32 / 20.0 * 0.4).min(30.0);
    let left = inner.left() + x(v.left);
    let right = inner.right() - x(v.right);
    let first = match v.first_line {
        FirstLine::None => 0.0,
        FirstLine::Indent => x(v.first_by),
        FirstLine::Hanging => -x(v.first_by),
    };
    let lengths = [1.0, 1.0, 1.0, 0.55];
    for (i, share) in lengths.into_iter().enumerate() {
        let start = if i == 0 { left + first } else { left };
        let full = (right - start).max(4.0);
        let last = i == lengths.len() - 1;
        let width = match v.alignment {
            Alignment::Justify if !last => full,
            Alignment::Distribute => full,
            _ => full * share * if last { 1.0 } else { 0.95 },
        };
        let x0 = match v.alignment {
            Alignment::Center => start + (full - width) / 2.0,
            Alignment::Right => start + full - width,
            _ => start,
        };
        painter.rect_filled(
            Rect::from_min_size(pos2(x0, y), vec2(width, line_height)),
            0.0,
            Color32::from_gray(80),
        );
        y += line_height + gap;
    }
    y += (v.after as f32 / 20.0 * 0.4).min(30.0);
    while y + line_height < inner.bottom() {
        painter.rect_filled(
            Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), line_height)),
            0.0,
            gray,
        );
        y += line_height + 3.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn resolved() -> ResolvedParagraphProps {
        letter_model::Document::new().resolve_paragraph(&letter_model::ParagraphProps::default())
    }

    #[test]
    fn values_round_trip_and_changes() {
        let mut props = resolved();
        props.first_line = Some(FirstLineIndent::Hanging(Twips(360)));
        props.line_spacing = LineSpacing::Multiple(276);
        let initial = ParagraphValues::new(&props);
        assert_eq!(initial.first_line, FirstLine::Hanging);
        assert_eq!(initial.line, LineKind::Multiple);
        assert!((initial.line_value - 1.15).abs() < 1e-9);
        assert!(initial.changes(&initial).is_empty());
        let mut edited = initial;
        edited.line = LineKind::Exact;
        edited.line_value = 300.0;
        edited.first_line = FirstLine::None;
        edited.keep_lines = true;
        let changes = edited.changes(&initial);
        assert_eq!(changes.line, Some(LineSpacing::Exact(Twips(300))));
        assert_eq!(changes.first_line, Some(None));
        assert_eq!(changes.keep_lines, Some(true));
        assert_eq!(changes.left, None);
    }

    #[test]
    fn dialog_gives_changes_on_ok() {
        let mut dialog = ParagraphDialog::default();
        dialog.show(ParagraphValues::new(&resolved()));
        let mut h = Harness::new_ui_state(
            |ui, state: &mut (ParagraphDialog, Vec<ParagraphChanges>)| {
                if let Some(changes) = state.0.ui(ui.ctx()) {
                    state.1.push(changes);
                }
            },
            (dialog, Vec::new()),
        );
        h.run();
        // Без изменений «ОК» ничего не даёт.
        h.get_by_label("ОК").click();
        h.run();
        assert!(h.state().1.is_empty() && !h.state().0.open);
        h.state_mut().0.show(ParagraphValues::new(&resolved()));
        h.run();
        h.get_by_label("Положение на странице").click();
        h.run();
        h.get_by_label("Не отрывать от следующего").click();
        h.run();
        h.get_by_label("ОК").click();
        h.run();
        assert_eq!(
            h.state().1,
            [ParagraphChanges {
                keep_next: Some(true),
                ..ParagraphChanges::default()
            }]
        );
        // «Отмена» и Esc закрывают без изменений.
        h.state_mut().0.show(ParagraphValues::new(&resolved()));
        h.run();
        h.get_by_label("Отмена").click();
        h.run();
        assert!(!h.state().0.open);
        assert_eq!(h.state().1.len(), 1);
    }
}
