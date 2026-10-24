//! Проверка орфографии по словарям Hunspell (`.aff` и `.dic`, как
//! в LibreOffice и Firefox): какие слова документа с ошибкой и какие
//! есть замены.
//!
//! Словари ищутся в папках системы (см. [`dictionary_dirs`]) и в папке,
//! которую передаст программа. Язык слова выбирается по буквам: кириллица
//! — русский словарь, латиница — английский. Слова, для письменности
//! которых словаря нет, не проверяются.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use letter_model::{Document, RevisionKind};
use spellbook::Dictionary;
use unicode_segmentation::UnicodeSegmentation;

use crate::Position;
use crate::edit::{paragraph_count, paragraph_ref};

/// Письменность слова: по ней выбирается словарь.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Script {
    /// Кириллица (русский).
    Cyrillic,
    /// Латиница (английский).
    Latin,
}

impl Script {
    /// Письменность по первой букве слова. Без букв или с буквами других
    /// языков (кроме русских и латинских) — `None`.
    pub fn of(word: &str) -> Option<Script> {
        let script = |c: char| match c {
            'а'..='я' | 'А'..='Я' | 'ё' | 'Ё' => Some(Script::Cyrillic),
            c if c.is_ascii_alphabetic() => Some(Script::Latin),
            _ => None,
        };
        let mut letters = word.chars().filter(|c| c.is_alphabetic()).peekable();
        let first = script(*letters.peek()?)?;
        letters.all(|c| script(c).is_some()).then_some(first)
    }

    /// Начало имени словаря этой письменности (`ru_RU.dic` и т. п.).
    fn prefix(self) -> &'static str {
        match self {
            Script::Cyrillic => "ru",
            Script::Latin => "en",
        }
    }
}

/// Словарь Hunspell с его языком.
pub struct Speller {
    dictionaries: Vec<(Script, Dictionary)>,
    /// «Пропустить все» и слова пользователя: не считаются ошибками.
    ignored: HashSet<String>,
}

impl std::fmt::Debug for Speller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Speller")
            .field(
                "dictionaries",
                &self.dictionaries.iter().map(|d| d.0).collect::<Vec<_>>(),
            )
            .field("ignored", &self.ignored.len())
            .finish()
    }
}

/// Папки, где обычно лежат словари Hunspell: системные (Linux),
/// LibreOffice и OpenOffice (все ОС).
pub fn dictionary_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = [
        "/usr/share/hunspell",
        "/usr/share/myspell",
        "/usr/share/myspell/dicts",
        "/usr/local/share/hunspell",
        "/Library/Spelling",
        "/usr/lib/libreoffice/share/extensions",
        "/opt/libreoffice/share/extensions",
        "/Applications/LibreOffice.app/Contents/Resources/extensions",
        "C:\\Program Files\\LibreOffice\\share\\extensions",
        "C:\\Program Files (x86)\\LibreOffice\\share\\extensions",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(Path::new(&home).join("Library/Spelling"));
        dirs.push(Path::new(&home).join(".local/share/hunspell"));
    }
    dirs
}

/// Текст файла словаря в кодировке из строки `SET` файла `.aff`
/// (UTF-8, KOI8-R, Windows-1251 и т. п.).
fn decode(bytes: &[u8], label: &str) -> String {
    let label = label.trim();
    match encoding_rs::Encoding::for_label(label.as_bytes()) {
        Some(encoding) if !label.eq_ignore_ascii_case("UTF-8") => {
            encoding.decode(bytes).0.into_owned()
        }
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// Читает словарь `name.aff` + `name.dic`. Битый словарь — `None`.
pub fn load_dictionary(aff: &Path, dic: &Path) -> Option<Dictionary> {
    let aff_bytes = std::fs::read(aff).ok()?;
    let dic_bytes = std::fs::read(dic).ok()?;
    let label = String::from_utf8_lossy(&aff_bytes)
        .lines()
        .find_map(|l| l.trim().strip_prefix("SET ").map(str::to_string))
        .unwrap_or_else(|| "UTF-8".into());
    let aff_text = decode(&aff_bytes, &label);
    let dic_text = decode(&dic_bytes, &label);
    // Строку `SET` библиотека не понимает иначе как UTF-8: текст уже
    // перекодирован.
    let aff_text: String = aff_text
        .lines()
        .map(|l| {
            if l.trim_start().starts_with("SET ") {
                "SET UTF-8"
            } else {
                l
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    Dictionary::new(&aff_text, &dic_text).ok()
}

/// Ищет в папке (и на один-два уровня глубже: так лежат расширения
/// LibreOffice) словарь для письменности: файл `<язык>…​.dic` с парой
/// `.aff`. Первый найденный.
fn find_dictionary(dir: &Path, script: Script, depth: usize) -> Option<(PathBuf, PathBuf)> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for path in &entries {
        let name = path.file_name()?.to_string_lossy().to_lowercase();
        if name.starts_with(script.prefix()) && name.ends_with(".dic") {
            let aff = path.with_extension("aff");
            if aff.is_file() {
                return Some((aff, path.clone()));
            }
        }
    }
    if depth == 0 {
        return None;
    }
    entries
        .iter()
        .filter(|p| p.is_dir())
        .find_map(|p| find_dictionary(p, script, depth - 1))
}

impl Speller {
    /// Проверка без словарей: ошибок нет.
    pub fn empty() -> Speller {
        Speller {
            dictionaries: Vec::new(),
            ignored: HashSet::new(),
        }
    }

    /// Ищет русский и английский словари в папках `dirs` по порядку.
    pub fn load(dirs: &[PathBuf]) -> Speller {
        let mut speller = Speller::empty();
        for script in [Script::Cyrillic, Script::Latin] {
            let found = dirs
                .iter()
                .filter_map(|dir| find_dictionary(dir, script, 2))
                .find_map(|(aff, dic)| load_dictionary(&aff, &dic));
            if let Some(dictionary) = found {
                speller.dictionaries.push((script, dictionary));
            }
        }
        speller
    }

    /// Добавляет готовый словарь для письменности `script`.
    pub fn add_dictionary(&mut self, script: Script, dictionary: Dictionary) {
        self.dictionaries.retain(|(s, _)| *s != script);
        self.dictionaries.push((script, dictionary));
    }

    /// Есть ли хоть один словарь.
    pub fn has_dictionaries(&self) -> bool {
        !self.dictionaries.is_empty()
    }

    /// «Пропустить все» или «Добавить в словарь»: слово больше не ошибка.
    pub fn ignore(&mut self, word: &str) {
        self.ignored.insert(word.to_string());
    }

    fn dictionary(&self, word: &str) -> Option<&Dictionary> {
        let script = Script::of(word)?;
        self.dictionaries
            .iter()
            .find(|(s, _)| *s == script)
            .map(|(_, d)| d)
    }

    /// Слово с ошибкой? Без словаря, с цифрами, из одних прописных
    /// (сокращения) и пропущенные — нет.
    pub fn is_misspelled(&self, word: &str) -> bool {
        if word.chars().any(|c| c.is_numeric())
            || word.chars().filter(|c| c.is_alphabetic()).count() < 2
            || word.chars().all(|c| !c.is_lowercase())
            || self.ignored.contains(word)
        {
            return false;
        }
        // Буквы разных письменностей подряд («Wordе») — ошибка всегда.
        let mixed = word
            .chars()
            .any(|c| Script::of(&c.to_string()) == Some(Script::Latin))
            && word
                .chars()
                .any(|c| Script::of(&c.to_string()) == Some(Script::Cyrillic));
        match self.dictionary(word) {
            Some(_) if mixed => true,
            // Буква «ё» в словарях часто записана как «е».
            Some(d) => !d.check(word) && !d.check(&word.replace('ё', "е").replace('Ё', "Е")),
            None => false,
        }
    }

    /// Замены для слова с ошибкой, лучшие первыми (не больше `limit`).
    pub fn suggest(&self, word: &str, limit: usize) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(d) = self.dictionary(word) {
            d.suggest(word, &mut out);
        }
        out.truncate(limit);
        out
    }

    /// Слова с ошибкой в абзацах документа: начало и конец каждого.
    /// Удалённый исправлением текст не проверяется.
    pub fn misspelled(&self, doc: &Document) -> Vec<(Position, Position)> {
        let mut out = Vec::new();
        if !self.has_dictionaries() {
            return out;
        }
        for index in 0..paragraph_count(doc) {
            out.extend(
                self.misspelled_in(doc, index)
                    .into_iter()
                    .map(|(a, b)| (Position::new(index, a), Position::new(index, b))),
            );
        }
        out
    }

    /// Слова с ошибкой в абзаце `index`: места в его тексте.
    pub fn misspelled_in(&self, doc: &Document, index: usize) -> Vec<(usize, usize)> {
        let Some(paragraph) = paragraph_ref(doc, index) else {
            return Vec::new();
        };
        // Куски удалённого текста.
        let mut deleted = Vec::new();
        let mut pos = 0;
        for run in paragraph.runs() {
            let len: usize = run
                .content
                .iter()
                .map(letter_model::RunContent::text_len)
                .sum();
            if run
                .props
                .revision
                .as_ref()
                .is_some_and(|r| r.kind == RevisionKind::Delete)
            {
                deleted.push((pos, pos + len));
            }
            pos += len;
        }
        let text = paragraph.plain_text();
        text.split_word_bound_indices()
            .filter(|(_, w)| w.chars().any(char::is_alphabetic))
            .filter(|&(i, w)| !deleted.iter().any(|&(a, b)| i < b && i + w.len() > a))
            .filter(|(_, w)| self.is_misspelled(w))
            .map(|(i, w)| (i, i + w.len()))
            .collect()
    }
}

/// Слово с ошибкой у места `at`: его начало, конец и текст.
pub fn misspelled_at(
    speller: &Speller,
    doc: &Document,
    at: Position,
) -> Option<(Position, Position, String)> {
    let text = paragraph_ref(doc, at.paragraph)?.plain_text();
    speller
        .misspelled_in(doc, at.paragraph)
        .into_iter()
        .find(|&(a, b)| a <= at.offset && at.offset <= b)
        .map(|(a, b)| {
            (
                Position::new(at.paragraph, a),
                Position::new(at.paragraph, b),
                text[a..b].to_string(),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::Paragraph;

    /// Маленький словарь для проверки (свой, не из чужих словарей).
    fn speller() -> Speller {
        let aff = "SET UTF-8\nTRY абвгдеёжзийклмнопрстуфхцчшщъыьэюяabcdefghijklmnopqrstuvwxyz\nSFX A Y 1\nSFX A 0 ы .\n";
        let ru = Dictionary::new(aff, "4\nмама\nмыла\nраму\nкот/A\n");
        let en = Dictionary::new(
            "SET UTF-8\nTRY abcdefghijklmnopqrstuvwxyz\n",
            "2\nhello\nworld\n",
        );
        let mut s = Speller::empty();
        if let (Ok(ru), Ok(en)) = (ru, en) {
            s.add_dictionary(Script::Cyrillic, ru);
            s.add_dictionary(Script::Latin, en);
        }
        s
    }

    #[test]
    fn words_are_checked_by_script() {
        let s = speller();
        assert!(s.has_dictionaries());
        assert!(!s.is_misspelled("мама"));
        assert!(!s.is_misspelled("коты"));
        assert!(s.is_misspelled("мыло"));
        assert!(!s.is_misspelled("hello"));
        assert!(s.is_misspelled("helo"));
        // Цифры, сокращения, смесь письменностей, чужие буквы.
        assert!(!s.is_misspelled("ГОСТ"));
        assert!(!s.is_misspelled("x86"));
        assert!(s.is_misspelled("мaма"));
        assert!(!s.is_misspelled("Straße"));
        assert!(s.suggest("helo", 3).contains(&"hello".to_string()));
        let mut s = s;
        s.ignore("helo");
        assert!(!s.is_misspelled("helo"));
        assert!(!Speller::empty().is_misspelled("helo"));
    }

    #[test]
    fn misspelled_words_in_the_document() {
        let s = speller();
        let mut d = Document::new();
        d.sections[0].blocks = vec![
            Paragraph::text("Мама мыло раму.").into(),
            Paragraph::text("hello wrld").into(),
        ];
        assert_eq!(
            s.misspelled(&d),
            [
                (Position::new(0, 9), Position::new(0, 17)),
                (Position::new(1, 6), Position::new(1, 10)),
            ]
        );
        let found = misspelled_at(&s, &d, Position::new(1, 7));
        assert_eq!(found.map(|f| f.2), Some("wrld".to_string()));
        assert_eq!(misspelled_at(&s, &d, Position::new(1, 2)), None);
    }

    #[test]
    fn dictionaries_are_found_and_decoded() {
        let dir = std::env::temp_dir().join(format!("letter-spell-{}", std::process::id()));
        let nested = dir.join("dict-ru");
        std::fs::create_dir_all(&nested).unwrap_or_default();
        // KOI8-R: «кот» = CB CF D4.
        let _ = std::fs::write(nested.join("ru_RU.aff"), b"SET KOI8-R\n");
        let _ = std::fs::write(nested.join("ru_RU.dic"), b"1\n\xcb\xcf\xd4\n");
        let s = Speller::load(std::slice::from_ref(&dir));
        assert!(s.has_dictionaries());
        assert!(!s.is_misspelled("кот"));
        assert!(s.is_misspelled("кит"));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!Speller::load(&[]).has_dictionaries());
    }
}
