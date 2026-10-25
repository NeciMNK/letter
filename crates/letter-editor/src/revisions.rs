//! Запись исправлений («Рецензирование → Исправления») и их принятие
//! или отклонение.
//!
//! Пока запись включена, набранный текст помечается вставкой, а удалённый
//! не исчезает, а помечается удалением (зачёркнут). Свою ещё не принятую
//! вставку удаление убирает сразу, как в Word. Знак абзаца исправлением
//! пока не бывает: Backspace в начале абзаца при записи только сдвигает
//! курсор.

use letter_model::{Document, Inline, Revision, RevisionKind, Run};
use unicode_segmentation::UnicodeSegmentation;

use crate::Position;
use crate::edit::{
    clamp_offset, item_len, paragraph_count, paragraph_mut, paragraph_ref, plain_text,
    split_content, text_len, tidy,
};

/// Кто и когда правит при записи исправлений.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reviewer {
    /// Автор.
    pub author: String,
    /// Дата и время, как в файле: `2026-10-25T12:00:00Z`.
    pub date: Option<String>,
}

/// Меняет фрагменты абзаца `index` между `from` и `to` (смещения):
/// `change` получает каждый такой фрагмент и говорит, оставить ли его.
/// Возвращает, сколько байт текста убрано.
fn change_runs(
    doc: &mut Document,
    index: usize,
    from: usize,
    to: usize,
    change: &mut impl FnMut(&mut Run) -> bool,
) -> usize {
    let Some(paragraph) = paragraph_mut(doc, index) else {
        return 0;
    };
    let from = clamp_offset(paragraph, from);
    let to = clamp_offset(paragraph, to).max(from);
    let tail = split_content(paragraph, to);
    let middle = split_content(paragraph, from);
    let mut removed = 0;
    for inline in middle {
        match inline {
            Inline::Run(mut run) => {
                if change(&mut run) {
                    paragraph.content.push(Inline::Run(run));
                } else {
                    removed += run.content.iter().map(item_len).sum::<usize>();
                    // Значки примечаний и сносок без текста не теряются.
                    run.content.retain(|c| item_len(c) == 0);
                    if !run.content.is_empty() {
                        paragraph.content.push(Inline::Run(run));
                    }
                }
            }
            other => paragraph.content.push(other),
        }
    }
    paragraph.content.extend(tail);
    tidy(paragraph);
    removed
}

/// Вызывает [`change_runs`] для каждого абзаца от `start` до `end`.
/// Возвращает, сколько байт убрано в последнем абзаце.
fn change_range(
    doc: &mut Document,
    start: Position,
    end: Position,
    mut change: impl FnMut(&mut Run) -> bool,
) -> usize {
    let (start, end) = ordered(start, end);
    let mut removed = 0;
    for index in start.paragraph..=end.paragraph {
        let from = if index == start.paragraph {
            start.offset
        } else {
            0
        };
        let to = if index == end.paragraph {
            end.offset
        } else {
            paragraph_ref(doc, index).map_or(0, text_len)
        };
        removed = change_runs(doc, index, from, to, &mut change);
    }
    removed
}

fn ordered(a: Position, b: Position) -> (Position, Position) {
    if (a.paragraph, a.offset) <= (b.paragraph, b.offset) {
        (a, b)
    } else {
        (b, a)
    }
}

/// Номер для нового исправления: больше всех номеров исправлений,
/// примечаний и закладок документа (Word требует, чтобы они не
/// совпадали).
fn next_id(doc: &Document) -> String {
    let mut next = 0u64;
    let mut take = |id: &str| {
        if let Ok(n) = id.parse::<u64>() {
            next = next.max(n + 1);
        }
    };
    for comment in &doc.comments {
        take(&comment.id);
    }
    for index in 0..paragraph_count(doc) {
        let Some(paragraph) = paragraph_ref(doc, index) else {
            continue;
        };
        for inline in &paragraph.content {
            match inline {
                Inline::Run(run) => {
                    if let Some(r) = &run.props.revision {
                        take(&r.id);
                    }
                }
                Inline::BookmarkStart(b) => take(&b.id),
                _ => {}
            }
        }
    }
    next.to_string()
}

/// Исправление вида `kind` от `reviewer`: то же, что у соседнего текста
/// у места `at` (тот же автор и вид), чтобы набор не дробился на
/// множество исправлений; иначе новое.
fn revision_near(
    doc: &Document,
    at: Position,
    kind: RevisionKind,
    reviewer: &Reviewer,
) -> Revision {
    let neighbour = paragraph_ref(doc, at.paragraph).and_then(|paragraph| {
        let mut offset = 0;
        let mut found = None;
        for run in paragraph.runs() {
            let len: usize = run.content.iter().map(item_len).sum();
            let touches = offset <= at.offset && at.offset <= offset + len;
            if touches
                && let Some(r) = &run.props.revision
                && r.kind == kind
                && r.author == reviewer.author
            {
                found = Some(r.clone());
            }
            offset += len;
        }
        found
    });
    neighbour.unwrap_or_else(|| Revision {
        kind,
        id: next_id(doc),
        author: reviewer.author.clone(),
        date: reviewer.date.clone(),
    })
}

/// Помечает вставкой только что вставленный текст от `start` до `end`
/// (оформление соседей, в том числе чужое исправление, заменяется).
pub fn mark_inserted(doc: &mut Document, start: Position, end: Position, reviewer: &Reviewer) {
    if start == end {
        return;
    }
    let revision = revision_near(doc, start, RevisionKind::Insert, reviewer);
    change_range(doc, start, end, |run| {
        run.props.revision = Some(revision.clone());
        true
    });
}

/// Снимает пометки исправлений с только что вставленного текста от
/// `start` до `end`: без записи набор рядом с исправлением — обычный
/// текст.
pub fn clear_revisions(doc: &mut Document, start: Position, end: Position) {
    if start == end {
        return;
    }
    change_range(doc, start, end, |run| {
        run.props.revision = None;
        true
    });
}

/// Сейчас, как дата в файле: `2026-10-25T12:00:00Z` (по Гринвичу).
pub fn now_iso() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let t = crate::DateTime::from_unix(seconds, 0);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    )
}

/// Удаляет при записи исправлений текст от `start` до `end`: обычный
/// текст помечается удалением, вставки убираются, уже удалённое не
/// меняется. Возвращает место, где был конец куска.
pub fn mark_deleted(
    doc: &mut Document,
    start: Position,
    end: Position,
    reviewer: &Reviewer,
) -> Position {
    let (start, end) = ordered(start, end);
    if start == end {
        return end;
    }
    let revision = revision_near(doc, start, RevisionKind::Delete, reviewer);
    let removed = change_range(doc, start, end, |run| match &run.props.revision {
        Some(r) if r.kind == RevisionKind::Insert => false,
        Some(_) => true,
        None => {
            run.props.revision = Some(revision.clone());
            true
        }
    });
    Position::new(end.paragraph, end.offset.saturating_sub(removed))
}

/// Backspace при записи исправлений: буква перед курсором помечается
/// удалением (своя вставка — убирается). Возвращает новое место курсора.
pub fn track_delete_backward(doc: &mut Document, at: Position, reviewer: &Reviewer) -> Position {
    let Some(paragraph) = paragraph_ref(doc, at.paragraph) else {
        return at;
    };
    let offset = clamp_offset(paragraph, at.offset);
    if offset == 0 {
        // Знак абзаца исправлением пока не бывает: курсор — в конец
        // прошлого абзаца.
        return match at.paragraph.checked_sub(1) {
            Some(previous) => {
                Position::new(previous, paragraph_ref(doc, previous).map_or(0, text_len))
            }
            None => at,
        };
    }
    let text = plain_text(paragraph);
    let start = text[..offset]
        .grapheme_indices(true)
        .next_back()
        .map_or(0, |(i, _)| i);
    let start = Position::new(at.paragraph, start);
    mark_deleted(doc, start, Position::new(at.paragraph, offset), reviewer);
    start
}

/// Delete при записи исправлений: буква после курсора помечается
/// удалением, курсор встаёт после неё.
pub fn track_delete_forward(doc: &mut Document, at: Position, reviewer: &Reviewer) -> Position {
    let Some(paragraph) = paragraph_ref(doc, at.paragraph) else {
        return at;
    };
    let offset = clamp_offset(paragraph, at.offset);
    let text = plain_text(paragraph);
    if offset >= text.len() {
        return if at.paragraph + 1 < paragraph_count(doc) {
            Position::new(at.paragraph + 1, 0)
        } else {
            at
        };
    }
    let end = text[offset..]
        .grapheme_indices(true)
        .nth(1)
        .map_or(text.len(), |(i, _)| offset + i);
    mark_deleted(
        doc,
        Position::new(at.paragraph, offset),
        Position::new(at.paragraph, end),
        reviewer,
    )
}

/// Принимает (`accept`) или отклоняет исправления в куске от `start` до
/// `end`; если `id` задан — только исправление с этим номером. Принятая
/// вставка и отклонённое удаление становятся обычным текстом, принятое
/// удаление и отклонённая вставка убираются.
pub fn resolve_revisions(
    doc: &mut Document,
    start: Position,
    end: Position,
    id: Option<&str>,
    accept: bool,
) {
    change_range(doc, start, end, |run| {
        let Some(revision) = &run.props.revision else {
            return true;
        };
        if id.is_some_and(|id| revision.id != id) {
            return true;
        }
        let keep = (revision.kind == RevisionKind::Insert) == accept;
        run.props.revision = None;
        keep
    });
}

/// Исправление у места `at` (во фрагменте, где стоит курсор, или сразу
/// перед ним).
pub fn revision_at(doc: &Document, at: Position) -> Option<Revision> {
    let paragraph = paragraph_ref(doc, at.paragraph)?;
    let mut offset = 0;
    let mut before = None;
    for run in paragraph.runs() {
        let len: usize = run.content.iter().map(item_len).sum();
        if let Some(r) = &run.props.revision {
            if offset <= at.offset && at.offset < offset + len {
                return Some(r.clone());
            }
            if offset + len == at.offset {
                before = Some(r.clone());
            }
        }
        offset += len;
    }
    before
}

/// Начало и конец исправления с номером `id` (первое и последнее места
/// его фрагментов в документе).
pub fn revision_range(doc: &Document, id: &str) -> Option<(Position, Position)> {
    let mut range: Option<(Position, Position)> = None;
    for index in 0..paragraph_count(doc) {
        let Some(paragraph) = paragraph_ref(doc, index) else {
            continue;
        };
        let mut offset = 0;
        for run in paragraph.runs() {
            let len: usize = run.content.iter().map(item_len).sum();
            if run.props.revision.as_ref().is_some_and(|r| r.id == id) {
                let (from, to) = (
                    Position::new(index, offset),
                    Position::new(index, offset + len),
                );
                range = Some(match range {
                    Some((start, _)) => (start, to),
                    None => (from, to),
                });
            }
            offset += len;
        }
    }
    range
}

/// Следующее (`forward`) или предыдущее исправление от места `at`:
/// начало и конец его куска. По кругу, как в Word.
pub fn next_revision(doc: &Document, at: Position, forward: bool) -> Option<(Position, Position)> {
    let mut starts: Vec<(Position, String)> = Vec::new();
    for index in 0..paragraph_count(doc) {
        let Some(paragraph) = paragraph_ref(doc, index) else {
            continue;
        };
        let mut offset = 0;
        let mut previous: Option<&str> = None;
        for run in paragraph.runs() {
            let len: usize = run.content.iter().map(item_len).sum();
            let id = run.props.revision.as_ref().map(|r| r.id.as_str());
            if let Some(id) = id
                && previous != Some(id)
                && !starts.iter().any(|(_, other)| other == id)
            {
                starts.push((Position::new(index, offset), id.to_string()));
            }
            previous = id;
            offset += len;
        }
    }
    let key = |p: Position| (p.paragraph, p.offset);
    let chosen = if forward {
        starts
            .iter()
            .find(|(p, _)| key(*p) > key(at))
            .or_else(|| starts.first())
    } else {
        starts
            .iter()
            .rev()
            .find(|(p, _)| key(*p) < key(at))
            .or_else(|| starts.last())
    }?;
    revision_range(doc, &chosen.1)
}

/// Есть ли в документе исправления.
pub fn has_revisions(doc: &Document) -> bool {
    (0..paragraph_count(doc)).any(|index| {
        paragraph_ref(doc, index).is_some_and(|p| p.runs().any(|r| r.props.revision.is_some()))
    })
}

/// Текст фрагментов-исправлений (для проверки).
#[cfg(test)]
fn marked(doc: &Document, index: usize) -> Vec<(String, Option<RevisionKind>)> {
    paragraph_ref(doc, index)
        .map(|p| {
            p.runs()
                .map(|r| {
                    let text: String = r
                        .content
                        .iter()
                        .filter_map(|c| match c {
                            letter_model::RunContent::Text(t) => Some(t.as_str()),
                            _ => None,
                        })
                        .collect();
                    (text, r.props.revision.as_ref().map(|r| r.kind))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, History, Selection};
    use RevisionKind::{Delete, Insert};
    use letter_model::Paragraph;

    fn doc() -> Document {
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![
            Paragraph::text("Hello world").into(),
            Paragraph::text("Second").into(),
        ];
        doc.settings.track_revisions = true;
        doc
    }

    fn me() -> Reviewer {
        Reviewer {
            author: "Никита".into(),
            date: Some("2026-10-25T12:00:00Z".into()),
        }
    }

    fn text(doc: &Document, index: usize) -> String {
        plain_text(paragraph_ref(doc, index).unwrap())
    }

    #[test]
    fn deletion_keeps_text_and_removes_own_insertions() {
        let mut doc = doc();
        mark_inserted(&mut doc, Position::new(0, 0), Position::new(0, 6), &me());
        assert_eq!(
            marked(&doc, 0),
            [("Hello ".into(), Some(Insert)), ("world".into(), None)]
        );
        // Удаляется «o w»: вставленное «o » уходит, «w» — зачёркнуто.
        let end = mark_deleted(&mut doc, Position::new(0, 4), Position::new(0, 7), &me());
        assert_eq!(end, Position::new(0, 5));
        assert_eq!(text(&doc, 0), "Hellworld");
        assert_eq!(
            marked(&doc, 0),
            [
                ("Hell".into(), Some(Insert)),
                ("w".into(), Some(Delete)),
                ("orld".into(), None)
            ]
        );
        // Отклонить всё: вставка уходит, удаление возвращается.
        resolve_revisions(
            &mut doc,
            Position::new(0, 0),
            Position::new(0, 9),
            None,
            false,
        );
        assert_eq!(text(&doc, 0), "world");
        assert_eq!(marked(&doc, 0), [("world".into(), None)]);
    }

    #[test]
    fn typing_and_deleting_are_recorded() {
        let mut doc = doc();
        let mut history = History::new();
        history.set_reviewer(me());
        let at = Selection::caret(Position::new(0, 5));
        let after = history.run(&mut doc, at, Command::Type(",".into()));
        let after = history.run(&mut doc, after, Command::Type(" dear".into()));
        assert_eq!(text(&doc, 0), "Hello, dear world");
        // Набор — одно исправление.
        assert_eq!(
            marked(&doc, 0),
            [
                ("Hello".into(), None),
                (", dear".into(), Some(Insert)),
                (" world".into(), None)
            ]
        );
        // Backspace по своей вставке убирает букву, по чужому тексту —
        // зачёркивает.
        let after = history.run(&mut doc, after, Command::DeleteBackward);
        assert_eq!(text(&doc, 0), "Hello, dea world");
        let start = Selection::caret(Position::new(0, 0));
        let after_delete = history.run(&mut doc, start, Command::DeleteForward);
        assert_eq!(after_delete.focus, Position::new(0, 1));
        assert_eq!(text(&doc, 0), "Hello, dea world");
        assert_eq!(marked(&doc, 0)[0], ("H".into(), Some(Delete)));
        // Набор поверх выделения: старое зачёркнуто, новое — после него.
        let selection = Selection::new(Position::new(0, 11), Position::new(0, 16));
        history.run(&mut doc, selection, Command::Type("all".into()));
        assert_eq!(text(&doc, 0), "Hello, dea worldall");
        let last = marked(&doc, 0);
        assert_eq!(last[last.len() - 2], ("world".into(), Some(Delete)));
        assert_eq!(last[last.len() - 1], ("all".into(), Some(Insert)));
        // Backspace в начале абзаца только сдвигает курсор.
        let start = Selection::caret(Position::new(1, 0));
        let moved = history.run(&mut doc, start, Command::DeleteBackward);
        assert_eq!(moved.focus.paragraph, 0);
        assert_eq!(text(&doc, 1), "Second");
        let _ = after;

        // Принять всё: удалённое уходит, вставленное остаётся.
        let all = Selection::caret(Position::new(1, 2));
        history.run(
            &mut doc,
            all,
            Command::ResolveRevisions {
                accept: true,
                all: true,
            },
        );
        assert_eq!(text(&doc, 0), "ello, dea all");
        assert!(!has_revisions(&doc));
        history.undo(&mut doc);
        assert!(has_revisions(&doc));
    }

    #[test]
    fn revision_at_cursor_and_navigation() {
        let mut doc = doc();
        mark_deleted(&mut doc, Position::new(0, 0), Position::new(0, 5), &me());
        mark_inserted(&mut doc, Position::new(1, 0), Position::new(1, 3), &me());
        let first = revision_at(&doc, Position::new(0, 2)).unwrap();
        assert_eq!(first.kind, Delete);
        assert_eq!(
            revision_range(&doc, &first.id),
            Some((Position::new(0, 0), Position::new(0, 5)))
        );
        let next = next_revision(&doc, Position::new(0, 2), true).unwrap();
        assert_eq!(next.0, Position::new(1, 0));
        // По кругу.
        let wrapped = next_revision(&doc, Position::new(1, 2), true).unwrap();
        assert_eq!(wrapped.0, Position::new(0, 0));
        let previous = next_revision(&doc, Position::new(0, 0), false).unwrap();
        assert_eq!(previous.0, Position::new(1, 0));
        // Принять одно исправление у курсора.
        let mut history = History::new();
        history.run(
            &mut doc,
            Selection::caret(Position::new(1, 1)),
            Command::ResolveRevisions {
                accept: false,
                all: false,
            },
        );
        assert_eq!(text(&doc, 1), "ond");
        assert!(revision_at(&doc, Position::new(0, 2)).is_some());
    }
}
