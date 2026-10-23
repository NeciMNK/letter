//! Окно пароля: спросить пароль, чтобы открыть зашифрованный документ,
//! или задать пароль на открытие («Файл → Сведения → Зашифровать
//! паролем»). Окно только собирает ответ; файл открывает и документ
//! меняет просмотрщик.

use std::path::PathBuf;

use eframe::egui::{self, Context, Key, TextEdit};

use crate::i18n::tr;

/// Что попросили в окне.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasswordAnswer {
    /// Открыть файл с этим паролем.
    Open(PathBuf, String),
    /// Шифровать документ этим паролем при сохранении; `None` — снять
    /// шифрование (пароль стёрт).
    Encrypt(Option<String>),
}

/// Зачем открыто окно.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Purpose {
    Open(PathBuf),
    Encrypt,
}

/// Состояние окна.
#[derive(Debug, Clone, Default)]
pub struct PasswordDialog {
    /// Окно открыто.
    pub open: bool,
    purpose: Option<Purpose>,
    /// Введённый пароль.
    pub password: String,
    /// Пароль ещё раз (при шифровании).
    pub confirm: String,
    /// Поставить фокус в поле пароля в следующем кадре.
    focus: bool,
}

impl PasswordDialog {
    /// Спрашивает пароль, чтобы открыть файл `path`.
    pub fn ask_to_open(&mut self, path: PathBuf) {
        self.start(Purpose::Open(path), String::new());
    }

    /// Спрашивает новый пароль на открытие; `current` — нынешний.
    pub fn ask_to_encrypt(&mut self, current: Option<&str>) {
        self.start(Purpose::Encrypt, current.unwrap_or_default().to_owned());
    }

    fn start(&mut self, purpose: Purpose, password: String) {
        self.open = true;
        self.focus = true;
        self.purpose = Some(purpose);
        self.confirm = password.clone();
        self.password = password;
    }

    /// Рисует окно и возвращает ответ.
    pub fn ui(&mut self, ctx: &Context) -> Option<PasswordAnswer> {
        if !self.open {
            return None;
        }
        let purpose = self.purpose.clone()?;
        let mut answer = None;
        let mut close = false;
        let mut open = true;
        let title = match purpose {
            Purpose::Open(_) => tr("password-title"),
            Purpose::Encrypt => tr("password-encrypt-title"),
        };
        egui::Window::new(title)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                match &purpose {
                    Purpose::Open(path) => {
                        ui.label(tr("password-open-prompt"));
                        if let Some(name) = path.file_name() {
                            ui.label(egui::RichText::new(name.to_string_lossy()).strong());
                        }
                    }
                    Purpose::Encrypt => {
                        ui.label(tr("password-encrypt-prompt"));
                    }
                }
                let label = ui.label(tr("password-label"));
                let field = ui
                    .add(
                        TextEdit::singleline(&mut self.password)
                            .password(true)
                            .desired_width(260.0)
                            .id_salt("password"),
                    )
                    .labelled_by(label.id);
                if std::mem::take(&mut self.focus) {
                    field.request_focus();
                }
                let mut enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let mut ready = !self.password.is_empty();
                if purpose == Purpose::Encrypt {
                    let label = ui.label(tr("password-confirm"));
                    let confirm = ui
                        .add(
                            TextEdit::singleline(&mut self.confirm)
                                .password(true)
                                .desired_width(260.0)
                                .id_salt("password-confirm"),
                        )
                        .labelled_by(label.id);
                    enter = confirm.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    ready = self.password == self.confirm;
                    if !ready && !self.confirm.is_empty() {
                        ui.label(tr("password-mismatch"));
                    }
                    ui.label(
                        egui::RichText::new(tr("password-warning"))
                            .small()
                            .color(ui.visuals().weak_text_color()),
                    );
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let ok = ui.add_enabled(ready, egui::Button::new(tr("ok"))).clicked();
                    if (ok || (enter && ready))
                        && let Some(a) = self.answer(&purpose)
                    {
                        answer = Some(a);
                    }
                    if ui.button(tr("cancel")).clicked() {
                        close = true;
                    }
                });
            });
        if !open || close || answer.is_some() || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.open = false;
            // Пароль не держим в окне дольше, чем нужно.
            self.password.clear();
            self.confirm.clear();
        }
        answer
    }

    fn answer(&self, purpose: &Purpose) -> Option<PasswordAnswer> {
        match purpose {
            Purpose::Open(path) => Some(PasswordAnswer::Open(path.clone(), self.password.clone())),
            Purpose::Encrypt => Some(PasswordAnswer::Encrypt(
                (!self.password.is_empty()).then(|| self.password.clone()),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn harness(dialog: PasswordDialog) -> Harness<'static, (PasswordDialog, Vec<PasswordAnswer>)> {
        Harness::new_ui_state(
            |ui, state: &mut (PasswordDialog, Vec<PasswordAnswer>)| {
                if let Some(answer) = state.0.ui(ui.ctx()) {
                    state.1.push(answer);
                }
            },
            (dialog, Vec::new()),
        )
    }

    #[test]
    fn password_to_open() {
        let mut dialog = PasswordDialog::default();
        dialog.ask_to_open(PathBuf::from("/docs/тайна.docx"));
        let mut h = harness(dialog);
        h.run();
        h.get_by_label("тайна.docx");
        h.state_mut().0.password = "пароль".into();
        h.run();
        h.get_by_label("ОК").click();
        h.run();
        assert_eq!(
            h.state().1,
            [PasswordAnswer::Open(
                PathBuf::from("/docs/тайна.docx"),
                "пароль".into()
            )]
        );
        assert!(!h.state().0.open);
        assert!(h.state().0.password.is_empty());
    }

    #[test]
    fn new_password_must_be_typed_twice() {
        let mut dialog = PasswordDialog::default();
        dialog.ask_to_encrypt(None);
        let mut h = harness(dialog);
        h.run();
        h.state_mut().0.password = "один".into();
        h.state_mut().0.confirm = "два".into();
        h.run();
        h.get_by_label("Пароли не совпадают.");
        h.get_by_label("ОК").click();
        h.run();
        assert!(h.state().1.is_empty());
        h.state_mut().0.confirm = "один".into();
        h.run();
        h.get_by_label("ОК").click();
        h.run();
        assert_eq!(h.state().1, [PasswordAnswer::Encrypt(Some("один".into()))]);
    }

    #[test]
    fn empty_password_removes_encryption() {
        let mut dialog = PasswordDialog::default();
        dialog.ask_to_encrypt(Some("старый"));
        assert_eq!(dialog.password, "старый");
        let mut h = harness(dialog);
        h.run();
        h.state_mut().0.password.clear();
        h.state_mut().0.confirm.clear();
        h.run();
        h.get_by_label("ОК").click();
        h.run();
        assert_eq!(h.state().1, [PasswordAnswer::Encrypt(None)]);
    }
}
