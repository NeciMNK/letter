//! Вкладка «Рассылки»: слияние документа с таблицей получателей (CSV или
//! таблица Word). Поля слияния вставляются в документ, «Просмотр
//! результатов» показывает в них значения записи, «Найти и объединить»
//! собирает новый документ — по копии на запись.

use eframe::egui::{RichText, Ui, vec2};
use letter_editor::MergeRule;

use crate::i18n::tr;
use crate::icons::{Icon, icon_button};
use crate::insert::menu_button;
use crate::keytips::{self, Level, TipAction};
use crate::merge_dialogs::RuleKind;
use crate::ribbon::{RibbonCommand, RibbonInfo, big_label, group, row_group};

/// Подсказки клавиш вкладки (для проверки).
#[cfg(test)]
pub const KEYTIPS: [&str; 12] = [
    "keytip-mailings-labels",
    "keytip-mailings-update-labels",
    "keytip-mailings-recipients",
    "keytip-mailings-edit-list",
    "keytip-mailings-address",
    "keytip-mailings-greeting",
    "keytip-mailings-rules",
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
    /// Документ — лист наклеек (есть поля «Следующая запись»).
    pub labels: bool,
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

/// Название листа наклеек в меню: марка, размер наклейки и сколько их.
fn label_name(spec: &letter_editor::LabelSpec) -> String {
    let mm = |t: letter_model::Twips| {
        let text = format!("{:.1}", t.to_mm());
        if crate::i18n::language() == crate::i18n::Language::Russian {
            text.replace('.', ",")
        } else {
            text
        }
    };
    format!(
        "{} — {} × {} {}, {} × {}",
        spec.name,
        mm(spec.width),
        mm(spec.height),
        tr("unit-mm"),
        spec.columns,
        spec.rows
    )
}

/// Вкладка «Рассылки».
pub fn mailings(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let enabled = info.has_document;
    let merge = &info.mailing;
    let has_data = enabled && !merge.fields.is_empty();
    row_group(ui, tr("group-mailings-start"), |ui| {
        menu_button(
            ui,
            Icon::Labels,
            tr("mailings-labels"),
            tr("mailings-labels-hint"),
            "keytip-mailings-labels",
            enabled,
            |ui| {
                for (index, spec) in letter_editor::LABELS.iter().enumerate() {
                    if ui.button(label_name(spec)).clicked() {
                        out.push(RibbonCommand::MakeLabels(index));
                        ui.close();
                    }
                }
            },
        );
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
        let command = RibbonCommand::EditRecipients;
        big(
            ui,
            Icon::EditRecipients,
            "edit-list",
            has_data,
            false,
            command,
            out,
        );
    });
    row_group(ui, tr("group-mailings-fields"), |ui| {
        let command = RibbonCommand::InsertMergeRule(MergeRule::AddressBlock);
        big(
            ui,
            Icon::AddressBlock,
            "address",
            has_data,
            false,
            command,
            out,
        );
        let russian = crate::i18n::language() == crate::i18n::Language::Russian;
        let command = RibbonCommand::InsertMergeRule(MergeRule::GreetingLine { russian });
        big(
            ui,
            Icon::GreetingLine,
            "greeting",
            has_data,
            false,
            command,
            out,
        );
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
        menu_button(
            ui,
            Icon::MergeRules,
            tr("mailings-rules"),
            tr("mailings-rules-hint"),
            "keytip-mailings-rules",
            has_data,
            |ui| {
                let items = [
                    ("rule-if", RibbonCommand::RuleDialog(RuleKind::If)),
                    (
                        "rule-record-number",
                        RibbonCommand::InsertMergeRule(MergeRule::RecordNumber),
                    ),
                    ("rule-skip", RibbonCommand::RuleDialog(RuleKind::SkipIf)),
                ];
                for (name, command) in items {
                    if ui.button(tr(name)).clicked() {
                        out.push(command);
                        ui.close();
                    }
                }
            },
        );
        let command = RibbonCommand::UpdateLabels;
        big(
            ui,
            Icon::Refresh,
            "update-labels",
            enabled && merge.labels,
            false,
            command,
            out,
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
        menu_button(
            ui,
            Icon::FinishMerge,
            tr("mailings-finish"),
            tr("mailings-finish-hint"),
            "keytip-mailings-finish",
            has_data,
            |ui| {
                let items = [
                    ("mailings-edit-documents", RibbonCommand::FinishMerge),
                    ("mailings-pdf", RibbonCommand::MergeToPdf),
                ];
                for (name, command) in items {
                    if ui.button(tr(name)).clicked() {
                        out.push(command);
                        ui.close();
                    }
                }
            },
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
                labels: true,
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
        h.get_all_by_label("Найти и объединить")
            .next()
            .unwrap()
            .click();
        h.run();
        h.get_by_label("Изменить отдельные документы…").click();
        h.run();
        h.get_by_label("Вставить поле слияния").click();
        h.run();
        h.get_by_label("Город").click();
        h.run();
        h.get_by_label("Блок адреса").click();
        h.run();
        h.get_all_by_label("Правила").next().unwrap().click();
        h.run();
        h.get_by_label("Пропустить запись, если…").click();
        h.run();
        h.get_by_label("Обновить наклейки").click();
        h.run();
        h.get_all_by_label("Наклейки").next().unwrap().click();
        h.run();
        h.get_by_label("Avery L7163 — 99,1 × 38,1 мм, 2 × 7")
            .click();
        h.run();
        assert_eq!(
            *h.state(),
            [
                RibbonCommand::PickRecipients,
                RibbonCommand::MergeRecord(1),
                RibbonCommand::FinishMerge,
                RibbonCommand::InsertMergeField("Город".into()),
                RibbonCommand::InsertMergeRule(MergeRule::AddressBlock),
                RibbonCommand::RuleDialog(RuleKind::SkipIf),
                RibbonCommand::UpdateLabels,
                RibbonCommand::MakeLabels(1),
            ]
        );
    }
}
