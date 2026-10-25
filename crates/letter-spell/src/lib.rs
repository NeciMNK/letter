//! Проверка орфографии по словарям Hunspell: русскому (`ru_RU`,
//! А. Лебедев) и английскому (`en_US`, SCOWL) из набора LibreOffice
//! (условия — в `dictionaries/README_*.txt`). Словари встроены в
//! программу, проверяет их библиотека `spellbook` (совместима с
//! Hunspell).
//!
//! Словарь для слова выбирается по буквам: кириллица — русский, латиница
//! — английский. Не проверяются слова с цифрами, слова из одних
//! прописных (сокращения), адреса сайтов и почты.
//!
//! - [`Speller::new`] — загрузить словари (около десятой доли секунды);
//! - [`Speller::misspelled`] — где в тексте ошибки;
//! - [`Speller::suggest`] — варианты исправления;
//! - [`Speller::ignore`] и [`Speller::add`] — «Пропустить все» и
//!   «Добавить в словарь».

use std::collections::HashSet;
use std::ops::Range;

use spellbook::Dictionary;

const RU_AFF: &str = include_str!("../dictionaries/ru_RU.aff");
const RU_DIC: &str = include_str!("../dictionaries/ru_RU.dic");
const EN_AFF: &str = include_str!("../dictionaries/en_US.aff");
const EN_DIC: &str = include_str!("../dictionaries/en_US.dic");

/// Язык слова (по его буквам).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    /// Русский: в слове есть кириллица.
    Russian,
    /// Английский: латиница.
    English,
}

/// Язык слова `word`; `None` — букв ни одного из языков нет.
pub fn language(word: &str) -> Option<Language> {
    let mut latin = false;
    for c in word.chars() {
        if matches!(c, '\u{400}'..='\u{4FF}') {
            return Some(Language::Russian);
        }
        latin |= c.is_ascii_alphabetic();
    }
    latin.then_some(Language::English)
}

/// Словари и списки слов пользователя.
pub struct Speller {
    russian: Dictionary,
    english: Dictionary,
    /// «Пропустить все» (до закрытия программы).
    ignored: HashSet<String>,
}

impl std::fmt::Debug for Speller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Speller")
            .field("ignored", &self.ignored)
            .finish_non_exhaustive()
    }
}

impl Speller {
    /// Загружает встроенные словари.
    pub fn new() -> Result<Speller, String> {
        let russian = Dictionary::new(RU_AFF, RU_DIC).map_err(|e| e.to_string())?;
        let english = Dictionary::new(EN_AFF, EN_DIC).map_err(|e| e.to_string())?;
        Ok(Speller {
            russian,
            english,
            ignored: HashSet::new(),
        })
    }

    fn dictionary(&self, language: Language) -> &Dictionary {
        match language {
            Language::Russian => &self.russian,
            Language::English => &self.english,
        }
    }

    /// Верно ли написано слово. Слова, которые не проверяются (см.
    /// описание модуля), считаются верными.
    pub fn check(&self, word: &str) -> bool {
        if !checkable(word) || self.ignored.contains(word) {
            return true;
        }
        let Some(language) = language(word) else {
            return true;
        };
        let word = normalize(word);
        self.dictionary(language).check(&word)
    }

    /// Ошибки в тексте: места неверно написанных слов (смещения в байтах).
    pub fn misspelled(&self, text: &str) -> Vec<Range<usize>> {
        words(text)
            .into_iter()
            .filter(|r| !self.check(&text[r.clone()]))
            .collect()
    }

    /// До `limit` вариантов исправления слова.
    pub fn suggest(&self, word: &str, limit: usize) -> Vec<String> {
        let Some(language) = language(word) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        self.dictionary(language)
            .suggest(&normalize(word), &mut out);
        out.truncate(limit);
        out
    }

    /// «Пропустить все»: слово больше не считается ошибкой.
    pub fn ignore(&mut self, word: &str) {
        self.ignored.insert(word.to_string());
    }

    /// «Добавить в словарь»: слово (и его формы с прописной буквы)
    /// считается верным. Сохранять список между запусками — дело
    /// программы.
    pub fn add(&mut self, word: &str) {
        let Some(language) = language(word) else {
            return;
        };
        let word = normalize(word);
        let dictionary = match language {
            Language::Russian => &mut self.russian,
            Language::English => &mut self.english,
        };
        // Слово без флагов не может быть ошибочным.
        let _ = dictionary.add(&word);
    }
}

/// Проверяется ли слово: без цифр и не сокращение из прописных.
fn checkable(word: &str) -> bool {
    if word.chars().any(|c| c.is_numeric()) {
        return false;
    }
    let letters = word.chars().filter(|c| c.is_alphabetic()).count();
    let upper = word.chars().filter(|c| c.is_uppercase()).count();
    !(letters > 1 && upper == letters)
}

/// Типографский апостроф — как в словаре.
fn normalize(word: &str) -> String {
    word.replace('\u{2019}', "'")
}

/// Слова текста (смещения в байтах): буквы, а внутри слова ещё
/// апостроф и дефис. Адреса сайтов и почты пропускаются.
pub fn words(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut chunk_start = 0;
    for (i, c) in text.char_indices().chain([(text.len(), ' ')]) {
        if !c.is_whitespace() {
            continue;
        }
        let chunk = &text[chunk_start..i];
        let address = chunk.contains('@') || chunk.contains("://") || chunk.starts_with("www.");
        if !address {
            chunk_words(chunk, chunk_start, &mut out);
        }
        chunk_start = i + c.len_utf8();
    }
    out
}

fn chunk_words(chunk: &str, base: usize, out: &mut Vec<Range<usize>>) {
    let chars: Vec<(usize, char)> = chunk.char_indices().collect();
    let mut start: Option<usize> = None;
    for (k, &(i, c)) in chars.iter().enumerate() {
        let joiner = matches!(c, '\'' | '\u{2019}' | '-')
            && start.is_some()
            && chars.get(k + 1).is_some_and(|(_, n)| n.is_alphabetic());
        if c.is_alphanumeric() || joiner || (start.is_some() && is_mark(c)) {
            start.get_or_insert(i);
            continue;
        }
        if let Some(s) = start.take() {
            out.push(base + s..base + i);
        }
    }
    if let Some(s) = start {
        out.push(base + s..base + chunk.len());
    }
}

/// Знак ударения и другие надстрочные знаки, идущие после буквы.
fn is_mark(c: char) -> bool {
    matches!(c, '\u{300}'..='\u{36F}')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speller() -> Speller {
        Speller::new().unwrap_or_else(|e| panic!("{e}"))
    }

    fn found<'a>(text: &'a str, ranges: &[Range<usize>]) -> Vec<&'a str> {
        ranges.iter().map(|r| &text[r.clone()]).collect()
    }

    #[test]
    fn words_are_split_like_in_word() {
        let text = "Кто-то сказал: «doesn't» — e-mail@mail.ru, https://x.org/a 2-й раз!";
        assert_eq!(
            found(text, &words(text)),
            ["Кто-то", "сказал", "doesn't", "2-й", "раз"]
        );
    }

    #[test]
    fn russian_and_english_words_are_checked() {
        let speller = speller();
        let text = "Превет, мир! Hello wrold. Ёлка и елка. Microsoft НАТО ООН 3D по-русски";
        assert_eq!(found(text, &speller.misspelled(text)), ["Превет", "wrold"]);
        assert!(speller.suggest("превет", 10).iter().any(|s| s == "привет"));
        assert!(speller.suggest("wrold", 10).iter().any(|s| s == "world"));
        assert!(speller.check("it\u{2019}s"));
        assert!(speller.suggest("123", 5).is_empty());
    }

    #[test]
    fn ignored_and_added_words() {
        let mut speller = speller();
        assert!(!speller.check("Летерр"));
        speller.ignore("Летерр");
        assert!(speller.check("Летерр"));
        assert!(!speller.check("гуглить"));
        speller.add("гуглить");
        assert!(speller.check("гуглить"));
        assert!(speller.check("Гуглить"));
    }
}
