//! Вкладка «Рецензирование»: группа «Защитить» («Ограничить
//! редактирование»).

use eframe::egui::{Button, Ui, vec2};

use crate::i18n::tr;
use crate::keytips::{self, Level, TipAction};
use crate::ribbon::{RibbonCommand, RibbonInfo, group};

/// Подсказки клавиш вкладки «Рецензирование» (для проверки).
#[cfg(test)]
pub const KEYTIPS: [&str; 1] = ["keytip-review-restrict"];

/// Вкладка «Рецензирование».
pub fn review(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let enabled = info.has_document && info.story.is_none();
    group(ui, tr("group-protect"), |ui| {
        let response = ui
            .add_enabled(
                enabled,
                Button::new(tr("restrict-editing")).min_size(vec2(60.0, 60.0)),
            )
            .on_hover_text(tr("restrict-editing-hint"));
        let command = RibbonCommand::RestrictEditing;
        keytips::tip(
            ui,
            &response,
            Level::Commands,
            "keytip-review-restrict",
            TipAction::Command(command.clone()),
        );
        if response.clicked() {
            out.push(command);
        }
    });
}
