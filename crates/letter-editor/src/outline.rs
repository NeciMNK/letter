//! Заголовки и закладки на свёрстанных страницах: для оглавления
//! (закладок) PDF и ссылок внутри документа при сохранении в PDF.

use std::collections::HashMap;

use letter_layout::DocumentLayout;
use letter_model::Document;

use crate::caret::caret;
use crate::links::bookmarks;
use crate::position::Position;
use crate::toc::headings;

/// Заголовок документа и где он на страницах.
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineEntry {
    /// Номер абзаца заголовка.
    pub paragraph: usize,
    /// Уровень, 1–9.
    pub level: u8,
    /// Текст заголовка.
    pub title: String,
    /// Номер страницы, с нуля.
    pub page: usize,
    /// Верх первой строки, пт от верхнего края листа.
    pub top: f32,
}

/// Заголовки документа (все уровни) по порядку с местами на страницах.
pub fn document_outline(doc: &Document, layout: &DocumentLayout) -> Vec<OutlineEntry> {
    // Пустой диапазон: пропускать нечего.
    #[allow(clippy::reversed_empty_ranges)]
    let skip = 1..=0;
    headings(doc, (1, 9), &skip)
        .into_iter()
        .filter_map(|heading| {
            let place = caret(layout, Position::new(heading.paragraph, 0))?;
            Some(OutlineEntry {
                paragraph: heading.paragraph,
                level: heading.level,
                title: heading.text,
                page: place.page,
                top: place.top,
            })
        })
        .collect()
}

/// Места закладок документа: имя строчными буквами (Word сравнивает
/// имена без учёта регистра) → страница (с нуля) и верх строки, пт от
/// верхнего края листа.
pub fn bookmark_places(doc: &Document, layout: &DocumentLayout) -> HashMap<String, (usize, f32)> {
    let mut out = HashMap::new();
    for (name, position) in bookmarks(doc) {
        if let Some(place) = caret(layout, position) {
            out.entry(name.to_lowercase())
                .or_insert((place.page, place.top));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_layout::{FontLibrary, Shaper, layout_document};
    use letter_model::{BreakKind, Paragraph, ParagraphProps, Run, RunContent, Style, StyleKind};

    fn heading(text: &str, level: u8) -> Paragraph {
        let mut paragraph = Paragraph::text(text);
        paragraph.props = ParagraphProps {
            style: Some(format!("Heading{level}")),
            ..ParagraphProps::default()
        };
        paragraph
    }

    #[test]
    fn headings_and_bookmarks_get_pages() {
        let mut doc = Document::new();
        for (id, name) in [("Heading1", "heading 1"), ("Heading2", "heading 2")] {
            let mut style = Style::new(id, StyleKind::Paragraph);
            style.name = Some(name.into());
            doc.styles.styles.push(style);
        }
        let mut after_break = Paragraph::from_runs([Run {
            props: Default::default(),
            content: vec![RunContent::Break(BreakKind::Page)],
        }]);
        after_break.content.insert(
            0,
            letter_model::Inline::BookmarkStart(letter_model::Bookmark {
                id: "0".into(),
                name: "Место".into(),
            }),
        );
        doc.sections[0].blocks = vec![
            heading("Введение", 1).into(),
            heading("Подробности", 2).into(),
            after_break.into(),
            heading("Итоги", 1).into(),
        ];
        let library = FontLibrary::embedded_only();
        let layout = layout_document(&doc, &mut Shaper::new(), &library);
        let outline = document_outline(&doc, &layout);
        let short: Vec<(u8, &str, usize)> = outline
            .iter()
            .map(|e| (e.level, e.title.as_str(), e.page))
            .collect();
        assert_eq!(
            short,
            [(1, "Введение", 0), (2, "Подробности", 0), (1, "Итоги", 1)]
        );
        assert!(outline[1].top > outline[0].top);
        let places = bookmark_places(&doc, &layout);
        assert_eq!(places.get("место").map(|p| p.0), Some(0));
    }
}
