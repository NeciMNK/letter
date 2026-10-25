//! Абзацы ([`Paragraph`]) и оформление абзаца ([`ParagraphProps`]).

use crate::NumberingRef;
use crate::{ParagraphBorders, RawXml, Run, Shading, Twips};

/// Абзац: последовательность фрагментов текста с общим оформлением абзаца.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Paragraph {
    /// Оформление абзаца.
    pub props: ParagraphProps,
    /// Содержимое абзаца по порядку: фрагменты текста и непонятые элементы.
    pub content: Vec<Inline>,
}

/// Элемент содержимого абзаца.
#[derive(Debug, Clone, PartialEq, Eq)]
// Фрагменты текста — основной случай, поэтому их не прячем в `Box`.
#[allow(clippy::large_enum_variant)]
pub enum Inline {
    /// Фрагмент текста.
    Run(Run),
    /// Начало закладки (`w:bookmarkStart`).
    BookmarkStart(Bookmark),
    /// Конец закладки (`w:bookmarkEnd`) с номером закладки.
    BookmarkEnd(String),
    /// Начало куска текста, к которому относится примечание
    /// (`w:commentRangeStart`), с номером примечания.
    CommentStart(String),
    /// Конец такого куска (`w:commentRangeEnd`).
    CommentEnd(String),
    /// Непонятый элемент (гиперссылка, закладка, исправление и т. п.),
    /// сохранённый как есть.
    Unknown(RawXml),
}

/// Закладка: именованное место или кусок документа. Начало и конец
/// лежат в содержимом абзацев (могут быть в разных абзацах) и связаны
/// номером.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    /// Номер (`w:id`), общий у начала и конца.
    pub id: String,
    /// Имя (`w:name`). Имена на `_` — скрытые (оглавление, ссылки).
    pub name: String,
}

impl From<Run> for Inline {
    fn from(run: Run) -> Inline {
        Inline::Run(run)
    }
}

impl Paragraph {
    /// Абзац из одного фрагмента с обычным текстом.
    pub fn text(text: impl Into<String>) -> Paragraph {
        Paragraph::from_runs([Run::text(text)])
    }

    /// Абзац из готовых фрагментов.
    pub fn from_runs(runs: impl IntoIterator<Item = Run>) -> Paragraph {
        Paragraph {
            props: ParagraphProps::default(),
            content: runs.into_iter().map(Inline::Run).collect(),
        }
    }

    /// Фрагменты текста абзаца (без непонятых элементов).
    pub fn runs(&self) -> impl Iterator<Item = &Run> {
        self.content.iter().filter_map(|item| match item {
            Inline::Run(run) => Some(run),
            _ => None,
        })
    }

    /// Тот же абзац, но с другим оформлением.
    pub fn with_props(mut self, props: ParagraphProps) -> Paragraph {
        self.props = props;
        self
    }

    /// Текст абзаца без оформления (см. [`Run::plain_text`]).
    pub fn plain_text(&self) -> String {
        self.runs().map(Run::plain_text).collect()
    }
}

/// Оформление абзаца. `None` значит «наследуется от стиля».
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParagraphProps {
    /// Идентификатор стиля абзаца (например, `"Heading1"`).
    pub style: Option<String>,
    /// Список, к которому относится абзац.
    pub numbering: Option<NumberingRef>,
    /// Выравнивание.
    pub alignment: Option<Alignment>,
    /// Отступы слева, справа и первой строки.
    pub indentation: Indentation,
    /// Интервалы до, после и между строками.
    pub spacing: Spacing,
    /// «Не добавлять интервал между абзацами одного стиля».
    pub contextual_spacing: Option<bool>,
    /// «Не отрывать от следующего»: абзац на той же странице, что
    /// и следующий.
    pub keep_next: Option<bool>,
    /// «Не разрывать абзац»: все строки на одной странице.
    pub keep_lines: Option<bool>,
    /// «С новой страницы».
    pub page_break_before: Option<bool>,
    /// «Запрет висячих строк»: одна строка абзаца не остаётся одна
    /// внизу или вверху страницы.
    pub widow_control: Option<bool>,
    /// Позиции табуляции. Пусто — в этом слое не заданы. Позиция вида
    /// [`TabStopKind::Clear`] убирает унаследованную от стиля.
    pub tabs: Vec<TabStop>,
    /// Границы абзаца.
    pub borders: Option<ParagraphBorders>,
    /// Заливка абзаца.
    pub shading: Option<Shading>,
    /// «Запретить автоматический перенос слов» (`w:suppressAutoHyphens`).
    pub suppress_auto_hyphens: Option<bool>,
    /// Свойства, которые программа пока не понимает, в порядке из файла.
    pub unknown: Vec<RawXml>,
}

/// Выравнивание абзаца.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Alignment {
    /// По левому краю (Ctrl+L). Для письма справа налево — по правому.
    Left,
    /// По центру (Ctrl+E).
    Center,
    /// По правому краю (Ctrl+R). Для письма справа налево — по левому.
    Right,
    /// По ширине (Ctrl+J): последняя строка по левому краю.
    Justify,
    /// Распределённое: по ширине, включая последнюю строку.
    Distribute,
}

/// Отступы абзаца от полей страницы.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Indentation {
    /// Отступ слева.
    pub left: Option<Twips>,
    /// Отступ справа.
    pub right: Option<Twips>,
    /// Отступ или выступ первой строки.
    pub first_line: Option<FirstLineIndent>,
}

/// Особый отступ первой строки (как поле «Первая строка» в диалоге «Абзац»).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FirstLineIndent {
    /// Первая строка сдвинута вправо на это расстояние («Отступ»).
    FirstLine(Twips),
    /// Первая строка выступает влево на это расстояние («Выступ»).
    Hanging(Twips),
}

/// Интервалы абзаца.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Spacing {
    /// Интервал перед абзацем.
    pub before: Option<Twips>,
    /// Интервал после абзаца.
    pub after: Option<Twips>,
    /// Межстрочный интервал.
    pub line: Option<LineSpacing>,
}

/// Межстрочный интервал.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LineSpacing {
    /// Множитель высоты строки в 240-х долях: `240` — одинарный,
    /// `276` — 1,15, `360` — полуторный, `480` — двойной.
    Multiple(u32),
    /// Ровно заданная высота строки («Точно»).
    Exact(Twips),
    /// Высота строки не меньше заданной («Минимум»).
    AtLeast(Twips),
}

impl LineSpacing {
    /// Одинарный интервал.
    pub const SINGLE: LineSpacing = LineSpacing::Multiple(240);
    /// Полуторный интервал.
    pub const ONE_AND_HALF: LineSpacing = LineSpacing::Multiple(360);
    /// Двойной интервал.
    pub const DOUBLE: LineSpacing = LineSpacing::Multiple(480);

    /// Множитель интервала, например `1.15`. Округляет до 1/240.
    pub fn multiple(factor: f64) -> LineSpacing {
        LineSpacing::Multiple((factor * 240.0).round().max(0.0) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RunProps;

    #[test]
    fn plain_text_joins_runs() {
        let bold = RunProps {
            bold: Some(true),
            ..RunProps::default()
        };
        let p = Paragraph::from_runs([Run::text("Привет, "), Run::text("мир").with_props(bold)]);
        assert_eq!(p.plain_text(), "Привет, мир");
        assert_eq!(p.runs().count(), 2);
    }

    #[test]
    fn unknown_inlines_are_kept_but_not_text() {
        let mut p = Paragraph::text("до");
        p.content
            .push(Inline::Unknown(RawXml::new("<w:bookmarkEnd w:id=\"0\"/>")));
        p.content.push(Run::text(" после").into());
        assert_eq!(p.plain_text(), "до после");
        assert_eq!(p.runs().count(), 2);
        assert_eq!(p.content.len(), 3);
    }

    #[test]
    fn line_spacing_multiple() {
        assert_eq!(LineSpacing::multiple(1.0), LineSpacing::SINGLE);
        assert_eq!(LineSpacing::multiple(1.15), LineSpacing::Multiple(276));
        assert_eq!(LineSpacing::multiple(2.0), LineSpacing::DOUBLE);
    }

    #[test]
    fn props_default_to_inherited() {
        let p = Paragraph::text("x");
        assert_eq!(p.props.alignment, None);
        assert_eq!(p.props.indentation.left, None);
        assert_eq!(p.props.spacing.line, None);
    }
}

/// Позиция табуляции.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabStop {
    /// Расстояние от левого поля.
    pub position: Twips,
    /// Вид.
    pub kind: TabStopKind,
    /// Заполнитель.
    pub leader: TabLeader,
}

/// Вид позиции табуляции.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TabStopKind {
    /// По левому краю.
    Left,
    /// По центру.
    Center,
    /// По правому краю.
    Right,
    /// По десятичному разделителю.
    Decimal,
    /// Вертикальная черта.
    Bar,
    /// Убирает унаследованную позицию в этом месте.
    Clear,
}

/// Заполнитель перед позицией табуляции.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TabLeader {
    /// Нет.
    #[default]
    None,
    /// Точки.
    Dot,
    /// Дефисы.
    Hyphen,
    /// Линия.
    Underscore,
    /// Толстая линия.
    Heavy,
    /// Точки по центру строки.
    MiddleDot,
}
