//! Вкладка «Рецензирование»: группа «Примечания» — создать, удалить,
//! перейти к предыдущему и следующему, показать или скрыть область
//! примечаний.

use eframe::egui::{Ui, vec2};

use crate::i18n::tr;
use crate::icons::{Icon, icon_button};
use crate::keytips::{self, Level, TipAction};
use crate::ribbon::{RibbonCommand, RibbonInfo, group};

/// Подсказки клавиш вкладки «Рецензирование» (для проверки).
#[cfg(test)]
pub const KEYTIPS: [&str; 5] = [
    "keytip-review-new",
    "keytip-review-delete",
    "keytip-review-previous",
    "keytip-review-next",
    "keytip-review-show",
];

/// Вкладка «Рецензирование».
pub fn review(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    group(ui, tr("group-comments"), |ui| {
        // Примечание ставится в основной текст, не в колонтитул.
        let can_add = info.has_document && info.story.is_none() && info.editable;
        let buttons = [
            (
                Icon::NewComment,
                "comment-new",
                "comment-new-hint",
                "keytip-review-new",
                RibbonCommand::NewComment,
                can_add,
                false,
            ),
            (
                Icon::DeleteComment,
                "comment-delete",
                "comment-delete-hint",
                "keytip-review-delete",
                RibbonCommand::DeleteComment,
                info.active_comment && info.editable,
                false,
            ),
            (
                Icon::ChevronLeft,
                "comment-previous",
                "comment-previous-hint",
                "keytip-review-previous",
                RibbonCommand::PreviousComment,
                info.comments,
                false,
            ),
            (
                Icon::ChevronRight,
                "comment-next",
                "comment-next-hint",
                "keytip-review-next",
                RibbonCommand::NextComment,
                info.comments,
                false,
            ),
            (
                Icon::ShowComments,
                "comment-show",
                "comment-show-hint",
                "keytip-review-show",
                RibbonCommand::ToggleComments,
                info.has_document,
                info.show_comments,
            ),
        ];
        ui.horizontal(|ui| {
            for (icon, label, hint, keytip, command, enabled, selected) in buttons {
                ui.vertical(|ui| {
                    let response =
                        icon_button(ui, icon, tr(label), vec2(44.0, 34.0), enabled, selected)
                            .on_hover_text(tr(hint));
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
    });
}
