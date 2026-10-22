//! Вкладки «Вставка» (пока — только группа «Таблицы») и «Макет
//! таблицы» (видна, когда курсор в таблице).

use eframe::egui::{self, Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use letter_editor::Command;

use crate::i18n::tr;
use crate::icons::{Icon, icon_button, icon_text_button};
use crate::keytips::{self, Level, TipAction};
use crate::ribbon::{RibbonCommand, RibbonInfo, group};
use crate::theme::Palette;

/// Сколько столбцов и строк в сетке выбора размера таблицы.
const PICKER: (usize, usize) = (10, 8);
/// Сторона клетки сетки, точки экрана.
const SQUARE: f32 = 16.0;

/// Подсказки клавиш вкладки «Вставка» (для проверки).
#[cfg(test)]
pub const INSERT_KEYTIPS: [&str; 1] = ["keytip-insert-table"];

/// Подсказки клавиш вкладки «Макет таблицы» (для проверки).
#[cfg(test)]
pub const TABLE_KEYTIPS: [&str; 5] = [
    "keytip-table-delete",
    "keytip-table-above",
    "keytip-table-below",
    "keytip-table-left",
    "keytip-table-right",
];

/// Вкладка «Вставка».
pub fn insert(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let doc = info.has_document;
    group(ui, tr("group-tables"), |ui| {
        ui.vertical(|ui| {
            let response = icon_button(ui, Icon::Table, tr("table"), vec2(44.0, 34.0), doc, false)
                .on_hover_text(tr("table-hint"));
            let popup = egui::Popup::default_response_id(&response);
            keytips::tip(
                ui,
                &response,
                Level::Commands,
                "keytip-insert-table",
                TipAction::Popup(popup),
            );
            egui::Popup::menu(&response).show(|ui| table_picker(ui, out));
            ui.label(tr("table"));
        });
    });
}

/// Сетка выбора размера новой таблицы: клетки до указателя
/// подсвечиваются, щелчок вставляет таблицу.
fn table_picker(ui: &mut Ui, out: &mut Vec<RibbonCommand>) {
    let (columns, rows) = PICKER;
    let size = vec2(columns as f32 * SQUARE, rows as f32 * SQUARE);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let chosen = response.hover_pos().map(|p| {
        let column = ((p.x - rect.left()) / SQUARE)
            .floor()
            .clamp(0.0, columns as f32 - 1.0);
        let row = ((p.y - rect.top()) / SQUARE)
            .floor()
            .clamp(0.0, rows as f32 - 1.0);
        (row as usize + 1, column as usize + 1)
    });
    let colors = Palette::of(ui.ctx());
    for row in 0..rows {
        for column in 0..columns {
            let square = Rect::from_min_size(
                pos2(
                    rect.left() + column as f32 * SQUARE,
                    rect.top() + row as f32 * SQUARE,
                ),
                vec2(SQUARE, SQUARE),
            )
            .shrink(1.5);
            let lit = chosen.is_some_and(|(r, c)| row < r && column < c);
            let fill = if lit {
                colors.accent_text.gamma_multiply(0.35)
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect(
                square,
                CornerRadius::ZERO,
                fill,
                Stroke::new(1.0, colors.border),
                egui::StrokeKind::Inside,
            );
        }
    }
    let caption = match chosen {
        Some((r, c)) => format!("{} {c} × {r}", tr("table")),
        None => tr("table-hint").to_owned(),
    };
    ui.label(caption);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tr("table-hint")));
    if response.clicked()
        && let Some((rows, columns)) = chosen
    {
        out.push(RibbonCommand::Edit(Command::InsertTable { rows, columns }));
        ui.close();
    }
}

/// Вкладка «Макет таблицы»: удаление и вставка строк и столбцов.
pub fn table_layout(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let enabled = info.has_document && info.in_table;
    group(ui, tr("group-rows-columns"), |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                let response = icon_button(
                    ui,
                    Icon::DeleteTable,
                    tr("table-delete"),
                    vec2(44.0, 34.0),
                    enabled,
                    false,
                )
                .on_hover_text(tr("table-delete-hint"));
                let popup = egui::Popup::default_response_id(&response);
                keytips::tip(
                    ui,
                    &response,
                    Level::Commands,
                    "keytip-table-delete",
                    TipAction::Popup(popup),
                );
                egui::Popup::menu(&response).show(|ui| {
                    for (label, command) in [
                        (tr("delete-columns"), Command::DeleteColumns),
                        (tr("delete-rows"), Command::DeleteRows),
                        (tr("delete-table"), Command::DeleteTable),
                    ] {
                        if ui.button(label).clicked() {
                            out.push(RibbonCommand::Edit(command));
                            ui.close();
                        }
                    }
                });
                ui.label(tr("table-delete"));
            });
            ui.vertical(|ui| {
                for (icon, label, key, command) in [
                    (
                        Icon::InsertAbove,
                        tr("insert-above"),
                        "keytip-table-above",
                        Command::InsertRows { below: false },
                    ),
                    (
                        Icon::InsertBelow,
                        tr("insert-below"),
                        "keytip-table-below",
                        Command::InsertRows { below: true },
                    ),
                ] {
                    table_button(ui, icon, label, key, command, enabled, out);
                }
            });
            ui.vertical(|ui| {
                for (icon, label, key, command) in [
                    (
                        Icon::InsertLeft,
                        tr("insert-left"),
                        "keytip-table-left",
                        Command::InsertColumns { right: false },
                    ),
                    (
                        Icon::InsertRight,
                        tr("insert-right"),
                        "keytip-table-right",
                        Command::InsertColumns { right: true },
                    ),
                ] {
                    table_button(ui, icon, label, key, command, enabled, out);
                }
            });
        });
    });
}

fn table_button(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    key: &str,
    command: Command,
    enabled: bool,
    out: &mut Vec<RibbonCommand>,
) {
    let response = icon_text_button(ui, icon, label, enabled);
    let command = RibbonCommand::Edit(command);
    keytips::tip(
        ui,
        &response,
        Level::Commands,
        key,
        TipAction::Command(command.clone()),
    );
    if response.clicked() {
        out.push(command);
    }
}
