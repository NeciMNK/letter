//! Окно «Ограничить редактирование» («Рецензирование → Защитить»):
//! включить защиту документа (только чтение, только примечания, только
//! исправления) с паролем или без, либо снять её.

use eframe::egui::{self, Align2, Context, Key, Vec2};
use letter_model::{Protection, ProtectionKind};

use crate::i18n::tr;

/// Что выбрано в окне.
#[derive(Debug, Clone, PartialEq)]
pub enum ProtectResult {
    /// Включить защиту.
    Protect(Protection),
    /// Снять защиту.
    Unprotect,
}

/// Состояние окна.
#[derive(Debug, Clone, PartialEq)]
pub struct ProtectDialog {
    /// Что разрешить.
    pub kind: ProtectionKind,
    /// Пароль.
    pub password: String,
    /// Пароль ещё раз.
    pub confirm: String,
    /// Сообщение об ошибке (пароли не совпали, пароль неверный).
    pub error: Option<&'static str>,
}

impl Default for ProtectDialog {
    fn default() -> ProtectDialog {
        ProtectDialog {
            kind: ProtectionKind::ReadOnly,
            password: String::new(),
            confirm: String::new(),
            error: None,
        }
    }
}

/// Виды защиты в окне.
const KINDS: [(ProtectionKind, &str); 3] = [
    (ProtectionKind::ReadOnly, "protect-read-only"),
    (ProtectionKind::Comments, "protect-comments"),
    (ProtectionKind::TrackedChanges, "protect-tracked"),
];

impl ProtectDialog {
    /// Проверяет введённое: для включения пароли должны совпасть, для
    /// снятия — подойти к `current`.
    pub fn submit(&mut self, current: Option<&Protection>) -> Option<ProtectResult> {
        match current.filter(|p| p.enforced) {
            Some(protection) => {
                let fits = protection
                    .password
                    .as_ref()
                    .is_none_or(|hash| letter_docx::check_password(hash, &self.password));
                if fits {
                    Some(ProtectResult::Unprotect)
                } else {
                    self.error = Some("protect-wrong-password");
                    None
                }
            }
            None => {
                if self.password != self.confirm {
                    self.error = Some("protect-mismatch");
                    return None;
                }
                Some(ProtectResult::Protect(Protection {
                    kind: self.kind,
                    enforced: true,
                    password: letter_docx::protect_password(&self.password),
                }))
            }
        }
    }

    /// Рисует окно. Возвращает выбор и закрыто ли окно.
    pub fn ui(
        &mut self,
        ctx: &Context,
        current: Option<&Protection>,
    ) -> (Option<ProtectResult>, bool) {
        let protected = current.is_some_and(|p| p.enforced);
        let has_password = current.is_some_and(|p| p.password.is_some());
        let mut open = true;
        let mut close = false;
        let mut result = None;
        egui::Window::new(tr("protect-title"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_width(320.0);
                if protected {
                    ui.label(tr("protect-is-on"));
                } else {
                    ui.label(tr("protect-allow"));
                    for (kind, label) in KINDS {
                        ui.radio_value(&mut self.kind, kind, tr(label));
                    }
                }
                let mut enter = false;
                if !protected || has_password {
                    ui.add_space(6.0);
                    ui.label(tr("protect-password"));
                    let field =
                        ui.add(egui::TextEdit::singleline(&mut self.password).password(true));
                    enter |= field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                }
                if !protected {
                    ui.label(tr("protect-confirm"));
                    let field =
                        ui.add(egui::TextEdit::singleline(&mut self.confirm).password(true));
                    enter |= field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    ui.small(tr("protect-password-hint"));
                }
                if let Some(error) = self.error {
                    ui.colored_label(egui::Color32::from_rgb(220, 60, 60), tr(error));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let label = if protected {
                        "protect-stop"
                    } else {
                        "protect-start"
                    };
                    if ui.button(tr(label)).clicked() || enter {
                        result = self.submit(current);
                    }
                    if ui.button(tr("cancel")).clicked() {
                        close = true;
                    }
                });
            });
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let done = result.is_some() || !open || close || escape;
        (result, done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protection_is_set_and_removed_with_password() {
        let mut dialog = ProtectDialog {
            kind: ProtectionKind::Comments,
            password: "пароль".into(),
            confirm: "пароли".into(),
            error: None,
        };
        assert_eq!(dialog.submit(None), None);
        assert_eq!(dialog.error, Some("protect-mismatch"));
        dialog.confirm = "пароль".into();
        let Some(ProtectResult::Protect(protection)) = dialog.submit(None) else {
            panic!("нет защиты");
        };
        assert_eq!(protection.kind, ProtectionKind::Comments);
        assert!(protection.enforced && protection.password.is_some());

        let mut unlock = ProtectDialog {
            password: "не тот".into(),
            ..ProtectDialog::default()
        };
        assert_eq!(unlock.submit(Some(&protection)), None);
        assert_eq!(unlock.error, Some("protect-wrong-password"));
        unlock.password = "пароль".into();
        assert_eq!(
            unlock.submit(Some(&protection)),
            Some(ProtectResult::Unprotect)
        );

        // Без пароля защита снимается сразу.
        let open = Protection {
            password: None,
            ..protection
        };
        assert_eq!(
            ProtectDialog::default().submit(Some(&open)),
            Some(ProtectResult::Unprotect)
        );
    }
}
