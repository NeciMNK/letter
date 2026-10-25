//! Вкладка «Рецензирование»:
//!
//! - «Примечания» — создать, удалить, перейти к предыдущему и следующему,
//!   показать или скрыть область примечаний;
//! - «Запись исправлений» — включить или выключить запись правки;
//! - «Изменения» — принять или отклонить исправления, перейти к
//!   предыдущему и следующему.

use eframe::egui::{Ui, vec2};
use letter_editor::Command;

use crate::i18n::tr;
use crate::icons::{Icon, icon_button};
use crate::keytips::{self, Level, TipAction};
use crate::ribbon::{RibbonCommand, RibbonInfo, group};

/// Подсказки клавиш вкладки «Рецензирование» (для проверки).
#[cfg(test)]
pub const KEYTIPS: [&str; 12] = [
    "keytip-review-new",
    "keytip-review-delete",
    "keytip-review-previous",
    "keytip-review-next",
    "keytip-review-show",
    "keytip-review-track",
    "keytip-review-accept",
    "keytip-review-accept-all",
    "keytip-review-reject",
    "keytip-review-reject-all",
    "keytip-review-revision-previous",
    "keytip-review-revision-next",
];

/// Кнопка группы: значок, подпись, подсказка, буквы, команда, доступна
/// ли, нажата ли.
type Button = (
    Icon,
    &'static str,
    &'static str,
    &'static str,
    RibbonCommand,
    bool,
    bool,
);

/// Ряд крупных кнопок с подписями.
fn buttons(ui: &mut Ui, buttons: Vec<Button>, out: &mut Vec<RibbonCommand>) {
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
}

/// Вкладка «Рецензирование».
pub fn review(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    group(ui, tr("group-comments"), |ui| {
        // Примечание ставится в основной текст, не в колонтитул.
        let can_add = info.has_document && info.story.is_none() && info.editable;
        let list = vec![
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
        buttons(ui, list, out);
    });
    group(ui, tr("group-tracking"), |ui| {
        let list = vec![(
            Icon::TrackChanges,
            "revision-track",
            "revision-track-hint",
            "keytip-review-track",
            RibbonCommand::ToggleTrackRevisions,
            info.has_document && info.editable,
            info.track_revisions,
        )];
        buttons(ui, list, out);
    });
    group(ui, tr("group-changes"), |ui| {
        let can_resolve = info.revisions && info.editable;
        let resolve = |accept, all| RibbonCommand::Edit(Command::ResolveRevisions { accept, all });
        let list = vec![
            (
                Icon::Accept,
                "revision-accept",
                "revision-accept-hint",
                "keytip-review-accept",
                resolve(true, false),
                can_resolve,
                false,
            ),
            (
                Icon::AcceptAll,
                "revision-accept-all",
                "revision-accept-all-hint",
                "keytip-review-accept-all",
                resolve(true, true),
                can_resolve,
                false,
            ),
            (
                Icon::Reject,
                "revision-reject",
                "revision-reject-hint",
                "keytip-review-reject",
                resolve(false, false),
                can_resolve,
                false,
            ),
            (
                Icon::RejectAll,
                "revision-reject-all",
                "revision-reject-all-hint",
                "keytip-review-reject-all",
                resolve(false, true),
                can_resolve,
                false,
            ),
            (
                Icon::ChevronLeft,
                "revision-previous",
                "revision-previous-hint",
                "keytip-review-revision-previous",
                RibbonCommand::GoToRevision { forward: false },
                info.revisions,
                false,
            ),
            (
                Icon::ChevronRight,
                "revision-next",
                "revision-next-hint",
                "keytip-review-revision-next",
                RibbonCommand::GoToRevision { forward: true },
                info.revisions,
                false,
            ),
        ];
        buttons(ui, list, out);
    });
}
