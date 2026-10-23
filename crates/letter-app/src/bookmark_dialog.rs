//! Окно «Закладка» («Вставка → Ссылки → Закладка»): добавить закладку
//! на выделенное, перейти к закладке, удалить её. Окно только собирает
//! ответ; документ меняет просмотрщик.

use eframe::egui::{self, Context, Key, TextEdit};

use crate::i18n::tr;

/// Что попросили в окне.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookmarkAction {
    /// «Добавить».
    Add(String),
    /// «Удалить».
    Delete(String),
    /// «Перейти».
    GoTo(String),
}

/// Состояние окна.
#[derive(Debug, Clone, Default)]
pub struct BookmarkDialog {
    /// Окно открыто.
    pub open: bool,
    /// Имя закладки.
    pub name: String,
    /// Закладки документа.
    pub bookmarks: Vec<String>,
    /// Поставить фокус в поле имени в следующем кадре.
    focus: bool,
}

impl BookmarkDialog {
    /// Открывает окно со списком закладок документа.
    pub fn show(&mut self, bookmarks: Vec<String>) {
        self.open = true;
        self.focus = true;
        self.name.clear();
        self.bookmarks = bookmarks;
    }

    /// Рисует окно и возвращает, что попросили.
    pub fn ui(&mut self, ctx: &Context) -> Option<BookmarkAction> {
        if !self.open {
            return None;
        }
        let mut action = None;
        let mut close = false;
        let mut open = true;
        egui::Window::new(tr("bookmark-title"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                let label = ui.label(tr("bookmark-name"));
                let field = ui
                    .add(
                        TextEdit::singleline(&mut self.name)
                            .desired_width(260.0)
                            .id_salt("bookmark-name"),
                    )
                    .labelled_by(label.id);
                if std::mem::take(&mut self.focus) {
                    field.request_focus();
                }
                let valid = valid_name(&self.name);
                if field.lost_focus() && valid && ui.input(|i| i.key_pressed(Key::Enter)) {
                    action = Some(BookmarkAction::Add(self.name.clone()));
                }
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(160.0)
                        .min_scrolled_height(160.0)
                        .show(ui, |ui| {
                            ui.set_min_width(250.0);
                            for name in &self.bookmarks {
                                let chosen = self.name.eq_ignore_ascii_case(name);
                                let item = ui.selectable_label(chosen, name);
                                if item.clicked() {
                                    self.name = name.clone();
                                }
                                if item.double_clicked() {
                                    action = Some(BookmarkAction::GoTo(name.clone()));
                                }
                            }
                        });
                });
                if !self.name.is_empty() && !valid {
                    ui.label(tr("bookmark-invalid"));
                }
                let known = self
                    .bookmarks
                    .iter()
                    .find(|b| b.eq_ignore_ascii_case(&self.name))
                    .cloned();
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(valid, egui::Button::new(tr("bookmark-add")))
                        .clicked()
                    {
                        action = Some(BookmarkAction::Add(self.name.clone()));
                    }
                    if ui
                        .add_enabled(known.is_some(), egui::Button::new(tr("bookmark-delete")))
                        .clicked()
                        && let Some(name) = &known
                    {
                        action = Some(BookmarkAction::Delete(name.clone()));
                    }
                    if ui
                        .add_enabled(known.is_some(), egui::Button::new(tr("bookmark-go-to")))
                        .clicked()
                        && let Some(name) = &known
                    {
                        action = Some(BookmarkAction::GoTo(name.clone()));
                    }
                    if ui.button(tr("cancel")).clicked() {
                        close = true;
                    }
                });
            });
        // «Удалить» оставляет окно открытым, как в Word.
        let keep = matches!(action, Some(BookmarkAction::Delete(_)));
        if let Some(BookmarkAction::Delete(name)) = &action {
            self.bookmarks.retain(|b| b != name);
            self.name.clear();
        }
        if !open
            || close
            || (action.is_some() && !keep)
            || ctx.input(|i| i.key_pressed(Key::Escape))
        {
            self.open = false;
        }
        action
    }
}

/// Подходит ли имя закладки, как в Word: начинается с буквы, дальше
/// буквы, цифры и `_`, не длиннее 40 знаков.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(char::is_alphabetic)
        && chars.all(|c| c.is_alphanumeric() || c == '_')
        && name.chars().count() <= 40
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn names_are_checked() {
        assert!(valid_name("Глава_1"));
        assert!(valid_name("Intro"));
        assert!(!valid_name("1st"));
        assert!(!valid_name("two words"));
        assert!(!valid_name("_hidden"));
        assert!(!valid_name(""));
        assert!(!valid_name(&"a".repeat(41)));
    }

    #[test]
    fn buttons_give_actions() {
        let mut dialog = BookmarkDialog::default();
        dialog.show(vec!["Intro".into(), "End".into()]);
        let mut h = Harness::new_ui_state(
            |ui, state: &mut (BookmarkDialog, Vec<BookmarkAction>)| {
                if let Some(action) = state.0.ui(ui.ctx()) {
                    state.1.push(action);
                }
            },
            (dialog, Vec::new()),
        );
        h.run();
        h.get_by_label("End").click();
        h.run();
        assert_eq!(h.state().0.name, "End");
        h.get_by_label("Удалить").click();
        h.run();
        assert!(h.state().0.open);
        h.state_mut().0.name = "Intro".into();
        h.run();
        h.get_by_label("Перейти").click();
        h.run();
        assert_eq!(
            h.state().1,
            [
                BookmarkAction::Delete("End".into()),
                BookmarkAction::GoTo("Intro".into())
            ]
        );
        assert!(!h.state().0.open);
    }
}
