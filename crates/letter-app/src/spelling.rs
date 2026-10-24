//! Проверка орфографии в программе: словари грузятся в фоне при запуске,
//! слова с ошибкой подчёркиваются красной волной, по правой кнопке —
//! замены, «Пропустить все» и «Добавить в словарь» (слова пользователя
//! хранятся в папке программы в `dictionary.txt`).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke};
use letter_editor::Position;
use letter_editor::spelling::{Speller, dictionary_dirs};
use letter_model::Document;

/// Файл слов пользователя в папке программы.
const USER_WORDS: &str = "dictionary.txt";

/// Цвет волны под словом с ошибкой.
pub const ERROR_COLOR: Color32 = Color32::from_rgb(0xE0, 0x20, 0x20);

/// Состояние проверки орфографии.
#[derive(Debug)]
pub struct Spelling {
    /// Словари (загружаются в фоне; пока не загружены — ошибок нет).
    speller: Arc<OnceLock<Speller>>,
    /// Проверять и подчёркивать.
    pub enabled: bool,
    /// Слова пользователя и «Пропустить все».
    ignored: HashSet<String>,
    /// Папка программы (для слов пользователя).
    dir: Option<PathBuf>,
    /// Ошибки по тексту абзаца (абзацы без исправлений): после правки
    /// заново проверяются только изменённые абзацы.
    memo: HashMap<String, Vec<(usize, usize)>>,
    /// Ошибки документа для состояния истории.
    cached: Option<(u64, Vec<(Position, Position)>)>,
    /// Замены последнего слова (меню по правой кнопке рисуется каждый
    /// кадр, а искать замены долго).
    suggested: Option<(String, Vec<String>)>,
}

impl Spelling {
    /// Начинает грузить словари в фоне: из папок системы и из папки
    /// `dictionaries` внутри папки программы `dir`.
    pub fn start(dir: Option<PathBuf>) -> Spelling {
        let speller = Arc::new(OnceLock::new());
        let mut dirs = Vec::new();
        if let Some(dir) = &dir {
            dirs.push(dir.join("dictionaries"));
        }
        dirs.extend(dictionary_dirs());
        let target = Arc::clone(&speller);
        let spawned = std::thread::Builder::new()
            .name("spelling".into())
            .spawn(move || {
                let _ = target.set(Speller::load(&dirs));
            });
        if spawned.is_err() {
            let _ = speller.set(Speller::empty());
        }
        let ignored = dir
            .as_deref()
            .map(|d| read_words(&d.join(USER_WORDS)))
            .unwrap_or_default();
        Spelling {
            speller,
            enabled: true,
            ignored,
            dir,
            memo: HashMap::new(),
            cached: None,
            suggested: None,
        }
    }

    /// Замены для слова (не больше пяти).
    pub fn suggestions(&mut self, word: &str) -> Vec<String> {
        if let Some((w, list)) = &self.suggested
            && w == word
        {
            return list.clone();
        }
        let list = self
            .speller
            .get()
            .map(|s| s.suggest(word, 5))
            .unwrap_or_default();
        self.suggested = Some((word.to_string(), list.clone()));
        list
    }

    /// Проверка с готовыми словарями (для тестов).
    #[cfg(test)]
    pub fn with_speller(speller: Speller) -> Spelling {
        let lock = OnceLock::new();
        let _ = lock.set(speller);
        Spelling {
            speller: Arc::new(lock),
            enabled: true,
            ignored: HashSet::new(),
            dir: None,
            memo: HashMap::new(),
            cached: None,
            suggested: None,
        }
    }

    fn is_ignored(&self, word: &str) -> bool {
        self.ignored.contains(word)
    }

    /// Слова с ошибкой в документе для состояния истории `state`.
    pub fn errors(&mut self, doc: &Document, state: u64) -> Vec<(Position, Position)> {
        if !self.enabled {
            return Vec::new();
        }
        let Some(speller) = self.speller.get() else {
            return Vec::new();
        };
        if let Some((cached, errors)) = &self.cached
            && *cached == state
        {
            return errors.clone();
        }
        let mut errors = Vec::new();
        let count = letter_editor::paragraph_count(doc);
        let mut memo = std::mem::take(&mut self.memo);
        let mut used = HashMap::new();
        for index in 0..count {
            let Some(paragraph) = letter_editor::paragraph_ref(doc, index) else {
                continue;
            };
            let revised = paragraph.runs().any(|r| r.props.revision.is_some());
            let text = paragraph.plain_text();
            let found = match memo.remove(&text) {
                Some(found) if !revised => found,
                _ => speller.misspelled_in(doc, index),
            };
            for &(a, b) in &found {
                if !text.get(a..b).is_some_and(|w| self.is_ignored(w)) {
                    errors.push((Position::new(index, a), Position::new(index, b)));
                }
            }
            if !revised {
                used.insert(text, found);
            }
        }
        self.memo = used;
        self.cached = Some((state, errors.clone()));
        errors
    }

    /// «Пропустить все»: слово больше не подчёркивается (до выхода).
    pub fn ignore(&mut self, word: &str) {
        self.ignored.insert(word.to_string());
        self.cached = None;
    }

    /// «Добавить в словарь»: слово запоминается в словаре пользователя.
    pub fn add_word(&mut self, word: &str) {
        self.ignore(word);
        if let Some(dir) = &self.dir {
            let mut words: Vec<&String> = self.ignored.iter().collect();
            words.sort();
            let text: String = words.iter().map(|w| format!("{w}\n")).collect();
            let _ = crate::files::write_atomic(&dir.join(USER_WORDS), text.as_bytes());
        }
    }

    /// Включает или выключает проверку.
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        self.cached = None;
    }
}

/// Слова из файла, по одному в строке.
fn read_words(path: &Path) -> HashSet<String> {
    std::fs::read_to_string(path)
        .map(|t| {
            t.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Рисует красную волну под прямоугольником слова.
pub fn paint_wave(painter: &Painter, rect: Rect, scale: f32) {
    let step = (2.0 * scale).clamp(1.5, 4.0);
    let y = rect.bottom() - step * 0.5;
    let mut points = Vec::new();
    let mut x = rect.left();
    let mut up = false;
    while x <= rect.right() {
        points.push(Pos2::new(
            x,
            if up { y - step * 0.5 } else { y + step * 0.5 },
        ));
        up = !up;
        x += step;
    }
    if points.len() > 1 {
        painter.add(Shape::line(points, Stroke::new(1.0, ERROR_COLOR)));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use letter_editor::spelling::Script;
    use letter_model::Paragraph;
    use spellbook::Dictionary;

    pub(crate) fn speller() -> Speller {
        let mut s = Speller::empty();
        if let Ok(d) = Dictionary::new(
            "SET UTF-8\nTRY abcdefghijklmnopqrstuvwxyz\n",
            "2\nhello\nworld\n",
        ) {
            s.add_dictionary(Script::Latin, d);
        }
        s
    }

    #[test]
    fn errors_are_cached_and_ignored() {
        let mut spelling = Spelling::with_speller(speller());
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![Paragraph::text("hello wrld").into()];
        let errors = spelling.errors(&doc, 1);
        assert_eq!(errors, [(Position::new(0, 6), Position::new(0, 10))]);
        assert_eq!(spelling.errors(&doc, 1), errors);
        spelling.ignore("wrld");
        assert!(spelling.errors(&doc, 1).is_empty());
        spelling.set_enabled(false);
        doc.sections[0].blocks = vec![Paragraph::text("helo").into()];
        assert!(spelling.errors(&doc, 2).is_empty());
        spelling.set_enabled(true);
        assert_eq!(spelling.errors(&doc, 2).len(), 1);
    }

    #[test]
    fn user_words_are_saved() {
        let dir = std::env::temp_dir().join(format!("letter-words-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap_or_default();
        let mut spelling = Spelling::start(Some(dir.clone()));
        spelling.add_word("летер");
        let again = Spelling::start(Some(dir.clone()));
        assert!(again.is_ignored("летер"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
