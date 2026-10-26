//! Представление «Файл» (Backstage), как в Word: слева список команд,
//! справа страница выбранной команды. Закрывается стрелкой «назад»
//! или клавишей Esc. Как и лента, само ничего не делает — возвращает
//! команды ([`RibbonCommand`]).

use crate::i18n::{Language, tr};
use std::path::PathBuf;

use eframe::egui::{self, Align, Color32, Key, Layout, Painter, Rect, RichText, Ui, vec2};

use crate::icons::{self, Icon};
use crate::print::{PageChoice, PrintJob, Printers, parse_pages, with_copies};
use crate::ribbon::{ACCENT, RibbonCommand};
use crate::settings::{AUTOSAVE_RANGE, Settings};
use crate::templates::Template;
use crate::theme::{Palette, ThemeChoice};

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
    /// Сохраняется с паролем на открытие.
    pub encrypted: bool,
    /// Страниц.
    pub pages: usize,
    /// Страница с курсором, с нуля.
    pub current_page: usize,
    /// Размеры страниц, пт: ширина и высота (для предварительного
    /// просмотра).
    pub page_sizes: Vec<(f32, f32)>,
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
    /// Принтеры; `None` — ещё ищутся.
    pub printers: Option<Printers>,
    /// Шаблоны для «Создать»: встроенные, потом личные.
    pub templates: Vec<Template>,
}

/// Что нарисовать в миниатюре.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Thumb {
    /// Страница открытого документа (номер с нуля) — «Печать».
    Page(usize),
    /// Первая страница шаблона (номер в [`BackstageInfo::templates`]).
    Template(usize),
}

/// Настройки на странице «Печать».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintSettings {
    /// Копий.
    pub copies: u32,
    /// Разобрать по копиям (1 2 3, 1 2 3).
    pub collate: bool,
    /// Какие страницы.
    pub pages: PageChoice,
    /// Страницы для [`PageChoice::Custom`]: «1-3, 5».
    pub custom: String,
    /// Выбранный принтер; `None` — по умолчанию.
    pub printer: Option<String>,
    /// Страница в предварительном просмотре, с нуля.
    pub preview: usize,
}

impl Default for PrintSettings {
    fn default() -> PrintSettings {
        PrintSettings {
            copies: 1,
            collate: true,
            pages: PageChoice::All,
            custom: String::new(),
            printer: None,
            preview: 0,
        }
    }
}

impl PrintSettings {
    /// Страницы для печати (с нуля, без копий); `None` — номера страниц
    /// введены с ошибкой.
    pub fn chosen_pages(&self, total: usize, current: usize) -> Option<Vec<usize>> {
        match self.pages {
            PageChoice::All => Some((0..total).collect()),
            PageChoice::Current => Some(vec![current.min(total.saturating_sub(1))]),
            PageChoice::Custom => parse_pages(&self.custom, total),
        }
    }
}

/// Состояние представления.
#[derive(Debug, Clone, Default)]
pub struct Backstage {
    /// Представление открыто вместо ленты и документа.
    pub open: bool,
    /// Выбранная страница.
    pub page: Page,
    /// Настройки печати (живут, пока открыта программа).
    pub print: PrintSettings,
}

/// Форматы для «Экспорт → Изменить тип файла»: расширение, название
/// и пояснение.
pub const EXPORT_FORMATS: [(&str, &str, &str); 7] = [
    ("pdf", "format-pdf", "format-pdf-about"),
    ("docx", "format-docx", "format-docx-about"),
    ("rtf", "format-rtf", "format-rtf-about"),
    ("odt", "format-odt", "format-odt-about"),
    ("txt", "format-txt", "format-txt-about"),
    ("html", "format-html", "format-html-about"),
    ("md", "format-md", "format-md-about"),
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
    /// `preview` рисует страницу документа или шаблона в прямоугольник —
    /// для предварительного просмотра на странице «Печать» и миниатюр
    /// шаблонов на странице «Создать».
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        info: &BackstageInfo,
        preview: &mut dyn FnMut(&Painter, Thumb, Rect),
    ) -> Vec<RibbonCommand> {
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
                if ui.add(back).on_hover_text(tr("backstage-back")).clicked() {
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
                item(ui, Page::Info, tr("backstage-info"), has_document);
                item(ui, Page::New, tr("backstage-new"), true);
                item(ui, Page::Open, tr("backstage-open"), true);
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
                action(ui, tr("save"), has_document, RibbonCommand::Save);
                action(ui, tr("save-as"), has_document, RibbonCommand::SaveAs);
                item(ui, Page::Print, tr("backstage-print"), has_document);
                item(ui, Page::Export, tr("backstage-export"), has_document);
                action(ui, tr("close"), has_document, RibbonCommand::CloseDocument);
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    action(ui, tr("backstage-exit"), true, RibbonCommand::Exit);
                    item(ui, Page::Options, tr("backstage-options"), true);
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Palette::of(ui.ctx()).page)
                    .inner_margin(egui::Margin::same(24)),
            )
            .show(ui, |ui| {
                // Без документа «Сведения», «Печать» и «Экспорт» не нужны.
                let page = match self.page {
                    Page::Info | Page::Print | Page::Export if !has_document => Page::Open,
                    page => page,
                };
                if page == Page::Print
                    && let Some(document) = &info.document
                {
                    print_page(ui, &mut self.print, document, info, preview, &mut out);
                    return;
                }
                egui::ScrollArea::vertical().show(ui, |ui| match page {
                    Page::Info => info_page(ui, info, &mut out),
                    Page::New => new_page(ui, info, preview, &mut out),
                    Page::Open | Page::Print => open_page(ui, info, &mut out),
                    Page::Export => export_page(ui, &mut out),
                    Page::Options => options_page(ui, info.settings, &mut out),
                });
            });
        // Команды с файлами закрывают представление, как в Word.
        if out.iter().any(|c| {
            !matches!(
                c,
                RibbonCommand::ChangeSettings(_) | RibbonCommand::AddTemplate
            )
        }) {
            self.open = false;
        }
        out
    }
}

fn heading(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(28.0)
            .color(Palette::of(ui.ctx()).heading),
    );
    ui.add_space(12.0);
}

/// «Сведения»: имя, путь, свойства и статистика.
fn info_page(ui: &mut Ui, info: &BackstageInfo, out: &mut Vec<RibbonCommand>) {
    heading(ui, tr("backstage-info"));
    let Some(document) = &info.document else {
        return;
    };
    ui.label(RichText::new(&document.name).size(18.0).strong());
    match &document.path {
        Some(path) => ui.label(path.display().to_string()),
        None => ui.label(tr("backstage-not-saved")),
    };
    if document.modified {
        ui.label(
            RichText::new(tr("backstage-unsaved-changes")).color(Color32::from_rgb(160, 60, 0)),
        );
    }
    ui.add_space(16.0);
    ui.label(RichText::new(tr("backstage-protect")).size(16.0));
    ui.label(if document.encrypted {
        tr("backstage-encrypted")
    } else {
        tr("backstage-not-encrypted")
    });
    if ui.button(tr("backstage-encrypt")).clicked() {
        out.push(RibbonCommand::EncryptDialog);
    }
    ui.add_space(16.0);
    ui.label(RichText::new(tr("backstage-properties")).size(16.0));
    let unknown = |v: &Option<String>| v.clone().unwrap_or_else(|| "—".to_owned());
    let date = |v: &Option<String>| v.as_deref().map_or_else(|| "—".to_owned(), show_date);
    let rows = [
        (tr("stats-pages"), document.pages.to_string()),
        (tr("stats-words"), document.words.to_string()),
        (tr("stats-characters"), document.characters.to_string()),
        (
            tr("stats-characters-spaces"),
            document.characters_with_spaces.to_string(),
        ),
        (tr("stats-paragraphs"), document.paragraphs.to_string()),
        (tr("backstage-title"), unknown(&document.title)),
        (tr("backstage-author"), unknown(&document.author)),
        (
            tr("backstage-last-modified-by"),
            unknown(&document.last_modified_by),
        ),
        (tr("backstage-created"), date(&document.created)),
        (tr("backstage-modified"), date(&document.changed)),
    ];
    egui::Grid::new("document-info")
        .num_columns(2)
        .spacing(vec2(24.0, 6.0))
        .show(ui, |ui| {
            for (name, value) in rows {
                ui.label(RichText::new(name).color(Palette::of(ui.ctx()).caption));
                ui.label(value);
                ui.end_row();
            }
        });
}

/// Размер миниатюры шаблона (лист A4 в уменьшении).
const THUMB: egui::Vec2 = egui::Vec2::new(150.0, 212.0);

/// Плитка шаблона: миниатюра и имя под ней. Возвращает, нажата ли.
fn template_tile(
    ui: &mut Ui,
    name: &str,
    index: Option<usize>,
    preview: &mut dyn FnMut(&Painter, Thumb, Rect),
) -> bool {
    let palette = Palette::of(ui.ctx());
    // Размер плитки известен заранее: так ряд плиток переносится.
    let size = vec2(THUMB.x, THUMB.y + 24.0);
    ui.allocate_ui_with_layout(size, Layout::top_down(Align::Min), |ui| {
        ui.set_min_size(size);
        let (rect, response) = ui.allocate_exact_size(THUMB, egui::Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
        let painter = ui.painter();
        painter.rect_filled(rect, 2.0, Color32::WHITE);
        if let Some(index) = index {
            preview(painter, Thumb::Template(index), rect);
        }
        let stroke = if response.hovered() {
            egui::Stroke::new(2.0, ACCENT)
        } else {
            egui::Stroke::new(1.0, palette.border)
        };
        painter.rect_stroke(rect, 2.0, stroke, egui::StrokeKind::Inside);
        ui.label(RichText::new(name).size(13.0));
        response.clicked()
    })
    .inner
}

/// «Создать»: пустой документ, встроенные шаблоны и личные шаблоны.
fn new_page(
    ui: &mut Ui,
    info: &BackstageInfo,
    preview: &mut dyn FnMut(&Painter, Thumb, Rect),
    out: &mut Vec<RibbonCommand>,
) {
    heading(ui, tr("backstage-new"));
    egui::ScrollArea::vertical().show(ui, |ui| {
        let tiles = |ui: &mut Ui,
                     items: &[(usize, &Template)],
                     preview: &mut dyn FnMut(&Painter, Thumb, Rect),
                     out: &mut Vec<RibbonCommand>,
                     blank: bool| {
            let wrapped = Layout::left_to_right(Align::TOP).with_main_wrap(true);
            ui.with_layout(wrapped, |ui| {
                ui.spacing_mut().item_spacing = vec2(20.0, 16.0);
                if blank && template_tile(ui, tr("new-document"), None, preview) {
                    out.push(RibbonCommand::New);
                }
                for (index, template) in items {
                    if template_tile(ui, &template.name(), Some(*index), preview) {
                        out.push(RibbonCommand::NewFromTemplate((*template).clone()));
                    }
                }
            });
        };
        let (builtin, personal): (Vec<_>, Vec<_>) = info
            .templates
            .iter()
            .enumerate()
            .partition(|(_, t)| matches!(t, Template::Builtin(_)));
        ui.label(RichText::new(tr("backstage-builtin-templates")).size(16.0));
        ui.add_space(6.0);
        tiles(ui, &builtin, preview, out, true);
        ui.add_space(20.0);
        ui.label(RichText::new(tr("backstage-personal-templates")).size(16.0));
        ui.add_space(6.0);
        if personal.is_empty() {
            ui.label(RichText::new(tr("backstage-personal-empty")).color(Color32::GRAY));
        } else {
            tiles(ui, &personal, preview, out, false);
        }
        ui.add_space(10.0);
        if ui
            .button(tr("backstage-add-template"))
            .on_hover_text(tr("backstage-add-template-hint"))
            .clicked()
        {
            out.push(RibbonCommand::AddTemplate);
        }
    });
}

/// «Открыть»: «Обзор» и последние файлы.
fn open_page(ui: &mut Ui, info: &BackstageInfo, out: &mut Vec<RibbonCommand>) {
    heading(ui, tr("backstage-open"));
    if ui
        .button(RichText::new(tr("backstage-browse")).size(15.0))
        .on_hover_text(tr("backstage-browse-hint"))
        .clicked()
    {
        out.push(RibbonCommand::Open);
    }
    ui.add_space(16.0);
    ui.label(RichText::new(tr("backstage-recent")).size(16.0));
    if info.recent.is_empty() {
        ui.label(RichText::new(tr("backstage-recent-empty")).color(Color32::GRAY));
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

/// «Печать»: слева настройки и кнопка, справа предварительный просмотр
/// страницы с листанием.
fn print_page(
    ui: &mut Ui,
    settings: &mut PrintSettings,
    document: &DocumentInfo,
    info: &BackstageInfo,
    preview: &mut dyn FnMut(&Painter, Thumb, Rect),
    out: &mut Vec<RibbonCommand>,
) {
    let total = document.pages.max(1);
    let chosen = settings.chosen_pages(document.pages, document.current_page);
    egui::Panel::left("print-settings")
        .exact_size(320.0)
        .resizable(false)
        .frame(egui::Frame::new().inner_margin(egui::Margin {
            right: 16,
            ..egui::Margin::ZERO
        }))
        .show(ui, |ui| {
            heading(ui, tr("backstage-print"));
            ui.horizontal(|ui| {
                let button = egui::Button::new((
                    icons::text(ui, Icon::Printer, 24.0),
                    RichText::new(tr("print-button")).size(16.0),
                ))
                .min_size(vec2(110.0, 56.0));
                let button = ui.add_enabled(chosen.is_some() && document.pages > 0, button);
                if button.clicked()
                    && let Some(pages) = &chosen
                {
                    out.push(RibbonCommand::Print(PrintJob {
                        pages: with_copies(pages, settings.copies, settings.collate),
                        printer: settings.printer.clone(),
                    }));
                }
                let label = ui.label(tr("print-copies"));
                ui.add(egui::DragValue::new(&mut settings.copies).range(1..=999))
                    .labelled_by(label.id);
            });
            ui.add_space(12.0);
            printer_choice(ui, settings, info.printers.as_ref());
            ui.add_space(12.0);
            let settings_label = ui.label(RichText::new(tr("print-settings")).size(16.0));
            let label = ui.label(tr("print-pages"));
            let name = |choice: PageChoice| match choice {
                PageChoice::All => tr("print-pages-all"),
                PageChoice::Current => tr("print-pages-current"),
                PageChoice::Custom => tr("print-pages-custom"),
            };
            egui::ComboBox::from_id_salt("print-pages")
                .width(290.0)
                .selected_text(name(settings.pages))
                .show_ui(ui, |ui| {
                    for choice in [PageChoice::All, PageChoice::Current, PageChoice::Custom] {
                        ui.selectable_value(&mut settings.pages, choice, name(choice));
                    }
                })
                .response
                .labelled_by(label.id);
            let field = egui::TextEdit::singleline(&mut settings.custom)
                .hint_text(tr("print-pages-hint"))
                .desired_width(290.0);
            let response = ui.add(field).labelled_by(label.id);
            if response.changed() && !settings.custom.trim().is_empty() {
                settings.pages = PageChoice::Custom;
            }
            if chosen.is_none() {
                ui.label(
                    RichText::new(tr("print-pages-invalid")).color(Color32::from_rgb(190, 40, 30)),
                );
            }
            ui.add_space(6.0);
            let collate = if settings.collate {
                tr("print-collated")
            } else {
                tr("print-uncollated")
            };
            egui::ComboBox::from_id_salt("print-collate")
                .width(290.0)
                .selected_text(collate)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut settings.collate, true, tr("print-collated"));
                    ui.selectable_value(&mut settings.collate, false, tr("print-uncollated"));
                })
                .response
                .labelled_by(settings_label.id);
        });

    // Просмотр: страница целиком по центру, внизу — листание.
    settings.preview = settings.preview.min(total - 1);
    let bar = 32.0;
    let area = ui.available_rect_before_wrap();
    let sheet_area =
        Rect::from_min_max(area.min, egui::pos2(area.max.x, area.max.y - bar)).shrink(12.0);
    let (width, height) = document
        .page_sizes
        .get(settings.preview)
        .copied()
        .unwrap_or((612.0, 792.0));
    if width > 0.0 && height > 0.0 && sheet_area.width() > 0.0 && sheet_area.height() > 0.0 {
        let scale = (sheet_area.width() / width).min(sheet_area.height() / height);
        let sheet = Rect::from_center_size(sheet_area.center(), vec2(width, height) * scale);
        preview(ui.painter(), Thumb::Page(settings.preview), sheet);
    }
    let bar_rect = Rect::from_min_max(egui::pos2(area.min.x, area.max.y - bar), area.max);
    ui.scope_builder(egui::UiBuilder::new().max_rect(bar_rect), |ui| {
        ui.horizontal_centered(|ui| {
            ui.add_space((bar_rect.width() / 2.0 - 90.0).max(0.0));
            let name = tr("print-previous-page");
            let size = vec2(26.0, 26.0);
            if icons::icon_button(
                ui,
                Icon::ChevronLeft,
                name,
                size,
                settings.preview > 0,
                false,
            )
            .on_hover_text(name)
            .clicked()
            {
                settings.preview -= 1;
            }
            ui.label(crate::i18n::tr_args(
                "print-page-of",
                &[
                    ("page", (settings.preview + 1).into()),
                    ("total", total.into()),
                ],
            ));
            let name = tr("print-next-page");
            let enabled = settings.preview + 1 < total;
            if icons::icon_button(ui, Icon::ChevronRight, name, size, enabled, false)
                .on_hover_text(name)
                .clicked()
            {
                settings.preview += 1;
            }
        });
    });
    // Листание колесом и клавишами, как в Word.
    let (scroll, page_up, page_down) = ui.input(|i| {
        (
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::MouseWheel { delta, .. } => Some(delta.y),
                    _ => None,
                })
                .sum::<f32>(),
            i.key_pressed(Key::PageUp),
            i.key_pressed(Key::PageDown),
        )
    });
    if (ui.rect_contains_pointer(area) && scroll != 0.0) || page_up || page_down {
        if (scroll > 0.0 || page_up) && settings.preview > 0 {
            settings.preview -= 1;
        } else if (scroll < 0.0 || page_down) && settings.preview + 1 < total {
            settings.preview += 1;
        }
    }
}

/// Выбор принтера.
fn printer_choice(ui: &mut Ui, settings: &mut PrintSettings, printers: Option<&Printers>) {
    let label = ui.label(RichText::new(tr("print-printer")).size(16.0));
    let Some(printers) = printers else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(tr("print-searching"));
        });
        return;
    };
    if printers.names.is_empty() {
        ui.label(tr("print-no-printers"));
        settings.printer = None;
        return;
    }
    if settings
        .printer
        .as_ref()
        .is_none_or(|p| !printers.names.contains(p))
    {
        settings.printer = printers
            .default
            .clone()
            .filter(|d| printers.names.contains(d))
            .or_else(|| printers.names.first().cloned());
    }
    let selected = settings.printer.clone().unwrap_or_default();
    egui::ComboBox::from_id_salt("print-printer")
        .width(290.0)
        .selected_text(&selected)
        .show_ui(ui, |ui| {
            for name in &printers.names {
                let text = if printers.default.as_ref() == Some(name) {
                    format!("{name} ({})", tr("print-default"))
                } else {
                    name.clone()
                };
                ui.selectable_value(&mut settings.printer, Some(name.clone()), text);
            }
        })
        .response
        .labelled_by(label.id);
}

/// «Экспорт → Изменить тип файла».
fn export_page(ui: &mut Ui, out: &mut Vec<RibbonCommand>) {
    heading(ui, tr("backstage-export"));
    ui.label(RichText::new(tr("backstage-change-type")).size(16.0));
    ui.add_space(6.0);
    for (extension, name, hint) in EXPORT_FORMATS {
        let text = format!("{} (*.{extension})\n{}", tr(name), tr(hint));
        let button = egui::Button::new(text).min_size(vec2(360.0, 0.0));
        if ui.add(button).clicked() {
            out.push(RibbonCommand::SaveAsFormat(extension));
        }
    }
}

/// «Параметры».
fn options_page(ui: &mut Ui, settings: Settings, out: &mut Vec<RibbonCommand>) {
    heading(ui, tr("backstage-options"));
    let mut changed = settings;
    ui.label(RichText::new(tr("options-save")).size(16.0));
    ui.horizontal(|ui| {
        ui.label(tr("options-autosave"));
        ui.add(
            egui::DragValue::new(&mut changed.autosave_minutes)
                .range(AUTOSAVE_RANGE)
                .speed(0.2),
        );
        ui.label(tr("options-minutes"));
    });
    ui.add_space(12.0);
    ui.label(RichText::new(tr("options-view")).size(16.0));
    ui.checkbox(&mut changed.show_ruler, tr("options-show-ruler"));
    ui.horizontal(|ui| {
        let label = ui.label(tr("options-theme"));
        egui::ComboBox::from_id_salt("theme")
            .selected_text(changed.theme.name())
            .show_ui(ui, |ui| {
                for theme in ThemeChoice::ALL {
                    ui.selectable_value(&mut changed.theme, theme, theme.name());
                }
            })
            .response
            .labelled_by(label.id);
    });
    ui.horizontal(|ui| {
        let label = ui.label(tr("options-language"));
        egui::ComboBox::from_id_salt("language")
            .selected_text(changed.language.name())
            .show_ui(ui, |ui| {
                for language in Language::ALL {
                    ui.selectable_value(&mut changed.language, language, language.name());
                }
            })
            .response
            .labelled_by(label.id);
    });
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
        sized_harness(info, vec2(1000.0, 700.0))
    }

    fn sized_harness(info: BackstageInfo, size: egui::Vec2) -> Harness<'static, State> {
        let mut backstage = Backstage::default();
        backstage.show(Page::Info);
        Harness::builder().with_size(size).build_ui_state(
            |ui, state: &mut State| {
                if state.backstage.open {
                    let out = state.backstage.ui(ui, &state.info, &mut |_, _, _| {});
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
            printers: None,
            templates: crate::templates::Builtin::ALL
                .map(Template::Builtin)
                .to_vec(),
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
    fn new_page_offers_templates() {
        let info = BackstageInfo {
            templates: crate::templates::Builtin::ALL
                .map(Template::Builtin)
                .to_vec(),
            ..BackstageInfo::default()
        };
        // Все плитки и кнопка видны без прокрутки.
        let mut h = sized_harness(info, vec2(1500.0, 1100.0));
        h.state_mut().backstage.show(Page::New);
        h.run();
        assert!(
            h.query_by_label_contains("Здесь появятся ваши шаблоны")
                .is_some()
        );
        h.get_all_by_label("Резюме").next().unwrap().click();
        h.run();
        assert_eq!(
            h.state().out,
            [RibbonCommand::NewFromTemplate(Template::Builtin(
                crate::templates::Builtin::Resume
            ))]
        );
        h.state_mut().out.clear();
        h.state_mut().backstage.show(Page::New);
        h.state_mut()
            .info
            .templates
            .push(Template::Personal("/tmp/Бланк.dotx".into()));
        h.run();
        h.get_all_by_label("Бланк").next().unwrap().click();
        h.run();
        h.state_mut().backstage.show(Page::New);
        h.run();
        h.get_all_by_label("Новый документ").next().unwrap().click();
        h.run();
        h.state_mut().backstage.show(Page::New);
        h.run();
        h.get_by_label("Добавить шаблон…").click();
        h.run();
        assert_eq!(
            h.state().out,
            [
                RibbonCommand::NewFromTemplate(Template::Personal("/tmp/Бланк.dotx".into())),
                RibbonCommand::New,
                RibbonCommand::AddTemplate,
            ]
        );
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

    #[test]
    fn print_page_previews_and_prints() {
        struct PrintState {
            backstage: Backstage,
            info: BackstageInfo,
            out: Vec<RibbonCommand>,
            previewed: Vec<usize>,
        }
        let info = BackstageInfo {
            document: Some(DocumentInfo {
                name: "Отчёт.docx".into(),
                pages: 3,
                current_page: 1,
                page_sizes: vec![(612.0, 792.0); 3],
                ..DocumentInfo::default()
            }),
            recent: Vec::new(),
            settings: Settings::default(),
            printers: Some(Printers {
                names: vec!["Office".into(), "Home".into()],
                default: Some("Home".into()),
            }),
            templates: Vec::new(),
        };
        let mut backstage = Backstage::default();
        backstage.show(Page::Print);
        let mut h = Harness::builder()
            .with_size(vec2(1100.0, 750.0))
            .build_ui_state(
                |ui, state: &mut PrintState| {
                    if state.backstage.open {
                        let previewed = &mut state.previewed;
                        let out = state.backstage.ui(ui, &state.info, &mut |_, thumb, _| {
                            if let Thumb::Page(page) = thumb {
                                previewed.push(page);
                            }
                        });
                        state.out.extend(out);
                    }
                },
                PrintState {
                    backstage,
                    info,
                    out: Vec::new(),
                    previewed: Vec::new(),
                },
            );
        h.run();
        assert_eq!(h.state().previewed.last(), Some(&0));
        // Принтер по умолчанию выбран сам.
        assert_eq!(h.state().backstage.print.printer.as_deref(), Some("Home"));
        h.get_by_label("Следующая страница").click();
        h.run();
        assert_eq!(h.state().previewed.last(), Some(&1));
        assert!(h.query_by_label("2 из 3").is_some());

        // Ошибка в номерах — печатать нельзя.
        h.state_mut().backstage.print.pages = PageChoice::Custom;
        h.state_mut().backstage.print.custom = "5".into();
        h.run();
        assert!(h.query_by_label("Неверные номера страниц").is_some());

        h.state_mut().backstage.print.custom = "2-3".into();
        h.state_mut().backstage.print.copies = 2;
        h.state_mut().backstage.print.collate = false;
        h.run();
        // Надпись кнопки — значок принтера и «Печать».
        h.get_by_label("\u{e141} Печать").click();
        h.run();
        assert_eq!(
            h.state().out,
            vec![RibbonCommand::Print(PrintJob {
                pages: vec![1, 1, 2, 2],
                printer: Some("Home".into()),
            })]
        );
        assert!(!h.state().backstage.open);
    }

    #[test]
    fn current_page_choice() {
        let mut settings = PrintSettings::default();
        assert_eq!(settings.chosen_pages(3, 1), Some(vec![0, 1, 2]));
        settings.pages = PageChoice::Current;
        assert_eq!(settings.chosen_pages(3, 1), Some(vec![1]));
        assert_eq!(settings.chosen_pages(3, 7), Some(vec![2]));
    }
}
