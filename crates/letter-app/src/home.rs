//! Вкладка «Главная» ленты: буфер обмена, шрифт, абзац.
//! Списки (F5.1), заливка и границы (F5.11) добавятся вместе с этими
//! возможностями документа.

use crate::i18n::tr;
use eframe::egui::{self, Color32, Key, RichText, TextEdit, Ui, vec2};
use letter_editor::{ListKind, ParagraphFormat, RunFormat};
use letter_model::{Alignment, Color, HalfPoints, Highlight, LineSpacing, Twips};

use crate::icons::{self, Icon, color_button, icon_button, icon_text_button};
use crate::keytips::{self, Level, TipAction};
use crate::ribbon::{RibbonCommand, RibbonInfo, group, group_with_launcher};
use crate::theme::Palette;

/// Размеры в списке «Размер шрифта», как в Word, пт.
pub const FONT_SIZES: [f32; 17] = [
    8.0, 9.0, 10.0, 10.5, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0, 36.0, 48.0,
    72.0,
];

/// Стандартные цвета палитры Word: название и цвет.
pub const STANDARD_COLORS: [(&str, (u8, u8, u8)); 10] = [
    ("color-dark-red", (0xC0, 0x00, 0x00)),
    ("color-red", (0xFF, 0x00, 0x00)),
    ("color-orange", (0xFF, 0xC0, 0x00)),
    ("color-yellow", (0xFF, 0xFF, 0x00)),
    ("color-light-green", (0x92, 0xD0, 0x50)),
    ("color-green", (0x00, 0xB0, 0x50)),
    ("color-light-blue", (0x00, 0xB0, 0xF0)),
    ("color-blue", (0x00, 0x70, 0xC0)),
    ("color-dark-blue", (0x00, 0x20, 0x60)),
    ("color-purple", (0x70, 0x30, 0xA0)),
];

/// Цвета маркера в порядке палитры Word (пять в ряд) и их названия.
pub const HIGHLIGHTS: [(Highlight, &str); 15] = [
    (Highlight::Yellow, "color-yellow"),
    (Highlight::Green, "color-bright-green"),
    (Highlight::Cyan, "color-turquoise"),
    (Highlight::Magenta, "color-pink"),
    (Highlight::Blue, "color-blue"),
    (Highlight::Red, "color-red"),
    (Highlight::DarkBlue, "color-dark-blue"),
    (Highlight::DarkCyan, "color-teal"),
    (Highlight::DarkGreen, "color-green"),
    (Highlight::DarkMagenta, "color-purple"),
    (Highlight::DarkRed, "color-dark-red"),
    (Highlight::DarkYellow, "color-dark-yellow"),
    (Highlight::DarkGray, "color-gray-50"),
    (Highlight::LightGray, "color-gray-25"),
    (Highlight::Black, "color-black"),
];

/// Что помнит вкладка «Главная» между кадрами.
#[derive(Debug, Clone)]
pub struct HomeState {
    /// Текст поиска в списке шрифтов.
    font_filter: String,
    /// Набираемый размер шрифта; `None`, пока поле не в фокусе.
    size_text: Option<String>,
    /// Последний выбранный цвет текста (его ставит кнопка без списка).
    font_color: Color,
    /// Последний выбранный цвет маркера.
    highlight: Highlight,
    /// Цвет в окне «Другие цвета».
    custom: Color32,
}

impl Default for HomeState {
    fn default() -> HomeState {
        // Как в Word: красный текст и жёлтый маркер.
        HomeState {
            font_filter: String::new(),
            size_text: None,
            font_color: Color::Rgb(0xFF, 0x00, 0x00),
            highlight: Highlight::Yellow,
            custom: Color32::from_rgb(0x00, 0x70, 0xC0),
        }
    }
}

/// Размер шрифта для показа: «11», «10,5». Полупункты.
pub fn size_label(half_points: u32) -> String {
    if half_points == 0 {
        String::new()
    } else if half_points.is_multiple_of(2) {
        (half_points / 2).to_string()
    } else {
        format!("{},5", half_points / 2)
    }
}

/// Размер, набранный в поле: число пунктов от 1 до 1638, с точностью
/// до половины (запятая или точка). Полупункты.
pub fn parse_size(text: &str) -> Option<HalfPoints> {
    let value: f32 = text.trim().replace(',', ".").parse().ok()?;
    (1.0..=1638.0)
        .contains(&value)
        .then(|| HalfPoints((value * 2.0).round() as u32))
}

fn color32((r, g, b): (u8, u8, u8)) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// Цвет полоски под кнопкой «Цвет текста».
fn font_color32(color: Color) -> Color32 {
    match color {
        Color::Auto => Color32::BLACK,
        Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

/// Подсказки клавиш вкладки «Главная» (имена в файлах перевода) — для
/// проверки, что они не мешают друг другу.
#[cfg(test)]
pub const KEYTIPS: [&str; 32] = [
    "keytip-home-paste",
    "keytip-home-cut",
    "keytip-home-copy",
    "keytip-home-font",
    "keytip-home-size",
    "keytip-home-grow",
    "keytip-home-shrink",
    "keytip-home-clear",
    "keytip-home-bold",
    "keytip-home-italic",
    "keytip-home-underline",
    "keytip-home-strike",
    "keytip-home-subscript",
    "keytip-home-superscript",
    "keytip-home-highlight",
    "keytip-home-font-color",
    "keytip-home-font-dialog",
    "keytip-home-bullets",
    "keytip-home-numbering",
    "keytip-home-indent-less",
    "keytip-home-indent-more",
    "keytip-home-marks",
    "keytip-home-align-left",
    "keytip-home-align-center",
    "keytip-home-align-right",
    "keytip-home-align-justify",
    "keytip-home-line-spacing",
    "keytip-home-paragraph-dialog",
    "keytip-home-find",
    "keytip-home-replace",
    "keytip-home-select-all",
    "keytip-home-styles",
];

/// Подсказка клавиш у кнопки с командой.
fn keytip(ui: &Ui, response: &egui::Response, id: &str, command: RibbonCommand) {
    keytips::tip(
        ui,
        response,
        Level::Commands,
        id,
        TipAction::Command(command),
    );
}

/// Подсказка клавиш у кнопки со списком или меню: открывает его.
fn popup_keytip(ui: &Ui, response: &egui::Response, id: &str) {
    let popup = egui::Popup::default_response_id(response);
    keytips::tip(ui, response, Level::Commands, id, TipAction::Popup(popup));
}

/// Вкладка «Главная».
pub fn home(ui: &mut Ui, state: &mut HomeState, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let doc = info.has_document;
    group(ui, tr("group-clipboard"), |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                let response =
                    icon_button(ui, Icon::Paste, tr("paste"), vec2(44.0, 34.0), doc, false)
                        .on_hover_text(tr("paste-hint"));
                keytip(ui, &response, "keytip-home-paste", RibbonCommand::Paste);
                if response.clicked() {
                    out.push(RibbonCommand::Paste);
                }
                ui.label(tr("paste"));
            });
            ui.vertical(|ui| {
                let selected = doc && info.has_selection;
                for (icon, name, tip, key, command) in [
                    (
                        Icon::Cut,
                        tr("cut"),
                        tr("cut-hint"),
                        "keytip-home-cut",
                        RibbonCommand::Cut,
                    ),
                    (
                        Icon::Copy,
                        tr("copy"),
                        tr("copy-hint"),
                        "keytip-home-copy",
                        RibbonCommand::Copy,
                    ),
                ] {
                    let response = icon_text_button(ui, icon, name, selected).on_hover_text(tip);
                    keytip(ui, &response, key, command.clone());
                    if response.clicked() {
                        out.push(command);
                    }
                }
            });
        });
    });
    let mut font = Vec::new();
    let launcher = (
        tr("font-settings"),
        "keytip-home-font-dialog",
        RibbonCommand::FontDialog,
    );
    if group_with_launcher(ui, tr("font"), Some(launcher), |ui| {
        font_group(ui, state, info, &mut font);
    }) {
        font.push(RibbonCommand::FontDialog);
    }
    out.extend(font);
    let mut paragraph = Vec::new();
    let launcher = (
        tr("paragraph-settings"),
        "keytip-home-paragraph-dialog",
        RibbonCommand::ParagraphDialog,
    );
    if group_with_launcher(ui, tr("paragraph"), Some(launcher), |ui| {
        paragraph_group(ui, info, &mut paragraph);
    }) {
        paragraph.push(RibbonCommand::ParagraphDialog);
    }
    out.extend(paragraph);
    group(ui, tr("group-styles"), |ui| styles_group(ui, info, out));
    group(ui, tr("group-editing"), |ui| {
        for (icon, name, tip, key, command) in [
            (
                Icon::Search,
                tr("find"),
                tr("find-hint"),
                "keytip-home-find",
                RibbonCommand::Find,
            ),
            (
                Icon::Replace,
                tr("replace"),
                tr("replace-hint"),
                "keytip-home-replace",
                RibbonCommand::Replace,
            ),
            (
                Icon::SelectAll,
                tr("select-all"),
                tr("select-all-hint"),
                "keytip-home-select-all",
                RibbonCommand::SelectAll,
            ),
        ] {
            let response = icon_text_button(ui, icon, name, doc).on_hover_text(tip);
            keytip(ui, &response, key, command.clone());
            if response.clicked() {
                out.push(command);
            }
        }
    });
}

/// Сколько стилей видно в галерее без списка (если хватает места).
const GALLERY_TILES: usize = 6;

/// Ширина плитки галереи с промежутком и место под группу
/// «Редактирование» справа от галереи, точки экрана.
const TILE_WIDTH: f32 = 68.0;
const EDITING_WIDTH: f32 = 150.0;

/// Галерея «Стили»: образцы первых стилей и список всех.
fn styles_group(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    // Узкое окно — меньше плиток, чтобы влезла группа справа.
    let room = ui.available_width() - EDITING_WIDTH - 20.0;
    let tiles = ((room / TILE_WIDTH).floor().max(1.0) as usize).min(GALLERY_TILES);
    ui.horizontal(|ui| {
        ui.add_enabled_ui(info.has_document, |ui| {
            for style in info.styles.iter().take(tiles) {
                let on = info.style.as_deref() == Some(style.id.as_str());
                if style_tile(ui, style, on).clicked() {
                    out.push(RibbonCommand::Style(style.id.clone()));
                }
            }
        });
        let response = drop_down(ui, info.has_document, tr("all-styles"), |ui| {
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for style in &info.styles {
                        let on = info.style.as_deref() == Some(style.id.as_str());
                        if ui
                            .selectable_label(on, sample_text(ui, style, &style.name))
                            .clicked()
                        {
                            out.push(RibbonCommand::Style(style.id.clone()));
                            ui.close();
                        }
                    }
                });
        });
        popup_keytip(ui, &response, "keytip-home-styles");
    });
}

/// Образец текста в стиле (размер ужат, чтобы влезть в ленту).
fn sample_text(ui: &Ui, style: &crate::styles::GalleryStyle, text: &str) -> RichText {
    // Чёрный («Авто») в тёмной теме — светлый, как в Word.
    let colors = Palette::of(ui.ctx());
    let color = if style.color == Color32::BLACK {
        colors.text
    } else {
        style.color
    };
    let mut text = RichText::new(text)
        .size(style.size.clamp(9.0, 13.0))
        .color(color);
    if style.bold {
        text = text.strong();
    }
    if style.italic {
        text = text.italics();
    }
    text
}

/// Плитка галереи: образец «АаБбВв» и название стиля.
fn style_tile(ui: &mut Ui, style: &crate::styles::GalleryStyle, on: bool) -> egui::Response {
    let response = ui
        .vertical(|ui| {
            ui.set_width(66.0);
            ui.add_sized(
                vec2(66.0, 30.0),
                egui::Button::selectable(on, sample_text(ui, style, tr("style-sample")))
                    .wrap_mode(egui::TextWrapMode::Truncate),
            )
        })
        .inner;
    let name = ui.painter().layout_no_wrap(
        style.name.clone(),
        egui::TextStyle::Small.resolve(ui.style()),
        Palette::of(ui.ctx()).caption,
    );
    let at = egui::pos2(
        response.rect.center().x - name.size().x.min(66.0) / 2.0,
        response.rect.bottom() + 1.0,
    );
    ui.painter()
        .with_clip_rect(response.rect.expand2(vec2(0.0, 14.0)))
        .galley(at, name, Palette::of(ui.ctx()).caption);
    // Для программ чтения с экрана (и тестов) кнопка называется по стилю.
    let enabled = ui.is_enabled();
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, enabled, on, &style.name));
    response.on_hover_text(&style.name)
}

/// Межстрочные интервалы в меню, как в Word.
pub const LINE_SPACINGS: [(&str, u32); 6] = [
    ("1,0", 240),
    ("1,15", 276),
    ("1,5", 360),
    ("2,0", 480),
    ("2,5", 600),
    ("3,0", 720),
];

/// Сколько добавляет «Добавить интервал перед/после абзаца»: 12 пт.
const ADDED_SPACE: Twips = Twips(240);

/// Группа «Абзац»: сверху отступы и знаки форматирования, снизу
/// выравнивание и интервалы.
fn paragraph_group(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let doc = info.has_document;
    let button = vec2(26.0, 24.0);
    ui.horizontal(|ui| {
        for (icon, name, tip, key, kind) in [
            (
                Icon::Bullets,
                tr("bullets"),
                tr("bullets-hint"),
                "keytip-home-bullets",
                ListKind::Bullet,
            ),
            (
                Icon::Numbering,
                tr("numbering"),
                tr("numbering-hint"),
                "keytip-home-numbering",
                ListKind::Numbered,
            ),
        ] {
            let on = info.list == Some(kind);
            let response = icon_button(ui, icon, name, button, doc, on).on_hover_text(tip);
            keytip(ui, &response, key, RibbonCommand::List(kind));
            if response.clicked() {
                out.push(RibbonCommand::List(kind));
            }
        }
        for (icon, name, key, format) in [
            (
                Icon::IndentLess,
                tr("indent-decrease"),
                "keytip-home-indent-less",
                ParagraphFormat::IndentLess,
            ),
            (
                Icon::IndentMore,
                tr("indent-increase"),
                "keytip-home-indent-more",
                ParagraphFormat::IndentMore,
            ),
        ] {
            let response = icon_button(ui, icon, name, button, doc, false).on_hover_text(name);
            keytip(ui, &response, key, RibbonCommand::Paragraph(format));
            if response.clicked() {
                out.push(RibbonCommand::Paragraph(format));
            }
        }
        ui.separator();
        let name = tr("show-marks");
        let response = icon_button(ui, Icon::Pilcrow, name, button, doc, info.marks)
            .on_hover_text(tr("show-marks-hint"));
        keytip(
            ui,
            &response,
            "keytip-home-marks",
            RibbonCommand::ToggleMarks,
        );
        if response.clicked() {
            out.push(RibbonCommand::ToggleMarks);
        }
    });
    ui.horizontal(|ui| {
        for (alignment, tip, key) in [
            (
                Alignment::Left,
                tr("align-left-hint"),
                "keytip-home-align-left",
            ),
            (
                Alignment::Center,
                tr("align-center-hint"),
                "keytip-home-align-center",
            ),
            (
                Alignment::Right,
                tr("align-right-hint"),
                "keytip-home-align-right",
            ),
            (
                Alignment::Justify,
                tr("align-justify-hint"),
                "keytip-home-align-justify",
            ),
        ] {
            let on = info.alignment == Some(alignment);
            let name = tip.split(" (").next().unwrap_or(tip);
            let response =
                icon_button(ui, Icon::Align(alignment), name, button, doc, on).on_hover_text(tip);
            keytip(ui, &response, key, RibbonCommand::Align(alignment));
            if response.clicked() {
                out.push(RibbonCommand::Align(alignment));
            }
        }
        ui.separator();
        let spacing = egui::Button::new(icons::text(ui, Icon::LineSpacing, 16.0))
            .frame(false)
            .min_size(button);
        ui.add_enabled_ui(doc, |ui| {
            let (response, _) = egui::containers::menu::MenuButton::from_button(spacing)
                .ui(ui, |ui| spacing_menu(ui, info, out));
            popup_keytip(ui, &response, "keytip-home-line-spacing");
            response.on_hover_text(tr("line-spacing"));
        });
    });
}

/// Меню «Интервал»: множители и интервалы перед и после абзаца.
fn spacing_menu(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    for (label, value) in LINE_SPACINGS {
        let on = info.line_spacing == Some(LineSpacing::Multiple(value));
        if ui.selectable_label(on, label).clicked() {
            out.push(RibbonCommand::Paragraph(ParagraphFormat::LineSpacing(
                LineSpacing::Multiple(value),
            )));
            ui.close();
        }
    }
    ui.separator();
    let (before, format) = if info.space_before > 0 {
        (tr("space-before-remove"), Twips(0))
    } else {
        (tr("space-before-add"), ADDED_SPACE)
    };
    if ui.button(before).clicked() {
        out.push(RibbonCommand::Paragraph(ParagraphFormat::SpaceBefore(
            format,
        )));
        ui.close();
    }
    let (after, format) = if info.space_after > 0 {
        (tr("space-after-remove"), Twips(0))
    } else {
        (tr("space-after-add"), ADDED_SPACE)
    };
    if ui.button(after).clicked() {
        out.push(RibbonCommand::Paragraph(ParagraphFormat::SpaceAfter(
            format,
        )));
        ui.close();
    }
}

/// Группа «Шрифт»: сверху имя, размер, больше/меньше, очистка; снизу
/// Ж, К, Ч, зачёркнутый, индексы, маркер и цвет текста.
fn font_group(ui: &mut Ui, state: &mut HomeState, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let doc = info.has_document;
    let button = vec2(24.0, 24.0);
    ui.horizontal(|ui| {
        ui.add_enabled_ui(doc, |ui| {
            font_list(ui, state, info, out);
            size_box(ui, state, info, out);
        });
        for (icon, name, tip, key, format) in [
            (
                Icon::Grow,
                tr("font-grow"),
                tr("font-grow-hint"),
                "keytip-home-grow",
                RunFormat::GrowStep,
            ),
            (
                Icon::Shrink,
                tr("font-shrink"),
                tr("font-shrink-hint"),
                "keytip-home-shrink",
                RunFormat::ShrinkStep,
            ),
            (
                Icon::ClearFormat,
                tr("clear-formatting"),
                tr("clear-formatting-hint"),
                "keytip-home-clear",
                RunFormat::Clear,
            ),
        ] {
            let response = icon_button(ui, icon, name, button, doc, false).on_hover_text(tip);
            keytip(ui, &response, key, RibbonCommand::Format(format.clone()));
            if response.clicked() {
                out.push(RibbonCommand::Format(format));
            }
        }
    });
    ui.horizontal(|ui| {
        for (on, text, tip, key, format) in [
            (
                info.bold,
                RichText::new(tr("bold-letter")).strong(),
                tr("bold-hint"),
                "keytip-home-bold",
                RunFormat::Bold,
            ),
            (
                info.italic,
                RichText::new(tr("italic-letter")).italics(),
                tr("italic-hint"),
                "keytip-home-italic",
                RunFormat::Italic,
            ),
            (
                info.underline,
                RichText::new(tr("underline-letter")).underline(),
                tr("underline-hint"),
                "keytip-home-underline",
                RunFormat::Underline,
            ),
        ] {
            let toggle = egui::Button::selectable(on, text).min_size(button);
            let response = ui.add_enabled(doc, toggle).on_hover_text(tip);
            keytip(ui, &response, key, RibbonCommand::Format(format.clone()));
            if response.clicked() {
                out.push(RibbonCommand::Format(format));
            }
        }
        for (icon, on, name, tip, key, format) in [
            (
                Icon::Strike,
                info.strike,
                tr("strikethrough"),
                tr("strikethrough"),
                "keytip-home-strike",
                RunFormat::Strike,
            ),
            (
                Icon::Subscript,
                info.subscript,
                tr("subscript"),
                tr("subscript-hint"),
                "keytip-home-subscript",
                RunFormat::Subscript,
            ),
            (
                Icon::Superscript,
                info.superscript,
                tr("superscript"),
                tr("superscript-hint"),
                "keytip-home-superscript",
                RunFormat::Superscript,
            ),
        ] {
            let response = icon_button(ui, icon, name, button, doc, on).on_hover_text(tip);
            keytip(ui, &response, key, RibbonCommand::Format(format.clone()));
            if response.clicked() {
                out.push(RibbonCommand::Format(format));
            }
        }
        ui.separator();
        let marker = state.highlight.rgb().map_or(Color32::TRANSPARENT, color32);
        let highlight = RibbonCommand::Format(RunFormat::Highlight(state.highlight));
        let response = color_button(ui, Icon::Highlight, tr("highlight-color"), marker, doc)
            .on_hover_text(tr("highlight-color"));
        keytip(ui, &response, "keytip-home-highlight", highlight.clone());
        if response.clicked() {
            out.push(highlight);
        }
        drop_down(ui, doc, tr("highlight-colors"), |ui| {
            highlight_menu(ui, state, out);
        });
        let color = font_color32(state.font_color);
        let font_color = RibbonCommand::Format(RunFormat::Color(state.font_color));
        let response = color_button(ui, Icon::FontColor, tr("font-color"), color, doc)
            .on_hover_text(tr("font-color"));
        keytip(ui, &response, "keytip-home-font-color", font_color.clone());
        if response.clicked() {
            out.push(font_color);
        }
        drop_down(ui, doc, tr("font-colors"), |ui| {
            color_menu(ui, state, out);
        });
    });
}

/// Узкая кнопка со стрелкой, открывающая меню.
fn drop_down(ui: &mut Ui, enabled: bool, name: &str, menu: impl FnOnce(&mut Ui)) -> egui::Response {
    let arrow = egui::Button::new(icons::text(ui, Icon::ChevronDown, 10.0))
        .frame(false)
        .min_size(vec2(12.0, 24.0));
    ui.add_enabled_ui(enabled, |ui| {
        let (response, _) = egui::containers::menu::MenuButton::from_button(arrow).ui(ui, menu);
        response.on_hover_text(name)
    })
    .inner
}

/// Список шрифтов с поиском.
fn font_list(ui: &mut Ui, state: &mut HomeState, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let response = egui::ComboBox::from_id_salt("font-name")
        .width(120.0)
        .selected_text(info.font.as_str())
        .height(320.0)
        .show_ui(ui, |ui| {
            ui.add(
                TextEdit::singleline(&mut state.font_filter)
                    .hint_text(tr("font-search"))
                    .desired_width(160.0),
            );
            let filter = state.font_filter.to_lowercase();
            for name in info
                .fonts
                .iter()
                .filter(|n| n.to_lowercase().contains(&filter))
            {
                if ui.selectable_label(*name == info.font, name).clicked() {
                    out.push(RibbonCommand::Format(RunFormat::Font(name.clone())));
                    state.font_filter.clear();
                }
            }
        })
        .response;
    popup_keytip(ui, &response, "keytip-home-font");
    response.on_hover_text(tr("font"));
}

/// Поле «Размер шрифта» со списком: размер можно набрать (Enter —
/// применить, Esc — отменить) или выбрать.
fn size_box(ui: &mut Ui, state: &mut HomeState, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let shown = size_label(info.size);
    let text = state.size_text.get_or_insert_with(|| shown.clone());
    let response = ui
        .add(TextEdit::singleline(text).desired_width(30.0))
        .on_hover_text(tr("font-size-box"));
    keytips::tip(
        ui,
        &response,
        Level::Commands,
        "keytip-home-size",
        TipAction::Focus(response.id),
    );
    if response.lost_focus() {
        let cancelled = ui.input(|i| i.key_pressed(Key::Escape));
        if !cancelled
            && *text != shown
            && let Some(size) = parse_size(text)
        {
            out.push(RibbonCommand::Format(RunFormat::Size(size)));
        }
        state.size_text = None;
    } else if !response.has_focus() {
        state.size_text = None;
    }
    drop_down(ui, true, tr("font-sizes"), |ui| {
        for size in FONT_SIZES {
            let half = (size * 2.0) as u32;
            if ui
                .selectable_label(half == info.size, size_label(half))
                .clicked()
            {
                out.push(RibbonCommand::Format(RunFormat::Size(HalfPoints(half))));
                ui.close();
            }
        }
    });
}

/// Палитра маркера.
fn highlight_menu(ui: &mut Ui, state: &mut HomeState, out: &mut Vec<RibbonCommand>) {
    egui::Grid::new("highlights")
        .spacing(vec2(2.0, 2.0))
        .show(ui, |ui| {
            for (i, (highlight, name)) in HIGHLIGHTS.into_iter().enumerate() {
                let fill = highlight.rgb().map_or(Color32::TRANSPARENT, color32);
                if swatch(ui, fill, tr(name)).clicked() {
                    state.highlight = highlight;
                    out.push(RibbonCommand::Format(RunFormat::Highlight(highlight)));
                    ui.close();
                }
                if i % 5 == 4 {
                    ui.end_row();
                }
            }
        });
    if ui.button(tr("color-none")).clicked() {
        out.push(RibbonCommand::Format(RunFormat::Highlight(Highlight::None)));
        ui.close();
    }
}

/// Палитра цвета текста: «Авто», стандартные цвета, другие цвета.
fn color_menu(ui: &mut Ui, state: &mut HomeState, out: &mut Vec<RibbonCommand>) {
    let mut pick = |color: Color, out: &mut Vec<RibbonCommand>| {
        state.font_color = color;
        out.push(RibbonCommand::Format(RunFormat::Color(color)));
    };
    if ui.button(tr("color-auto")).clicked() {
        pick(Color::Auto, out);
        ui.close();
    }
    ui.label(RichText::new(tr("color-standard")).small());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (name, rgb) in STANDARD_COLORS {
            if swatch(ui, color32(rgb), tr(name)).clicked() {
                pick(Color::Rgb(rgb.0, rgb.1, rgb.2), out);
                ui.close();
            }
        }
    });
    ui.menu_button(tr("color-more"), |ui| {
        egui::color_picker::color_picker_color32(
            ui,
            &mut state.custom,
            egui::color_picker::Alpha::Opaque,
        );
        if ui.button(tr("apply")).clicked() {
            let [r, g, b, _] = state.custom.to_array();
            state.font_color = Color::Rgb(r, g, b);
            out.push(RibbonCommand::Format(RunFormat::Color(state.font_color)));
            ui.close();
        }
    });
}

/// Квадратик цвета в палитре.
fn swatch(ui: &mut Ui, fill: Color32, name: &str) -> egui::Response {
    let button = egui::Button::new("")
        .fill(fill)
        .stroke(egui::Stroke::new(1.0, Color32::from_gray(160)))
        .min_size(vec2(16.0, 16.0));
    ui.add(button).on_hover_text(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_shown_and_read_like_in_word() {
        assert_eq!(size_label(22), "11");
        assert_eq!(size_label(21), "10,5");
        assert_eq!(size_label(0), "");
        assert_eq!(parse_size("10,5"), Some(HalfPoints(21)));
        assert_eq!(parse_size(" 12 "), Some(HalfPoints(24)));
        assert_eq!(parse_size("11.3"), Some(HalfPoints(23)));
        assert_eq!(parse_size("0"), None);
        assert_eq!(parse_size("2000"), None);
        assert_eq!(parse_size("abc"), None);
    }
}
