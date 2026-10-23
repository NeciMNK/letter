//! Лента в стиле Word: панель быстрого доступа (сохранить, отменить,
//! повторить), меню «Файл», вкладки и их группы команд. Лента сама
//! ничего не делает с документом — она возвращает выбранные команды
//! ([`RibbonCommand`]), а выполняет их окно.
//!
//! Вкладка «Главная» — в [`crate::home`].

use crate::i18n::tr;
use crate::keytips::{self, KeyTips, Level, Outcome, TipAction};
use crate::theme::Palette;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{
    self, Align, Color32, CornerRadius, Layout, Rect, RichText, Stroke, Ui, pos2, vec2,
};
use letter_editor::{ListKind, ParagraphFormat, RunFormat};

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
    /// «Конструктор»: фон страницы.
    Design,
    /// «Макет».
    Layout,
    /// «Ссылки».
    References,
    /// «Рецензирование».
    Review,
    /// «Вид».
    View,
    /// «Конструктор таблиц»: видна, только когда курсор в таблице.
    TableDesign,
    /// «Макет таблицы»: видна, только когда курсор в таблице.
    TableLayout,
    /// «Формат рисунка»: видна, только когда выделен рисунок.
    PictureFormat,
    /// «Колонтитулы»: видна, пока колонтитул открыт для правки.
    HeaderFooter,
    /// «Структура»: видна в режиме «Структура».
    Outline,
}

impl Tab {
    /// Все вкладки по порядку.
    pub const ALL: [Tab; 7] = [
        Tab::Home,
        Tab::Insert,
        Tab::Design,
        Tab::Layout,
        Tab::References,
        Tab::Review,
        Tab::View,
    ];

    /// Подсказка клавиш вкладки (имя в файлах перевода).
    pub fn keytip(self) -> &'static str {
        match self {
            Tab::Home => "keytip-tab-home",
            Tab::Insert => "keytip-tab-insert",
            Tab::Design => "keytip-tab-design",
            Tab::Layout => "keytip-tab-layout",
            Tab::References => "keytip-tab-references",
            Tab::Review => "keytip-tab-review",
            Tab::View => "keytip-tab-view",
            Tab::TableDesign => "keytip-tab-table-design",
            Tab::TableLayout => "keytip-tab-table-layout",
            Tab::PictureFormat => "keytip-tab-picture-format",
            Tab::HeaderFooter => "keytip-tab-header-footer",
            Tab::Outline => "keytip-tab-outline",
        }
    }

    /// Название на вкладке.
    pub fn title(self) -> &'static str {
        match self {
            Tab::Home => tr("tab-home"),
            Tab::Insert => tr("tab-insert"),
            Tab::Design => tr("tab-design"),
            Tab::Layout => tr("tab-layout"),
            Tab::References => tr("tab-references"),
            Tab::Review => tr("tab-review"),
            Tab::View => tr("tab-view"),
            Tab::TableDesign => tr("tab-table-design"),
            Tab::TableLayout => tr("tab-table-layout"),
            Tab::PictureFormat => tr("tab-picture-format"),
            Tab::HeaderFooter => tr("tab-header-footer"),
            Tab::Outline => tr("tab-outline"),
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
    /// «Файл → Сведения → Зашифровать паролем».
    EncryptDialog,
    /// «Выход».
    Exit,
    /// Открыть представление «Файл».
    Backstage,
    /// «Файл → Печать» (Ctrl+P): открыть страницу печати.
    PrintPreview,
    /// Напечатать.
    Print(crate::print::PrintJob),
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
    /// «Маркеры» или «Нумерация».
    List(ListKind),
    /// «Многоуровневый список».
    MultilevelList,
    /// «Начать заново с 1».
    RestartNumbering,
    /// «Продолжить нумерацию».
    ContinueNumbering,
    /// Показать или скрыть знаки форматирования.
    ToggleMarks,
    /// Применить стиль с этим идентификатором.
    Style(String),
    /// Окно «Найти».
    Find,
    /// Режим просмотра («Вид → Режимы», кнопки в строке состояния).
    ViewMode(crate::view::ViewMode),
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
    /// «Вид → Область навигации»: показать или скрыть.
    ToggleNavigation,
    /// «Создать примечание» (Ctrl+Alt+M).
    NewComment,
    /// Удалить примечание у курсора.
    DeleteComment,
    /// «Удалить все примечания в документе».
    DeleteAllComments,
    /// Предыдущее (`false`) или следующее примечание.
    NextComment(bool),
    /// Показать или скрыть область примечаний.
    ToggleComments,
    /// «Следующая сноска» и соседние пункты: к следующей (`forward`) или
    /// предыдущей ссылке на сноску вида `kind`.
    NextNote {
        /// Вид сноски.
        kind: letter_model::NoteKind,
        /// Вперёд.
        forward: bool,
    },
    /// Развернуть (`true`) или свернуть заголовок у курсора (режим
    /// «Структура»).
    OutlineExpand(bool),
    /// «Показать уровень»; `None` — все уровни.
    ShowLevel(Option<u8>),
    /// Ctrl+Backspace и Ctrl+Delete: удалить слово до или после курсора.
    DeleteWord {
        /// Слово после курсора.
        forward: bool,
    },
    /// Вставить знак (неразрывный пробел).
    Insert(&'static str),
    /// Встроенный стиль Word по имени (`heading 1`).
    BuiltInStyle(&'static str),
    /// «Вставка → Рисунки»: выбрать файл картинки и вставить рисунок.
    InsertPicture,
    /// «Обтекание текстом» выделенного рисунка.
    PictureWrap(letter_editor::PictureWrap),
    /// Открыть для правки верхний (`header`) или нижний колонтитул.
    EditHeaderFooter {
        /// Верхний.
        header: bool,
    },
    /// «Номер страницы».
    PageNumber(crate::headers::PageNumberPlace),
    /// «Особый колонтитул для первой страницы».
    TitlePage(bool),
    /// «Разные колонтитулы для чётных и нечётных страниц».
    EvenOddHeaders(bool),
    /// Окно «Формат номеров страниц».
    PageNumberFormat,
    /// «Закрыть окно колонтитулов».
    CloseHeaderFooter,
    /// Обновить поля в выделенном (F9).
    UpdateFields,
    /// Вставить оглавление у курсора.
    InsertToc,
    /// Перестроить оглавления документа.
    UpdateToc,
    /// Вставить поле с этим кодом у курсора (с готовым результатом).
    InsertField(String),
    /// Окно «Ссылка» (Ctrl+K).
    LinkDialog,
    /// Окно «Закладка».
    BookmarkDialog,
    /// Вставить знак из меню или окна «Символ».
    InsertSymbol(char),
    /// Окно «Символ».
    SymbolDialog,
    /// «Титульная страница» этого вида (тексты подставляет просмотрщик).
    CoverPage(letter_editor::CoverLayout),
    /// Перейти по ссылке у курсора.
    OpenLink,
    /// Удалить ссылку у курсора (или в выделенном).
    RemoveLink,
    /// Правка документа командой редактора (вставка таблицы и т. п.).
    Edit(letter_editor::Command),
    /// Открыть список или меню на ленте (подсказки клавиш).
    OpenPopup(egui::Id),
    /// Поставить фокус в поле ввода на ленте (подсказки клавиш).
    Focus(egui::Id),
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
    /// Список, в котором абзац с курсором.
    pub list: Option<ListKind>,
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
    /// Область навигации показана.
    pub navigation: bool,
    /// В документе есть примечания.
    pub has_comments: bool,
    /// Курсор в тексте примечания.
    pub in_comment: bool,
    /// Область примечаний показана.
    pub comments: bool,
    /// «Показать уровень» режима «Структура»; `None` — все уровни.
    pub show_level: Option<u8>,
    /// Уровень абзаца у курсора в структуре (1–9); `None` — обычный текст.
    pub outline_level: Option<u8>,
    /// Режим просмотра.
    pub view_mode: crate::view::ViewMode,
    /// Курсор в таблице.
    pub in_table: bool,
    /// Выделено несколько ячеек таблицы: их можно объединить.
    pub can_merge: bool,
    /// Обтекание выделенного рисунка (`None` — рисунок не выделен).
    pub picture_wrap: Option<letter_editor::PictureWrap>,
    /// Параметры страницы раздела с курсором.
    pub section: Option<letter_model::SectionProps>,
    /// Открыт колонтитул: `Some(true)` — верхний, `Some(false)` — нижний.
    pub story: Option<bool>,
    /// У раздела открытого колонтитула особый колонтитул первой страницы.
    pub title_page: bool,
    /// «Разные колонтитулы для чётных и нечётных страниц».
    pub even_odd: bool,
    /// В документе включена автоматическая расстановка переносов.
    pub auto_hyphenation: bool,
    /// Отступ слева абзаца с курсором, твипы.
    pub indent_left: i32,
    /// Отступ справа абзаца с курсором, твипы.
    pub indent_right: i32,
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
    /// Подсказки клавиш по Alt.
    pub keytips: KeyTips,
    /// В прошлом кадре был открыт колонтитул.
    story_open: bool,
    /// В прошлом кадре был режим «Структура».
    outline_open: bool,
}

impl Ribbon {
    /// Клавиши для подсказок по Alt (до остальной клавиатуры).
    pub fn handle_keys(&mut self, ctx: &egui::Context) -> Vec<RibbonCommand> {
        let tips = keytips::collected(ctx);
        let mut commands = Vec::new();
        for outcome in self.keytips.handle(ctx, &tips) {
            match outcome {
                Outcome::Tab(tab) => {
                    self.tab = tab;
                    self.collapsed = false;
                }
                Outcome::Command(command) => commands.push(command),
            }
        }
        commands
    }

    /// Рисует ленту целиком и возвращает выбранные команды.
    pub fn ui(&mut self, ui: &mut Ui, info: &RibbonInfo) -> Vec<RibbonCommand> {
        let mut out = Vec::new();
        keytips::begin(ui.ctx(), self.keytips.level);
        ui.spacing_mut().item_spacing.x = 2.0;
        quick_access(ui, info, &mut out);
        // Вкладка таблицы пропадает, когда курсор уходит из таблицы.
        if matches!(self.tab, Tab::TableDesign | Tab::TableLayout) && !info.in_table {
            self.tab = Tab::Home;
        }
        // И вкладка рисунка — когда рисунок перестаёт быть выделенным.
        if self.tab == Tab::PictureFormat && info.picture_wrap.is_none() {
            self.tab = Tab::Home;
        }
        // Вкладка колонтитулов открывается вместе с колонтитулом.
        if info.story.is_some() && !self.story_open {
            self.tab = Tab::HeaderFooter;
        }
        if info.story.is_none() && self.tab == Tab::HeaderFooter {
            self.tab = Tab::Home;
        }
        self.story_open = info.story.is_some();
        // Вкладка «Структура» открывается вместе с режимом.
        let outline = info.view_mode == crate::view::ViewMode::Outline;
        if outline && !self.outline_open {
            self.tab = Tab::Outline;
        }
        if !outline && self.tab == Tab::Outline {
            self.tab = Tab::Home;
        }
        self.outline_open = outline;
        self.tabs(ui, info, &mut out);
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
                        Tab::Insert => crate::insert::insert(ui, info, &mut out),
                        Tab::Design => crate::design::design(ui, info, &mut out),
                        Tab::References => crate::references::references(ui, info, &mut out),
                        Tab::Layout => crate::layout::layout(ui, info, &mut out),
                        Tab::TableDesign => crate::insert::table_design(ui, info, &mut out),
                        Tab::TableLayout => crate::insert::table_layout(ui, info, &mut out),
                        Tab::PictureFormat => crate::insert::picture_format(ui, info, &mut out),
                        Tab::HeaderFooter => crate::headers::header_footer(ui, info, &mut out),
                        Tab::Review => crate::comments::review(ui, info, &mut out),
                        Tab::Outline => crate::outlining::outlining(ui, info, &mut out),
                    });
                });
        }
        if self.keytips.active() {
            keytips::paint(ui.ctx(), &keytips::collected(ui.ctx()), &self.keytips.typed);
        }
        out
    }

    /// Меню «Файл» и вкладки.
    fn tabs(&mut self, ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
        ui.horizontal(|ui| {
            let file = egui::Button::new(RichText::new(tr("tab-file")).color(Color32::WHITE))
                .fill(ACCENT)
                .corner_radius(CornerRadius::same(2))
                .min_size(vec2(52.0, 0.0));
            let response = ui.add(file).on_hover_text(tr("tab-file-hint"));
            keytips::tip(
                ui,
                &response,
                Level::Tabs,
                "keytip-file",
                TipAction::Command(RibbonCommand::Backstage),
            );
            if response.clicked() {
                out.push(RibbonCommand::Backstage);
            }
            let mut contextual = Vec::new();
            if info.in_table {
                contextual.extend([Tab::TableDesign, Tab::TableLayout]);
            }
            if info.picture_wrap.is_some() {
                contextual.push(Tab::PictureFormat);
            }
            if info.story.is_some() {
                contextual.push(Tab::HeaderFooter);
            }
            // «Структура» — перед остальными вкладками, как в Word.
            let outline =
                (info.view_mode == crate::view::ViewMode::Outline).then_some(Tab::Outline);
            for tab in outline.into_iter().chain(Tab::ALL).chain(contextual) {
                let selected = self.tab == tab && !self.collapsed;
                let text = if selected {
                    RichText::new(tab.title())
                        .color(Palette::of(ui.ctx()).accent_text)
                        .strong()
                } else {
                    RichText::new(tab.title())
                };
                let response = ui.add(egui::Button::selectable(selected, text));
                keytips::tip(
                    ui,
                    &response,
                    Level::Tabs,
                    tab.keytip(),
                    TipAction::Tab(tab),
                );
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
        for (icon, name, tip, key, enabled, command) in [
            (
                Icon::Save,
                tr("save"),
                tr("save-hint"),
                "keytip-save",
                info.has_document,
                RibbonCommand::Save,
            ),
            (
                Icon::Undo,
                tr("undo"),
                tr("undo-hint"),
                "keytip-undo",
                info.can_undo,
                RibbonCommand::Undo,
            ),
            (
                Icon::Redo,
                tr("redo"),
                tr("redo-hint"),
                "keytip-redo",
                info.can_redo,
                RibbonCommand::Redo,
            ),
        ] {
            let response = icon_button(ui, icon, name, size, enabled, false).on_hover_text(tip);
            let action = TipAction::Command(command.clone());
            keytips::tip(ui, &response, Level::Tabs, key, action);
            if response.clicked() {
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
/// группы (как «Шрифт» и «Абзац» в Word); `launcher` — название окна,
/// подсказка клавиш и команда для неё. Возвращает, нажата ли кнопка.
pub(crate) fn group_with_launcher(
    ui: &mut Ui,
    caption: &str,
    launcher: Option<(&str, &str, RibbonCommand)>,
    content: impl FnOnce(&mut Ui),
) -> bool {
    let mut clicked = false;
    ui.vertical(|ui| {
        // Группа не уже своей подписи (в английском подписи длиннее).
        let font = egui::TextStyle::Small.resolve(ui.style());
        let caption_width = ui
            .painter()
            .layout_no_wrap(caption.to_owned(), font, Color32::PLACEHOLDER)
            .size()
            .x;
        ui.set_min_width(caption_width + 4.0);
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
        if let Some((name, keytip, command)) = launcher {
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
            keytips::tip(
                ui,
                &response,
                keytips::Level::Commands,
                keytip,
                keytips::TipAction::Command(command),
            );
            clicked = response.clicked();
        }
    });
    ui.separator();
    clicked
}

/// Подсказки клавиш вкладки «Вид» (для проверки).
#[cfg(test)]
pub const VIEW_KEYTIPS: [&str; 5] = [
    "keytip-view-ruler",
    "keytip-view-zoom-in",
    "keytip-view-zoom-out",
    "keytip-view-100",
    "keytip-view-page-width",
];

/// Кнопка с подсказкой клавиш: нажата ли.
fn command_button(
    ui: &mut Ui,
    response: egui::Response,
    keytip: &str,
    command: RibbonCommand,
    out: &mut Vec<RibbonCommand>,
) {
    keytips::tip(
        ui,
        &response,
        Level::Commands,
        keytip,
        TipAction::Command(command.clone()),
    );
    if response.clicked() {
        out.push(command);
    }
}

/// Вкладка «Вид».
fn view(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    group(ui, tr("group-views"), |ui| {
        ui.horizontal(|ui| {
            for mode in crate::view::ViewMode::ALL {
                let response = ui.add(egui::Button::selectable(
                    info.view_mode == mode,
                    tr(mode.key()),
                ));
                let keytip = match mode {
                    crate::view::ViewMode::Read => "keytip-view-read",
                    crate::view::ViewMode::Print => "keytip-view-print",
                    crate::view::ViewMode::Web => "keytip-view-web",
                    crate::view::ViewMode::Outline => "keytip-view-outline",
                    crate::view::ViewMode::Draft => "keytip-view-draft",
                };
                command_button(ui, response, keytip, RibbonCommand::ViewMode(mode), out);
            }
        });
    });
    group(ui, tr("group-show"), |ui| {
        let mut ruler = info.ruler;
        let response = ui.checkbox(&mut ruler, tr("ruler"));
        keytips::tip(
            ui,
            &response,
            Level::Commands,
            "keytip-view-ruler",
            TipAction::Command(RibbonCommand::ToggleRuler),
        );
        if response.changed() {
            out.push(RibbonCommand::ToggleRuler);
        }
        let mut navigation = info.navigation;
        let response = ui.checkbox(&mut navigation, tr("nav-title"));
        keytips::tip(
            ui,
            &response,
            Level::Commands,
            "keytip-view-navigation",
            TipAction::Command(RibbonCommand::ToggleNavigation),
        );
        if response.changed() {
            out.push(RibbonCommand::ToggleNavigation);
        }
    });
    group(ui, tr("zoom"), |ui| {
        ui.horizontal(|ui| {
            let response = ui.button(tr("zoom-in"));
            command_button(
                ui,
                response,
                "keytip-view-zoom-in",
                RibbonCommand::ZoomIn,
                out,
            );
            let response = ui.button(tr("zoom-out"));
            command_button(
                ui,
                response,
                "keytip-view-zoom-out",
                RibbonCommand::ZoomOut,
                out,
            );
        });
        ui.horizontal(|ui| {
            let response = ui.button("100 %");
            command_button(
                ui,
                response,
                "keytip-view-100",
                RibbonCommand::ZoomReset,
                out,
            );
            let response = ui.add(egui::Button::selectable(
                info.page_width,
                tr("zoom-page-width"),
            ));
            let command = RibbonCommand::PageWidth;
            command_button(ui, response, "keytip-view-page-width", command, out);
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
    fn shading_and_borders_buttons() {
        use letter_editor::{BorderEdge, ParagraphFormat};
        let mut h = harness(document());
        h.get_by_label("Заливка").click();
        h.run();
        h.get_by_label("Нижняя граница").click();
        h.run();
        assert_eq!(
            h.state().out,
            vec![
                RibbonCommand::Paragraph(ParagraphFormat::Shading(Some((0xD9, 0xD9, 0xD9)))),
                RibbonCommand::Paragraph(ParagraphFormat::Border(BorderEdge::Bottom)),
            ]
        );
    }

    #[test]
    fn design_tab_sets_page_color_and_borders() {
        use letter_editor::{Command, PageBorderPreset, PageSetup};
        let mut h = harness(document());
        h.get_by_label("Конструктор").click();
        h.run();
        h.get_all_by_label("Цвет страницы")
            .next()
            .expect("кнопка")
            .click();
        h.run();
        h.get_by_label("Нет цвета").click();
        h.run();
        h.get_all_by_label("Границы страниц")
            .next()
            .expect("кнопка")
            .click();
        h.run();
        h.get_by_label("Рамка").click();
        h.run();
        h.get_all_by_label("Подложка")
            .next()
            .expect("кнопка")
            .click();
        h.run();
        h.get_by_label("ЧЕРНОВИК").click();
        h.run();
        assert_eq!(
            h.state().out,
            vec![
                RibbonCommand::Edit(Command::PageColor(None)),
                RibbonCommand::Edit(Command::PageSetup(PageSetup::Borders(
                    PageBorderPreset::Box
                ))),
                RibbonCommand::Edit(Command::Watermark(Some("ЧЕРНОВИК".into()))),
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
    fn insert_a_table_and_change_it() {
        let mut h = harness(document());
        // Без таблицы вкладки «Макет таблицы» нет.
        assert!(h.query_by_label("Макет таблицы").is_none());
        h.get_by_label("Вставка").click();
        h.run();
        h.get_all_by_label("Таблица")
            .next()
            .expect("кнопка")
            .click();
        h.run();
        // Указатель над третьим столбцом второй строки сетки.
        let grid = h.get_by_label("Вставить таблицу").rect();
        let pos = grid.min + vec2(2.5 * 16.0, 1.5 * 16.0);
        h.input_mut().events.push(egui::Event::PointerMoved(pos));
        h.run();
        for pressed in [true, false] {
            h.input_mut().events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        h.run();
        assert_eq!(
            h.state().out,
            vec![RibbonCommand::Edit(letter_editor::Command::InsertTable {
                rows: 2,
                columns: 3
            })]
        );

        let mut h = harness(RibbonInfo {
            in_table: true,
            ..document()
        });
        h.get_by_label("Макет таблицы").click();
        h.run();
        h.get_by_label("Вставить снизу").click();
        h.run();
        h.get_by_label("Вставить слева").click();
        h.run();
        assert_eq!(
            h.state().out,
            vec![
                RibbonCommand::Edit(letter_editor::Command::InsertRows { below: true }),
                RibbonCommand::Edit(letter_editor::Command::InsertColumns { right: false }),
            ]
        );
        h.state_mut().out.clear();
        h.get_by_label("Конструктор таблиц").click();
        h.run();
        h.get_all_by_label("Границы")
            .next()
            .expect("кнопка")
            .click();
        h.run();
        h.get_by_label("Все границы").click();
        h.run();
        h.get_all_by_label("Заливка")
            .next()
            .expect("кнопка")
            .click();
        h.run();
        h.get_by_label("Нет цвета").click();
        h.run();
        assert_eq!(
            h.state().out,
            vec![
                RibbonCommand::Edit(letter_editor::Command::CellBorders(
                    letter_editor::BorderPreset::All
                )),
                RibbonCommand::Edit(letter_editor::Command::CellShading(None)),
            ]
        );
        // Курсор ушёл из таблицы — снова «Главная».
        h.state_mut().info.in_table = false;
        h.run();
        assert_eq!(h.state().ribbon.tab, Tab::Home);
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
    fn english_labels() {
        crate::i18n::set_language(crate::i18n::Language::English);
        let mut h = harness(document());
        h.get_by_label("File").click();
        h.run();
        h.get_by_label("Home");
        h.get_by_label("Clipboard");
        assert_eq!(h.state().out, vec![RibbonCommand::Backstage]);
        crate::i18n::set_language(crate::i18n::Language::Russian);
    }

    #[test]
    fn file_button_opens_backstage() {
        let mut h = harness(document());
        h.get_by_label("Файл").click();
        h.run();
        assert_eq!(h.state().out, vec![RibbonCommand::Backstage]);
    }
}
