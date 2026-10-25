//! Вкладки «Вставка» (пока — группы «Таблицы» и «Иллюстрации»), «Конструктор
//! таблиц» и «Макет таблицы» (видны, когда курсор в таблице) и «Формат
//! рисунка» (видна, когда выделен рисунок).

use eframe::egui::{self, Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use letter_editor::{BorderPreset, Command, CoverLayout, PictureWrap};
use letter_model::BreakKind;
use letter_model::Color;

use crate::i18n::tr;
use crate::icons::{Icon, icon_button, icon_text_button};
use crate::keytips::{self, Level, TipAction};
use crate::ribbon::{RibbonCommand, RibbonInfo, big_label, group, row_group};
use crate::theme::Palette;

/// Сколько столбцов и строк в сетке выбора размера таблицы.
const PICKER: (usize, usize) = (10, 8);
/// Сторона клетки сетки, точки экрана.
const SQUARE: f32 = 16.0;

/// Подсказки клавиш вкладки «Вставка» (для проверки).
#[cfg(test)]
pub const INSERT_KEYTIPS: [&str; 13] = [
    "keytip-insert-cover",
    "keytip-insert-blank-page",
    "keytip-insert-page-break",
    "keytip-insert-table",
    "keytip-insert-picture",
    "keytip-insert-link",
    "keytip-insert-bookmark",
    "keytip-insert-header",
    "keytip-insert-footer",
    "keytip-insert-page-number",
    "keytip-insert-date-time",
    "keytip-insert-field",
    "keytip-insert-symbol",
];

/// Подсказки клавиш вкладки «Макет таблицы» (для проверки).
#[cfg(test)]
pub const TABLE_KEYTIPS: [&str; 6] = [
    "keytip-table-merge",
    "keytip-table-delete",
    "keytip-table-above",
    "keytip-table-below",
    "keytip-table-left",
    "keytip-table-right",
];

/// Подсказки клавиш вкладки «Конструктор таблиц» (для проверки).
#[cfg(test)]
pub const DESIGN_KEYTIPS: [&str; 2] = ["keytip-design-shading", "keytip-design-borders"];

/// Подсказки клавиш вкладки «Формат рисунка» (для проверки).
#[cfg(test)]
pub const PICTURE_KEYTIPS: [&str; 1] = ["keytip-picture-wrap"];

/// Светло-серые заливки (как «Белый, фон 1, более тёмный оттенок»).
const GRAYS: [(u8, u8, u8); 4] = [
    (242, 242, 242),
    (217, 217, 217),
    (191, 191, 191),
    (166, 166, 166),
];

/// Вкладка «Вставка».
pub fn insert(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let doc = info.has_document;
    pages_group(ui, info, out);
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
            big_label(ui, tr("table"));
        });
    });
    group(ui, tr("group-illustrations"), |ui| {
        ui.vertical(|ui| {
            let response = icon_button(
                ui,
                Icon::Picture,
                tr("pictures"),
                vec2(44.0, 34.0),
                doc,
                false,
            )
            .on_hover_text(tr("pictures-hint"));
            let command = RibbonCommand::InsertPicture;
            keytips::tip(
                ui,
                &response,
                Level::Commands,
                "keytip-insert-picture",
                TipAction::Command(command.clone()),
            );
            if response.clicked() {
                out.push(command);
            }
            big_label(ui, tr("pictures"));
        });
    });
    row_group(ui, tr("group-links"), |ui| {
        ui.vertical(|ui| {
            let response = icon_button(ui, Icon::Link, tr("link"), vec2(44.0, 34.0), doc, false)
                .on_hover_text(tr("link-hint"));
            let command = RibbonCommand::LinkDialog;
            keytips::tip(
                ui,
                &response,
                Level::Commands,
                "keytip-insert-link",
                TipAction::Command(command.clone()),
            );
            if response.clicked() {
                out.push(command);
            }
            big_label(ui, tr("link"));
        });
        ui.vertical(|ui| {
            let response = icon_button(
                ui,
                Icon::Bookmark,
                tr("bookmark"),
                vec2(44.0, 34.0),
                doc,
                false,
            )
            .on_hover_text(tr("bookmark-hint"));
            let command = RibbonCommand::BookmarkDialog;
            keytips::tip(
                ui,
                &response,
                Level::Commands,
                "keytip-insert-bookmark",
                TipAction::Command(command.clone()),
            );
            if response.clicked() {
                out.push(command);
            }
            big_label(ui, tr("bookmark"));
        });
    });
    crate::headers::insert_group(ui, info, out);
    crate::fields::text_group(ui, info, out);
    crate::symbols::group_ui(ui, info, out);
}

/// Группа «Страницы»: титульная страница, пустая страница и разрыв
/// страницы. В колонтитуле недоступна.
fn pages_group(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let enabled = info.has_document && info.story.is_none();
    row_group(ui, tr("group-pages"), |ui| {
        menu_button(
            ui,
            Icon::CoverPage,
            tr("cover-page"),
            tr("cover-page-hint"),
            "keytip-insert-cover",
            enabled,
            |ui| {
                for layout in CoverLayout::ALL {
                    let name = match layout {
                        CoverLayout::Plain => "cover-plain",
                        CoverLayout::Band => "cover-band",
                        CoverLayout::Side => "cover-side",
                    };
                    if ui.button(tr(name)).clicked() {
                        out.push(RibbonCommand::CoverPage(layout));
                        ui.close();
                    }
                }
            },
        );
        command_button(
            ui,
            (Icon::BlankPage, tr("blank-page"), tr("blank-page-hint")),
            "keytip-insert-blank-page",
            enabled,
            RibbonCommand::Edit(Command::BlankPage),
            out,
        );
        command_button(
            ui,
            (Icon::Breaks, tr("page-break"), tr("page-break-hint")),
            "keytip-insert-page-break",
            enabled,
            RibbonCommand::Edit(Command::Break(BreakKind::Page)),
            out,
        );
    });
}

/// Большая кнопка со значком и подписью под ней: значок, подпись
/// и подсказка — в `face`.
fn command_button(
    ui: &mut Ui,
    face: (Icon, &str, &str),
    key: &str,
    enabled: bool,
    command: RibbonCommand,
    out: &mut Vec<RibbonCommand>,
) {
    let (icon, label, hint) = face;
    ui.vertical(|ui| {
        let response =
            icon_button(ui, icon, label, vec2(44.0, 34.0), enabled, false).on_hover_text(hint);
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
        big_label(ui, label);
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
    group(ui, tr("group-merge"), |ui| {
        let response = icon_button(
            ui,
            Icon::MergeCells,
            tr("merge-cells"),
            vec2(44.0, 34.0),
            enabled && info.can_merge,
            false,
        )
        .on_hover_text(tr("merge-cells"));
        let command = RibbonCommand::Edit(Command::MergeCells);
        keytips::tip(
            ui,
            &response,
            Level::Commands,
            "keytip-table-merge",
            TipAction::Command(command.clone()),
        );
        if response.clicked() {
            out.push(command);
        }
        big_label(ui, tr("merge-cells"));
    });
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
                big_label(ui, tr("table-delete"));
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

/// Большая кнопка со значком, подписью под ней и меню.
pub(crate) fn menu_button(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    hint: &str,
    key: &str,
    enabled: bool,
    menu: impl FnOnce(&mut Ui),
) {
    ui.vertical(|ui| {
        let response =
            icon_button(ui, icon, label, vec2(44.0, 34.0), enabled, false).on_hover_text(hint);
        let popup = egui::Popup::default_response_id(&response);
        keytips::tip(ui, &response, Level::Commands, key, TipAction::Popup(popup));
        egui::Popup::menu(&response).show(menu);
        big_label(ui, label);
    });
}

/// Вкладка «Формат рисунка»: обтекание выделенного рисунка текстом.
pub fn picture_format(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let current = info.picture_wrap;
    group(ui, tr("group-arrange"), |ui| {
        menu_button(
            ui,
            Icon::WrapText,
            tr("wrap-text"),
            tr("wrap-text-hint"),
            "keytip-picture-wrap",
            info.has_document && current.is_some(),
            |ui| {
                for wrap in PictureWrap::ALL {
                    if ui
                        .add(egui::Button::selectable(
                            current == Some(wrap),
                            tr(wrap_label(wrap)),
                        ))
                        .clicked()
                    {
                        out.push(RibbonCommand::PictureWrap(wrap));
                        ui.close();
                    }
                }
            },
        );
    });
}

/// Имя пункта меню «Обтекание текстом».
fn wrap_label(wrap: PictureWrap) -> &'static str {
    match wrap {
        PictureWrap::InLine => "wrap-inline",
        PictureWrap::Square => "wrap-square",
        PictureWrap::Tight => "wrap-tight",
        PictureWrap::Through => "wrap-through",
        PictureWrap::TopAndBottom => "wrap-top-bottom",
        PictureWrap::Behind => "wrap-behind",
        PictureWrap::InFront => "wrap-in-front",
    }
}

/// Вкладка «Конструктор таблиц»: заливка и границы ячеек.
pub fn table_design(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let enabled = info.has_document && info.in_table;
    group(ui, tr("group-shading"), |ui| {
        menu_button(
            ui,
            Icon::Shading,
            tr("shading"),
            tr("shading-hint"),
            "keytip-design-shading",
            enabled,
            |ui| shading_menu(ui, out),
        );
    });
    group(ui, tr("group-table-borders"), |ui| {
        menu_button(
            ui,
            Icon::BordersAll,
            tr("borders"),
            tr("borders-hint"),
            "keytip-design-borders",
            enabled,
            |ui| {
                for (icon, label, preset) in [
                    (
                        Icon::BorderBottom,
                        tr("border-bottom"),
                        BorderPreset::Bottom,
                    ),
                    (Icon::BorderTop, tr("border-top"), BorderPreset::Top),
                    (Icon::BorderLeft, tr("border-left"), BorderPreset::Left),
                    (Icon::BorderRight, tr("border-right"), BorderPreset::Right),
                    (Icon::BordersNone, tr("border-none"), BorderPreset::None),
                    (Icon::BordersAll, tr("border-all"), BorderPreset::All),
                    (
                        Icon::BordersOutside,
                        tr("border-outside"),
                        BorderPreset::Outside,
                    ),
                    (Icon::Table, tr("border-inside"), BorderPreset::Inside),
                ] {
                    if icon_text_button(ui, icon, label, true).clicked() {
                        out.push(RibbonCommand::Edit(Command::CellBorders(preset)));
                        ui.close();
                    }
                }
            },
        );
    });
}

/// Меню «Заливка» ячеек.
fn shading_menu(ui: &mut Ui, out: &mut Vec<RibbonCommand>) {
    shading_palette(ui, |fill| {
        let color = fill.map(|(r, g, b)| Color::Rgb(r, g, b));
        out.push(RibbonCommand::Edit(Command::CellShading(color)));
    });
}

/// Палитра «Заливка»: без цвета, стандартные цвета и серые. `pick`
/// получает выбранный цвет (`None` — «Нет цвета»), меню закрывается.
pub(crate) fn shading_palette(ui: &mut Ui, mut pick: impl FnMut(Option<(u8, u8, u8)>)) {
    if ui.button(tr("shading-none")).clicked() {
        pick(None);
        ui.close();
    }
    ui.label(egui::RichText::new(tr("color-standard")).small());
    for colors in [
        &crate::home::STANDARD_COLORS.map(|(_, rgb)| rgb)[..],
        &GRAYS[..],
    ] {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for &rgb in colors {
                let name = crate::home::STANDARD_COLORS
                    .iter()
                    .find(|(_, c)| *c == rgb)
                    .map_or_else(
                        || format!("#{:02X}{:02X}{:02X}", rgb.0, rgb.1, rgb.2),
                        |(name, _)| tr(name).to_string(),
                    );
                if crate::home::swatch(ui, crate::home::color32(rgb), &name).clicked() {
                    pick(Some(rgb));
                    ui.close();
                }
            }
        });
    }
}
