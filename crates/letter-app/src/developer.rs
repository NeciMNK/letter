//! Вкладка «Разработчик»: вставка элементов управления содержимым
//! (текст, флажок, списки, дата) и «Ограничить редактирование».

use eframe::egui::{Button, Ui, vec2};
use letter_editor::NewControl;

use crate::i18n::tr;
use crate::icons::{Icon, icon_text_button};
use crate::keytips::{self, Level, TipAction};
use crate::ribbon::{RibbonCommand, RibbonInfo, group};

/// Подсказки клавиш вкладки (для проверки).
#[cfg(test)]
pub const KEYTIPS: [&str; 9] = [
    "keytip-developer-rich-text",
    "keytip-developer-plain-text",
    "keytip-developer-checkbox",
    "keytip-developer-combo-box",
    "keytip-developer-drop-down",
    "keytip-developer-date",
    "keytip-developer-properties",
    "keytip-developer-design-mode",
    "keytip-developer-restrict",
];

/// Кнопки группы «Элементы управления»: значок, имя (подпись,
/// подсказка и подсказка клавиш — `control-…`, `keytip-developer-…`)
/// и вид элемента.
const CONTROLS: [(Icon, &str, NewControl); 6] = [
    (Icon::ControlRichText, "rich-text", NewControl::RichText),
    (Icon::ControlPlainText, "plain-text", NewControl::PlainText),
    (Icon::ControlCheckBox, "checkbox", NewControl::CheckBox),
    (Icon::ControlComboBox, "combo-box", NewControl::ComboBox),
    (Icon::ControlDropDown, "drop-down", NewControl::DropDownList),
    (Icon::ControlDate, "date", NewControl::Date),
];

/// Вкладка «Разработчик».
pub fn developer(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let enabled = info.has_document;
    group(ui, tr("group-developer-controls"), |ui| {
        ui.horizontal_top(|ui| {
            for column in CONTROLS.chunks(3) {
                ui.vertical(|ui| {
                    for &(icon, name, kind) in column {
                        let response =
                            icon_text_button(ui, icon, tr(&format!("control-{name}")), enabled)
                                .on_hover_text(tr(&format!("control-{name}-hint")));
                        let command = RibbonCommand::InsertControl(kind);
                        keytips::tip(
                            ui,
                            &response,
                            Level::Commands,
                            &format!("keytip-developer-{name}"),
                            TipAction::Command(command.clone()),
                        );
                        if response.clicked() {
                            out.push(command);
                        }
                    }
                });
            }
            ui.vertical(|ui| {
                let response = ui
                    .add_enabled(
                        enabled && info.in_control,
                        Button::new(tr("control-properties")),
                    )
                    .on_hover_text(tr("control-properties-hint"));
                keytips::tip(
                    ui,
                    &response,
                    Level::Commands,
                    "keytip-developer-properties",
                    TipAction::Command(RibbonCommand::ControlProperties),
                );
                if response.clicked() {
                    out.push(RibbonCommand::ControlProperties);
                }
                let response = ui
                    .add_enabled(
                        enabled,
                        Button::new(tr("design-mode")).selected(info.design_mode),
                    )
                    .on_hover_text(tr("design-mode-hint"));
                keytips::tip(
                    ui,
                    &response,
                    Level::Commands,
                    "keytip-developer-design-mode",
                    TipAction::Command(RibbonCommand::DesignMode),
                );
                if response.clicked() {
                    out.push(RibbonCommand::DesignMode);
                }
            });
        });
    });
    group(ui, tr("group-protect"), |ui| {
        let response = ui
            .add_enabled(
                enabled,
                Button::new(tr("restrict-editing")).min_size(vec2(60.0, 60.0)),
            )
            .on_hover_text(tr("restrict-editing-hint"));
        keytips::tip(
            ui,
            &response,
            Level::Commands,
            "keytip-developer-restrict",
            TipAction::Command(RibbonCommand::RestrictEditing),
        );
        if response.clicked() {
            out.push(RibbonCommand::RestrictEditing);
        }
    });
}

/// Подсказки, которые снова появляются в стёртом элементе (на языке
/// интерфейса).
pub fn placeholder_texts() -> letter_editor::PlaceholderTexts {
    letter_editor::PlaceholderTexts {
        text: tr("control-placeholder-text").into(),
        list: tr("control-placeholder-list").into(),
        date: tr("control-placeholder-date").into(),
    }
}

/// Текст-подсказка нового элемента вида `kind` (на языке интерфейса).
pub fn placeholder(kind: NewControl) -> &'static str {
    match kind {
        NewControl::ComboBox | NewControl::DropDownList => tr("control-placeholder-list"),
        NewControl::Date => tr("control-placeholder-date"),
        _ => tr("control-placeholder-text"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn buttons_insert_controls() {
        let info = RibbonInfo {
            has_document: true,
            in_control: true,
            ..RibbonInfo::default()
        };
        let mut h = Harness::new_ui_state(
            move |ui, out: &mut Vec<RibbonCommand>| developer(ui, &info, out),
            Vec::new(),
        );
        h.run();
        h.get_by_label("Флажок").click();
        h.run();
        h.get_by_label("Выбор даты").click();
        h.run();
        h.get_by_label("Свойства").click();
        h.run();
        h.get_by_label("Режим конструктора").click();
        h.run();
        assert_eq!(
            *h.state(),
            [
                RibbonCommand::InsertControl(NewControl::CheckBox),
                RibbonCommand::InsertControl(NewControl::Date),
                RibbonCommand::ControlProperties,
                RibbonCommand::DesignMode,
            ]
        );
    }
}
