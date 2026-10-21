//! Абзацы ([`Paragraph`]) и оформление абзаца ([`ParagraphProps`]).

use crate::{Run, Twips};

/// Абзац: последовательность фрагментов текста с общим оформлением абзаца.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Paragraph {
    /// Оформление абзаца.
    pub props: ParagraphProps,
    /// Фрагменты текста по порядку.
    pub runs: Vec<Run>,
}

impl Paragraph {
    /// Абзац из одного фрагмента с обычным текстом.
    pub fn text(text: impl Into<String>) -> Paragraph {
        Paragraph {
            props: ParagraphProps::default(),
            runs: vec![Run::text(text)],
        }
    }

    /// Абзац из готовых фрагментов.
    pub fn from_runs(runs: impl IntoIterator<Item = Run>) -> Paragraph {
        Paragraph {
            props: ParagraphProps::default(),
            runs: runs.into_iter().collect(),
        }
    }

    /// Тот же абзац, но с другим оформлением.
    pub fn with_props(mut self, props: ParagraphProps) -> Paragraph {
        self.props = props;
        self
    }

    /// Текст абзаца без оформления (см. [`Run::plain_text`]).
    pub fn plain_text(&self) -> String {
        self.runs.iter().map(Run::plain_text).collect()
    }
}

/// Оформление абзаца. `None` значит «наследуется от стиля».
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParagraphProps {
    /// Идентификатор стиля абзаца (например, `"Heading1"`).
    pub style: Option<String>,
    /// Выравнивание.
    pub alignment: Option<Alignment>,
    /// Отступы слева, справа и первой строки.
    pub indentation: Indentation,
    /// Интервалы до, после и между строками.
    pub spacing: Spacing,
    /// «Не добавлять интервал между абзацами одного стиля».
    pub contextual_spacing: Option<bool>,
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
        assert_eq!(p.runs.len(), 2);
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
