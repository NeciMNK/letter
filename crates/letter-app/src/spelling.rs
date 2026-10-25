//! Проверка орфографии в окне: ошибки подчёркиваются красной волной,
//! по правой кнопке на слове — варианты, «Пропустить все» и «Добавить
//! в словарь», F7 — к следующей ошибке.
//!
//! Словари загружаются в отдельном потоке при первом показе документа
//! (около секунды), до того ошибки не показываются. Слова, добавленные
//! в словарь, хранятся в `dictionary.txt` в папке программы, по слову на
//! строку.

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use eframe::egui::{Color32, Painter, Rect, Stroke, pos2};
use letter_spell::Speller;

/// Цвет волнистой черты под ошибкой.
pub const ERROR_COLOR: Color32 = Color32::from_rgb(220, 40, 40);

/// Сколько разных абзацев помнить, прежде чем начать заново.
const CACHE_LIMIT: usize = 20_000;

/// Сколько вариантов показывать в меню.
pub const SUGGESTIONS: usize = 5;

/// Состояние проверки орфографии.
#[derive(Debug)]
pub struct Spelling {
    /// Словари; `None`, пока загружаются.
    speller: Arc<Mutex<Option<Speller>>>,
    /// Загрузка начата.
    started: bool,
    /// Начинать ли загрузку самой (в тестах словари грузятся явно).
    autoload: bool,
    /// Файл слов пользователя.
    personal: Option<PathBuf>,
    /// Ошибки по тексту абзаца.
    cache: HashMap<Arc<str>, Arc<[Range<usize>]>>,
    /// Показывать ошибки.
    pub show: bool,
}

/// Действие проверки орфографии (лента, F7, меню по правой кнопке).
#[derive(Debug, Clone, PartialEq)]
pub enum SpellAction {
    /// Выделить следующую ошибку (F7).
    Next,
    /// Показать или скрыть подчёркивание ошибок.
    ToggleShow,
    /// Заменить слово с ошибкой вариантом.
    Replace {
        /// Номер абзаца.
        paragraph: usize,
        /// Место слова.
        range: Range<usize>,
        /// Чем заменить.
        with: String,
    },
    /// «Пропустить все».
    Ignore(String),
    /// «Добавить в словарь».
    Add(String),
}

/// Слово с ошибкой у курсора и варианты для него (меню по правой
/// кнопке).
#[derive(Debug, Clone, PartialEq)]
pub struct Misspelling {
    /// Номер абзаца.
    pub paragraph: usize,
    /// Место слова в абзаце.
    pub range: Range<usize>,
    /// Само слово.
    pub word: String,
    /// Варианты исправления.
    pub suggestions: Vec<String>,
}

impl Spelling {
    /// Проверка со словами пользователя из файла `personal`. Если
    /// `autoload`, словари начнут загружаться при первом показе.
    pub fn new(personal: Option<PathBuf>, autoload: bool) -> Spelling {
        Spelling {
            speller: Arc::new(Mutex::new(None)),
            started: false,
            autoload,
            personal,
            cache: HashMap::new(),
            show: true,
        }
    }

    /// Загружает словари в отдельном потоке (один раз).
    pub fn start(&mut self) {
        if std::mem::replace(&mut self.started, true) {
            return;
        }
        let slot = Arc::clone(&self.speller);
        let personal = self.personal.clone();
        std::thread::spawn(move || {
            if let Some(speller) = load(personal) {
                *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(speller);
            }
        });
    }

    /// Загружает словари сразу (для тестов).
    #[cfg(test)]
    pub fn load_now(&mut self) {
        self.started = true;
        *self.lock() = load(self.personal.clone());
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Speller>> {
        self.speller.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Словари загружены.
    pub fn ready(&self) -> bool {
        self.lock().is_some()
    }

    /// Ошибки в тексте абзаца (пусто, пока словари не загружены или
    /// показ выключен).
    pub fn errors(&mut self, text: &Arc<str>) -> Arc<[Range<usize>]> {
        if !self.show {
            return Arc::from([]);
        }
        if self.autoload {
            self.start();
        }
        if let Some(found) = self.cache.get(text) {
            return Arc::clone(found);
        }
        let guard = self.speller.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(speller) = guard.as_ref() else {
            return Arc::from([]);
        };
        let found: Arc<[Range<usize>]> = speller.misspelled(text).into();
        drop(guard);
        if self.cache.len() >= CACHE_LIMIT {
            self.cache.clear();
        }
        self.cache.insert(Arc::clone(text), Arc::clone(&found));
        found
    }

    /// Слово с ошибкой в абзаце `paragraph` (текст `text`) у места
    /// `offset`, с вариантами исправления.
    pub fn misspelling_at(
        &mut self,
        paragraph: usize,
        text: &Arc<str>,
        offset: usize,
    ) -> Option<Misspelling> {
        let range = self
            .errors(text)
            .iter()
            .find(|r| r.start <= offset && offset <= r.end)?
            .clone();
        let word = text.get(range.clone())?.to_string();
        let suggestions = self.lock().as_ref()?.suggest(&word, SUGGESTIONS);
        Some(Misspelling {
            paragraph,
            range,
            word,
            suggestions,
        })
    }

    /// «Пропустить все».
    pub fn ignore(&mut self, word: &str) {
        if let Some(speller) = self.lock().as_mut() {
            speller.ignore(word);
        }
        self.cache.clear();
    }

    /// «Добавить в словарь»: слово запоминается и в файле пользователя.
    pub fn add(&mut self, word: &str) {
        if let Some(speller) = self.lock().as_mut() {
            speller.add(word);
        }
        self.cache.clear();
        if let Some(file) = &self.personal {
            use std::io::Write;
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            // Не вышло записать — слово всё равно верно до закрытия.
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(file)
            {
                let _ = writeln!(f, "{word}");
            }
        }
    }
}

/// Загружает словари и слова пользователя.
fn load(personal: Option<PathBuf>) -> Option<Speller> {
    let mut speller = Speller::new().ok()?;
    if let Some(text) = personal.and_then(|f| std::fs::read_to_string(f).ok()) {
        for word in text.lines().map(str::trim).filter(|w| !w.is_empty()) {
            speller.add(word);
        }
    }
    Some(speller)
}

/// Волнистая черта под ошибкой: по нижнему краю `rect`.
pub fn paint_wave(painter: &Painter, rect: Rect, scale: f32) {
    let step = (2.0 * scale).max(1.5);
    let y = rect.bottom() - step;
    let mut points = Vec::new();
    let mut x = rect.left();
    let mut up = true;
    while x <= rect.right() {
        points.push(pos2(x, if up { y } else { y + step }));
        up = !up;
        x += step;
    }
    if points.len() >= 2 {
        painter.add(eframe::egui::Shape::line(
            points,
            Stroke::new((0.8 * scale).max(1.0), ERROR_COLOR),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_cached_and_words_added() {
        let dir = crate::files::tests::temp_dir("spelling");
        let file = dir.join("dictionary.txt");
        let mut spelling = Spelling::new(Some(file.clone()), false);
        let text: Arc<str> = Arc::from("Превет, мир");
        // До загрузки ошибок нет.
        assert!(spelling.errors(&text).is_empty());
        spelling.load_now();
        assert_eq!(spelling.errors(&text).to_vec(), vec![0.."Превет".len()]);
        let found = spelling.misspelling_at(3, &text, 2).unwrap();
        assert_eq!(found.word, "Превет");
        assert!(found.suggestions.iter().any(|s| s == "Привет"));
        assert!(spelling.misspelling_at(3, &text, text.len()).is_none());

        spelling.add("Превет");
        assert!(spelling.errors(&text).is_empty());
        // Слово сохранилось и есть после новой загрузки.
        let mut again = Spelling::new(Some(file), false);
        again.load_now();
        assert!(again.errors(&text).is_empty());
        again.show = false;
        assert!(again.errors(&Arc::from("Ашипка")).is_empty());
    }
}
