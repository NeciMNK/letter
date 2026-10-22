//! Окно «Ссылка» (Ctrl+K, «Вставка → Ссылка»): адрес или закладка
//! и текст ссылки. Окно только собирает ответ; документ меняет
//! просмотрщик.

use eframe::egui::{self, Context, Key, TextEdit};
use letter_model::Hyperlink;

use crate::i18n::tr;

/// Что попросили в окне.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkAction {
    /// Поставить ссылку; `text` — новый текст (`None` — текст не менять).
    Apply {
        /// Ссылка.
        link: Hyperlink,
        /// Текст.
        text: Option<String>,
    },
    /// «Удалить ссылку».
    Remove,
}

/// Состояние окна.
#[derive(Debug, Clone, Default)]
pub struct LinkDialog {
    /// Окно открыто.
    pub open: bool,
    /// Текст ссылки.
    pub text: String,
    /// Текст можно менять (выделение не больше одного абзаца).
    pub text_enabled: bool,
    /// Текст при открытии окна: если не меняли, остаётся как был.
    original_text: String,
    /// Адрес.
    pub address: String,
    /// Место в документе (закладка) вместо адреса.
    pub to_bookmark: bool,
    /// Выбранная закладка.
    pub bookmark: String,
    /// Закладки документа.
    pub bookmarks: Vec<String>,
    /// Правится существующая ссылка (есть «Удалить ссылку»).
    pub editing: bool,
    /// Остальное от правленой ссылки (подсказка, окно).
    base: Hyperlink,
    /// Поставить фокус в поле адреса в следующем кадре.
    focus: bool,
}

impl LinkDialog {
    /// Открывает окно. `text` — текст для поля «Текст» (`None` — поле
    /// недоступно), `existing` — ссылка у курсора.
    pub fn show(
        &mut self,
        text: Option<String>,
        existing: Option<Hyperlink>,
        bookmarks: Vec<String>,
    ) {
        self.open = true;
        self.focus = true;
        self.text_enabled = text.is_some();
        self.text = text.unwrap_or_default();
        self.original_text = self.text.clone();
        self.bookmarks = bookmarks;
        self.editing = existing.is_some();
        let existing = existing.unwrap_or_default();
        self.to_bookmark = existing.target.is_none() && existing.anchor.is_some();
        self.address = existing.target.clone().unwrap_or_default();
        if !self.to_bookmark
            && let Some(anchor) = &existing.anchor
        {
            self.address = format!("{}#{anchor}", self.address);
        }
        self.bookmark = existing
            .anchor
            .clone()
            .filter(|_| self.to_bookmark)
            .or_else(|| self.bookmarks.first().cloned())
            .unwrap_or_default();
        self.base = existing;
    }

    /// Ссылка из полей окна; `None` — адрес пуст.
    fn link(&self) -> Option<Hyperlink> {
        let mut link = Hyperlink {
            target: None,
            anchor: None,
            history: true,
            ..self.base.clone()
        };
        if self.to_bookmark {
            if self.bookmark.is_empty() {
                return None;
            }
            link.anchor = Some(self.bookmark.clone());
        } else {
            let address = normalized(&self.address)?;
            match address.split_once('#') {
                Some((target, anchor)) if !target.is_empty() && !anchor.is_empty() => {
                    link.target = Some(target.to_string());
                    link.anchor = Some(anchor.to_string());
                }
                _ => link.target = Some(address),
            }
        }
        Some(link)
    }

    /// Ответ «ОК».
    fn apply(&self) -> Option<LinkAction> {
        let link = self.link()?;
        let text = if self.text_enabled && (self.text != self.original_text || self.text.is_empty())
        {
            let text = self.text.trim();
            Some(if text.is_empty() {
                link.address()
            } else {
                text.to_string()
            })
        } else {
            None
        };
        Some(LinkAction::Apply { link, text })
    }

    /// Рисует окно и возвращает, что попросили.
    pub fn ui(&mut self, ctx: &Context) -> Option<LinkAction> {
        if !self.open {
            return None;
        }
        let mut action = None;
        let mut close = false;
        let mut open = true;
        egui::Window::new(tr("link-title"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Grid::new("link-fields")
                    .num_columns(2)
                    .spacing(egui::vec2(8.0, 6.0))
                    .show(ui, |ui| {
                        let label = ui.label(tr("link-text"));
                        ui.add_enabled(
                            self.text_enabled,
                            TextEdit::singleline(&mut self.text)
                                .desired_width(280.0)
                                .id_salt("link-text"),
                        )
                        .labelled_by(label.id);
                        ui.end_row();
                        ui.label(tr("link-to"));
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut self.to_bookmark, false, tr("link-to-address"));
                            ui.add_enabled_ui(!self.bookmarks.is_empty(), |ui| {
                                ui.radio_value(&mut self.to_bookmark, true, tr("link-to-bookmark"));
                            });
                        });
                        ui.end_row();
                        if self.to_bookmark {
                            let label = ui.label(tr("link-bookmark"));
                            egui::ComboBox::from_id_salt("link-bookmark")
                                .selected_text(self.bookmark.clone())
                                .width(280.0)
                                .show_ui(ui, |ui| {
                                    for name in &self.bookmarks {
                                        ui.selectable_value(&mut self.bookmark, name.clone(), name);
                                    }
                                })
                                .response
                                .labelled_by(label.id);
                        } else {
                            let label = ui.label(tr("link-address"));
                            let field = ui
                                .add(
                                    TextEdit::singleline(&mut self.address)
                                        .desired_width(280.0)
                                        .hint_text("https://")
                                        .id_salt("link-address"),
                                )
                                .labelled_by(label.id);
                            if std::mem::take(&mut self.focus) {
                                field.request_focus();
                            }
                            if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                                action = self.apply();
                            }
                        }
                        ui.end_row();
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    let ready = self.link().is_some();
                    if ui.add_enabled(ready, egui::Button::new(tr("ok"))).clicked() {
                        action = self.apply();
                    }
                    if ui.button(tr("cancel")).clicked() {
                        close = true;
                    }
                    if self.editing && ui.button(tr("remove-link")).clicked() {
                        action = Some(LinkAction::Remove);
                    }
                });
            });
        if !open || close || action.is_some() || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.open = false;
        }
        action
    }
}

/// Адрес как его поймёт браузер: `www.…` → `http://www.…`, почта →
/// `mailto:`. Пустой — `None`.
pub fn normalized(address: &str) -> Option<String> {
    let address = address.trim();
    if address.is_empty() {
        return None;
    }
    let has_scheme = address.split_once(':').is_some_and(|(scheme, _)| {
        scheme.len() > 1
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    });
    // Путь к файлу: «C:\…», «/…», «\\сервер\…», «./…».
    let drive = address.as_bytes().get(1) == Some(&b':')
        && address
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic);
    Some(
        if has_scheme || drive || address.starts_with(['/', '\\', '.']) {
            address.to_string()
        } else if address.contains('@') && !address.contains('/') {
            format!("mailto:{address}")
        } else {
            format!("http://{address}")
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn addresses_are_normalized() {
        assert_eq!(
            normalized(" www.example.com ").as_deref(),
            Some("http://www.example.com")
        );
        assert_eq!(
            normalized("https://a.b/c").as_deref(),
            Some("https://a.b/c")
        );
        assert_eq!(
            normalized("me@mail.ru").as_deref(),
            Some("mailto:me@mail.ru")
        );
        assert_eq!(
            normalized("C:\\docs\\a.docx").as_deref(),
            Some("C:\\docs\\a.docx")
        );
        assert_eq!(normalized("  "), None);
    }

    fn harness(dialog: LinkDialog) -> Harness<'static, (LinkDialog, Vec<LinkAction>)> {
        Harness::new_ui_state(
            |ui, state: &mut (LinkDialog, Vec<LinkAction>)| {
                if let Some(action) = state.0.ui(ui.ctx()) {
                    state.1.push(action);
                }
            },
            (dialog, Vec::new()),
        )
    }

    #[test]
    fn new_link_takes_the_address_as_text() {
        let mut dialog = LinkDialog::default();
        dialog.show(Some(String::new()), None, Vec::new());
        let mut h = harness(dialog);
        h.run();
        h.state_mut().0.address = "example.com".into();
        h.run();
        h.get_by_label("ОК").click();
        h.run();
        assert_eq!(
            h.state().1,
            [LinkAction::Apply {
                link: Hyperlink::to("http://example.com"),
                text: Some("http://example.com".into()),
            }]
        );
        assert!(!h.state().0.open);
    }

    #[test]
    fn existing_link_keeps_its_text_and_can_be_removed() {
        let mut dialog = LinkDialog::default();
        let existing = Hyperlink {
            tooltip: Some("tip".into()),
            ..Hyperlink::to_bookmark("Chapter")
        };
        dialog.show(
            Some("chapter".into()),
            Some(existing.clone()),
            vec!["Chapter".into(), "End".into()],
        );
        assert!(dialog.to_bookmark);
        assert_eq!(
            dialog.apply(),
            Some(LinkAction::Apply {
                link: existing,
                text: None
            })
        );
        let mut h = harness(dialog);
        h.run();
        h.get_by_label("Удалить ссылку").click();
        h.run();
        assert_eq!(h.state().1, [LinkAction::Remove]);
    }
}
