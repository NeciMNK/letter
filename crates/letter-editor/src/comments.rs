//! Примечания: где они в тексте, добавление, ответы, «решено», удаление.

use letter_model::{Block, Comment, Document, Inline, Paragraph, Run, RunContent, RunProps};

use crate::edit::{paragraph_count, paragraph_mut, paragraph_ref, split_content, tidy};
use crate::position::Position;

/// Стиль знака примечания, как у Word.
const REFERENCE_STYLE: &str = "CommentReference";

/// Где в тексте примечание.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentPlace {
    /// Номер примечания.
    pub id: String,
    /// Начало отмеченного текста.
    pub start: Position,
    /// Конец отмеченного текста (у примечания без текста — как начало).
    pub end: Position,
}

/// Места всех примечаний в тексте, по порядку начала. Примечание без
/// отметок в тексте (только ответ без своих отметок и т. п.) не входит.
pub fn comment_places(doc: &Document) -> Vec<CommentPlace> {
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
                Inline::Run(run) => {
                    for item in &run.content {
                        if let RunContent::CommentReference(id) = item {
                            references.push((id.clone(), Position::new(index, offset)));
                        }
                        offset += item.text_len();
                    }
                }
                Inline::CommentStart(id) => starts.push((id.clone(), at)),
                Inline::CommentEnd(id) => ends.push((id.clone(), at)),
                _ => {}
            }
        }
    }
    let find = |list: &[(String, Position)], id: &str| {
        list.iter().find(|(i, _)| i == id).map(|(_, at)| *at)
    };
    let mut places = Vec::new();
    for comment in &doc.comments {
        let end = find(&ends, &comment.id).or_else(|| find(&references, &comment.id));
        let Some(start) = find(&starts, &comment.id).or(end) else {
            continue;
        };
        let end = end.unwrap_or(start);
        places.push(CommentPlace {
            id: comment.id.clone(),
            start,
            end,
        });
    }
    places.sort_by_key(|p| (p.start.paragraph, p.start.offset));
    places
}

/// Новый номер: больше всех номеров примечаний документа.
fn next_id(doc: &Document) -> String {
    let next = doc
        .comments
        .iter()
        .filter_map(|c| c.id.parse::<u64>().ok())
        .map(|id| id + 1)
        .max()
        .unwrap_or(0);
    next.to_string()
}

/// Текст примечания из простого текста: строки — абзацы (со стилем
/// «Текст примечания», если он есть в документе).
pub fn comment_blocks(doc: &Document, text: &str) -> Vec<Block> {
    let has = |id: &str| doc.styles.styles.iter().any(|s| s.id == id);
    let style = has("CommentText").then(|| "CommentText".to_string());
    text.split('\n')
        .map(|line| {
            let mut paragraph = Paragraph::text(line);
            paragraph.props.style = style.clone();
            paragraph.into()
        })
        .collect()
}

/// Ставит отметку `marker` в место `at` (перед отметками, которые там
/// уже есть); `after` — ещё фрагменты сразу за ней.
fn insert_at(doc: &mut Document, at: Position, marker: Inline, after: Vec<Inline>) {
    let Some(paragraph) = paragraph_mut(doc, at.paragraph) else {
        return;
    };
    let tail = split_content(paragraph, at.offset);
    paragraph.content.push(marker);
    paragraph.content.extend(after);
    paragraph.content.extend(tail);
    tidy(paragraph);
}

/// Фрагмент со ссылкой на примечание `id`.
fn reference_run(doc: &Document, id: &str) -> Inline {
    let style = doc
        .styles
        .styles
        .iter()
        .any(|s| s.id == REFERENCE_STYLE)
        .then(|| REFERENCE_STYLE.to_string());
    Inline::Run(Run {
        props: RunProps {
            style,
            ..RunProps::default()
        },
        content: vec![RunContent::CommentReference(id.to_string())],
    })
}

/// Добавляет примечание `comment` к тексту от `start` до `end` (номер
/// даётся новый) и возвращает его номер.
pub fn add_comment(doc: &mut Document, start: Position, end: Position, comment: Comment) -> String {
    let (start, end) = if (start.paragraph, start.offset) <= (end.paragraph, end.offset) {
        (start, end)
    } else {
        (end, start)
    };
    let id = next_id(doc);
    let reference = reference_run(doc, &id);
    insert_at(doc, end, Inline::CommentEnd(id.clone()), vec![reference]);
    insert_at(doc, start, Inline::CommentStart(id.clone()), Vec::new());
    doc.comments.push(Comment {
        id: id.clone(),
        parent: None,
        para_id: None,
        ..comment
    });
    id
}

/// Отвечает на примечание `parent`: ответ отмечает тот же текст. Ответ на
/// ответ становится ответом на всю ветку, как в Word. `None` — такого
/// примечания нет.
pub fn reply_to_comment(doc: &mut Document, parent: &str, reply: Comment) -> Option<String> {
    let root = thread_root(doc, parent)?;
    let place = comment_places(doc).into_iter().find(|p| p.id == root)?;
    let id = add_comment(doc, place.start, place.end, reply);
    if let Some(comment) = doc.comments.iter_mut().find(|c| c.id == id) {
        comment.parent = Some(root);
    }
    Some(id)
}

/// Первое примечание ветки, в которой примечание `id`.
pub fn thread_root(doc: &Document, id: &str) -> Option<String> {
    let mut current = doc.comments.iter().find(|c| c.id == id)?;
    for _ in 0..doc.comments.len() {
        let Some(parent) = current
            .parent
            .as_ref()
            .and_then(|p| doc.comments.iter().find(|c| c.id == *p))
        else {
            break;
        };
        current = parent;
    }
    Some(current.id.clone())
}

/// Удаляет примечание `id` вместе с ответами на него и их отметками
/// в тексте. `false` — такого нет.
pub fn delete_comment(doc: &mut Document, id: &str) -> bool {
    if !doc.comments.iter().any(|c| c.id == id) {
        return false;
    }
    let mut gone = vec![id.to_string()];
    // Ответы (и ответы на ответы).
    loop {
        let more: Vec<String> = doc
            .comments
            .iter()
            .filter(|c| !gone.contains(&c.id))
            .filter(|c| c.parent.as_ref().is_some_and(|p| gone.contains(p)))
            .map(|c| c.id.clone())
            .collect();
        if more.is_empty() {
            break;
        }
        gone.extend(more);
    }
    doc.comments.retain(|c| !gone.contains(&c.id));
    remove_marks(doc, &gone);
    true
}

/// Удаляет все примечания.
pub fn delete_all_comments(doc: &mut Document) {
    let all: Vec<String> = doc.comments.iter().map(|c| c.id.clone()).collect();
    doc.comments.clear();
    remove_marks(doc, &all);
}

fn remove_marks(doc: &mut Document, ids: &[String]) {
    for index in 0..paragraph_count(doc) {
        let Some(paragraph) = paragraph_mut(doc, index) else {
            continue;
        };
        let before = paragraph.content.len();
        let mut changed = false;
        paragraph.content.retain(|inline| match inline {
            Inline::CommentStart(id) | Inline::CommentEnd(id) => !ids.contains(id),
            _ => true,
        });
        for inline in &mut paragraph.content {
            if let Inline::Run(run) = inline {
                let len = run.content.len();
                run.content.retain(
                    |item| !matches!(item, RunContent::CommentReference(id) if ids.contains(id)),
                );
                changed |= run.content.len() != len;
            }
        }
        if changed || paragraph.content.len() != before {
            tidy(paragraph);
        }
    }
}

/// Убирает из блоков отметки примечаний (для скопированного куска:
/// примечания в него не копируются).
pub(crate) fn strip_comment_marks(blocks: &mut [Block]) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                let before = p.content.clone();
                p.content
                    .retain(|i| !matches!(i, Inline::CommentStart(_) | Inline::CommentEnd(_)));
                for inline in &mut p.content {
                    if let Inline::Run(run) = inline {
                        run.content
                            .retain(|c| !matches!(c, RunContent::CommentReference(_)));
                    }
                }
                if p.content != before {
                    tidy(p);
                }
            }
            Block::Table(t) => {
                for cell in t.rows.iter_mut().flat_map(|r| &mut r.cells) {
                    strip_comment_marks(&mut cell.blocks);
                }
            }
            Block::Unknown(_) => {}
        }
    }
}

/// Отмечает ветку примечания `id` «решено» (или снимает отметку).
pub fn set_comment_done(doc: &mut Document, id: &str, done: bool) {
    let Some(root) = thread_root(doc, id) else {
        return;
    };
    for comment in &mut doc.comments {
        if comment.id == root {
            comment.done = done;
        }
    }
}

/// Заменяет текст примечания `id`.
pub fn set_comment_text(doc: &mut Document, id: &str, blocks: Vec<Block>) {
    if let Some(comment) = doc.comments.iter_mut().find(|c| c.id == id) {
        comment.blocks = blocks;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Document {
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![
            Paragraph::text("The quick brown fox.").into(),
            Paragraph::text("Second line.").into(),
        ];
        doc
    }

    fn comment(author: &str, text: &str) -> Comment {
        Comment {
            author: author.into(),
            blocks: vec![Paragraph::text(text).into()],
            ..Comment::default()
        }
    }

    #[test]
    fn comment_marks_its_text() {
        let mut doc = doc();
        let id = add_comment(
            &mut doc,
            Position::new(0, 10),
            Position::new(0, 4),
            comment("Anna", "Which?"),
        );
        assert_eq!(id, "0");
        let places = comment_places(&doc);
        assert_eq!(places.len(), 1);
        assert_eq!(places[0].start, Position::new(0, 4));
        assert_eq!(places[0].end, Position::new(0, 10));
        // Текст не изменился.
        assert_eq!(
            doc.sections[0].blocks[0].plain_text(),
            "The quick brown fox."
        );
        let second = add_comment(
            &mut doc,
            Position::new(1, 0),
            Position::new(1, 6),
            comment("Boris", "Ok"),
        );
        assert_eq!(second, "1");
    }

    #[test]
    fn replies_join_the_thread_and_go_away_with_it() {
        let mut doc = doc();
        let id = add_comment(
            &mut doc,
            Position::new(0, 4),
            Position::new(0, 9),
            comment("Anna", "Which?"),
        );
        let reply = reply_to_comment(&mut doc, &id, comment("Boris", "This one")).unwrap();
        // Ответ на ответ — в ту же ветку.
        let again = reply_to_comment(&mut doc, &reply, comment("Anna", "Thanks")).unwrap();
        assert_eq!(doc.comments[2].parent.as_deref(), Some(id.as_str()));
        assert_eq!(thread_root(&doc, &again), Some(id.clone()));
        let places = comment_places(&doc);
        assert_eq!(places.len(), 3);
        assert!(places.iter().all(|p| p.start == Position::new(0, 4)));

        set_comment_done(&mut doc, &again, true);
        assert!(doc.comments[0].done);
        let blocks = comment_blocks(&doc, "a\nb");
        set_comment_text(&mut doc, &reply, blocks);
        assert_eq!(doc.comments[1].plain_text(), "a\nb");

        assert!(delete_comment(&mut doc, &id));
        assert!(doc.comments.is_empty());
        assert!(comment_places(&doc).is_empty());
        let Block::Paragraph(p) = &doc.sections[0].blocks[0] else {
            panic!();
        };
        assert!(p.content.iter().all(|i| matches!(i, Inline::Run(_))));
        assert_eq!(p.plain_text(), "The quick brown fox.");
        assert!(!delete_comment(&mut doc, &id));
    }

    #[test]
    fn all_comments_are_deleted() {
        let mut doc = doc();
        add_comment(
            &mut doc,
            Position::new(0, 0),
            Position::new(0, 3),
            comment("A", "1"),
        );
        add_comment(
            &mut doc,
            Position::new(1, 0),
            Position::new(1, 3),
            comment("B", "2"),
        );
        delete_all_comments(&mut doc);
        assert!(doc.comments.is_empty());
        assert!(comment_places(&doc).is_empty());
    }

    #[test]
    fn comment_commands_undo() {
        use crate::{Command, History, Selection};
        let mut doc = doc();
        let before = doc.clone();
        let mut history = History::new();
        let selection = Selection::new(Position::new(0, 4), Position::new(0, 9));
        let after = history.run(
            &mut doc,
            selection,
            Command::AddComment(Box::new(comment("Anna", "Which?"))),
        );
        assert_eq!(after, selection);
        assert_eq!(doc.comments.len(), 1);
        let added = doc.clone();
        history.run(
            &mut doc,
            selection,
            Command::CommentDone {
                id: "0".into(),
                done: true,
            },
        );
        assert!(doc.comments[0].done);
        history.run(&mut doc, selection, Command::DeleteComment("0".into()));
        assert!(doc.comments.is_empty());
        history.undo(&mut doc);
        history.undo(&mut doc);
        assert_eq!(doc, added);
        history.undo(&mut doc);
        assert_eq!(doc, before);
        history.redo(&mut doc);
        assert_eq!(doc, added);
    }

    #[test]
    fn copies_leave_comments_behind() {
        let mut doc = doc();
        add_comment(
            &mut doc,
            Position::new(0, 4),
            Position::new(0, 9),
            comment("Anna", "Which?"),
        );
        let fragment = crate::copy_range(&doc, Position::new(0, 0), Position::new(0, 20));
        let Block::Paragraph(p) = &fragment.sections[0].blocks[0] else {
            panic!();
        };
        assert_eq!(p.plain_text(), "The quick brown fox.");
        assert!(comment_places(&fragment).is_empty());
        assert!(
            p.runs()
                .flat_map(|r| &r.content)
                .all(|c| !matches!(c, RunContent::CommentReference(_)))
        );
        assert!(p.content.iter().all(|i| matches!(i, Inline::Run(_))));
    }
}
