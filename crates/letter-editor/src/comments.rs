//! Примечания («Рецензирование → Создать примечание»): кусок текста
//! с отметками начала и конца и значок примечания после него, сам текст
//! примечания — в [`Document::comments`].
//!
//! Отметки места в тексте не занимают, поэтому номера позиций курсора от
//! них не меняются. Примечание, отметок которого в тексте нет (удалили
//! или отменили вставку), не показывается и в файл не пишется: так отмена
//! и повтор работают через обычную отмену правки абзацев.

use letter_model::{
    Block, Comment, Document, HalfPoints, Inline, Paragraph, Run, RunContent, RunProps, StyleKind,
};

use crate::Position;
use crate::edit::{
    clamp_offset, item_len, paragraph_count, paragraph_mut, paragraph_ref, split_content, tidy,
};
use crate::notes::ensure_style;

/// Стиль текста примечания (как у Word).
pub const COMMENT_TEXT_STYLE: &str = "CommentText";
/// Стиль значка примечания.
pub const COMMENT_REFERENCE_STYLE: &str = "CommentReference";

/// Где в документе примечание: начало и конец куска текста.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentPlace {
    /// Номер примечания.
    pub id: String,
    /// Начало куска.
    pub start: Position,
    /// Конец куска.
    pub end: Position,
}

/// Примечания, которые есть в тексте, по порядку начала куска (ответ —
/// сразу после того, на что отвечает). Без отметки начала началом
/// считается значок.
pub fn comment_places(doc: &Document) -> Vec<CommentPlace> {
    let mut places: Vec<CommentPlace> = Vec::new();
    let mut starts: Vec<(String, Position)> = Vec::new();
    let mut ends: Vec<(String, Position)> = Vec::new();
    let mut references: Vec<(String, Position)> = Vec::new();
    for index in 0..paragraph_count(doc) {
        let Some(paragraph) = paragraph_ref(doc, index) else {
            continue;
        };
        let mut offset = 0;
        for inline in &paragraph.content {
            let at = Position::new(index, offset);
            match inline {
                Inline::CommentStart(id) => starts.push((id.clone(), at)),
                Inline::CommentEnd(id) => ends.push((id.clone(), at)),
                Inline::Run(run) => {
                    for item in &run.content {
                        if let RunContent::CommentReference(id) = item {
                            references.push((id.clone(), Position::new(index, offset)));
                        }
                        offset += item_len(item);
                    }
                }
                _ => {}
            }
        }
    }
    for (id, reference) in references {
        if places.iter().any(|p| p.id == id) {
            continue;
        }
        let find = |list: &[(String, Position)]| {
            list.iter()
                .find(|(other, _)| *other == id)
                .map(|(_, at)| *at)
        };
        let end = find(&ends).unwrap_or(reference);
        let start = find(&starts).unwrap_or(end);
        places.push(CommentPlace { id, start, end });
    }
    let order = |p: &CommentPlace| (p.start.paragraph, p.start.offset);
    places.sort_by_key(order);
    // Ответы — сразу после исходного примечания, по порядку в списке.
    let mut sorted: Vec<CommentPlace> = Vec::new();
    let parent = |id: &str| {
        doc.comments
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.parent.clone())
    };
    let roots: Vec<CommentPlace> = places
        .iter()
        .filter(|p| parent(&p.id).is_none_or(|parent| !places.iter().any(|q| q.id == parent)))
        .cloned()
        .collect();
    for root in roots {
        let thread = thread_ids(doc, &root.id);
        sorted.push(root);
        for id in thread.into_iter().skip(1) {
            if let Some(place) = places.iter().find(|p| p.id == id) {
                sorted.push(place.clone());
            }
        }
    }
    sorted
}

/// Примечание `id` и все ответы на него (и ответы на ответы) по порядку
/// в списке примечаний.
pub fn thread_ids(doc: &Document, id: &str) -> Vec<String> {
    let mut ids = vec![id.to_string()];
    let mut i = 0;
    while i < ids.len() {
        for comment in &doc.comments {
            if comment.parent.as_deref() == Some(ids[i].as_str()) && !ids.contains(&comment.id) {
                ids.push(comment.id.clone());
            }
        }
        i += 1;
    }
    ids
}

/// Свободный номер для нового примечания.
fn next_id(doc: &Document) -> String {
    let next = doc
        .comments
        .iter()
        .filter_map(|c| c.id.parse::<i64>().ok())
        .max()
        .map_or(0, |n| n.max(-1) + 1);
    next.to_string()
}

/// Ставит стили примечаний, если их ещё нет; возвращает стили текста
/// и значка.
fn comment_styles(doc: &mut Document) -> (String, String) {
    let text = COMMENT_TEXT_STYLE;
    ensure_style(
        doc,
        text,
        "annotation text",
        StyleKind::Paragraph,
        |style| {
            style.run.size = Some(HalfPoints(20));
        },
    );
    let reference = COMMENT_REFERENCE_STYLE;
    ensure_style(
        doc,
        reference,
        "annotation reference",
        StyleKind::Character,
        |style| {
            style.run.size = Some(HalfPoints(16));
        },
    );
    (text.to_string(), reference.to_string())
}

/// Текст нового примечания: абзацы со стилем текста примечания, в начале
/// первого — служебная отметка `w:annotationRef`, как у Word.
pub fn comment_blocks(doc: &mut Document, text: &str) -> Vec<Block> {
    let (text_style, reference_style) = comment_styles(doc);
    text.split('\n')
        .enumerate()
        .map(|(i, line)| {
            let mut runs = Vec::new();
            if i == 0 {
                runs.push(Run {
                    props: RunProps {
                        style: Some(reference_style.clone()),
                        ..RunProps::default()
                    },
                    content: vec![RunContent::Unknown(letter_model::RawXml::new(
                        "<w:annotationRef/>".to_string(),
                    ))],
                });
            }
            if !line.is_empty() {
                runs.push(Run::text(line));
            }
            let mut paragraph = Paragraph::from_runs(runs);
            paragraph.props.style = Some(text_style.clone());
            Block::Paragraph(paragraph)
        })
        .collect()
}

/// Значок примечания `id`.
fn reference_run(id: &str, style: &str) -> Inline {
    Inline::Run(Run {
        props: RunProps {
            style: Some(style.to_string()),
            ..RunProps::default()
        },
        content: vec![RunContent::CommentReference(id.to_string())],
    })
}

/// Вставляет элементы `items` в место `offset` абзаца (после отметок,
/// которые там уже есть, и перед текстом).
fn insert_inlines(paragraph: &mut Paragraph, offset: usize, items: Vec<Inline>) {
    let offset = clamp_offset(paragraph, offset);
    let tail = split_content(paragraph, offset);
    paragraph.content.extend(items);
    paragraph.content.extend(tail);
    tidy(paragraph);
}

/// Добавляет примечание `comment` к куску от `start` до `end` (номер
/// даётся новый). Возвращает номер.
pub fn insert_comment(
    doc: &mut Document,
    start: Position,
    end: Position,
    mut comment: Comment,
) -> String {
    let (start, end) = if (start.paragraph, start.offset) <= (end.paragraph, end.offset) {
        (start, end)
    } else {
        (end, start)
    };
    let id = next_id(doc);
    let (_, reference_style) = comment_styles(doc);
    let reference = reference_run(&id, &reference_style);
    if start == end {
        if let Some(paragraph) = paragraph_mut(doc, start.paragraph) {
            let items = vec![
                Inline::CommentStart(id.clone()),
                Inline::CommentEnd(id.clone()),
                reference,
            ];
            insert_inlines(paragraph, start.offset, items);
        }
    } else {
        if let Some(paragraph) = paragraph_mut(doc, end.paragraph) {
            insert_inlines(
                paragraph,
                end.offset,
                vec![Inline::CommentEnd(id.clone()), reference],
            );
        }
        if let Some(paragraph) = paragraph_mut(doc, start.paragraph) {
            insert_inlines(
                paragraph,
                start.offset,
                vec![Inline::CommentStart(id.clone())],
            );
        }
    }
    comment.id = id.clone();
    comment.para_id = None;
    doc.comments.push(comment);
    id
}

/// Добавляет ответ `reply` на примечание `parent`: его отметки встают
/// рядом с отметками исходного. Возвращает номер ответа (`None` —
/// исходного примечания в тексте нет).
pub fn reply_to_comment(doc: &mut Document, parent: &str, mut reply: Comment) -> Option<String> {
    let id = next_id(doc);
    let (_, reference_style) = comment_styles(doc);
    let mut found = false;
    for index in 0..paragraph_count(doc) {
        let Some(paragraph) = paragraph_mut(doc, index) else {
            continue;
        };
        let mut content = Vec::with_capacity(paragraph.content.len() + 3);
        let mut changed = false;
        for inline in std::mem::take(&mut paragraph.content) {
            let extra = match &inline {
                Inline::CommentStart(p) if p == parent => Some(Inline::CommentStart(id.clone())),
                Inline::CommentEnd(p) if p == parent => Some(Inline::CommentEnd(id.clone())),
                Inline::Run(run)
                    if run
                        .content
                        .iter()
                        .any(|c| matches!(c, RunContent::CommentReference(p) if p == parent)) =>
                {
                    found = true;
                    Some(reference_run(&id, &reference_style))
                }
                _ => None,
            };
            content.push(inline);
            if let Some(extra) = extra {
                content.push(extra);
                changed = true;
            }
        }
        paragraph.content = content;
        if changed {
            tidy(paragraph);
        }
    }
    if !found {
        return None;
    }
    reply.id = id.clone();
    reply.parent = Some(parent.to_string());
    reply.para_id = None;
    doc.comments.push(reply);
    Some(id)
}

/// Убирает из текста примечание `id` вместе с ответами на него.
/// `false` — его в тексте не было.
pub fn delete_comment(doc: &mut Document, id: &str) -> bool {
    let ids = thread_ids(doc, id);
    let gone = |other: &String| ids.contains(other);
    let mut found = false;
    for index in 0..paragraph_count(doc) {
        let Some(paragraph) = paragraph_mut(doc, index) else {
            continue;
        };
        let before = paragraph.content.len();
        paragraph.content.retain_mut(|inline| match inline {
            Inline::CommentStart(other) | Inline::CommentEnd(other) => !gone(other),
            Inline::Run(run) => {
                let had = run.content.len();
                run.content
                    .retain(|c| !matches!(c, RunContent::CommentReference(other) if gone(other)));
                if run.content.len() != had {
                    found = true;
                }
                !run.content.is_empty() || had == 0
            }
            _ => true,
        });
        if paragraph.content.len() != before {
            found = true;
            tidy(paragraph);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::plain_text;
    use crate::{Command, History, Selection};

    fn doc() -> Document {
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![
            Paragraph::text("Первый абзац текста.").into(),
            Paragraph::text("Второй.").into(),
        ];
        doc
    }

    fn note(text: &str) -> Comment {
        Comment {
            author: "Никита".into(),
            blocks: vec![Paragraph::text(text).into()],
            ..Comment::default()
        }
    }

    fn text(doc: &Document, index: usize) -> String {
        plain_text(paragraph_ref(doc, index).unwrap())
    }

    #[test]
    fn comment_marks_a_piece_of_text() {
        let mut doc = doc();
        let id = insert_comment(
            &mut doc,
            Position::new(0, "Первый ".len()),
            Position::new(1, "Второй".len()),
            note("Проверить"),
        );
        assert_eq!(id, "0");
        // Текст не изменился: отметки места не занимают.
        assert_eq!(text(&doc, 0), "Первый абзац текста.");
        assert_eq!(text(&doc, 1), "Второй.");
        let places = comment_places(&doc);
        assert_eq!(
            places,
            [CommentPlace {
                id: "0".into(),
                start: Position::new(0, "Первый ".len()),
                end: Position::new(1, "Второй".len()),
            }]
        );
        assert!(doc.styles.get(COMMENT_REFERENCE_STYLE).is_some());

        // Ответ встаёт рядом и идёт после исходного.
        let reply = reply_to_comment(&mut doc, "0", note("Готово")).unwrap();
        assert_eq!(reply, "1");
        assert_eq!(doc.comments[1].parent.as_deref(), Some("0"));
        let ids: Vec<_> = comment_places(&doc).into_iter().map(|p| p.id).collect();
        assert_eq!(ids, ["0", "1"]);
        assert_eq!(reply_to_comment(&mut doc, "9", note("Нет")), None);

        // Удаление убирает и ответы.
        assert!(delete_comment(&mut doc, "0"));
        assert!(comment_places(&doc).is_empty());
        assert_eq!(text(&doc, 1), "Второй.");
        assert!(!delete_comment(&mut doc, "0"));
    }

    #[test]
    fn empty_range_comment_and_order() {
        let mut doc = doc();
        let at = Position::new(1, "Вто".len());
        insert_comment(&mut doc, at, at, note("b"));
        insert_comment(
            &mut doc,
            Position::new(0, 0),
            Position::new(0, 4),
            note("a"),
        );
        let places = comment_places(&doc);
        assert_eq!(places[0].id, "1");
        assert_eq!(places[1].start, at);
        assert_eq!(places[1].end, at);
        assert_eq!(text(&doc, 1), "Второй.");
    }

    #[test]
    fn comment_commands_can_be_undone() {
        let mut doc = doc();
        let mut history = History::new();
        let selection = Selection {
            anchor: Position::new(0, 0),
            focus: Position::new(0, 6),
        };
        let after = history.run(
            &mut doc,
            selection,
            Command::InsertComment(Box::new(note("Что это?"))),
        );
        assert_eq!(after, selection);
        assert_eq!(comment_places(&doc).len(), 1);
        let id = comment_places(&doc)[0].id.clone();

        let mut changed = doc.comments[0].clone();
        changed.done = true;
        history.run(&mut doc, after, Command::SetComment(Box::new(changed)));
        assert!(doc.comments[0].done);
        history.run(
            &mut doc,
            after,
            Command::ReplyComment {
                parent: id.clone(),
                reply: Box::new(note("Ответ")),
            },
        );
        assert_eq!(comment_places(&doc).len(), 2);
        history.run(&mut doc, after, Command::DeleteComment(id));
        assert!(comment_places(&doc).is_empty());

        history.undo(&mut doc);
        assert_eq!(comment_places(&doc).len(), 2);
        history.undo(&mut doc);
        history.undo(&mut doc);
        assert!(!doc.comments[0].done);
        history.undo(&mut doc);
        assert!(comment_places(&doc).is_empty());
        history.redo(&mut doc);
        assert_eq!(comment_places(&doc).len(), 1);
    }
}
