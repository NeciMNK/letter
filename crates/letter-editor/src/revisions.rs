//! Исправления (рецензирование): правка с записью исправлений, принять
//! и отклонить.
//!
//! При записи исправлений набранный текст получает исправление
//! «вставлено», а стёртый не исчезает, а получает «удалено» (свой ещё
//! не принятый вставленный текст стирается по-настоящему, как в Word).
//! Знаки абзацев пока не записываются: Enter делит абзац как обычно,
//! а Backspace в начале абзаца только сдвигает курсор.

use letter_model::{Document, Inline, Paragraph, Revision, RevisionKind, Run};
use unicode_segmentation::UnicodeSegmentation;

use crate::Position;
use crate::edit::{
    insert_text, paragraph_count, paragraph_mut, paragraph_ref, split_content, tidy,
};

/// Исправление этого вида от `author` с текущим временем.
pub fn revision_now(kind: RevisionKind, author: &str) -> Revision {
    Revision {
        kind,
        author: author.to_string(),
        date: Some(now_w3c()),
    }
}

/// Текущее время UTC в формате файла: `2024-05-01T10:00:00Z`.
pub fn now_w3c() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    w3c(secs)
}

/// Секунды от 1970 года в формате файла (UTC).
fn w3c(secs: u64) -> String {
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rest = secs % 86_400;
    // Дата из числа дней (алгоритм Г. Хиннанта).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

/// Длина фрагмента в тексте абзаца.
fn run_len(run: &Run) -> usize {
    run.content
        .iter()
        .map(letter_model::RunContent::text_len)
        .sum()
}

/// Вырезает из абзаца кусок `from..to` (фрагменты на краях разрезаются),
/// отдаёт его `change` и вставляет то, что она вернула, обратно.
fn edit_piece(
    paragraph: &mut Paragraph,
    from: usize,
    to: usize,
    change: impl FnOnce(Vec<Inline>) -> Vec<Inline>,
) {
    let tail = split_content(paragraph, to);
    let middle = split_content(paragraph, from);
    paragraph.content.extend(change(middle));
    paragraph.content.extend(tail);
    tidy(paragraph);
}

/// Набирает `text` у `at` как вставку `revision`. Возвращает место после
/// текста.
pub fn insert_tracked(
    doc: &mut Document,
    at: Position,
    text: &str,
    revision: &Revision,
) -> Position {
    let end = insert_text(doc, at, text);
    if end.paragraph == at.paragraph
        && let Some(paragraph) = paragraph_mut(doc, at.paragraph)
    {
        edit_piece(paragraph, at.offset, end.offset, |mut items| {
            for item in &mut items {
                if let Inline::Run(run) = item {
                    run.props.revision = Some(revision.clone());
                }
            }
            items
        });
    }
    end
}

/// Стирает кусок от `start` до `end` с записью исправлений: вставленный
/// (ещё не принятый) текст исчезает, остальной помечается удалённым
/// `revision`. Знаки абзацев остаются. Возвращает, где оказался `end`.
pub fn delete_tracked(
    doc: &mut Document,
    start: Position,
    end: Position,
    revision: &Revision,
) -> Position {
    let (start, end) = if (start.paragraph, start.offset) <= (end.paragraph, end.offset) {
        (start, end)
    } else {
        (end, start)
    };
    let mut end_offset = end.offset;
    for index in start.paragraph..=end.paragraph {
        let Some(paragraph) = paragraph_mut(doc, index) else {
            continue;
        };
        let len: usize = paragraph.runs().map(run_len).sum();
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
        let mut removed = 0;
        edit_piece(paragraph, from, to, |items| {
            items
                .into_iter()
                .filter_map(|item| match item {
                    Inline::Run(run)
                        if run
                            .props
                            .revision
                            .as_ref()
                            .is_some_and(|r| r.kind == RevisionKind::Insert) =>
                    {
                        removed += run_len(&run);
                        None
                    }
                    Inline::Run(mut run) => {
                        if run.props.revision.is_none() {
                            run.props.revision = Some(revision.clone());
                        }
                        Some(Inline::Run(run))
                    }
                    other => Some(other),
                })
                .collect()
        });
        if index == end.paragraph {
            end_offset = to - removed;
        }
    }
    Position::new(end.paragraph, end_offset)
}

/// Принимает (`accept`) или отклоняет исправления в абзацах `range`
/// (все абзацы, если `None`): принятая вставка и отклонённое удаление
/// становятся обычным текстом, принятое удаление и отклонённая вставка
/// исчезают. Абзац (не в таблице), весь текст которого исчез, исчезает
/// целиком: так ведут себя целиком удалённые или вставленные абзацы
/// (например, после сравнения документов).
pub fn resolve_revisions(doc: &mut Document, range: Option<(usize, usize)>, accept: bool) {
    let (first, last) = range.unwrap_or((0, paragraph_count(doc).saturating_sub(1)));
    let mut emptied = Vec::new();
    for index in first..=last {
        let Some(paragraph) = paragraph_mut(doc, index) else {
            continue;
        };
        let mut changed = false;
        let had_text = paragraph.runs().any(|r| run_len(r) > 0);
        paragraph.content.retain_mut(|item| {
            let Inline::Run(run) = item else {
                return true;
            };
            let Some(revision) = run.props.revision.take() else {
                return true;
            };
            changed = true;
            // Удаляется: принятое удаление или отклонённая вставка.
            (revision.kind == RevisionKind::Insert) == accept
        });
        if changed {
            tidy(paragraph);
            if had_text && !paragraph.runs().any(|r| run_len(r) > 0) {
                emptied.push(index);
            }
        }
    }
    // С конца, чтобы номера ещё не удалённых не сдвигались.
    for index in emptied.into_iter().rev() {
        let Some(path) = crate::edit::find(doc, index) else {
            continue;
        };
        if !path.cells.is_empty() {
            continue;
        }
        let Some(section) = doc.sections.get_mut(path.section) else {
            continue;
        };
        // В разделе остаётся хотя бы один блок.
        if section.blocks.len() > 1 && path.block < section.blocks.len() {
            section.blocks.remove(path.block);
        }
    }
}

/// Есть ли в абзацах `range` (во всём документе, если `None`)
/// исправления.
pub fn has_revisions(doc: &Document, range: Option<(usize, usize)>) -> bool {
    let (first, last) = range.unwrap_or((0, usize::MAX));
    (first..=last)
        .map_while(|i| paragraph_ref(doc, i))
        .any(|p| p.runs().any(|r| r.props.revision.is_some()))
}

/// Начало следующего (`forward`) или предыдущего исправления от `at`;
/// `None` — больше нет.
pub fn next_revision(doc: &Document, at: Position, forward: bool) -> Option<Position> {
    let count = paragraph_count(doc);
    // Начала исправлений: (абзац, место).
    let mut starts = Vec::new();
    for index in 0..count {
        let Some(paragraph) = paragraph_ref(doc, index) else {
            continue;
        };
        let mut pos = 0;
        let mut previous: Option<&Revision> = None;
        for run in paragraph.runs() {
            let current = run.props.revision.as_ref();
            if current.is_some() && current != previous {
                starts.push((index, pos));
            }
            previous = current;
            pos += run_len(run);
        }
    }
    let here = (at.paragraph, at.offset);
    let found = if forward {
        starts.into_iter().find(|&s| s > here)
    } else {
        starts.into_iter().rev().find(|&s| s < here)
    };
    found.map(|(p, o)| Position::new(p, o))
}

/// Где кончается графема перед `offset` в абзаце (для Backspace)
/// или после него (для Delete); `None` — край абзаца.
pub fn grapheme_step(doc: &Document, at: Position, forward: bool) -> Option<usize> {
    let text = paragraph_ref(doc, at.paragraph)?.plain_text();
    let offset = at.offset.min(text.len());
    if forward {
        text[offset..]
            .graphemes(true)
            .next()
            .map(|g| offset + g.len())
    } else {
        text[..offset]
            .graphemes(true)
            .next_back()
            .map(|g| offset - g.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::Paragraph;

    fn doc(text: &str) -> Document {
        let mut d = Document::new();
        d.sections[0].blocks = vec![Paragraph::text(text).into()];
        d
    }

    fn runs(d: &Document) -> Vec<(String, Option<RevisionKind>)> {
        paragraph_ref(d, 0)
            .map(|p| {
                p.runs()
                    .map(|r| (r.plain_text(), r.props.revision.as_ref().map(|r| r.kind)))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn rev(kind: RevisionKind) -> Revision {
        Revision {
            kind,
            author: "A".into(),
            date: None,
        }
    }

    #[test]
    fn dates_are_written_like_word() {
        assert_eq!(w3c(0), "1970-01-01T00:00:00Z");
        assert_eq!(w3c(1_714_557_600), "2024-05-01T10:00:00Z");
        assert_eq!(w3c(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(now_w3c().len(), 20);
    }

    #[test]
    fn typing_and_deleting_are_tracked() {
        let mut d = doc("abcd");
        let end = insert_tracked(
            &mut d,
            Position::new(0, 2),
            "XY",
            &rev(RevisionKind::Insert),
        );
        assert_eq!(end, Position::new(0, 4));
        assert_eq!(
            runs(&d),
            [
                ("ab".into(), None),
                ("XY".into(), Some(RevisionKind::Insert)),
                ("cd".into(), None)
            ]
        );
        // Стирание «bXYc»: своё вставленное исчезает, остальное — удалено.
        let end = delete_tracked(
            &mut d,
            Position::new(0, 1),
            Position::new(0, 5),
            &rev(RevisionKind::Delete),
        );
        assert_eq!(end, Position::new(0, 3));
        assert_eq!(
            runs(&d),
            [
                ("a".into(), None),
                ("bc".into(), Some(RevisionKind::Delete)),
                ("d".into(), None)
            ]
        );
        assert!(has_revisions(&d, None));
        assert_eq!(
            next_revision(&d, Position::new(0, 0), true),
            Some(Position::new(0, 1))
        );
        assert_eq!(next_revision(&d, Position::new(0, 1), true), None);
        assert_eq!(grapheme_step(&d, Position::new(0, 1), false), Some(0));
        assert_eq!(grapheme_step(&d, Position::new(0, 0), false), None);
    }

    #[test]
    fn accept_and_reject() {
        let mut d = doc("abcd");
        insert_tracked(&mut d, Position::new(0, 4), "+", &rev(RevisionKind::Insert));
        delete_tracked(
            &mut d,
            Position::new(0, 0),
            Position::new(0, 1),
            &rev(RevisionKind::Delete),
        );
        let mut accepted = d.clone();
        resolve_revisions(&mut accepted, None, true);
        assert_eq!(accepted.plain_text(), "bcd+");
        assert!(!has_revisions(&accepted, None));
        let mut rejected = d.clone();
        resolve_revisions(&mut rejected, Some((0, 0)), false);
        // Целиком вставленный абзац при отклонении исчезает.
        let mut two = doc("a");
        two.sections[0].blocks.push(Paragraph::text("новый").into());
        crate::edit::paragraph_mut(&mut two, 1)
            .into_iter()
            .flat_map(|p| p.content.iter_mut())
            .for_each(|i| {
                if let Inline::Run(r) = i {
                    r.props.revision = Some(rev(RevisionKind::Insert));
                }
            });
        resolve_revisions(&mut two, None, false);
        assert_eq!(two.sections[0].blocks.len(), 1);
        assert_eq!(rejected.plain_text(), "abcd");
        assert_eq!(runs(&rejected), [("abcd".into(), None)]);
    }
}
