//! Вкладки «Вставка» (пока — группы «Таблицы» и «Иллюстрации»), «Конструктор
//! таблиц» и «Макет таблицы» (видны, когда курсор в таблице) и «Формат
//! рисунка» и «Формат фигуры» (видны, когда выделен рисунок или фигура).

use eframe::egui::{self, Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use letter_editor::{BorderPreset, Command, CoverLayout, PictureWrap};
use letter_model::BreakKind;
use letter_model::Color;
use letter_model::{TableArea, TableAreas, TableLook};

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
pub const INSERT_KEYTIPS: [&str; 15] = [
    "keytip-insert-equation",
    "keytip-insert-shapes",
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
pub const DESIGN_KEYTIPS: [&str; 9] = [
    "keytip-design-shading",
    "keytip-design-borders",
    "keytip-table-styles",
    "keytip-look-header-row",
    "keytip-look-total-row",
    "keytip-look-banded-rows",
    "keytip-look-first-column",
    "keytip-look-last-column",
    "keytip-look-banded-columns",
];

/// Стиль таблицы в галерее «Стили таблиц»: подпись и сам стиль.
#[derive(Debug, Clone)]
pub struct GalleryTableStyle {
    /// Подпись (всплывающая подсказка).
    pub label: String,
    /// Стиль.
    pub style: letter_model::Style,
    /// Готовый стиль (иначе — стиль из документа).
    pub preset: bool,
}

/// Стили для галереи «Стили таблиц»: сначала готовые стили (если такой
/// уже есть в документе — его вариант из документа), затем остальные
/// стили таблиц документа (кроме стиля по умолчанию).
pub fn table_gallery(doc: &letter_model::Document) -> Vec<GalleryTableStyle> {
    let mut out = Vec::new();
    for preset in letter_editor::TablePreset::ALL {
        let kind = match preset {
            letter_editor::TablePreset::Plain => tr("table-style-plain"),
            letter_editor::TablePreset::Grid => tr("table-style-grid"),
            letter_editor::TablePreset::List => tr("table-style-list"),
        };
        for color in 0..letter_editor::PRESET_COLORS.len() {
            let mut style = letter_editor::table_preset(doc, preset, color);
            if let Some(own) = doc.styles.get(&style.id) {
                style = own.clone();
            }
            let color = if color == 0 {
                tr("table-color-text").to_string()
            } else {
                format!("{} {color}", tr("table-color-accent"))
            };
            out.push(GalleryTableStyle {
                label: format!("{kind}: {color}"),
                style,
                preset: true,
            });
        }
    }
    let others: Vec<GalleryTableStyle> = doc
        .styles
        .styles
        .iter()
        .filter(|s| s.kind == letter_model::StyleKind::Table && !s.is_default)
        .filter(|s| !out.iter().any(|g| g.style.id == s.id))
        .map(|s| GalleryTableStyle {
            label: s.name.clone().unwrap_or_else(|| s.id.clone()),
            style: s.clone(),
            preset: false,
        })
        .collect();
    out.extend(others);
    out
}

/// Подсказки клавиш вкладки «Формат рисунка» (для проверки).
#[cfg(test)]
pub const PICTURE_KEYTIPS: [&str; 1] = ["keytip-picture-wrap"];

/// Подсказки клавиш вкладки «Формат фигуры» (для проверки).
#[cfg(test)]
pub const SHAPE_KEYTIPS: [&str; 4] = [
    "keytip-shape-change",
    "keytip-shape-fill",
    "keytip-shape-outline",
    "keytip-picture-wrap",
];

/// Названия групп меню «Фигуры» (по порядку [`letter_editor::SHAPE_GROUPS`]).
const SHAPE_GROUP_NAMES: [&str; 6] = [
    "shapes-lines",
    "shapes-rectangles",
    "shapes-basic",
    "shapes-arrows",
    "shapes-stars",
    "shapes-text-box",
];

/// Толщины контура в меню «Толщина», пт.
const LINE_WIDTHS: [f32; 9] = [0.25, 0.5, 0.75, 1.0, 1.5, 2.25, 3.0, 4.5, 6.0];

/// Сантиметров в EMU.
const EMU_PER_CM: f64 = 360_000.0;

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
    row_group(ui, tr("group-illustrations"), |ui| {
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
        menu_button(
            ui,
            Icon::Shapes,
            tr("shapes"),
            tr("shapes-hint"),
            "keytip-insert-shapes",
            doc,
            |ui| {
                shape_gallery(ui, true, |name| out.push(RibbonCommand::InsertShape(name)));
            },
        );
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
    group(ui, tr("group-arrange"), |ui| wrap_menu(ui, info, out));
}

/// Меню готовых фигур по группам; `pick` получает имя выбранной фигуры.
/// `text_box` — показывать ли «Надпись».
fn shape_gallery(ui: &mut Ui, text_box: bool, mut pick: impl FnMut(&'static str)) {
    for (i, (names, title)) in letter_editor::SHAPE_GROUPS
        .iter()
        .zip(SHAPE_GROUP_NAMES)
        .enumerate()
    {
        if !text_box && names.contains(&letter_editor::TEXT_BOX) {
            continue;
        }
        if i > 0 {
            ui.separator();
        }
        ui.label(egui::RichText::new(tr(title)).small().strong());
        for name in names.iter() {
            if ui.button(tr(&format!("shape-{name}"))).clicked() {
                pick(name);
                ui.close();
            }
        }
    }
}

/// Меню «Обтекание текстом» рисунка или фигуры.
fn wrap_menu(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let current = info.picture_wrap;
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
}

/// Вкладка «Формат фигуры»: вид фигуры, заливка, контур, обтекание и
/// размер выделенной фигуры.
pub fn shape_format(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let Some(shape) = &info.shape else {
        return;
    };
    let enabled = info.has_document;
    let edit =
        |shape: letter_model::Shape| RibbonCommand::Edit(Command::PictureShape(Box::new(shape)));
    group(ui, tr("group-insert-shapes"), |ui| {
        menu_button(
            ui,
            Icon::Replace,
            tr("change-shape"),
            tr("change-shape-hint"),
            "keytip-shape-change",
            enabled,
            |ui| {
                shape_gallery(ui, false, |name| {
                    out.push(edit(letter_editor::with_geometry(shape, name)));
                });
            },
        );
    });
    row_group(ui, tr("group-shape-styles"), |ui| {
        ui.horizontal(|ui| {
            menu_button(
                ui,
                Icon::ShapeFill,
                tr("shape-fill"),
                tr("shape-fill-hint"),
                "keytip-shape-fill",
                enabled,
                |ui| {
                    shading_palette(ui, |fill| {
                        out.push(edit(letter_editor::with_fill(
                            shape,
                            fill.map(|(r, g, b)| [r, g, b]),
                        )));
                    });
                },
            );
            menu_button(
                ui,
                Icon::ShapeOutline,
                tr("shape-outline"),
                tr("shape-outline-hint"),
                "keytip-shape-outline",
                enabled,
                |ui| {
                    shading_palette(ui, |color| {
                        out.push(edit(letter_editor::with_line_color(
                            shape,
                            color.map(|(r, g, b)| [r, g, b]),
                        )));
                    });
                    ui.separator();
                    ui.menu_button(tr("shape-outline-width"), |ui| {
                        let current = shape.line.as_ref().map(|l| l.width);
                        for points in LINE_WIDTHS {
                            let emu = (points * 12_700.0).round() as i64;
                            let label = crate::i18n::tr_args(
                                "unit-pt",
                                &[("value", f64::from(points).into())],
                            );
                            if ui
                                .add(egui::Button::selectable(current == Some(emu), label))
                                .clicked()
                            {
                                out.push(edit(letter_editor::with_line_width(shape, emu)));
                                ui.close();
                            }
                        }
                    });
                },
            );
        });
    });
    group(ui, tr("group-arrange"), |ui| wrap_menu(ui, info, out));
    group(ui, tr("group-size"), |ui| {
        let Some((width, height)) = info.picture_size else {
            return;
        };
        let mut size = [height, width].map(|v| v as f64 / EMU_PER_CM);
        let mut changed = false;
        for (value, label) in size.iter_mut().zip([tr("shape-height"), tr("shape-width")]) {
            ui.horizontal(|ui| {
                ui.label(label);
                let response = ui.add_enabled(
                    enabled,
                    egui::DragValue::new(value)
                        .range(0.0..=55.0)
                        .speed(0.05)
                        .fixed_decimals(2)
                        .suffix(format!(" {}", tr("shape-size-unit"))),
                );
                changed |= response.changed();
            });
        }
        if changed {
            let [height, width] = size.map(|v| (v * EMU_PER_CM).round() as i64);
            out.push(RibbonCommand::Edit(Command::ResizePicture {
                width,
                height,
            }));
        }
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
    let look = info.table_look.unwrap_or(TableLook::WORD);
    group(ui, tr("group-table-style-options"), |ui| {
        look_options(ui, look, enabled, out);
    });
    group(ui, tr("group-table-styles"), |ui| {
        menu_button(
            ui,
            Icon::Table,
            tr("table-styles"),
            tr("table-styles-hint"),
            "keytip-table-styles",
            enabled,
            |ui| {
                egui::ScrollArea::vertical()
                    .max_height(420.0)
                    .show(ui, |ui| {
                        let (presets, others): (Vec<_>, Vec<_>) =
                            info.table_styles.iter().partition(|g| g.preset);
                        for (title, entries, id) in [
                            (tr("table-styles-ready"), presets, "letter-table-presets"),
                            (tr("table-styles-document"), others, "letter-table-document"),
                        ] {
                            if entries.is_empty() {
                                continue;
                            }
                            ui.label(egui::RichText::new(title).small());
                            let picked = table_style_grid(ui, id, &entries, info, look);
                            if let Some(style) = picked {
                                out.push(RibbonCommand::Edit(Command::TableStyle(Box::new(style))));
                                ui.close();
                            }
                        }
                    });
            },
        );
    });
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

/// Образцы стилей таблиц по семь в ряд; возвращает выбранный стиль.
fn table_style_grid(
    ui: &mut Ui,
    id: &str,
    entries: &[&GalleryTableStyle],
    info: &RibbonInfo,
    look: TableLook,
) -> Option<letter_model::Style> {
    let mut picked = None;
    egui::Grid::new(id).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
        let columns = letter_editor::PRESET_COLORS.len();
        for (i, entry) in entries.iter().enumerate() {
            let current = info.table_style.as_deref() == Some(&entry.style.id);
            let response =
                table_preview(ui, &entry.style, look, current).on_hover_text(&entry.label);
            // Название образца — для диктора.
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &entry.label));
            if response.clicked() {
                picked = Some(entry.style.clone());
            }
            if (i + 1) % columns == 0 {
                ui.end_row();
            }
        }
    });
    picked
}

/// «Параметры стилей таблиц»: какие части таблицы стиль оформляет
/// по-своему (строка заголовка, полосы…). Два столбца по три флажка.
fn look_options(ui: &mut Ui, look: TableLook, enabled: bool, out: &mut Vec<RibbonCommand>) {
    type Get = fn(&mut TableLook) -> &mut bool;
    // Для полос в модели хранится «без полос», а флажок — «полосы».
    let options: [(&str, &str, Get, bool); 6] = [
        (
            "look-header-row",
            "keytip-look-header-row",
            |l| &mut l.first_row,
            false,
        ),
        (
            "look-total-row",
            "keytip-look-total-row",
            |l| &mut l.last_row,
            false,
        ),
        (
            "look-banded-rows",
            "keytip-look-banded-rows",
            |l| &mut l.no_h_band,
            true,
        ),
        (
            "look-first-column",
            "keytip-look-first-column",
            |l| &mut l.first_column,
            false,
        ),
        (
            "look-last-column",
            "keytip-look-last-column",
            |l| &mut l.last_column,
            false,
        ),
        (
            "look-banded-columns",
            "keytip-look-banded-columns",
            |l| &mut l.no_v_band,
            true,
        ),
    ];
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            for column in options.chunks(3) {
                ui.vertical(|ui| {
                    for &(label, key, get, inverted) in column {
                        let mut toggled = look;
                        *get(&mut toggled) ^= true;
                        let mut copy = look;
                        let mut on = *get(&mut copy) != inverted;
                        let response = ui.checkbox(&mut on, tr(label));
                        let command = RibbonCommand::Edit(Command::TableLook(toggled));
                        keytips::tip(
                            ui,
                            &response,
                            Level::Commands,
                            key,
                            TipAction::Command(command.clone()),
                        );
                        if response.changed() {
                            out.push(command);
                        }
                    }
                });
            }
        });
    });
}

/// Маленький образец таблицы 5 × 5 в стиле `style` при параметрах `look`
/// для галереи «Стили таблиц»: заливка, линии и «текст» ячеек (полоска
/// цвета текста, у полужирного — толще). `current` — стиль таблицы
/// с курсором (обведён).
fn table_preview(
    ui: &mut Ui,
    style: &letter_model::Style,
    look: TableLook,
    current: bool,
) -> egui::Response {
    const ROWS: usize = 5;
    const COLUMNS: usize = 5;
    let (rect, response) = ui.allocate_exact_size(vec2(72.0, 52.0), Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    let rgb = |c: Option<Color>| match c {
        Some(Color::Rgb(r, g, b)) => Color32::from_rgb(r, g, b),
        _ => Color32::BLACK,
    };
    let line = |b: &Option<letter_model::Border>| {
        b.as_ref().filter(|b| !b.is_none()).map(|b| {
            Stroke::new(
                if b.size.unwrap_or(4) > 6 { 1.5 } else { 1.0 },
                rgb(b.color),
            )
        })
    };
    let table = style.table.borders.clone().unwrap_or_default();
    let (w, h) = (rect.width() / COLUMNS as f32, rect.height() / ROWS as f32);
    for r in 0..ROWS {
        for c in 0..COLUMNS {
            let areas = TableAreas::of_cell(look, (r, ROWS), ((c, c + 1), COLUMNS), (1, 1));
            let mut fill = style.table.shading.as_ref().and_then(|s| s.background());
            let mut text = Color32::BLACK;
            let mut bold = false;
            let pick = |edge: bool, outer: &Option<_>, inner: &Option<_>| {
                if edge { line(outer) } else { line(inner) }
            };
            let mut sides = [
                pick(r == 0, &table.top, &table.inside_h),
                pick(r + 1 == ROWS, &table.bottom, &table.inside_h),
                pick(c == 0, &table.left, &table.inside_v),
                pick(c + 1 == COLUMNS, &table.right, &table.inside_v),
            ];
            for area in TableArea::ORDER.iter().filter(|a| areas.contains(**a)) {
                for part in style.table_areas.iter().filter(|p| p.area == *area) {
                    if let Some(shading) = &part.cell.shading {
                        fill = shading.background();
                    }
                    if part.run.color.is_some() {
                        text = rgb(part.run.color);
                    }
                    if let Some(on) = part.run.bold {
                        bold = on;
                    }
                    if let Some(b) = &part.cell.borders {
                        for (side, border) in
                            sides.iter_mut().zip([&b.top, &b.bottom, &b.left, &b.right])
                        {
                            if border.is_some() {
                                *side = line(border);
                            }
                        }
                    }
                }
            }
            let cell = Rect::from_min_size(
                pos2(rect.left() + c as f32 * w, rect.top() + r as f32 * h),
                vec2(w, h),
            );
            if let Some((r, g, b)) = fill {
                painter.rect_filled(cell, 0.0, Color32::from_rgb(r, g, b));
            }
            let y = cell.center().y;
            painter.line_segment(
                [pos2(cell.left() + 3.0, y), pos2(cell.right() - 4.0, y)],
                Stroke::new(if bold { 2.0 } else { 1.0 }, text),
            );
            let [top, bottom, left, right] = sides;
            for (stroke, a, b) in [
                (top, cell.left_top(), cell.right_top()),
                (bottom, cell.left_bottom(), cell.right_bottom()),
                (left, cell.left_top(), cell.left_bottom()),
                (right, cell.right_top(), cell.right_bottom()),
            ] {
                if let Some(stroke) = stroke {
                    painter.line_segment([a, b], stroke);
                }
            }
        }
    }
    let frame = if current || response.hovered() {
        Stroke::new(2.0, Palette::of(ui.ctx()).accent_text)
    } else {
        Stroke::new(1.0, Color32::from_gray(200))
    };
    ui.painter()
        .rect_stroke(rect, 0.0, frame, egui::StrokeKind::Outside);
    response
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
