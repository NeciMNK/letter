//! Сравнение двух документов («Рецензирование → Сравнить»): из исходного
//! и изменённого документа получается третий — изменённый, в котором
//! отличия от исходного записаны исправлениями (вставками и удалениями).
//!
//! Сначала сопоставляются абзацы (наибольшая общая подпоследовательность
//! по тексту), затем в парах изменённых абзацев — слова. Таблицы и другие
//! блоки сравниваются целиком: изменённая таблица берётся из нового
//! документа без пометок.

use letter_model::{Block, Document, Inline, Paragraph, Revision, RevisionKind, Run, RunContent};
use unicode_segmentation::UnicodeSegmentation;

/// Больше стольких пар (абзацев или слов) наибольшая общая
/// подпоследовательность не ищется: всё различие — целиком.
const MAX_CELLS: usize = 25_000_000;

/// Сравнивает исходный документ `original` с изменённым `revised`.
/// Возвращает изменённый документ, где отличия — исправления автора
/// `author` с датой `date`.
pub fn compare_documents(
    original: &Document,
    revised: &Document,
    author: &str,
    date: Option<String>,
) -> Document {
    let make = |kind| Revision {
        kind,
        author: author.to_string(),
        date: date.clone(),
    };
    let (insert, delete) = (make(RevisionKind::Insert), make(RevisionKind::Delete));
    let old: Vec<&Block> = original.blocks().collect();
    // Блоки нового документа с номером раздела.
    let new: Vec<(usize, &Block)> = revised
        .sections
        .iter()
        .enumerate()
        .flat_map(|(s, section)| section.blocks.iter().map(move |b| (s, b)))
        .collect();
    let old_keys: Vec<String> = old.iter().map(|b| block_key(b)).collect();
    let new_keys: Vec<String> = new.iter().map(|(_, b)| block_key(b)).collect();
    let script = diff(&old_keys, &new_keys);

    // Блоки результата с разделом (удалённые идут в раздел следующего
    // нового блока).
    let mut out: Vec<(Option<usize>, Block)> = Vec::new();
    let mut removed: Vec<&Block> = Vec::new();
    let mut added: Vec<usize> = Vec::new();
    let flush = |out: &mut Vec<(Option<usize>, Block)>,
                 removed: &mut Vec<&Block>,
                 added: &mut Vec<usize>| {
        let pairs = removed.len().min(added.len());
        for (i, block) in removed.iter().enumerate() {
            if i < pairs {
                let (section, new_block) = new[added[i]];
                out.push((
                    Some(section),
                    changed_block(block, new_block, &insert, &delete),
                ));
            } else {
                out.push((None, marked(block, &delete)));
            }
        }
        for &j in &added[pairs..] {
            let (section, block) = new[j];
            out.push((Some(section), marked(block, &insert)));
        }
        removed.clear();
        added.clear();
    };
    for op in script {
        match op {
            Op::Same(_, j) => {
                flush(&mut out, &mut removed, &mut added);
                let (section, block) = new[j];
                out.push((Some(section), block.clone()));
            }
            Op::Removed(i) => removed.push(old[i]),
            Op::Added(j) => added.push(j),
        }
    }
    flush(&mut out, &mut removed, &mut added);

    let mut result = revised.clone();
    for section in &mut result.sections {
        section.blocks.clear();
    }
    let last = result.sections.len().saturating_sub(1);
    let mut pending = Vec::new();
    for (section, block) in out {
        match section {
            None => pending.push(block),
            Some(s) => {
                if let Some(target) = result.sections.get_mut(s) {
                    target.blocks.append(&mut pending);
                    target.blocks.push(block);
                }
            }
        }
    }
    if let Some(target) = result.sections.get_mut(last) {
        target.blocks.append(&mut pending);
    }
    for section in &mut result.sections {
        if section.blocks.is_empty() {
            section.blocks.push(Paragraph::default().into());
        }
    }
    result
}

/// По чему сопоставляются блоки: текст абзаца, текст таблицы и т. п.
fn block_key(block: &Block) -> String {
    match block {
        Block::Paragraph(p) => format!("p{}", p.plain_text()),
        Block::Table(_) => format!("t{}", block.plain_text()),
        Block::Unknown(raw) => format!("u{}", raw.as_str()),
    }
}

/// Блок целиком вставлен или удалён: у абзаца все фрагменты получают
/// исправление, другие блоки берутся как есть.
fn marked(block: &Block, revision: &Revision) -> Block {
    match block {
        Block::Paragraph(p) => {
            let mut p = p.clone();
            for item in &mut p.content {
                if let Inline::Run(run) = item {
                    run.props.revision = Some(revision.clone());
                }
            }
            Block::Paragraph(p)
        }
        other => other.clone(),
    }
}

/// Пара изменённых блоков: абзацы сравниваются по словам, остальное —
/// новый блок без пометок (абзац вместо таблицы — удаление и вставка
/// не помещаются в один блок, поэтому тоже новый блок).
fn changed_block(old: &Block, new: &Block, insert: &Revision, delete: &Revision) -> Block {
    match (old, new) {
        (Block::Paragraph(a), Block::Paragraph(b)) => {
            Block::Paragraph(compare_paragraphs(a, b, insert, delete))
        }
        _ => new.clone(),
    }
}

/// Только текст, табуляции и разрывы: такие абзацы режутся по словам.
fn plain_only(p: &Paragraph) -> bool {
    p.content.iter().all(|item| match item {
        Inline::Run(run) => run.content.iter().all(|c| {
            matches!(
                c,
                RunContent::Text(_) | RunContent::Tab | RunContent::Break(_)
            )
        }),
        _ => false,
    })
}

/// Абзац `new`, где отличия от `old` по словам записаны исправлениями.
/// Абзац с полями, рисунками и т. п. — удаление старого текста и вставка
/// нового целиком.
fn compare_paragraphs(
    old: &Paragraph,
    new: &Paragraph,
    insert: &Revision,
    delete: &Revision,
) -> Paragraph {
    let mut result = Paragraph {
        props: new.props.clone(),
        content: Vec::new(),
    };
    let mark = |runs: Vec<Run>, revision: &Revision| -> Vec<Inline> {
        runs.into_iter()
            .map(|mut r| {
                r.props.revision = Some(revision.clone());
                Inline::Run(r)
            })
            .collect()
    };
    if !plain_only(old) || !plain_only(new) {
        result
            .content
            .extend(mark(slice_runs(old, 0, usize::MAX), delete));
        result
            .content
            .extend(mark(slice_runs(new, 0, usize::MAX), insert));
        return result;
    }
    let (old_text, new_text) = (old.plain_text(), new.plain_text());
    let old_words: Vec<(usize, &str)> = old_text.split_word_bound_indices().collect();
    let new_words: Vec<(usize, &str)> = new_text.split_word_bound_indices().collect();
    let a: Vec<&str> = old_words.iter().map(|w| w.1).collect();
    let b: Vec<&str> = new_words.iter().map(|w| w.1).collect();
    let span = |words: &[(usize, &str)], i: usize| (words[i].0, words[i].0 + words[i].1.len());
    for op in diff(&a, &b) {
        match op {
            Op::Same(_, j) => {
                let (from, to) = span(&new_words, j);
                result
                    .content
                    .extend(slice_runs(new, from, to).into_iter().map(Inline::Run));
            }
            Op::Removed(i) => {
                let (from, to) = span(&old_words, i);
                result
                    .content
                    .extend(mark(slice_runs(old, from, to), delete));
            }
            Op::Added(j) => {
                let (from, to) = span(&new_words, j);
                result
                    .content
                    .extend(mark(slice_runs(new, from, to), insert));
            }
        }
    }
    merge_runs(&mut result);
    result
}

/// Соседние фрагменты с одинаковым оформлением и исправлением —
/// в один.
fn merge_runs(p: &mut Paragraph) {
    let mut merged: Vec<Inline> = Vec::new();
    for item in std::mem::take(&mut p.content) {
        if let (Some(Inline::Run(last)), Inline::Run(run)) = (merged.last_mut(), &item)
            && last.props == run.props
        {
            for c in &run.content {
                match (last.content.last_mut(), c) {
                    (Some(RunContent::Text(a)), RunContent::Text(b)) => a.push_str(b),
                    _ => last.content.push(c.clone()),
                }
            }
            continue;
        }
        merged.push(item);
    }
    p.content = merged;
}

/// Фрагменты абзаца с текстом от `from` до `to` (байты текста абзаца),
/// с их оформлением.
fn slice_runs(p: &Paragraph, from: usize, to: usize) -> Vec<Run> {
    let mut out = Vec::new();
    let mut pos = 0;
    for run in p.runs() {
        let mut piece = Run {
            props: run.props.clone(),
            content: Vec::new(),
        };
        for item in &run.content {
            let len = item.text_len();
            let (a, b) = (pos, pos + len);
            pos = b;
            if len == 0 {
                if from <= a && a < to {
                    piece.content.push(item.clone());
                }
                continue;
            }
            if b <= from || a >= to {
                continue;
            }
            match item {
                RunContent::Text(text) => {
                    let start = from.saturating_sub(a);
                    let end = (to - a).min(len);
                    if let Some(part) = text.get(start..end) {
                        piece.content.push(RunContent::Text(part.to_string()));
                    }
                }
                other => piece.content.push(other.clone()),
            }
        }
        if !piece.content.is_empty() {
            out.push(piece);
        }
    }
    out
}

/// Шаг сравнения двух последовательностей.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    /// Одинаковые: номер в первой и во второй.
    Same(usize, usize),
    /// Есть только в первой.
    Removed(usize),
    /// Есть только во второй.
    Added(usize),
}

/// Наименьший набор шагов от `a` к `b` (наибольшая общая
/// подпоследовательность). Одинаковые начало и конец отрезаются сразу.
fn diff<T: PartialEq>(a: &[T], b: &[T]) -> Vec<Op> {
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (am, bm) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    let mut ops: Vec<Op> = (0..prefix).map(|i| Op::Same(i, i)).collect();
    let (n, m) = (am.len(), bm.len());
    if n * m > MAX_CELLS {
        ops.extend((0..n).map(|i| Op::Removed(prefix + i)));
        ops.extend((0..m).map(|j| Op::Added(prefix + j)));
    } else {
        // lcs[i][j] — длина общей части am[i..] и bm[j..].
        let width = m + 1;
        let mut lcs = vec![0u32; (n + 1) * width];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                lcs[i * width + j] = if am[i] == bm[j] {
                    lcs[(i + 1) * width + j + 1] + 1
                } else {
                    lcs[(i + 1) * width + j].max(lcs[i * width + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n || j < m {
            if i < n && j < m && am[i] == bm[j] {
                ops.push(Op::Same(prefix + i, prefix + j));
                i += 1;
                j += 1;
            } else if j < m && (i == n || lcs[i * width + j + 1] >= lcs[(i + 1) * width + j]) {
                ops.push(Op::Added(prefix + j));
                j += 1;
            } else {
                ops.push(Op::Removed(prefix + i));
                i += 1;
            }
        }
        // Удаления раньше вставок на одном месте: так пары абзацев
        // совпадают по порядку.
        let mut k = 0;
        while k < ops.len() {
            let end = (k..ops.len())
                .find(|&e| matches!(ops[e], Op::Same(..)))
                .unwrap_or(ops.len());
            ops[k..end].sort_by_key(|op| !matches!(op, Op::Removed(_)));
            k = end + 1;
        }
    }
    let (sa, sb) = (a.len() - suffix, b.len() - suffix);
    ops.extend((0..suffix).map(|k| Op::Same(sa + k, sb + k)));
    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::revisions::resolve_revisions;

    fn doc(paragraphs: &[&str]) -> Document {
        let mut d = Document::new();
        d.sections[0].blocks = paragraphs
            .iter()
            .map(|t| Paragraph::text(*t).into())
            .collect();
        d
    }

    fn marks(d: &Document) -> Vec<Vec<(String, Option<RevisionKind>)>> {
        d.blocks()
            .map(|b| match b {
                Block::Paragraph(p) => p
                    .runs()
                    .map(|r| (r.plain_text(), r.props.revision.as_ref().map(|r| r.kind)))
                    .collect(),
                _ => Vec::new(),
            })
            .collect()
    }

    #[test]
    fn diff_finds_the_common_part() {
        let ops = diff(&["a", "b", "c", "d"], &["a", "x", "c", "d", "e"]);
        assert_eq!(
            ops,
            [
                Op::Same(0, 0),
                Op::Removed(1),
                Op::Added(1),
                Op::Same(2, 2),
                Op::Same(3, 3),
                Op::Added(4)
            ]
        );
        assert_eq!(diff::<&str>(&[], &[]), []);
    }

    #[test]
    fn words_and_paragraphs_become_revisions() {
        let old = doc(&["Один", "Мама мыла раму.", "Удалён"]);
        let new = doc(&["Один", "Мама вымыла раму.", "Новый"]);
        let result = compare_documents(&old, &new, "Сравнение", None);
        let m = marks(&result);
        assert_eq!(m[0], [("Один".to_string(), None)]);
        assert_eq!(
            m[1],
            [
                ("Мама ".to_string(), None),
                ("мыла".to_string(), Some(RevisionKind::Delete)),
                ("вымыла".to_string(), Some(RevisionKind::Insert)),
                (" раму.".to_string(), None),
            ]
        );
        assert_eq!(
            m[2],
            [
                ("Удалён".to_string(), Some(RevisionKind::Delete)),
                ("Новый".to_string(), Some(RevisionKind::Insert)),
            ]
        );
        // Принять всё — новый документ, отклонить всё — старый.
        let mut accepted = result.clone();
        resolve_revisions(&mut accepted, None, true);
        assert_eq!(accepted.plain_text(), new.plain_text());
        let mut rejected = result;
        resolve_revisions(&mut rejected, None, false);
        assert_eq!(rejected.plain_text(), old.plain_text());
    }

    #[test]
    fn whole_paragraphs_added_and_removed() {
        let old = doc(&["a", "b", "c"]);
        let new = doc(&["a", "c", "d", "e"]);
        let result = compare_documents(&old, &new, "X", Some("2024-01-01T00:00:00Z".into()));
        assert_eq!(result.blocks().count(), 5);
        let mut accepted = result.clone();
        resolve_revisions(&mut accepted, None, true);
        assert_eq!(accepted.plain_text(), "a\nc\nd\ne");
        let mut rejected = result;
        resolve_revisions(&mut rejected, None, false);
        assert_eq!(rejected.plain_text(), "a\nb\nc");
        // Одинаковые документы — без исправлений.
        let same = compare_documents(&old, &old, "X", None);
        assert!(!crate::revisions::has_revisions(&same, None));
    }
}
