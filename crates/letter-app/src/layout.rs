//! Вкладка «Макет»: группа «Параметры страницы» — поля, ориентация,
//! размер, колонки, разрывы и расстановка переносов (меняются у разделов
//! выделенного), и группа «Абзац» — отступы и интервалы выделенных
//! абзацев.

use eframe::egui::{self, RichText, Ui};
use letter_editor::{ColumnPreset, Command, PageSetup, ParagraphChanges, ParagraphFormat};
use letter_model::{BreakKind, Orientation, PageMargins, PageSize, SectionStart, Twips};

use crate::i18n::tr;
use crate::icons::Icon;
use crate::insert::menu_button;
use crate::keytips::{self, Level, TipAction};
use crate::ribbon::{RibbonCommand, RibbonInfo, group, row_group};
use crate::units::{cm_field, pt_field};

/// Подсказки клавиш вкладки «Макет» (для проверки).
#[cfg(test)]
pub const KEYTIPS: [&str; 10] = [
    "keytip-layout-margins",
    "keytip-layout-orientation",
    "keytip-layout-size",
    "keytip-layout-columns",
    "keytip-layout-breaks",
    "keytip-layout-hyphenation",
    "keytip-layout-indent-left",
    "keytip-layout-indent-right",
    "keytip-layout-space-before",
    "keytip-layout-space-after",
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
    row_group(ui, tr("group-page-setup"), |ui| {
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
        menu_button(
            ui,
            Icon::Hyphenation,
            tr("hyphenation"),
            tr("hyphenation-hint"),
            "keytip-layout-hyphenation",
            enabled,
            |ui| {
                for (label, on) in [("hyphenation-none", false), ("hyphenation-auto", true)] {
                    if ui
                        .add(egui::Button::selectable(
                            info.auto_hyphenation == on,
                            tr(label),
                        ))
                        .clicked()
                    {
                        push(ui, Command::AutoHyphenation(on));
                    }
                }
            },
        );
    });
    paragraph_group(ui, info, out);
}

/// Группа «Абзац»: отступы слева и справа (см) и интервалы до и после
/// (пт) абзаца с курсором; новое значение ставится всем выделенным.
fn paragraph_group(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let enabled = info.has_document;
    group(ui, tr("paragraph"), |ui| {
        ui.add_enabled_ui(enabled, |ui| {
            egui::Grid::new("layout-paragraph")
                .num_columns(4)
                .spacing([6.0, 4.0])
                .show(ui, |ui| {
                    let mut changes = ParagraphChanges::default();
                    ui.label(RichText::new(tr("layout-indent")).strong());
                    ui.end_row();
                    let mut left = info.indent_left;
                    let label = ui.label(tr("layout-indent-left"));
                    let field = cm_field(ui, label.id, &mut left, -50.0, 55.0);
                    tip(ui, &field, "keytip-layout-indent-left");
                    if field.changed() {
                        changes.left = Some(Twips(left));
                    }
                    let mut before = info.space_before;
                    let label = ui.label(tr("layout-space-before"));
                    let field = pt_field(ui, label.id, &mut before, 0.0, 1584.0, 1.0);
                    tip(ui, &field, "keytip-layout-space-before");
                    if field.changed() {
                        changes.before = Some(Twips(before));
                    }
                    ui.end_row();
                    let mut right = info.indent_right;
                    let label = ui.label(tr("layout-indent-right"));
                    let field = cm_field(ui, label.id, &mut right, -50.0, 55.0);
                    tip(ui, &field, "keytip-layout-indent-right");
                    if field.changed() {
                        changes.right = Some(Twips(right));
                    }
                    let mut after = info.space_after;
                    let label = ui.label(tr("layout-space-after"));
                    let field = pt_field(ui, label.id, &mut after, 0.0, 1584.0, 1.0);
                    tip(ui, &field, "keytip-layout-space-after");
                    if field.changed() {
                        changes.after = Some(Twips(after));
                    }
                    ui.end_row();
                    if changes != ParagraphChanges::default() {
                        let format = ParagraphFormat::Set(changes);
                        out.push(RibbonCommand::Paragraph(format));
                    }
                });
        });
    });
}

/// Подсказка клавиш поля: ставит в него фокус.
fn tip(ui: &Ui, field: &egui::Response, key: &str) {
    keytips::tip(ui, field, Level::Commands, key, TipAction::Focus(field.id));
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

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};

    #[test]
    fn paragraph_fields_set_indents_and_spacing() {
        let info = RibbonInfo {
            has_document: true,
            indent_left: 0,
            space_after: 160,
            ..RibbonInfo::default()
        };
        let mut h = Harness::new_ui_state(
            move |ui, out: &mut Vec<RibbonCommand>| paragraph_group(ui, &info, out),
            Vec::new(),
        );
        h.run();
        let field = h
            .get_all_by_label("Слева:")
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::SpinButton)
            .expect("нет поля «Слева»");
        field.focus();
        h.run();
        let field = h
            .get_all_by_label("Слева:")
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::SpinButton)
            .expect("нет поля «Слева»");
        field.type_text("2");
        h.key_press(egui::Key::Enter);
        h.run();
        let changes: Vec<_> = h
            .state()
            .iter()
            .filter_map(|c| match c {
                RibbonCommand::Paragraph(ParagraphFormat::Set(c)) => Some(*c),
                _ => None,
            })
            .collect();
        assert!(
            changes
                .iter()
                .any(|c| c.left == Some(Twips(1134)) && c.after.is_none()),
            "{changes:?}"
        );
    }
}
