//! Вкладка «Макет»: группа «Параметры страницы» — поля, ориентация,
//! размер, колонки и разрывы. Всё меняется у разделов выделенного.

use eframe::egui::{self, RichText, Ui};
use letter_editor::{ColumnPreset, Command, PageSetup};
use letter_model::{BreakKind, Orientation, PageMargins, PageSize, SectionStart, Twips};

use crate::i18n::tr;
use crate::icons::Icon;
use crate::insert::menu_button;
use crate::ribbon::{RibbonCommand, RibbonInfo, group};

/// Подсказки клавиш вкладки «Макет» (для проверки).
#[cfg(test)]
pub const KEYTIPS: [&str; 5] = [
    "keytip-layout-margins",
    "keytip-layout-orientation",
    "keytip-layout-size",
    "keytip-layout-columns",
    "keytip-layout-breaks",
];

/// Готовые поля: имя и поля сверху, справа, снизу, слева (твипы).
const MARGINS: [(&str, [i32; 4]); 4] = [
    ("margins-normal", [1134, 850, 1134, 1701]),
    ("margins-narrow", [720, 720, 720, 720]),
    ("margins-moderate", [1440, 1080, 1440, 1080]),
    ("margins-wide", [1440, 2880, 1440, 2880]),
];

/// Размеры листа: имя и книжные ширина и высота (твипы).
const SIZES: [(&str, i32, i32); 5] = [
    ("size-a4", 11906, 16838),
    ("size-a5", 8391, 11906),
    ("size-a3", 16838, 23811),
    ("size-letter", 12240, 15840),
    ("size-legal", 12240, 20160),
];

/// Вкладка «Макет».
pub fn layout(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let enabled = info.has_document;
    let section = info.section.as_ref();
    let mut push = |ui: &mut Ui, command: Command| {
        out.push(RibbonCommand::Edit(command));
        ui.close();
    };
    group(ui, tr("group-page-setup"), |ui| {
        let margins = section.and_then(|s| s.margins).unwrap_or_default();
        menu_button(
            ui,
            Icon::Margins,
            tr("margins"),
            tr("margins-hint"),
            "keytip-layout-margins",
            enabled,
            |ui| {
                for (label, [top, right, bottom, left]) in MARGINS {
                    let preset = PageMargins {
                        top: Twips(top),
                        right: Twips(right),
                        bottom: Twips(bottom),
                        left: Twips(left),
                        ..margins
                    };
                    if ui
                        .add(egui::Button::selectable(margins == preset, tr(label)))
                        .clicked()
                    {
                        push(ui, Command::PageSetup(PageSetup::Margins(preset)));
                    }
                }
            },
        );
        let size = section.and_then(|s| s.page_size).unwrap_or(PageSize::A4);
        menu_button(
            ui,
            Icon::Orientation,
            tr("orientation"),
            tr("orientation-hint"),
            "keytip-layout-orientation",
            enabled,
            |ui| {
                for (label, orientation) in [
                    ("orientation-portrait", Orientation::Portrait),
                    ("orientation-landscape", Orientation::Landscape),
                ] {
                    let current = size.orientation == orientation;
                    if ui
                        .add(egui::Button::selectable(current, tr(label)))
                        .clicked()
                    {
                        push(ui, Command::PageSetup(PageSetup::Orientation(orientation)));
                    }
                }
            },
        );
        menu_button(
            ui,
            Icon::PageSize,
            tr("page-size"),
            tr("page-size-hint"),
            "keytip-layout-size",
            enabled,
            |ui| {
                for (label, width, height) in SIZES {
                    let current = (
                        size.width.0.min(size.height.0),
                        size.width.0.max(size.height.0),
                    ) == (width, height);
                    if ui
                        .add(egui::Button::selectable(current, tr(label)))
                        .clicked()
                    {
                        let size = PageSize {
                            width: Twips(width),
                            height: Twips(height),
                            orientation: Orientation::Portrait,
                        };
                        push(ui, Command::PageSetup(PageSetup::Size(size)));
                    }
                }
            },
        );
        let columns = section.and_then(|s| ColumnPreset::of(s.columns.as_ref()));
        menu_button(
            ui,
            Icon::Columns,
            tr("columns"),
            tr("columns-hint"),
            "keytip-layout-columns",
            enabled,
            |ui| {
                for preset in ColumnPreset::ALL {
                    if ui
                        .add(egui::Button::selectable(
                            columns == Some(preset),
                            tr(column_label(preset)),
                        ))
                        .clicked()
                    {
                        push(ui, Command::PageSetup(PageSetup::Columns(preset)));
                    }
                }
            },
        );
        menu_button(
            ui,
            Icon::Breaks,
            tr("breaks"),
            tr("breaks-hint"),
            "keytip-layout-breaks",
            enabled,
            |ui| {
                ui.label(RichText::new(tr("breaks-page-group")).strong());
                for (label, kind) in [
                    ("break-page", BreakKind::Page),
                    ("break-column", BreakKind::Column),
                ] {
                    if ui.button(tr(label)).clicked() {
                        push(ui, Command::Break(kind));
                    }
                }
                ui.separator();
                ui.label(RichText::new(tr("breaks-section-group")).strong());
                for (label, start) in [
                    ("break-next-page", SectionStart::NextPage),
                    ("break-continuous", SectionStart::Continuous),
                    ("break-even-page", SectionStart::EvenPage),
                    ("break-odd-page", SectionStart::OddPage),
                ] {
                    let button = ui.add_enabled(!info.in_table, egui::Button::new(tr(label)));
                    if button.clicked() {
                        push(ui, Command::SectionBreak(start));
                    }
                }
            },
        );
    });
}

/// Имя пункта меню «Колонки».
fn column_label(preset: ColumnPreset) -> &'static str {
    match preset {
        ColumnPreset::One => "columns-one",
        ColumnPreset::Two => "columns-two",
        ColumnPreset::Three => "columns-three",
        ColumnPreset::Left => "columns-left",
        ColumnPreset::Right => "columns-right",
    }
}
