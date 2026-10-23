//! Вкладка «Ссылки»: группы «Оглавление» (вставить и обновить) и
//! «Сноски» (вставить сноску или концевую сноску у курсора).

use eframe::egui::{Ui, vec2};
use letter_editor::Command;
use letter_model::NoteKind;

use crate::i18n::tr;
use crate::icons::{Icon, icon_button, icon_text_button};
use crate::insert::menu_button;
use crate::keytips::{self, Level, TipAction};
use crate::ribbon::{RibbonCommand, RibbonInfo, group};

/// Подсказки клавиш вкладки «Ссылки» (для проверки).
#[cfg(test)]
pub const KEYTIPS: [&str; 4] = [
    "keytip-references-toc",
    "keytip-references-update-toc",
    "keytip-references-footnote",
    "keytip-references-endnote",
];

/// Вкладка «Ссылки».
pub fn references(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    // В колонтитул сноску не вставить, как и в Word.
    let enabled = info.has_document && info.story.is_none();
    group(ui, tr("group-toc"), |ui| {
        menu_button(
            ui,
            Icon::Contents,
            tr("toc"),
            tr("toc-hint"),
            "keytip-references-toc",
            enabled,
            |ui| {
                if ui.button(tr("toc-automatic")).clicked() {
                    out.push(RibbonCommand::InsertToc);
                    ui.close();
                }
            },
        );
        ui.vertical(|ui| {
            let response = icon_text_button(ui, Icon::Refresh, tr("toc-update"), enabled)
                .on_hover_text(tr("toc-update-hint"));
            let command = RibbonCommand::UpdateToc;
            keytips::tip(
                ui,
                &response,
                Level::Commands,
                "keytip-references-update-toc",
                TipAction::Command(command.clone()),
            );
            if response.clicked() {
                out.push(command);
            }
        });
    });
    group(ui, tr("group-footnotes"), |ui| {
        let buttons = [
            (
                NoteKind::Footnote,
                Icon::Superscript,
                "insert-footnote",
                "insert-footnote-hint",
                "keytip-references-footnote",
            ),
            (
                NoteKind::Endnote,
                Icon::Endnote,
                "insert-endnote",
                "insert-endnote-hint",
                "keytip-references-endnote",
            ),
        ];
        for (kind, icon, label, hint, keytip) in buttons {
            ui.vertical(|ui| {
                let response = icon_button(ui, icon, tr(label), vec2(44.0, 34.0), enabled, false)
                    .on_hover_text(tr(hint));
                let command = RibbonCommand::Edit(Command::InsertNote(kind));
                keytips::tip(
                    ui,
                    &response,
                    Level::Commands,
                    keytip,
                    TipAction::Command(command.clone()),
                );
                if response.clicked() {
                    out.push(command);
                }
                ui.label(tr(label));
            });
        }
    });
}
