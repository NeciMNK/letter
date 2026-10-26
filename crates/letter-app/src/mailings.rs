//! Вкладка «Рассылки»: слияние документа с таблицей получателей (CSV или
//! таблица Word). Поля слияния вставляются в документ, «Просмотр
//! результатов» показывает в них значения записи, «Найти и объединить»
//! собирает новый документ — по копии на запись.

use eframe::egui::{RichText, Ui, vec2};

use crate::i18n::tr;
use crate::icons::{Icon, icon_button};
use crate::insert::menu_button;
use crate::keytips::{self, Level, TipAction};
use crate::ribbon::{RibbonCommand, RibbonInfo, big_label, group, row_group};

/// Подсказки клавиш вкладки (для проверки).
#[cfg(test)]
pub const KEYTIPS: [&str; 6] = [
    "keytip-mailings-recipients",
    "keytip-mailings-insert-field",
    "keytip-mailings-preview",
    "keytip-mailings-previous",
    "keytip-mailings-next",
    "keytip-mailings-finish",
];

/// Что вкладка показывает о слиянии.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailingInfo {
    /// Имена полей (столбцов) выбранных получателей.
    pub fields: Vec<String>,
    /// Сколько записей.
    pub records: usize,
    /// Показанная запись (с 0).
    pub record: usize,
    /// Включён «Просмотр результатов».
    pub preview: bool,
}

/// Большая кнопка-значок с подписью; нажатие добавляет `command`.
fn big(
    ui: &mut Ui,
    icon: Icon,
    name: &str,
    enabled: bool,
    selected: bool,
    command: RibbonCommand,
    out: &mut Vec<RibbonCommand>,
) {
    ui.vertical(|ui| {
        let label = tr(&format!("mailings-{name}"));
        let response = icon_button(ui, icon, label, vec2(44.0, 34.0), enabled, selected)
            .on_hover_text(tr(&format!("mailings-{name}-hint")));
        keytips::tip(
            ui,
            &response,
            Level::Commands,
            &format!("keytip-mailings-{name}"),
            TipAction::Command(command.clone()),
        );
        if response.clicked() {
            out.push(command);
        }
        big_label(ui, label);
    });
}

/// Вкладка «Рассылки».
pub fn mailings(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let enabled = info.has_document;
    let merge = &info.mailing;
    let has_data = enabled && !merge.fields.is_empty();
    group(ui, tr("group-mailings-start"), |ui| {
        let command = RibbonCommand::PickRecipients;
        big(
            ui,
            Icon::Recipients,
            "recipients",
            enabled,
            false,
            command,
            out,
        );
    });
    group(ui, tr("group-mailings-fields"), |ui| {
        menu_button(
            ui,
            Icon::MergeField,
            tr("mailings-insert-field"),
            tr("mailings-insert-field-hint"),
            "keytip-mailings-insert-field",
            has_data,
            |ui| {
                for name in &merge.fields {
                    if ui.button(name).clicked() {
                        out.push(RibbonCommand::InsertMergeField(name.clone()));
                        ui.close();
                    }
                }
            },
        );
    });
    row_group(ui, tr("group-mailings-preview"), |ui| {
        let command = RibbonCommand::MergePreview(!merge.preview);
        big(
            ui,
            Icon::MergePreview,
            "preview",
            has_data,
            merge.preview,
            command,
            out,
        );
        ui.vertical(|ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let can = has_data && merge.preview;
                for (name, step, text) in [("previous", -1, "<"), ("next", 1, ">")] {
                    if name == "next" {
                        let shown = if merge.records == 0 {
                            String::new()
                        } else {
                            format!("{} / {}", merge.record + 1, merge.records)
                        };
                        ui.label(RichText::new(shown).monospace());
                    }
                    let enabled = can
                        && if step < 0 {
                            merge.record > 0
                        } else {
                            merge.record + 1 < merge.records
                        };
                    let response = ui
                        .add_enabled(enabled, eframe::egui::Button::new(text))
                        .on_hover_text(tr(&format!("mailings-{name}")));
                    let command = RibbonCommand::MergeRecord(step);
                    keytips::tip(
                        ui,
                        &response,
                        Level::Commands,
                        &format!("keytip-mailings-{name}"),
                        TipAction::Command(command.clone()),
                    );
                    if response.clicked() {
                        out.push(command);
                    }
                }
            });
        });
    });
    group(ui, tr("group-mailings-finish"), |ui| {
        let command = RibbonCommand::FinishMerge;
        big(
            ui,
            Icon::FinishMerge,
            "finish",
            has_data,
            false,
            command,
            out,
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn buttons_give_commands() {
        let info = RibbonInfo {
            has_document: true,
            mailing: MailingInfo {
                fields: vec!["Имя".into(), "Город".into()],
                records: 3,
                record: 0,
                preview: true,
            },
            ..RibbonInfo::default()
        };
        let mut h = Harness::new_ui_state(
            move |ui, out: &mut Vec<RibbonCommand>| mailings(ui, &info, out),
            Vec::new(),
        );
        h.run();
        h.get_by_label("Выбрать получателей").click();
        h.run();
        h.get_by_label(">").click();
        h.run();
        h.get_by_label("Найти и объединить").click();
        h.run();
        h.get_by_label("Вставить поле слияния").click();
        h.run();
        h.get_by_label("Город").click();
        h.run();
        assert_eq!(
            *h.state(),
            [
                RibbonCommand::PickRecipients,
                RibbonCommand::MergeRecord(1),
                RibbonCommand::FinishMerge,
                RibbonCommand::InsertMergeField("Город".into()),
            ]
        );
    }
}
