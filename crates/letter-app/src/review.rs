//! Вкладка «Рецензирование»: группы «Запись исправлений» и «Изменения»
//! (принять, отклонить, перейти к исправлению) и «Защитить»
//! («Ограничить редактирование»).

use eframe::egui::{Button, Response, Ui, vec2};
use letter_editor::Command;

use crate::i18n::tr;
use crate::keytips::{self, Level, TipAction};
use crate::ribbon::{RibbonCommand, RibbonInfo, group};

/// Подсказки клавиш вкладки «Рецензирование» (для проверки).
#[cfg(test)]
pub const KEYTIPS: [&str; 6] = [
    "keytip-review-track",
    "keytip-review-accept",
    "keytip-review-reject",
    "keytip-review-previous",
    "keytip-review-next",
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
        for (label, keytip, accept) in [
            ("revision-accept", "keytip-review-accept", true),
            ("revision-reject", "keytip-review-reject", false),
        ] {
            ui.vertical(|ui| {
                let response = ui
                    .add_enabled(active, Button::new(tr(label)).min_size(vec2(60.0, 40.0)))
                    .on_hover_text(tr(if accept {
                        "revision-accept-hint"
                    } else {
                        "revision-reject-hint"
                    }));
                let edit = if accept {
                    Command::AcceptRevisions { all: false }
                } else {
                    Command::RejectRevisions { all: false }
                };
                command(ui, response, keytip, RibbonCommand::Edit(edit), out);
                let all = if accept {
                    Command::AcceptRevisions { all: true }
                } else {
                    Command::RejectRevisions { all: true }
                };
                let label = if accept {
                    "revision-accept-all"
                } else {
                    "revision-reject-all"
                };
                if ui
                    .add_enabled(active, Button::new(tr(label)).small())
                    .clicked()
                {
                    out.push(RibbonCommand::Edit(all));
                }
            });
        }
        ui.vertical(|ui| {
            for (label, keytip, forward) in [
                ("revision-previous", "keytip-review-previous", false),
                ("revision-next", "keytip-review-next", true),
            ] {
                let response = ui.add_enabled(active, Button::new(tr(label)));
                command(
                    ui,
                    response,
                    keytip,
                    RibbonCommand::NextRevision(forward),
                    out,
                );
            }
        });
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
