//! Шейпинг: превращение текста в глифы (изображения букв в шрифте)
//! с размерами и смещениями.
//!
//! Шаги:
//!
//! 1. Текст делится на куски одного направления письма по алгоритму
//!    Unicode Bidi (иврит и арабский пишутся справа налево).
//! 2. Каждый кусок делится по шрифтам: если в выбранном шрифте нет
//!    буквы, берётся другой шрифт, где она есть.
//! 3. Каждый получившийся кусок отдаётся `harfrust` (перенос HarfBuzz
//!    на Rust), который выбирает
//!    глифы, лигатуры и кернинг по таблицам шрифта.
//!
//! Все размеры в результате — в пунктах.

use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use harfrust::{Buffer, Direction, Feature, Font, ShapeOptions, ShaperFont, Tag};
use unicode_bidi::BidiInfo;

use crate::fonts::{FontId, FontLibrary, FontMatch};

/// Шрифты, которые пробуются первыми, если в выбранном шрифте нет буквы.
/// Дальше — все остальные шрифты по порядку.
const FALLBACK_FAMILIES: &[&str] = &[
    "Liberation Serif",
    "Liberation Sans",
    "Carlito",
    "Segoe UI",
    "Arial Unicode MS",
    "DejaVu Sans",
    "Noto Sans",
    "Noto Sans CJK SC",
    "Microsoft YaHei",
    "SimSun",
    "Segoe UI Symbol",
    "Segoe UI Emoji",
    "Noto Color Emoji",
];

/// Оформление текста, нужное для шейпинга.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    /// Имя шрифта, как в документе.
    pub family: String,
    /// Размер в пунктах.
    pub size: f32,
    /// Жирный.
    pub bold: bool,
    /// Курсив.
    pub italic: bool,
    /// Кернинг (сближение пар букв вроде «AV»). В Word по умолчанию
    /// выключен, включается свойством `w:kern`.
    pub kerning: bool,
    /// Стандартные лигатуры («fi» одним знаком). В Word по умолчанию
    /// выключены. Обязательные лигатуры (например, в арабском) работают
    /// всегда.
    pub ligatures: bool,
    /// Добавка к ширине каждого знака, пт (разреженный или уплотнённый
    /// интервал).
    pub letter_spacing: f32,
    /// Масштаб по ширине: `1.0` — как есть.
    pub width_scale: f32,
    /// Скрытый текст: глифы есть (для курсора), но места не занимают
    /// и не рисуются.
    pub hidden: bool,
}

impl TextStyle {
    /// Обычный текст заданного шрифта и размера.
    pub fn new(family: impl Into<String>, size: f32) -> TextStyle {
        TextStyle {
            family: family.into(),
            size,
            bold: false,
            italic: false,
            kerning: false,
            ligatures: false,
            letter_spacing: 0.0,
            width_scale: 1.0,
            hidden: false,
        }
    }
}

/// Один глиф после шейпинга.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapedGlyph {
    /// Номер глифа в шрифте (0 — «нет такой буквы»).
    pub glyph_id: u16,
    /// Начало буквы (кластера) в исходном тексте, в байтах.
    pub cluster: usize,
    /// Насколько сдвинуться вправо после глифа, пт.
    pub x_advance: f32,
    /// Сдвиг глифа по горизонтали, пт.
    pub x_offset: f32,
    /// Сдвиг глифа вверх, пт.
    pub y_offset: f32,
}

/// Кусок текста одним шрифтом и одного направления письма.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapedRun {
    /// Какой кусок исходного текста, в байтах.
    pub range: Range<usize>,
    /// Шрифт, которым набран кусок.
    pub font: FontMatch,
    /// Размер шрифта, пт.
    pub size: f32,
    /// Уровень Bidi: чётный — слева направо, нечётный — справа налево.
    pub level: u8,
    /// Глифы в порядке отображения (для письма справа налево — с конца).
    pub glyphs: Vec<ShapedGlyph>,
    /// Масштаб глифов по ширине (`w:w`); ширины в `glyphs` уже с ним.
    pub width_scale: f32,
    /// Скрытый текст: ширина ноль, не рисуется.
    pub hidden: bool,
}

impl ShapedRun {
    /// Кусок пишется справа налево.
    pub fn is_rtl(&self) -> bool {
        self.level % 2 == 1
    }

    /// Ширина куска, пт.
    pub fn width(&self) -> f32 {
        self.glyphs.iter().map(|g| g.x_advance).sum()
    }
}

/// Размеры шрифта в долях кегля (умножить на размер в пунктах).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontMetrics {
    /// Высота над базовой линией по таблице `hhea` (или `OS/2`, если
    /// шрифт просит использовать её).
    pub ascent: f32,
    /// Глубина под базовой линией (положительное число).
    pub descent: f32,
    /// Дополнительный промежуток между строками.
    pub line_gap: f32,
    /// Высота над базовой линией по Windows (`usWinAscent`). Word
    /// считает высоту строки по этим значениям.
    pub win_ascent: f32,
    /// Глубина под базовой линией по Windows (`usWinDescent`).
    pub win_descent: f32,
    /// Дополнительный промежуток над строкой по правилам Windows
    /// («внешний интерлиньяж»): то, на что промежуток из `hhea` больше
    /// разницы между размерами Windows и `hhea`. Одинарный интервал
    /// в Word — `win_ascent + win_descent + external_leading`.
    pub external_leading: f32,
    /// Положение линии подчёркивания (отрицательное — под базовой линией).
    pub underline_position: f32,
    /// Толщина линии подчёркивания.
    pub underline_thickness: f32,
    /// Положение линии зачёркивания над базовой линией.
    pub strikeout_position: f32,
    /// Толщина линии зачёркивания.
    pub strikeout_thickness: f32,
}

/// Байты шрифта, номер начертания в файле и шрифт, готовый к шейпингу.
struct FontData {
    bytes: Arc<Vec<u8>>,
    index: u32,
    font: Font,
}

/// Шейпер с кешем шрифтов. Один на программу (или на поток).
#[derive(Default)]
pub struct Shaper {
    data: HashMap<FontId, Option<Rc<FontData>>>,
    /// Какой шрифт подходит для буквы, если в основном её нет:
    /// (буква, жирный, курсив) → шрифт.
    fallback: HashMap<(char, bool, bool), Option<FontMatch>>,
    /// Есть ли буква в шрифте: разбирать файл шрифта на каждый вопрос
    /// дорого, а спрашивают про каждую букву текста.
    glyphs: HashMap<(FontId, char), bool>,
}

impl std::fmt::Debug for Shaper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shaper")
            .field("fonts", &self.data.len())
            .finish()
    }
}

impl Shaper {
    /// Новый шейпер с пустым кешем.
    pub fn new() -> Shaper {
        Shaper::default()
    }

    fn font_data(&mut self, library: &FontLibrary, id: FontId) -> Option<Rc<FontData>> {
        self.data
            .entry(id)
            .or_insert_with(|| {
                let (bytes, index) =
                    library.with_face_data(id, |bytes, index| (Arc::new(bytes.to_vec()), index))?;
                ttf_parser::Face::parse(&bytes, index).ok()?;
                let blob: Arc<dyn AsRef<[u8]> + Send + Sync> = bytes.clone();
                let font = Font::new(blob, index)?;
                Some(Rc::new(FontData { bytes, index, font }))
            })
            .clone()
    }

    /// Размеры шрифта. `None`, если файл шрифта не читается.
    pub fn metrics(&mut self, library: &FontLibrary, id: FontId) -> Option<FontMetrics> {
        let data = self.font_data(library, id)?;
        let face = ttf_parser::Face::parse(&data.bytes, data.index).ok()?;
        let em = f32::from(face.units_per_em());
        let units = |v: i16| f32::from(v) / em;
        let os2 = face.tables().os2;
        let use_typo = os2.is_some_and(|t| t.use_typographic_metrics());
        let (ascent, descent, line_gap) = match os2 {
            Some(t) if use_typo => (
                t.typographic_ascender(),
                t.typographic_descender(),
                t.typographic_line_gap(),
            ),
            _ => (face.ascender(), face.descender(), face.line_gap()),
        };
        let (win_ascent, win_descent) = match os2 {
            Some(t) => (
                f32::from(t.windows_ascender()) / em,
                // В таблице usWinDescent положительный, ttf-parser отдаёт
                // его со знаком минус, как и остальные глубины.
                -f32::from(t.windows_descender()) / em,
            ),
            None => (units(face.ascender()), -units(face.descender())),
        };
        let hhea = face.tables().hhea;
        let hhea_height = i32::from(hhea.ascender) - i32::from(hhea.descender);
        let external_leading = (i32::from(hhea.line_gap)
            - ((win_ascent + win_descent) * em).round() as i32
            + hhea_height)
            .max(0);
        let underline = face.underline_metrics();
        let strikeout = face.strikeout_metrics();
        Some(FontMetrics {
            ascent: units(ascent),
            descent: -units(descent),
            line_gap: units(line_gap),
            win_ascent,
            win_descent,
            external_leading: external_leading as f32 / em,
            underline_position: underline.map_or(-0.1, |m| units(m.position)),
            underline_thickness: underline.map_or(0.05, |m| units(m.thickness)),
            strikeout_position: strikeout.map_or(0.3, |m| units(m.position)),
            strikeout_thickness: strikeout.map_or(0.05, |m| units(m.thickness)),
        })
    }

    /// Есть ли в шрифте буква.
    pub fn has_glyph(&mut self, library: &FontLibrary, id: FontId, c: char) -> bool {
        if let Some(&known) = self.glyphs.get(&(id, c)) {
            return known;
        }
        let found = self.font_data(library, id).is_some_and(|data| {
            ttf_parser::Face::parse(&data.bytes, data.index)
                .is_ok_and(|face| face.glyph_index(c).is_some())
        });
        self.glyphs.insert((id, c), found);
        found
    }

    /// Шрифт, в котором есть буква `c`. Сначала пробуются известные
    /// запасные шрифты, потом все остальные. Результат запоминается.
    fn fallback_for(
        &mut self,
        library: &FontLibrary,
        c: char,
        bold: bool,
        italic: bool,
    ) -> Option<FontMatch> {
        if let Some(found) = self.fallback.get(&(c, bold, italic)) {
            return found.clone();
        }
        let mut found = None;
        for family in FALLBACK_FAMILIES {
            if !library.has_family(family) {
                continue;
            }
            if let Some(m) = library.find(family, bold, italic)
                && self.has_glyph(library, m.id, c)
            {
                found = Some(m);
                break;
            }
        }
        if found.is_none() {
            for (id, family) in library.faces() {
                if self.has_glyph(library, id, c) {
                    found = library.find(&family, bold, italic);
                    break;
                }
            }
        }
        self.fallback.insert((c, bold, italic), found.clone());
        found
    }

    /// Превращает текст абзаца (или его часть) в глифы. Куски
    /// возвращаются в логическом порядке (как в тексте); переставить
    /// куски справа налево для показа — дело разбиения на строки.
    pub fn shape(
        &mut self,
        library: &FontLibrary,
        text: &str,
        style: &TextStyle,
    ) -> Vec<ShapedRun> {
        let mut runs = Vec::new();
        for (range, level) in direction_runs(text) {
            runs.extend(self.shape_range(library, text, range, level, style));
        }
        runs
    }

    /// Превращает в глифы кусок `range` текста `text`, который весь
    /// пишется в одном направлении (`level` — уровень Bidi, его считает
    /// вызывающий для всего абзаца). Кусок делится по шрифтам, если
    /// каких-то букв нет в основном. Позиции в результате — в байтах
    /// всего `text`.
    pub fn shape_range(
        &mut self,
        library: &FontLibrary,
        text: &str,
        range: Range<usize>,
        level: u8,
        style: &TextStyle,
    ) -> Vec<ShapedRun> {
        if range.is_empty() || text.get(range.clone()).is_none() {
            return Vec::new();
        }
        let Some(primary) = library.find(&style.family, style.bold, style.italic) else {
            return Vec::new();
        };
        let mut runs = Vec::new();
        for (font_range, font) in self.split_by_font(library, text, range, &primary, style) {
            if let Some(run) = self.shape_segment(library, text, font_range, font, level, style) {
                runs.push(run);
            }
        }
        runs
    }

    /// Делит кусок текста на части, для которых есть буквы в одном шрифте.
    fn split_by_font(
        &mut self,
        library: &FontLibrary,
        text: &str,
        range: Range<usize>,
        primary: &FontMatch,
        style: &TextStyle,
    ) -> Vec<(Range<usize>, FontMatch)> {
        let mut parts: Vec<(Range<usize>, FontMatch)> = Vec::new();
        for (offset, c) in text[range.clone()].char_indices() {
            let start = range.start + offset;
            let end = start + c.len_utf8();
            // Пробелы, управляющие и составные знаки — тем же шрифтом,
            // что и предыдущая буква.
            let font = match parts.last() {
                Some((_, previous)) if c.is_whitespace() || c.is_control() || is_combining(c) => {
                    previous.clone()
                }
                _ if self.has_glyph(library, primary.id, c)
                    || (c == NO_BREAK_HYPHEN && self.has_glyph(library, primary.id, '-'))
                    || c.is_whitespace()
                    || c.is_control() =>
                {
                    primary.clone()
                }
                _ => self
                    .fallback_for(library, c, style.bold, style.italic)
                    .unwrap_or_else(|| primary.clone()),
            };
            match parts.last_mut() {
                Some((r, f)) if f.id == font.id => r.end = end,
                _ => parts.push((start..end, font)),
            }
        }
        parts
    }

    fn shape_segment(
        &mut self,
        library: &FontLibrary,
        text: &str,
        range: Range<usize>,
        font: FontMatch,
        level: u8,
        style: &TextStyle,
    ) -> Option<ShapedRun> {
        let segment = &text[range.clone()];
        // Неразрывный дефис есть не во всех шрифтах: тогда он рисуется
        // обычным дефисом, как в Word.
        let hyphen =
            segment.contains(NO_BREAK_HYPHEN) && !self.has_glyph(library, font.id, NO_BREAK_HYPHEN);
        let data = self.font_data(library, font.id)?;
        let shaper = ShaperFont::new(&data.font);
        let mut buffer = Buffer::new();
        if hyphen {
            for (i, c) in segment.char_indices() {
                let c = if c == NO_BREAK_HYPHEN { '-' } else { c };
                buffer.push(u32::from(c), u32::try_from(i).unwrap_or(u32::MAX));
            }
        } else {
            buffer.push_str(segment);
        }
        buffer.set_direction(if level % 2 == 1 {
            Direction::RightToLeft
        } else {
            Direction::LeftToRight
        });
        buffer.guess_segment_properties();
        let mut features = Vec::new();
        if !style.kerning {
            features.push(Feature::new(Tag::new(b"kern"), 0, ..));
        }
        if !style.ligatures {
            for tag in [b"liga", b"clig"] {
                features.push(Feature::new(Tag::new(tag), 0, ..));
            }
        }
        harfrust::shape(
            &shaper,
            &mut buffer,
            ShapeOptions::new().features(&features),
        )
        .ok()?;
        let scale = style.size / f32::from(shaper.units_per_em());
        let width_scale = if style.width_scale > 0.0 {
            style.width_scale
        } else {
            1.0
        };
        let output = buffer;
        let glyphs = output
            .glyph_infos()
            .iter()
            .zip(output.glyph_positions())
            .map(|(info, pos)| {
                let advance = pos.x_advance as f32 * scale * width_scale + style.letter_spacing;
                ShapedGlyph {
                    glyph_id: u16::try_from(info.glyph_id).unwrap_or(0),
                    cluster: range.start + info.cluster as usize,
                    x_advance: if style.hidden { 0.0 } else { advance },
                    x_offset: pos.x_offset as f32 * scale * width_scale,
                    y_offset: pos.y_offset as f32 * scale,
                }
            })
            .collect();
        Some(ShapedRun {
            range,
            font,
            size: style.size,
            level,
            glyphs,
            width_scale,
            hidden: style.hidden,
        })
    }
}

/// Куски текста одного направления письма (в логическом порядке)
/// и их уровни Bidi.
pub fn direction_runs(text: &str) -> Vec<(Range<usize>, u8)> {
    let info = BidiInfo::new(text, None);
    let mut runs: Vec<(Range<usize>, u8)> = Vec::new();
    for (i, level) in info.levels.iter().enumerate() {
        let level = level.number();
        match runs.last_mut() {
            Some((range, l)) if *l == level => range.end = i + 1,
            _ => runs.push((i..i + 1, level)),
        }
    }
    runs
}

/// Неразрывный дефис (Ctrl+Shift+-).
const NO_BREAK_HYPHEN: char = '\u{2011}';

/// Составной знак (ударение, точки над буквой) или невидимый
/// соединитель, который должен остаться с предыдущей буквой.
fn is_combining(c: char) -> bool {
    matches!(c,
        '\u{0300}'..='\u{036F}'
        | '\u{1AB0}'..='\u{1AFF}'
        | '\u{1DC0}'..='\u{1DFF}'
        | '\u{20D0}'..='\u{20FF}'
        | '\u{FE20}'..='\u{FE2F}'
        | '\u{FE00}'..='\u{FE0F}'
        | '\u{200C}' | '\u{200D}'
        | '\u{E0100}'..='\u{E01EF}')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(text: &str, style: &TextStyle) -> Vec<ShapedRun> {
        let library = FontLibrary::embedded_only();
        Shaper::new().shape(&library, text, style)
    }

    #[test]
    fn latin_text_uses_font_advances() {
        let runs = shape("Hello", &TextStyle::new("Times New Roman", 12.0));
        assert_eq!(runs.len(), 1);
        let run = &runs[0];
        assert_eq!(run.font.family, "Liberation Serif");
        assert_eq!(run.glyphs.len(), 5);
        assert!(!run.is_rtl());
        // «H» в Times New Roman и Liberation Serif — 1479/2048 кегля.
        let h = run.glyphs[0].x_advance;
        assert!((h - 1479.0 / 2048.0 * 12.0).abs() < 1e-4, "{h}");
        let clusters: Vec<_> = run.glyphs.iter().map(|g| g.cluster).collect();
        assert_eq!(clusters, [0, 1, 2, 3, 4]);
        assert!((run.width() - run.glyphs.iter().map(|g| g.x_advance).sum::<f32>()).abs() < 1e-6);
    }

    #[test]
    fn kerning_is_optional() {
        let plain = TextStyle::new("Liberation Serif", 24.0);
        let kerned = TextStyle {
            kerning: true,
            ..plain.clone()
        };
        let without = shape("AV", &plain)[0].width();
        let with = shape("AV", &kerned)[0].width();
        assert!(with < without, "{with} < {without}");
    }

    #[test]
    fn ligatures_are_optional() {
        let plain = TextStyle::new("Carlito", 12.0);
        let liga = TextStyle {
            ligatures: true,
            ..plain.clone()
        };
        assert_eq!(shape("fi", &plain)[0].glyphs.len(), 2);
        assert_eq!(shape("fi", &liga)[0].glyphs.len(), 1);
    }

    #[test]
    fn right_to_left_text() {
        let text = "abc שלום def";
        let runs = shape(text, &TextStyle::new("Arial", 12.0));
        let levels: Vec<_> = runs.iter().map(|r| r.level).collect();
        assert_eq!(levels, [0, 1, 0]);
        let hebrew = &runs[1];
        assert!(hebrew.is_rtl());
        assert_eq!(&text[hebrew.range.clone()], "שלום");
        // Глифы идут в порядке показа: последняя буква слова — первой.
        let clusters: Vec<_> = hebrew.glyphs.iter().map(|g| g.cluster).collect();
        let mut sorted = clusters.clone();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        assert_eq!(clusters, sorted);
        assert!(hebrew.glyphs.iter().all(|g| g.glyph_id != 0));
    }

    #[test]
    fn missing_letters_come_from_another_font() {
        // В Caladea (замена Cambria) нет кириллицы.
        let text = "ab Жук";
        let runs = shape(text, &TextStyle::new("Cambria", 11.0));
        let parts: Vec<_> = runs
            .iter()
            .map(|r| (&text[r.range.clone()], r.font.family.as_str()))
            .collect();
        assert_eq!(parts, [("ab ", "Caladea"), ("Жук", "Liberation Serif")]);
        assert!(runs.iter().flat_map(|r| &r.glyphs).all(|g| g.glyph_id != 0));
    }

    #[test]
    fn unknown_letters_keep_the_main_font() {
        // Иероглифа нет ни в одном встроенном шрифте.
        let runs = shape("a中", &TextStyle::new("Carlito", 11.0));
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].glyphs[1].glyph_id, 0);
        assert!(shape("", &TextStyle::new("Carlito", 11.0)).is_empty());
    }

    #[test]
    fn part_of_a_text_is_shaped_in_place() {
        let library = FontLibrary::embedded_only();
        let style = TextStyle::new("Carlito", 11.0);
        let mut shaper = Shaper::new();
        let runs = shaper.shape_range(&library, "ab cd", 3..5, 0, &style);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].range, 3..5);
        let clusters: Vec<_> = runs[0].glyphs.iter().map(|g| g.cluster).collect();
        assert_eq!(clusters, [3, 4]);
        // Пустой кусок и кусок за концом текста — пусто.
        assert!(
            shaper
                .shape_range(&library, "ab", 1..1, 0, &style)
                .is_empty()
        );
        assert!(
            shaper
                .shape_range(&library, "ab", 1..9, 0, &style)
                .is_empty()
        );
    }

    #[test]
    fn combining_marks_stay_with_their_letter() {
        let text = "Ж\u{0301}";
        let runs = shape(text, &TextStyle::new("Cambria", 11.0));
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].range, 0..text.len());
    }

    #[test]
    fn metrics() {
        let library = FontLibrary::embedded_only();
        let mut shaper = Shaper::new();
        let serif = library.find("Liberation Serif", false, false).unwrap();
        let m = shaper.metrics(&library, serif.id).unwrap();
        assert!((m.win_ascent - 1825.0 / 2048.0).abs() < 1e-6);
        assert!((m.win_descent - 443.0 / 2048.0).abs() < 1e-6);
        assert!((m.line_gap - 87.0 / 2048.0).abs() < 1e-6);
        // Одинарная строка Times New Roman 12 пт в Word — 13,8 пт.
        let single = (m.win_ascent + m.win_descent + m.external_leading) * 12.0;
        assert!((single - 13.799).abs() < 1e-3, "{single}");
        // А Calibri (Carlito) 11 пт — 13,43 пт: промежуток уже внутри
        // размеров Windows.
        let carlito = library.find("Carlito", false, false).unwrap();
        let c = shaper.metrics(&library, carlito.id).unwrap();
        assert_eq!(c.external_leading, 0.0);
        let single = (c.win_ascent + c.win_descent) * 11.0;
        assert!((single - 13.428).abs() < 1e-3, "{single}");
        assert!(m.underline_position < 0.0 && m.underline_thickness > 0.0);
        // Caladea по высоте как Cambria: 950 + 222 на 1000 единиц.
        let caladea = library.find("Caladea", false, false).unwrap();
        let c = shaper.metrics(&library, caladea.id).unwrap();
        assert!((c.win_ascent - 0.95).abs() < 1e-6 && (c.win_descent - 0.222).abs() < 1e-6);
    }

    #[test]
    fn direction_runs_split_mixed_text() {
        assert_eq!(direction_runs("abc"), [(0..3, 0)]);
        assert!(direction_runs("").is_empty());
        let runs = direction_runs("א b");
        assert_eq!(runs[0].1 % 2, 1);
    }
}
