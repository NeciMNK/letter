//! Представление «Файл» (Backstage), как в Word: слева список команд,
//! справа страница выбранной команды. Закрывается стрелкой «назад»
//! или клавишей Esc. Как и лента, само ничего не делает — возвращает
//! команды ([`RibbonCommand`]).

use std::path::PathBuf;

use eframe::egui::{self, Align, Color32, Key, Layout, RichText, Ui, vec2};

use crate::icons::{self, Icon};
use crate::ribbon::{ACCENT, RibbonCommand};
use crate::settings::{AUTOSAVE_RANGE, Settings};

/// Страница представления «Файл».
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    /// «Сведения» о документе.
    #[default]
    Info,
    /// «Создать».
    New,
    /// «Открыть»: последние файлы и «Обзор».
    Open,
    /// «Печать».
    Print,
    /// «Экспорт»: сохранить в другом формате.
    Export,
    /// «Параметры».
    Options,
}

/// Сведения о документе для страницы «Сведения».
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocumentInfo {
    /// Имя файла (или «Документ1» для несохранённого).
    pub name: String,
    /// Полный путь, если документ сохранён.
    pub path: Option<PathBuf>,
    /// Изменён после сохранения.
    pub modified: bool,
    /// Страниц.
    pub pages: usize,
    /// Слов.
    pub words: u32,
    /// Знаков без пробелов.
    pub characters: u32,
    /// Знаков с пробелами.
    pub characters_with_spaces: u32,
    /// Непустых абзацев.
    pub paragraphs: u32,
    /// Название (из свойств документа).
    pub title: Option<String>,
    /// Автор.
    pub author: Option<String>,
    /// Кем изменён в последний раз.
    pub last_modified_by: Option<String>,
    /// Когда создан (как в файле, ISO 8601).
    pub created: Option<String>,
    /// Когда изменён.
    pub changed: Option<String>,
}

/// Что представление показывает.
#[derive(Debug, Clone, Default)]
pub struct BackstageInfo {
    /// Открытый документ (`None` — документа нет).
    pub document: Option<DocumentInfo>,
    /// Недавние файлы.
    pub recent: Vec<PathBuf>,
    /// Параметры программы.
    pub settings: Settings,
}

/// Состояние представления.
#[derive(Debug, Clone, Default)]
pub struct Backstage {
    /// Представление открыто вместо ленты и документа.
    pub open: bool,
    /// Выбранная страница.
    pub page: Page,
}

/// Форматы для «Экспорт → Изменить тип файла»: расширение, название
/// и пояснение.
pub const EXPORT_FORMATS: [(&str, &str, &str); 6] = [
    ("docx", "Документ Word", "Основной формат Word (.docx)"),
    (
        "rtf",
        "Текст в формате RTF",
        "Читается почти любым редактором",
    ),
    (
        "odt",
        "Текст OpenDocument",
        "Формат LibreOffice и OpenOffice",
    ),
    ("txt", "Обычный текст", "Только текст, без оформления"),
    ("html", "Веб-страница", "Открывается в браузере"),
    ("md", "Markdown", "Простая разметка текста"),
];

/// Дата из свойств документа (`2024-05-01T10:30:00Z`) так, как её
/// показывает Word: `01.05.2024 10:30`. Время — как записано (UTC).
pub fn show_date(iso: &str) -> String {
    let (date, time) = iso.split_once('T').unwrap_or((iso, ""));
    let mut parts = date.split('-');
    let (Some(y), Some(m), Some(d)) = (parts.next(), parts.next(), parts.next()) else {
        return iso.to_owned();
    };
    let time: String = time.chars().take(5).collect();
    if time.len() == 5 {
        format!("{d}.{m}.{y} {time}")
    } else {
        format!("{d}.{m}.{y}")
    }
}

impl Backstage {
    /// Открывает представление на странице `page`.
    pub fn show(&mut self, page: Page) {
        self.open = true;
        self.page = page;
    }

    /// Рисует представление на всё окно и возвращает выбранные команды.
    pub fn ui(&mut self, ui: &mut Ui, info: &BackstageInfo) -> Vec<RibbonCommand> {
        let mut out = Vec::new();
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.open = false;
            return out;
        }
        let has_document = info.document.is_some();
        egui::Panel::left("backstage-menu")
            .exact_size(190.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(ACCENT)
                    .inner_margin(egui::Margin::same(8)),
            )
            .show(ui, |ui| {
                ui.visuals_mut().override_text_color = Some(Color32::WHITE);
                ui.spacing_mut().item_spacing.y = 4.0;
                let back = egui::Button::new(icons::text(ui, Icon::Back, 22.0))
                    .frame(false)
                    .min_size(vec2(36.0, 36.0));
                if ui.add(back).on_hover_text("Назад (Esc)").clicked() {
                    self.open = false;
                }
                ui.add_space(8.0);
                let mut item = |ui: &mut Ui, page: Page, text: &str, enabled: bool| {
                    let selected = self.page == page;
                    let button = egui::Button::selectable(selected, RichText::new(text).size(15.0))
                        .min_size(vec2(170.0, 30.0));
                    if ui.add_enabled(enabled, button).clicked() {
                        self.page = page;
                    }
                };
                item(ui, Page::Info, "Сведения", has_document);
                item(ui, Page::New, "Создать", true);
                item(ui, Page::Open, "Открыть", true);
                ui.separator();
                let mut action =
                    |ui: &mut Ui, text: &str, enabled: bool, command: RibbonCommand| {
                        let button = egui::Button::new(RichText::new(text).size(15.0))
                            .frame(false)
                            .min_size(vec2(170.0, 30.0));
                        if ui.add_enabled(enabled, button).clicked() {
                            out.push(command);
                        }
                    };
                action(ui, "Сохранить", has_document, RibbonCommand::Save);
                action(ui, "Сохранить как", has_document, RibbonCommand::SaveAs);
                item(ui, Page::Print, "Печать", has_document);
                item(ui, Page::Export, "Экспорт", has_document);
                action(ui, "Закрыть", has_document, RibbonCommand::CloseDocument);
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    action(ui, "Выход", true, RibbonCommand::Exit);
                    item(ui, Page::Options, "Параметры", true);
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .inner_margin(egui::Margin::same(24)),
            )
            .show(ui, |ui| {
                // Без документа «Сведения», «Печать» и «Экспорт» не нужны.
                let page = match self.page {
                    Page::Info | Page::Print | Page::Export if !has_document => Page::Open,
                    page => page,
                };
                egui::ScrollArea::vertical().show(ui, |ui| match page {
                    Page::Info => info_page(ui, info),
                    Page::New => new_page(ui, &mut out),
                    Page::Open => open_page(ui, info, &mut out),
                    Page::Print => print_page(ui),
                    Page::Export => export_page(ui, &mut out),
                    Page::Options => options_page(ui, info.settings, &mut out),
                });
            });
        // Команды с файлами закрывают представление, как в Word.
        if out
            .iter()
            .any(|c| !matches!(c, RibbonCommand::ChangeSettings(_)))
        {
            self.open = false;
        }
        out
    }
}

fn heading(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(28.0).color(Color32::from_gray(40)));
    ui.add_space(12.0);
}

/// «Сведения»: имя, путь, свойства и статистика.
fn info_page(ui: &mut Ui, info: &BackstageInfo) {
    heading(ui, "Сведения");
    let Some(document) = &info.document else {
        return;
    };
    ui.label(RichText::new(&document.name).size(18.0).strong());
    match &document.path {
        Some(path) => ui.label(path.display().to_string()),
        None => ui.label("Документ ещё не сохранён"),
    };
    if document.modified {
        ui.label(
            RichText::new("Есть несохранённые изменения").color(Color32::from_rgb(160, 60, 0)),
        );
    }
    ui.add_space(16.0);
    ui.label(RichText::new("Свойства").size(16.0));
    let unknown = |v: &Option<String>| v.clone().unwrap_or_else(|| "—".to_owned());
    let date = |v: &Option<String>| v.as_deref().map_or_else(|| "—".to_owned(), show_date);
    let rows = [
        ("Страниц", document.pages.to_string()),
        ("Слов", document.words.to_string()),
        ("Знаков (без пробелов)", document.characters.to_string()),
        (
            "Знаков (с пробелами)",
            document.characters_with_spaces.to_string(),
        ),
        ("Абзацев", document.paragraphs.to_string()),
        ("Название", unknown(&document.title)),
        ("Автор", unknown(&document.author)),
        ("Кем изменён", unknown(&document.last_modified_by)),
        ("Создан", date(&document.created)),
        ("Изменён", date(&document.changed)),
    ];
    egui::Grid::new("document-info")
        .num_columns(2)
        .spacing(vec2(24.0, 6.0))
        .show(ui, |ui| {
            for (name, value) in rows {
                ui.label(RichText::new(name).color(Color32::from_gray(90)));
                ui.label(value);
                ui.end_row();
            }
        });
}

/// «Создать»: новый пустой документ.
fn new_page(ui: &mut Ui, out: &mut Vec<RibbonCommand>) {
    heading(ui, "Создать");
    let tile = egui::Button::new(RichText::new("Новый документ").size(15.0))
        .min_size(vec2(160.0, 200.0))
        .fill(Color32::from_gray(250))
        .stroke(egui::Stroke::new(1.0, Color32::from_gray(190)));
    if ui
        .add(tile)
        .on_hover_text("Пустой документ (Ctrl+N)")
        .clicked()
    {
        out.push(RibbonCommand::New);
    }
}

/// «Открыть»: «Обзор» и последние файлы.
fn open_page(ui: &mut Ui, info: &BackstageInfo, out: &mut Vec<RibbonCommand>) {
    heading(ui, "Открыть");
    if ui
        .button(RichText::new("Обзор…").size(15.0))
        .on_hover_text("Выбрать файл (Ctrl+O)")
        .clicked()
    {
        out.push(RibbonCommand::Open);
    }
    ui.add_space(16.0);
    ui.label(RichText::new("Последние").size(16.0));
    if info.recent.is_empty() {
        ui.label(RichText::new("Здесь появятся открытые недавно файлы.").color(Color32::GRAY));
    }
    for path in &info.recent {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let folder = path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let text = format!("{name}\n{folder}");
        let button = egui::Button::new(text)
            .frame(false)
            .min_size(vec2(400.0, 0.0));
        if ui
            .add(button)
            .on_hover_text(path.display().to_string())
            .clicked()
        {
            out.push(RibbonCommand::OpenRecent(path.clone()));
        }
    }
}

/// «Печать». Сама печать — задача фазы 6.
fn print_page(ui: &mut Ui) {
    heading(ui, "Печать");
    ui.label("Печать и предварительный просмотр появятся в одной из следующих версий.");
    ui.label("Пока можно сохранить документ и напечатать его из другой программы.");
}

/// «Экспорт → Изменить тип файла».
fn export_page(ui: &mut Ui, out: &mut Vec<RibbonCommand>) {
    heading(ui, "Экспорт");
    ui.label(RichText::new("Изменить тип файла").size(16.0));
    ui.add_space(6.0);
    for (extension, name, hint) in EXPORT_FORMATS {
        let text = format!("{name} (*.{extension})\n{hint}");
        let button = egui::Button::new(text).min_size(vec2(360.0, 0.0));
        if ui.add(button).clicked() {
            out.push(RibbonCommand::SaveAsFormat(extension));
        }
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Сохранение в PDF появится вместе с печатью.").color(Color32::GRAY));
}

/// «Параметры».
fn options_page(ui: &mut Ui, settings: Settings, out: &mut Vec<RibbonCommand>) {
    heading(ui, "Параметры");
    let mut changed = settings;
    ui.label(RichText::new("Сохранение").size(16.0));
    ui.horizontal(|ui| {
        ui.label("Автосохранение каждые");
        ui.add(
            egui::DragValue::new(&mut changed.autosave_minutes)
                .range(AUTOSAVE_RANGE)
                .speed(0.2),
        );
        ui.label("мин");
    });
    ui.add_space(12.0);
    ui.label(RichText::new("Вид").size(16.0));
    ui.checkbox(&mut changed.show_ruler, "Показывать линейку при запуске");
    if changed != settings {
        out.push(RibbonCommand::ChangeSettings(changed));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn dates_are_shown_like_in_word() {
        assert_eq!(show_date("2024-05-01T10:30:00Z"), "01.05.2024 10:30");
        assert_eq!(show_date("2024-05-01"), "01.05.2024");
        assert_eq!(show_date("вчера"), "вчера");
    }

    struct State {
        backstage: Backstage,
        info: BackstageInfo,
        out: Vec<RibbonCommand>,
    }

    fn harness(info: BackstageInfo) -> Harness<'static, State> {
        let mut backstage = Backstage::default();
        backstage.show(Page::Info);
        Harness::builder()
            .with_size(vec2(1000.0, 700.0))
            .build_ui_state(
                |ui, state: &mut State| {
                    if state.backstage.open {
                        let out = state.backstage.ui(ui, &state.info);
                        state.out.extend(out);
                    }
                },
                State {
                    backstage,
                    info,
                    out: Vec::new(),
                },
            )
    }

    #[test]
    fn pages_and_commands() {
        let info = BackstageInfo {
            document: Some(DocumentInfo {
                name: "Отчёт.docx".into(),
                pages: 3,
                words: 120,
                ..DocumentInfo::default()
            }),
            recent: vec![PathBuf::from("/tmp/a.docx")],
            settings: Settings::default(),
        };
        let mut h = harness(info);
        h.run();
        assert!(h.query_by_label("Отчёт.docx").is_some());
        assert!(h.query_by_label("120").is_some());
        h.get_by_label("Экспорт").click();
        h.run();
        assert_eq!(h.state().backstage.page, Page::Export);
        h.get_by_label_contains("Текст в формате RTF").click();
        h.run();
        assert_eq!(h.state().out, vec![RibbonCommand::SaveAsFormat("rtf")]);
        // Команда с файлом закрывает представление.
        assert!(!h.state().backstage.open);
    }

    #[test]
    fn without_a_document_and_escape() {
        let mut h = harness(BackstageInfo::default());
        h.run();
        // Без документа открывается «Открыть».
        assert!(h.query_by_label("Обзор…").is_some());
        h.key_press(Key::Escape);
        h.run();
        assert!(!h.state().backstage.open);
    }
}
