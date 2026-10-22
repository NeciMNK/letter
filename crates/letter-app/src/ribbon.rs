//! Лента в стиле Word: панель быстрого доступа (сохранить, отменить,
//! повторить), меню «Файл», вкладки и их группы команд. Лента сама
//! ничего не делает с документом — она возвращает выбранные команды
//! ([`RibbonCommand`]), а выполняет их окно.
//!
//! Значки пока простые, нарисованные линиями ([`crate::icons`]); набор
//! значков — задача F4.2.

use std::path::PathBuf;

use eframe::egui::{self, Align, Color32, CornerRadius, Layout, RichText, Stroke, Ui, vec2};
use letter_editor::RunFormat;

use crate::icons::{Icon, icon_button, icon_text_button};
use letter_model::Alignment;

/// Высота области с группами команд, точки экрана.
const BODY_HEIGHT: f32 = 78.0;

/// Фон ленты и выделенной вкладки.
const RIBBON_FILL: Color32 = Color32::from_gray(247);
const ACCENT: Color32 = Color32::from_rgb(43, 87, 154);

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
    /// Выравнивание абзаца с курсором.
    pub alignment: Option<Alignment>,
    /// Недавние файлы.
    pub recent: Vec<PathBuf>,
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
}

impl Ribbon {
    /// Рисует ленту целиком и возвращает выбранные команды.
    pub fn ui(&mut self, ui: &mut Ui, info: &RibbonInfo) -> Vec<RibbonCommand> {
        let mut out = Vec::new();
        ui.spacing_mut().item_spacing.x = 2.0;
        quick_access(ui, info, &mut out);
        self.tabs(ui, info, &mut out);
        if !self.collapsed {
            egui::Frame::new()
                .fill(RIBBON_FILL)
                .stroke(Stroke::new(1.0, Color32::from_gray(215)))
                .inner_margin(egui::Margin::symmetric(4, 2))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.set_min_height(BODY_HEIGHT);
                    ui.set_max_height(BODY_HEIGHT);
                    ui.horizontal(|ui| match self.tab {
                        Tab::Home => home(ui, info, &mut out),
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
    fn tabs(&mut self, ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
        ui.horizontal(|ui| {
            let file = egui::Button::new(RichText::new("Файл").color(Color32::WHITE))
                .fill(ACCENT)
                .corner_radius(CornerRadius::same(2))
                .min_size(vec2(52.0, 0.0));
            let (response, _) = egui::containers::menu::MenuButton::from_button(file)
                .ui(ui, |ui| file_menu(ui, info, out));
            response.on_hover_text("Создать, открыть, сохранить документ");
            for tab in Tab::ALL {
                let selected = self.tab == tab && !self.collapsed;
                let text = if selected {
                    RichText::new(tab.title()).color(ACCENT).strong()
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

/// Меню «Файл». Полноценное представление Backstage — задача F4.4.
fn file_menu(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let item = |ui: &mut Ui, enabled: bool, text: &str, shortcut: &str| {
        ui.add_enabled(enabled, egui::Button::new(text).shortcut_text(shortcut))
            .clicked()
    };
    if item(ui, true, "Создать", "Ctrl+N") {
        out.push(RibbonCommand::New);
    }
    if item(ui, true, "Открыть…", "Ctrl+O") {
        out.push(RibbonCommand::Open);
    }
    ui.add_enabled_ui(!info.recent.is_empty(), |ui| {
        ui.menu_button("Последние", |ui| {
            for path in &info.recent {
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                if ui
                    .button(name)
                    .on_hover_text(path.display().to_string())
                    .clicked()
                {
                    out.push(RibbonCommand::OpenRecent(path.clone()));
                }
            }
        });
    });
    ui.separator();
    if item(ui, info.has_document, "Сохранить", "Ctrl+S") {
        out.push(RibbonCommand::Save);
    }
    if item(ui, info.has_document, "Сохранить как…", "F12") {
        out.push(RibbonCommand::SaveAs);
    }
    ui.separator();
    if item(ui, true, "Выход", "") {
        out.push(RibbonCommand::Exit);
    }
}

/// Группа команд: содержимое и подпись снизу по центру, справа —
/// разделитель. Ширина группы — по содержимому.
fn group(ui: &mut Ui, caption: &str, content: impl FnOnce(&mut Ui)) {
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
                        .color(Color32::from_gray(100)),
                );
            },
        );
    });
    ui.separator();
}

/// Вкладка «Главная».
fn home(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
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
    group(ui, "Шрифт", |ui| {
        ui.horizontal(|ui| {
            let mut toggle = |ui: &mut Ui, on: bool, text: RichText, tip: &str, f: RunFormat| {
                let button = egui::Button::selectable(on, text).min_size(vec2(24.0, 24.0));
                if ui.add_enabled(doc, button).on_hover_text(tip).clicked() {
                    out.push(RibbonCommand::Format(f));
                }
            };
            toggle(
                ui,
                info.bold,
                RichText::new("Ж").strong(),
                "Полужирный (Ctrl+B)",
                RunFormat::Bold,
            );
            toggle(
                ui,
                info.italic,
                RichText::new("К").italics(),
                "Курсив (Ctrl+I)",
                RunFormat::Italic,
            );
            toggle(
                ui,
                info.underline,
                RichText::new("Ч").underline(),
                "Подчёркнутый (Ctrl+U)",
                RunFormat::Underline,
            );
            ui.separator();
            for (icon, name, tip, f) in [
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
            ] {
                if icon_button(ui, icon, name, vec2(26.0, 24.0), doc, false)
                    .on_hover_text(tip)
                    .clicked()
                {
                    out.push(RibbonCommand::Format(f));
                }
            }
        });
    });
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
            ..RibbonInfo::default()
        }
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
    fn file_menu() {
        let mut info = document();
        info.recent = vec![PathBuf::from("/docs/отчёт.docx")];
        let mut h = harness(info);
        h.get_by_label_contains("Файл").click();
        h.run();
        h.get_by_label_contains("Сохранить как").click();
        h.run();
        assert_eq!(h.state().out, vec![RibbonCommand::SaveAs]);
    }
}
