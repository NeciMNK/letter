//! «Рецензирование → Сравнить»: разница двух версий документа в виде
//! исправлений, как в Word.
//!
//! За основу берётся новая версия (её оформление, разделы, примечания).
//! Абзацы обеих версий сопоставляются по тексту (наибольшая общая
//! подпоследовательность); абзацы между совпавшими, стоящие друг против
//! друга, сравниваются по словам: новые слова помечаются вставкой, а
//! исчезнувшие возвращаются в абзац зачёркнутыми, со своим старым
//! оформлением. Лишние новые абзацы — вставки целиком, лишние старые —
//! удаления целиком (после принятия такой абзац исчезает, см.
//! [`crate::resolve_revisions`]). Таблицы сравниваются целиком.

use letter_model::{Block, Document, Inline, Paragraph, Revision, RevisionKind, RunContent};

use crate::edit::{plain_text, split_content, tidy};
use crate::revisions::{Reviewer, change_paragraph, next_id};

/// Выдаёт номера новых исправлений.
struct Ids {
    next: u64,
    reviewer: Reviewer,
}

impl Ids {
    fn revision(&mut self, kind: RevisionKind) -> Revision {
        let id = self.next.to_string();
        self.next += 1;
        Revision {
            kind,
            id,
            author: self.reviewer.author.clone(),
            date: self.reviewer.date.clone(),
        }
    }
}

/// Сравнивает версии: `original` — прежняя, `revised` — новая.
/// Возвращает новую версию, где разница отмечена исправлениями от
/// имени `reviewer`.
pub fn compare_documents(original: &Document, revised: &Document, reviewer: &Reviewer) -> Document {
    let mut result = revised.clone();
    let mut ids = Ids {
        next: next_id(revised).parse().unwrap_or(0),
        reviewer: reviewer.clone(),
    };
    let old: Vec<&Block> = original.sections.iter().flat_map(|s| &s.blocks).collect();
    // Блоки новой версии с номером раздела.
    let new: Vec<(usize, &Block)> = revised
        .sections
        .iter()
        .enumerate()
        .flat_map(|(i, s)| s.blocks.iter().map(move |b| (i, b)))
        .collect();
    let new_blocks: Vec<&Block> = new.iter().map(|(_, b)| *b).collect();
    let pairs = common(&old, &new_blocks, same_block);

    let mut sections: Vec<Vec<Block>> = vec![Vec::new(); revised.sections.len()];
    // Удалённые блоки ждут следующего блока новой версии, чтобы встать в
    // его раздел перед ним.
    let mut pending: Vec<Block> = Vec::new();
    let (mut i, mut j) = (0, 0);
    for (oi, nj) in pairs.into_iter().chain([(old.len(), new.len())]) {
        // Промежуток: старые old[i..oi] против новых new[j..nj].
        let gap_old = &old[i..oi];
        let gap_new = &new[j..nj];
        let paired = gap_old.len().min(gap_new.len());
        for (k, (section, block)) in gap_new.iter().enumerate() {
            let changed = match (gap_old.get(k), block) {
                // Пустой абзац с непустым не сравнивается: после принятия
                // или отклонения абзац должен остаться или исчезнуть
                // целиком.
                (Some(Block::Paragraph(before)), Block::Paragraph(after))
                    if k < paired
                        && !plain_text(before).is_empty()
                        && !plain_text(after).is_empty() =>
                {
                    Block::Paragraph(compare_paragraphs(before, after, &mut ids))
                }
                (Some(before), _) if k < paired => {
                    delete_block(before, &mut pending, &mut ids);
                    let mut added = (*block).clone();
                    mark_block(&mut added, ids.revision(RevisionKind::Insert));
                    added
                }
                _ => {
                    let mut added = (*block).clone();
                    mark_block(&mut added, ids.revision(RevisionKind::Insert));
                    added
                }
            };
            emit(&mut sections, &mut pending, *section, changed);
        }
        for before in gap_old.iter().skip(paired) {
            delete_block(before, &mut pending, &mut ids);
        }
        if let Some((section, block)) = new.get(nj) {
            emit(&mut sections, &mut pending, *section, (*block).clone());
        }
        (i, j) = (oi + 1, nj + 1);
    }
    if let Some(last) = sections.last_mut() {
        last.append(&mut pending);
    }
    for (section, blocks) in result.sections.iter_mut().zip(sections) {
        section.blocks = blocks;
    }
    result
}

/// Ставит блок новой версии в раздел `at`, а перед ним — ждущие
/// удалённые блоки.
fn emit(sections: &mut [Vec<Block>], pending: &mut Vec<Block>, at: usize, block: Block) {
    sections[at].append(pending);
    sections[at].push(block);
}

/// Удалённый блок прежней версии, зачёркнутый. Пустой абзац не
/// возвращается: пометить в нём нечего.
fn delete_block(block: &Block, pending: &mut Vec<Block>, ids: &mut Ids) {
    if matches!(block, Block::Paragraph(p) if plain_text(p).is_empty()) {
        return;
    }
    let mut gone = block.clone();
    mark_block(&mut gone, ids.revision(RevisionKind::Delete));
    pending.push(gone);
}

/// Одинаковы ли блоки: абзацы — по тексту, таблицы — целиком.
fn same_block(a: &&Block, b: &&Block) -> bool {
    match (a, b) {
        (Block::Paragraph(a), Block::Paragraph(b)) => plain_text(a) == plain_text(b),
        _ => a == b,
    }
}

/// Помечает весь текст блока исправлением.
fn mark_block(block: &mut Block, revision: Revision) {
    match block {
        Block::Paragraph(p) => mark_paragraph(p, &revision),
        Block::Table(table) => {
            for row in &mut table.rows {
                for cell in &mut row.cells {
                    for block in &mut cell.blocks {
                        mark_block(block, revision.clone());
                    }
                }
            }
        }
        _ => {}
    }
}

fn mark_paragraph(paragraph: &mut Paragraph, revision: &Revision) {
    if revision.kind == RevisionKind::Delete {
        // Закладки и примечания прежней версии в новую не переходят.
        paragraph.content.retain(|c| matches!(c, Inline::Run(_)));
        for inline in &mut paragraph.content {
            if let Inline::Run(run) = inline {
                run.content
                    .retain(|c| !matches!(c, RunContent::CommentReference(_)));
            }
        }
    }
    let len = plain_text(paragraph).len();
    change_paragraph(paragraph, 0, len, &mut |run| {
        run.props.revision = Some(revision.clone());
        true
    });
}

/// Сравнивает абзацы по словам: новый абзац с вставками и зачёркнутыми
/// старыми словами.
fn compare_paragraphs(before: &Paragraph, after: &Paragraph, ids: &mut Ids) -> Paragraph {
    let old_text = plain_text(before);
    let new_text = plain_text(after);
    let old_words = words(&old_text);
    let new_words = words(&new_text);
    let pairs = common(&old_words, &new_words, |a, b| a.1 == b.1);
    let mut result = after.clone();
    // Место в результате: растёт и на вставленном, и на возвращённом
    // старом тексте.
    let mut offset = 0;
    let (mut i, mut j) = (0, 0);
    for (oi, nj) in pairs
        .into_iter()
        .chain([(old_words.len(), new_words.len())])
    {
        let span = |list: &[(usize, &str)], from: usize, to: usize, total: usize| {
            let start = list.get(from).map_or(total, |w| w.0);
            let end = list.get(to).map_or(total, |w| w.0);
            (start, end)
        };
        let (old_from, old_to) = span(&old_words, i, oi, old_text.len());
        let (new_from, new_to) = span(&new_words, j, nj, new_text.len());
        if old_to > old_from {
            // Старый кусок со своим оформлением, зачёркнутый.
            let mut piece = before.clone();
            split_content(&mut piece, old_to);
            let mut gone = Paragraph {
                content: split_content(&mut piece, old_from),
                ..Paragraph::default()
            };
            mark_paragraph(&mut gone, &ids.revision(RevisionKind::Delete));
            let tail = split_content(&mut result, offset);
            result.content.extend(gone.content);
            result.content.extend(tail);
            tidy(&mut result);
            offset += old_to - old_from;
        }
        if new_to > new_from {
            let revision = ids.revision(RevisionKind::Insert);
            change_paragraph(
                &mut result,
                offset,
                offset + new_to - new_from,
                &mut |run| {
                    run.props.revision = Some(revision.clone());
                    true
                },
            );
            offset += new_to - new_from;
        }
        if let Some(word) = new_words.get(nj) {
            offset += word.1.len();
        }
        (i, j) = (oi + 1, nj + 1);
    }
    result
}

/// Слова и промежутки между ними (каждый пробел и знак препинания —
/// отдельно) с их смещениями.
fn words(text: &str) -> Vec<(usize, &str)> {
    let mut list = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        if c.is_alphanumeric() {
            start.get_or_insert(i);
            continue;
        }
        if let Some(s) = start.take() {
            list.push((s, &text[s..i]));
        }
        list.push((i, &text[i..i + c.len_utf8()]));
    }
    if let Some(s) = start {
        list.push((s, &text[s..]));
    }
    list
}

/// Наибольшая общая подпоследовательность: пары номеров совпавших
/// элементов по возрастанию. Одинаковые начало и конец отрезаются
/// сразу, остальное — обычной таблицей.
fn common<T>(a: &[T], b: &[T], same: impl Fn(&T, &T) -> bool) -> Vec<(usize, usize)> {
    let mut head = 0;
    while head < a.len() && head < b.len() && same(&a[head], &b[head]) {
        head += 1;
    }
    let mut tail = 0;
    while tail < a.len() - head
        && tail < b.len() - head
        && same(&a[a.len() - 1 - tail], &b[b.len() - 1 - tail])
    {
        tail += 1;
    }
    let (n, m) = (a.len() - head - tail, b.len() - head - tail);
    // Слишком большая таблица (десятки тысяч изменённых абзацев): середина
    // считается изменённой целиком.
    if (n + 1).saturating_mul(m + 1) > 16_000_000 {
        let mut pairs: Vec<(usize, usize)> = (0..head).map(|k| (k, k)).collect();
        pairs.extend((0..tail).map(|k| (a.len() - tail + k, b.len() - tail + k)));
        return pairs;
    }
    // lengths[x][y] — длина общей части a[head+x..] и b[head+y..].
    let mut lengths = vec![vec![0u32; m + 1]; n + 1];
    for x in (0..n).rev() {
        for y in (0..m).rev() {
            lengths[x][y] = if same(&a[head + x], &b[head + y]) {
                lengths[x + 1][y + 1] + 1
            } else {
                lengths[x + 1][y].max(lengths[x][y + 1])
            };
        }
    }
    let mut pairs: Vec<(usize, usize)> = (0..head).map(|k| (k, k)).collect();
    let (mut x, mut y) = (0, 0);
    while x < n && y < m {
        if same(&a[head + x], &b[head + y]) {
            pairs.push((head + x, head + y));
            x += 1;
            y += 1;
        } else if lengths[x + 1][y] >= lengths[x][y + 1] {
            x += 1;
        } else {
            y += 1;
        }
    }
    pairs.extend((0..tail).map(|k| (a.len() - tail + k, b.len() - tail + k)));
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Position, has_revisions, resolve_revisions};
    use letter_model::{Run, RunProps};

    fn doc(paragraphs: &[&str]) -> Document {
        let mut doc = Document::new();
        doc.sections[0].blocks = paragraphs
            .iter()
            .map(|t| Paragraph::text(*t).into())
            .collect();
        doc
    }

    fn me() -> Reviewer {
        Reviewer {
            author: "Сравнение".into(),
            date: None,
        }
    }

    fn texts(doc: &Document) -> Vec<String> {
        doc.blocks()
            .filter_map(|b| match b {
                Block::Paragraph(p) => Some(plain_text(p)),
                _ => None,
            })
            .collect()
    }

    /// Принимает или отклоняет все исправления.
    fn resolve(doc: &Document, accept: bool) -> Vec<String> {
        let mut doc = doc.clone();
        let last = crate::edit::paragraph_count(&doc) - 1;
        let end = crate::edit::paragraph_ref(&doc, last).map_or(0, crate::edit::text_len);
        resolve_revisions(
            &mut doc,
            Position::new(0, 0),
            Position::new(last, end),
            None,
            accept,
        );
        assert!(!has_revisions(&doc));
        texts(&doc)
    }

    #[test]
    fn words_and_paragraphs_are_compared() {
        let old = doc(&[
            "Один",
            "The cat sat on the mat.",
            "Удалённый абзац",
            "Конец",
        ]);
        let new = doc(&[
            "Один",
            "The black cat sat on a mat.",
            "Новый абзац",
            "Ещё",
            "Конец",
        ]);
        let result = compare_documents(&old, &new, &me());
        let lines = texts(&result);
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[1], "The black cat sat on thea mat.");
        // Принять всё — новая версия, отклонить всё — прежняя.
        assert_eq!(resolve(&result, true), texts(&new));
        assert_eq!(resolve(&result, false), texts(&old));
    }

    #[test]
    fn removed_and_added_paragraphs() {
        let old = doc(&["А", "Б", "В"]);
        let new = doc(&["А", "В", "Г"]);
        let result = compare_documents(&old, &new, &me());
        assert_eq!(texts(&result), ["А", "Б", "В", "Г"]);
        assert_eq!(resolve(&result, true), texts(&new));
        assert_eq!(resolve(&result, false), texts(&old));
        // Одинаковые версии — без исправлений.
        assert!(!has_revisions(&compare_documents(&old, &old, &me())));
    }

    #[test]
    fn deleted_words_keep_their_formatting() {
        let mut old = doc(&[]);
        let bold = RunProps {
            bold: Some(true),
            ..RunProps::default()
        };
        old.sections[0].blocks = vec![
            Paragraph {
                content: vec![
                    Inline::Run(Run::text("Very ")),
                    Inline::Run(Run {
                        props: bold.clone(),
                        content: vec![RunContent::Text("bold".into())],
                    }),
                    Inline::Run(Run::text(" text")),
                ],
                ..Paragraph::default()
            }
            .into(),
        ];
        let new = doc(&["Very text"]);
        let result = compare_documents(&old, &new, &me());
        let Some(Block::Paragraph(p)) = result.blocks().next() else {
            panic!("нет абзаца");
        };
        let gone: Vec<&Run> = p
            .runs()
            .filter(|r| {
                r.props
                    .revision
                    .as_ref()
                    .is_some_and(|r| r.kind == RevisionKind::Delete)
            })
            .collect();
        assert!(gone.iter().any(|r| r.props.bold == Some(true)));
        assert_eq!(resolve(&result, false), ["Very bold text"]);
    }
}
