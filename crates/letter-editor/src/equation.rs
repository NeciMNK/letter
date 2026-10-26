//! Формулы в редакторе: вставка («Вставка → Уравнение») и замена
//! формулы новой (вкладка «Уравнение»).
//!
//! Формула стоит в тексте одним знаком [`letter_model::OBJECT_CHAR`];
//! выделить её — выделить этот знак.

use letter_model::{Document, Inline, Math, OBJECT_CHAR, RunContent};

use crate::edit::{clamp_offset, insert_items, paragraph_mut, paragraph_ref, text_len};
use crate::position::Position;

/// Формула в позиции `at` (знак формулы сразу за `at`).
pub fn math_ref(doc: &Document, at: Position) -> Option<&Math> {
    let paragraph = paragraph_ref(doc, at.paragraph)?;
    let mut pos = 0;
    for run in paragraph.runs() {
        for item in &run.content {
            if pos == at.offset
                && let RunContent::Math(math) = item
            {
                return Some(math);
            }
            pos += item.text_len();
        }
    }
    None
}

fn math_mut(doc: &mut Document, at: Position) -> Option<&mut Math> {
    let paragraph = paragraph_mut(doc, at.paragraph)?;
    let mut pos = 0;
    for inline in &mut paragraph.content {
        let Inline::Run(run) = inline else { continue };
        for item in &mut run.content {
            let len = item.text_len();
            if pos == at.offset
                && let RunContent::Math(math) = item
            {
                return Some(math);
            }
            pos += len;
        }
    }
    None
}

/// Новая формула для вставки в позицию `at`: в пустом абзаце —
/// отдельной строкой, как в Word, иначе — в строке текста.
pub fn new_math(doc: &Document, at: Position, linear: &str) -> Math {
    let empty = paragraph_ref(doc, at.paragraph).is_some_and(|p| text_len(p) == 0);
    Math::from_linear(linear, empty)
}

/// Вставляет формулу в позицию `at`. Возвращает позицию за ней.
pub fn insert_math(doc: &mut Document, at: Position, math: Math) -> Position {
    let Some(paragraph) = paragraph_mut(doc, at.paragraph) else {
        return at;
    };
    let offset = clamp_offset(paragraph, at.offset);
    insert_items(paragraph, offset, vec![RunContent::Math(math)]);
    Position::new(at.paragraph, offset + OBJECT_CHAR.len_utf8())
}

/// Заменяет формулу в позиции `at` формулой `math` (исходный XML
/// сбрасывается, чтобы сохранилась новая). `false`, если там нет формулы.
pub fn set_math(doc: &mut Document, at: Position, mut math: Math) -> bool {
    let Some(old) = math_mut(doc, at) else {
        return false;
    };
    math.raw = None;
    *old = math;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::{MathNode, Paragraph};

    #[test]
    fn insert_and_replace() {
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![Paragraph::default().into()];
        let at = Position::new(0, 0);
        let math = new_math(&doc, at, "x^2");
        assert!(math.display);
        let end = insert_math(&mut doc, at, math);
        assert_eq!(end, Position::new(0, OBJECT_CHAR.len_utf8()));
        assert_eq!(
            math_ref(&doc, at).map(Math::linear_text).as_deref(),
            Some("x^2")
        );
        // В абзаце с текстом — формула в строке.
        assert!(!new_math(&doc, end, "y").display);
        let mut next = Math::from_linear("a/b", true);
        next.raw = Some(letter_model::RawXml::new("<m:oMath/>"));
        assert!(set_math(&mut doc, at, next));
        let math = math_ref(&doc, at).expect("формула на месте");
        assert!(math.raw.is_none());
        assert!(matches!(math.lines[0][..], [MathNode::Fraction { .. }]));
        assert!(!set_math(&mut doc, end, Math::default()));
        assert!(math_ref(&doc, end).is_none());
    }

    #[test]
    fn edits_undo_as_one_step() {
        use crate::{Command, History, Selection};
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![Paragraph::default().into()];
        let mut history = History::default();
        let at = Position::new(0, 0);
        let math = Box::new(Math::from_linear("x", true));
        let end = history.run(&mut doc, Selection::new(at, at), Command::InsertMath(math));
        let picked = Selection::new(at, end.focus);
        for text in ["x+", "x+1", "x+1/2"] {
            let math = Box::new(Math::from_linear(text, true));
            assert_eq!(
                history.run(&mut doc, picked, Command::SetMath(math)),
                picked
            );
        }
        let linear = |doc: &Document| math_ref(doc, at).map(Math::linear_text);
        assert_eq!(linear(&doc).as_deref(), Some("x+1/2"));
        history.undo(&mut doc);
        assert_eq!(linear(&doc).as_deref(), Some("x"));
        history.undo(&mut doc);
        assert_eq!(linear(&doc), None);
    }
}
