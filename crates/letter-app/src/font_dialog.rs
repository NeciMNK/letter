//! Окно «Шрифт»: вкладки «Шрифт» (шрифт, начертание, размер, цвет,
//! подчёркивание, видоизменение) и «Дополнительно» (масштаб, интервал,
//! смещение, кернинг), образец. Как и окно «Абзац», собирает только то,
//! что поменяли.

use crate::i18n::tr;
use eframe::egui::{
    self, Color32, ComboBox, Context, Key, Painter, Rect, ScrollArea, Sense, TextEdit, Ui, vec2,
};
use letter_editor::RunChanges;
use letter_model::VerticalAlign;
use letter_model::{Color, HalfPoints, ResolvedRunProps, RunFonts, RunProps, Twips, Underline};

use crate::home::{FONT_SIZES, STANDARD_COLORS, parse_size, size_label};
use crate::units::{FIELD_SIZE, pt_field};

/// Виды подчёркивания в списке, как в Word.
const UNDERLINES: [(Underline, &str); 18] = [
    (Underline::None, "none-in-parens"),
    (Underline::Single, "underline-single"),
    (Underline::Words, "underline-words"),
    (Underline::Double, "underline-double"),
    (Underline::Thick, "underline-thick"),
    (Underline::Dotted, "underline-dotted"),
    (Underline::DottedHeavy, "underline-dotted-heavy"),
    (Underline::Dash, "underline-dash"),
    (Underline::DashedHeavy, "underline-dash-heavy"),
    (Underline::DashLong, "underline-dash-long"),
    (Underline::DashLongHeavy, "underline-dash-long-heavy"),
    (Underline::DotDash, "underline-dot-dash"),
    (Underline::DashDotHeavy, "underline-dot-dash-heavy"),
    (Underline::DotDotDash, "underline-dot-dot-dash"),
    (Underline::DashDotDotHeavy, "underline-dot-dot-dash-heavy"),
    (Underline::Wave, "underline-wave"),
    (Underline::WavyHeavy, "underline-wave-heavy"),
    (Underline::WavyDouble, "underline-wave-double"),
];

/// Начертания в списке.
const STYLES: [(bool, bool, &str); 4] = [
    (false, false, "font-style-regular"),
    (false, true, "font-style-italic"),
    (true, false, "font-style-bold"),
    (true, true, "font-style-bold-italic"),
];

/// Масштабы в списке, проценты.
const SCALES: [u16; 8] = [200, 150, 100, 90, 80, 66, 50, 33];

/// Значения полей окна.
#[derive(Debug, Clone, PartialEq)]
pub struct FontValues {
    /// Шрифт.
    pub font: String,
    /// Полужирный.
    pub bold: bool,
    /// Курсив.
    pub italic: bool,
    /// Размер, полупункты.
    pub size: HalfPoints,
    /// Цвет текста.
    pub color: Color,
    /// Подчёркивание.
    pub underline: Underline,
    /// Цвет подчёркивания; `None` — «Авто».
    pub underline_color: Option<Color>,
    /// Зачёркнутый.
    pub strike: bool,
    /// Двойное зачёркивание.
    pub double_strike: bool,
    /// Надстрочный.
    pub superscript: bool,
    /// Подстрочный.
    pub subscript: bool,
    /// Малые прописные.
    pub small_caps: bool,
    /// Все прописные.
    pub caps: bool,
    /// Скрытый.
    pub hidden: bool,
    /// Масштаб, проценты.
    pub scale: u16,
    /// Интервал между буквами, твипы (меньше нуля — уплотнённый).
    pub spacing: i32,
    /// Смещение, полупункты (меньше нуля — вниз).
    pub position: i32,
    /// Кернинг от этого размера, полупункты; ноль — выключен.
    pub kerning: u32,
}

impl FontValues {
    /// Значения текста у курсора.
    pub fn new(props: &ResolvedRunProps) -> FontValues {
        FontValues {
            font: props.fonts.ascii.clone(),
            bold: props.bold,
            italic: props.italic,
            size: props.size,
            color: props.color,
            underline: props.underline,
            underline_color: props.underline_color,
            strike: props.strike,
            double_strike: props.double_strike,
            superscript: props.vertical_align == VerticalAlign::Superscript,
            subscript: props.vertical_align == VerticalAlign::Subscript,
            small_caps: props.small_caps,
            caps: props.caps,
            hidden: props.hidden,
            scale: props.scale,
            spacing: props.spacing.0,
            position: props.position,
            kerning: props.kerning.0,
        }
    }

    fn vertical_align(&self) -> VerticalAlign {
        if self.superscript {
            VerticalAlign::Superscript
        } else if self.subscript {
            VerticalAlign::Subscript
        } else {
            VerticalAlign::Baseline
        }
    }

    /// Что изменилось по сравнению с `initial`.
    pub fn changes(&self, initial: &FontValues) -> RunChanges {
        fn changed<T: PartialEq>(new: T, old: T) -> Option<T> {
            (new != old).then_some(new)
        }
        let font = self.font.trim();
        RunChanges {
            font: (!font.is_empty() && font != initial.font).then(|| font.to_owned()),
            bold: changed(self.bold, initial.bold),
            italic: changed(self.italic, initial.italic),
            size: changed(self.size, initial.size),
            color: changed(self.color, initial.color),
            underline: changed(self.underline, initial.underline),
            underline_color: changed(self.underline_color, initial.underline_color),
            strike: changed(self.strike, initial.strike),
            double_strike: changed(self.double_strike, initial.double_strike),
            vertical_align: changed(self.vertical_align(), initial.vertical_align()),
            small_caps: changed(self.small_caps, initial.small_caps),
            caps: changed(self.caps, initial.caps),
            hidden: changed(self.hidden, initial.hidden),
            scale: changed(self.scale, initial.scale),
            spacing: changed(Twips(self.spacing), Twips(initial.spacing)),
            position: changed(self.position, initial.position),
            kerning: changed(HalfPoints(self.kerning), HalfPoints(initial.kerning)),
        }
    }

    /// Оформление для образца.
    pub fn sample_props(&self) -> RunProps {
        RunProps {
            fonts: RunFonts::new(self.font.trim()),
            size: Some(self.size),
            bold: Some(self.bold),
            italic: Some(self.italic),
            underline: Some(self.underline),
            underline_color: self.underline_color,
            strike: Some(self.strike),
            double_strike: Some(self.double_strike),
            color: Some(self.color),
            vertical_align: Some(self.vertical_align()),
            caps: Some(self.caps),
            small_caps: Some(self.small_caps),
            scale: Some(self.scale),
            spacing: Some(Twips(self.spacing)),
            position: Some(self.position),
            kerning: Some(HalfPoints(self.kerning)),
            ..RunProps::default()
        }
    }
}

/// Окно «Шрифт».
#[derive(Debug, Clone, Default)]
pub struct FontDialog {
    /// Окно открыто.
    pub open: bool,
    /// Открытая вкладка: 0 — «Шрифт», 1 — «Дополнительно».
    pub tab: usize,
    initial: Option<FontValues>,
    /// Значения в полях.
    pub values: Option<FontValues>,
    /// Текст в поле размера, пока его набирают.
    size_text: Option<String>,
}

impl FontDialog {
    /// Открывает окно со значениями текста у курсора.
    pub fn show(&mut self, values: FontValues) {
        self.open = true;
        self.initial = Some(values.clone());
        self.values = Some(values);
        self.size_text = None;
    }

    /// Рисует окно. `fonts` — шрифты для списка; `sample` рисует образец
    /// текста с оформлением в прямоугольнике. По «ОК» возвращает
    /// изменения (пустые не возвращаются).
    pub fn ui(
        &mut self,
        ctx: &Context,
        fonts: &[String],
        mut sample: impl FnMut(&Painter, Rect, &RunProps),
    ) -> Option<RunChanges> {
        if !self.open {
            return None;
        }
        let (Some(initial), Some(mut values)) = (self.initial.clone(), self.values.take()) else {
            self.open = false;
            return None;
        };
        let mut open = true;
        let mut result = None;
        let mut close = false;
        egui::Window::new(tr("font"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, 0, tr("font"));
                    ui.selectable_value(&mut self.tab, 1, tr("font-advanced"));
                });
                ui.separator();
                if self.tab == 0 {
                    font_tab(ui, &mut values, fonts, &mut self.size_text);
                } else {
                    advanced_tab(ui, &mut values);
                }
                ui.separator();
                ui.label(tr("preview"));
                let (rect, _) = ui.allocate_exact_size(vec2(420.0, 64.0), Sense::hover());
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 0.0, Color32::WHITE);
                sample(&painter, rect, &values.sample_props());
                painter.rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(1.0, Color32::from_gray(180)),
                    egui::StrokeKind::Inside,
                );
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

/// Вкладка «Шрифт».
fn font_tab(ui: &mut Ui, v: &mut FontValues, fonts: &[String], size_text: &mut Option<String>) {
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            let label = ui.label(tr("font-name"));
            ui.add(TextEdit::singleline(&mut v.font).desired_width(190.0))
                .labelled_by(label.id);
            list(ui, "font-list", 190.0, |ui| {
                for name in fonts {
                    let selected = name.eq_ignore_ascii_case(v.font.trim());
                    let response = ui.selectable_label(selected, name);
                    if selected && response.gained_focus() {
                        response.scroll_to_me(None);
                    }
                    if response.clicked() {
                        v.font = name.clone();
                    }
                }
            });
        });
        ui.vertical(|ui| {
            ui.label(tr("font-style"));
            list(ui, "style-list", 140.0, |ui| {
                for (bold, italic, name) in STYLES {
                    let selected = v.bold == bold && v.italic == italic;
                    if ui.selectable_label(selected, tr(name)).clicked() {
                        v.bold = bold;
                        v.italic = italic;
                    }
                }
            });
        });
        ui.vertical(|ui| {
            let label = ui.label(tr("font-size"));
            let text = size_text.get_or_insert_with(|| size_label(v.size.0));
            let field = ui
                .add(TextEdit::singleline(text).desired_width(60.0))
                .labelled_by(label.id);
            if field.changed()
                && let Some(size) = parse_size(text)
            {
                v.size = size;
            }
            if field.lost_focus() {
                *size_text = None;
            }
            list(ui, "size-list", 60.0, |ui| {
                for points in FONT_SIZES {
                    let half = HalfPoints((points * 2.0) as u32);
                    if ui
                        .selectable_label(v.size == half, size_label(half.0))
                        .clicked()
                    {
                        v.size = half;
                        *size_text = None;
                    }
                }
            });
        });
    });
    ui.add_space(6.0);
    egui::Grid::new("font-colors")
        .num_columns(3)
        .spacing(vec2(12.0, 4.0))
        .show(ui, |ui| {
            ui.label(tr("font-color-label"));
            let underline_label = ui.label(tr("font-underline"));
            ui.label(tr("font-underline-color-label"));
            ui.end_row();
            color_combo(ui, "font-color", &mut v.color, tr("font-color"));
            let name = UNDERLINES
                .iter()
                .find(|(u, _)| *u == v.underline)
                .map_or("", |(_, n)| tr(n));
            ComboBox::from_id_salt("font-underline")
                .selected_text(name)
                .width(150.0)
                .show_ui(ui, |ui| {
                    for (underline, name) in UNDERLINES {
                        ui.selectable_value(&mut v.underline, underline, tr(name));
                    }
                })
                .response
                .labelled_by(underline_label.id);
            ui.add_enabled_ui(v.underline != Underline::None, |ui| {
                let mut color = v.underline_color.unwrap_or(Color::Auto);
                if color_combo(
                    ui,
                    "font-underline-color",
                    &mut color,
                    tr("font-underline-color"),
                ) {
                    v.underline_color = (color != Color::Auto).then_some(color);
                }
            });
            ui.end_row();
        });
    ui.add_space(6.0);
    ui.strong(tr("font-effects"));
    egui::Grid::new("font-effects")
        .num_columns(2)
        .spacing(vec2(24.0, 2.0))
        .show(ui, |ui| {
            // Взаимоисключающие пары, как в Word.
            if ui.checkbox(&mut v.strike, tr("strikethrough")).changed() && v.strike {
                v.double_strike = false;
            }
            if ui
                .checkbox(&mut v.small_caps, tr("font-small-caps"))
                .changed()
                && v.small_caps
            {
                v.caps = false;
            }
            ui.end_row();
            if ui
                .checkbox(&mut v.double_strike, tr("font-double-strike"))
                .changed()
                && v.double_strike
            {
                v.strike = false;
            }
            if ui.checkbox(&mut v.caps, tr("font-all-caps")).changed() && v.caps {
                v.small_caps = false;
            }
            ui.end_row();
            if ui
                .checkbox(&mut v.superscript, tr("font-superscript"))
                .changed()
                && v.superscript
            {
                v.subscript = false;
            }
            ui.checkbox(&mut v.hidden, tr("font-hidden"));
            ui.end_row();
            if ui
                .checkbox(&mut v.subscript, tr("font-subscript"))
                .changed()
                && v.subscript
            {
                v.superscript = false;
            }
            ui.end_row();
        });
}

/// Список с прокруткой под полем.
fn list(ui: &mut Ui, id: &str, width: f32, content: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .stroke(egui::Stroke::new(
            1.0,
            crate::theme::Palette::of(ui.ctx()).border,
        ))
        .show(ui, |ui| {
            ui.set_width(width);
            ScrollArea::vertical()
                .id_salt(id)
                .max_height(110.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), content);
                });
        });
}

/// Список цветов: «Авто» и стандартные цвета Word. Возвращает, выбран ли
/// цвет.
fn color_combo(ui: &mut Ui, id: &str, color: &mut Color, name: &str) -> bool {
    let label = |c: Color| match c {
        Color::Auto => tr("color-auto").to_owned(),
        Color::Rgb(r, g, b) => STANDARD_COLORS
            .iter()
            .find(|(_, rgb)| *rgb == (r, g, b))
            .map_or_else(
                || format!("#{r:02X}{g:02X}{b:02X}"),
                |(n, _)| tr(n).to_owned(),
            ),
    };
    let before = *color;
    let response = ComboBox::from_id_salt(id)
        .selected_text(label(*color))
        .width(130.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(color, Color::Auto, tr("color-auto"));
            for (name, (r, g, b)) in STANDARD_COLORS {
                let value = Color::Rgb(r, g, b);
                ui.horizontal(|ui| {
                    let (swatch, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                    ui.painter()
                        .rect_filled(swatch, 0.0, Color32::from_rgb(r, g, b));
                    ui.selectable_value(color, value, tr(name));
                });
            }
        })
        .response;
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, name));
    *color != before
}

/// Вкладка «Дополнительно».
fn advanced_tab(ui: &mut Ui, v: &mut FontValues) {
    ui.strong(tr("font-character-spacing"));
    egui::Grid::new("font-advanced")
        .num_columns(4)
        .spacing(vec2(8.0, 6.0))
        .show(ui, |ui| {
            let label = ui.label(tr("font-scale"));
            let mut scale = f64::from(v.scale);
            let field = ui
                .add_sized(
                    FIELD_SIZE,
                    egui::DragValue::new(&mut scale)
                        .range(1.0..=600.0)
                        .speed(1.0)
                        .suffix(" %"),
                )
                .labelled_by(label.id);
            if field.changed() {
                v.scale = scale.round() as u16;
            }
            ComboBox::from_id_salt("font-scales")
                .selected_text("")
                .width(20.0)
                .show_ui(ui, |ui| {
                    for s in SCALES {
                        ui.selectable_value(&mut v.scale, s, format!("{s} %"));
                    }
                })
                .response
                .widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::ComboBox, true, tr("font-scales"))
                });
            ui.end_row();

            let label = ui.label(tr("font-spacing"));
            let names = [
                tr("font-spacing-normal"),
                tr("font-spacing-expanded"),
                tr("font-spacing-condensed"),
            ];
            let mut kind = match v.spacing {
                0 => 0,
                s if s > 0 => 1,
                _ => 2,
            };
            let before = kind;
            ComboBox::from_id_salt("font-spacing")
                .selected_text(names[kind])
                .width(110.0)
                .show_ui(ui, |ui| {
                    for (i, name) in names.into_iter().enumerate() {
                        ui.selectable_value(&mut kind, i, name);
                    }
                })
                .response
                .labelled_by(label.id);
            if kind != before {
                // Word предлагает 1 пт.
                v.spacing = match kind {
                    0 => 0,
                    1 => v.spacing.abs().max(20),
                    _ => -v.spacing.abs().max(20),
                };
            }
            let label = ui.label(tr("by"));
            let mut amount = v.spacing.abs();
            ui.add_enabled_ui(kind != 0, |ui| {
                if pt_field(ui, label.id, &mut amount, 0.0, 1584.0, 0.1).changed() {
                    v.spacing = if kind == 2 { -amount } else { amount };
                }
            });
            ui.end_row();

            let label = ui.label(tr("font-position"));
            let names = [
                tr("font-position-normal"),
                tr("font-position-raised"),
                tr("font-position-lowered"),
            ];
            let mut kind = match v.position {
                0 => 0,
                p if p > 0 => 1,
                _ => 2,
            };
            let before = kind;
            ComboBox::from_id_salt("font-position")
                .selected_text(names[kind])
                .width(110.0)
                .show_ui(ui, |ui| {
                    for (i, name) in names.into_iter().enumerate() {
                        ui.selectable_value(&mut kind, i, name);
                    }
                })
                .response
                .labelled_by(label.id);
            if kind != before {
                // Word предлагает 3 пт.
                v.position = match kind {
                    0 => 0,
                    1 => v.position.abs().max(6),
                    _ => -v.position.abs().max(6),
                };
            }
            let label = ui.label(tr("by"));
            // Полупункты ↔ твипы для поля в пунктах.
            let mut amount = v.position.abs() * 10;
            ui.add_enabled_ui(kind != 0, |ui| {
                if pt_field(ui, label.id, &mut amount, 0.0, 1584.0, 0.5).changed() {
                    let half = (amount as f32 / 10.0).round() as i32;
                    v.position = if kind == 2 { -half } else { half };
                }
            });
            ui.end_row();
        });
    ui.horizontal(|ui| {
        let mut kerned = v.kerning > 0;
        if ui.checkbox(&mut kerned, tr("font-kerning")).changed() {
            // Word предлагает размер текста.
            v.kerning = if kerned { v.size.0.max(2) } else { 0 };
        }
        ui.add_enabled_ui(kerned, |ui| {
            let label = ui.label(tr("font-kerning-from"));
            // Выключенный кернинг показывает размер текста.
            let shown = if kerned { v.kerning } else { v.size.0 };
            let mut twips = (shown * 10) as i32;
            if pt_field(ui, label.id, &mut twips, 1.0, 1638.0, 0.5).changed() && kerned {
                v.kerning = ((twips as f32 / 10.0).round() as u32).max(2);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn resolved() -> ResolvedRunProps {
        let doc = letter_model::Document::new();
        doc.resolve_run(
            &letter_model::ParagraphProps::default(),
            &RunProps::default(),
        )
    }

    #[test]
    fn values_and_changes() {
        let initial = FontValues::new(&resolved());
        assert!(initial.changes(&initial).is_empty());
        let mut edited = initial.clone();
        edited.font = " Arial ".into();
        edited.superscript = true;
        edited.underline = Underline::Double;
        edited.underline_color = Some(Color::Rgb(255, 0, 0));
        edited.spacing = -30;
        let changes = edited.changes(&initial);
        assert_eq!(changes.font.as_deref(), Some("Arial"));
        assert_eq!(changes.vertical_align, Some(VerticalAlign::Superscript));
        assert_eq!(changes.underline, Some(Underline::Double));
        assert_eq!(changes.underline_color, Some(Some(Color::Rgb(255, 0, 0))));
        assert_eq!(changes.spacing, Some(Twips(-30)));
        assert_eq!(changes.bold, None);
        let sample = edited.sample_props();
        assert_eq!(sample.fonts.ascii.as_deref(), Some("Arial"));
        assert_eq!(sample.spacing, Some(Twips(-30)));
    }

    #[test]
    fn dialog_gives_changes_on_ok() {
        let mut dialog = FontDialog::default();
        dialog.show(FontValues::new(&resolved()));
        let fonts: Vec<String> = vec!["Arial".into(), "Times New Roman".into()];
        let mut h = Harness::new_ui_state(
            move |ui, state: &mut (FontDialog, Vec<RunChanges>, usize)| {
                let mut drawn = 0;
                if let Some(changes) = state.0.ui(ui.ctx(), &fonts, |_, _, _| drawn += 1) {
                    state.1.push(changes);
                }
                state.2 += drawn;
            },
            (dialog, Vec::new(), 0),
        );
        h.run();
        assert!(h.state().2 > 0, "образец рисуется");
        h.get_by_label("Arial").click();
        h.run();
        h.get_by_label("Полужирный курсив").click();
        h.run();
        h.get_by_label("Все прописные").click();
        h.run();
        h.get_by_label("Дополнительно").click();
        h.run();
        h.get_by_label("Кернинг для знаков размером").click();
        h.run();
        h.get_by_label("ОК").click();
        h.run();
        let size = resolved().size;
        assert_eq!(
            h.state().1,
            [RunChanges {
                font: Some("Arial".into()),
                bold: Some(true),
                italic: Some(true),
                caps: Some(true),
                kerning: Some(size),
                ..RunChanges::default()
            }]
        );
        assert!(!h.state().0.open);
    }
}
