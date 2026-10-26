//! Вкладка «Конструктор»: группа «Фон страницы» — подложка, цвет
//! страницы и границы страниц.

use eframe::egui::{self, Ui};
use letter_editor::{Command, PageBorderPreset, PageSetup};
use letter_model::Color;

use crate::i18n::tr;
use crate::icons::Icon;
use crate::insert::{menu_button, shading_palette};
use crate::ribbon::{RibbonCommand, RibbonInfo, row_group};

/// Подсказки клавиш вкладки «Конструктор» (для проверки).
#[cfg(test)]
pub const KEYTIPS: [&str; 3] = [
    "keytip-design-watermark",
    "keytip-design-page-color",
    "keytip-design-page-borders",
];

/// Готовые надписи меню «Подложка» (имена в файлах перевода), как
/// в Word.
const WATERMARKS: [&str; 5] = [
    "watermark-confidential",
    "watermark-do-not-copy",
    "watermark-sample",
    "watermark-draft",
    "watermark-urgent",
];

/// Вкладка «Конструктор».
pub fn design(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    // Пока открыт колонтитул, страница не меняется.
    let enabled = info.has_document && info.story.is_none();
    row_group(ui, tr("group-page-background"), |ui| {
        menu_button(
            ui,
            Icon::Watermark,
            tr("watermark"),
            tr("watermark-hint"),
            "keytip-design-watermark",
            enabled,
            |ui| watermark_menu(ui, out),
        );
        menu_button(
            ui,
            Icon::PageColor,
            tr("page-color"),
            tr("page-color-hint"),
            "keytip-design-page-color",
            enabled,
            |ui| {
                shading_palette(ui, |fill| {
                    let color = fill.map(|(r, g, b)| Color::Rgb(r, g, b));
                    out.push(RibbonCommand::Edit(Command::PageColor(color)));
                });
            },
        );
        menu_button(
            ui,
            Icon::PageBorders,
            tr("page-borders"),
            tr("page-borders-hint"),
            "keytip-design-page-borders",
            enabled,
            |ui| {
                for preset in PageBorderPreset::ALL {
                    let label = match preset {
                        PageBorderPreset::None => tr("page-borders-none"),
                        PageBorderPreset::Box => tr("page-borders-box"),
                        PageBorderPreset::Thick => tr("page-borders-thick"),
                    };
                    if ui.button(label).clicked() {
                        let setup = PageSetup::Borders(preset);
                        out.push(RibbonCommand::Edit(Command::PageSetup(setup)));
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button(tr("borders-dialog")).clicked() {
                    out.push(RibbonCommand::BordersDialog(
                        crate::borders_dialog::BordersTab::Page,
                    ));
                    ui.close();
                }
            },
        );
    });
}

/// Меню «Подложка»: готовые надписи, своя надпись и «Удалить подложку».
fn watermark_menu(ui: &mut Ui, out: &mut Vec<RibbonCommand>) {
    let mut put = |text: Option<String>, ui: &mut Ui| {
        out.push(RibbonCommand::Edit(Command::Watermark(text)));
        ui.close();
    };
    for name in WATERMARKS {
        if ui.button(tr(name)).clicked() {
            put(Some(tr(name).to_string()), ui);
        }
    }
    ui.separator();
    // Своя надпись живёт в памяти окна, пока открыто меню.
    let id = egui::Id::NULL.with("letter-custom-watermark");
    let mut text: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
    ui.horizontal(|ui| {
        let field = egui::TextEdit::singleline(&mut text)
            .hint_text(tr("watermark-custom"))
            .desired_width(140.0);
        let response = ui.add(field);
        let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button(tr("apply")).clicked() || enter) && !text.trim().is_empty() {
            put(Some(text.trim().to_string()), ui);
            text.clear();
        }
    });
    ui.data_mut(|d| d.insert_temp(id, text));
    ui.separator();
    if ui.button(tr("watermark-remove")).clicked() {
        put(None, ui);
    }
}
