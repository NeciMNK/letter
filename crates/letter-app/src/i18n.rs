//! Перевод интерфейса: русский и английский. Тексты лежат в файлах
//! `assets/i18n/*.ftl` (формат Fluent), в коде — только их имена:
//! `tr("save")` → «Сохранить» или «Save».
//!
//! Язык хранится отдельно для каждого потока: окно программы работает в
//! одном потоке, а тесты, которые идут параллельно, не мешают друг другу.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

/// Язык интерфейса.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Language {
    /// Русский.
    #[default]
    Russian,
    /// Английский.
    English,
}

impl Language {
    /// Все языки по порядку списка.
    pub const ALL: [Language; 2] = [Language::Russian, Language::English];

    /// Название языка на нём самом — так его узнают при любом текущем
    /// языке.
    pub fn name(self) -> &'static str {
        match self {
            Language::Russian => "Русский",
            Language::English => "English",
        }
    }

    /// Значение в файле параметров.
    pub fn key(self) -> &'static str {
        match self {
            Language::Russian => "ru",
            Language::English => "en",
        }
    }

    /// Язык по значению из файла параметров.
    pub fn from_key(key: &str) -> Option<Language> {
        Language::ALL.into_iter().find(|l| l.key() == key)
    }

    /// Тексты на этом языке.
    fn source(self) -> &'static str {
        match self {
            Language::Russian => include_str!("../assets/i18n/ru.ftl"),
            Language::English => include_str!("../assets/i18n/en.ftl"),
        }
    }
}

thread_local! {
    static CURRENT: Cell<Language> = const { Cell::new(Language::Russian) };
}

/// Текущий язык интерфейса.
pub fn language() -> Language {
    CURRENT.with(Cell::get)
}

/// Меняет язык интерфейса.
pub fn set_language(language: Language) {
    CURRENT.with(|c| c.set(language));
}

type Bundle = FluentBundle<FluentResource>;

/// Набор текстов языка (собирается один раз).
fn bundle(language: Language) -> &'static Bundle {
    static BUNDLES: OnceLock<[Bundle; 2]> = OnceLock::new();
    let bundles = BUNDLES.get_or_init(|| Language::ALL.map(make_bundle));
    &bundles[language as usize]
}

fn make_bundle(language: Language) -> Bundle {
    let id: LanguageIdentifier = language.key().parse().unwrap_or_default();
    let mut bundle = FluentBundle::new_concurrent(vec![id]);
    // Без невидимых знаков направления текста вокруг подстановок.
    bundle.set_use_isolating(false);
    // Ошибки в файлах текстов ловит тест `files_parse`; здесь
    // берётся то, что удалось разобрать.
    let resource = FluentResource::try_new(language.source().to_owned()).unwrap_or_else(|(r, _)| r);
    let _ = bundle.add_resource(resource);
    bundle
}

/// Текст в одном языке или `None`, если его там нет.
fn format(language: Language, id: &str, args: Option<&FluentArgs>) -> Option<String> {
    let bundle = bundle(language);
    let pattern = bundle.get_message(id)?.value()?;
    let mut errors = Vec::new();
    Some(
        bundle
            .format_pattern(pattern, args, &mut errors)
            .into_owned(),
    )
}

/// Текст в текущем языке; если перевода нет — по-русски, если нет и его —
/// само имя.
fn lookup(id: &str, args: Option<&FluentArgs>) -> String {
    format(language(), id, args)
        .or_else(|| format(Language::Russian, id, args))
        .unwrap_or_else(|| id.to_owned())
}

/// Текст интерфейса по имени в текущем языке.
///
/// Тексты без подстановок запоминаются навсегда (их немного и они не
/// меняются), поэтому результат живёт всё время работы программы и
/// подходит туда, где нужен `&'static str`.
pub fn tr(id: &str) -> &'static str {
    static CACHE: OnceLock<Mutex<HashMap<(Language, String), &'static str>>> = OnceLock::new();
    let language = language();
    let cache = CACHE.get_or_init(Default::default);
    let mut cache = match cache.lock() {
        Ok(cache) => cache,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(text) = cache.get(&(language, id.to_owned())) {
        return text;
    }
    let text: &'static str = Box::leak(lookup(id, None).into_boxed_str());
    cache.insert((language, id.to_owned()), text);
    text
}

/// Текст с подстановками: `tr_args("status-page", &[("page", 1.into()),
/// ("pages", 3.into())])` → «Страница 1 из 3». Числа выбирают нужную
/// форму слова («1 word», «2 words»).
pub fn tr_args(id: &str, args: &[(&str, FluentValue<'_>)]) -> String {
    let mut fluent = FluentArgs::new();
    for (name, value) in args {
        fluent.set(*name, value.clone());
    }
    lookup(id, Some(&fluent))
}

/// Есть ли такой текст (в русском — основном — языке).
pub fn has(id: &str) -> bool {
    bundle(Language::Russian).has_message(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// Имена текстов в файле языка.
    fn ids(language: Language) -> BTreeSet<String> {
        language
            .source()
            .lines()
            .filter(|l| !l.starts_with([' ', '#']))
            .filter_map(|l| l.split_once('='))
            .map(|(id, _)| id.trim().to_owned())
            .collect()
    }

    #[test]
    fn files_parse() {
        for language in Language::ALL {
            if let Err((_, errors)) = FluentResource::try_new(language.source().to_owned()) {
                panic!("{language:?}: {errors:?}");
            }
        }
    }

    #[test]
    fn languages_have_same_texts() {
        let ru = ids(Language::Russian);
        let en = ids(Language::English);
        assert!(ru.len() > 300);
        let only_ru: Vec<_> = ru.difference(&en).collect();
        let only_en: Vec<_> = en.difference(&ru).collect();
        assert!(
            only_ru.is_empty() && only_en.is_empty(),
            "{only_ru:?} {only_en:?}"
        );
    }

    /// Каждое `tr("…")` и `tr_args("…"` в коде программы есть в файлах.
    #[test]
    fn used_texts_exist() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let mut missing = Vec::new();
        let mut used = 0;
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let Ok(code) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            // Тесты проверяют и несуществующие имена.
            let code = code.split("\n#[cfg(test)]").next().unwrap_or_default();
            for start in ["tr(\"", "tr_args(\""] {
                for (at, _) in code.match_indices(start) {
                    let rest = &code[at + start.len()..];
                    let id = &rest[..rest.find('"').unwrap_or(0)];
                    used += 1;
                    if !has(id) {
                        missing.push(id.to_owned());
                    }
                }
            }
        }
        assert!(used > 200, "{used}");
        assert!(missing.is_empty(), "{missing:?}");
    }

    #[test]
    fn switching_language() {
        assert_eq!(language(), Language::Russian);
        assert_eq!(tr("save"), "Сохранить");
        assert_eq!(
            tr_args("status-page", &[("page", 2.into()), ("pages", 5.into())]),
            "Страница 2 из 5"
        );
        set_language(Language::English);
        assert_eq!(tr("save"), "Save");
        assert_eq!(tr_args("status-words", &[("words", 1.into())]), "1 word");
        assert_eq!(tr_args("status-words", &[("words", 12.into())]), "12 words");
        assert_eq!(tr("no-such-text"), "no-such-text");
        set_language(Language::Russian);
        assert_eq!(
            tr_args("status-words", &[("words", 12.into())]),
            "Число слов: 12"
        );
        for language in Language::ALL {
            assert_eq!(Language::from_key(language.key()), Some(language));
        }
        assert_eq!(Language::from_key("xx"), None);
    }
}
