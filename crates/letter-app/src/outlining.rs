//! Вкладка «Структура» (видна в режиме «Структура»), как в Word:
//! повысить и понизить уровень, сделать обычным текстом, развернуть
//! и свернуть заголовок, «Показать уровень» и «Закрыть режим структуры».

use eframe::egui::{self, Ui};
use letter_editor::{Command, OutlineChange};

use crate::i18n::{tr, tr_args};
use crate::icons::{Icon, icon_text_button};
use crate::keytips::{self, Level, TipAction};
use crate::ribbon::{RibbonCommand, RibbonInfo, group};
use crate::view::ViewMode;

/// Подсказки клавиш вкладки (для проверки).
#[cfg(test)]
pub const KEYTIPS: [&str; 7] = [
    "keytip-outline-promote",
    "keytip-outline-demote",
    "keytip-outline-body",
    "keytip-outline-expand",
    "keytip-outline-collapse",
    "keytip-outline-level",
    "keytip-outline-close",
];

/// Вкладка «Структура».
pub fn outlining(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let enabled = info.has_document;
    group(ui, tr("group-outline-tools"), |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                let edit = |change| RibbonCommand::Edit(Command::OutlineLevel(change));
                let buttons = [
                    (
                        Icon::ChevronLeft,
                        "outline-promote",
                        "keytip-outline-promote",
                        edit(OutlineChange::Promote),
                    ),
                    (
                        Icon::ChevronRight,
                        "outline-demote",
                        "keytip-outline-demote",
                        edit(OutlineChange::Demote),
                    ),
                    (
                        Icon::Pilcrow,
                        "outline-body",
                        "keytip-outline-body",
                        edit(OutlineChange::Body),
                    ),
                ];
                for (icon, label, keytip, command) in buttons {
                    let response = icon_text_button(ui, icon, tr(label), enabled);
                    tip(ui, &response, keytip, command, out);
                }
            });
            ui.vertical(|ui| {
                let response =
                    icon_text_button(ui, Icon::ChevronDown, tr("outline-expand"), enabled);
                tip(
                    ui,
                    &response,
                    "keytip-outline-expand",
                    RibbonCommand::OutlineExpand(true),
                    out,
                );
                let response =
                    icon_text_button(ui, Icon::ChevronRight, tr("outline-collapse"), enabled);
                tip(
                    ui,
                    &response,
                    "keytip-outline-collapse",
                    RibbonCommand::OutlineExpand(false),
                    out,
                );
                let name = |level: Option<u8>| match level {
                    None => tr("outline-all-levels").to_owned(),
                    Some(n) => tr_args("outline-level-n", &[("level", u32::from(n).into())]),
                };
                ui.horizontal(|ui| {
                    ui.label(tr("outline-show-level"));
                    let response = egui::ComboBox::from_id_salt("outline-show-level")
                        .selected_text(name(info.show_level))
                        .show_ui(ui, |ui| {
                            for level in std::iter::once(None).chain((1..=9).map(Some)) {
                                if ui
                                    .selectable_label(info.show_level == level, name(level))
                                    .clicked()
                                {
                                    out.push(RibbonCommand::ShowLevel(level));
                                }
                            }
                        })
                        .response;
                    let popup = egui::Popup::default_response_id(&response);
                    keytips::tip(
                        ui,
                        &response,
                        Level::Commands,
                        "keytip-outline-level",
                        TipAction::Popup(popup),
                    );
                });
            });
        });
    });
    group(ui, tr("group-outline-close"), |ui| {
        let response = icon_text_button(ui, Icon::Close, tr("outline-close"), true);
        tip(
            ui,
            &response,
            "keytip-outline-close",
            RibbonCommand::ViewMode(ViewMode::Print),
            out,
        );
    });
}

/// Подсказка клавиши у кнопки и её команда по щелчку.
fn tip(
    ui: &mut Ui,
    response: &egui::Response,
    keytip: &str,
    command: RibbonCommand,
    out: &mut Vec<RibbonCommand>,
) {
    keytips::tip(
        ui,
        response,
        Level::Commands,
        keytip,
        TipAction::Command(command.clone()),
    );
    if response.clicked() {
        out.push(command);
    }
}
