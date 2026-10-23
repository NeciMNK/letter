//! Окно «Найти и заменить» (Ctrl+F, Ctrl+H, кнопки «Найти» и «Заменить»
//! на «Главной»). Окно только собирает запрос; ищет и меняет документ
//! просмотрщик.

use crate::i18n::tr;
use eframe::egui::{self, Context, Key, TextEdit};
use letter_editor::SearchOptions;

/// Что попросили в окне.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindAction {
    /// «Найти далее».
    Next,
    /// «Заменить»: заменить выделенное найденное и найти следующее.
    Replace,
    /// «Заменить все».
    ReplaceAll,
}

/// Состояние окна.
#[derive(Debug, Clone, Default)]
pub struct FindDialog {
    /// Окно открыто.
    pub open: bool,
    /// Показаны поля замены.
    pub replace: bool,
    /// Что искать.
    pub query: String,
    /// На что заменить.
    pub replacement: String,
    /// Параметры поиска.
    pub options: SearchOptions,
    /// Сообщение под полями («Не найдено», «Выполнено замен: 3»).
    pub message: Option<String>,
    /// Поставить фокус в поле «Найти» в следующем кадре.
    focus: bool,
}

impl FindDialog {
    /// Открывает окно: для поиска или (`replace`) для замены.
    pub fn show(&mut self, replace: bool) {
        self.open = true;
        self.replace = replace;
        self.focus = true;
        self.message = None;
    }

    /// Рисует окно и возвращает, что попросили.
    pub fn ui(&mut self, ctx: &Context) -> Option<FindAction> {
        if !self.open {
            return None;
        }
        let mut action = None;
        let mut open = true;
        egui::Window::new(tr("find-replace-title"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_pos(ctx.content_rect().right_top() + egui::vec2(-420.0, 140.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.replace, false, tr("find"));
                    ui.selectable_value(&mut self.replace, true, tr("replace"));
                });
                ui.separator();
                egui::Grid::new("find-fields")
                    .num_columns(2)
                    .spacing(egui::vec2(8.0, 6.0))
                    .show(ui, |ui| {
                        let label = ui.label(tr("find-what"));
                        let query = ui
                            .add(
                                TextEdit::singleline(&mut self.query)
                                    .desired_width(240.0)
                                    .id_salt("find-query"),
                            )
                            .labelled_by(label.id);
                        if std::mem::take(&mut self.focus) {
                            query.request_focus();
                        }
                        if query.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            action = Some(FindAction::Next);
                            query.request_focus();
                        }
                        ui.end_row();
                        if self.replace {
                            let label = ui.label(tr("replace-with"));
                            ui.add(
                                TextEdit::singleline(&mut self.replacement)
                                    .desired_width(240.0)
                                    .id_salt("find-replacement"),
                            )
                            .labelled_by(label.id);
                            ui.end_row();
                        }
                    });
                ui.checkbox(&mut self.options.match_case, tr("find-match-case"));
                ui.checkbox(&mut self.options.whole_word, tr("find-whole-words"));
                if let Some(message) = &self.message {
                    ui.label(message);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let ready = !self.query.is_empty();
                    if self.replace {
                        if ui
                            .add_enabled(ready, egui::Button::new(tr("replace")))
                            .clicked()
                        {
                            action = Some(FindAction::Replace);
                        }
                        if ui
                            .add_enabled(ready, egui::Button::new(tr("replace-all")))
                            .clicked()
                        {
                            action = Some(FindAction::ReplaceAll);
                        }
                    }
                    if ui
                        .add_enabled(ready, egui::Button::new(tr("find-next")))
                        .clicked()
                    {
                        action = Some(FindAction::Next);
                    }
                });
            });
        if !open || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.open = false;
        }
        // Пустой запрос ничего не ищет; поля без перевода строки.
        self.query.retain(|c| c != '\n');
        if self.query.is_empty() {
            return None;
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn buttons_give_actions() {
        let mut dialog = FindDialog::default();
        dialog.show(true);
        let mut h = Harness::new_ui_state(
            |ui, state: &mut (FindDialog, Vec<FindAction>)| {
                if let Some(action) = state.0.ui(ui.ctx()) {
                    state.1.push(action);
                }
            },
            (dialog, Vec::new()),
        );
        h.run();
        // Без текста кнопки не работают.
        h.get_by_label("Заменить все").click();
        h.run();
        assert!(h.state().1.is_empty());
        h.state_mut().0.query = "кот".into();
        h.run();
        h.get_by_label("Заменить все").click();
        h.run();
        h.get_by_label("Найти далее").click();
        h.run();
        assert_eq!(h.state().1, [FindAction::ReplaceAll, FindAction::Next]);
        h.key_press(Key::Escape);
        h.run();
        assert!(!h.state().0.open);
    }
}
