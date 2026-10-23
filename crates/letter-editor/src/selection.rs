//! Выделение: от «якоря» (где выделение начали) до курсора, слово
//! (двойной щелчок), абзац (тройной), весь документ (Ctrl+A) и
//! прямоугольники, которыми выделение закрашивается на страницах.

use letter_layout::{DocumentLayout, LineEnd, PageLine};
use unicode_segmentation::UnicodeSegmentation;

use crate::caret::line_stops;
use crate::position::Position;

/// Выделенный кусок документа. Пустое выделение (`anchor == focus`) —
/// просто курсор.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Selection {
    /// Где выделение начали (не двигается при Shift+стрелках).
    pub anchor: Position,
    /// Где курсор.
    pub focus: Position,
}

impl Selection {
    /// Пустое выделение: курсор в `at`.
    pub fn caret(at: Position) -> Selection {
        Selection {
            anchor: at,
            focus: at,
        }
    }

    /// Выделение от `anchor` до `focus`.
    pub fn new(anchor: Position, focus: Position) -> Selection {
        Selection { anchor, focus }
    }

    /// Выделено ли что-нибудь.
    pub fn is_empty(&self) -> bool {
        key(self.anchor) == key(self.focus)
    }

    /// Начало (что раньше в документе).
    pub fn start(&self) -> Position {
        if key(self.anchor) <= key(self.focus) {
            self.anchor
        } else {
            self.focus
        }
    }

    /// Конец (что дальше в документе).
    pub fn end(&self) -> Position {
        if key(self.anchor) <= key(self.focus) {
            self.focus
        } else {
            self.anchor
        }
    }
}

/// Порядок позиций в документе (привязка к строке не важна).
fn key(p: Position) -> (usize, usize) {
    (p.paragraph, p.offset)
}

/// Прямоугольник выделения на странице, пт от левого верхнего угла листа.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionRect {
    /// Номер страницы, с нуля.
    pub page: usize,
    /// Левый край.
    pub left: f32,
    /// Правый край.
    pub right: f32,
    /// Верх.
    pub top: f32,
    /// Низ.
    pub bottom: f32,
}

/// Текст абзаца `paragraph` из вёрстки.
fn paragraph_text(layout: &DocumentLayout, paragraph: usize) -> Option<&str> {
    layout
        .pages
        .iter()
        .flat_map(|p| &p.lines)
        .find(|l| l.paragraph == paragraph)
        .map(|l| &*l.text)
}

/// Слово под позицией `at` (двойной щелчок): как в Word, вместе
/// с пробелами после него. Пробелы и знаки препинания выделяются сами
/// по себе.
pub fn word_at(layout: &DocumentLayout, at: Position) -> Selection {
    let Some(text) = paragraph_text(layout, at.paragraph) else {
        return Selection::caret(at);
    };
    let segments: Vec<(usize, &str)> = text.split_word_bound_indices().collect();
    // Кусок, в котором стоит позиция; в конце абзаца — последний.
    let Some(index) = segments
        .iter()
        .position(|(i, s)| at.offset < i + s.len())
        .or_else(|| segments.len().checked_sub(1))
    else {
        return Selection::caret(at);
    };
    let (start, segment) = segments[index];
    let mut end = start + segment.len();
    let is_space = |s: &str| s.chars().all(|c| c == ' ' || c == '\u{a0}');
    if is_word(segment)
        && let Some((i, next)) = segments.get(index + 1)
        && is_space(next)
    {
        end = i + next.len();
    }
    Selection::new(
        Position::new(at.paragraph, start),
        Position::new(at.paragraph, end),
    )
}

fn is_word(segment: &str) -> bool {
    segment.chars().any(char::is_alphanumeric)
}

/// Абзац целиком (тройной щелчок).
pub fn paragraph_at(layout: &DocumentLayout, paragraph: usize) -> Selection {
    let len = paragraph_text(layout, paragraph).map_or(0, str::len);
    Selection::new(Position::new(paragraph, 0), Position::new(paragraph, len))
}

/// Весь документ (Ctrl+A).
pub fn select_all(layout: &DocumentLayout) -> Selection {
    let paragraphs = || {
        layout
            .pages
            .iter()
            .flat_map(|p| &p.lines)
            .map(|l| l.paragraph)
    };
    let first = paragraphs().min().unwrap_or(0);
    let last = paragraphs().max().unwrap_or(first);
    let end = paragraph_text(layout, last).map_or(0, str::len);
    Selection::new(Position::new(first, 0), Position::new(last, end))
}

/// Прямоугольники, которыми закрашивается выделение: по одному на кусок
/// строки (в строке со смешанным направлением письма их может быть
/// несколько). Если выделение идёт дальше конца абзаца, за последней
/// буквой закрашивается ещё место знака абзаца, как в Word.
pub fn selection_rects(layout: &DocumentLayout, selection: Selection) -> Vec<SelectionRect> {
    let mut out = Vec::new();
    if selection.is_empty() {
        return out;
    }
    let (start, end) = (key(selection.start()), key(selection.end()));
    for (page, p) in layout.pages.iter().enumerate() {
        for line in &p.lines {
            let range = &line.line.range;
            let (a, b) = ((line.paragraph, range.start), (line.paragraph, range.end));
            if b < start || a > end || (b == start && range.start < range.end) {
                continue;
            }
            let from = if start.0 == line.paragraph {
                start.1.max(range.start)
            } else {
                range.start
            };
            let to = if end.0 == line.paragraph {
                end.1.min(range.end)
            } else {
                range.end
            };
            line_rects(line, page, from, to, &mut out);
            // Знак абзаца выделен, если выделение уходит в следующий абзац.
            if line.line.end != LineEnd::Wrap
                && range.end == line.text.len()
                && end.0 > line.paragraph
            {
                let right = line_stops(line)
                    .iter()
                    .find(|s| s.offset == range.end)
                    .map_or(line.x, |s| s.x);
                out.push(SelectionRect {
                    page,
                    left: right,
                    right: right + mark_width(line),
                    top: line.top,
                    bottom: line.top + line.height,
                });
            }
        }
    }
    merge(&mut out);
    out
}

/// Ширина знака абзаца — примерно ширина пробела.
fn mark_width(line: &PageLine) -> f32 {
    (line.line.ascent + line.line.descent) * 0.3
}

/// Куски строки с текстом от `from` до `to`.
fn line_rects(line: &PageLine, page: usize, from: usize, to: usize, out: &mut Vec<SelectionRect>) {
    if from >= to {
        return;
    }
    let stops = line_stops(line);
    for pair in stops.windows(2) {
        let (lo, hi) = (
            pair[0].offset.min(pair[1].offset),
            pair[0].offset.max(pair[1].offset),
        );
        if lo >= from && hi <= to && lo < hi {
            out.push(SelectionRect {
                page,
                left: pair[0].x.min(pair[1].x),
                right: pair[0].x.max(pair[1].x),
                top: line.top,
                bottom: line.top + line.height,
            });
        }
    }
}

/// Склеивает соседние прямоугольники одной строки.
fn merge(rects: &mut Vec<SelectionRect>) {
    let mut merged: Vec<SelectionRect> = Vec::with_capacity(rects.len());
    for r in rects.drain(..) {
        if let Some(last) = merged.last_mut()
            && last.page == r.page
            && last.top == r.top
            && last.bottom == r.bottom
            && (r.left - last.right).abs() < 0.01
        {
            last.right = r.right;
            continue;
        }
        merged.push(r);
    }
    *rects = merged;
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_layout::{FontLibrary, Shaper, layout_document};
    use letter_model::{Block, Document, Paragraph};

    fn layout(texts: &[&str]) -> DocumentLayout {
        let mut doc = Document::new();
        doc.sections[0].blocks = texts
            .iter()
            .map(|t| Block::Paragraph(Paragraph::text(*t)))
            .collect();
        layout_document(&doc, &mut Shaper::new(), &FontLibrary::embedded_only())
    }

    fn range(s: Selection) -> ((usize, usize), (usize, usize)) {
        (key(s.start()), key(s.end()))
    }

    #[test]
    fn order_does_not_matter() {
        let s = Selection::new(Position::new(2, 1), Position::new(0, 5));
        assert_eq!(s.start(), Position::new(0, 5));
        assert_eq!(s.end(), Position::new(2, 1));
        assert!(!s.is_empty());
        assert!(Selection::caret(Position::new(1, 1)).is_empty());
    }

    #[test]
    fn words_take_their_spaces() {
        let doc = layout(&["Привет,  мир! end"]);
        let at = |o| range(word_at(&doc, Position::new(0, o)));
        // «Привет» без запятой (за ним сразу запятая).
        assert_eq!(at(3), ((0, 0), (0, 12)));
        // Запятая — сама по себе.
        assert_eq!(at(12), ((0, 12), (0, 13)));
        // «мир» с пробелом? Нет: за ним «!».
        assert_eq!(at(15), ((0, 15), (0, 21)));
        // Последнее слово, курсор в конце абзаца.
        assert_eq!(at(26), ((0, 23), (0, 26)));
        let doc = layout(&["one two"]);
        assert_eq!(range(word_at(&doc, Position::new(0, 1))), ((0, 0), (0, 4)));
        // Пустой абзац.
        let doc = layout(&[""]);
        assert!(word_at(&doc, Position::new(0, 0)).is_empty());
    }

    #[test]
    fn paragraph_and_all() {
        let doc = layout(&["one", "two three"]);
        assert_eq!(range(paragraph_at(&doc, 1)), ((1, 0), (1, 9)));
        assert_eq!(range(select_all(&doc)), ((0, 0), (1, 9)));
    }

    #[test]
    fn rects_cover_lines_and_paragraph_marks() {
        let long = "word ".repeat(40);
        let doc = layout(&["abc", long.trim_end(), "end"]);
        let lines = &doc.pages[0].lines;
        assert!(lines.len() >= 4);
        assert!(selection_rects(&doc, Selection::caret(Position::new(0, 1))).is_empty());

        // Внутри одной строки: от «b» до конца «c», знак абзаца не выделен.
        let rects = selection_rects(
            &doc,
            Selection::new(Position::new(0, 3), Position::new(0, 1)),
        );
        assert_eq!(rects.len(), 1);
        let stops = line_stops(&lines[0]);
        assert!((rects[0].left - stops[1].x).abs() < 0.01);
        assert!((rects[0].right - stops[3].x).abs() < 0.01);
        assert_eq!(rects[0].top, lines[0].top);

        // От «b» до «n» в последнем абзаце: строка на каждую строку
        // текста, у первой — знак абзаца (прямоугольник шире текста).
        let all = selection_rects(
            &doc,
            Selection::new(Position::new(0, 1), Position::new(2, 2)),
        );
        assert_eq!(all.len(), lines.len());
        assert!(all[0].right > stops[3].x + 1.0);
        // Перенос внутри длинного абзаца: без знака абзаца.
        let wrapped = &lines[1];
        let wrap_end = line_stops(wrapped).last().map(|s| s.x).unwrap_or(0.0);
        assert!(all[1].right <= wrap_end + 0.01);
    }
}
