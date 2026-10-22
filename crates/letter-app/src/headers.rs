//! Колонтитулы на ленте: группа «Колонтитулы» на «Вставке» и вкладка
//! «Колонтитулы», которая видна, пока колонтитул открыт для правки.

use eframe::egui::{self, Ui};

use crate::i18n::tr;
use crate::icons::{Icon, icon_text_button};
use crate::insert::menu_button;
use crate::keytips::{self, Level, TipAction};
use crate::ribbon::{RibbonCommand, RibbonInfo, group};

/// Подсказки клавиш вкладки «Колонтитулы» (для проверки).
#[cfg(test)]
pub const KEYTIPS: [&str; 5] = [
    "keytip-hf-header",
    "keytip-hf-footer",
    "keytip-hf-page-number",
    "keytip-hf-title-page",
    "keytip-hf-close",
];

/// Куда вставить номер страницы.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageNumberPlace {
    /// В верхний колонтитул, по центру (заменяет его содержимое).
    Top,
    /// В нижний колонтитул, по центру (заменяет его содержимое).
    Bottom,
    /// У курсора в открытом колонтитуле.
    Here,
}

/// Группа «Колонтитулы» вкладки «Вставка».
pub fn insert_group(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    group(ui, tr("group-header-footer"), |ui| {
        buttons(ui, info, out, "keytip-insert");
    });
}

/// Вкладка «Колонтитулы».
pub fn header_footer(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    group(ui, tr("group-header-footer"), |ui| {
        buttons(ui, info, out, "keytip-hf");
    });
    group(ui, tr("group-options"), |ui| {
        let mut on = info.title_page;
        let response = ui
            .checkbox(&mut on, tr("title-page"))
            .on_hover_text(tr("title-page-hint"));
        let command = RibbonCommand::TitlePage(!info.title_page);
        keytips::tip(
            ui,
            &response,
            Level::Commands,
            "keytip-hf-title-page",
            TipAction::Command(command.clone()),
        );
        if response.changed() {
            out.push(command);
        }
    });
    group(ui, tr("group-close"), |ui| {
        let response = icon_text_button(ui, Icon::Close, tr("close-header-footer"), true);
        let command = RibbonCommand::CloseHeaderFooter;
        keytips::tip(
            ui,
            &response,
            Level::Commands,
            "keytip-hf-close",
            TipAction::Command(command.clone()),
        );
        if response.clicked() {
            out.push(command);
        }
    });
}

/// Кнопки «Верхний колонтитул», «Нижний колонтитул», «Номер страницы»;
/// `prefix` — начало имён подсказок клавиш вкладки.
fn buttons(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>, prefix: &str) {
    let enabled = info.has_document;
    for (icon, label, hint, key, header) in [
        (Icon::Header, "header", "header-hint", "header", true),
        (Icon::Footer, "footer", "footer-hint", "footer", false),
    ] {
        ui.vertical(|ui| {
            let response = crate::icons::icon_button(
                ui,
                icon,
                tr(label),
                egui::vec2(44.0, 34.0),
                enabled,
                info.story == Some(header),
            )
            .on_hover_text(tr(hint));
            let command = RibbonCommand::EditHeaderFooter { header };
            keytips::tip(
                ui,
                &response,
                Level::Commands,
                &format!("{prefix}-{key}"),
                TipAction::Command(command.clone()),
            );
            if response.clicked() {
                out.push(command);
            }
            ui.label(tr(label));
        });
    }
    menu_button(
        ui,
        Icon::PageNumber,
        tr("page-number"),
        tr("page-number-hint"),
        &format!("{prefix}-page-number"),
        enabled,
        |ui| {
            for (label, place) in [
                ("page-number-top", PageNumberPlace::Top),
                ("page-number-bottom", PageNumberPlace::Bottom),
                ("page-number-here", PageNumberPlace::Here),
            ] {
                let allowed = place != PageNumberPlace::Here || info.story.is_some();
                if ui
                    .add_enabled(allowed, egui::Button::new(tr(label)))
                    .clicked()
                {
                    out.push(RibbonCommand::PageNumber(place));
                    ui.close();
                }
            }
        },
    );
}
