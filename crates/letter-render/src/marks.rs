//! Знаки форматирования (кнопка «¶» на ленте): точки на месте
//! пробелов, стрелки на месте табуляций, «¶» в конце абзаца и «↵» на
//! месте разрыва строки, «¬» на месте мягкого переноса. Рисуются только на экране.

use letter_layout::{LineEnd, LineItem, Page};

/// Какой знак.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkKind {
    /// Пробел: точка посередине.
    Space,
    /// Неразрывный пробел: кружок.
    NonBreakingSpace,
    /// Мягкий перенос, на котором строка не перенесена: «¬».
    SoftHyphen,
    /// Табуляция: стрелка вправо.
    Tab,
    /// Конец абзаца: «¶».
    Paragraph,
    /// Разрыв строки (Shift+Enter): «↵».
    LineBreak,
}

impl MarkKind {
    /// Буква, которой рисуется знак.
    pub fn symbol(self) -> char {
        match self {
            MarkKind::Space => '·',
            MarkKind::NonBreakingSpace => '°',
            MarkKind::SoftHyphen => '¬',
            MarkKind::Tab => '→',
            MarkKind::Paragraph => '¶',
            MarkKind::LineBreak => '↵',
        }
    }
}

/// Знак форматирования на листе.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mark {
    /// Вид.
    pub kind: MarkKind,
    /// Для пробела и табуляции — середина места по горизонтали, для «¶»
    /// и «↵» — левый край, пт от левого края листа.
    pub x: f32,
    /// Базовая линия, пт от верхнего края листа.
    pub baseline: f32,
    /// Размер шрифта текста рядом, пт.
    pub size: f32,
}

/// Знаки форматирования листа `page`.
pub fn formatting_marks(page: &Page) -> Vec<Mark> {
    let mut marks = Vec::new();
    let notes = page.notes.iter().flat_map(|n| &n.story.lines);
    let stories = page
        .header
        .lines
        .iter()
        .chain(&page.footer.lines)
        .chain(notes);
    for line in stories.chain(&page.lines) {
        let mark = |kind, x: f32, size: f32| Mark {
            kind,
            x: line.x + x,
            baseline: line.baseline,
            size,
        };
        let mut end = line.line.x;
        let mut size = line.spans.last().map_or(11.0, |s| s.size.0 as f32 / 2.0);
        for item in &line.line.items {
            match item {
                LineItem::Text(text) => {
                    let mut pen = text.x;
                    for glyph in &text.run.glyphs {
                        let kind =
                            match line
                                .text
                                .get(glyph.cluster..)
                                .and_then(|t| t.chars().next())
                            {
                                Some(' ') => Some(MarkKind::Space),
                                Some('\u{a0}') => Some(MarkKind::NonBreakingSpace),
                                // На месте переноса уже виден дефис.
                                Some('\u{ad}')
                                    if !text.run.glyphs.iter().any(|g| {
                                        g.cluster == glyph.cluster && g.x_advance > 0.0
                                    }) =>
                                {
                                    Some(MarkKind::SoftHyphen)
                                }
                                _ => None,
                            };
                        if let Some(kind) = kind {
                            marks.push(mark(kind, pen + glyph.x_advance / 2.0, text.run.size));
                        }
                        pen += glyph.x_advance;
                    }
                    end = end.max(pen);
                    size = text.run.size;
                }
                LineItem::Tab(tab) => {
                    marks.push(mark(MarkKind::Tab, tab.x + tab.width / 2.0, size));
                    end = end.max(tab.x + tab.width);
                }
                LineItem::Object(object) => end = end.max(object.x + object.width),
            }
        }
        let last = match line.line.end {
            LineEnd::Paragraph => Some(MarkKind::Paragraph),
            LineEnd::Break => Some(MarkKind::LineBreak),
            LineEnd::Wrap | LineEnd::Page | LineEnd::Column => None,
        };
        if let Some(kind) = last {
            marks.push(mark(kind, end, size));
        }
    }
    marks
}
