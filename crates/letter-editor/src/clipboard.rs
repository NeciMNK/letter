//! Кусок документа для буфера обмена: копирование выделенного
//! и вставка — куска с оформлением или простого текста.
//!
//! Кусок — обычный [`Document`] с одним разделом и стилями источника.
//! Последний абзац куска — без знака абзаца (его оформление при вставке
//! не переносится), остальные — целые, как при копировании в Word.

use letter_model::{Block, Document, Paragraph};

use crate::edit::{
    blocks_mut, clamp_offset, find, insert_text, split_content, split_paragraph, text_len, tidy,
};
use crate::position::Position;

/// Копирует текст от `start` до `end` (порядок неважен). Таблица,
/// выделенная целиком, копируется таблицей, а выделенная частично —
/// абзацами своих ячеек.
pub fn copy_range(doc: &Document, start: Position, end: Position) -> Document {
    let (start, end) = if (start.paragraph, start.offset) <= (end.paragraph, end.offset) {
        (start, end)
    } else {
        (end, start)
    };
    let mut copier = Copier {
        start,
        end,
        next: 0,
    };
    let mut blocks = Vec::new();
    for section in &doc.sections {
        blocks.extend(copier.blocks(&section.blocks));
    }
    let mut fragment = Document::new();
    fragment.styles = doc.styles.clone();
    fragment.settings = doc.settings.clone();
    if blocks.is_empty() {
        blocks.push(Paragraph::default().into());
    }
    crate::comments::strip_comment_marks(&mut blocks);
    fragment.sections[0].blocks = blocks;
    fragment
}

struct Copier {
    start: Position,
    end: Position,
    /// Номер следующего абзаца в документе.
    next: usize,
}

impl Copier {
    fn blocks(&mut self, blocks: &[Block]) -> Vec<Block> {
        let mut out = Vec::new();
        for block in blocks {
            match block {
                Block::Paragraph(p) => {
                    let index = self.next;
                    self.next += 1;
                    if index < self.start.paragraph || index > self.end.paragraph {
                        continue;
                    }
                    let mut p = p.clone();
                    if index == self.end.paragraph {
                        let to = clamp_offset(&p, self.end.offset);
                        split_content(&mut p, to);
                    }
                    if index == self.start.paragraph {
                        let from = clamp_offset(&p, self.start.offset);
                        p.content = split_content(&mut p, from);
                        tidy(&mut p);
                    }
                    out.push(p.into());
                }
                Block::Table(table) => {
                    let first = self.next;
                    let count = count_paragraphs(block);
                    let last = first + count.saturating_sub(1);
                    let whole = (self.start.paragraph, self.start.offset) <= (first, 0)
                        && self.end.paragraph > last;
                    if count == 0 || last < self.start.paragraph || first > self.end.paragraph {
                        self.next += count;
                    } else if whole {
                        self.next += count;
                        out.push(Block::Table(table.clone()));
                    } else {
                        for row in &table.rows {
                            for cell in &row.cells {
                                let inner = self.blocks(&cell.blocks);
                                out.extend(inner);
                            }
                        }
                    }
                }
                Block::Unknown(_) => {}
            }
        }
        out
    }
}

/// Сколько абзацев в блоке (с абзацами ячеек).
fn count_paragraphs(block: &Block) -> usize {
    match block {
        Block::Paragraph(_) => 1,
        Block::Table(t) => t
            .rows
            .iter()
            .flat_map(|r| &r.cells)
            .flat_map(|c| &c.blocks)
            .map(count_paragraphs)
            .sum(),
        Block::Unknown(_) => 0,
    }
}

/// Вставляет кусок `fragment` на место курсора и возвращает позицию
/// после вставленного. Как в Word: если в куске несколько абзацев,
/// абзац с курсором до места вставки получает оформление первого
/// абзаца куска, а последний — остаётся со своим. Стили, которых нет
/// в документе, переносятся из куска.
pub fn insert_fragment(doc: &mut Document, at: Position, fragment: &Document) -> Position {
    for style in &fragment.styles.styles {
        if doc.styles.get(&style.id).is_none() {
            doc.styles.styles.push(style.clone());
        }
    }
    let mut blocks: Vec<Block> = fragment
        .sections
        .iter()
        .flat_map(|s| &s.blocks)
        .filter(|b| !matches!(b, Block::Unknown(_)))
        .cloned()
        .collect();
    let Some(path) = find(doc, at.paragraph) else {
        return at;
    };
    let Some(container) = blocks_mut(doc, &path) else {
        return at;
    };
    let Some(Block::Paragraph(target)) = container.get_mut(path.block) else {
        return at;
    };
    let offset = clamp_offset(target, at.offset);

    // Один абзац — просто текст с оформлением внутрь абзаца.
    if let [Block::Paragraph(only)] = blocks.as_slice() {
        let added = text_len(only);
        let tail = split_content(target, offset);
        target.content.extend(only.content.iter().cloned());
        target.content.extend(tail);
        tidy(target);
        return Position::new(at.paragraph, offset + added);
    }
    let Some(last) = blocks.pop() else {
        return Position::new(at.paragraph, offset);
    };
    let tail = split_content(target, offset);
    let original = target.props.clone();
    if let Some(Block::Paragraph(first)) = blocks.first() {
        target.props = first.props.clone();
        target.content.extend(first.content.iter().cloned());
        tidy(target);
        blocks.remove(0);
    }
    let (closing, cursor) = match last {
        Block::Paragraph(p) => {
            let len = text_len(&p);
            let mut content = p.content;
            content.extend(tail);
            (
                Paragraph {
                    props: original,
                    content,
                },
                len,
            )
        }
        other => {
            blocks.push(other);
            (
                Paragraph {
                    props: original,
                    content: tail,
                },
                0,
            )
        }
    };
    let mut closing = closing;
    // «С новой страницы» остаётся у первой части, как при Enter.
    closing.props.page_break_before = None;
    tidy(&mut closing);
    let count = blocks.iter().map(count_paragraphs).sum::<usize>();
    blocks.push(closing.into());
    let place = path.block + 1;
    container.splice(place..place, blocks);
    Position::new(at.paragraph + 1 + count, cursor)
}

/// Вставляет простой текст: строки — абзацы. Текст получает
/// оформление места вставки, как набранный.
pub fn insert_plain_text(doc: &mut Document, at: Position, text: &str) -> Position {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut position = at;
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            position = split_paragraph(doc, position);
        }
        position = insert_text(doc, position, line);
    }
    position
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::paragraph_mut;
    use letter_model::{
        ParagraphProps, Run, RunProps, Style, StyleKind, Table, TableCell, TableRow,
    };

    fn bold() -> RunProps {
        RunProps {
            bold: Some(true),
            ..RunProps::default()
        }
    }

    fn doc(paragraphs: Vec<Paragraph>) -> Document {
        let mut doc = Document::new();
        doc.sections[0].blocks = paragraphs.into_iter().map(Block::from).collect();
        doc
    }

    fn texts(doc: &Document) -> Vec<String> {
        let mut doc = doc.clone();
        (0..)
            .map_while(|i| paragraph_mut(&mut doc, i).map(|p| p.plain_text()))
            .collect()
    }

    fn styled(text: &str, style: &str) -> Paragraph {
        Paragraph::text(text).with_props(ParagraphProps {
            style: Some(style.into()),
            ..ParagraphProps::default()
        })
    }

    #[test]
    fn copy_keeps_formatting_and_trims_edges() {
        let d = doc(vec![
            Paragraph::from_runs([Run::text("ab"), Run::text("cd").with_props(bold())]),
            styled("middle", "Heading1"),
            Paragraph::text("end"),
        ]);
        let part = copy_range(&d, Position::new(0, 1), Position::new(0, 3));
        assert_eq!(texts(&part), vec!["bc"]);
        let Some(Block::Paragraph(p)) = part.sections[0].blocks.first() else {
            panic!()
        };
        assert_eq!(p.runs().nth(1).map(|r| r.props.bold), Some(Some(true)));

        let part = copy_range(&d, Position::new(2, 2), Position::new(0, 3));
        assert_eq!(texts(&part), vec!["d", "middle", "en"]);
        assert_eq!(part.plain_text(), "d\nmiddle\nen");
        // Пустое выделение — пустой абзац.
        let part = copy_range(&d, Position::new(1, 2), Position::new(1, 2));
        assert_eq!(texts(&part), vec![""]);
    }

    #[test]
    fn tables_are_copied_whole_or_by_cells() {
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
        d.sections[0].blocks = vec![
            Paragraph::text("x").into(),
            table.into(),
            Paragraph::text("y").into(),
        ];
        let all = copy_range(&d, Position::new(0, 0), Position::new(3, 1));
        assert!(matches!(all.sections[0].blocks[1], Block::Table(_)));
        assert_eq!(all.sections[0].blocks.len(), 3);
        let part = copy_range(&d, Position::new(2, 0), Position::new(3, 1));
        assert_eq!(part.sections[0].blocks.len(), 2);
        assert_eq!(texts(&part), vec!["B", "y"]);
    }

    #[test]
    fn pasting_one_paragraph_goes_inline() {
        let source = doc(vec![Paragraph::from_runs([
            Run::text("XY").with_props(bold())
        ])]);
        let mut d = doc(vec![Paragraph::text("abc")]);
        let p = insert_fragment(&mut d, Position::new(0, 1), &source);
        assert_eq!(p, Position::new(0, 3));
        assert_eq!(texts(&d), vec!["aXYbc"]);
        let Some(Block::Paragraph(par)) = d.sections[0].blocks.first() else {
            panic!()
        };
        assert_eq!(par.runs().count(), 3);
        assert_eq!(par.runs().nth(1).map(|r| r.props.bold), Some(Some(true)));
    }

    #[test]
    fn pasting_paragraphs_splits_the_target() {
        let mut heading = Style::new("Heading1", StyleKind::Paragraph);
        heading.name = Some("heading 1".into());
        let mut source = doc(vec![
            styled("Title", "Heading1"),
            Paragraph::text("body"),
            Paragraph::text("tail"),
        ]);
        source.styles.styles.push(heading);
        let part = copy_range(&source, Position::new(0, 0), Position::new(2, 2));

        let mut d = doc(vec![styled("1234", "Quote")]);
        let p = insert_fragment(&mut d, Position::new(0, 2), &part);
        assert_eq!(texts(&d), vec!["12Title", "body", "ta34"]);
        assert_eq!(p, Position::new(2, 2));
        let style = |i: usize| match &d.sections[0].blocks[i] {
            Block::Paragraph(p) => p.props.style.clone(),
            _ => None,
        };
        // Первый абзац — оформление куска, последний — своё.
        assert_eq!(style(0).as_deref(), Some("Heading1"));
        assert_eq!(style(2).as_deref(), Some("Quote"));
        assert!(d.styles.get("Heading1").is_some());
    }

    #[test]
    fn pasting_a_table() {
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
        let mut source = Document::new();
        source.sections[0].blocks = vec![
            Paragraph::text("x").into(),
            table.into(),
            Paragraph::text("y").into(),
        ];
        let part = copy_range(&source, Position::new(0, 0), Position::new(3, 0));
        let mut d = doc(vec![Paragraph::text("12")]);
        let p = insert_fragment(&mut d, Position::new(0, 1), &part);
        assert_eq!(texts(&d), vec!["1x", "A", "B", "2"]);
        assert_eq!(p, Position::new(3, 0));
        assert!(matches!(d.sections[0].blocks[1], Block::Table(_)));
    }

    #[test]
    fn plain_text_becomes_paragraphs() {
        let mut d = doc(vec![Paragraph::from_runs([
            Run::text("ab").with_props(bold())
        ])]);
        let p = insert_plain_text(&mut d, Position::new(0, 1), "1\r\n2\t3\n");
        assert_eq!(texts(&d), vec!["a1", "2\t3", "b"]);
        assert_eq!(p, Position::new(2, 0));
        // Вставленное — с оформлением места вставки.
        let Some(Block::Paragraph(second)) = d.sections[0].blocks.get(1) else {
            panic!()
        };
        assert!(second.runs().all(|r| r.props.bold == Some(true)));
    }
}
