//! Оформление выделенного: полужирный, курсив, подчёркивание, размер
//! шрифта (Ctrl+B, Ctrl+I, Ctrl+U, Ctrl+] и Ctrl+[) и выравнивание
//! абзацев (Ctrl+L, Ctrl+E, Ctrl+R, Ctrl+J).

use letter_model::{Alignment, Document, HalfPoints, Inline, Paragraph, Run, RunProps, Underline};
use unicode_segmentation::UnicodeSegmentation;

use crate::edit::{paragraph_mut, paragraph_ref, split_content, text_len, tidy};
use crate::position::Position;

/// Что поменять в оформлении символов.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
                RunFormat::Grow | RunFormat::Shrink => true,
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
        apply(paragraph, from, to, |props| match format {
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
