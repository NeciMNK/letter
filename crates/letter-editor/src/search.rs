//! Поиск и замена текста (окно «Найти и заменить», Ctrl+F и Ctrl+H).
//! Ищется текст внутри абзацев: найденное не переходит через границу
//! абзаца. Табуляция в тексте абзаца — `\t`, разрыв строки — `\n`.

use letter_model::Document;

use crate::edit::{clamp_offset, delete_range, insert_text, paragraph_ref, plain_text};
use crate::position::Position;

/// Параметры поиска.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchOptions {
    /// «Учитывать регистр».
    pub match_case: bool,
    /// «Только слово целиком».
    pub whole_word: bool,
}

/// Найденный кусок: начало и конец.
pub type Found = (Position, Position);

/// Все вхождения `query` в тексте одного абзаца: начала и концы, байты.
fn matches_in(text: &str, query: &str, options: SearchOptions) -> Vec<(usize, usize)> {
    let fold = |c: char| {
        if options.match_case {
            c
        } else {
            c.to_lowercase().next().unwrap_or(c)
        }
    };
    let pattern: Vec<char> = query.chars().map(fold).collect();
    if pattern.is_empty() {
        return Vec::new();
    }
    let is_word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut found = Vec::new();
    let mut next_free = 0;
    for (start, _) in text.char_indices() {
        if start < next_free {
            continue;
        }
        let mut chars = text[start..].char_indices();
        let mut end = start;
        let mut matched = true;
        for &p in &pattern {
            match chars.next() {
                Some((i, c)) if fold(c) == p => end = start + i + c.len_utf8(),
                _ => {
                    matched = false;
                    break;
                }
            }
        }
        if !matched {
            continue;
        }
        if options.whole_word
            && (is_word(text[..start].chars().next_back()) || is_word(text[end..].chars().next()))
        {
            continue;
        }
        found.push((start, end));
        next_free = end;
    }
    found
}

/// Все вхождения `query` в документе по порядку.
pub fn find_all(doc: &Document, query: &str, options: SearchOptions) -> Vec<Found> {
    let mut found = Vec::new();
    let mut index = 0;
    while let Some(paragraph) = paragraph_ref(doc, index) {
        let text = plain_text(paragraph);
        found.extend(
            matches_in(&text, query, options)
                .into_iter()
                .map(|(a, b)| (Position::new(index, a), Position::new(index, b))),
        );
        index += 1;
    }
    found
}

/// Найденное с окружающим текстом для списка результатов («Область
/// навигации → Результаты»): до `around` знаков до и после, по границам
/// слов; табуляции и разрывы строк — пробелами.
pub fn found_context(doc: &Document, found: Found, around: usize) -> (String, String, String) {
    let Some(paragraph) = paragraph_ref(doc, found.0.paragraph) else {
        return Default::default();
    };
    let text = plain_text(paragraph);
    let clean = |s: &str| -> String {
        s.chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .filter(|&c| c != letter_model::OBJECT_CHAR)
            .collect()
    };
    let boundary = |offset: usize| {
        let mut offset = offset.min(text.len());
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    };
    let start = boundary(found.0.offset);
    let end = boundary(found.1.offset.max(start));
    let before: Vec<char> = text[..start].chars().collect();
    let mut from = before.len().saturating_sub(around);
    if from > 0 {
        // С начала слова.
        while from < before.len() && !before[from - 1].is_whitespace() {
            from += 1;
        }
    }
    let before: String = before[from..].iter().collect();
    let after: Vec<char> = text[end..].chars().collect();
    let mut to = after.len().min(around);
    if to < after.len() {
        while to > 0 && !after[to].is_whitespace() {
            to -= 1;
        }
    }
    let after: String = after[..to].iter().collect();
    let ellipsis = |cut: bool| if cut { "…" } else { "" };
    (
        format!("{}{}", ellipsis(from > 0), clean(&before)),
        clean(&text[start..end]),
        format!(
            "{}{}",
            clean(&after),
            ellipsis(to < text[end..].chars().count())
        ),
    )
}

/// Следующее вхождение `query`, которое начинается не раньше `from`;
/// дойдя до конца документа, поиск продолжается с начала (как «Найти
/// далее» в Word). `None`, если текста нет нигде.
pub fn find_next(
    doc: &Document,
    from: Position,
    query: &str,
    options: SearchOptions,
) -> Option<Found> {
    let all = find_all(doc, query, options);
    let key = |p: Position| (p.paragraph, p.offset);
    all.iter()
        .find(|(start, _)| key(*start) >= key(from))
        .or_else(|| all.first())
        .copied()
}

/// Заменяет все вхождения `query` на `replacement` и возвращает, сколько
/// замен сделано.
pub fn replace_all(
    doc: &mut Document,
    query: &str,
    replacement: &str,
    options: SearchOptions,
) -> usize {
    let found = find_all(doc, query, options);
    // С конца, чтобы не сдвигались ещё не заменённые места.
    for &(start, end) in found.iter().rev() {
        let at = delete_range(doc, start, end);
        insert_text(doc, at, replacement);
    }
    found.len()
}

/// Позиция `at`, поправленная после правки: внутри абзаца и на границе
/// буквы.
pub(crate) fn clamp(doc: &Document, at: Position) -> Position {
    match paragraph_ref(doc, at.paragraph) {
        Some(paragraph) => Position::new(at.paragraph, clamp_offset(paragraph, at.offset)),
        None => Position::new(0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::{Block, Paragraph};

    fn doc(texts: &[&str]) -> Document {
        let mut doc = Document::new();
        doc.sections[0].blocks = texts
            .iter()
            .map(|t| Block::from(Paragraph::text(*t)))
            .collect();
        doc
    }

    #[test]
    fn finds_with_case_and_whole_words() {
        let d = doc(&["Кот и кот", "котёнок, КОТ"]);
        let any = SearchOptions::default();
        let found = find_all(&d, "кот", any);
        assert_eq!(found.len(), 4);
        // «Кот» — 6 байт, «и» и пробелы — ещё 4.
        assert_eq!(found[1], (Position::new(0, 10), Position::new(0, 16)));
        let case = SearchOptions {
            match_case: true,
            ..any
        };
        assert_eq!(find_all(&d, "кот", case).len(), 2);
        let words = SearchOptions {
            whole_word: true,
            ..any
        };
        assert_eq!(find_all(&d, "кот", words).len(), 3);
        assert!(find_all(&d, "", any).is_empty());
        // Вхождения не перекрываются.
        assert_eq!(find_all(&doc(&["aaaa"]), "aa", any).len(), 2);
    }

    #[test]
    fn next_wraps_around() {
        let d = doc(&["one two", "two"]);
        let any = SearchOptions::default();
        let next = |p| find_next(&d, p, "two", any).map(|(s, _)| s);
        assert_eq!(next(Position::new(0, 0)), Some(Position::new(0, 4)));
        assert_eq!(next(Position::new(0, 5)), Some(Position::new(1, 0)));
        assert_eq!(next(Position::new(1, 1)), Some(Position::new(0, 4)));
        assert_eq!(find_next(&d, Position::new(0, 0), "три", any), None);
    }

    #[test]
    fn replaces_everything() {
        let mut d = doc(&["a-b-c", "b"]);
        let n = replace_all(&mut d, "B", "XY", SearchOptions::default());
        assert_eq!(n, 2);
        let texts: Vec<String> = (0..2)
            .map(|i| plain_text(paragraph_ref(&d, i).unwrap()))
            .collect();
        assert_eq!(texts, ["a-XY-c", "XY"]);
        assert_eq!(replace_all(&mut d, "XY", "", SearchOptions::default()), 2);
        assert_eq!(plain_text(paragraph_ref(&d, 0).unwrap()), "a--c");
        assert_eq!(clamp(&d, Position::new(1, 9)), Position::new(1, 0));
    }

    #[test]
    fn context_around_found_text() {
        let mut doc = Document::new();
        doc.sections[0].blocks =
            vec![Paragraph::text("Первое слово второе\tслово и ещё немного слов в конце").into()];
        let found = find_all(&doc, "второе", SearchOptions::default());
        let (before, text, after) = found_context(&doc, found[0], 10);
        assert_eq!(text, "второе");
        assert_eq!(before, "…слово ");
        assert_eq!(after, " слово и…");
        let (before, _, after) = found_context(&doc, found[0], 40);
        assert_eq!(before, "Первое слово ");
        assert_eq!(after, " слово и ещё немного слов в конце");
    }
}
