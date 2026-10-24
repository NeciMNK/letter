//! Вкладка «Ссылки»: группы «Оглавление» (вставить, удалить, обновить,
//! «Добавить текст» — уровень абзаца в оглавлении) и «Сноски» (вставить
//! сноску или концевую сноску у курсора, перейти к следующей).

use eframe::egui::{self, Ui, vec2};
use letter_editor::{Command, OutlineChange};
use letter_model::NoteKind;

use crate::i18n::tr;
use crate::icons::{Icon, icon_button, icon_text_button};
use crate::insert::menu_button;
use crate::keytips::{self, Level, TipAction};
use crate::ribbon::{RibbonCommand, RibbonInfo, group};

/// Подсказки клавиш вкладки «Ссылки» (для проверки).
#[cfg(test)]
pub const KEYTIPS: [&str; 6] = [
    "keytip-references-toc",
    "keytip-references-add-text",
    "keytip-references-update-toc",
    "keytip-references-footnote",
    "keytip-references-endnote",
    "keytip-references-next-note",
];

/// Маленькая кнопка с подписью, открывающая меню.
fn small_menu(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    hint: &str,
    key: &str,
    enabled: bool,
    menu: impl FnOnce(&mut Ui),
) {
    let response = icon_text_button(ui, icon, label, enabled).on_hover_text(hint);
    let popup = egui::Popup::default_response_id(&response);
    keytips::tip(ui, &response, Level::Commands, key, TipAction::Popup(popup));
    egui::Popup::menu(&response).show(menu);
}

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
                ui.separator();
                if ui.button(tr("toc-delete")).clicked() {
                    out.push(RibbonCommand::Edit(Command::DeleteToc));
                    ui.close();
                }
            },
        );
        ui.vertical(|ui| {
            small_menu(
                ui,
                Icon::Pilcrow,
                tr("toc-add-text"),
                tr("toc-add-text-hint"),
                "keytip-references-add-text",
                enabled,
                |ui| {
                    let items = [
                        ("toc-add-text-body", OutlineChange::Body),
                        ("toc-add-text-level-1", OutlineChange::Level(1)),
                        ("toc-add-text-level-2", OutlineChange::Level(2)),
                        ("toc-add-text-level-3", OutlineChange::Level(3)),
                    ];
                    for (label, change) in items {
                        let current = match change {
                            OutlineChange::Level(level) => info.outline_level == Some(level),
                            _ => info.outline_level.is_none(),
                        };
                        if ui
                            .add(egui::Button::selectable(current, tr(label)))
                            .clicked()
                        {
                            out.push(RibbonCommand::Edit(Command::OutlineLevel(change)));
                            ui.close();
                        }
                    }
                },
            );
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
        small_menu(
            ui,
            Icon::ChevronRight,
            tr("next-footnote"),
            tr("next-footnote-hint"),
            "keytip-references-next-note",
            info.has_document,
            |ui| {
                let items = [
                    ("next-footnote", NoteKind::Footnote, true),
                    ("previous-footnote", NoteKind::Footnote, false),
                    ("next-endnote", NoteKind::Endnote, true),
                    ("previous-endnote", NoteKind::Endnote, false),
                ];
                for (label, kind, forward) in items {
                    if ui.button(tr(label)).clicked() {
                        out.push(RibbonCommand::NextNote { kind, forward });
                        ui.close();
                    }
                }
            },
        );
    });
}
