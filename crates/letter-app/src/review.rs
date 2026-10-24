//! Вкладка «Рецензирование»: группы «Правописание», «Примечания»
//! (см. [`crate::comments`]), «Запись исправлений», «Изменения» (принять,
//! отклонить, перейти к исправлению), «Сравнение» и «Защитить»
//! («Ограничить редактирование»).

use eframe::egui::{Button, Response, Ui, vec2};
use letter_editor::Command;

use crate::i18n::tr;
use crate::keytips::{self, Level, TipAction};
use crate::ribbon::{RibbonCommand, RibbonInfo, group};

/// Подсказки клавиш вкладки «Рецензирование» (для проверки).
#[cfg(test)]
pub const KEYTIPS: [&str; 9] = [
    "keytip-review-spelling",
    "keytip-review-next-error",
    "keytip-review-track",
    "keytip-review-accept",
    "keytip-review-reject",
    "keytip-review-previous-change",
    "keytip-review-next-change",
    "keytip-review-compare",
    "keytip-review-restrict",
];

/// Кнопка с подсказкой клавиш: по щелчку — `command`.
fn command(
    ui: &mut Ui,
    response: Response,
    keytip: &'static str,
    command: RibbonCommand,
    out: &mut Vec<RibbonCommand>,
) {
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
}

/// Вкладка «Рецензирование».
pub fn review(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let enabled = info.has_document && info.story.is_none();
    group(ui, tr("group-proofing"), |ui| {
        ui.vertical(|ui| {
            let response = ui
                .add_enabled(
                    info.has_document,
                    Button::new(tr("spelling-check")).selected(info.spelling),
                )
                .on_hover_text(tr("spelling-check-hint"));
            command(
                ui,
                response,
                "keytip-review-spelling",
                RibbonCommand::ToggleSpelling,
                out,
            );
            let response = ui
                .add_enabled(enabled && info.spelling, Button::new(tr("spelling-next")))
                .on_hover_text(tr("spelling-next-hint"));
            command(
                ui,
                response,
                "keytip-review-next-error",
                RibbonCommand::NextSpellingError,
                out,
            );
        });
    });
    crate::comments::review(ui, info, out);
    group(ui, tr("group-tracking"), |ui| {
        let response = ui
            .add_enabled(
                enabled && !info.track_changes_forced,
                Button::new(tr("track-changes"))
                    .selected(info.track_changes)
                    .min_size(vec2(60.0, 60.0)),
            )
            .on_hover_text(tr("track-changes-hint"));
        command(
            ui,
            response,
            "keytip-review-track",
            RibbonCommand::ToggleTrackChanges,
            out,
        );
    });
    group(ui, tr("group-changes"), |ui| {
        let active = enabled && info.has_revisions;
        // Две строки: «Принять», «Отклонить», «Назад» и то же «все»
        // и «Далее» под ними.
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                let response = ui
                    .add_enabled(active, Button::new(tr("revision-accept")))
                    .on_hover_text(tr("revision-accept-hint"));
                let accept = RibbonCommand::Edit(Command::AcceptRevisions { all: false });
                command(ui, response, "keytip-review-accept", accept, out);
                let response = ui
                    .add_enabled(active, Button::new(tr("revision-reject")))
                    .on_hover_text(tr("revision-reject-hint"));
                let reject = RibbonCommand::Edit(Command::RejectRevisions { all: false });
                command(ui, response, "keytip-review-reject", reject, out);
                let response = ui.add_enabled(active, Button::new(tr("revision-previous")));
                let previous = RibbonCommand::NextRevision(false);
                command(ui, response, "keytip-review-previous-change", previous, out);
            });
            ui.horizontal(|ui| {
                for (label, all) in [
                    (
                        "revision-accept-all",
                        Command::AcceptRevisions { all: true },
                    ),
                    (
                        "revision-reject-all",
                        Command::RejectRevisions { all: true },
                    ),
                ] {
                    if ui.add_enabled(active, Button::new(tr(label))).clicked() {
                        out.push(RibbonCommand::Edit(all));
                    }
                }
                let response = ui.add_enabled(active, Button::new(tr("revision-next")));
                let next = RibbonCommand::NextRevision(true);
                command(ui, response, "keytip-review-next-change", next, out);
            });
        });
    });
    group(ui, tr("group-compare"), |ui| {
        let response = ui
            .add_enabled(
                true,
                Button::new(tr("compare-documents")).min_size(vec2(60.0, 60.0)),
            )
            .on_hover_text(tr("compare-documents-hint"));
        command(
            ui,
            response,
            "keytip-review-compare",
            RibbonCommand::CompareDocuments,
            out,
        );
    });
    group(ui, tr("group-protect"), |ui| {
        let response = ui
            .add_enabled(
                enabled,
                Button::new(tr("restrict-editing")).min_size(vec2(60.0, 60.0)),
            )
            .on_hover_text(tr("restrict-editing-hint"));
        command(
            ui,
            response,
            "keytip-review-restrict",
            RibbonCommand::RestrictEditing,
            out,
        );
    });
}
