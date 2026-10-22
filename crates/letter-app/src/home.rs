//! Вкладка «Главная» ленты: буфер обмена, шрифт, абзац.

use eframe::egui::{self, Color32, Key, RichText, TextEdit, Ui, vec2};
use letter_editor::RunFormat;
use letter_model::{Alignment, Color, HalfPoints, Highlight};

use crate::icons::{self, Icon, color_button, icon_button, icon_text_button};
use crate::ribbon::{RibbonCommand, RibbonInfo, group};

/// Размеры в списке «Размер шрифта», как в Word, пт.
pub const FONT_SIZES: [f32; 17] = [
    8.0, 9.0, 10.0, 10.5, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0, 36.0, 48.0,
    72.0,
];

/// Стандартные цвета палитры Word: название и цвет.
pub const STANDARD_COLORS: [(&str, (u8, u8, u8)); 10] = [
    ("Тёмно-красный", (0xC0, 0x00, 0x00)),
    ("Красный", (0xFF, 0x00, 0x00)),
    ("Оранжевый", (0xFF, 0xC0, 0x00)),
    ("Жёлтый", (0xFF, 0xFF, 0x00)),
    ("Светло-зелёный", (0x92, 0xD0, 0x50)),
    ("Зелёный", (0x00, 0xB0, 0x50)),
    ("Голубой", (0x00, 0xB0, 0xF0)),
    ("Синий", (0x00, 0x70, 0xC0)),
    ("Тёмно-синий", (0x00, 0x20, 0x60)),
    ("Фиолетовый", (0x70, 0x30, 0xA0)),
];

/// Цвета маркера в порядке палитры Word (пять в ряд) и их названия.
pub const HIGHLIGHTS: [(Highlight, &str); 15] = [
    (Highlight::Yellow, "Жёлтый"),
    (Highlight::Green, "Ярко-зелёный"),
    (Highlight::Cyan, "Бирюзовый"),
    (Highlight::Magenta, "Розовый"),
    (Highlight::Blue, "Синий"),
    (Highlight::Red, "Красный"),
    (Highlight::DarkBlue, "Тёмно-синий"),
    (Highlight::DarkCyan, "Сине-зелёный"),
    (Highlight::DarkGreen, "Зелёный"),
    (Highlight::DarkMagenta, "Фиолетовый"),
    (Highlight::DarkRed, "Тёмно-красный"),
    (Highlight::DarkYellow, "Коричнево-жёлтый"),
    (Highlight::DarkGray, "Серый 50 %"),
    (Highlight::LightGray, "Серый 25 %"),
    (Highlight::Black, "Чёрный"),
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

/// Вкладка «Главная».
pub fn home(ui: &mut Ui, state: &mut HomeState, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let doc = info.has_document;
    group(ui, "Буфер обмена", |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                if icon_button(ui, Icon::Paste, "Вставить", vec2(44.0, 34.0), doc, false)
                    .on_hover_text("Вставить (Ctrl+V)")
                    .clicked()
                {
                    out.push(RibbonCommand::Paste);
                }
                ui.label("Вставить");
            });
            ui.vertical(|ui| {
                let selected = doc && info.has_selection;
                for (icon, name, tip, command) in [
                    (
                        Icon::Cut,
                        "Вырезать",
                        "Вырезать (Ctrl+X)",
                        RibbonCommand::Cut,
                    ),
                    (
                        Icon::Copy,
                        "Копировать",
                        "Копировать (Ctrl+C)",
                        RibbonCommand::Copy,
                    ),
                ] {
                    if icon_text_button(ui, icon, name, selected)
                        .on_hover_text(tip)
                        .clicked()
                    {
                        out.push(command);
                    }
                }
            });
        });
    });
    group(ui, "Шрифт", |ui| font_group(ui, state, info, out));
    group(ui, "Абзац", |ui| {
        ui.horizontal(|ui| {
            for (alignment, tip) in [
                (Alignment::Left, "Выровнять по левому краю (Ctrl+L)"),
                (Alignment::Center, "По центру (Ctrl+E)"),
                (Alignment::Right, "Выровнять по правому краю (Ctrl+R)"),
                (Alignment::Justify, "По ширине (Ctrl+J)"),
            ] {
                let on = info.alignment == Some(alignment);
                let name = tip.split(" (").next().unwrap_or(tip);
                if icon_button(ui, Icon::Align(alignment), name, vec2(26.0, 24.0), doc, on)
                    .on_hover_text(tip)
                    .clicked()
                {
                    out.push(RibbonCommand::Align(alignment));
                }
            }
        });
    });
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
        for (icon, name, tip, format) in [
            (
                Icon::Grow,
                "Увеличить размер шрифта",
                "Увеличить размер шрифта (Ctrl+])",
                RunFormat::Grow,
            ),
            (
                Icon::Shrink,
                "Уменьшить размер шрифта",
                "Уменьшить размер шрифта (Ctrl+[)",
                RunFormat::Shrink,
            ),
            (
                Icon::ClearFormat,
                "Очистить оформление",
                "Очистить оформление (Ctrl+Пробел)",
                RunFormat::Clear,
            ),
        ] {
            if icon_button(ui, icon, name, button, doc, false)
                .on_hover_text(tip)
                .clicked()
            {
                out.push(RibbonCommand::Format(format));
            }
        }
    });
    ui.horizontal(|ui| {
        for (on, text, tip, format) in [
            (
                info.bold,
                RichText::new("Ж").strong(),
                "Полужирный (Ctrl+B)",
                RunFormat::Bold,
            ),
            (
                info.italic,
                RichText::new("К").italics(),
                "Курсив (Ctrl+I)",
                RunFormat::Italic,
            ),
            (
                info.underline,
                RichText::new("Ч").underline(),
                "Подчёркнутый (Ctrl+U)",
                RunFormat::Underline,
            ),
        ] {
            let toggle = egui::Button::selectable(on, text).min_size(button);
            if ui.add_enabled(doc, toggle).on_hover_text(tip).clicked() {
                out.push(RibbonCommand::Format(format));
            }
        }
        for (icon, on, name, format) in [
            (Icon::Strike, info.strike, "Зачёркнутый", RunFormat::Strike),
            (
                Icon::Subscript,
                info.subscript,
                "Подстрочный знак",
                RunFormat::Subscript,
            ),
            (
                Icon::Superscript,
                info.superscript,
                "Надстрочный знак",
                RunFormat::Superscript,
            ),
        ] {
            if icon_button(ui, icon, name, button, doc, on)
                .on_hover_text(name)
                .clicked()
            {
                out.push(RibbonCommand::Format(format));
            }
        }
        ui.separator();
        let marker = state.highlight.rgb().map_or(Color32::TRANSPARENT, color32);
        if color_button(ui, Icon::Highlight, "Цвет выделения текста", marker, doc)
            .on_hover_text("Цвет выделения текста")
            .clicked()
        {
            out.push(RibbonCommand::Format(RunFormat::Highlight(state.highlight)));
        }
        drop_down(ui, doc, "Цвета выделения", |ui| {
            highlight_menu(ui, state, out);
        });
        let color = font_color32(state.font_color);
        if color_button(ui, Icon::FontColor, "Цвет текста", color, doc)
            .on_hover_text("Цвет текста")
            .clicked()
        {
            out.push(RibbonCommand::Format(RunFormat::Color(state.font_color)));
        }
        drop_down(ui, doc, "Цвета текста", |ui| {
            color_menu(ui, state, out);
        });
    });
}

/// Узкая кнопка со стрелкой, открывающая меню.
fn drop_down(ui: &mut Ui, enabled: bool, name: &str, menu: impl FnOnce(&mut Ui)) {
    let arrow = egui::Button::new(icons::text(ui, Icon::ChevronDown, 10.0))
        .frame(false)
        .min_size(vec2(12.0, 24.0));
    ui.add_enabled_ui(enabled, |ui| {
        let (response, _) = egui::containers::menu::MenuButton::from_button(arrow).ui(ui, menu);
        response.on_hover_text(name);
    });
}

/// Список шрифтов с поиском.
fn font_list(ui: &mut Ui, state: &mut HomeState, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    egui::ComboBox::from_id_salt("font-name")
        .width(120.0)
        .selected_text(info.font.as_str())
        .height(320.0)
        .show_ui(ui, |ui| {
            ui.add(
                TextEdit::singleline(&mut state.font_filter)
                    .hint_text("Найти шрифт")
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
        .response
        .on_hover_text("Шрифт");
}

/// Поле «Размер шрифта» со списком: размер можно набрать (Enter —
/// применить, Esc — отменить) или выбрать.
fn size_box(ui: &mut Ui, state: &mut HomeState, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let shown = size_label(info.size);
    let text = state.size_text.get_or_insert_with(|| shown.clone());
    let response = ui
        .add(TextEdit::singleline(text).desired_width(30.0))
        .on_hover_text("Размер шрифта");
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
    drop_down(ui, true, "Размеры шрифта", |ui| {
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
                if swatch(ui, fill, name).clicked() {
                    state.highlight = highlight;
                    out.push(RibbonCommand::Format(RunFormat::Highlight(highlight)));
                    ui.close();
                }
                if i % 5 == 4 {
                    ui.end_row();
                }
            }
        });
    if ui.button("Нет цвета").clicked() {
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
    if ui.button("Авто").clicked() {
        pick(Color::Auto, out);
        ui.close();
    }
    ui.label(RichText::new("Стандартные цвета").small());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (name, rgb) in STANDARD_COLORS {
            if swatch(ui, color32(rgb), name).clicked() {
                pick(Color::Rgb(rgb.0, rgb.1, rgb.2), out);
                ui.close();
            }
        }
    });
    ui.menu_button("Другие цвета…", |ui| {
        egui::color_picker::color_picker_color32(
            ui,
            &mut state.custom,
            egui::color_picker::Alpha::Opaque,
        );
        if ui.button("Применить").clicked() {
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
