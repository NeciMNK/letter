//! Разница в наследовании «переключателей» (полужирный, курсив,
//! зачёркивание) между Word и ODF.
//!
//! В Word полужирный стиль символов внутри полужирного стиля абзаца
//! *отменяет* полужирность, а в ODF значение просто перекрывается: текст
//! остаётся полужирным. Где правила дают разный ответ, значение
//! записывается во фрагмент явно, и тогда оно одинаково в обоих мирах.

use letter_model::{Document, ParagraphProps, RunProps, StyleKind};

/// Сколько уровней `basedOn` проходить (защита от циклов).
const MAX_DEPTH: usize = 64;

type Get = fn(&RunProps) -> Option<bool>;
type Set = fn(&mut RunProps, bool);

const TOGGLES: [(Get, Set); 4] = [
    (|r| r.bold, |r, v| r.bold = Some(v)),
    (|r| r.italic, |r, v| r.italic = Some(v)),
    (|r| r.strike, |r, v| r.strike = Some(v)),
    (|r| r.double_strike, |r, v| r.double_strike = Some(v)),
];

/// Для переключателей, не заданных во фрагменте явно, где Word и ODF
/// расходятся, записывает явное значение по правилам Word (перед
/// записью в ODF).
pub(crate) fn fix(doc: &Document, paragraph: &ParagraphProps, run: &mut RunProps) {
    fix_with(doc, paragraph, run, false);
}

/// То же, но верными считаются правила ODF.
pub(crate) fn fix_from_odf(doc: &Document, paragraph: &ParagraphProps, run: &mut RunProps) {
    fix_with(doc, paragraph, run, true);
}

fn fix_with(doc: &Document, paragraph: &ParagraphProps, run: &mut RunProps, odf_wins: bool) {
    if run.style.is_none() {
        return;
    }
    let para_style = doc
        .resolve_paragraph(paragraph)
        .style
        .and_then(|id| doc.styles.get(&id));
    let char_style = run
        .style
        .as_deref()
        .and_then(|id| doc.styles.get(id))
        .filter(|s| s.kind == StyleKind::Character);
    let Some(char_style) = char_style else {
        return;
    };
    let word = doc.resolve_run(paragraph, run);
    let words = [word.bold, word.italic, word.strike, word.double_strike];
    for ((get, set), word_value) in TOGGLES.into_iter().zip(words) {
        if get(run).is_some() {
            continue;
        }
        let odf_value = chain_value(doc, Some(char_style.id.as_str()), get)
            .or_else(|| chain_value(doc, para_style.map(|s| s.id.as_str()), get))
            .or_else(|| get(&doc.styles.defaults.run))
            .unwrap_or(false);
        if odf_value != word_value {
            set(run, if odf_wins { odf_value } else { word_value });
        }
    }
}

/// Значение из стиля или ближайшего предка, где оно задано.
fn chain_value<'a>(doc: &'a Document, mut id: Option<&'a str>, get: Get) -> Option<bool> {
    for _ in 0..MAX_DEPTH {
        let style = doc.styles.get(id?)?;
        if let Some(value) = get(&style.run) {
            return Some(value);
        }
        id = style.based_on.as_deref();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::Style;

    fn doc() -> Document {
        let mut doc = Document::new();
        let mut heading = Style::new("Heading", StyleKind::Paragraph);
        heading.run.bold = Some(true);
        let mut strong = Style::new("Strong", StyleKind::Character);
        strong.run.bold = Some(true);
        doc.styles.styles = vec![heading, strong];
        doc
    }

    #[test]
    fn bold_in_bold_differs() {
        let doc = doc();
        let para = ParagraphProps {
            style: Some("Heading".into()),
            ..ParagraphProps::default()
        };
        let plain = RunProps {
            style: Some("Strong".into()),
            ..RunProps::default()
        };
        // По Word текст обычный: при записи в ODF это надо сказать явно.
        let mut run = plain.clone();
        fix(&doc, &para, &mut run);
        assert_eq!(run.bold, Some(false));
        // По ODF текст полужирный: при чтении это записывается явно.
        let mut run = plain.clone();
        fix_from_odf(&doc, &para, &mut run);
        assert_eq!(run.bold, Some(true));
        assert!(doc.resolve_run(&para, &run).bold);
        // Без стиля символов правила совпадают.
        let mut run = RunProps::default();
        fix(&doc, &para, &mut run);
        assert_eq!(run, RunProps::default());
    }
}
