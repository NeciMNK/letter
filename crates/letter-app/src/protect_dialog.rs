//! Окно «Ограничить редактирование» («Рецензирование → Защитить»):
//! включить защиту документа с паролем или без, снять её, задать пароль
//! на изменение файла. Окно только собирает ответ: пароль проверяет
//! и документ меняет просмотрщик (неверный пароль — [`ProtectDialog::fail`]).

use eframe::egui::{self, Context, Key, TextEdit};
use letter_model::EditRestriction;

use crate::i18n::tr;

/// Что попросили в окне.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtectAction {
    /// «Да, включить защиту» с этим ограничением и паролем (пустой — без
    /// пароля).
    Protect {
        /// Что разрешено.
        edit: EditRestriction,
        /// Пароль.
        password: String,
    },
    /// «Отключить защиту» с этим паролем.
    Unprotect(String),
    /// Пароль на изменение файла (пустой — убрать) и «рекомендовать
    /// только чтение».
    WriteProtect {
        /// Пароль.
        password: String,
        /// Рекомендовать доступ только для чтения.
        recommended: bool,
    },
    /// Разрешить правку файла с паролем на изменение.
    AllowEditing(String),
}

/// С чем открыто окно.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProtectMode {
    /// Защиты нет: можно включить.
    #[default]
    Protect,
    /// Защита включена: можно снять.
    Unprotect,
    /// Файл открыт только для чтения (пароль на изменение): ввести пароль.
    AllowEditing,
}

/// Ограничения, которые можно выбрать в окне, и их подписи.
const RESTRICTIONS: [(EditRestriction, &str); 4] = [
    (EditRestriction::ReadOnly, "protect-read-only"),
    (EditRestriction::Comments, "protect-comments"),
    (EditRestriction::TrackedChanges, "protect-tracked-changes"),
    (EditRestriction::Forms, "protect-forms"),
];

/// Состояние окна.
#[derive(Debug, Clone, Default)]
pub struct ProtectDialog {
    /// Окно открыто.
    pub open: bool,
    /// С чем открыто.
    pub mode: ProtectMode,
    /// Выбранное ограничение.
    pub edit: EditRestriction,
    /// Пароль.
    pub password: String,
    /// Пароль ещё раз.
    pub confirm: String,
    /// Пароль на изменение файла.
    pub modify_password: String,
    /// «Рекомендовать доступ только для чтения».
    pub recommended: bool,
    /// Показать «Неверный пароль».
    wrong: bool,
}

impl ProtectDialog {
    /// Открывает окно в режиме `mode`; `recommended` — нынешний флаг
    /// «рекомендовать только чтение».
    pub fn show(&mut self, mode: ProtectMode, recommended: bool) {
        *self = ProtectDialog {
            open: true,
            mode,
            edit: EditRestriction::ReadOnly,
            recommended,
            ..ProtectDialog::default()
        };
    }

    /// Пароль не подошёл: окно остаётся открытым с сообщением.
    pub fn fail(&mut self) {
        self.open = true;
        self.wrong = true;
        self.password.clear();
    }

    /// Рисует окно и возвращает, что попросили.
    pub fn ui(&mut self, ctx: &Context) -> Option<ProtectAction> {
        if !self.open {
            return None;
        }
        let mut action = None;
        let mut close = false;
        let mut open = true;
        egui::Window::new(tr("protect-title"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                let password = |ui: &mut egui::Ui, label: &str, text: &mut String, id: &str| {
                    let label = ui.label(label);
                    ui.add(
                        TextEdit::singleline(text)
                            .password(true)
                            .desired_width(240.0)
                            .id_salt(id),
                    )
                    .labelled_by(label.id)
                };
                match self.mode {
                    ProtectMode::Protect => {
                        ui.label(tr("protect-restriction"));
                        for (kind, label) in RESTRICTIONS {
                            ui.radio_value(&mut self.edit, kind, tr(label));
                        }
                        password(ui, tr("protect-password"), &mut self.password, "pw");
                        password(ui, tr("protect-confirm"), &mut self.confirm, "pw2");
                        let same = self.password == self.confirm;
                        if !same {
                            ui.label(tr("protect-mismatch"));
                        }
                        if ui
                            .add_enabled(same, egui::Button::new(tr("protect-start")))
                            .clicked()
                        {
                            action = Some(ProtectAction::Protect {
                                edit: self.edit,
                                password: self.password.clone(),
                            });
                        }
                        ui.separator();
                        password(
                            ui,
                            tr("protect-modify-password"),
                            &mut self.modify_password,
                            "pw3",
                        );
                        ui.checkbox(&mut self.recommended, tr("protect-recommended"));
                        if ui.button(tr("protect-modify-apply")).clicked() {
                            action = Some(ProtectAction::WriteProtect {
                                password: self.modify_password.clone(),
                                recommended: self.recommended,
                            });
                        }
                    }
                    ProtectMode::Unprotect | ProtectMode::AllowEditing => {
                        let field =
                            password(ui, tr("protect-password"), &mut self.password, "pw-check");
                        let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        if self.wrong {
                            ui.label(tr("protect-wrong"));
                        }
                        let label = if self.mode == ProtectMode::Unprotect {
                            "protect-stop"
                        } else {
                            "protect-allow-editing"
                        };
                        if ui.button(tr(label)).clicked() || enter {
                            let password = self.password.clone();
                            action = Some(if self.mode == ProtectMode::Unprotect {
                                ProtectAction::Unprotect(password)
                            } else {
                                ProtectAction::AllowEditing(password)
                            });
                        }
                    }
                }
                if ui.button(tr("cancel")).clicked() {
                    close = true;
                }
            });
        if !open || close || action.is_some() || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.open = false;
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn harness(mode: ProtectMode) -> Harness<'static, (ProtectDialog, Vec<ProtectAction>)> {
        let mut dialog = ProtectDialog::default();
        dialog.show(mode, false);
        Harness::new_ui_state(
            |ui, state: &mut (ProtectDialog, Vec<ProtectAction>)| {
                if let Some(action) = state.0.ui(ui.ctx()) {
                    state.1.push(action);
                }
            },
            (dialog, Vec::new()),
        )
    }

    #[test]
    fn protect_with_password() {
        let mut h = harness(ProtectMode::Protect);
        h.run();
        h.get_by_label("Примечания").click();
        h.state_mut().0.password = "1".into();
        h.run();
        // Пароли разные — кнопка не работает.
        h.get_by_label("Да, включить защиту").click();
        h.run();
        assert!(h.state().1.is_empty());
        h.state_mut().0.confirm = "1".into();
        h.run();
        h.get_by_label("Да, включить защиту").click();
        h.run();
        assert_eq!(
            h.state().1,
            [ProtectAction::Protect {
                edit: EditRestriction::Comments,
                password: "1".into()
            }]
        );
        assert!(!h.state().0.open);
    }

    #[test]
    fn wrong_password_keeps_the_window() {
        let mut h = harness(ProtectMode::Unprotect);
        h.state_mut().0.password = "x".into();
        h.run();
        h.get_by_label("Отключить защиту").click();
        h.run();
        assert_eq!(h.state().1, [ProtectAction::Unprotect("x".into())]);
        h.state_mut().0.fail();
        h.run();
        assert!(h.state().0.open);
        h.get_by_label("Неверный пароль.");
    }
}
