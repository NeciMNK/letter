//! Шрифты: поиск среди установленных в системе и встроенных в программу,
//! замена отсутствующих шрифтов похожими.
//!
//! Word хранит в документе только имя шрифта. Если такого шрифта на
//! компьютере нет, нужен шрифт с теми же размерами букв, иначе строки
//! и страницы разобьются по-другому. Для самых частых шрифтов Microsoft
//! есть открытые замены с такими же размерами (Calibri → Carlito и т. д.);
//! они встроены в программу.

use std::collections::HashMap;
use std::sync::Arc;

use fontdb::{Database, Family, Query, Source, Style, Weight};

/// Встроенный шрифт: имя файла и содержимое.
type EmbeddedFont = (&'static str, &'static [u8]);

macro_rules! embedded {
    ($($file:literal),* $(,)?) => {
        &[$(($file, include_bytes!(concat!("../fonts/", $file)))),*]
    };
}

/// Открытые шрифты, встроенные в программу (см. `fonts/README.md`).
const EMBEDDED_FONTS: &[EmbeddedFont] = embedded!(
    "Carlito-Regular.ttf",
    "Carlito-Bold.ttf",
    "Carlito-Italic.ttf",
    "Carlito-BoldItalic.ttf",
    "Caladea-Regular.ttf",
    "Caladea-Bold.ttf",
    "Caladea-Italic.ttf",
    "Caladea-BoldItalic.ttf",
    "LiberationSerif-Regular.ttf",
    "LiberationSerif-Bold.ttf",
    "LiberationSerif-Italic.ttf",
    "LiberationSerif-BoldItalic.ttf",
    "LiberationSans-Regular.ttf",
    "LiberationSans-Bold.ttf",
    "LiberationSans-Italic.ttf",
    "LiberationSans-BoldItalic.ttf",
    "LiberationMono-Regular.ttf",
    "LiberationMono-Bold.ttf",
    "LiberationMono-Italic.ttf",
    "LiberationMono-BoldItalic.ttf",
);

/// Замены шрифтов: если шрифта слева нет, пробуем шрифты справа по порядку.
/// Сначала — шрифты с такими же размерами букв.
const SUBSTITUTES: &[(&str, &[&str])] = &[
    ("Calibri", &["Carlito"]),
    ("Calibri Light", &["Carlito"]),
    ("Cambria", &["Caladea"]),
    ("Cambria Math", &["Caladea"]),
    ("Times New Roman", &["Liberation Serif"]),
    ("Times", &["Times New Roman", "Liberation Serif"]),
    ("Arial", &["Liberation Sans"]),
    ("Helvetica", &["Arial", "Liberation Sans"]),
    (
        "Arial Narrow",
        &["Liberation Sans Narrow", "Liberation Sans"],
    ),
    ("Courier New", &["Liberation Mono"]),
    ("Courier", &["Courier New", "Liberation Mono"]),
    ("Consolas", &["Liberation Mono"]),
    ("Segoe UI", &["Carlito", "Liberation Sans"]),
    ("Tahoma", &["DejaVu Sans", "Liberation Sans"]),
    ("Verdana", &["DejaVu Sans", "Liberation Sans"]),
    ("Georgia", &["Gelasio", "Liberation Serif"]),
    ("Aptos", &["Carlito", "Liberation Sans"]),
];

/// Высота строки шрифтов Microsoft, у которых замена совпадает по
/// ширине букв, но не по высоте: (над базовой линией, под ней,
/// внешний интерлиньяж) в долях кегля, по таблицам самих шрифтов
/// (`usWinAscent`, `usWinDescent`, `hhea.lineGap` на 2048 единиц).
/// У Carlito и Liberation высота та же, что у Calibri, Arial,
/// Times New Roman и Courier New, поэтому их здесь нет.
const MICROSOFT_LINE_METRICS: &[(&str, [f32; 3])] =
    &[("Cambria", [1946.0 / 2048.0, 455.0 / 2048.0, 0.0])];

/// Высота строки шрифта Microsoft `family`, если его замена по высоте
/// другая (пока только Cambria): над базовой линией, под ней
/// и внешний интерлиньяж в долях кегля.
pub fn microsoft_line_metrics(family: &str) -> Option<[f32; 3]> {
    MICROSOFT_LINE_METRICS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(family))
        .map(|(_, m)| *m)
}

/// Последний запасной вариант, если не нашлось ни шрифта, ни замены.
/// Word в таком случае тоже чаще всего берёт Times New Roman.
const LAST_RESORT: &[&str] = &["Times New Roman", "Liberation Serif"];

/// Идентификатор найденного начертания шрифта.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontId(fontdb::ID);

/// Результат подбора шрифта.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontMatch {
    /// Найденное начертание.
    pub id: FontId,
    /// Имя семейства, которое на самом деле используется.
    pub family: String,
    /// Шрифт заменён другим (нужного нет на компьютере).
    pub substituted: bool,
    /// Жирного начертания нет: жирность нужно рисовать самим.
    pub synthetic_bold: bool,
    /// Курсивного начертания нет: наклон нужно рисовать самим.
    pub synthetic_italic: bool,
}

/// Все доступные шрифты: системные и встроенные.
pub struct FontLibrary {
    db: Database,
    /// Имя семейства в нижнем регистре → имя, как оно записано в шрифте.
    families: HashMap<String, String>,
}

impl std::fmt::Debug for FontLibrary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontLibrary")
            .field("faces", &self.db.len())
            .field("families", &self.families.len())
            .finish()
    }
}

impl FontLibrary {
    /// Библиотека без шрифтов. Нужна для тестов и особых случаев.
    pub fn empty() -> FontLibrary {
        FontLibrary {
            db: Database::new(),
            families: HashMap::new(),
        }
    }

    /// Только встроенные шрифты. Результат одинаков на любом компьютере,
    /// поэтому так удобно проверять вёрстку в тестах.
    pub fn embedded_only() -> FontLibrary {
        let mut library = FontLibrary::empty();
        for (_, data) in EMBEDDED_FONTS {
            library.db.load_font_source(Source::Binary(Arc::new(*data)));
        }
        library.index_families();
        library
    }

    /// Встроенные шрифты и все шрифты, установленные в системе.
    /// Системные идут первыми: если на компьютере есть настоящий шрифт,
    /// берётся он.
    pub fn new() -> FontLibrary {
        let mut library = FontLibrary::empty();
        library.db.load_system_fonts();
        for (_, data) in EMBEDDED_FONTS {
            library.db.load_font_source(Source::Binary(Arc::new(*data)));
        }
        library.index_families();
        library
    }

    /// Добавляет шрифт из байтов (например, шрифт, встроенный в документ).
    pub fn load_font_data(&mut self, data: Vec<u8>) {
        self.db.load_font_data(data);
        self.index_families();
    }

    fn index_families(&mut self) {
        for face in self.db.faces() {
            for (name, _) in &face.families {
                self.families
                    .entry(name.to_lowercase())
                    .or_insert_with(|| name.clone());
            }
        }
    }

    /// Число начертаний в библиотеке.
    pub fn len(&self) -> usize {
        self.db.len()
    }

    /// В библиотеке нет ни одного шрифта.
    pub fn is_empty(&self) -> bool {
        self.db.is_empty()
    }

    /// Есть ли семейство с таким именем (без учёта регистра).
    pub fn has_family(&self, family: &str) -> bool {
        self.families.contains_key(&family.to_lowercase())
    }

    /// Подбирает шрифт по имени и начертанию. Если такого шрифта нет,
    /// берёт замену из таблицы, а в крайнем случае — Times New Roman или
    /// встроенный Liberation Serif. `None` — только если библиотека пуста.
    pub fn find(&self, family: &str, bold: bool, italic: bool) -> Option<FontMatch> {
        let requested = family.trim();
        for (candidate, substituted) in candidates(requested) {
            let Some(name) = self.families.get(&candidate.to_lowercase()) else {
                continue;
            };
            let query = Query {
                families: &[Family::Name(name)],
                weight: if bold { Weight::BOLD } else { Weight::NORMAL },
                style: if italic { Style::Italic } else { Style::Normal },
                ..Query::default()
            };
            let Some(id) = self.db.query(&query) else {
                continue;
            };
            let face = self.db.face(id)?;
            return Some(FontMatch {
                id: FontId(id),
                family: name.clone(),
                substituted,
                synthetic_bold: bold && face.weight.0 < Weight::SEMIBOLD.0,
                synthetic_italic: italic && face.style == Style::Normal,
            });
        }
        // Ничего из списка нет: любой шрифт лучше, чем никакого.
        let face = self.db.faces().next()?;
        Some(FontMatch {
            id: FontId(face.id),
            family: face
                .families
                .first()
                .map(|(n, _)| n.clone())
                .unwrap_or_default(),
            substituted: true,
            synthetic_bold: bold,
            synthetic_italic: italic,
        })
    }

    /// Все начертания: идентификатор и имя семейства.
    pub fn faces(&self) -> impl Iterator<Item = (FontId, String)> + '_ {
        self.db.faces().map(|face| {
            let family = face
                .families
                .first()
                .map(|(name, _)| name.clone())
                .unwrap_or_default();
            (FontId(face.id), family)
        })
    }

    /// Вызывает `f` с байтами файла шрифта и номером начертания внутри
    /// файла (в коллекциях `.ttc` их несколько). `None`, если файл
    /// не удалось прочитать.
    pub fn with_face_data<T>(&self, id: FontId, f: impl FnOnce(&[u8], u32) -> T) -> Option<T> {
        self.db.with_face_data(id.0, f)
    }
}

impl Default for FontLibrary {
    fn default() -> FontLibrary {
        FontLibrary::new()
    }
}

/// Порядок поиска: сам шрифт, его замены (рекурсивно), запасные.
/// Второй элемент пары — это уже замена.
fn candidates(family: &str) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    fn push(name: &str, substituted: bool, out: &mut Vec<(String, bool)>) {
        if !out.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)) {
            out.push((name.to_string(), substituted));
        }
    }
    if !family.is_empty() {
        push(family, false, &mut out);
    }
    // Замены замен (Times → Times New Roman → Liberation Serif).
    let mut i = 0;
    while i < out.len() {
        let current = out[i].0.clone();
        if let Some((_, subs)) = SUBSTITUTES
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(&current))
        {
            for sub in *subs {
                push(sub, true, &mut out);
            }
        }
        i += 1;
    }
    for name in LAST_RESORT {
        push(name, true, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> FontLibrary {
        FontLibrary::embedded_only()
    }

    #[test]
    fn embedded_fonts_are_loaded() {
        let lib = library();
        assert_eq!(lib.len(), EMBEDDED_FONTS.len());
        for family in [
            "Carlito",
            "Caladea",
            "Liberation Serif",
            "Liberation Sans",
            "Liberation Mono",
        ] {
            assert!(lib.has_family(family), "{family}");
        }
    }

    #[test]
    fn exact_family_and_style() {
        let lib = library();
        let m = lib.find("liberation serif", true, true).unwrap();
        assert_eq!(m.family, "Liberation Serif");
        assert!(!m.substituted && !m.synthetic_bold && !m.synthetic_italic);
        let face = lib.db.face(m.id.0).unwrap();
        assert_eq!(face.weight, Weight::BOLD);
        assert_eq!(face.style, Style::Italic);
    }

    #[test]
    fn microsoft_fonts_are_substituted() {
        let lib = library();
        let cases = [
            ("Calibri", "Carlito"),
            ("CAMBRIA", "Caladea"),
            ("Times New Roman", "Liberation Serif"),
            ("Times", "Liberation Serif"),
            ("Arial", "Liberation Sans"),
            ("Courier New", "Liberation Mono"),
            ("Совсем неизвестный шрифт", "Liberation Serif"),
            ("", "Liberation Serif"),
        ];
        for (requested, expected) in cases {
            let m = lib.find(requested, false, false).unwrap();
            assert_eq!(m.family, expected, "{requested}");
            assert!(m.substituted, "{requested}");
        }
    }

    #[test]
    fn missing_styles_are_synthesized() {
        let mut lib = FontLibrary::empty();
        assert!(lib.find("Caladea", false, false).is_none());
        lib.load_font_data(include_bytes!("../fonts/Caladea-Regular.ttf").to_vec());
        let m = lib.find("Caladea", true, true).unwrap();
        assert_eq!(m.family, "Caladea");
        assert!(m.synthetic_bold && m.synthetic_italic);
        // Шрифта из списка нет вовсе — берётся любой имеющийся.
        let any = lib.find("Arial", false, false).unwrap();
        assert_eq!(any.family, "Caladea");
        assert!(any.substituted);
    }

    #[test]
    fn face_data_is_a_font_file() {
        let lib = library();
        let m = lib.find("Carlito", false, false).unwrap();
        let header = lib.with_face_data(m.id, |data, index| (data[..4].to_vec(), index));
        assert_eq!(header, Some((vec![0, 1, 0, 0], 0)));
    }

    #[test]
    fn substitution_chain() {
        let names: Vec<_> = candidates("Helvetica")
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert_eq!(
            names,
            [
                "Helvetica",
                "Arial",
                "Liberation Sans",
                "Times New Roman",
                "Liberation Serif"
            ]
        );
    }

    #[test]
    fn system_library_contains_embedded_fonts() {
        let lib = FontLibrary::new();
        assert!(lib.len() >= EMBEDDED_FONTS.len());
        assert!(lib.find("Calibri", false, false).is_some());
    }
}
