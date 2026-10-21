//! Команды правки и их отмена и повтор (Ctrl+Z, Ctrl+Y).
//!
//! Каждая правка — объект [`Command`]. [`History::run`] выполняет её
//! и запоминает, что изменилось: блоки верхнего уровня (абзацы и
//! таблицы разделов), которые правка могла задеть, до и после. Отмена
//! ставит прежние блоки на место — так не нужно писать «обратную»
//! операцию к каждой правке, а память тратится только на задетые абзацы.
//! Набор текста объединяется в один шаг по словам, как в Word.

use letter_model::{Block, Document, Section};

use crate::edit::{
    delete_backward, delete_forward, delete_range, insert_line_break, insert_text, split_paragraph,
    top_level,
};
use crate::selection::Selection;

/// Правка документа.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Набрать текст (заменяет выделенное).
    Type(String),
    /// Enter: новый абзац.
    SplitParagraph,
    /// Shift+Enter: разрыв строки.
    LineBreak,
    /// Backspace.
    DeleteBackward,
    /// Delete.
    DeleteForward,
}

/// Сколько шагов помнить.
const LIMIT: usize = 1000;

/// Что заменить при отмене или повторе.
#[derive(Debug, Clone, PartialEq)]
enum Patch {
    /// Блоки одного раздела, начиная с `start`.
    Blocks {
        section: usize,
        start: usize,
        before: Vec<Block>,
        after: Vec<Block>,
    },
    /// Разделы целиком, начиная с `start` (правка через границу разделов).
    Sections {
        start: usize,
        before: Vec<Section>,
        after: Vec<Section>,
    },
}

impl Patch {
    /// Ставит на место `before` (`undo`) или `after`.
    fn apply(&self, doc: &mut Document, undo: bool) {
        match self {
            Patch::Blocks {
                section,
                start,
                before,
                after,
            } => {
                let (remove, insert) = if undo {
                    (after, before)
                } else {
                    (before, after)
                };
                if let Some(section) = doc.sections.get_mut(*section) {
                    let end = (start + remove.len()).min(section.blocks.len());
                    let start = (*start).min(end);
                    section.blocks.splice(start..end, insert.iter().cloned());
                }
            }
            Patch::Sections {
                start,
                before,
                after,
            } => {
                let (remove, insert) = if undo {
                    (after, before)
                } else {
                    (before, after)
                };
                let end = (start + remove.len()).min(doc.sections.len());
                let start = (*start).min(end);
                doc.sections.splice(start..end, insert.iter().cloned());
            }
        }
    }
}

/// Один шаг истории.
#[derive(Debug, Clone, PartialEq)]
struct Step {
    patch: Patch,
    /// Выделение до и после шага.
    before: Selection,
    after: Selection,
    /// Набранный текст, если шаг — набор (для объединения по словам).
    typed: Option<String>,
}

/// Отмена и повтор.
#[derive(Debug, Clone, Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// Следующий набор начнёт новый шаг (курсор двигали и т. п.).
    sealed: bool,
}

impl History {
    /// Пустая история.
    pub fn new() -> History {
        History::default()
    }

    /// Выполняет `command` над выделением `selection` и возвращает новое
    /// выделение (пустое — курсор).
    pub fn run(&mut self, doc: &mut Document, selection: Selection, command: Command) -> Selection {
        let (start, end) = (selection.start(), selection.end());
        // Какие абзацы правка может задеть: Backspace и Delete на краю
        // абзаца соединяют его с соседним.
        let erasing = !selection.is_empty();
        let (first, last) = match command {
            Command::DeleteBackward if !erasing => {
                (start.paragraph.saturating_sub(1), end.paragraph)
            }
            Command::DeleteForward if !erasing => (start.paragraph, end.paragraph + 1),
            _ => (start.paragraph, end.paragraph),
        };
        let from = top_level(doc, first).or_else(|| top_level(doc, start.paragraph));
        let to = top_level(doc, last).or_else(|| top_level(doc, end.paragraph));
        let (Some(from), Some(to)) = (from, to) else {
            return selection;
        };
        let sections_before = doc.sections.len();
        let blocks_before = doc.sections.get(to.0).map_or(0, |s| s.blocks.len());
        let saved = if from.0 == to.0 {
            Saved::Blocks(doc.sections[from.0].blocks[from.1..=to.1].to_vec())
        } else {
            Saved::Sections(doc.sections[from.0..=to.0].to_vec())
        };

        let at = if erasing {
            delete_range(doc, start, end)
        } else {
            selection.focus
        };
        let focus = match &command {
            Command::Type(text) => insert_text(doc, at, text),
            Command::SplitParagraph => split_paragraph(doc, at),
            Command::LineBreak => insert_line_break(doc, at),
            Command::DeleteBackward | Command::DeleteForward if erasing => at,
            Command::DeleteBackward => delete_backward(doc, at),
            Command::DeleteForward => delete_forward(doc, at),
        };
        let after = Selection::caret(focus);

        let patch = match saved {
            Saved::Blocks(before) => {
                let blocks = &doc.sections[from.0].blocks;
                let len = (to.1 + 1 + blocks.len()).saturating_sub(blocks_before);
                let len = len.saturating_sub(from.1).min(blocks.len() - from.1);
                Patch::Blocks {
                    section: from.0,
                    start: from.1,
                    before,
                    after: blocks[from.1..from.1 + len].to_vec(),
                }
            }
            Saved::Sections(before) => {
                let count = (to.0 + 1 + doc.sections.len()).saturating_sub(sections_before);
                let count = count
                    .saturating_sub(from.0)
                    .min(doc.sections.len() - from.0);
                Patch::Sections {
                    start: from.0,
                    before,
                    after: doc.sections[from.0..from.0 + count].to_vec(),
                }
            }
        };
        let unchanged = match &patch {
            Patch::Blocks { before, after, .. } => before == after,
            Patch::Sections { before, after, .. } => before == after,
        };
        if unchanged {
            return after;
        }
        let typed = match &command {
            Command::Type(text) => Some(text.clone()),
            _ => None,
        };
        self.redo.clear();
        let step = Step {
            patch,
            before: selection,
            after,
            typed,
        };
        if !std::mem::take(&mut self.sealed)
            && let Some(last) = self.undo.last_mut()
            && joins(last, &step)
        {
            if let (
                Patch::Blocks { after, .. },
                Patch::Blocks {
                    after: new_after, ..
                },
            ) = (&mut last.patch, step.patch)
            {
                *after = new_after;
            }
            last.after = step.after;
            if let (Some(a), Some(b)) = (&mut last.typed, step.typed) {
                a.push_str(&b);
            }
            return after;
        }
        self.undo.push(step);
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
        after
    }

    /// Следующий набор текста начнёт новый шаг отмены. Вызывать, когда
    /// курсор переставили (клавишами, мышью).
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// Ctrl+Z: отменяет последний шаг и возвращает выделение до него.
    /// `None`, если отменять нечего.
    pub fn undo(&mut self, doc: &mut Document) -> Option<Selection> {
        let step = self.undo.pop()?;
        step.patch.apply(doc, true);
        let selection = step.before;
        self.redo.push(step);
        self.sealed = true;
        Some(selection)
    }

    /// Ctrl+Y: повторяет отменённый шаг и возвращает выделение после него.
    pub fn redo(&mut self, doc: &mut Document) -> Option<Selection> {
        let step = self.redo.pop()?;
        step.patch.apply(doc, false);
        let selection = step.after;
        self.undo.push(step);
        self.sealed = true;
        Some(selection)
    }

    /// Есть ли что отменить.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Есть ли что повторить.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

enum Saved {
    Blocks(Vec<Block>),
    Sections(Vec<Section>),
}

/// Продолжает ли набор `next` прошлый шаг: оба — набор без выделения
/// в одних и тех же блоках, курсор с тех пор не двигали, и это не
/// начало нового слова (Word отменяет набранное по словам).
fn joins(last: &Step, next: &Step) -> bool {
    let (Some(previous), Some(typed)) = (&last.typed, &next.typed) else {
        return false;
    };
    let same_place = match (&last.patch, &next.patch) {
        (
            Patch::Blocks {
                section: s1,
                start: b1,
                after,
                ..
            },
            Patch::Blocks {
                section: s2,
                start: b2,
                before,
                ..
            },
        ) => s1 == s2 && b1 == b2 && after == before,
        _ => false,
    };
    let new_word =
        previous.ends_with(char::is_whitespace) && !typed.starts_with(char::is_whitespace);
    same_place && next.before.is_empty() && last.after == next.before && !new_word
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::{Paragraph, Table, TableCell, TableRow};

    use crate::edit::paragraph_mut;
    use crate::position::Position;

    fn doc(texts: &[&str]) -> Document {
        let mut doc = Document::new();
        doc.sections[0].blocks = texts.iter().map(|t| Paragraph::text(*t).into()).collect();
        doc
    }

    fn texts(doc: &Document) -> Vec<String> {
        let mut doc = doc.clone();
        (0..)
            .map_while(|i| paragraph_mut(&mut doc, i).map(|p| p.plain_text()))
            .collect()
    }

    fn caret(p: usize, o: usize) -> Selection {
        Selection::caret(Position::new(p, o))
    }

    #[test]
    fn typing_is_undone_by_words() {
        let mut d = doc(&[""]);
        let mut h = History::new();
        let mut s = caret(0, 0);
        for c in ["h", "i", " ", "y", "o", "u"] {
            s = h.run(&mut d, s, Command::Type(c.into()));
        }
        assert_eq!(texts(&d), vec!["hi you"]);
        assert_eq!(h.undo(&mut d), Some(caret(0, 3)));
        assert_eq!(texts(&d), vec!["hi "]);
        assert_eq!(h.undo(&mut d), Some(caret(0, 0)));
        assert_eq!(texts(&d), vec![""]);
        assert_eq!(h.undo(&mut d), None);
        assert_eq!(h.redo(&mut d), Some(caret(0, 3)));
        assert_eq!(h.redo(&mut d), Some(caret(0, 6)));
        assert_eq!(texts(&d), vec!["hi you"]);
        assert!(!h.can_redo());
    }

    #[test]
    fn moving_the_cursor_starts_a_new_step() {
        let mut d = doc(&["ab"]);
        let mut h = History::new();
        h.run(&mut d, caret(0, 2), Command::Type("c".into()));
        h.seal();
        h.run(&mut d, caret(0, 0), Command::Type("x".into()));
        assert_eq!(texts(&d), vec!["xabc"]);
        h.undo(&mut d);
        assert_eq!(texts(&d), vec!["abc"]);
        // Новая правка после отмены забывает повтор.
        h.run(&mut d, caret(0, 0), Command::Type("y".into()));
        assert!(!h.can_redo());
        assert_eq!(texts(&d), vec!["yabc"]);
    }

    #[test]
    fn enter_backspace_delete_and_selections() {
        let mut d = doc(&["one", "two", "three"]);
        let mut h = History::new();
        let s = h.run(&mut d, caret(0, 1), Command::SplitParagraph);
        assert_eq!(s, caret(1, 0));
        let s = h.run(&mut d, s, Command::DeleteBackward);
        assert_eq!(s, caret(0, 1));
        h.run(&mut d, caret(1, 3), Command::DeleteForward);
        assert_eq!(texts(&d), vec!["one", "twothree"]);
        let sel = Selection::new(Position::new(1, 2), Position::new(0, 1));
        let s = h.run(&mut d, sel, Command::Type("Z".into()));
        assert_eq!(texts(&d), vec!["oZothree"]);
        assert_eq!(s, caret(0, 2));
        // Отмена по одному шагу назад до начала.
        assert_eq!(h.undo(&mut d), Some(sel));
        assert_eq!(texts(&d), vec!["one", "twothree"]);
        h.undo(&mut d);
        assert_eq!(texts(&d), vec!["one", "two", "three"]);
        h.undo(&mut d);
        assert_eq!(texts(&d), vec!["o", "ne", "two", "three"]);
        h.undo(&mut d);
        assert_eq!(texts(&d), vec!["one", "two", "three"]);
        assert!(!h.can_undo());
        while h.redo(&mut d).is_some() {}
        assert_eq!(texts(&d), vec!["oZothree"]);
        // Ничего не изменилось — шага нет.
        let mut h = History::new();
        h.run(&mut d, caret(0, 0), Command::DeleteBackward);
        assert!(!h.can_undo());
    }

    #[test]
    fn tables_and_sections() {
        let table = Table {
            raw_props: Vec::new(),
            rows: vec![TableRow {
                raw_props: Vec::new(),
                cells: vec![TableCell::text("A"), TableCell::text("B")],
            }],
        };
        let mut d = Document::new();
        d.sections[0].blocks = vec![Paragraph::text("x").into(), table.into()];
        let mut second = d.sections[0].clone();
        second.blocks = vec![Paragraph::text("next").into()];
        d.sections.push(second);
        let original = d.clone();
        let mut h = History::new();
        // В ячейке.
        h.run(&mut d, caret(2, 1), Command::Type("!".into()));
        assert_eq!(texts(&d), vec!["x", "A", "B!", "next"]);
        // Через границу разделов.
        let sel = Selection::new(Position::new(0, 0), Position::new(3, 2));
        h.run(&mut d, sel, Command::DeleteForward);
        assert_eq!(texts(&d), vec!["", "", "", "xt"]);
        h.undo(&mut d);
        h.undo(&mut d);
        assert_eq!(d, original);
    }
}
