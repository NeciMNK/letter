//! Титульная страница («Вставка → Страницы → Титульная страница»):
//! название, подзаголовок, автор и дата на отдельной первой странице.
//!
//! Word хранит титульные страницы готовыми блоками со своими полями;
//! у нас это обычные абзацы с прямым оформлением в начале документа,
//! а после них — разрыв страницы.

use letter_model::{
    Alignment, Block, Border, BreakKind, Color, HalfPoints, Indentation, Paragraph,
    ParagraphBorders, ParagraphProps, Run, RunContent, RunProps, Shading, Spacing, Twips,
};

use crate::position::Position;

/// Вид титульной страницы.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoverLayout {
    /// Название крупно слева, автор и дата внизу.
    Plain,
    /// Название по центру на цветной полосе.
    Band,
    /// Название с толстой цветной линией слева.
    Side,
}

impl CoverLayout {
    /// Все виды по порядку меню.
    pub const ALL: [CoverLayout; 3] = [CoverLayout::Plain, CoverLayout::Band, CoverLayout::Side];
}

/// Что написать на титульной странице.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverPage {
    /// Вид.
    pub layout: CoverLayout,
    /// Название документа.
    pub title: String,
    /// Подзаголовок.
    pub subtitle: String,
    /// Автор.
    pub author: String,
    /// Дата.
    pub date: String,
}

/// Тёмно-синий цвет титульных страниц.
const ACCENT: Color = Color::Rgb(0x1F, 0x38, 0x64);
/// Серый цвет подписей.
const GRAY: Color = Color::Rgb(0x59, 0x59, 0x59);

/// Вставляет титульную страницу в начало документа и возвращает начало
/// её названия.
pub fn insert_cover_page(doc: &mut letter_model::Document, cover: &CoverPage) -> Position {
    let Some(section) = doc.sections.first_mut() else {
        return Position::new(0, 0);
    };
    let blocks = cover_blocks(cover);
    section.blocks.splice(0..0, blocks);
    Position::new(0, 0)
}

/// Абзацы титульной страницы и абзац с разрывом страницы после них.
fn cover_blocks(cover: &CoverPage) -> Vec<Block> {
    let centered = cover.layout == CoverLayout::Band;
    let align = centered.then_some(Alignment::Center);
    let side_line = (cover.layout == CoverLayout::Side).then(|| ParagraphBorders {
        left: Some(Border {
            color: Some(ACCENT),
            space: Some(12),
            ..Border::single(48)
        }),
        ..ParagraphBorders::default()
    });
    let side_indent = Indentation {
        left: side_line.as_ref().map(|_| Twips(240)),
        ..Indentation::default()
    };

    let band = cover.layout == CoverLayout::Band;
    let title_color = if band {
        Color::Rgb(0xFF, 0xFF, 0xFF)
    } else {
        ACCENT
    };
    let mut title = paragraph(&cover.title, 72, true, title_color);
    title.props.alignment = align;
    title.props.spacing = spacing(3600, 120);
    title.props.indentation = side_indent;
    title.props.borders = side_line.clone();
    if band {
        title.props.shading = Some(Shading::fill(ACCENT));
        title.props.spacing = spacing(3600, 0);
    }

    let mut subtitle = paragraph(&cover.subtitle, 32, false, GRAY);
    subtitle.props.alignment = align;
    subtitle.props.spacing = spacing(if centered { 240 } else { 0 }, 0);
    subtitle.props.indentation = side_indent;
    subtitle.props.borders = side_line;
    if centered {
        subtitle.props.borders = Some(ParagraphBorders {
            bottom: Some(Border {
                color: Some(ACCENT),
                space: Some(6),
                ..Border::single(12)
            }),
            ..ParagraphBorders::default()
        });
    }

    let mut author = paragraph(&cover.author, 28, false, ACCENT);
    author.props.alignment = align;
    author.props.spacing = spacing(4800, 0);
    let mut date = paragraph(&cover.date, 28, false, GRAY);
    date.props.alignment = align;
    date.props.spacing = spacing(0, 0);

    let page_break = Paragraph::from_runs([Run {
        props: RunProps::default(),
        content: vec![RunContent::Break(BreakKind::Page)],
    }]);
    [title, subtitle, author, date, page_break]
        .into_iter()
        .map(Block::from)
        .collect()
}

/// Абзац с текстом размера `half_points` / 2 пт цвета `color`.
fn paragraph(text: &str, half_points: u32, bold: bool, color: Color) -> Paragraph {
    let run = Run {
        props: RunProps {
            size: Some(HalfPoints(half_points)),
            bold: bold.then_some(true),
            color: Some(color),
            ..RunProps::default()
        },
        content: vec![RunContent::Text(text.to_string())],
    };
    let mut paragraph = Paragraph::from_runs([run]);
    paragraph.props = ParagraphProps::default();
    paragraph
}

/// Интервалы до и после, твипы; строки одинарные.
fn spacing(before: i32, after: i32) -> Spacing {
    Spacing {
        before: Some(Twips(before)),
        after: Some(Twips(after)),
        line: Some(letter_model::LineSpacing::Multiple(240)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::plain_text;
    use crate::history::{Command, History};
    use crate::selection::Selection;
    use letter_model::Document;

    fn cover(layout: CoverLayout) -> CoverPage {
        CoverPage {
            layout,
            title: "Отчёт".into(),
            subtitle: "За год".into(),
            author: "Никита".into(),
            date: "22.10.2026".into(),
        }
    }

    fn texts(doc: &Document) -> Vec<String> {
        doc.sections[0]
            .blocks
            .iter()
            .map(|b| match b {
                Block::Paragraph(p) => plain_text(p),
                _ => String::new(),
            })
            .collect()
    }

    #[test]
    fn cover_page_goes_first_and_can_be_undone() {
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![Paragraph::text("Текст").into()];
        let original = doc.clone();
        let mut history = History::new();
        let caret = Selection::caret(Position::new(0, 3));
        let after = history.run(
            &mut doc,
            caret,
            Command::InsertCoverPage(Box::new(cover(CoverLayout::Plain))),
        );
        assert_eq!(after, Selection::caret(Position::new(0, 0)));
        assert_eq!(
            texts(&doc),
            ["Отчёт", "За год", "Никита", "22.10.2026", "\u{c}", "Текст"]
        );
        history.undo(&mut doc);
        assert_eq!(doc, original);
        // Выделение не стирается.
        let selection = Selection {
            anchor: Position::new(0, 0),
            focus: Position::new(0, 5),
        };
        history.run(
            &mut doc,
            selection,
            Command::InsertCoverPage(Box::new(cover(CoverLayout::Side))),
        );
        assert_eq!(texts(&doc).last().map(String::as_str), Some("Текст"));
    }

    #[test]
    fn blank_page_is_two_page_breaks() {
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![Paragraph::text("ab").into()];
        let mut history = History::new();
        let after = history.run(
            &mut doc,
            Selection::caret(Position::new(0, 1)),
            Command::BlankPage,
        );
        assert_eq!(texts(&doc), ["a\u{c}\u{c}b"]);
        assert_eq!(after, Selection::caret(Position::new(0, 3)));
        history.undo(&mut doc);
        assert_eq!(texts(&doc), ["ab"]);
    }

    #[test]
    fn layouts_differ() {
        let band = cover_blocks(&cover(CoverLayout::Band));
        let Block::Paragraph(title) = &band[0] else {
            panic!("нет названия");
        };
        assert_eq!(title.props.alignment, Some(Alignment::Center));
        assert!(title.props.shading.is_some());
        let side = cover_blocks(&cover(CoverLayout::Side));
        let Block::Paragraph(title) = &side[0] else {
            panic!("нет названия");
        };
        assert!(
            title
                .props
                .borders
                .as_ref()
                .is_some_and(|b| b.left.is_some())
        );
        let plain = cover_blocks(&cover(CoverLayout::Plain));
        let Block::Paragraph(title) = &plain[0] else {
            panic!("нет названия");
        };
        assert!(title.props.borders.is_none() && title.props.shading.is_none());
    }
}
