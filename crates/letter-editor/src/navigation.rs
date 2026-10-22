//! Перемещение курсора с клавиатуры: стрелки, Home/End, по словам,
//! по абзацам, на экран вверх и вниз, в начало и конец документа.
//!
//! Всё считается по свёрстанным страницам: в них есть и текст каждого
//! абзаца, и где какая строка.

use std::collections::BTreeMap;

use letter_layout::{DocumentLayout, PageLine};
use unicode_segmentation::UnicodeSegmentation;

use crate::caret::{affinity_at, caret, line_stops, position_in_line};
use crate::position::{Affinity, Position};

/// Куда передвинуть курсор (клавиши — как в Word).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Motion {
    /// ← — на букву назад.
    Left,
    /// → — на букву вперёд.
    Right,
    /// Ctrl+← — в начало слова (или предыдущего слова).
    WordLeft,
    /// Ctrl+→ — в начало следующего слова.
    WordRight,
    /// ↑ — на строку вверх.
    Up,
    /// ↓ — на строку вниз.
    Down,
    /// Home — в начало строки.
    LineStart,
    /// End — в конец строки.
    LineEnd,
    /// Ctrl+↑ — в начало абзаца (или предыдущего абзаца).
    ParagraphUp,
    /// Ctrl+↓ — в начало следующего абзаца.
    ParagraphDown,
    /// PageUp — вверх на столько пунктов документа, сколько видно на экране.
    ScreenUp(f32),
    /// PageDown — вниз на столько пунктов документа, сколько видно на экране.
    ScreenDown(f32),
    /// Ctrl+Home — в начало документа.
    DocumentStart,
    /// Ctrl+End — в конец документа.
    DocumentEnd,
}

/// Курсор после перемещения.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moved {
    /// Новая позиция.
    pub position: Position,
    /// Колонка, к которой курсор стремится при движении вверх и вниз,
    /// пт от левого края листа. Её помнят между нажатиями ↑ и ↓, чтобы
    /// курсор, пройдя короткую строку, вернулся в прежнюю колонку.
    /// `None` после движения по горизонтали.
    pub goal_x: Option<f32>,
}

/// Передвигает курсор из `from` по команде `motion`. `goal_x` — колонка
/// из прошлого [`Moved`] (для ↑, ↓, PageUp, PageDown). Если двигаться
/// некуда (← в начале документа), позиция остаётся прежней.
pub fn move_cursor(
    layout: &DocumentLayout,
    from: Position,
    motion: Motion,
    goal_x: Option<f32>,
) -> Moved {
    let nav = Navigator::new(layout);
    let horizontal = |position: Option<Position>| Moved {
        position: position.unwrap_or(from),
        goal_x: None,
    };
    match motion {
        Motion::Left => horizontal(nav.left(from)),
        Motion::Right => horizontal(nav.right(from)),
        Motion::WordLeft => horizontal(nav.word_left(from)),
        Motion::WordRight => horizontal(nav.word_right(from)),
        Motion::LineStart => horizontal(nav.line_edge(from, false)),
        Motion::LineEnd => horizontal(nav.line_edge(from, true)),
        Motion::ParagraphUp => horizontal(nav.paragraph_up(from)),
        Motion::ParagraphDown => horizontal(nav.paragraph_down(from)),
        Motion::DocumentStart => horizontal(nav.paragraphs.first().map(|p| Position::new(p.0, 0))),
        Motion::DocumentEnd => {
            horizontal(nav.paragraphs.last().map(|p| Position::new(p.0, p.1.len())))
        }
        Motion::Up => nav.vertical(from, goal_x, Step::Lines(-1)),
        Motion::Down => nav.vertical(from, goal_x, Step::Lines(1)),
        Motion::ScreenUp(distance) => nav.vertical(from, goal_x, Step::Points(-distance)),
        Motion::ScreenDown(distance) => nav.vertical(from, goal_x, Step::Points(distance)),
    }
}

/// Шаг по вертикали.
#[derive(Clone, Copy)]
enum Step {
    /// На столько строк (минус — вверх).
    Lines(isize),
    /// На столько пунктов (минус — вверх).
    Points(f32),
}

/// Строки и абзацы документа в удобном для перемещения виде.
struct Navigator<'a> {
    /// Абзацы со строками по порядку: номер и текст.
    paragraphs: Vec<(usize, &'a str)>,
    /// Все строки сверху вниз: страница, номер строки на ней, строка.
    lines: Vec<(usize, usize, &'a PageLine)>,
    /// Верх каждой страницы, если сложить листы стопкой без зазоров, пт.
    page_tops: Vec<f32>,
    layout: &'a DocumentLayout,
}

impl<'a> Navigator<'a> {
    fn new(layout: &'a DocumentLayout) -> Navigator<'a> {
        let mut texts = BTreeMap::new();
        let mut lines = Vec::new();
        let mut page_tops = Vec::with_capacity(layout.pages.len());
        let mut top = 0.0;
        for (p, page) in layout.pages.iter().enumerate() {
            page_tops.push(top);
            top += page.height;
            for (l, line) in page.lines.iter().enumerate() {
                texts.entry(line.paragraph).or_insert(&*line.text);
                lines.push((p, l, line));
            }
        }
        Navigator {
            paragraphs: texts.into_iter().collect(),
            lines,
            page_tops,
            layout,
        }
    }

    /// Номер абзаца `paragraph` в списке [`Self::paragraphs`].
    fn index_of(&self, paragraph: usize) -> Option<usize> {
        self.paragraphs
            .binary_search_by_key(&paragraph, |p| p.0)
            .ok()
    }

    fn text(&self, paragraph: usize) -> Option<&'a str> {
        self.index_of(paragraph).map(|i| self.paragraphs[i].1)
    }

    /// Конец предыдущего абзаца.
    fn previous_end(&self, paragraph: usize) -> Option<Position> {
        let i = self.index_of(paragraph)?.checked_sub(1)?;
        let (p, text) = self.paragraphs[i];
        Some(Position::new(p, text.len()))
    }

    /// Начало следующего абзаца.
    fn next_start(&self, paragraph: usize) -> Option<Position> {
        let i = self.index_of(paragraph)? + 1;
        self.paragraphs.get(i).map(|p| Position::new(p.0, 0))
    }

    fn left(&self, from: Position) -> Option<Position> {
        let text = self.text(from.paragraph)?;
        match text.get(..from.offset)?.graphemes(true).next_back() {
            Some(g) => Some(Position::new(from.paragraph, from.offset - g.len())),
            None => self.previous_end(from.paragraph),
        }
    }

    fn right(&self, from: Position) -> Option<Position> {
        let text = self.text(from.paragraph)?;
        match text.get(from.offset..)?.graphemes(true).next() {
            Some(g) => Some(Position::new(from.paragraph, from.offset + g.len())),
            None => self.next_start(from.paragraph),
        }
    }

    fn word_left(&self, from: Position) -> Option<Position> {
        let text = self.text(from.paragraph)?;
        if from.offset == 0 {
            return self.previous_end(from.paragraph);
        }
        let start = word_starts(text)
            .into_iter()
            .rev()
            .find(|&s| s < from.offset)
            .unwrap_or(0);
        Some(Position::new(from.paragraph, start))
    }

    fn word_right(&self, from: Position) -> Option<Position> {
        let text = self.text(from.paragraph)?;
        if from.offset >= text.len() {
            return self.next_start(from.paragraph);
        }
        // Конец абзаца (перед знаком ¶) — тоже остановка, как в Word.
        let next = word_starts(text)
            .into_iter()
            .find(|&s| s > from.offset)
            .unwrap_or(text.len());
        Some(Position::new(from.paragraph, next))
    }

    fn paragraph_up(&self, from: Position) -> Option<Position> {
        if from.offset > 0 {
            return Some(Position::new(from.paragraph, 0));
        }
        let i = self.index_of(from.paragraph)?.checked_sub(1)?;
        Some(Position::new(self.paragraphs[i].0, 0))
    }

    fn paragraph_down(&self, from: Position) -> Option<Position> {
        self.next_start(from.paragraph).or_else(|| {
            let text = self.text(from.paragraph)?;
            Some(Position::new(from.paragraph, text.len()))
        })
    }

    /// Номер строки с курсором в [`Self::lines`].
    fn line_of(&self, position: Position) -> Option<usize> {
        let c = caret(self.layout, position)?;
        self.lines
            .iter()
            .position(|&(p, l, _)| p == c.page && l == c.line)
    }

    /// Начало (`end == false`) или конец строки с курсором.
    fn line_edge(&self, from: Position, end: bool) -> Option<Position> {
        let (_, _, line) = self.lines[self.line_of(from)?];
        let offsets = line_stops(line).into_iter().map(|s| s.offset);
        let offset = if end { offsets.max()? } else { offsets.min()? };
        let affinity = if end {
            affinity_at(line, offset)
        } else {
            Affinity::Downstream
        };
        Some(Position::new(line.paragraph, offset).with_affinity(affinity))
    }

    /// Середина строки по вертикали в стопке листов, пт.
    fn line_middle(&self, index: usize) -> f32 {
        let (page, _, line) = self.lines[index];
        self.page_tops[page] + line.top + line.height / 2.0
    }

    /// Верх и низ строки в стопке листов, пт.
    fn line_span(&self, index: usize) -> (f32, f32) {
        let (page, _, line) = self.lines[index];
        let top = self.page_tops[page] + line.top;
        (top, top + line.height)
    }

    /// Строка на `n` строк выше или ниже строки `current` при курсоре
    /// на высоте `x`. Вне таблиц — соседняя по порядку. В таблице —
    /// сначала строки той же ячейки, а за её краем — ближайшая строка
    /// по вертикали в столбце под курсором (или вне таблицы), а не
    /// соседняя ячейка той же строки.
    fn line_step(&self, current: usize, x: f32, n: isize) -> Option<usize> {
        let mut at = current;
        for _ in 0..n.unsigned_abs() {
            at = self.one_line(at, x, n > 0)?;
        }
        Some(at)
    }

    fn one_line(&self, current: usize, x: f32, down: bool) -> Option<usize> {
        let line = self.lines[current].2;
        let neighbour = if down {
            current.checked_add(1).filter(|&i| i < self.lines.len())
        } else {
            current.checked_sub(1)
        };
        let in_table = |i: usize| self.lines[i].2.cell.is_some();
        if line.cell.is_none() && !neighbour.is_some_and(in_table) {
            return neighbour;
        }
        // Следующая строка той же ячейки.
        if let Some(i) = neighbour
            && line.cell.is_some()
            && self.lines[i].2.cell == line.cell
        {
            return Some(i);
        }
        // Край ячейки (или строки вне таблицы) в стопке листов.
        let (page, _, _) = self.lines[current];
        let edge = match (line.cell, down) {
            (Some(cell), true) => self.page_tops[page] + cell.bottom,
            (Some(cell), false) => self.page_tops[page] + cell.top,
            (None, true) => self.line_span(current).1,
            (None, false) => self.line_span(current).0,
        };
        let beyond = |i: usize| {
            let (top, bottom) = self.line_span(i);
            if down {
                top >= edge - 0.5
            } else {
                bottom <= edge + 0.5
            }
        };
        let column = |i: usize| {
            self.lines[i]
                .2
                .cell
                .is_none_or(|c| (c.left..c.right).contains(&x))
        };
        let distance = |i: usize| {
            let (top, bottom) = self.line_span(i);
            if down { top - edge } else { edge - bottom }
        };
        let candidates: Vec<usize> = (0..self.lines.len())
            .filter(|&i| i != current && beyond(i))
            .collect();
        let pick = |list: &mut dyn Iterator<Item = usize>| {
            list.min_by(|&a, &b| distance(a).total_cmp(&distance(b)))
        };
        pick(&mut candidates.iter().copied().filter(|&i| column(i)))
            .or_else(|| pick(&mut candidates.iter().copied()))
    }

    fn vertical(&self, from: Position, goal_x: Option<f32>, step: Step) -> Moved {
        let stay = Moved {
            position: from,
            goal_x,
        };
        let (Some(current), Some(c)) = (self.line_of(from), caret(self.layout, from)) else {
            return stay;
        };
        let x = goal_x.unwrap_or(c.x);
        let target = match step {
            Step::Lines(n) => self.line_step(current, x, n),
            Step::Points(distance) => {
                let y = self.line_middle(current) + distance;
                // Строка, середина которой ближе всего к нужной высоте.
                (0..self.lines.len())
                    .min_by(|&a, &b| {
                        let da = (self.line_middle(a) - y).abs();
                        let db = (self.line_middle(b) - y).abs();
                        da.total_cmp(&db)
                    })
                    .filter(|&i| i != current)
            }
        };
        let Some(target) = target else {
            return Moved {
                position: from,
                goal_x: Some(x),
            };
        };
        match position_in_line(self.lines[target].2, x) {
            Some(position) => Moved {
                position,
                goal_x: Some(x),
            },
            None => stay,
        }
    }
}

/// Начала слов в тексте абзаца: знаки препинания — отдельные слова,
/// пробелы после слова относятся к нему (Ctrl+→ перепрыгивает их).
fn word_starts(text: &str) -> Vec<usize> {
    text.split_word_bound_indices()
        .filter(|(_, w)| !w.chars().all(char::is_whitespace))
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_layout::{FontLibrary, Shaper, layout_document};
    use letter_model::{Block, Document, Paragraph};

    fn layout(paragraphs: Vec<Paragraph>) -> DocumentLayout {
        let mut doc = Document::new();
        doc.sections[0].blocks = paragraphs.into_iter().map(Block::Paragraph).collect();
        layout_document(&doc, &mut Shaper::new(), &FontLibrary::embedded_only())
    }

    fn texts(texts: &[&str]) -> DocumentLayout {
        layout(texts.iter().map(|t| Paragraph::text(*t)).collect())
    }

    fn go(doc: &DocumentLayout, from: Position, motion: Motion) -> Position {
        move_cursor(doc, from, motion, None).position
    }

    #[test]
    fn arrows_step_over_graphemes_and_paragraphs() {
        // «й» из двух кодов — одна графема.
        let doc = texts(&["ай\u{306}", "b"]);
        let mut p = Position::new(0, 0);
        let mut seen = Vec::new();
        for _ in 0..5 {
            p = go(&doc, p, Motion::Right);
            seen.push((p.paragraph, p.offset));
        }
        // Дальше конца документа не уходит.
        assert_eq!(seen, vec![(0, 2), (0, 6), (1, 0), (1, 1), (1, 1)]);
        let mut back = Vec::new();
        for _ in 0..5 {
            p = go(&doc, p, Motion::Left);
            back.push((p.paragraph, p.offset));
        }
        assert_eq!(back, vec![(1, 0), (0, 6), (0, 2), (0, 0), (0, 0)]);
    }

    #[test]
    fn words_like_word() {
        let doc = texts(&["Hello, big  world", "Next"]);
        let mut p = Position::new(0, 0);
        let mut stops = Vec::new();
        for _ in 0..6 {
            p = go(&doc, p, Motion::WordRight);
            stops.push((p.paragraph, p.offset));
        }
        // «,» — отдельное слово, пробелы — часть слова, конец абзаца —
        // остановка.
        assert_eq!(
            stops,
            vec![(0, 5), (0, 7), (0, 12), (0, 17), (1, 0), (1, 4)]
        );
        let mut back = Vec::new();
        for _ in 0..4 {
            p = go(&doc, p, Motion::WordLeft);
            back.push((p.paragraph, p.offset));
        }
        assert_eq!(back, vec![(1, 0), (0, 17), (0, 12), (0, 7)]);
        // Из середины слова — в его начало.
        assert_eq!(go(&doc, Position::new(0, 9), Motion::WordLeft).offset, 7);
    }

    #[test]
    fn home_and_end_stay_on_the_line() {
        let words = "word ".repeat(40);
        let doc = texts(&[words.trim_end()]);
        let lines = &doc.pages[0].lines;
        let wrap = lines[0].line.range.end;
        let end = go(&doc, Position::new(0, 3), Motion::LineEnd);
        // Конец первой строки — на ней, а не в начале второй.
        assert_eq!(
            end,
            Position::new(0, wrap).with_affinity(Affinity::Upstream)
        );
        assert_eq!(caret(&doc, end).unwrap().line, 0);
        assert_eq!(go(&doc, end, Motion::LineStart), Position::new(0, 0));
        // Со второй строки Home — в её начало.
        let second = Position::new(0, wrap + 2);
        assert_eq!(go(&doc, second, Motion::LineStart), Position::new(0, wrap));
        let last = go(&doc, second, Motion::LineEnd);
        assert_eq!(caret(&doc, last).unwrap().line, 1);
    }

    #[test]
    fn up_and_down_keep_the_column() {
        let doc = texts(&["Long first line here", "ab", "Another long line"]);
        let start = Position::new(0, 10);
        let x = caret(&doc, start).unwrap().x;
        let down = move_cursor(&doc, start, Motion::Down, None);
        // Короткая строка — в её конец, колонка запомнена.
        assert_eq!(down.position, Position::new(1, 2));
        assert_eq!(down.goal_x, Some(x));
        let down = move_cursor(&doc, down.position, Motion::Down, down.goal_x);
        assert_eq!(down.position.paragraph, 2);
        let back_x = caret(&doc, down.position).unwrap().x;
        assert!((back_x - x).abs() < 5.0, "{back_x} vs {x}");
        let up = move_cursor(&doc, down.position, Motion::Up, down.goal_x);
        let up = move_cursor(&doc, up.position, Motion::Up, up.goal_x);
        assert_eq!(up.position, start);
        // Выше первой строки некуда.
        assert_eq!(go(&doc, start, Motion::Up), start);
        // Горизонтальное движение забывает колонку.
        assert_eq!(
            move_cursor(&doc, start, Motion::Right, Some(x)).goal_x,
            None
        );
    }

    #[test]
    fn up_and_down_go_through_table_columns() {
        use letter_model::{Table, TableCell, TableRow, Twips};
        let cell = |text: &str| TableCell {
            blocks: text.split('|').map(|t| Paragraph::text(t).into()).collect(),
            ..TableCell::default()
        };
        let mut doc = Document::new();
        let table = Table {
            grid: vec![Twips(2000), Twips(2000)],
            rows: vec![
                TableRow {
                    cells: vec![cell("a"), cell("b1|b2")],
                    ..TableRow::default()
                },
                TableRow {
                    cells: vec![cell("c"), cell("d")],
                    ..TableRow::default()
                },
            ],
            ..Table::default()
        };
        doc.sections[0].blocks = vec![
            Paragraph::text("top").into(),
            table.into(),
            Paragraph::text("end").into(),
        ];
        let layout = layout_document(&doc, &mut Shaper::new(), &FontLibrary::embedded_only());
        let down = |p: usize| go(&layout, Position::new(p, 0), Motion::Down).paragraph;
        let up = |p: usize| go(&layout, Position::new(p, 0), Motion::Up).paragraph;
        // «a» → «c» мимо «b1», «b2»; «b1» → «b2» → «d» → «end».
        assert_eq!(down(1), 4);
        assert_eq!(down(2), 3);
        assert_eq!(down(3), 5);
        assert_eq!(down(5), 6);
        assert_eq!(up(5), 3);
        assert_eq!(up(4), 1);
        assert_eq!(down(0), 1);
        assert_eq!(up(1), 0);
    }

    #[test]
    fn paragraphs_and_document_edges() {
        let doc = texts(&["One two", "Three", "Four"]);
        let mid = Position::new(1, 3);
        assert_eq!(go(&doc, mid, Motion::ParagraphUp), Position::new(1, 0));
        assert_eq!(
            go(&doc, Position::new(1, 0), Motion::ParagraphUp),
            Position::new(0, 0)
        );
        assert_eq!(go(&doc, mid, Motion::ParagraphDown), Position::new(2, 0));
        assert_eq!(
            go(&doc, Position::new(2, 0), Motion::ParagraphDown),
            Position::new(2, 4)
        );
        assert_eq!(go(&doc, mid, Motion::DocumentStart), Position::new(0, 0));
        assert_eq!(go(&doc, mid, Motion::DocumentEnd), Position::new(2, 4));
    }

    #[test]
    fn page_down_moves_by_a_screen() {
        let paragraphs: Vec<String> = (0..120).map(|i| format!("Line {i}")).collect();
        let refs: Vec<&str> = paragraphs.iter().map(String::as_str).collect();
        let doc = texts(&refs);
        assert!(doc.pages.len() >= 2);
        let first = &doc.pages[0].lines;
        let pitch = first[1].top - first[0].top;
        let start = Position::new(0, 0);
        let down = move_cursor(&doc, start, Motion::ScreenDown(pitch * 10.0), None);
        assert_eq!(down.position.paragraph, 10);
        let up = move_cursor(
            &doc,
            down.position,
            Motion::ScreenUp(pitch * 10.0),
            down.goal_x,
        );
        assert_eq!(up.position, start);
        // Через границу страниц — на следующий лист.
        let far = go(&doc, start, Motion::ScreenDown(doc.pages[0].height));
        assert_eq!(caret(&doc, far).unwrap().page, 1);
        // Ниже последней строки — последняя строка.
        let end = go(&doc, start, Motion::ScreenDown(1.0e6));
        assert_eq!(end.paragraph, 119);
    }
}
