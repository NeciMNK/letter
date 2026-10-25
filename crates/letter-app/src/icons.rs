//! Значки интерфейса из открытого набора Lucide (лицензия ISC, не значки
//! Microsoft). Набор подключён шрифтом `assets/lucide/lucide.ttf`: каждый
//! значок — один символ. Как добавить значок — `assets/lucide/README.md`.

use eframe::egui::{
    Align2, Color32, Context, CornerRadius, FontData, FontFamily, FontId, Painter, Rect, Response,
    RichText, Sense, Ui, Vec2, WidgetInfo,
    epaint::text::{FontInsert, FontPriority, InsertFontFamily},
    pos2, vec2,
};
use letter_model::Alignment;

/// Значок.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// Сохранить: дискета.
    Save,
    /// Отменить: стрелка назад.
    Undo,
    /// Повторить: стрелка вперёд.
    Redo,
    /// Вырезать: ножницы.
    Cut,
    /// Копировать: два листа.
    Copy,
    /// Вставить: планшет с листом.
    Paste,
    /// Выравнивание абзаца: строки.
    Align(Alignment),
    /// Увеличить размер шрифта.
    Grow,
    /// Уменьшить размер шрифта.
    Shrink,
    /// Зачёркнутый.
    Strike,
    /// Подстрочный знак.
    Subscript,
    /// Надстрочный знак.
    Superscript,
    /// Цвет выделения текста: маркер.
    Highlight,
    /// Цвет текста: буква над полоской.
    FontColor,
    /// Очистить оформление.
    ClearFormat,
    /// Стрелка вниз: открыть список.
    ChevronDown,
    /// Уменьшить отступ.
    IndentLess,
    /// Увеличить отступ.
    IndentMore,
    /// Межстрочный интервал.
    LineSpacing,
    /// Показать знаки форматирования: «¶».
    Pilcrow,
    /// Знак табуляции: стрелка вправо.
    TabMark,
    /// Знак разрыва строки: стрелка вниз и влево.
    LineBreakMark,
    /// Найти: лупа.
    Search,
    /// Заменить.
    Replace,
    /// Выделить все.
    SelectAll,
    /// Назад: стрелка влево.
    Back,
    /// Режим чтения: открытая книга.
    ReadMode,
    /// Разметка страницы: лист с текстом.
    PrintLayout,
    /// Веб-документ: глобус.
    WebLayout,
    /// Черновик: лист с карандашом.
    Draft,
    /// Открыть окно группы ленты: стрелка в угол.
    Launcher,
    /// Вставить с исходным оформлением.
    PasteKeep,
    /// Вставить, объединив оформление.
    PasteMerge,
    /// Вставить только текст.
    PasteText,
    /// Маркированный список.
    Bullets,
    /// Нумерованный список.
    Numbering,
    /// Многоуровневый список.
    Multilevel,
    /// Таблица.
    Table,
    /// Рисунок: картинка с горами.
    Picture,
    /// Обтекание текстом.
    WrapText,
    /// Поля страницы.
    Margins,
    /// Ориентация листа.
    Orientation,
    /// Размер листа.
    PageSize,
    /// Колонки.
    Columns,
    /// Разрывы.
    Breaks,
    /// Верхний колонтитул.
    Header,
    /// Нижний колонтитул.
    Footer,
    /// Номер страницы.
    PageNumber,
    /// Закрыть.
    Close,
    /// Дата и время: календарь с часами.
    DateTime,
    /// Поле: фигурные скобки.
    Field,
    /// Ссылка: звенья цепи.
    Link,
    /// Закладка.
    Bookmark,
    /// Концевая сноска: строки и знак конца.
    Endnote,
    /// Оглавление.
    Contents,
    /// Обновить: круговые стрелки.
    Refresh,
    /// Подложка: штамп.
    Watermark,
    /// Цвет страницы: палитра.
    PageColor,
    /// Символ: омега.
    Symbol,
    /// Расстановка переносов: стрелка к краю строки.
    Hyphenation,
    /// Титульная страница: книга с текстом.
    CoverPage,
    /// Пустая страница: лист с плюсом.
    BlankPage,
    /// Печать: принтер.
    Printer,
    /// Предыдущая страница: стрелка влево.
    ChevronLeft,
    /// Следующая страница: стрелка вправо.
    ChevronRight,
    /// Границы страниц: уголки рамки.
    PageBorders,
    /// Удалить строки, столбцы или таблицу.
    DeleteTable,
    /// Вставить строку сверху.
    InsertAbove,
    /// Вставить строку снизу.
    InsertBelow,
    /// Вставить столбец слева.
    InsertLeft,
    /// Вставить столбец справа.
    InsertRight,
    /// Заливка: ведро с краской.
    Shading,
    /// Все границы.
    BordersAll,
    /// Нет границ.
    BordersNone,
    /// Внешние границы.
    BordersOutside,
    /// Верхняя граница.
    BorderTop,
    /// Нижняя граница.
    BorderBottom,
    /// Левая граница.
    BorderLeft,
    /// Правая граница.
    BorderRight,
    /// Объединить ячейки.
    MergeCells,
}

/// Шрифт со значками.
const FONT: &[u8] = include_bytes!("../assets/lucide/lucide.ttf");

/// Имя семейства шрифта со значками в egui.
const FAMILY: &str = "lucide";

impl Icon {
    /// Символ значка в шрифте.
    pub fn glyph(self) -> char {
        self.entry().1
    }

    /// Имя значка в наборе Lucide и его символ.
    fn entry(self) -> (&'static str, char) {
        match self {
            Icon::Save => ("save", '\u{E14D}'),
            Icon::Undo => ("undo-2", '\u{E2A1}'),
            Icon::Redo => ("redo-2", '\u{E2A0}'),
            Icon::Cut => ("scissors", '\u{E14E}'),
            Icon::Copy => ("copy", '\u{E09E}'),
            Icon::Paste => ("clipboard-paste", '\u{E3E8}'),
            Icon::Align(Alignment::Left) => ("text-align-start", '\u{E185}'),
            Icon::Align(Alignment::Center) => ("text-align-center", '\u{E182}'),
            Icon::Align(Alignment::Right) => ("text-align-end", '\u{E183}'),
            Icon::Align(Alignment::Justify | Alignment::Distribute) => {
                ("text-align-justify", '\u{E184}')
            }
            Icon::Grow => ("a-arrow-up", '\u{E586}'),
            Icon::Shrink => ("a-arrow-down", '\u{E585}'),
            Icon::Strike => ("strikethrough", '\u{E177}'),
            Icon::Subscript => ("subscript", '\u{E25C}'),
            Icon::Superscript => ("superscript", '\u{E25E}'),
            Icon::Highlight => ("highlighter", '\u{E0F4}'),
            Icon::FontColor => ("baseline", '\u{E285}'),
            Icon::ClearFormat => ("remove-formatting", '\u{E3B3}'),
            Icon::ChevronDown => ("chevron-down", '\u{E06D}'),
            Icon::IndentLess => ("list-indent-decrease", '\u{E107}'),
            Icon::IndentMore => ("list-indent-increase", '\u{E108}'),
            Icon::LineSpacing => ("line-height", '\u{E7B7}'),
            Icon::Pilcrow => ("pilcrow", '\u{E3A3}'),
            Icon::TabMark => ("move-right", '\u{E490}'),
            Icon::LineBreakMark => ("corner-down-left", '\u{E0A1}'),
            Icon::Search => ("search", '\u{E151}'),
            Icon::Replace => ("replace", '\u{E3DB}'),
            Icon::SelectAll => ("text-select", '\u{E6EA}'),
            Icon::Back => ("arrow-left", '\u{E048}'),
            Icon::ReadMode => ("book-open", '\u{E05F}'),
            Icon::PrintLayout => ("file-text", '\u{E0CC}'),
            Icon::WebLayout => ("globe", '\u{E0E8}'),
            Icon::Draft => ("file-pen-line", '\u{E320}'),
            Icon::Launcher => ("arrow-down-right", '\u{E045}'),
            Icon::PasteKeep => ("clipboard-pen-line", '\u{E308}'),
            Icon::PasteMerge => ("clipboard-list", '\u{E086}'),
            Icon::PasteText => ("clipboard-type", '\u{E309}'),
            Icon::Bullets => ("list", '\u{E106}'),
            Icon::Numbering => ("list-ordered", '\u{E1D1}'),
            Icon::Multilevel => ("list-tree", '\u{E408}'),
            Icon::Table => ("table", '\u{E17D}'),
            Icon::Picture => ("image", '\u{E0F6}'),
            Icon::WrapText => ("wrap-text", '\u{E248}'),
            Icon::Margins => ("fullscreen", '\u{E534}'),
            Icon::Orientation => ("rectangle-horizontal", '\u{E376}'),
            Icon::PageSize => ("scaling", '\u{E2EC}'),
            Icon::Columns => ("columns-2", '\u{E098}'),
            Icon::Breaks => ("separator-horizontal", '\u{E1C8}'),
            Icon::Header => ("panel-top-dashed", '\u{E437}'),
            Icon::Footer => ("panel-bottom-dashed", '\u{E42E}'),
            Icon::PageNumber => ("hash", '\u{E0EF}'),
            Icon::Close => ("x", '\u{E1B2}'),
            Icon::DateTime => ("calendar-clock", '\u{E304}'),
            Icon::Field => ("braces", '\u{E36A}'),
            Icon::Link => ("link", '\u{E102}'),
            Icon::Bookmark => ("bookmark", '\u{E060}'),
            Icon::Endnote => ("list-end", '\u{E2DF}'),
            Icon::Contents => ("table-of-contents", '\u{E61E}'),
            Icon::Refresh => ("refresh-cw", '\u{E145}'),
            Icon::Watermark => ("stamp", '\u{E3BB}'),
            Icon::PageColor => ("palette", '\u{E1DD}'),
            Icon::Symbol => ("omega", '\u{E619}'),
            Icon::Hyphenation => ("arrow-right-to-line", '\u{E459}'),
            Icon::CoverPage => ("book-text", '\u{E54B}'),
            Icon::BlankPage => ("file-plus", '\u{E0C9}'),
            Icon::Printer => ("printer", '\u{E141}'),
            Icon::ChevronLeft => ("chevron-left", '\u{E06E}'),
            Icon::ChevronRight => ("chevron-right", '\u{E06F}'),
            Icon::PageBorders => ("scan", '\u{E257}'),
            Icon::DeleteTable => ("grid-2x2-x", '\u{E5E5}'),
            Icon::InsertAbove => ("between-horizontal-start", '\u{E592}'),
            Icon::InsertBelow => ("between-horizontal-end", '\u{E591}'),
            Icon::InsertLeft => ("between-vertical-start", '\u{E594}'),
            Icon::InsertRight => ("between-vertical-end", '\u{E593}'),
            Icon::Shading => ("paint-bucket", '\u{E2E6}'),
            Icon::BordersAll => ("grid-2x2", '\u{E4FF}'),
            Icon::BordersNone => ("square-dashed", '\u{E1CB}'),
            Icon::BordersOutside => ("square", '\u{E167}'),
            Icon::BorderTop => ("panel-top", '\u{E435}'),
            Icon::BorderBottom => ("panel-bottom", '\u{E42C}'),
            Icon::BorderLeft => ("panel-left", '\u{E12A}'),
            Icon::BorderRight => ("panel-right", '\u{E431}'),
            Icon::MergeCells => ("table-cells-merge", '\u{E5C7}'),
        }
    }
}

/// Кнопка-значок размера `size`. `name` — название кнопки для подсказки
/// и для программ чтения с экрана (и тестов).
pub fn icon_button(
    ui: &mut Ui,
    icon: Icon,
    name: &str,
    size: Vec2,
    enabled: bool,
    selected: bool,
) -> Response {
    icon_button_at(ui, icon, name, size, enabled, selected, 0.0)
}

/// Как [`icon_button`], но значок сдвинут по вертикали на `dy`.
fn icon_button_at(
    ui: &mut Ui,
    icon: Icon,
    name: &str,
    size: Vec2,
    enabled: bool,
    selected: bool,
    dy: f32,
) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response
        .widget_info(|| WidgetInfo::selected(eframe::egui::Role::Button, enabled, selected, name));
    let visuals = ui.style().interact_selectable(&response, selected);
    if enabled && (selected || response.hovered()) {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(2), visuals.weak_bg_fill);
    }
    let color = if enabled {
        visuals.fg_stroke.color
    } else {
        crate::theme::Palette::of(ui.ctx()).disabled
    };
    let side = rect.height().min(rect.width()).min(32.0) * 0.7;
    let area = Rect::from_center_size(rect.center() + vec2(0.0, dy), Vec2::splat(side));
    paint(ui.painter(), area, icon, color);
    response
}

/// Кнопка-значок с цветной полоской снизу (цвет текста, маркер): значок
/// чуть выше середины, под ним полоска цвета `bar`.
pub fn color_button(ui: &mut Ui, icon: Icon, name: &str, bar: Color32, enabled: bool) -> Response {
    let size = vec2(24.0, 24.0);
    let response = icon_button_at(ui, icon, name, size, enabled, false, -2.5);
    let r = response.rect;
    let strip = Rect::from_min_max(
        pos2(r.left() + 5.0, r.bottom() - 6.0),
        pos2(r.right() - 5.0, r.bottom() - 3.0),
    );
    let bar = if enabled {
        bar
    } else {
        crate::theme::Palette::of(ui.ctx()).disabled
    };
    ui.painter().rect_filled(strip, 0.0, bar);
    response
}

/// Символ значка как текст для обычных кнопок egui (например, стрелка
/// у списка). Пока шрифт не готов — пустая строка.
pub fn text(ui: &Ui, icon: Icon, size: f32) -> RichText {
    install(ui.ctx());
    if ready(ui.ctx()) {
        RichText::new(icon.glyph().to_string()).font(FontId::new(size, family()))
    } else {
        RichText::new("")
    }
}

/// Кнопка «значок и надпись» в одну строку.
pub fn icon_text_button(ui: &mut Ui, icon: Icon, text: &str, enabled: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        eframe::egui::TextStyle::Body.resolve(ui.style()),
        Color32::PLACEHOLDER,
    );
    let height = 20.0_f32.max(galley.size().y + 4.0);
    let size = vec2(height + 4.0 + galley.size().x + 4.0, height);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| WidgetInfo::labeled(eframe::egui::Role::Button, enabled, text));
    let visuals = ui.style().interact(&response);
    if enabled && response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(2), visuals.weak_bg_fill);
    }
    let color = if enabled {
        visuals.fg_stroke.color
    } else {
        crate::theme::Palette::of(ui.ctx()).disabled
    };
    let icon_rect = Rect::from_min_size(rect.min, Vec2::splat(height));
    let side = height * 0.7;
    paint(
        ui.painter(),
        Rect::from_center_size(icon_rect.center(), Vec2::splat(side)),
        icon,
        color,
    );
    let text_pos = pos2(
        icon_rect.right() + 4.0,
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(text_pos, galley, color);
    response
}

/// Рисует значок в квадрате `r`.
pub fn paint(painter: &Painter, r: Rect, icon: Icon, color: Color32) {
    let ctx = painter.ctx();
    install(ctx);
    if !ready(ctx) {
        // Шрифт подключится к следующему кадру.
        ctx.request_repaint();
        return;
    }
    painter.text(
        r.center(),
        Align2::CENTER_CENTER,
        icon.glyph(),
        FontId::new(r.height(), family()),
        color,
    );
}

/// Семейство шрифта со значками.
fn family() -> FontFamily {
    FontFamily::Name(FAMILY.into())
}

/// Подключает шрифт со значками; он появится к началу следующего кадра
/// (или текущего, если в нём ещё не было текста). Можно вызывать и до
/// первого кадра; повторный вызов ничего не стоит.
pub fn install(ctx: &Context) {
    ctx.add_font(FontInsert::new(
        FAMILY,
        FontData::from_static(FONT),
        vec![InsertFontFamily {
            family: family(),
            priority: FontPriority::Highest,
        }],
    ));
}

/// Готов ли шрифт со значками. Только во время кадра.
fn ready(ctx: &Context) -> bool {
    ctx.fonts(|f| f.definitions().families.contains_key(&family()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Icon; 66] = [
        Icon::Save,
        Icon::Undo,
        Icon::Redo,
        Icon::Cut,
        Icon::Copy,
        Icon::Paste,
        Icon::Align(Alignment::Left),
        Icon::Align(Alignment::Center),
        Icon::Align(Alignment::Right),
        Icon::Align(Alignment::Justify),
        Icon::Align(Alignment::Distribute),
        Icon::Grow,
        Icon::Shrink,
        Icon::Strike,
        Icon::Subscript,
        Icon::Superscript,
        Icon::Highlight,
        Icon::FontColor,
        Icon::ClearFormat,
        Icon::ChevronDown,
        Icon::IndentLess,
        Icon::IndentMore,
        Icon::LineSpacing,
        Icon::Pilcrow,
        Icon::TabMark,
        Icon::LineBreakMark,
        Icon::Search,
        Icon::Replace,
        Icon::SelectAll,
        Icon::Back,
        Icon::ReadMode,
        Icon::PrintLayout,
        Icon::WebLayout,
        Icon::Draft,
        Icon::Launcher,
        Icon::PasteKeep,
        Icon::PasteMerge,
        Icon::PasteText,
        Icon::Picture,
        Icon::WrapText,
        Icon::Margins,
        Icon::Orientation,
        Icon::PageSize,
        Icon::Columns,
        Icon::Breaks,
        Icon::Header,
        Icon::Footer,
        Icon::PageNumber,
        Icon::Close,
        Icon::DateTime,
        Icon::Field,
        Icon::Link,
        Icon::Bookmark,
        Icon::Endnote,
        Icon::Contents,
        Icon::Refresh,
        Icon::Watermark,
        Icon::PageColor,
        Icon::PageBorders,
        Icon::Symbol,
        Icon::Hyphenation,
        Icon::CoverPage,
        Icon::BlankPage,
        Icon::Printer,
        Icon::ChevronLeft,
        Icon::ChevronRight,
    ];

    #[test]
    fn codes_match_the_font() {
        let codes = include_str!("../assets/lucide/codes.txt");
        for icon in ALL {
            let code = format!("{:X}", icon.glyph() as u32);
            let line = codes
                .lines()
                .find(|l| l.split(' ').next() == Some(icon.entry().0))
                .unwrap_or_else(|| panic!("нет значка {}", icon.entry().0));
            assert_eq!(line, format!("{} {code}", icon.entry().0));
        }
    }

    #[test]
    fn font_has_the_glyphs() {
        let mut h = egui_kittest::Harness::new_ui(|ui| {
            install(ui.ctx());
        });
        h.run();
        assert!(ready(&h.ctx));
        let id = FontId::new(16.0, family());
        for icon in ALL {
            assert!(
                h.ctx.fonts_mut(|f| f.has_glyph(&id, icon.glyph())),
                "{icon:?}"
            );
        }
    }
}
