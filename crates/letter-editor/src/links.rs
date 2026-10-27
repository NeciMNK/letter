//! Гиперссылки и закладки: ссылка у курсора, поставить и снять ссылку,
//! где закладка.

use letter_model::{
    Bookmark, Color, Document, FieldCharKind, Hyperlink, Inline, Paragraph, RunContent, RunProps,
    Underline,
};

use crate::edit::{paragraph_count, paragraph_mut, paragraph_ref, split_content, tidy};
use crate::format::apply;
use crate::position::Position;

/// Цвет ссылки, если в документе нет стиля «Гиперссылка».
const LINK_COLOR: Color = Color::Rgb(0x05, 0x63, 0xC1);

/// Ссылка у места `at` и где она: начало и конец (в одном абзаце).
/// Смотрим знак после места, а в конце ссылки — знак перед ним.
pub fn link_at(doc: &Document, at: Position) -> Option<(Hyperlink, Position, Position)> {
    let paragraph = paragraph_ref(doc, at.paragraph)?;
    let segments = segments(paragraph);
    let index = segments
        .iter()
        .position(|(a, b, _)| *a <= at.offset && at.offset < *b)
        .filter(|&i| segments[i].2.is_some())
        .or_else(|| {
            segments
                .iter()
                .position(|(a, b, link)| *a < at.offset && at.offset == *b && link.is_some())
        })?;
    let link = segments[index].2.clone()?;
    // Соседние куски с той же ссылкой — одна ссылка.
    let mut first = index;
    while first > 0 && segments[first - 1].2.as_ref() == Some(&link) {
        first -= 1;
    }
    let mut last = index;
    while segments
        .get(last + 1)
        .is_some_and(|s| s.2.as_ref() == Some(&link))
    {
        last += 1;
    }
    let start = Position::new(at.paragraph, segments[first].0);
    let end = Position::new(at.paragraph, segments[last].1);
    Some((link, start, end))
}

/// Куски текста абзаца с их ссылками: начало, конец (байты) и ссылка.
/// Пустые фрагменты пропускаются.
/// Результат поля `HYPERLINK` — тоже ссылка, как в Word.
fn segments(paragraph: &Paragraph) -> Vec<(usize, usize, Option<Hyperlink>)> {
    let mut out: Vec<(usize, usize, Option<Hyperlink>)> = Vec::new();
    let mut offset = 0;
    // Открытые поля: код и ссылка из него (после `separate`).
    let mut fields: Vec<(String, Option<Hyperlink>)> = Vec::new();
    for run in paragraph.runs() {
        for item in &run.content {
            match item {
                RunContent::FieldChar(c) => match c.kind {
                    FieldCharKind::Begin => fields.push((String::new(), None)),
                    FieldCharKind::Separate => {
                        if let Some((code, link)) = fields.last_mut() {
                            *link = Hyperlink::from_field_code(code);
                        }
                    }
                    FieldCharKind::End => {
                        fields.pop();
                    }
                },
                RunContent::FieldCode(text) => {
                    if let Some((code, None)) = fields.last_mut() {
                        code.push_str(text);
                    }
                }
                _ => {}
            }
            let len = item.text_len();
            if len == 0 {
                continue;
            }
            let link = run
                .props
                .link
                .clone()
                .or_else(|| fields.iter().rev().find_map(|(_, link)| link.clone()));
            match out.last_mut() {
                Some((_, end, last)) if *end == offset && *last == link => *end += len,
                _ => out.push((offset, offset + len, link)),
            }
            offset += len;
        }
    }
    out
}

/// Ставит ссылку `link` на текст от `start` до `end` (`None` — снимает).
/// Новая ссылка получает стиль «Гиперссылка», если он есть в документе,
/// иначе синий цвет с подчёркиванием; у снятой это оформление убирается.
pub fn set_link(doc: &mut Document, start: Position, end: Position, link: Option<&Hyperlink>) {
    let (start, end) = if (start.paragraph, start.offset) <= (end.paragraph, end.offset) {
        (start, end)
    } else {
        (end, start)
    };
    let style = hyperlink_style(doc);
    for index in start.paragraph..=end.paragraph {
        let Some(paragraph) = paragraph_mut(doc, index) else {
            continue;
        };
        let len: usize = paragraph
            .runs()
            .flat_map(|r| &r.content)
            .map(|c| c.text_len())
            .sum();
        let from = if index == start.paragraph {
            start.offset.min(len)
        } else {
            0
        };
        let to = if index == end.paragraph {
            end.offset.min(len)
        } else {
            len
        };
        if from >= to {
            continue;
        }
        apply(paragraph, from, to, |props| {
            mark(props, link, style.as_deref());
        });
    }
}

/// Ссылка и её оформление у одного фрагмента.
fn mark(props: &mut RunProps, link: Option<&Hyperlink>, style: Option<&str>) {
    match link {
        Some(link) => {
            if props.link.is_none() {
                match style {
                    Some(style) => props.style = Some(style.to_string()),
                    None => {
                        props.color = Some(LINK_COLOR);
                        props.underline = Some(Underline::Single);
                    }
                }
            }
            props.link = Some(link.clone());
        }
        None => {
            if props.link.take().is_some() {
                if style.is_some() && props.style.as_deref() == style {
                    props.style = None;
                }
                if props.color == Some(LINK_COLOR) {
                    props.color = None;
                }
                if props.underline == Some(Underline::Single) {
                    props.underline = None;
                }
            }
        }
    }
}

/// Идентификатор стиля символов «Hyperlink» («Гиперссылка»), если он
/// есть в документе.
fn hyperlink_style(doc: &Document) -> Option<String> {
    doc.styles
        .styles
        .iter()
        .find(|s| {
            s.kind == letter_model::StyleKind::Character
                && (s
                    .name
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case("hyperlink"))
                    || s.id.eq_ignore_ascii_case("hyperlink"))
        })
        .map(|s| s.id.clone())
}

/// Закладки документа по порядку: имя и где начинается.
pub fn bookmarks(doc: &Document) -> Vec<(String, Position)> {
    let mut out = Vec::new();
    for index in 0..paragraph_count(doc) {
        let Some(paragraph) = paragraph_ref(doc, index) else {
            continue;
        };
        let mut offset = 0;
        for inline in &paragraph.content {
            match inline {
                Inline::Run(run) => {
                    offset += run.content.iter().map(|c| c.text_len()).sum::<usize>()
                }
                Inline::BookmarkStart(bookmark) => {
                    out.push((bookmark.name.clone(), Position::new(index, offset)));
                }
                _ => {}
            }
        }
    }
    out
}

/// Где начинается закладка `name` (имена без учёта регистра, как в Word).
pub fn bookmark_position(doc: &Document, name: &str) -> Option<Position> {
    bookmark_range(doc, name).map(|(start, _)| start)
}

/// Начало и конец закладки `name`; без конца — пустая закладка на месте
/// начала.
pub fn bookmark_range(doc: &Document, name: &str) -> Option<(Position, Position)> {
    let mut start = None;
    for index in 0..paragraph_count(doc) {
        let Some(paragraph) = paragraph_ref(doc, index) else {
            continue;
        };
        let mut offset = 0;
        for inline in &paragraph.content {
            match inline {
                Inline::Run(run) => {
                    offset += run.content.iter().map(|c| c.text_len()).sum::<usize>()
                }
                Inline::BookmarkStart(b)
                    if start.is_none() && b.name.eq_ignore_ascii_case(name) =>
                {
                    start = Some((b.id.clone(), Position::new(index, offset)));
                }
                Inline::BookmarkEnd(id) => {
                    if let Some((wanted, from)) = &start
                        && wanted == id
                    {
                        return Some((*from, Position::new(index, offset)));
                    }
                }
                _ => {}
            }
        }
    }
    start.map(|(_, at)| (at, at))
}

/// Добавляет закладку `name` от `start` до `end`. Закладка с таким же
/// именем (без учёта регистра) переносится сюда, как в Word.
pub fn add_bookmark(doc: &mut Document, name: &str, start: Position, end: Position) {
    let (start, end) = if (start.paragraph, start.offset) <= (end.paragraph, end.offset) {
        (start, end)
    } else {
        (end, start)
    };
    delete_bookmark(doc, name);
    // Новый номер — больше всех номеров закладок документа.
    let mut next = 0;
    for index in 0..paragraph_count(doc) {
        let Some(paragraph) = paragraph_ref(doc, index) else {
            continue;
        };
        for inline in &paragraph.content {
            let id = match inline {
                Inline::BookmarkStart(b) => &b.id,
                Inline::BookmarkEnd(id) => id,
                _ => continue,
            };
            if let Ok(id) = id.parse::<u64>() {
                next = next.max(id + 1);
            }
        }
    }
    let id = next.to_string();
    insert_marker(doc, end, Inline::BookmarkEnd(id.clone()));
    let bookmark = Bookmark {
        id,
        name: name.to_string(),
    };
    insert_marker(doc, start, Inline::BookmarkStart(bookmark));
}

/// Убирает закладку `name` (начало и конец). `false` — такой нет.
pub fn delete_bookmark(doc: &mut Document, name: &str) -> bool {
    let mut id = None;
    for index in 0..paragraph_count(doc) {
        let Some(paragraph) = paragraph_mut(doc, index) else {
            continue;
        };
        paragraph.content.retain(|inline| match inline {
            Inline::BookmarkStart(b) if id.is_none() && b.name.eq_ignore_ascii_case(name) => {
                id = Some(b.id.clone());
                false
            }
            Inline::BookmarkEnd(end) => id.as_ref() != Some(end),
            _ => true,
        });
    }
    id.is_some()
}

/// Ставит отметку `marker` в место `at` (перед отметками, которые там
/// уже есть).
fn insert_marker(doc: &mut Document, at: Position, marker: Inline) {
    let Some(paragraph) = paragraph_mut(doc, at.paragraph) else {
        return;
    };
    let tail = split_content(paragraph, at.offset);
    paragraph.content.push(marker);
    paragraph.content.extend(tail);
    tidy(paragraph);
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::Run;

    fn doc() -> Document {
        let mut doc = Document::new();
        let mut link = Run::text("site");
        link.props.link = Some(Hyperlink::to("https://example.com"));
        let mut p = Paragraph::from_runs([Run::text("See "), link, Run::text(" now")]);
        p.content.insert(
            1,
            Inline::BookmarkStart(Bookmark {
                id: "1".into(),
                name: "Here".into(),
            }),
        );
        doc.sections[0].blocks = vec![p.into()];
        doc
    }

    #[test]
    fn hyperlink_field_is_a_link() {
        use letter_model::{FieldChar, RunContent};
        let char_run = |kind| Run {
            props: RunProps::default(),
            content: vec![RunContent::FieldChar(FieldChar::new(kind))],
        };
        let code = Run {
            props: RunProps::default(),
            content: vec![RunContent::FieldCode(
                r#" HYPERLINK "https://example.com" \o "Пример" "#.into(),
            )],
        };
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![
            Paragraph::from_runs([
                Run::text("ab "),
                char_run(FieldCharKind::Begin),
                code,
                char_run(FieldCharKind::Separate),
                Run::text("site"),
                char_run(FieldCharKind::End),
                Run::text(" c"),
            ])
            .into(),
        ];
        let (link, start, end) = link_at(&doc, Position::new(0, 4)).unwrap();
        assert_eq!(link.target.as_deref(), Some("https://example.com"));
        assert_eq!(link.tooltip.as_deref(), Some("Пример"));
        assert_eq!((start.offset, end.offset), (3, 7));
        assert!(link_at(&doc, Position::new(0, 1)).is_none());
        assert!(link_at(&doc, Position::new(0, 8)).is_none());
    }

    #[test]
    fn link_is_found_around_the_cursor() {
        let doc = doc();
        let found =
            |offset| link_at(&doc, Position::new(0, offset)).map(|(_, s, e)| (s.offset, e.offset));
        assert_eq!(found(3), None);
        assert_eq!(found(4), Some((4, 8)));
        assert_eq!(found(6), Some((4, 8)));
        // Сразу за ссылкой — тоже она.
        assert_eq!(found(8), Some((4, 8)));
        assert_eq!(found(9), None);
    }

    #[test]
    fn links_are_set_and_removed() {
        let mut doc = doc();
        let link = Hyperlink::to_bookmark("Here");
        set_link(
            &mut doc,
            Position::new(0, 9),
            Position::new(0, 12),
            Some(&link),
        );
        let (found, start, end) = link_at(&doc, Position::new(0, 10)).unwrap();
        assert_eq!(found, link);
        assert_eq!((start.offset, end.offset), (9, 12));
        let Some(letter_model::Block::Paragraph(p)) = doc.sections[0].blocks.first() else {
            panic!();
        };
        let run = p.runs().find(|r| r.plain_text() == "now").unwrap();
        assert_eq!(run.props.color, Some(LINK_COLOR));
        set_link(&mut doc, Position::new(0, 0), Position::new(0, 12), None);
        assert!(link_at(&doc, Position::new(0, 5)).is_none());
        let Some(letter_model::Block::Paragraph(p)) = doc.sections[0].blocks.first() else {
            panic!();
        };
        assert!(p.runs().all(|r| r.props == RunProps::default()));
        assert_eq!(p.plain_text(), "See site now");
    }

    #[test]
    fn link_command_inserts_and_removes() {
        use crate::{Command, History, Selection};
        let mut doc = doc();
        let before = doc.clone();
        let mut history = History::new();
        let at = Selection::caret(Position::new(0, 12));
        let command = Command::Link {
            link: Some(Box::new(Hyperlink::to("mailto:a@b.c"))),
            text: Some("!mail".into()),
        };
        let after = history.run(&mut doc, at, command);
        assert_eq!(after.focus, Position::new(0, 17));
        assert_eq!(doc.sections[0].blocks[0].plain_text(), "See site now!mail");
        let (link, start, _) = link_at(&doc, Position::new(0, 14)).unwrap();
        assert_eq!(link.address(), "mailto:a@b.c");
        assert_eq!(start.offset, 12);
        // Без выделения снимается вся ссылка у курсора.
        let remove = Command::Link {
            link: None,
            text: None,
        };
        history.run(&mut doc, Selection::caret(Position::new(0, 6)), remove);
        assert!(link_at(&doc, Position::new(0, 6)).is_none());
        assert!(link_at(&doc, Position::new(0, 14)).is_some());
        history.undo(&mut doc);
        history.undo(&mut doc);
        assert_eq!(doc, before);
    }

    #[test]
    fn bookmarks_are_added_and_deleted() {
        let mut doc = doc();
        doc.sections[0]
            .blocks
            .push(Paragraph::text("second line").into());
        add_bookmark(&mut doc, "Span", Position::new(0, 9), Position::new(1, 6));
        assert_eq!(
            bookmark_range(&doc, "span"),
            Some((Position::new(0, 9), Position::new(1, 6)))
        );
        // Новый номер больше прежних.
        let Some(letter_model::Block::Paragraph(p)) = doc.sections[0].blocks.first() else {
            panic!();
        };
        assert!(
            p.content
                .iter()
                .any(|i| matches!(i, Inline::BookmarkStart(b) if b.id == "2"))
        );
        assert_eq!(p.plain_text(), "See site now");
        // То же имя — закладка переносится.
        add_bookmark(&mut doc, "SPAN", Position::new(1, 0), Position::new(1, 0));
        assert_eq!(
            bookmark_range(&doc, "Span"),
            Some((Position::new(1, 0), Position::new(1, 0)))
        );
        assert_eq!(bookmarks(&doc).len(), 2);
        assert!(delete_bookmark(&mut doc, "span"));
        assert!(!delete_bookmark(&mut doc, "span"));
        assert_eq!(bookmarks(&doc).len(), 1);
        assert_eq!(doc.sections[0].blocks[1].plain_text(), "second line");
    }

    #[test]
    fn bookmarks_are_found() {
        let doc = doc();
        assert_eq!(bookmarks(&doc), [("Here".to_string(), Position::new(0, 4))]);
        assert_eq!(bookmark_position(&doc, "here"), Some(Position::new(0, 4)));
        assert_eq!(bookmark_position(&doc, "nowhere"), None);
    }
}
