//! Вкладка «Ссылки»: пока — группа «Сноски» (вставить сноску или
//! концевую сноску у курсора).

use eframe::egui::{Ui, vec2};
use letter_editor::Command;
use letter_model::NoteKind;

use crate::i18n::tr;
use crate::icons::{Icon, icon_button};
use crate::keytips::{self, Level, TipAction};
use crate::ribbon::{RibbonCommand, RibbonInfo, group};

/// Подсказки клавиш вкладки «Ссылки» (для проверки).
#[cfg(test)]
pub const KEYTIPS: [&str; 2] = ["keytip-references-footnote", "keytip-references-endnote"];

/// Вкладка «Ссылки».
pub fn references(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    // В колонтитул сноску не вставить, как и в Word.
    let enabled = info.has_document && info.story.is_none();
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
