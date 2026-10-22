//! Правка текста: ввод букв, Enter, Shift+Enter, Tab, Backspace, Delete.
//!
//! Все функции меняют модель документа и возвращают новую позицию курсора.
//! Абзацы нумеруются так же, как в [`Position`]: по порядку во всём
//! документе, абзацы ячеек таблиц тоже. Смещение — байты в тексте абзаца,
//! где табуляция, разрыв строки и разрыв страницы — по одному знаку.

use letter_model::{Block, BreakKind, Document, Inline, Paragraph, Run, RunContent};
use unicode_segmentation::UnicodeSegmentation;

use crate::lists::{list_item, remove_from_list};
use crate::position::Position;

/// Вставляет текст на место курсора. Знак `\t` становится табуляцией,
/// `\n` — разрывом строки, остальные управляющие знаки пропускаются.
/// Новый текст получает оформление буквы перед курсором (в начале абзаца —
/// первой буквы), как в Word.
pub fn insert_text(doc: &mut Document, at: Position, text: &str) -> Position {
    let Some(paragraph) = paragraph_mut(doc, at.paragraph) else {
        return at;
    };
    let offset = clamp_offset(paragraph, at.offset);
    let mut items = Vec::new();
    let mut plain = String::new();
    for c in text.chars() {
        let special = match c {
            '\t' => Some(RunContent::Tab),
            '\n' | '\u{b}' => Some(RunContent::Break(BreakKind::Line)),
            c if c.is_control() => continue,
            _ => None,
        };
        match special {
            Some(item) => {
                if !plain.is_empty() {
                    items.push(RunContent::Text(std::mem::take(&mut plain)));
                }
                items.push(item);
            }
            None => plain.push(c),
        }
    }
    if !plain.is_empty() {
        items.push(RunContent::Text(plain));
    }
    let added: usize = items.iter().map(item_len).sum();
    insert_items(paragraph, offset, items);
    Position::new(at.paragraph, offset + added)
}

/// Shift+Enter: разрыв строки в том же абзаце.
pub fn insert_line_break(doc: &mut Document, at: Position) -> Position {
    insert_text(doc, at, "\n")
}

/// Enter: делит абзац на два. Новый абзац получает оформление прежнего,
/// а если Enter нажат в конце абзаца со стилем, у которого задан
/// «стиль следующего абзаца» (после заголовка — «Обычный»), — этот стиль.
/// Новый пункт списка продолжает список; Enter в пустом пункте
/// заканчивает список.
pub fn split_paragraph(doc: &mut Document, at: Position) -> Position {
    let Some(path) = find(doc, at.paragraph) else {
        return at;
    };
    let empty = paragraph_ref(doc, at.paragraph).is_some_and(|p| text_len(p) == 0);
    if empty && list_item(doc, at.paragraph).is_some() {
        remove_from_list(doc, at.paragraph, false);
        return at;
    }
    let next_style = {
        let Some(paragraph) = paragraph_mut(doc, at.paragraph) else {
            return at;
        };
        let offset = clamp_offset(paragraph, at.offset);
        let at_end = offset == text_len(paragraph);
        let style = paragraph.props.style.clone();
        (at_end, style, offset)
    };
    let (at_end, style, offset) = next_style;
    let next = style
        .as_deref()
        .and_then(|id| doc.styles.get(id))
        .and_then(|s| s.next.clone())
        .filter(|_| at_end);
    let Some(blocks) = blocks_mut(doc, &path) else {
        return at;
    };
    let Some(Block::Paragraph(paragraph)) = blocks.get_mut(path.block) else {
        return at;
    };
    let mut tail = Paragraph {
        props: paragraph.props.clone(),
        content: split_content(paragraph, offset),
    };
    if let Some(next) = next {
        tail.props.style = Some(next);
    }
    // Разрыв страницы перед абзацем остаётся только у первой части.
    tail.props.page_break_before = None;
    blocks.insert(path.block + 1, tail.into());
    Position::new(at.paragraph + 1, 0)
}

/// Backspace: удаляет букву перед курсором, а в начале абзаца
/// соединяет абзац с предыдущим.
pub fn delete_backward(doc: &mut Document, at: Position) -> Position {
    let Some(paragraph) = paragraph_mut(doc, at.paragraph) else {
        return at;
    };
    let offset = clamp_offset(paragraph, at.offset);
    // В начале пункта списка Backspace сначала убирает номер.
    if offset == 0 && list_item(doc, at.paragraph).is_some() {
        remove_from_list(doc, at.paragraph, true);
        return at;
    }
    let Some(paragraph) = paragraph_mut(doc, at.paragraph) else {
        return at;
    };
    if offset > 0 {
        let text = plain_text(paragraph);
        let start = text[..offset]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i);
        remove_range(paragraph, start, offset);
        return Position::new(at.paragraph, start);
    }
    match at.paragraph.checked_sub(1) {
        Some(previous) => merge_with_next(doc, previous).unwrap_or(at),
        None => at,
    }
}

/// Delete: удаляет букву после курсора, а в конце абзаца присоединяет
/// следующий абзац.
pub fn delete_forward(doc: &mut Document, at: Position) -> Position {
    let Some(paragraph) = paragraph_mut(doc, at.paragraph) else {
        return at;
    };
    let offset = clamp_offset(paragraph, at.offset);
    let text = plain_text(paragraph);
    if offset < text.len() {
        let end = text[offset..]
            .grapheme_indices(true)
            .nth(1)
            .map_or(text.len(), |(i, _)| offset + i);
        remove_range(paragraph, offset, end);
        return Position::new(at.paragraph, offset);
    }
    merge_with_next(doc, at.paragraph).unwrap_or(Position::new(at.paragraph, offset))
}

/// Удаляет текст от `start` до `end` (выделение; порядок неважен)
/// и возвращает позицию курсора — начало удалённого. Абзацы целиком
/// внутри удаляются, первый и последний соединяются. Абзацы в ячейках
/// таблиц только очищаются: строки и ячейки остаются.
pub fn delete_range(doc: &mut Document, start: Position, end: Position) -> Position {
    let (start, end) = if (start.paragraph, start.offset) <= (end.paragraph, end.offset) {
        (start, end)
    } else {
        (end, start)
    };
    let Some(first) = paragraph_mut(doc, start.paragraph) else {
        return start;
    };
    let from = clamp_offset(first, start.offset);
    if start.paragraph == end.paragraph {
        let to = clamp_offset(first, end.offset).max(from);
        remove_range(first, from, to);
        return Position::new(start.paragraph, from);
    }
    let len = text_len(first);
    remove_range(first, from, len);
    for index in start.paragraph + 1..=end.paragraph {
        let Some(paragraph) = paragraph_mut(doc, index) else {
            break;
        };
        let to = if index == end.paragraph {
            clamp_offset(paragraph, end.offset)
        } else {
            text_len(paragraph)
        };
        remove_range(paragraph, 0, to);
    }
    // Соединяем с конца, чтобы номера ещё не тронутых абзацев не менялись.
    for index in (start.paragraph..end.paragraph).rev() {
        merge_with_next(doc, index);
    }
    Position::new(start.paragraph, from)
}

/// Присоединяет к абзацу `index` следующий. Работает, только если
/// следующий абзац стоит сразу за ним в том же месте (разделе, ячейке);
/// иначе (между ними таблица, граница ячейки) ничего не делает.
fn merge_with_next(doc: &mut Document, index: usize) -> Option<Position> {
    let first = find(doc, index)?;
    let second = find(doc, index + 1)?;
    if first.section != second.section
        || first.cells != second.cells
        || first.block + 1 != second.block
    {
        return None;
    }
    let blocks = blocks_mut(doc, &first)?;
    let Block::Paragraph(next) = blocks.remove(second.block) else {
        return None;
    };
    let Some(Block::Paragraph(paragraph)) = blocks.get_mut(first.block) else {
        return None;
    };
    let join = text_len(paragraph);
    // У пустого абзаца остаётся оформление присоединённого.
    if join == 0 {
        paragraph.props = next.props;
        paragraph.content.retain(|i| !is_empty_run(i));
    }
    paragraph.content.extend(next.content);
    tidy(paragraph);
    Some(Position::new(index, join))
}

/// Где абзац лежит в документе.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Path {
    pub(crate) section: usize,
    /// Ячейки по пути: (блок таблицы, строка, ячейка).
    pub(crate) cells: Vec<(usize, usize, usize)>,
    /// Номер блока в своём списке.
    pub(crate) block: usize,
}

pub(crate) fn find(doc: &Document, index: usize) -> Option<Path> {
    fn walk(
        blocks: &[Block],
        cells: &mut Vec<(usize, usize, usize)>,
        section: usize,
        left: &mut usize,
    ) -> Option<Path> {
        for (b, block) in blocks.iter().enumerate() {
            match block {
                Block::Paragraph(_) => {
                    if *left == 0 {
                        return Some(Path {
                            section,
                            cells: cells.clone(),
                            block: b,
                        });
                    }
                    *left -= 1;
                }
                Block::Table(table) => {
                    for (r, row) in table.rows.iter().enumerate() {
                        for (c, cell) in row.cells.iter().enumerate() {
                            cells.push((b, r, c));
                            let found = walk(&cell.blocks, cells, section, left);
                            cells.pop();
                            if found.is_some() {
                                return found;
                            }
                        }
                    }
                }
                Block::Unknown(_) => {}
            }
        }
        None
    }
    let mut left = index;
    doc.sections
        .iter()
        .enumerate()
        .find_map(|(s, section)| walk(&section.blocks, &mut Vec::new(), s, &mut left))
}

/// Раздел и номер блока верхнего уровня (абзаца или таблицы раздела),
/// в котором лежит абзац `index`.
pub(crate) fn top_level(doc: &Document, index: usize) -> Option<(usize, usize)> {
    let path = find(doc, index)?;
    let block = path.cells.first().map_or(path.block, |&(b, _, _)| b);
    Some((path.section, block))
}

pub(crate) fn blocks_mut<'a>(doc: &'a mut Document, path: &Path) -> Option<&'a mut Vec<Block>> {
    let mut blocks = &mut doc.sections.get_mut(path.section)?.blocks;
    for &(b, r, c) in &path.cells {
        let Some(Block::Table(table)) = blocks.get_mut(b) else {
            return None;
        };
        blocks = &mut table.rows.get_mut(r)?.cells.get_mut(c)?.blocks;
    }
    Some(blocks)
}

/// Абзац номер `index` для чтения.
pub(crate) fn paragraph_ref(doc: &Document, index: usize) -> Option<&Paragraph> {
    let path = find(doc, index)?;
    let mut blocks = &doc.sections.get(path.section)?.blocks;
    for &(b, r, c) in &path.cells {
        let Some(Block::Table(table)) = blocks.get(b) else {
            return None;
        };
        blocks = &table.rows.get(r)?.cells.get(c)?.blocks;
    }
    match blocks.get(path.block)? {
        Block::Paragraph(p) => Some(p),
        _ => None,
    }
}

/// Абзац номер `index` (по порядку во всём документе).
pub fn paragraph_mut(doc: &mut Document, index: usize) -> Option<&mut Paragraph> {
    let path = find(doc, index)?;
    match blocks_mut(doc, &path)?.get_mut(path.block)? {
        Block::Paragraph(p) => Some(p),
        _ => None,
    }
}

/// Длина элемента в тексте абзаца.
fn item_len(item: &RunContent) -> usize {
    item.text_len()
}

pub(crate) fn plain_text(paragraph: &Paragraph) -> String {
    let mut text = String::new();
    for run in paragraph.runs() {
        for item in &run.content {
            match item {
                RunContent::Text(t) => text.push_str(t),
                RunContent::Tab => text.push('\t'),
                RunContent::Break(BreakKind::Line) => text.push('\n'),
                RunContent::Break(_) => text.push('\u{c}'),
                RunContent::Picture(_) => text.push(letter_model::OBJECT_CHAR),
                RunContent::FieldChar(_) | RunContent::FieldCode(_) | RunContent::Unknown(_) => {}
            }
        }
    }
    text
}

/// Сколько абзацев в документе (вместе с абзацами таблиц).
pub fn paragraph_count(doc: &Document) -> usize {
    (0..)
        .take_while(|&i| paragraph_ref(doc, i).is_some())
        .count()
}

pub(crate) fn text_len(paragraph: &Paragraph) -> usize {
    paragraph
        .runs()
        .flat_map(|r| &r.content)
        .map(item_len)
        .sum()
}

/// Смещение внутри текста абзаца и на границе букв.
pub(crate) fn clamp_offset(paragraph: &Paragraph, offset: usize) -> usize {
    let text = plain_text(paragraph);
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// Вставляет элементы в позицию `offset`, во фрагмент с буквой перед ней.
pub(crate) fn insert_items(paragraph: &mut Paragraph, offset: usize, items: Vec<RunContent>) {
    if items.is_empty() {
        return;
    }
    // Ищем фрагмент и место в нём: (номер элемента абзаца, номер элемента
    // фрагмента, смещение в тексте элемента).
    let mut pos = 0;
    let mut target: Option<(usize, usize, usize)> = None;
    'outer: for (i, inline) in paragraph.content.iter().enumerate() {
        let Inline::Run(run) = inline else { continue };
        if target.is_none() {
            // В начале абзаца — в начало первого фрагмента.
            target = Some((i, 0, 0));
        }
        for (k, item) in run.content.iter().enumerate() {
            let len = item_len(item);
            if offset > pos && offset <= pos + len {
                target = Some((i, k, offset - pos));
                break 'outer;
            }
            if offset == pos && offset == 0 {
                break 'outer;
            }
            pos += len;
        }
        if offset == pos && pos > 0 {
            // Сразу за этим фрагментом (он кончается перед курсором).
            target = Some((i, run.content.len(), 0));
            break;
        }
    }
    let Some((i, k, inner)) = target else {
        paragraph.content.push(Inline::Run(Run {
            props: Default::default(),
            content: items,
        }));
        tidy(paragraph);
        return;
    };
    let Some(Inline::Run(run)) = paragraph.content.get_mut(i) else {
        return;
    };
    // Внутри текста или в его конце — режем текст.
    let index = match run.content.get_mut(k) {
        Some(RunContent::Text(t)) if inner > 0 => {
            let rest = t.split_off(inner);
            if rest.is_empty() {
                k + 1
            } else {
                run.content.insert(k + 1, RunContent::Text(rest));
                k + 1
            }
        }
        Some(_) if inner > 0 => k + 1,
        _ => k,
    };
    for (n, item) in items.into_iter().enumerate() {
        run.content.insert(index + n, item);
    }
    tidy(paragraph);
}

/// Отрезает содержимое абзаца после `offset` и возвращает его.
pub(crate) fn split_content(paragraph: &mut Paragraph, offset: usize) -> Vec<Inline> {
    let mut head = Vec::new();
    let mut tail = Vec::new();
    let mut pos = 0;
    // Оформление последнего фрагмента до разреза: им будет печататься
    // текст в пустом новом абзаце.
    let mut last_props = None;
    for inline in std::mem::take(&mut paragraph.content) {
        let Inline::Run(run) = inline else {
            if pos < offset {
                head.push(inline);
            } else {
                tail.push(inline);
            }
            continue;
        };
        let mut left = Run {
            props: run.props.clone(),
            content: Vec::new(),
        };
        let mut right = Run {
            props: run.props.clone(),
            content: Vec::new(),
        };
        for item in run.content {
            let len = item_len(&item);
            if pos + len <= offset {
                left.content.push(item);
            } else if pos >= offset {
                right.content.push(item);
            } else if let RunContent::Text(mut t) = item {
                let rest = t.split_off(offset - pos);
                left.content.push(RunContent::Text(t));
                right.content.push(RunContent::Text(rest));
            }
            pos += len;
        }
        if !left.content.is_empty() || pos <= offset {
            last_props = Some(left.props.clone());
        }
        if !left.content.is_empty() {
            head.push(Inline::Run(left));
        }
        if !right.content.is_empty() {
            tail.push(Inline::Run(right));
        }
    }
    let has_runs = |items: &[Inline]| items.iter().any(|i| matches!(i, Inline::Run(_)));
    // Пустые части хранят оформление пустым фрагментом: набранный потом
    // текст получит его, как в Word.
    if !has_runs(&tail)
        && let Some(props) = last_props.clone()
    {
        tail.insert(
            0,
            Inline::Run(Run {
                props,
                content: Vec::new(),
            }),
        );
    }
    if !has_runs(&head)
        && let Some(Inline::Run(first)) = tail.iter().find(|i| matches!(i, Inline::Run(_)))
    {
        head.push(Inline::Run(Run {
            props: first.props.clone(),
            content: Vec::new(),
        }));
    }
    paragraph.content = head;
    tidy(paragraph);
    tail
}

/// Удаляет текст абзаца с `start` до `end`.
fn remove_range(paragraph: &mut Paragraph, start: usize, end: usize) {
    let mut pos = 0;
    for inline in &mut paragraph.content {
        let Inline::Run(run) = inline else { continue };
        run.content.retain_mut(|item| {
            let len = item_len(item);
            let (a, b) = (pos, pos + len);
            pos = b;
            if len == 0 || b <= start || a >= end {
                return true;
            }
            match item {
                RunContent::Text(t) => {
                    let from = start.max(a) - a;
                    let to = end.min(b) - a;
                    t.replace_range(from..to, "");
                    !t.is_empty()
                }
                _ => false,
            }
        });
    }
    tidy(paragraph);
}

fn is_empty_run(inline: &Inline) -> bool {
    matches!(inline, Inline::Run(r) if r.content.is_empty())
}

/// Наводит порядок: соседние тексты одного фрагмента склеены, пустые
/// фрагменты убраны (кроме одного, если весь абзац пуст: он хранит
/// оформление для набора), соседние фрагменты с одинаковым оформлением
/// склеены.
pub(crate) fn tidy(paragraph: &mut Paragraph) {
    let all_empty = paragraph
        .content
        .iter()
        .all(|i| !matches!(i, Inline::Run(_)) || is_empty_run(i));
    let mut kept_empty = false;
    paragraph.content.retain(|i| {
        if !is_empty_run(i) {
            return true;
        }
        let keep = all_empty && !kept_empty;
        kept_empty |= keep;
        keep
    });
    let mut merged: Vec<Inline> = Vec::with_capacity(paragraph.content.len());
    for inline in std::mem::take(&mut paragraph.content) {
        if let (Some(Inline::Run(last)), Inline::Run(run)) = (merged.last_mut(), &inline)
            && last.props == run.props
        {
            last.content.extend(run.content.iter().cloned());
            continue;
        }
        merged.push(inline);
    }
    for inline in &mut merged {
        if let Inline::Run(run) = inline {
            let mut content: Vec<RunContent> = Vec::with_capacity(run.content.len());
            for item in std::mem::take(&mut run.content) {
                match (content.last_mut(), item) {
                    (Some(RunContent::Text(a)), RunContent::Text(b)) => a.push_str(&b),
                    (_, RunContent::Text(b)) if b.is_empty() => {}
                    (_, item) => content.push(item),
                }
            }
            run.content = content;
        }
    }
    paragraph.content = merged;
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::{
        ParagraphProps, RawXml, RunProps, Style, StyleKind, Table, TableCell, TableRow,
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
        let mut out = Vec::new();
        let mut i = 0;
        let mut doc = doc.clone();
        while let Some(p) = paragraph_mut(&mut doc, i) {
            out.push(plain_text(p));
            i += 1;
        }
        out
    }

    #[test]
    fn typing_takes_formatting_of_the_letter_before() {
        let mut d = doc(vec![Paragraph::from_runs([
            Run::text("ab"),
            Run::text("cd").with_props(bold()),
        ])]);
        // Между «b» и «c»: оформление «b».
        let p = insert_text(&mut d, Position::new(0, 2), "X");
        assert_eq!(p, Position::new(0, 3));
        // В конце: оформление «d».
        insert_text(&mut d, Position::new(0, 5), "Y");
        // В начале: оформление первой буквы.
        insert_text(&mut d, Position::new(0, 0), "Ж");
        assert_eq!(texts(&d), vec!["ЖabXcdY"]);
        let Some(Block::Paragraph(p)) = d.sections[0].blocks.first() else {
            panic!()
        };
        let runs: Vec<String> = p.runs().map(Run::plain_text).collect();
        assert_eq!(runs, vec!["ЖabX", "cdY"]);
        assert_eq!(p.runs().nth(1).map(|r| r.props.bold), Some(Some(true)));
    }

    #[test]
    fn picture_is_one_letter() {
        let picture = letter_model::Picture {
            width: 12_700,
            height: 12_700,
            image: None,
            relationship: Some("rId1".into()),
            raw: None,
            anchor: None,
        };
        let mut d = doc(vec![Paragraph::from_runs([Run {
            props: Default::default(),
            content: vec![
                RunContent::Text("ab".into()),
                RunContent::Picture(picture),
                RunContent::Text("cd".into()),
            ],
        }])]);
        assert_eq!(texts(&d), vec!["ab\u{fffc}cd"]);
        // Текст за рисунком: после трёх байтов знака объекта.
        let p = insert_text(&mut d, Position::new(0, 5), "X");
        assert_eq!(p, Position::new(0, 6));
        assert_eq!(texts(&d), vec!["ab\u{fffc}Xcd"]);
        // Enter сразу за рисунком: рисунок остаётся в первом абзаце.
        let p = split_paragraph(&mut d, Position::new(0, 5));
        assert_eq!(texts(&d), vec!["ab\u{fffc}", "Xcd"]);
        // Backspace после рисунка удаляет его целиком.
        delete_backward(&mut d, Position::new(0, 5));
        assert_eq!(texts(&d), vec!["ab", "Xcd"]);
        assert_eq!(p, Position::new(1, 0));
    }

    #[test]
    fn tabs_breaks_and_empty_paragraphs() {
        let mut d = doc(vec![Paragraph::default()]);
        let p = insert_text(&mut d, Position::new(0, 0), "a\tb");
        let p = insert_line_break(&mut d, p);
        let p = insert_text(&mut d, p, "c\u{7}");
        assert_eq!(p, Position::new(0, 5));
        assert_eq!(texts(&d), vec!["a\tb\nc"]);
        let Some(Block::Paragraph(par)) = d.sections[0].blocks.first() else {
            panic!()
        };
        let content = &par
            .runs()
            .next()
            .map(|r| r.content.clone())
            .unwrap_or_default();
        assert!(content.contains(&RunContent::Tab));
        assert!(content.contains(&RunContent::Break(BreakKind::Line)));
        // Позиция за концом абзаца прижимается к концу.
        assert_eq!(
            insert_text(&mut d, Position::new(0, 99), "!"),
            Position::new(0, 6)
        );
        // Нет такого абзаца — ничего не меняется.
        assert_eq!(
            insert_text(&mut d, Position::new(5, 0), "x"),
            Position::new(5, 0)
        );
    }

    #[test]
    fn enter_splits_and_backspace_joins() {
        let mut d = doc(vec![Paragraph::from_runs([
            Run::text("Hello "),
            Run::text("world").with_props(bold()),
        ])]);
        let p = split_paragraph(&mut d, Position::new(0, 8));
        assert_eq!(p, Position::new(1, 0));
        assert_eq!(texts(&d), vec!["Hello wo", "rld"]);
        // Текст второй части — полужирный, как был.
        let Some(Block::Paragraph(second)) = d.sections[0].blocks.get(1) else {
            panic!()
        };
        assert_eq!(second.runs().next().map(|r| r.props.bold), Some(Some(true)));

        let p = delete_backward(&mut d, p);
        assert_eq!(p, Position::new(0, 8));
        assert_eq!(texts(&d), vec!["Hello world"]);
        let Some(Block::Paragraph(joined)) = d.sections[0].blocks.first() else {
            panic!()
        };
        assert_eq!(joined.runs().count(), 2);

        // Enter в конце: новый пустой абзац; набранный текст — полужирный.
        let p = split_paragraph(&mut d, Position::new(0, 11));
        assert_eq!(texts(&d), vec!["Hello world", ""]);
        insert_text(&mut d, p, "!");
        let Some(Block::Paragraph(last)) = d.sections[0].blocks.get(1) else {
            panic!()
        };
        assert_eq!(last.runs().next().map(|r| r.props.bold), Some(Some(true)));

        // Enter в начале: пустой абзац перед текстом.
        split_paragraph(&mut d, Position::new(0, 0));
        assert_eq!(texts(&d), vec!["", "Hello world", "!"]);
        // Backspace в начале документа ничего не делает.
        assert_eq!(
            delete_backward(&mut d, Position::new(0, 0)),
            Position::new(0, 0)
        );
    }

    #[test]
    fn delete_removes_whole_letters() {
        let mut d = doc(vec![Paragraph::text("e\u{301}x😀"), Paragraph::text("y")]);
        // «é» из двух кодов — одна буква.
        let p = delete_forward(&mut d, Position::new(0, 0));
        assert_eq!(p, Position::new(0, 0));
        assert_eq!(texts(&d), vec!["x😀", "y"]);
        let p = delete_backward(&mut d, Position::new(0, 5));
        assert_eq!(p, Position::new(0, 1));
        assert_eq!(texts(&d), vec!["x", "y"]);
        // Delete в конце абзаца присоединяет следующий.
        let p = delete_forward(&mut d, Position::new(0, 1));
        assert_eq!(p, Position::new(0, 1));
        assert_eq!(texts(&d), vec!["xy"]);
        // В конце документа — ничего.
        assert_eq!(
            delete_forward(&mut d, Position::new(0, 2)),
            Position::new(0, 2)
        );
        // Последнюю букву можно стереть: абзац пуст, но остаётся.
        delete_backward(&mut d, Position::new(0, 2));
        delete_backward(&mut d, Position::new(0, 1));
        assert_eq!(texts(&d), vec![""]);
    }

    #[test]
    fn heading_is_followed_by_its_next_style() {
        let mut heading = Style::new("Heading1", StyleKind::Paragraph);
        heading.next = Some("Normal".into());
        let mut d = doc(vec![Paragraph::text("Глава").with_props(ParagraphProps {
            style: Some("Heading1".into()),
            page_break_before: Some(true),
            ..ParagraphProps::default()
        })]);
        d.styles.styles.push(heading);
        split_paragraph(&mut d, Position::new(0, 10));
        let Some(Block::Paragraph(next)) = d.sections[0].blocks.get(1) else {
            panic!()
        };
        assert_eq!(next.props.style.as_deref(), Some("Normal"));
        assert_eq!(next.props.page_break_before, None);
        // Посередине — стиль тот же.
        split_paragraph(&mut d, Position::new(0, 8));
        let Some(Block::Paragraph(middle)) = d.sections[0].blocks.get(1) else {
            panic!()
        };
        assert_eq!(middle.props.style.as_deref(), Some("Heading1"));
        assert_eq!(texts(&d), vec!["Глав", "а", ""]);
    }

    #[test]
    fn deleting_a_selection() {
        let mut d = doc(vec![
            Paragraph::text("first"),
            Paragraph::from_runs([Run::text("sec"), Run::text("ond").with_props(bold())]),
            Paragraph::text("third"),
        ]);
        // Внутри абзаца, задом наперёд.
        let p = delete_range(&mut d, Position::new(1, 4), Position::new(1, 2));
        assert_eq!(p, Position::new(1, 2));
        assert_eq!(texts(&d), vec!["first", "send", "third"]);
        // Через три абзаца: «fi» + «rd».
        let p = delete_range(&mut d, Position::new(0, 2), Position::new(2, 3));
        assert_eq!(p, Position::new(0, 2));
        assert_eq!(texts(&d), vec!["fird"]);
        // Всё.
        let p = delete_range(&mut d, Position::new(0, 0), Position::new(0, 99));
        assert_eq!(p, Position::new(0, 0));
        assert_eq!(texts(&d), vec![""]);
    }

    #[test]
    fn deleting_across_a_table_keeps_its_cells() {
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
            Paragraph::text("до").into(),
            table.into(),
            Paragraph::text("после").into(),
        ];
        delete_range(&mut d, Position::new(0, 2), Position::new(3, 4));
        assert_eq!(texts(&d), vec!["д", "", "", "сле"]);
    }

    #[test]
    fn tables_and_unknown_content() {
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
        let mut before = Paragraph::text("до");
        before
            .content
            .push(Inline::Unknown(RawXml::new("<w:bookmarkEnd/>")));
        let mut d = Document::new();
        d.sections[0].blocks = vec![before.into(), table.into(), Paragraph::text("после").into()];
        // Абзацы: «до», «A», «B», «после».
        insert_text(&mut d, Position::new(2, 1), "2");
        let p = split_paragraph(&mut d, Position::new(1, 1));
        assert_eq!(p, Position::new(2, 0));
        assert_eq!(texts(&d), vec!["до", "A", "", "B2", "после"]);
        // Границы ячеек и таблицы Backspace не переходит.
        assert_eq!(
            delete_backward(&mut d, Position::new(3, 0)),
            Position::new(3, 0)
        );
        assert_eq!(
            delete_backward(&mut d, Position::new(4, 0)),
            Position::new(4, 0)
        );
        // Внутри ячейки — соединяет.
        assert_eq!(
            delete_backward(&mut d, Position::new(2, 0)),
            Position::new(1, 1)
        );
        assert_eq!(texts(&d), vec!["до", "A", "B2", "после"]);
        // Закладка в первом абзаце не потерялась.
        insert_text(&mut d, Position::new(0, 4), "!");
        let Some(Block::Paragraph(first)) = d.sections[0].blocks.first() else {
            panic!()
        };
        assert!(
            first
                .content
                .iter()
                .any(|i| matches!(i, Inline::Unknown(_)))
        );
        assert_eq!(texts(&d)[0], "до!");
    }
}
