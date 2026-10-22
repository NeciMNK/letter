//! Лента в стиле Word: панель быстрого доступа (сохранить, отменить,
//! повторить), меню «Файл», вкладки и их группы команд. Лента сама
//! ничего не делает с документом — она возвращает выбранные команды
//! ([`RibbonCommand`]), а выполняет их окно.
//!
//! Вкладка «Главная» — в [`crate::home`].

use crate::theme::Palette;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{
    self, Align, Color32, CornerRadius, Layout, Rect, RichText, Stroke, Ui, pos2, vec2,
};
use letter_editor::{ParagraphFormat, RunFormat};

use crate::home::{HomeState, home};
use crate::icons::{Icon, icon_button};
use letter_model::Alignment;

/// Высота области с группами команд, точки экрана.
const BODY_HEIGHT: f32 = 78.0;

/// Фон ленты и выделенной вкладки.
pub(crate) const ACCENT: Color32 = Color32::from_rgb(43, 87, 154);

/// Вкладка ленты.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    /// «Главная».
    #[default]
    Home,
    /// «Вставка».
    Insert,
    /// «Макет».
    Layout,
    /// «Ссылки».
    References,
    /// «Рецензирование».
    Review,
    /// «Вид».
    View,
}

impl Tab {
    /// Все вкладки по порядку.
    pub const ALL: [Tab; 6] = [
        Tab::Home,
        Tab::Insert,
        Tab::Layout,
        Tab::References,
        Tab::Review,
        Tab::View,
    ];

    /// Название на вкладке.
    pub fn title(self) -> &'static str {
        match self {
            Tab::Home => "Главная",
            Tab::Insert => "Вставка",
            Tab::Layout => "Макет",
            Tab::References => "Ссылки",
            Tab::Review => "Рецензирование",
            Tab::View => "Вид",
        }
    }
}

/// Команда, выбранная на ленте.
#[derive(Debug, Clone, PartialEq)]
pub enum RibbonCommand {
    /// «Файл → Создать».
    New,
    /// «Файл → Открыть».
    Open,
    /// Открыть недавний файл.
    OpenRecent(PathBuf),
    /// «Сохранить».
    Save,
    /// «Сохранить как».
    SaveAs,
    /// «Выход».
    Exit,
    /// Открыть представление «Файл».
    Backstage,
    /// «Файл → Закрыть»: закрыть документ, программа остаётся.
    CloseDocument,
    /// «Экспорт → Изменить тип файла»: сохранить как файл с этим
    /// расширением.
    SaveAsFormat(&'static str),
    /// «Параметры» изменены.
    ChangeSettings(crate::settings::Settings),
    /// Отменить.
    Undo,
    /// Повторить.
    Redo,
    /// Вырезать.
    Cut,
    /// Копировать.
    Copy,
    /// Вставить.
    Paste,
    /// Оформление символов.
    Format(RunFormat),
    /// Выравнивание абзаца.
    Align(Alignment),
    /// Отступы и интервалы абзаца.
    Paragraph(ParagraphFormat),
    /// Показать или скрыть знаки форматирования.
    ToggleMarks,
    /// Применить стиль с этим идентификатором.
    Style(String),
    /// Окно «Найти».
    Find,
    /// Окно «Заменить».
    Replace,
    /// «Выделить все».
    SelectAll,
    /// Вставить способом из «Параметров вставки».
    PasteSpecial(crate::clipboard::PasteMode),
    /// Окно «Шрифт».
    FontDialog,
    /// Окно «Абзац».
    ParagraphDialog,
    /// Масштаб крупнее.
    ZoomIn,
    /// Масштаб мельче.
    ZoomOut,
    /// Масштаб 100 %.
    ZoomReset,
    /// Масштаб по ширине страницы.
    PageWidth,
    /// Показать или скрыть линейку.
    ToggleRuler,
}

/// Что лента показывает о документе.
#[derive(Debug, Clone, Default)]
pub struct RibbonInfo {
    /// Документ открыт.
    pub has_document: bool,
    /// Есть что отменить и повторить.
    pub can_undo: bool,
    /// Есть что повторить.
    pub can_redo: bool,
    /// Что-то выделено.
    pub has_selection: bool,
    /// Оформление у курсора: полужирный, курсив, подчёркивание.
    pub bold: bool,
    /// Курсив.
    pub italic: bool,
    /// Подчёркивание.
    pub underline: bool,
    /// Зачёркивание.
    pub strike: bool,
    /// Подстрочный знак.
    pub subscript: bool,
    /// Надстрочный знак.
    pub superscript: bool,
    /// Имя шрифта у курсора.
    pub font: String,
    /// Размер шрифта у курсора, полупункты (0 — неизвестен).
    pub size: u32,
    /// Шрифты для списка «Шрифт».
    pub fonts: Arc<[String]>,
    /// Выравнивание абзаца с курсором.
    pub alignment: Option<Alignment>,
    /// Межстрочный интервал абзаца с курсором.
    pub line_spacing: Option<letter_model::LineSpacing>,
    /// Интервалы перед и после абзаца с курсором, твипы.
    pub space_before: i32,
    /// Интервал после абзаца.
    pub space_after: i32,
    /// Знаки форматирования показаны.
    pub marks: bool,
    /// Стили для галереи.
    pub styles: Vec<crate::styles::GalleryStyle>,
    /// Стиль абзаца с курсором.
    pub style: Option<String>,
    /// Масштаб «по ширине страницы» включён.
    pub page_width: bool,
    /// Линейка показана.
    pub ruler: bool,
}

/// Состояние ленты.
#[derive(Debug, Clone, Default)]
pub struct Ribbon {
    /// Выбранная вкладка.
    pub tab: Tab,
    /// Лента свёрнута (двойной щелчок по вкладке): видны только вкладки.
    pub collapsed: bool,
    /// Состояние вкладки «Главная».
    pub home: HomeState,
}

impl Ribbon {
    /// Рисует ленту целиком и возвращает выбранные команды.
    pub fn ui(&mut self, ui: &mut Ui, info: &RibbonInfo) -> Vec<RibbonCommand> {
        let mut out = Vec::new();
        ui.spacing_mut().item_spacing.x = 2.0;
        quick_access(ui, info, &mut out);
        self.tabs(ui, &mut out);
        if !self.collapsed {
            let colors = Palette::of(ui.ctx());
            egui::Frame::new()
                .fill(colors.ribbon)
                .stroke(Stroke::new(1.0, colors.border))
                .inner_margin(egui::Margin::symmetric(4, 2))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.set_min_height(BODY_HEIGHT);
                    ui.set_max_height(BODY_HEIGHT);
                    ui.horizontal(|ui| match self.tab {
                        Tab::Home => home(ui, &mut self.home, info, &mut out),
                        Tab::View => view(ui, info, &mut out),
                        _ => {
                            ui.label(
                                RichText::new("Команды этой вкладки появятся позже.")
                                    .color(Color32::GRAY),
                            );
                        }
                    });
                });
        }
        out
    }

    /// Меню «Файл» и вкладки.
    fn tabs(&mut self, ui: &mut Ui, out: &mut Vec<RibbonCommand>) {
        ui.horizontal(|ui| {
            let file = egui::Button::new(RichText::new("Файл").color(Color32::WHITE))
                .fill(ACCENT)
                .corner_radius(CornerRadius::same(2))
                .min_size(vec2(52.0, 0.0));
            if ui
                .add(file)
                .on_hover_text("Создать, открыть, сохранить, параметры")
                .clicked()
            {
                out.push(RibbonCommand::Backstage);
            }
            for tab in Tab::ALL {
                let selected = self.tab == tab && !self.collapsed;
                let text = if selected {
                    RichText::new(tab.title())
                        .color(Palette::of(ui.ctx()).accent_text)
                        .strong()
                } else {
                    RichText::new(tab.title())
                };
                let response = ui.add(egui::Button::selectable(selected, text));
                if response.double_clicked() {
                    self.collapsed = !self.collapsed;
                    self.tab = tab;
                } else if response.clicked() {
                    if self.collapsed && self.tab == tab {
                        self.collapsed = false;
                    }
                    self.tab = tab;
                }
            }
        });
    }
}

/// Панель быстрого доступа.
fn quick_access(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    ui.horizontal(|ui| {
        let size = vec2(24.0, 20.0);
        for (icon, name, tip, enabled, command) in [
            (
                Icon::Save,
                "Сохранить",
                "Сохранить (Ctrl+S)",
                info.has_document,
                RibbonCommand::Save,
            ),
            (
                Icon::Undo,
                "Отменить",
                "Отменить (Ctrl+Z)",
                info.can_undo,
                RibbonCommand::Undo,
            ),
            (
                Icon::Redo,
                "Повторить",
                "Повторить (Ctrl+Y)",
                info.can_redo,
                RibbonCommand::Redo,
            ),
        ] {
            if icon_button(ui, icon, name, size, enabled, false)
                .on_hover_text(tip)
                .clicked()
            {
                out.push(command);
            }
        }
    });
}

/// Группа команд: содержимое и подпись снизу по центру, справа —
/// разделитель. Ширина группы — по содержимому.
pub(crate) fn group(ui: &mut Ui, caption: &str, content: impl FnOnce(&mut Ui)) {
    group_with_launcher(ui, caption, None, content);
}

/// Группа ленты с кнопкой-стрелкой у подписи, которая открывает окно
/// группы (как «Шрифт» и «Абзац» в Word); `launcher` — название окна.
/// Возвращает, нажата ли кнопка.
pub(crate) fn group_with_launcher(
    ui: &mut Ui,
    caption: &str,
    launcher: Option<&str>,
    content: impl FnOnce(&mut Ui),
) -> bool {
    let mut clicked = false;
    ui.vertical(|ui| {
        let top = ui.cursor().top();
        ui.scope(content);
        let width = ui.min_rect().width();
        let caption_height = 14.0;
        let used = ui.cursor().top() - top;
        ui.add_space((BODY_HEIGHT - 6.0 - caption_height - used).max(0.0));
        ui.allocate_ui_with_layout(
            vec2(width, caption_height),
            Layout::top_down(Align::Center),
            |ui| {
                ui.label(
                    RichText::new(caption)
                        .small()
                        .color(Palette::of(ui.ctx()).caption),
                );
            },
        );
        if let Some(name) = launcher {
            let rect = ui.min_rect();
            let at = Rect::from_min_size(
                pos2(
                    rect.right() - caption_height,
                    rect.bottom() - caption_height,
                ),
                vec2(caption_height, caption_height),
            );
            let response = ui
                .put(at, |ui: &mut Ui| {
                    crate::icons::icon_button(
                        ui,
                        crate::icons::Icon::Launcher,
                        name,
                        vec2(caption_height, caption_height),
                        true,
                        false,
                    )
                })
                .on_hover_text(name);
            clicked = response.clicked();
        }
    });
    ui.separator();
    clicked
}

/// Вкладка «Вид».
fn view(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    group(ui, "Показать", |ui| {
        let mut ruler = info.ruler;
        if ui.checkbox(&mut ruler, "Линейка").changed() {
            out.push(RibbonCommand::ToggleRuler);
        }
    });
    group(ui, "Масштаб", |ui| {
        ui.horizontal(|ui| {
            if ui.button("Увеличить").on_hover_text("Ctrl+=").clicked() {
                out.push(RibbonCommand::ZoomIn);
            }
            if ui.button("Уменьшить").on_hover_text("Ctrl+-").clicked() {
                out.push(RibbonCommand::ZoomOut);
            }
        });
        ui.horizontal(|ui| {
            if ui.button("100 %").on_hover_text("Ctrl+0").clicked() {
                out.push(RibbonCommand::ZoomReset);
            }
            if ui
                .add(egui::Button::selectable(
                    info.page_width,
                    "По ширине страницы",
                ))
                .clicked()
            {
                out.push(RibbonCommand::PageWidth);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    struct State {
        ribbon: Ribbon,
        info: RibbonInfo,
        out: Vec<RibbonCommand>,
    }

    fn harness(info: RibbonInfo) -> Harness<'static, State> {
        let state = State {
            ribbon: Ribbon::default(),
            info,
            out: Vec::new(),
        };
        Harness::builder()
            .with_size(vec2(1200.0, 200.0))
            .build_ui_state(
                |ui, state: &mut State| {
                    let out = state.ribbon.ui(ui, &state.info);
                    state.out.extend(out);
                },
                state,
            )
    }

    fn document() -> RibbonInfo {
        RibbonInfo {
            has_document: true,
            can_undo: true,
            has_selection: true,
            font: "Calibri".into(),
            size: 22,
            fonts: vec!["Arial".to_string(), "Calibri".to_string()].into(),
            styles: crate::styles::gallery(&crate::files::blank()),
            style: Some("Normal".into()),
            ..RibbonInfo::default()
        }
    }

    #[test]
    fn style_gallery_applies_styles() {
        let mut h = harness(document());
        h.get_by_label("Заголовок 1").click();
        h.run();
        assert_eq!(h.state().out, vec![RibbonCommand::Style("Heading1".into())]);
    }

    #[test]
    fn font_group_gives_commands() {
        let mut h = harness(document());
        for name in [
            "Зачёркнутый",
            "Надстрочный знак",
            "Цвет текста",
            "Цвет выделения текста",
            "Очистить оформление",
        ] {
            h.get_by_label(name).click();
            h.run();
        }
        assert_eq!(
            h.state().out,
            vec![
                RibbonCommand::Format(RunFormat::Strike),
                RibbonCommand::Format(RunFormat::Superscript),
                RibbonCommand::Format(RunFormat::Color(letter_model::Color::Rgb(255, 0, 0))),
                RibbonCommand::Format(RunFormat::Highlight(letter_model::Highlight::Yellow)),
                RibbonCommand::Format(RunFormat::Clear),
            ]
        );
    }

    #[test]
    fn font_name_and_size_are_chosen() {
        let mut h = harness(document());
        let fonts = h.get_by_role(egui::accesskit::Role::ComboBox);
        assert_eq!(fonts.value().as_deref(), Some("Calibri"));
        fonts.click();
        h.run();
        h.get_by_label("Arial").click();
        h.run();
        let size = h.get_by_role(egui::accesskit::Role::TextInput);
        assert_eq!(size.value().as_deref(), Some("11"));
        size.focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.get_by_role(egui::accesskit::Role::TextInput)
            .type_text("10,5");
        h.run();
        h.key_press(egui::Key::Enter);
        h.run();
        assert_eq!(
            h.state().out,
            vec![
                RibbonCommand::Format(RunFormat::Font("Arial".into())),
                RibbonCommand::Format(RunFormat::Size(letter_model::HalfPoints(21))),
            ]
        );
    }

    #[test]
    fn home_buttons_give_commands() {
        let mut h = harness(document());
        h.get_by_label("Ж").click();
        h.run();
        h.get_by_label("Копировать").click();
        h.run();
        h.get_by_label("Отменить").click();
        h.run();
        assert_eq!(
            h.state().out,
            vec![
                RibbonCommand::Format(RunFormat::Bold),
                RibbonCommand::Copy,
                RibbonCommand::Undo
            ]
        );
    }

    #[test]
    fn disabled_without_a_document() {
        let mut h = harness(RibbonInfo::default());
        h.get_by_label("Ж").click();
        h.run();
        h.get_by_label("Повторить").click();
        h.run();
        assert!(h.state().out.is_empty());
    }

    #[test]
    fn tabs_switch_and_collapse() {
        let mut h = harness(document());
        h.get_by_label("Вид").click();
        h.run();
        assert_eq!(h.state().ribbon.tab, Tab::View);
        h.get_by_label("Линейка").click();
        h.run();
        h.get_by_label("По ширине страницы").click();
        h.run();
        assert_eq!(
            h.state().out,
            vec![RibbonCommand::ToggleRuler, RibbonCommand::PageWidth]
        );
    }

    #[test]
    fn double_click_collapses_and_a_click_expands() {
        let mut h = harness(document());
        // Два щелчка в одном кадре: в тестовом окне время между кадрами
        // не всегда успевает для двойного щелчка.
        let pos = h.get_by_label("Вид").rect().center();
        for pressed in [true, false, true, false] {
            h.input_mut().events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        h.run();
        assert!(h.state().ribbon.collapsed);
        assert_eq!(h.state().ribbon.tab, Tab::View);
        assert!(h.query_by_label("Линейка").is_none());

        let mut h = harness(document());
        h.state_mut().ribbon.collapsed = true;
        h.run();
        assert!(h.query_by_label("Ж").is_none());
        h.get_by_label("Главная").click();
        h.run();
        assert!(!h.state().ribbon.collapsed);
        assert!(h.query_by_label("Ж").is_some());
    }

    #[test]
    fn file_button_opens_backstage() {
        let mut h = harness(document());
        h.get_by_label("Файл").click();
        h.run();
        assert_eq!(h.state().out, vec![RibbonCommand::Backstage]);
    }
}
