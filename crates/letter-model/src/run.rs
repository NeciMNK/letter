//! Фрагменты текста ([`Run`]) и оформление символов ([`RunProps`]).

use crate::RawXml;

/// Фрагмент абзаца: кусок содержимого с одинаковым оформлением символов.
///
/// Абзац «Привет, **мир**» состоит из двух фрагментов: «Привет, » и «мир»
/// (второй — жирный).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Run {
    /// Оформление символов этого фрагмента.
    pub props: RunProps,
    /// Содержимое по порядку: текст, табуляции, разрывы.
    pub content: Vec<RunContent>,
}

impl Run {
    /// Фрагмент с обычным текстом и пустым оформлением.
    pub fn text(text: impl Into<String>) -> Run {
        Run {
            props: RunProps::default(),
            content: vec![RunContent::Text(text.into())],
        }
    }

    /// Тот же фрагмент, но с другим оформлением.
    pub fn with_props(mut self, props: RunProps) -> Run {
        self.props = props;
        self
    }

    /// Текст фрагмента без оформления: табуляция становится `\t`,
    /// разрыв строки — `\n`, разрыв страницы или колонки — `\u{c}`.
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for item in &self.content {
            item.push_plain_text(&mut out);
        }
        out
    }
}

/// Один элемент содержимого фрагмента.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunContent {
    /// Обычный текст.
    Text(String),
    /// Знак табуляции.
    Tab,
    /// Принудительный разрыв.
    Break(BreakKind),
    /// Непонятый элемент (рисунок, поле, символ и т. п.), сохранённый как есть.
    Unknown(RawXml),
}

impl RunContent {
    fn push_plain_text(&self, out: &mut String) {
        match self {
            RunContent::Text(text) => out.push_str(text),
            RunContent::Tab => out.push('\t'),
            RunContent::Break(BreakKind::Line) => out.push('\n'),
            RunContent::Break(BreakKind::Page | BreakKind::Column) => out.push('\u{c}'),
            RunContent::Unknown(_) => {}
        }
    }
}

/// Вид принудительного разрыва.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BreakKind {
    /// Разрыв строки (Shift+Enter): новая строка в том же абзаце.
    Line,
    /// Разрыв страницы (Ctrl+Enter).
    Page,
    /// Разрыв колонки (Ctrl+Shift+Enter).
    Column,
}

/// Оформление символов: шрифт, размер, начертание, цвет и т. п.
///
/// Каждое поле необязательно: `None` значит «наследуется» от стиля символов,
/// стиля абзаца или значений документа по умолчанию.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunProps {
    /// Идентификатор стиля символов (например, `"Strong"`).
    pub style: Option<String>,
    /// Шрифты для разных видов письменности.
    pub fonts: RunFonts,
    /// Размер шрифта.
    pub size: Option<crate::HalfPoints>,
    /// Полужирный.
    pub bold: Option<bool>,
    /// Курсив.
    pub italic: Option<bool>,
    /// Подчёркивание и его вид.
    pub underline: Option<Underline>,
    /// Зачёркивание одной линией.
    pub strike: Option<bool>,
    /// Зачёркивание двойной линией.
    pub double_strike: Option<bool>,
    /// Цвет текста.
    pub color: Option<Color>,
    /// Выделение цветом (маркер).
    pub highlight: Option<Highlight>,
    /// Надстрочный или подстрочный знак.
    pub vertical_align: Option<VerticalAlign>,
    /// Свойства, которые программа пока не понимает, в порядке из файла.
    pub unknown: Vec<RawXml>,
}

/// Шрифты фрагмента. Word выбирает шрифт по виду символа: латиница и
/// кириллица берут `ascii`/`h_ansi`, иероглифы — `east_asia`,
/// арабское письмо и иврит — `complex_script`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunFonts {
    /// Шрифт для символов ASCII (`w:ascii`).
    pub ascii: Option<String>,
    /// Шрифт для остальных символов «высокой» части ANSI, в том числе кириллицы (`w:hAnsi`).
    pub h_ansi: Option<String>,
    /// Шрифт для восточноазиатского письма (`w:eastAsia`).
    pub east_asia: Option<String>,
    /// Шрифт для сложного письма: арабского, иврита и др. (`w:cs`).
    pub complex_script: Option<String>,
}

impl RunFonts {
    /// Один и тот же шрифт для латиницы и кириллицы — самый частый случай.
    pub fn new(name: impl Into<String>) -> RunFonts {
        let name = name.into();
        RunFonts {
            ascii: Some(name.clone()),
            h_ansi: Some(name),
            east_asia: None,
            complex_script: None,
        }
    }
}

/// Цвет текста.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Color {
    /// «Авто»: Word сам выбирает чёрный или белый в зависимости от фона.
    Auto,
    /// Конкретный цвет: красный, зелёный, синий (0–255).
    Rgb(u8, u8, u8),
}

/// Вид подчёркивания (как в списке «Подчёркивание» диалога «Шрифт»).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Underline {
    /// Без подчёркивания (явно отключено).
    None,
    /// Одинарное.
    Single,
    /// Только слова, без пробелов.
    Words,
    /// Двойное.
    Double,
    /// Толстое.
    Thick,
    /// Точками.
    Dotted,
    /// Толстое точками.
    DottedHeavy,
    /// Штрихами.
    Dash,
    /// Толстое штрихами.
    DashedHeavy,
    /// Длинными штрихами.
    DashLong,
    /// Толстое длинными штрихами.
    DashLongHeavy,
    /// Штрих-пунктир.
    DotDash,
    /// Толстый штрих-пунктир.
    DashDotHeavy,
    /// Штрих-пунктир с двумя точками.
    DotDotDash,
    /// Толстый штрих-пунктир с двумя точками.
    DashDotDotHeavy,
    /// Волнистое.
    Wave,
    /// Толстое волнистое.
    WavyHeavy,
    /// Двойное волнистое.
    WavyDouble,
}

/// Цвет выделения (маркера). В Word их ровно 16 плюс «нет цвета».
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum Highlight {
    None,
    Black,
    Blue,
    Cyan,
    Green,
    Magenta,
    Red,
    Yellow,
    White,
    DarkBlue,
    DarkCyan,
    DarkGreen,
    DarkMagenta,
    DarkRed,
    DarkYellow,
    DarkGray,
    LightGray,
}

impl Highlight {
    /// Цвет маркера в RGB, как его рисует Word. Для [`Highlight::None`] — `None`.
    pub fn rgb(self) -> Option<(u8, u8, u8)> {
        let rgb = match self {
            Highlight::None => return None,
            Highlight::Black => (0x00, 0x00, 0x00),
            Highlight::Blue => (0x00, 0x00, 0xFF),
            Highlight::Cyan => (0x00, 0xFF, 0xFF),
            Highlight::Green => (0x00, 0xFF, 0x00),
            Highlight::Magenta => (0xFF, 0x00, 0xFF),
            Highlight::Red => (0xFF, 0x00, 0x00),
            Highlight::Yellow => (0xFF, 0xFF, 0x00),
            Highlight::White => (0xFF, 0xFF, 0xFF),
            Highlight::DarkBlue => (0x00, 0x00, 0x80),
            Highlight::DarkCyan => (0x00, 0x80, 0x80),
            Highlight::DarkGreen => (0x00, 0x80, 0x00),
            Highlight::DarkMagenta => (0x80, 0x00, 0x80),
            Highlight::DarkRed => (0x80, 0x00, 0x00),
            Highlight::DarkYellow => (0x80, 0x80, 0x00),
            Highlight::DarkGray => (0x80, 0x80, 0x80),
            Highlight::LightGray => (0xC0, 0xC0, 0xC0),
        };
        Some(rgb)
    }
}

/// Положение символов относительно базовой линии.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VerticalAlign {
    /// Обычное положение (явно отменяет индекс, заданный в стиле).
    Baseline,
    /// Надстрочный знак: x².
    Superscript,
    /// Подстрочный знак: H₂O.
    Subscript,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_of_mixed_content() {
        let run = Run {
            props: RunProps::default(),
            content: vec![
                RunContent::Text("a".into()),
                RunContent::Tab,
                RunContent::Text("b".into()),
                RunContent::Break(BreakKind::Line),
                RunContent::Break(BreakKind::Page),
            ],
        };
        assert_eq!(run.plain_text(), "a\tb\n\u{c}");
    }

    #[test]
    fn text_constructor_and_props() {
        let bold = RunProps {
            bold: Some(true),
            ..RunProps::default()
        };
        let run = Run::text("жирный").with_props(bold.clone());
        assert_eq!(run.plain_text(), "жирный");
        assert_eq!(run.props, bold);
        assert_eq!(run.props.italic, None);
    }

    #[test]
    fn fonts_new_sets_latin_and_cyrillic() {
        let fonts = RunFonts::new("Calibri");
        assert_eq!(fonts.ascii.as_deref(), Some("Calibri"));
        assert_eq!(fonts.h_ansi.as_deref(), Some("Calibri"));
        assert_eq!(fonts.east_asia, None);
    }

    #[test]
    fn highlight_colors() {
        assert_eq!(Highlight::None.rgb(), None);
        assert_eq!(Highlight::Yellow.rgb(), Some((255, 255, 0)));
    }
}
