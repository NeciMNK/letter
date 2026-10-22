//! Команды правки и их отмена и повтор (Ctrl+Z, Ctrl+Y).
//!
//! Каждая правка — объект [`Command`]. [`History::run`] выполняет её
//! и запоминает, что изменилось: блоки верхнего уровня (абзацы и
//! таблицы разделов), которые правка могла задеть, до и после. Отмена
//! ставит прежние блоки на место — так не нужно писать «обратную»
//! операцию к каждой правке, а память тратится только на задетые абзацы.
//! Набор текста объединяется в один шаг по словам, как в Word.

use letter_model::{
    Alignment, Anchor, Block, BreakKind, Color, Document, Picture, Section, SectionStart,
};

use crate::clipboard::{insert_fragment, insert_plain_text};
use crate::edit::paragraph_count;
use crate::edit::{
    delete_backward, delete_forward, delete_range, insert_line_break, insert_text, split_paragraph,
    top_level,
};
use crate::format::{
    ParagraphFormat, RunFormat, align_paragraphs, apply_style, format_paragraphs, format_runs,
};
use crate::lists::{
    ListKind, change_list_level, continue_numbering, multilevel_list, restart_numbering,
    tab_changes_level, toggle_list,
};
use crate::picture::{insert_picture, resize_picture, set_picture_anchor};
use crate::search::{SearchOptions, clamp, replace_all};
use crate::section::{PageSetup, insert_break, insert_section_break, set_page_setup};
use crate::selection::Selection;
use crate::table::{
    BorderPreset, delete_columns, delete_rows, delete_table, in_table, insert_columns, insert_rows,
    insert_table, merge_cells, next_cell, set_cell_borders, set_cell_shading,
};

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
    /// Вставить кусок документа с оформлением (см. [`crate::clipboard`]).
    Paste(Box<Document>),
    /// Вставить простой текст: строки — абзацы.
    PasteText(String),
    /// Поменять оформление символов выделенного (Ctrl+B и т. п.).
    Format(RunFormat),
    /// Выровнять абзацы выделенного (Ctrl+L, Ctrl+E, Ctrl+R, Ctrl+J).
    Align(Alignment),
    /// Поменять отступы и интервалы абзацев выделенного.
    Paragraph(ParagraphFormat),
    /// Применить стиль с этим идентификатором.
    Style(String),
    /// Кнопка «Маркеры» или «Нумерация».
    List(ListKind),
    /// Уровень пунктов списка глубже (+1) или выше (−1).
    ListLevel(i8),
    /// Кнопка «Многоуровневый список».
    MultilevelList,
    /// «Начать заново с 1» у пункта с курсором.
    RestartNumbering,
    /// «Продолжить нумерацию» у пункта с курсором.
    ContinueNumbering,
    /// Tab (`shift` — Shift+Tab): в начале пункта списка меняет уровень,
    /// иначе Tab вставляет табуляцию, а Shift+Tab ничего не делает.
    Tab {
        /// С Shift.
        shift: bool,
    },
    /// Вставить таблицу (строк × столбцов) у курсора.
    InsertTable {
        /// Строк.
        rows: usize,
        /// Столбцов.
        columns: usize,
    },
    /// Вставить строки таблицы над или под выделенными.
    InsertRows {
        /// Под выделенными.
        below: bool,
    },
    /// Вставить столбцы таблицы слева или справа от выделенных.
    InsertColumns {
        /// Справа.
        right: bool,
    },
    /// Удалить строки таблицы с выделенным.
    DeleteRows,
    /// Удалить столбцы таблицы с выделенным.
    DeleteColumns,
    /// Удалить таблицу с курсором.
    DeleteTable,
    /// Границы выделенных ячеек (меню «Границы»).
    CellBorders(BorderPreset),
    /// Заливка выделенных ячеек; `None` — без заливки.
    CellShading(Option<Color>),
    /// Объединить выделенные ячейки.
    MergeCells,
    /// Вставить рисунок у курсора (заменяет выделенное).
    InsertPicture(Box<Picture>),
    /// Новый размер выделенного рисунка, EMU (выделение остаётся).
    ResizePicture {
        /// Ширина.
        width: i64,
        /// Высота.
        height: i64,
    },
    /// Новая привязка выделенного рисунка (обтекание, место на странице);
    /// `None` — рисунок в тексте.
    PictureAnchor(Option<Box<Anchor>>),
    /// Разрыв страницы (Ctrl+Enter) или колонки (Ctrl+Shift+Enter)
    /// у курсора (заменяет выделенное).
    Break(BreakKind),
    /// Разрыв раздела у курсора (заменяет выделенное): дальше — новый
    /// раздел, который начинается так, как сказано.
    SectionBreak(SectionStart),
    /// Ориентация, размер, поля или колонки разделов выделенного.
    PageSetup(PageSetup),
    /// «Заменить все»: найденное `query` заменить на `replacement` во
    /// всём документе. Курсор остаётся в том же абзаце (выделение
    /// снимается).
    ReplaceAll {
        /// Что искать.
        query: String,
        /// На что заменить.
        replacement: String,
        /// Параметры поиска.
        options: SearchOptions,
    },
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
    /// Номер состояния документа после шага (см. [`History::state`]).
    id: u64,
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
    /// Последний выданный номер состояния.
    last_id: u64,
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
        let command = match command {
            // В таблице Tab переходит в следующую ячейку, а в последней
            // добавляет строку.
            Command::Tab { shift } if in_table(doc, selection.focus.paragraph) => {
                match next_cell(doc, selection.focus.paragraph, shift) {
                    Some(next) => return next,
                    None if !shift => Command::InsertRows { below: true },
                    None => return selection,
                }
            }
            Command::Tab { shift } if tab_changes_level(doc, start, end) => {
                Command::ListLevel(if shift { -1 } else { 1 })
            }
            Command::Tab { shift: true } => return selection,
            Command::Tab { shift: false } => Command::Type("\t".into()),
            command => command,
        };
        // Какие абзацы правка может задеть: Backspace и Delete на краю
        // абзаца соединяют его с соседним.
        // Оформление меняет выделенное, а не заменяет его.
        let formatting = matches!(
            command,
            Command::Format(_)
                | Command::Align(_)
                | Command::Paragraph(_)
                | Command::Style(_)
                | Command::List(_)
                | Command::ListLevel(_)
                | Command::MultilevelList
                | Command::RestartNumbering
                | Command::ContinueNumbering
                | Command::ReplaceAll { .. }
                | Command::InsertRows { .. }
                | Command::InsertColumns { .. }
                | Command::DeleteRows
                | Command::DeleteColumns
                | Command::DeleteTable
                | Command::CellBorders(_)
                | Command::CellShading(_)
                | Command::MergeCells
                | Command::ResizePicture { .. }
                | Command::PictureAnchor(_)
                | Command::PageSetup(_)
        );
        let erasing = !selection.is_empty() && !formatting;
        let (first, last) = match command {
            Command::DeleteBackward if !erasing => {
                (start.paragraph.saturating_sub(1), end.paragraph)
            }
            Command::DeleteForward if !erasing => (start.paragraph, end.paragraph + 1),
            // Замена может задеть любой абзац.
            Command::ReplaceAll { .. } => (0, paragraph_count(doc).saturating_sub(1)),
            // Новый номер получают и следующие пункты списка.
            Command::RestartNumbering | Command::ContinueNumbering => {
                (start.paragraph, paragraph_count(doc).saturating_sub(1))
            }
            _ => (start.paragraph, end.paragraph),
        };
        let from = top_level(doc, first).or_else(|| top_level(doc, start.paragraph));
        let to = top_level(doc, last).or_else(|| top_level(doc, end.paragraph));
        let (Some(from), Some(to)) = (from, to) else {
            return selection;
        };
        let sections_before = doc.sections.len();
        let blocks_before = doc.sections.get(to.0).map_or(0, |s| s.blocks.len());
        // Разрыв раздела и параметры страницы меняют разделы целиком.
        let whole = matches!(command, Command::SectionBreak(_) | Command::PageSetup(_));
        let saved = if from.0 == to.0 && !whole {
            Saved::Blocks(doc.sections[from.0].blocks[from.1..=to.1].to_vec())
        } else {
            Saved::Sections(doc.sections[from.0..=to.0].to_vec())
        };

        let at = if erasing {
            delete_range(doc, start, end)
        } else {
            selection.focus
        };
        let after = match &command {
            Command::Format(format) => {
                format_runs(doc, start, end, format.clone());
                Some(selection)
            }
            Command::Align(alignment) => {
                align_paragraphs(doc, start, end, *alignment);
                Some(selection)
            }
            Command::Paragraph(format) => {
                format_paragraphs(doc, start, end, *format);
                Some(selection)
            }
            Command::Style(id) => {
                apply_style(doc, start, end, id);
                Some(selection)
            }
            Command::List(kind) => {
                toggle_list(doc, start, end, *kind);
                Some(selection)
            }
            Command::ListLevel(delta) => {
                change_list_level(doc, start, end, *delta);
                Some(selection)
            }
            Command::MultilevelList => {
                multilevel_list(doc, start, end);
                Some(selection)
            }
            Command::RestartNumbering => {
                restart_numbering(doc, selection.focus.paragraph);
                Some(selection)
            }
            Command::ContinueNumbering => {
                continue_numbering(doc, selection.focus.paragraph);
                Some(selection)
            }
            Command::ReplaceAll {
                query,
                replacement,
                options,
            } => {
                replace_all(doc, query, replacement, *options);
                Some(Selection::caret(clamp(doc, selection.focus)))
            }
            Command::InsertRows { below } => {
                let at = insert_rows(doc, start.paragraph, end.paragraph, *below);
                Some(at.map_or(selection, Selection::caret))
            }
            Command::InsertColumns { right } => {
                let at = insert_columns(doc, start.paragraph, end.paragraph, *right);
                Some(at.map_or(selection, Selection::caret))
            }
            Command::DeleteRows => {
                let at = delete_rows(doc, start.paragraph, end.paragraph);
                Some(at.map_or(selection, Selection::caret))
            }
            Command::DeleteColumns => {
                let at = delete_columns(doc, start.paragraph, end.paragraph);
                Some(at.map_or(selection, Selection::caret))
            }
            Command::DeleteTable => {
                let at = delete_table(doc, selection.focus.paragraph);
                Some(at.map_or(selection, Selection::caret))
            }
            Command::CellBorders(preset) => {
                set_cell_borders(doc, start.paragraph, end.paragraph, *preset);
                Some(selection)
            }
            Command::CellShading(color) => {
                set_cell_shading(doc, start.paragraph, end.paragraph, *color);
                Some(selection)
            }
            Command::MergeCells => {
                let at = merge_cells(doc, start.paragraph, end.paragraph);
                Some(at.map_or(selection, Selection::caret))
            }
            Command::ResizePicture { width, height } => {
                resize_picture(doc, start, *width, *height);
                Some(selection)
            }
            Command::PictureAnchor(anchor) => {
                set_picture_anchor(doc, start, anchor.clone());
                Some(selection)
            }
            Command::PageSetup(setup) => {
                set_page_setup(doc, start.paragraph, end.paragraph, *setup);
                Some(selection)
            }
            _ => None,
        };
        let focus = match &command {
            _ if after.is_some() => at,
            Command::Type(text) => insert_text(doc, at, text),
            Command::SplitParagraph => split_paragraph(doc, at),
            Command::LineBreak => insert_line_break(doc, at),
            Command::DeleteBackward | Command::DeleteForward if erasing => at,
            Command::DeleteBackward => delete_backward(doc, at),
            Command::DeleteForward => delete_forward(doc, at),
            Command::Paste(fragment) => insert_fragment(doc, at, fragment),
            Command::PasteText(text) => insert_plain_text(doc, at, text),
            Command::InsertTable { rows, columns } => insert_table(doc, at, *rows, *columns),
            Command::InsertPicture(picture) => insert_picture(doc, at, (**picture).clone()),
            Command::Break(kind) => insert_break(doc, at, *kind),
            Command::SectionBreak(start) => insert_section_break(doc, at, *start),
            Command::Format(_)
            | Command::Align(_)
            | Command::Paragraph(_)
            | Command::Style(_)
            | Command::List(_)
            | Command::ListLevel(_)
            | Command::MultilevelList
            | Command::RestartNumbering
            | Command::ContinueNumbering
            | Command::Tab { .. }
            | Command::ReplaceAll { .. }
            | Command::InsertRows { .. }
            | Command::InsertColumns { .. }
            | Command::DeleteRows
            | Command::DeleteColumns
            | Command::DeleteTable
            | Command::CellBorders(_)
            | Command::CellShading(_)
            | Command::MergeCells
            | Command::ResizePicture { .. }
            | Command::PictureAnchor(_)
            | Command::PageSetup(_) => at,
        };
        let after = after.unwrap_or(Selection::caret(focus));

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
        self.last_id += 1;
        let step = Step {
            id: self.last_id,
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
            last.id = step.id;
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

    /// Номер состояния документа: меняется с каждой правкой, а отмена
    /// и повтор возвращают прежний номер. Если запомнить его при
    /// сохранении, по нему видно, изменён ли документ с тех пор.
    pub fn state(&self) -> u64 {
        self.undo.last().map_or(0, |s| s.id)
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

    #[test]
    fn replace_all_is_one_undo_step() {
        let mut d = doc(&["кот и кот", "кот"]);
        let mut h = History::new();
        let all = Selection::caret(Position::new(1, 3));
        let command = Command::ReplaceAll {
            query: "кот".into(),
            replacement: "пёс".into(),
            options: SearchOptions::default(),
        };
        let after = h.run(&mut d, all, command);
        assert_eq!(texts(&d), ["пёс и пёс", "пёс"]);
        assert_eq!(after, Selection::caret(Position::new(1, 2)));
        h.undo(&mut d);
        assert_eq!(texts(&d), ["кот и кот", "кот"]);
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
    fn pictures_are_inserted_resized_and_undone() {
        let mut d = doc(&["ab"]);
        let mut h = History::new();
        let picture = Picture {
            width: 127_000,
            height: 254_000,
            image: None,
            relationship: None,
            raw: None,
            anchor: None,
        };
        // Вставка заменяет выделенное.
        let picked = Selection::new(Position::new(0, 0), Position::new(0, 1));
        let s = h.run(&mut d, picked, Command::InsertPicture(Box::new(picture)));
        assert_eq!(s, caret(0, 3));
        assert_eq!(texts(&d), ["\u{fffc}b"]);
        let selected = Selection::new(Position::new(0, 0), Position::new(0, 3));
        let command = Command::ResizePicture {
            width: 254_000,
            height: 508_000,
        };
        assert_eq!(h.run(&mut d, selected, command), selected);
        let size = |d: &Document| {
            crate::picture::picture_ref(d, Position::new(0, 0)).map(|p| (p.width, p.height))
        };
        assert_eq!(size(&d), Some((254_000, 508_000)));
        h.undo(&mut d);
        assert_eq!(size(&d), Some((127_000, 254_000)));
        h.undo(&mut d);
        assert_eq!(texts(&d), ["ab"]);
    }

    #[test]
    fn tables_are_undone() {
        let mut d = doc(&["ab"]);
        let mut h = History::new();
        let caret = |p, o| Selection::caret(Position::new(p, o));
        let insert = Command::InsertTable {
            rows: 1,
            columns: 2,
        };
        let s = h.run(&mut d, caret(0, 2), insert);
        assert_eq!(s, caret(1, 0));
        assert_eq!(d.plain_text(), "ab\n\t\n");
        // Tab — в следующую ячейку, в последней — новая строка.
        let s = h.run(&mut d, s, Command::Type("x".into()));
        let s = h.run(&mut d, s, Command::Tab { shift: false });
        assert_eq!(s, caret(2, 0));
        let s = h.run(&mut d, s, Command::Tab { shift: false });
        assert_eq!(s, caret(3, 0));
        assert_eq!(d.plain_text(), "ab\nx\t\n\t\n");
        let s = h.run(&mut d, s, Command::Tab { shift: true });
        assert_eq!(s.focus, Position::new(2, 0));
        h.run(&mut d, caret(1, 0), Command::DeleteTable);
        assert_eq!(d.plain_text(), "ab\n");
        h.undo(&mut d);
        assert_eq!(d.plain_text(), "ab\nx\t\n\t\n");
        h.undo(&mut d);
        h.undo(&mut d);
        h.undo(&mut d);
        assert_eq!(d.plain_text(), "ab");
    }

    #[test]
    fn cell_formatting_and_merging_are_undone() {
        let mut d = doc(&["ab"]);
        let mut h = History::new();
        let insert = Command::InsertTable {
            rows: 1,
            columns: 2,
        };
        h.run(&mut d, Selection::caret(Position::new(0, 0)), insert);
        let both = Selection::new(Position::new(0, 0), Position::new(1, 0));
        let before = d.clone();
        let s = h.run(
            &mut d,
            both,
            Command::CellShading(Some(letter_model::Color::Rgb(9, 9, 9))),
        );
        assert_eq!(s, both);
        h.run(&mut d, both, Command::CellBorders(BorderPreset::None));
        let s = h.run(&mut d, both, Command::MergeCells);
        assert_eq!(s, Selection::caret(Position::new(0, 0)));
        assert_eq!(d.plain_text(), "\nab");
        h.undo(&mut d);
        h.undo(&mut d);
        h.undo(&mut d);
        assert_eq!(d, before);
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
    fn state_tells_whether_the_document_changed() {
        let mut d = doc(&[""]);
        let mut h = History::new();
        assert_eq!(h.state(), 0);
        let s = h.run(&mut d, caret(0, 0), Command::Type("a".into()));
        let saved = h.state();
        assert_ne!(saved, 0);
        // Набор, слитый с прошлым шагом, — всё равно новое состояние.
        h.run(&mut d, s, Command::Type("b".into()));
        assert_ne!(h.state(), saved);
        h.undo(&mut d);
        assert_eq!(h.state(), 0);
        h.redo(&mut d);
        let after = h.state();
        assert_ne!(after, saved);
        h.undo(&mut d);
        h.redo(&mut d);
        assert_eq!(h.state(), after);
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
    fn paste_is_one_step() {
        let mut d = doc(&["abc"]);
        let mut h = History::new();
        let s = h.run(&mut d, caret(0, 1), Command::PasteText("1\n2".into()));
        assert_eq!(s, caret(1, 1));
        let part = crate::clipboard::copy_range(&d, Position::new(0, 0), Position::new(1, 1));
        let sel = Selection::new(Position::new(1, 0), Position::new(1, 3));
        h.run(&mut d, sel, Command::Paste(Box::new(part)));
        assert_eq!(texts(&d), vec!["a1", "a1", "2"]);
        h.undo(&mut d);
        assert_eq!(texts(&d), vec!["a1", "2bc"]);
        h.undo(&mut d);
        assert_eq!(texts(&d), vec!["abc"]);
    }

    #[test]
    fn formatting_keeps_the_selection_and_is_undone() {
        let mut d = doc(&["one two"]);
        let mut h = History::new();
        let sel = Selection::new(Position::new(0, 4), Position::new(0, 0));
        assert_eq!(h.run(&mut d, sel, Command::Format(RunFormat::Bold)), sel);
        assert_eq!(h.run(&mut d, sel, Command::Align(Alignment::Center)), sel);
        assert_eq!(texts(&d), vec!["one two"]);
        let changed = d.clone();
        h.undo(&mut d);
        h.undo(&mut d);
        assert_eq!(d, doc(&["one two"]));
        h.redo(&mut d);
        h.redo(&mut d);
        assert_eq!(d, changed);
    }

    #[test]
    fn tables_and_sections() {
        let table = Table {
            grid: Vec::new(),
            raw_props: Vec::new(),
            props: Default::default(),
            rows: vec![TableRow {
                raw_props: Vec::new(),
                props: Default::default(),
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

    #[test]
    fn section_breaks_and_page_setup_are_undone() {
        let mut d = doc(&["ab", "cd"]);
        let original = d.clone();
        let mut h = History::new();
        let s = h.run(
            &mut d,
            caret(0, 1),
            Command::SectionBreak(SectionStart::Continuous),
        );
        assert_eq!(s, caret(1, 0));
        assert_eq!(d.sections.len(), 2);
        let columns = Command::PageSetup(PageSetup::Columns(crate::ColumnPreset::Two));
        h.run(&mut d, caret(2, 0), columns);
        assert!(d.sections[1].props.columns.is_some());
        assert!(d.sections[0].props.columns.is_none());
        h.run(&mut d, caret(2, 1), Command::Break(BreakKind::Page));
        assert_eq!(texts(&d), ["a", "b", "c\u{c}d"]);
        h.undo(&mut d);
        h.undo(&mut d);
        assert!(d.sections[1].props.columns.is_none());
        h.undo(&mut d);
        assert_eq!(d, original);
        h.redo(&mut d);
        assert_eq!(d.sections.len(), 2);
    }
}
