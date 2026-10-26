//! Окно «Свойства элемента управления содержимым» («Разработчик →
//! Свойства»): название, тег, запреты удаления и правки, пункты списка,
//! образец даты. Окно только собирает новый элемент; документ меняет
//! просмотрщик.

use eframe::egui::{self, Context, Key, TextEdit};
use letter_editor::{Position, format_date};
use letter_model::{ContentControl, ControlKind, ControlLock, ListItem};

use crate::i18n::tr;

/// Образцы даты в списке окна (обозначения Word).
const DATE_FORMATS: [&str; 7] = [
    "dd.MM.yyyy",
    "dd.MM.yy",
    "d MMMM yyyy",
    "dddd, d MMMM yyyy",
    "yyyy-MM-dd",
    "M/d/yyyy",
    "MMMM d, yyyy",
];

/// Состояние окна.
#[derive(Debug, Clone, Default)]
pub struct ControlDialog {
    /// Окно открыто.
    pub open: bool,
    /// Место элемента и сам элемент, как был.
    control: Option<(Position, ContentControl)>,
    /// Название (`w:alias`).
    pub title: String,
    /// Тег (`w:tag`).
    pub tag: String,
    /// «Элемент нельзя удалить».
    pub lock_control: bool,
    /// «Содержимое нельзя редактировать».
    pub lock_content: bool,
    /// Пункты списка (у списков).
    pub items: Option<Vec<ListItem>>,
    /// Выбранный пункт.
    chosen: usize,
    /// Образец даты (у выбора даты).
    pub date_format: Option<String>,
}

impl ControlDialog {
    /// Открывает окно для элемента `control` у места `at`.
    pub fn show(&mut self, at: Position, control: &ContentControl) {
        let lock = control.lock();
        let (items, date_format) = match control.kind() {
            ControlKind::List { items, .. } => (Some(items), None),
            ControlKind::Date { format } => (None, Some(format.unwrap_or_default())),
            _ => (None, None),
        };
        *self = ControlDialog {
            open: true,
            control: Some((at, control.clone())),
            title: control.title().unwrap_or_default(),
            tag: control.tag().unwrap_or_default(),
            lock_control: matches!(lock, ControlLock::Control | ControlLock::Both),
            lock_content: matches!(lock, ControlLock::Content | ControlLock::Both),
            items,
            chosen: 0,
            date_format,
        };
    }

    /// Элемент с новыми свойствами.
    fn result(&self) -> Option<(Position, ContentControl)> {
        let (at, control) = self.control.as_ref()?;
        let lock = match (self.lock_control, self.lock_content) {
            (false, false) => ControlLock::None,
            (true, false) => ControlLock::Control,
            (false, true) => ControlLock::Content,
            (true, true) => ControlLock::Both,
        };
        let mut new = control.with_settings(self.title.trim(), self.tag.trim(), lock);
        if let Some(items) = &self.items {
            new = new.with_list_items(items);
        }
        if let Some(format) = &self.date_format
            && !format.trim().is_empty()
        {
            new = new.with_date_format(format.trim());
        }
        Some((*at, new))
    }

    /// Рисует окно и возвращает место и элемент с новыми свойствами
    /// (после «ОК»).
    pub fn ui(&mut self, ctx: &Context) -> Option<(Position, ContentControl)> {
        if !self.open {
            return None;
        }
        let mut done = None;
        let mut close = false;
        let mut open = true;
        egui::Window::new(tr("control-dialog-title"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Grid::new("control-dialog-names")
                    .num_columns(2)
                    .show(ui, |ui| {
                        let label = ui.label(tr("control-dialog-name"));
                        ui.add(TextEdit::singleline(&mut self.title).desired_width(260.0))
                            .labelled_by(label.id);
                        ui.end_row();
                        let label = ui.label(tr("control-dialog-tag"));
                        ui.add(TextEdit::singleline(&mut self.tag).desired_width(260.0))
                            .labelled_by(label.id);
                        ui.end_row();
                    });
                ui.separator();
                ui.label(egui::RichText::new(tr("control-dialog-locking")).strong());
                ui.checkbox(&mut self.lock_control, tr("control-dialog-lock-control"));
                ui.checkbox(&mut self.lock_content, tr("control-dialog-lock-content"));
                if let Some(items) = &mut self.items {
                    ui.separator();
                    ui.label(egui::RichText::new(tr("control-dialog-items")).strong());
                    list_editor(ui, items, &mut self.chosen);
                }
                if let Some(format) = &mut self.date_format {
                    ui.separator();
                    ui.label(egui::RichText::new(tr("control-dialog-date")).strong());
                    date_editor(ui, format);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(tr("ok")).clicked() {
                        done = self.result();
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
}

/// Пункты списка: выбор пункта, его текст и значение, кнопки
/// «Добавить», «Удалить», «Вверх», «Вниз».
fn list_editor(ui: &mut egui::Ui, items: &mut Vec<ListItem>, chosen: &mut usize) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        egui::ScrollArea::vertical()
            .max_height(120.0)
            .min_scrolled_height(120.0)
            .show(ui, |ui| {
                ui.set_min_width(330.0);
                for (index, item) in items.iter().enumerate() {
                    let shown = if item.text.is_empty() {
                        " "
                    } else {
                        item.text.as_str()
                    };
                    if ui.selectable_label(index == *chosen, shown).clicked() {
                        *chosen = index;
                    }
                }
            });
    });
    if let Some(item) = items.get_mut(*chosen) {
        egui::Grid::new("control-dialog-item")
            .num_columns(2)
            .show(ui, |ui| {
                let label = ui.label(tr("control-dialog-item-text"));
                ui.add(TextEdit::singleline(&mut item.text).desired_width(220.0))
                    .labelled_by(label.id);
                ui.end_row();
                let label = ui.label(tr("control-dialog-item-value"));
                ui.add(TextEdit::singleline(&mut item.value).desired_width(220.0))
                    .labelled_by(label.id);
                ui.end_row();
            });
    }
    ui.horizontal(|ui| {
        if ui.button(tr("control-dialog-add")).clicked() {
            let text = format!("{} {}", tr("control-dialog-new-item"), items.len() + 1);
            items.push(ListItem {
                value: text.clone(),
                text,
            });
            *chosen = items.len() - 1;
        }
        let some = *chosen < items.len();
        if ui
            .add_enabled(some, egui::Button::new(tr("control-dialog-remove")))
            .clicked()
        {
            items.remove(*chosen);
            *chosen = (*chosen).min(items.len().saturating_sub(1));
        }
        if ui
            .add_enabled(
                some && *chosen > 0,
                egui::Button::new(tr("control-dialog-up")),
            )
            .clicked()
        {
            items.swap(*chosen, *chosen - 1);
            *chosen -= 1;
        }
        if ui
            .add_enabled(
                *chosen + 1 < items.len(),
                egui::Button::new(tr("control-dialog-down")),
            )
            .clicked()
        {
            items.swap(*chosen, *chosen + 1);
            *chosen += 1;
        }
    });
}

/// Образец даты: поле, готовые образцы и пример на сегодня.
fn date_editor(ui: &mut egui::Ui, format: &mut String) {
    let russian = crate::i18n::language() == crate::i18n::Language::Russian;
    let (today, _) = crate::fields::now();
    let label = ui.label(tr("control-dialog-date-format"));
    ui.add(TextEdit::singleline(format).desired_width(330.0))
        .labelled_by(label.id);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_width(330.0);
        for pattern in DATE_FORMATS {
            let sample = format_date(pattern, today, russian);
            if ui.selectable_label(format == pattern, sample).clicked() {
                *format = pattern.to_string();
            }
        }
    });
    if !format.trim().is_empty() {
        ui.label(format!(
            "{} {}",
            tr("control-dialog-sample"),
            format_date(format.trim(), today, russian)
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use letter_model::RawXml;

    fn control(props: &str) -> ContentControl {
        ContentControl {
            props: Some(RawXml::new(format!(
                r#"<w:sdtPr xmlns:w="urn:w">{props}</w:sdtPr>"#
            ))),
            end_props: None,
        }
    }

    type State = (ControlDialog, Option<(Position, ContentControl)>);

    fn harness(dialog: ControlDialog) -> Harness<'static, State> {
        Harness::new_ui_state(
            |ui, state: &mut State| {
                if let Some(done) = state.0.ui(ui.ctx()) {
                    state.1 = Some(done);
                }
            },
            (dialog, None),
        )
    }

    #[test]
    fn list_items_are_edited() {
        let list = control(
            r#"<w:alias w:val="Отдел"/><w:id w:val="3"/><w:dropDownList><w:listItem w:displayText="Склад" w:value="1"/><w:listItem w:displayText="Офис" w:value="2"/></w:dropDownList>"#,
        );
        let mut dialog = ControlDialog::default();
        let at = Position::new(0, 4);
        dialog.show(at, &list);
        assert_eq!(dialog.title, "Отдел");
        let mut h = harness(dialog);
        h.run();
        h.get_by_label("Офис").click();
        h.run();
        h.get_by_label("Вверх").click();
        h.run();
        h.get_by_label("Добавить").click();
        h.run();
        h.get_by_label("Элемент управления содержимым нельзя удалить")
            .click();
        h.run();
        h.state_mut().0.tag = "dept".into();
        h.run();
        h.get_by_label("ОК").click();
        h.run();
        let (place, new) = h.state().1.clone().expect("ОК даёт элемент");
        assert_eq!(place, at);
        assert!(!h.state().0.open);
        assert_eq!(new.title().as_deref(), Some("Отдел"));
        assert_eq!(new.tag().as_deref(), Some("dept"));
        assert_eq!(new.lock(), ControlLock::Control);
        let ControlKind::List { items, .. } = new.kind() else {
            panic!("ждали список");
        };
        let texts: Vec<_> = items.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(texts, ["Офис", "Склад", "Новый пункт 3"]);
    }

    #[test]
    fn date_format_is_chosen() {
        let date = control(r#"<w:date><w:dateFormat w:val="dd.MM.yyyy"/></w:date>"#);
        let mut dialog = ControlDialog::default();
        dialog.show(Position::new(0, 0), &date);
        let mut h = harness(dialog);
        h.run();
        h.state_mut().0.date_format = Some("yyyy-MM-dd".into());
        h.run();
        h.get_by_label("ОК").click();
        h.run();
        let (_, new) = h.state().1.clone().expect("ОК даёт элемент");
        assert_eq!(
            new.kind(),
            ControlKind::Date {
                format: Some("yyyy-MM-dd".into())
            }
        );
        // «Отмена» ничего не даёт.
        let mut dialog = ControlDialog::default();
        dialog.show(Position::new(0, 0), &date);
        let mut h = harness(dialog);
        h.run();
        h.get_by_label("Отмена").click();
        h.run();
        assert!(h.state().1.is_none());
        assert!(!h.state().0.open);
    }
}
