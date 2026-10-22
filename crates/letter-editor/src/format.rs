//! Оформление выделенного: шрифт и размер, полужирный, курсив,
//! подчёркивание, зачёркивание, индексы, цвет, выделение цветом,
//! очистка оформления (Ctrl+B, Ctrl+I, Ctrl+U, Ctrl+], Ctrl+[, Ctrl+=,
//! Ctrl+Shift+=, Ctrl+Пробел) и выравнивание абзацев (Ctrl+L, Ctrl+E,
//! Ctrl+R, Ctrl+J).

use letter_model::{
    Alignment, Color, Document, FirstLineIndent, HalfPoints, Highlight, Inline, LineSpacing,
    Paragraph, ResolvedParagraphProps, ResolvedRunProps, Run, RunProps, StyleKind, TabLeader,
    TabStop, TabStopKind, Twips, Underline, VerticalAlign,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::edit::{paragraph_mut, paragraph_ref, split_content, text_len, tidy};
use crate::position::Position;

/// Что поменять в оформлении символов.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunFormat {
    /// Ctrl+B: полужирный — включить или выключить.
    Bold,
    /// Ctrl+I: курсив.
    Italic,
    /// Ctrl+U: одинарное подчёркивание.
    Underline,
    /// Ctrl+]: шрифт на 1 пт больше.
    Grow,
    /// Ctrl+[: шрифт на 1 пт меньше.
    Shrink,
    /// Зачёркнутый — включить или выключить.
    Strike,
    /// Ctrl+=: подстрочный знак — включить или выключить.
    Subscript,
    /// Ctrl+Shift+=: надстрочный знак — включить или выключить.
    Superscript,
    /// Шрифт с этим именем (для латиницы, кириллицы и сложных письменностей,
    /// как список «Шрифт» на ленте Word).
    Font(String),
    /// Размер шрифта.
    Size(HalfPoints),
    /// Цвет текста; [`Color::Auto`] — «Авто».
    Color(Color),
    /// Выделение цветом (маркер); [`Highlight::None`] — «Нет цвета».
    Highlight(Highlight),
    /// Ctrl+Пробел: убрать оформление символов, оставив оформление стиля.
    Clear,
    /// Стиль символов с этим идентификатором.
    CharStyle(String),
}

/// Самый маленький и самый большой размер шрифта Word, полупункты.
const MIN_SIZE: u32 = 2;
const MAX_SIZE: u32 = 3276;

/// Кусок абзаца, который меняется: номер абзаца, начало и конец.
type Piece = (usize, usize, usize);

/// Меняет оформление текста от `start` до `end`. Если ничего не
/// выделено, как в Word меняется слово под курсором (курсор внутри
/// слова) или пустой абзац (набранный в нём текст получит это
/// оформление). Полужирный, курсив и подчёркивание включаются, если
/// хоть где-то в выделенном их нет, иначе выключаются.
pub fn format_runs(doc: &mut Document, start: Position, end: Position, format: RunFormat) {
    let pieces = pieces(doc, start, end);
    if pieces.is_empty() {
        return;
    }
    // Включить или выключить: смотрим итоговое оформление выделенного.
    let mut all_on = true;
    for &(index, from, to) in &pieces {
        let Some(paragraph) = paragraph_ref(doc, index) else {
            continue;
        };
        let default = RunProps::default();
        let mut touched: Vec<&RunProps> = run_props_in(paragraph, from, to).collect();
        // Пустой абзац без фрагментов: оформление по умолчанию.
        if touched.is_empty() {
            touched.push(&default);
        }
        for props in touched {
            let resolved = doc.resolve_run(&paragraph.props, props);
            all_on &= match format {
                RunFormat::Bold => resolved.bold,
                RunFormat::Italic => resolved.italic,
                RunFormat::Underline => resolved.underline != Underline::None,
                RunFormat::Strike => resolved.strike,
                RunFormat::Subscript => resolved.vertical_align == VerticalAlign::Subscript,
                RunFormat::Superscript => resolved.vertical_align == VerticalAlign::Superscript,
                _ => true,
            };
        }
    }
    for (index, from, to) in pieces {
        // Размер считается от итогового каждого фрагмента: нужен доступ
        // к стилям, поэтому сначала собираем итоговые размеры.
        let sizes: Vec<u32> = match paragraph_ref(doc, index) {
            Some(p) => run_props_in(p, from, to)
                .map(|r| doc.resolve_run(&p.props, r).size.0)
                .collect(),
            None => continue,
        };
        let Some(paragraph) = paragraph_mut(doc, index) else {
            continue;
        };
        let mut sizes = sizes.into_iter();
        apply(paragraph, from, to, |props| match &format {
            RunFormat::Bold => props.bold = Some(!all_on),
            RunFormat::Italic => props.italic = Some(!all_on),
            RunFormat::Underline => {
                props.underline = Some(if all_on {
                    Underline::None
                } else {
                    Underline::Single
                });
            }
            RunFormat::Grow | RunFormat::Shrink => {
                let size = sizes.next().unwrap_or(22);
                let size = if format == RunFormat::Grow {
                    size + 2
                } else {
                    size.saturating_sub(2)
                };
                props.size = Some(HalfPoints(size.clamp(MIN_SIZE, MAX_SIZE)));
            }
            RunFormat::Strike => {
                props.strike = Some(!all_on);
                if !all_on {
                    props.double_strike = Some(false);
                }
            }
            RunFormat::Subscript | RunFormat::Superscript => {
                props.vertical_align = Some(if all_on {
                    VerticalAlign::Baseline
                } else if format == RunFormat::Subscript {
                    VerticalAlign::Subscript
                } else {
                    VerticalAlign::Superscript
                });
            }
            RunFormat::Font(name) => {
                let fonts = &mut props.fonts;
                fonts.ascii = Some(name.clone());
                fonts.h_ansi = Some(name.clone());
                fonts.complex_script = Some(name.clone());
                fonts.ascii_theme = None;
                fonts.h_ansi_theme = None;
                fonts.complex_script_theme = None;
            }
            RunFormat::Size(size) => {
                props.size = Some(HalfPoints(size.0.clamp(MIN_SIZE, MAX_SIZE)));
            }
            RunFormat::Color(color) => {
                props.color = Some(*color);
                props.theme_color = None;
            }
            RunFormat::Highlight(highlight) => props.highlight = Some(*highlight),
            RunFormat::CharStyle(id) => props.style = Some(id.clone()),
            // Неизвестные свойства (их нам не показать) остаются.
            RunFormat::Clear => {
                *props = RunProps {
                    unknown: std::mem::take(&mut props.unknown),
                    ..RunProps::default()
                };
            }
        });
    }
}

/// Выравнивает абзацы от `start` до `end`. Как в Word, повторное
/// нажатие (все абзацы уже так выровнены) возвращает выравнивание
/// по левому краю.
pub fn align_paragraphs(doc: &mut Document, start: Position, end: Position, alignment: Alignment) {
    let (first, last) = ordered(start, end);
    let range = first.paragraph..=last.paragraph;
    let all = range.clone().all(|i| {
        paragraph_ref(doc, i).is_none_or(|p| doc.resolve_paragraph(&p.props).alignment == alignment)
    });
    let value = if all && alignment != Alignment::Left {
        Alignment::Left
    } else {
        alignment
    };
    for index in range {
        if let Some(paragraph) = paragraph_mut(doc, index) {
            paragraph.props.alignment = Some(value);
        }
    }
}

/// Что поменять в оформлении абзацев (группа «Абзац» на ленте).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParagraphFormat {
    /// «Увеличить отступ»: отступ слева до следующей позиции табуляции
    /// по умолчанию.
    IndentMore,
    /// «Уменьшить отступ»: до предыдущей позиции, не меньше нуля.
    IndentLess,
    /// Межстрочный интервал.
    LineSpacing(LineSpacing),
    /// Интервал перед абзацем.
    SpaceBefore(Twips),
    /// Интервал после абзаца.
    SpaceAfter(Twips),
    /// Отступы, как их ставит линейка: слева, справа и первой строки.
    Indents {
        /// Отступ слева.
        left: Twips,
        /// Отступ справа.
        right: Twips,
        /// Отступ или выступ первой строки.
        first_line: Option<FirstLineIndent>,
    },
    /// Добавить позицию табуляции (щелчок по линейке). Позиция в том же
    /// месте заменяется.
    AddTab(TabStop),
    /// Перенести позицию табуляции (перетаскивание по линейке).
    MoveTab {
        /// Где была.
        from: Twips,
        /// Куда перенесена.
        to: Twips,
    },
    /// Убрать позицию табуляции (стащить с линейки).
    RemoveTab(Twips),
}

/// Меняет оформление абзацев от `start` до `end`.
pub fn format_paragraphs(
    doc: &mut Document,
    start: Position,
    end: Position,
    format: ParagraphFormat,
) {
    let (first, last) = ordered(start, end);
    // Шаг отступа — табуляция по умолчанию (в русском Word 1,25 см).
    let step = doc.settings.default_tab_stop.0.max(1);
    for index in first.paragraph..=last.paragraph {
        let Some(left) =
            paragraph_ref(doc, index).map(|p| doc.resolve_paragraph(&p.props).indent_left.0)
        else {
            continue;
        };
        let Some(paragraph) = paragraph_mut(doc, index) else {
            continue;
        };
        let props = &mut paragraph.props;
        match format {
            ParagraphFormat::IndentMore => {
                let next = (left.div_euclid(step) + 1) * step;
                props.indentation.left = Some(Twips(next));
            }
            ParagraphFormat::IndentLess => {
                let previous = (left + step - 1).div_euclid(step) - 1;
                props.indentation.left = Some(Twips((previous * step).max(0)));
            }
            ParagraphFormat::LineSpacing(line) => props.spacing.line = Some(line),
            ParagraphFormat::SpaceBefore(before) => props.spacing.before = Some(before),
            ParagraphFormat::SpaceAfter(after) => props.spacing.after = Some(after),
            ParagraphFormat::Indents {
                left,
                right,
                first_line,
            } => {
                props.indentation.left = Some(left);
                props.indentation.right = Some(right);
                props.indentation.first_line = first_line;
            }
            ParagraphFormat::AddTab(stop) => {
                change_tabs(doc, index, |tabs| {
                    tabs.retain(|t| t.position != stop.position);
                    tabs.push(stop);
                });
            }
            ParagraphFormat::MoveTab { from, to } => change_tabs(doc, index, |tabs| {
                if let Some(moved) = tabs.iter().find(|t| t.position == from).copied() {
                    tabs.retain(|t| t.position != from && t.position != to);
                    tabs.push(TabStop {
                        position: to,
                        ..moved
                    });
                }
            }),
            ParagraphFormat::RemoveTab(at) => {
                change_tabs(doc, index, |tabs| tabs.retain(|t| t.position != at));
            }
        }
    }
}

/// Меняет итоговые позиции табуляции абзаца `index` и записывает в абзац
/// только отличия от стиля: новые позиции и [`TabStopKind::Clear`] для
/// убранных позиций стиля.
fn change_tabs(doc: &mut Document, index: usize, change: impl FnOnce(&mut Vec<TabStop>)) {
    let Some(paragraph) = paragraph_ref(doc, index) else {
        return;
    };
    let mut tabs = doc.resolve_paragraph(&paragraph.props).tabs;
    change(&mut tabs);
    let mut own = paragraph.props.clone();
    own.tabs.clear();
    let inherited = doc.resolve_paragraph(&own).tabs;
    let mut direct: Vec<TabStop> = inherited
        .iter()
        .filter(|t| !tabs.iter().any(|n| n.position == t.position))
        .map(|t| TabStop {
            position: t.position,
            kind: TabStopKind::Clear,
            leader: TabLeader::None,
        })
        .chain(tabs.iter().filter(|t| !inherited.contains(t)).copied())
        .collect();
    direct.sort_by_key(|t| t.position);
    if let Some(paragraph) = paragraph_mut(doc, index) {
        paragraph.props.tabs = direct;
    }
}

/// Применяет стиль `id` (галерея «Стили» на ленте): стиль абзаца —
/// к абзацам выделенного, стиль символов — к выделенному тексту (или
/// слову под курсором). Неизвестный стиль ничего не меняет.
pub fn apply_style(doc: &mut Document, start: Position, end: Position, id: &str) {
    match doc.styles.get(id).map(|s| s.kind) {
        Some(StyleKind::Paragraph) => {
            let (first, last) = ordered(start, end);
            for index in first.paragraph..=last.paragraph {
                if let Some(paragraph) = paragraph_mut(doc, index) {
                    paragraph.props.style = Some(id.to_owned());
                }
            }
        }
        Some(StyleKind::Character) => {
            format_runs(doc, start, end, RunFormat::CharStyle(id.to_owned()));
        }
        _ => {}
    }
}

/// Оформление в позиции `at`, как его показывает лента Word: абзаца
/// и буквы перед курсором (в начале абзаца — первой буквы). `None`,
/// если такого абзаца нет.
pub fn formatting_at(
    doc: &Document,
    at: Position,
) -> Option<(ResolvedParagraphProps, ResolvedRunProps)> {
    let (paragraph, run) = run_at(doc, at)?;
    let default = RunProps::default();
    let run = run.unwrap_or(&default);
    Some((
        doc.resolve_paragraph(&paragraph.props),
        doc.resolve_run(&paragraph.props, run),
    ))
}

/// Язык текста в позиции `at` (`"ru-RU"`), как его показывает строка
/// состояния Word: буквы перед курсором (в начале абзаца — первой).
/// `None`, если язык нигде не указан или абзаца нет.
pub fn language_at(doc: &Document, at: Position) -> Option<String> {
    let (paragraph, run) = run_at(doc, at)?;
    let default = RunProps::default();
    doc.run_language(&paragraph.props, run.unwrap_or(&default))
}

/// Абзац в позиции `at` и оформление буквы перед ней (в начале абзаца —
/// первой буквы; `None`, если в абзаце нет фрагментов).
fn run_at(doc: &Document, at: Position) -> Option<(&letter_model::Paragraph, Option<&RunProps>)> {
    let paragraph = paragraph_ref(doc, at.paragraph)?;
    let mut pos = 0;
    let mut chosen: Option<&RunProps> = None;
    for inline in &paragraph.content {
        let Inline::Run(run) = inline else { continue };
        let len: usize = run
            .content
            .iter()
            .map(|c| match c {
                letter_model::RunContent::Text(t) => t.len(),
                letter_model::RunContent::Unknown(_) => 0,
                _ => 1,
            })
            .sum();
        if chosen.is_none() || (len > 0 && pos < at.offset) {
            chosen = Some(&run.props);
        }
        pos += len;
        if pos >= at.offset && at.offset > 0 && len > 0 {
            break;
        }
    }
    Some((paragraph, chosen))
}

fn ordered(a: Position, b: Position) -> (Position, Position) {
    if (a.paragraph, a.offset) <= (b.paragraph, b.offset) {
        (a, b)
    } else {
        (b, a)
    }
}

/// Куски абзацев, которые надо оформить.
fn pieces(doc: &Document, start: Position, end: Position) -> Vec<Piece> {
    let (start, end) = ordered(start, end);
    if (start.paragraph, start.offset) == (end.paragraph, end.offset) {
        let Some(paragraph) = paragraph_ref(doc, start.paragraph) else {
            return Vec::new();
        };
        let text = paragraph.plain_text();
        if text.is_empty() {
            return vec![(start.paragraph, 0, 0)];
        }
        // Слово, внутри которого курсор (не на его краю).
        return text
            .split_word_bound_indices()
            .find(|(i, w)| {
                *i < start.offset
                    && start.offset < i + w.len()
                    && w.chars().any(char::is_alphanumeric)
            })
            .map(|(i, w)| vec![(start.paragraph, i, i + w.len())])
            .unwrap_or_default();
    }
    (start.paragraph..=end.paragraph)
        .filter_map(|index| {
            let len = text_len(paragraph_ref(doc, index)?);
            let from = if index == start.paragraph {
                start.offset.min(len)
            } else {
                0
            };
            let to = if index == end.paragraph {
                end.offset.min(len)
            } else {
                len
            };
            // Пустой абзац внутри выделения тоже оформляется.
            (from < to || len == 0).then_some((index, from, to))
        })
        .collect()
}

/// Оформление фрагментов, задетых куском от `from` до `to`. У пустого
/// куска пустого абзаца — оформление его (пустого) фрагмента.
fn run_props_in(paragraph: &Paragraph, from: usize, to: usize) -> impl Iterator<Item = &RunProps> {
    let mut pos = 0;
    paragraph.content.iter().filter_map(move |inline| {
        let Inline::Run(run) = inline else {
            return None;
        };
        let len: usize = run
            .content
            .iter()
            .map(|c| match c {
                letter_model::RunContent::Text(t) => t.len(),
                letter_model::RunContent::Unknown(_) => 0,
                _ => 1,
            })
            .sum();
        let (a, b) = (pos, pos + len);
        pos = b;
        let touched = if from == to {
            len == 0
        } else {
            a < to && b > from
        };
        touched.then_some(&run.props)
    })
}

/// Применяет `change` к фрагментам абзаца от `from` до `to`, разрезая
/// фрагменты на краях.
fn apply(paragraph: &mut Paragraph, from: usize, to: usize, mut change: impl FnMut(&mut RunProps)) {
    if from == to {
        if !paragraph
            .content
            .iter()
            .any(|i| matches!(i, Inline::Run(_)))
        {
            paragraph.content.push(Inline::Run(Run::default()));
        }
        for inline in &mut paragraph.content {
            if let Inline::Run(run) = inline
                && run.content.is_empty()
            {
                change(&mut run.props);
            }
        }
        return;
    }
    let tail = split_content(paragraph, to);
    let mut middle = split_content(paragraph, from);
    for inline in &mut middle {
        if let Inline::Run(run) = inline
            && !run.content.is_empty()
        {
            change(&mut run.props);
        }
    }
    paragraph.content.extend(middle);
    paragraph.content.extend(tail);
    tidy(paragraph);
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::{Block, ParagraphProps, Style, StyleKind};

    fn doc(paragraphs: Vec<Paragraph>) -> Document {
        let mut doc = Document::new();
        doc.sections[0].blocks = paragraphs.into_iter().map(Block::from).collect();
        doc
    }

    fn para(doc: &Document, i: usize) -> &Paragraph {
        paragraph_ref(doc, i).unwrap()
    }

    fn para_mut(doc: &mut Document, i: usize) -> &mut Paragraph {
        paragraph_mut(doc, i).unwrap()
    }

    fn runs(doc: &Document, i: usize) -> Vec<(String, Option<bool>)> {
        para(doc, i)
            .runs()
            .map(|r| (r.plain_text(), r.props.bold))
            .collect()
    }

    #[test]
    fn bold_toggles_on_then_off() {
        let mut d = doc(vec![Paragraph::text("abcdef"), Paragraph::text("gh")]);
        format_runs(
            &mut d,
            Position::new(0, 2),
            Position::new(0, 4),
            RunFormat::Bold,
        );
        assert_eq!(
            runs(&d, 0),
            vec![
                ("ab".into(), None),
                ("cd".into(), Some(true)),
                ("ef".into(), None)
            ]
        );
        // Выделение шире полужирного — включается для всего.
        format_runs(
            &mut d,
            Position::new(1, 1),
            Position::new(0, 3),
            RunFormat::Bold,
        );
        assert_eq!(
            runs(&d, 0),
            vec![("ab".into(), None), ("cdef".into(), Some(true))]
        );
        assert_eq!(
            runs(&d, 1),
            vec![("g".into(), Some(true)), ("h".into(), None)]
        );
        // Теперь всё полужирное — выключается.
        format_runs(
            &mut d,
            Position::new(0, 2),
            Position::new(1, 1),
            RunFormat::Bold,
        );
        assert_eq!(
            runs(&d, 0),
            vec![("ab".into(), None), ("cdef".into(), Some(false))]
        );
        assert_eq!(para(&d, 0).plain_text(), "abcdef");
    }

    #[test]
    fn style_bold_counts_as_bold() {
        let mut strong = Style::new("Head", StyleKind::Paragraph);
        strong.run.bold = Some(true);
        let mut d = doc(vec![Paragraph::text("title").with_props(ParagraphProps {
            style: Some("Head".into()),
            ..ParagraphProps::default()
        })]);
        d.styles.styles.push(strong);
        format_runs(
            &mut d,
            Position::new(0, 0),
            Position::new(0, 5),
            RunFormat::Bold,
        );
        assert_eq!(runs(&d, 0), vec![("title".into(), Some(false))]);
    }

    #[test]
    fn cursor_inside_a_word_formats_the_word() {
        let mut d = doc(vec![Paragraph::text("one two"), Paragraph::default()]);
        format_runs(
            &mut d,
            Position::new(0, 5),
            Position::new(0, 5),
            RunFormat::Italic,
        );
        let italic: Vec<_> = para(&d, 0)
            .runs()
            .map(|r| (r.plain_text(), r.props.italic))
            .collect();
        assert_eq!(
            italic,
            vec![("one ".into(), None), ("two".into(), Some(true))]
        );
        // На краю слова — ничего.
        let before = d.clone();
        format_runs(
            &mut d,
            Position::new(0, 3),
            Position::new(0, 3),
            RunFormat::Italic,
        );
        assert_eq!(d, before);
        // Пустой абзац запоминает оформление для набора.
        format_runs(
            &mut d,
            Position::new(1, 0),
            Position::new(1, 0),
            RunFormat::Underline,
        );
        crate::edit::insert_text(&mut d, Position::new(1, 0), "x");
        assert_eq!(
            para(&d, 1).runs().next().map(|r| r.props.underline),
            Some(Some(Underline::Single))
        );
    }

    #[test]
    fn font_size_steps_by_a_point() {
        let mut d = doc(vec![Paragraph::from_runs([
            Run::text("a"),
            Run::text("b").with_props(RunProps {
                size: Some(HalfPoints(3)),
                ..RunProps::default()
            }),
        ])]);
        let base = d
            .resolve_run(&ParagraphProps::default(), &RunProps::default())
            .size
            .0;
        format_runs(
            &mut d,
            Position::new(0, 0),
            Position::new(0, 2),
            RunFormat::Grow,
        );
        let sizes: Vec<_> = para(&d, 0).runs().map(|r| r.props.size).collect();
        assert_eq!(sizes, vec![Some(HalfPoints(base + 2)), Some(HalfPoints(5))]);
        for _ in 0..3 {
            format_runs(
                &mut d,
                Position::new(0, 0),
                Position::new(0, 2),
                RunFormat::Shrink,
            );
        }
        let sizes: Vec<_> = para(&d, 0).runs().map(|r| r.props.size).collect();
        assert_eq!(
            sizes,
            vec![Some(HalfPoints(base - 4)), Some(HalfPoints(MIN_SIZE))]
        );
    }

    #[test]
    fn formatting_is_read_before_the_cursor() {
        let mut d = doc(vec![Paragraph::from_runs([
            Run::text("ab"),
            Run::text("cd").with_props(RunProps {
                bold: Some(true),
                ..RunProps::default()
            }),
        ])]);
        let bold = |d: &Document, o| formatting_at(d, Position::new(0, o)).map(|(_, r)| r.bold);
        assert_eq!(bold(&d, 0), Some(false));
        assert_eq!(bold(&d, 2), Some(false));
        assert_eq!(bold(&d, 3), Some(true));
        assert_eq!(bold(&d, 4), Some(true));
        assert_eq!(bold(&d, 9), Some(true));
        align_paragraphs(
            &mut d,
            Position::new(0, 0),
            Position::new(0, 0),
            Alignment::Right,
        );
        let (paragraph, _) = formatting_at(&d, Position::new(0, 1)).unwrap();
        assert_eq!(paragraph.alignment, Alignment::Right);
        assert!(formatting_at(&d, Position::new(5, 0)).is_none());
        // Первая буква, если курсор в начале абзаца.
        let d = doc(vec![Paragraph::from_runs([Run::text("x").with_props(
            RunProps {
                italic: Some(true),
                ..RunProps::default()
            },
        )])]);
        let (_, run) = formatting_at(&d, Position::new(0, 0)).unwrap();
        assert!(run.italic);
    }

    #[test]
    fn language_is_read_before_the_cursor() {
        let english = RunProps {
            unknown: vec![letter_model::RawXml::new(r#"<w:lang w:val="en-US"/>"#)],
            ..RunProps::default()
        };
        let mut d = doc(vec![Paragraph::from_runs([
            Run::text("да"),
            Run::text("yes").with_props(english),
        ])]);
        d.styles.defaults.run.unknown =
            vec![letter_model::RawXml::new(r#"<w:lang w:val="ru-RU"/>"#)];
        let at = |o| language_at(&d, Position::new(0, o));
        assert_eq!(at(0).as_deref(), Some("ru-RU"));
        assert_eq!(at(4).as_deref(), Some("ru-RU"));
        assert_eq!(at(5).as_deref(), Some("en-US"));
        assert_eq!(language_at(&d, Position::new(3, 0)), None);
    }

    #[test]
    fn strike_indexes_and_values() {
        let mut d = doc(vec![Paragraph::text("abcd")]);
        let all = |d: &mut Document, f: RunFormat| {
            format_runs(d, Position::new(0, 0), Position::new(0, 4), f);
        };
        let props = |d: &Document| para(d, 0).runs().next().unwrap().props.clone();
        all(&mut d, RunFormat::Strike);
        assert_eq!(props(&d).strike, Some(true));
        all(&mut d, RunFormat::Strike);
        assert_eq!(props(&d).strike, Some(false));
        // Подстрочный сменяется надстрочным, повтор возвращает обычный.
        all(&mut d, RunFormat::Subscript);
        assert_eq!(props(&d).vertical_align, Some(VerticalAlign::Subscript));
        all(&mut d, RunFormat::Superscript);
        assert_eq!(props(&d).vertical_align, Some(VerticalAlign::Superscript));
        all(&mut d, RunFormat::Superscript);
        assert_eq!(props(&d).vertical_align, Some(VerticalAlign::Baseline));
        all(&mut d, RunFormat::Font("Arial".into()));
        all(&mut d, RunFormat::Size(HalfPoints(28)));
        all(&mut d, RunFormat::Color(Color::Rgb(255, 0, 0)));
        all(&mut d, RunFormat::Highlight(Highlight::Yellow));
        let p = props(&d);
        assert_eq!(p.fonts.ascii.as_deref(), Some("Arial"));
        assert_eq!(p.fonts.h_ansi.as_deref(), Some("Arial"));
        assert_eq!(p.size, Some(HalfPoints(28)));
        assert_eq!(p.color, Some(Color::Rgb(255, 0, 0)));
        assert_eq!(p.highlight, Some(Highlight::Yellow));
        let resolved = d.resolve_run(&ParagraphProps::default(), &p);
        assert_eq!(resolved.fonts.ascii, "Arial");
        // Очистка возвращает оформление по умолчанию.
        all(&mut d, RunFormat::Bold);
        all(&mut d, RunFormat::Clear);
        assert_eq!(props(&d), RunProps::default());
    }

    #[test]
    fn ruler_indents_and_tabs() {
        let mut d = doc(vec![Paragraph::text("a"), Paragraph::text("b")]);
        let stop = |at: i32, kind| TabStop {
            position: Twips(at),
            kind,
            leader: TabLeader::None,
        };
        // Стиль абзаца даёт позицию 1000.
        let mut normal = letter_model::Style::new("Normal", StyleKind::Paragraph);
        normal.is_default = true;
        normal.paragraph.tabs = vec![stop(1000, TabStopKind::Left)];
        d.styles.styles.push(normal);
        let run = |d: &mut Document, f| {
            format_paragraphs(d, Position::new(0, 0), Position::new(1, 0), f);
        };
        let tabs = |d: &Document, i| d.resolve_paragraph(&para(d, i).props).tabs;
        run(
            &mut d,
            ParagraphFormat::Indents {
                left: Twips(720),
                right: Twips(360),
                first_line: Some(FirstLineIndent::Hanging(Twips(360))),
            },
        );
        let resolved = d.resolve_paragraph(&para(&d, 1).props);
        assert_eq!(
            (
                resolved.indent_left,
                resolved.indent_right,
                resolved.first_line
            ),
            (
                Twips(720),
                Twips(360),
                Some(FirstLineIndent::Hanging(Twips(360)))
            )
        );
        run(
            &mut d,
            ParagraphFormat::AddTab(stop(2000, TabStopKind::Center)),
        );
        assert_eq!(
            tabs(&d, 0),
            [
                stop(1000, TabStopKind::Left),
                stop(2000, TabStopKind::Center)
            ]
        );
        // В абзаце — только новая позиция, позиция стиля не повторяется.
        assert_eq!(para(&d, 0).props.tabs, [stop(2000, TabStopKind::Center)]);
        run(
            &mut d,
            ParagraphFormat::MoveTab {
                from: Twips(1000),
                to: Twips(1500),
            },
        );
        assert_eq!(
            tabs(&d, 1),
            [
                stop(1500, TabStopKind::Left),
                stop(2000, TabStopKind::Center)
            ]
        );
        // Позиция стиля убрана знаком «очистить».
        assert!(
            para(&d, 1)
                .props
                .tabs
                .contains(&stop(1000, TabStopKind::Clear))
        );
        run(&mut d, ParagraphFormat::RemoveTab(Twips(2000)));
        run(&mut d, ParagraphFormat::RemoveTab(Twips(1500)));
        assert!(tabs(&d, 0).is_empty());
        // Возврат позиции стиля снимает «очистить».
        run(
            &mut d,
            ParagraphFormat::AddTab(stop(1000, TabStopKind::Left)),
        );
        assert!(para(&d, 0).props.tabs.is_empty());
        // Перенос несуществующей позиции ничего не меняет.
        run(
            &mut d,
            ParagraphFormat::MoveTab {
                from: Twips(7),
                to: Twips(8),
            },
        );
        assert_eq!(tabs(&d, 0), [stop(1000, TabStopKind::Left)]);
    }

    #[test]
    fn indents_step_by_the_default_tab() {
        let mut d = doc(vec![Paragraph::text("a"), Paragraph::text("b")]);
        d.settings.default_tab_stop = Twips(709);
        let left = |d: &Document, i| para(d, i).props.indentation.left;
        let run = |d: &mut Document, f| {
            format_paragraphs(d, Position::new(0, 0), Position::new(1, 0), f);
        };
        run(&mut d, ParagraphFormat::IndentMore);
        assert_eq!(left(&d, 0), Some(Twips(709)));
        para_mut(&mut d, 1).props.indentation.left = Some(Twips(1000));
        run(&mut d, ParagraphFormat::IndentMore);
        assert_eq!(
            (left(&d, 0), left(&d, 1)),
            (Some(Twips(1418)), Some(Twips(1418)))
        );
        para_mut(&mut d, 1).props.indentation.left = Some(Twips(1000));
        run(&mut d, ParagraphFormat::IndentLess);
        assert_eq!(
            (left(&d, 0), left(&d, 1)),
            (Some(Twips(709)), Some(Twips(709)))
        );
        run(&mut d, ParagraphFormat::IndentLess);
        run(&mut d, ParagraphFormat::IndentLess);
        assert_eq!(left(&d, 0), Some(Twips(0)));
        run(
            &mut d,
            ParagraphFormat::LineSpacing(LineSpacing::ONE_AND_HALF),
        );
        run(&mut d, ParagraphFormat::SpaceBefore(Twips(240)));
        run(&mut d, ParagraphFormat::SpaceAfter(Twips(0)));
        let resolved = d.resolve_paragraph(&para(&d, 1).props);
        assert_eq!(resolved.line_spacing, LineSpacing::ONE_AND_HALF);
        assert_eq!(
            (resolved.space_before, resolved.space_after),
            (Twips(240), Twips(0))
        );
    }

    #[test]
    fn styles_apply_to_paragraphs_or_text() {
        let mut d = doc(vec![Paragraph::text("one two"), Paragraph::text("b")]);
        let mut strong = letter_model::Style::new("Strong", StyleKind::Character);
        strong.run.bold = Some(true);
        d.styles.styles.push(strong);
        d.styles
            .styles
            .push(letter_model::Style::new("Heading1", StyleKind::Paragraph));
        apply_style(&mut d, Position::new(0, 1), Position::new(1, 0), "Heading1");
        assert_eq!(para(&d, 0).props.style.as_deref(), Some("Heading1"));
        assert_eq!(para(&d, 1).props.style.as_deref(), Some("Heading1"));
        apply_style(&mut d, Position::new(0, 5), Position::new(0, 5), "Strong");
        let styles: Vec<_> = para(&d, 0)
            .runs()
            .map(|r| (r.plain_text(), r.props.style.clone()))
            .collect();
        assert_eq!(
            styles,
            vec![("one ".into(), None), ("two".into(), Some("Strong".into()))]
        );
        let before = d.clone();
        apply_style(&mut d, Position::new(0, 0), Position::new(1, 0), "Missing");
        assert_eq!(d, before);
    }

    #[test]
    fn alignment_toggles_back_to_left() {
        let mut d = doc(vec![Paragraph::text("a"), Paragraph::text("b")]);
        let align = |d: &Document, i| para(d, i).props.alignment;
        align_paragraphs(
            &mut d,
            Position::new(0, 0),
            Position::new(1, 0),
            Alignment::Center,
        );
        assert_eq!(align(&d, 0), Some(Alignment::Center));
        assert_eq!(align(&d, 1), Some(Alignment::Center));
        align_paragraphs(
            &mut d,
            Position::new(1, 0),
            Position::new(1, 0),
            Alignment::Center,
        );
        assert_eq!(align(&d, 1), Some(Alignment::Left));
        assert_eq!(align(&d, 0), Some(Alignment::Center));
        align_paragraphs(
            &mut d,
            Position::new(0, 0),
            Position::new(0, 0),
            Alignment::Left,
        );
        assert_eq!(align(&d, 0), Some(Alignment::Left));
    }
}
