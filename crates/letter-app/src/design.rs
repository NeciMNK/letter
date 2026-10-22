//! Вкладка «Конструктор»: группа «Фон страницы» — цвет страницы
//! и границы страниц.

use eframe::egui::Ui;
use letter_editor::{Command, PageBorderPreset, PageSetup};
use letter_model::Color;

use crate::i18n::tr;
use crate::icons::Icon;
use crate::insert::{menu_button, shading_palette};
use crate::ribbon::{RibbonCommand, RibbonInfo, group};

/// Подсказки клавиш вкладки «Конструктор» (для проверки).
#[cfg(test)]
pub const KEYTIPS: [&str; 2] = ["keytip-design-page-color", "keytip-design-page-borders"];

/// Вкладка «Конструктор».
pub fn design(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    // Пока открыт колонтитул, страница не меняется.
    let enabled = info.has_document && info.story.is_none();
    group(ui, tr("group-page-background"), |ui| {
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
            },
        );
    });
}
