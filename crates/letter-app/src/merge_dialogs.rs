//! Окна рассылок: «Изменить список получателей» (отметки, сортировка,
//! фильтр) и правила «Если… то… иначе», «Пропустить запись, если…».
//! Окна только собирают ответ; документ меняет просмотрщик.

use eframe::egui::{self, Context, Key, RichText, TextEdit, Ui};
use letter_editor::{Compare, MergeData, MergeRule};

use crate::i18n::tr;

/// Название сравнения на языке интерфейса.
pub fn compare_name(compare: Compare) -> &'static str {
    tr(match compare {
        Compare::Equal => "compare-equal",
        Compare::NotEqual => "compare-not-equal",
        Compare::Less => "compare-less",
        Compare::Greater => "compare-greater",
        Compare::LessOrEqual => "compare-less-or-equal",
        Compare::GreaterOrEqual => "compare-greater-or-equal",
        Compare::IsBlank => "compare-is-blank",
        Compare::IsNotBlank => "compare-is-not-blank",
        Compare::Contains => "compare-contains",
        Compare::NotContains => "compare-not-contains",
    })
}

/// Нужно ли сравнению значение (у «пусто» и «не пусто» — нет).
fn needs_value(compare: Compare) -> bool {
    !matches!(compare, Compare::IsBlank | Compare::IsNotBlank)
}

/// Выбор столбца, сравнения и значения в одну строку.
fn condition(
    ui: &mut Ui,
    id: &str,
    fields: &[String],
    field: &mut usize,
    compares: &[Compare],
    compare: &mut Compare,
    value: &mut String,
) {
    let named = |response: egui::Response, role, name: &str| {
        response.widget_info(|| egui::WidgetInfo::labeled(role, true, name));
    };
    ui.horizontal(|ui| {
        let response = egui::ComboBox::from_id_salt((id, "field"))
            .selected_text(fields.get(*field).map_or("", String::as_str))
            .show_ui(ui, |ui| {
                for (i, name) in fields.iter().enumerate() {
                    ui.selectable_value(field, i, name);
                }
            })
            .response;
        named(response, egui::Role::ComboBox, tr("condition-field"));
        let response = egui::ComboBox::from_id_salt((id, "compare"))
            .selected_text(compare_name(*compare))
            .show_ui(ui, |ui| {
                for c in compares {
                    ui.selectable_value(compare, *c, compare_name(*c));
                }
            })
            .response;
        named(response, egui::Role::ComboBox, tr("condition-compare"));
        if needs_value(*compare) {
            let response = ui.add(TextEdit::singleline(value).desired_width(140.0));
            named(response, egui::Role::TextInput, tr("condition-value"));
        }
    });
}

/// Окно «Изменить список получателей».
#[derive(Debug, Clone, Default)]
pub struct RecipientsDialog {
    /// Окно открыто.
    pub open: bool,
    /// Получатели с отметками (правится в окне).
    pub data: MergeData,
    /// По какому столбцу отсортировано и в обратном ли порядке.
    sorted: Option<(usize, bool)>,
    /// Фильтр: столбец, сравнение, значение.
    filter_field: usize,
    filter_compare: Compare,
    filter_value: String,
}

impl RecipientsDialog {
    /// Открывает окно со списком `data`.
    pub fn show(&mut self, data: &MergeData) {
        *self = RecipientsDialog {
            open: true,
            data: data.clone(),
            sorted: None,
            filter_field: 0,
            filter_compare: Compare::Equal,
            filter_value: String::new(),
        };
    }

    /// Рисует окно; после «ОК» возвращает новый список.
    pub fn ui(&mut self, ctx: &Context) -> Option<MergeData> {
        if !self.open {
            return None;
        }
        let mut done = None;
        let mut close = false;
        let mut open = true;
        egui::Window::new(tr("recipients-title"))
            .open(&mut open)
            .collapsible(false)
            .default_width(620.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(tr("recipients-help"));
                self.table(ui);
                ui.separator();
                ui.label(RichText::new(tr("recipients-filter")).strong());
                condition(
                    ui,
                    "recipients-filter",
                    &self.data.headers,
                    &mut self.filter_field,
                    &Compare::FILTERS,
                    &mut self.filter_compare,
                    &mut self.filter_value,
                );
                ui.horizontal(|ui| {
                    if ui.button(tr("recipients-apply-filter")).clicked() {
                        self.data.filter(
                            self.filter_field,
                            self.filter_compare,
                            &self.filter_value,
                        );
                    }
                    if ui.button(tr("recipients-all")).clicked() {
                        self.data.excluded.clear();
                    }
                });
                ui.label(format!(
                    "{} {} / {}",
                    tr("recipients-chosen"),
                    self.data.records().len(),
                    self.data.rows.len()
                ));
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(tr("ok")).clicked() {
                        done = Some(self.data.clone());
                    }
                    if ui.button(tr("cancel")).clicked() {
                        close = true;
                    }
                });
            });
        if !open || close || done.is_some() || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.open = false;
        }
        done
    }

    /// Таблица получателей: отметки и значения; щелчок по заголовку
    /// сортирует.
    fn table(&mut self, ui: &mut Ui) {
        egui::ScrollArea::both()
            .max_height(260.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Grid::new("recipients-table")
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("");
                        for column in 0..self.data.headers.len() {
                            let arrow = match self.sorted {
                                Some((c, false)) if c == column => " ^",
                                Some((c, true)) if c == column => " v",
                                _ => "",
                            };
                            let title = format!("{}{arrow}", self.data.headers[column]);
                            if ui.button(RichText::new(title).strong()).clicked() {
                                let descending = self.sorted == Some((column, false));
                                self.data.sort_by(column, descending);
                                self.sorted = Some((column, descending));
                            }
                        }
                        ui.end_row();
                        for row in 0..self.data.rows.len() {
                            let mut on = self.data.included(row);
                            let response = ui.checkbox(&mut on, "");
                            if response.changed() {
                                self.data.set_included(row, on);
                            }
                            let name = self.data.rows[row].first().cloned().unwrap_or_default();
                            response.widget_info(|| {
                                egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, &name)
                            });
                            for column in 0..self.data.headers.len() {
                                let value = self.data.rows[row].get(column).cloned();
                                let value = value.unwrap_or_default().replace('\n', " ");
                                ui.label(value);
                            }
                            ui.end_row();
                        }
                    });
            });
    }
}

/// Какое правило правит окно.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuleKind {
    /// «Если… то… иначе».
    #[default]
    If,
    /// «Пропустить запись, если…».
    SkipIf,
}

/// Окно правила рассылки.
#[derive(Debug, Clone, Default)]
pub struct RuleDialog {
    /// Окно открыто.
    pub open: bool,
    /// Какое правило.
    pub kind: RuleKind,
    /// Столбцы получателей.
    fields: Vec<String>,
    /// Выбранный столбец.
    pub field: usize,
    /// Сравнение.
    pub compare: Compare,
    /// С чем сравнивать.
    pub value: String,
    /// Текст, если условие выполняется.
    pub then: String,
    /// Текст, если нет.
    pub otherwise: String,
}

impl RuleDialog {
    /// Открывает окно правила `kind` со столбцами `fields`.
    pub fn show(&mut self, kind: RuleKind, fields: &[String]) {
        *self = RuleDialog {
            open: true,
            kind,
            fields: fields.to_vec(),
            ..RuleDialog::default()
        };
    }

    /// Рисует окно; после «ОК» возвращает правило.
    pub fn ui(&mut self, ctx: &Context) -> Option<MergeRule> {
        if !self.open {
            return None;
        }
        let mut done = None;
        let mut close = false;
        let mut open = true;
        let title = match self.kind {
            RuleKind::If => tr("rule-if-title"),
            RuleKind::SkipIf => tr("rule-skip-title"),
        };
        egui::Window::new(title)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(tr("rule-condition"));
                condition(
                    ui,
                    "rule",
                    &self.fields,
                    &mut self.field,
                    &Compare::RULES,
                    &mut self.compare,
                    &mut self.value,
                );
                if self.kind == RuleKind::If {
                    let label = ui.label(tr("rule-then"));
                    ui.add(TextEdit::multiline(&mut self.then).desired_rows(2))
                        .labelled_by(label.id);
                    let label = ui.label(tr("rule-otherwise"));
                    ui.add(TextEdit::multiline(&mut self.otherwise).desired_rows(2))
                        .labelled_by(label.id);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let ready = self.field < self.fields.len();
                    if ui.add_enabled(ready, egui::Button::new(tr("ok"))).clicked() {
                        done = Some(self.rule());
                    }
                    if ui.button(tr("cancel")).clicked() {
                        close = true;
                    }
                });
            });
        if !open || close || done.is_some() || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.open = false;
        }
        done
    }

    /// Правило из полей окна.
    fn rule(&self) -> MergeRule {
        let field = self.fields.get(self.field).cloned().unwrap_or_default();
        let value = if needs_value(self.compare) {
            self.value.clone()
        } else {
            String::new()
        };
        match self.kind {
            RuleKind::If => MergeRule::If {
                field,
                compare: self.compare,
                value,
                then: self.then.clone(),
                otherwise: self.otherwise.clone(),
            },
            RuleKind::SkipIf => MergeRule::SkipIf {
                field,
                compare: self.compare,
                value,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn recipients_are_chosen() {
        let data = MergeData::from_csv("Имя;Город\nВера;Омск\nАнна;Тверь\n");
        let mut dialog = RecipientsDialog::default();
        dialog.show(&data);
        type State = (RecipientsDialog, Option<MergeData>);
        let mut h = Harness::new_ui_state(
            |ui, state: &mut State| {
                if let Some(done) = state.0.ui(ui.ctx()) {
                    state.1 = Some(done);
                }
            },
            (dialog, None),
        );
        h.run();
        h.get_by_label("Имя").click();
        h.run();
        assert_eq!(h.state().0.data.rows[0][0], "Анна");
        h.state_mut().0.filter_field = 1;
        h.state_mut().0.filter_value = "Омск".into();
        h.run();
        h.get_by_label("Отфильтровать").click();
        h.run();
        assert_eq!(h.state().0.data.records(), [1]);
        h.get_by_label("ОК").click();
        h.run();
        let chosen = h.state().1.clone().expect("ОК даёт список");
        assert_eq!(chosen.records(), [1]);
        assert_eq!(chosen.rows[1][0], "Вера");
    }

    #[test]
    fn rule_is_built() {
        let mut dialog = RuleDialog::default();
        dialog.show(RuleKind::If, &["Пол".into()]);
        dialog.value = "Ж".into();
        dialog.then = "Уважаемая".into();
        dialog.otherwise = "Уважаемый".into();
        assert_eq!(
            dialog.rule(),
            MergeRule::If {
                field: "Пол".into(),
                compare: Compare::Equal,
                value: "Ж".into(),
                then: "Уважаемая".into(),
                otherwise: "Уважаемый".into(),
            }
        );
        dialog.show(RuleKind::SkipIf, &["Долг".into()]);
        dialog.compare = Compare::IsBlank;
        dialog.value = "не нужно".into();
        assert_eq!(
            dialog.rule(),
            MergeRule::SkipIf {
                field: "Долг".into(),
                compare: Compare::IsBlank,
                value: String::new(),
            }
        );
    }
}
