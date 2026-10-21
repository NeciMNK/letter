//! Строки текста из PDF: где на странице стоит каждая строка и что в ней
//! написано. Нужно для сверки нашей вёрстки с PDF, сохранённым из Word.
//!
//! Это не полноценный разбор PDF, а только то, что нужно для сверки:
//! содержимое страницы проигрывается (матрицы, шрифт, сдвиги текста),
//! для каждой буквы запоминаются положение и символ (по таблице
//! `ToUnicode` шрифта), затем буквы с одной базовой линией собираются
//! в строки. Картинки, линии и цвета пропускаются.

use std::collections::HashMap;

use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object};

/// Сколько вложенных форм (`Do`) проигрываем, не больше.
const MAX_FORM_DEPTH: usize = 8;

/// Страница PDF.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfPage {
    /// Ширина, пт.
    pub width: f32,
    /// Высота, пт.
    pub height: f32,
    /// Строки текста сверху вниз.
    pub lines: Vec<PdfLine>,
}

/// Строка текста на странице PDF.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfLine {
    /// Левый край первой буквы, пт от левого края листа.
    pub x: f32,
    /// Базовая линия, пт от верхнего края листа.
    pub baseline: f32,
    /// Текст строки слева направо; между далеко стоящими буквами
    /// вставлен пробел.
    pub text: String,
}

/// Читает строки текста всех страниц PDF.
pub fn read_pdf_lines(bytes: &[u8]) -> Result<Vec<PdfPage>, String> {
    let doc = Document::load_mem(bytes).map_err(|e| format!("не удалось прочитать PDF: {e}"))?;
    let mut pages = Vec::new();
    for page_id in doc.get_pages().into_values() {
        let page = doc
            .get_dictionary(page_id)
            .map_err(|e| format!("битая страница PDF: {e}"))?;
        let [x0, y0, x1, y1] = media_box(&doc, page);
        let mut player = Player::new(&doc);
        if let Some(resources) = inherited(&doc, page, b"Resources").and_then(|o| dict(&doc, o)) {
            let content = doc.get_page_content(page_id);
            let base = [1.0, 0.0, 0.0, 1.0, -x0, -y0];
            player.play(&content, resources, base, 0);
        }
        let height = (y1 - y0).abs();
        pages.push(PdfPage {
            width: (x1 - x0).abs(),
            height,
            lines: group_lines(player.glyphs, height),
        });
    }
    Ok(pages)
}

/// Одна нарисованная буква.
#[derive(Debug, Clone)]
struct Glyph {
    text: String,
    /// Левый край, пт от левого края листа.
    x: f32,
    /// Правый край.
    right: f32,
    /// Базовая линия, пт от нижнего края листа (как в PDF).
    y: f32,
    /// Размер шрифта на листе, пт.
    size: f32,
}

/// Матрица PDF `[a b c d e f]`.
type Matrix = [f32; 6];

const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `m` × `n`: сначала `m`, потом `n`.
fn multiply(m: Matrix, n: Matrix) -> Matrix {
    [
        m[0] * n[0] + m[1] * n[2],
        m[0] * n[1] + m[1] * n[3],
        m[2] * n[0] + m[3] * n[2],
        m[2] * n[1] + m[3] * n[3],
        m[4] * n[0] + m[5] * n[2] + n[4],
        m[4] * n[1] + m[5] * n[3] + n[5],
    ]
}

fn apply(m: Matrix, x: f32, y: f32) -> (f32, f32) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

/// Шрифт PDF: ширины букв и их текст.
#[derive(Debug, Default)]
struct Font {
    /// Коды по два байта (составной шрифт `Type0`).
    two_bytes: bool,
    /// Ширина буквы по коду, в единицах шрифта.
    widths: HashMap<u32, f32>,
    /// Ширина, если кода нет в таблице.
    default_width: f32,
    /// Единицы шрифта → пункты при размере 1 (обычно 1/1000).
    scale: f32,
    /// Текст буквы по коду (`ToUnicode`).
    unicode: HashMap<u32, String>,
}

impl Font {
    fn load(doc: &Document, font: &Dictionary) -> Font {
        let subtype = font
            .get(b"Subtype")
            .and_then(Object::as_name)
            .unwrap_or(b"");
        let mut result = Font {
            scale: 0.001,
            unicode: to_unicode(doc, font),
            ..Font::default()
        };
        if subtype == b"Type0" {
            result.two_bytes = true;
            result.default_width = 1000.0;
            let descendant = font
                .get(b"DescendantFonts")
                .ok()
                .and_then(|o| array(doc, o))
                .and_then(|a| a.first())
                .and_then(|o| dict(doc, o));
            if let Some(cid) = descendant {
                if let Some(dw) = cid.get(b"DW").ok().and_then(|o| number(doc, o)) {
                    result.default_width = dw;
                }
                if let Some(w) = cid.get(b"W").ok().and_then(|o| array(doc, o)) {
                    read_cid_widths(doc, w, &mut result.widths);
                }
            }
        } else {
            if subtype == b"Type3"
                && let Some(m) = font.get(b"FontMatrix").ok().and_then(|o| array(doc, o))
                && let Some(a) = m.first().and_then(|o| number(doc, o))
            {
                result.scale = a;
            }
            let first = font
                .get(b"FirstChar")
                .ok()
                .and_then(|o| number(doc, o))
                .unwrap_or(0.0);
            if let Some(widths) = font.get(b"Widths").ok().and_then(|o| array(doc, o)) {
                for (i, w) in widths.iter().enumerate() {
                    if let Some(w) = number(doc, w) {
                        result.widths.insert(first as u32 + i as u32, w);
                    }
                }
            }
            result.default_width = font
                .get(b"FontDescriptor")
                .ok()
                .and_then(|o| dict(doc, o))
                .and_then(|d| d.get(b"MissingWidth").ok())
                .and_then(|o| number(doc, o))
                .unwrap_or(0.0);
        }
        result
    }

    /// Коды букв в строке PDF.
    fn codes(&self, bytes: &[u8]) -> Vec<u32> {
        if self.two_bytes {
            bytes
                .chunks(2)
                .map(|c| c.iter().fold(0u32, |acc, b| (acc << 8) | u32::from(*b)))
                .collect()
        } else {
            bytes.iter().map(|b| u32::from(*b)).collect()
        }
    }

    fn width(&self, code: u32) -> f32 {
        self.widths
            .get(&code)
            .copied()
            .unwrap_or(self.default_width)
            * self.scale
    }

    fn text(&self, code: u32) -> String {
        match self.unicode.get(&code) {
            Some(text) => text.clone(),
            // Без таблицы: для простого шрифта байт почти всегда совпадает
            // с Latin-1.
            None if !self.two_bytes => char::from_u32(code).map(String::from).unwrap_or_default(),
            None => String::new(),
        }
    }
}

/// Ширины составного шрифта: `[код [w w …]]` или `[первый последний w]`.
fn read_cid_widths(doc: &Document, w: &[Object], widths: &mut HashMap<u32, f32>) {
    let mut i = 0;
    while i < w.len() {
        let Some(first) = number(doc, &w[i]) else {
            return;
        };
        let first = first as u32;
        match w.get(i + 1) {
            Some(next) if array(doc, next).is_some() => {
                for (k, v) in array(doc, next).unwrap_or(&[]).iter().enumerate() {
                    if let Some(v) = number(doc, v) {
                        widths.insert(first + k as u32, v);
                    }
                }
                i += 2;
            }
            Some(last) => {
                let (Some(last), Some(v)) =
                    (number(doc, last), w.get(i + 2).and_then(|o| number(doc, o)))
                else {
                    return;
                };
                // Защита от огромных диапазонов в битых файлах.
                for code in first..=(last as u32).min(first.saturating_add(65_535)) {
                    widths.insert(code, v);
                }
                i += 3;
            }
            None => return,
        }
    }
}

/// Таблица `ToUnicode`: код буквы → текст.
fn to_unicode(doc: &Document, font: &Dictionary) -> HashMap<u32, String> {
    let mut map = HashMap::new();
    let Some(Object::Stream(stream)) = font
        .get(b"ToUnicode")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .map(|(_, o)| o)
    else {
        return map;
    };
    let data = stream
        .decompressed_content()
        .unwrap_or_else(|_| stream.content.clone());
    let tokens = cmap_tokens(&data);
    let mut i = 0;
    while i < tokens.len() {
        match tokens[i] {
            CmapToken::Word(b"beginbfchar") => {
                i += 1;
                while let (Some(CmapToken::Hex(src)), Some(CmapToken::Hex(dst))) =
                    (tokens.get(i), tokens.get(i + 1))
                {
                    map.insert(code_of(src), utf16(dst));
                    i += 2;
                }
            }
            CmapToken::Word(b"beginbfrange") => {
                i += 1;
                while let (Some(CmapToken::Hex(lo)), Some(CmapToken::Hex(hi))) =
                    (tokens.get(i), tokens.get(i + 1))
                {
                    let (lo, hi) = (code_of(lo), code_of(hi));
                    let hi = hi.min(lo.saturating_add(65_535));
                    match tokens.get(i + 2) {
                        Some(CmapToken::Hex(dst)) => {
                            let mut units = utf16_units(dst);
                            for code in lo..=hi {
                                map.insert(code, String::from_utf16_lossy(&units));
                                if let Some(last) = units.last_mut() {
                                    *last = last.wrapping_add(1);
                                }
                            }
                            i += 3;
                        }
                        Some(CmapToken::Array(items)) => {
                            for (code, dst) in (lo..=hi).zip(items) {
                                map.insert(code, utf16(dst));
                            }
                            i += 3;
                        }
                        _ => break,
                    }
                }
            }
            _ => i += 1,
        }
    }
    map
}

#[derive(Debug)]
enum CmapToken<'a> {
    Hex(Vec<u8>),
    Array(Vec<Vec<u8>>),
    Word(&'a [u8]),
}

/// Простейший разбор CMap: строки `<…>`, массивы строк `[…]` и слова.
fn cmap_tokens(data: &[u8]) -> Vec<CmapToken<'_>> {
    let mut tokens = Vec::new();
    let mut array: Option<Vec<Vec<u8>>> = None;
    let mut i = 0;
    while i < data.len() {
        match data[i] {
            b'<' if data.get(i + 1) != Some(&b'<') => {
                let end = data[i..]
                    .iter()
                    .position(|&b| b == b'>')
                    .map_or(data.len(), |p| i + p);
                let hex = hex_bytes(&data[i + 1..end]);
                match &mut array {
                    Some(items) => items.push(hex),
                    None => tokens.push(CmapToken::Hex(hex)),
                }
                i = end + 1;
            }
            b'[' => {
                array = Some(Vec::new());
                i += 1;
            }
            b']' => {
                if let Some(items) = array.take() {
                    tokens.push(CmapToken::Array(items));
                }
                i += 1;
            }
            b'%' => {
                while i < data.len() && data[i] != b'\n' && data[i] != b'\r' {
                    i += 1;
                }
            }
            b if b.is_ascii_alphabetic() => {
                let start = i;
                while i < data.len() && data[i].is_ascii_alphanumeric() {
                    i += 1;
                }
                tokens.push(CmapToken::Word(&data[start..i]));
            }
            _ => i += 1,
        }
    }
    tokens
}

fn hex_bytes(hex: &[u8]) -> Vec<u8> {
    let digits: Vec<u8> = hex
        .iter()
        .filter_map(|&b| (b as char).to_digit(16).map(|d| d as u8))
        .collect();
    digits
        .chunks(2)
        .map(|pair| (pair[0] << 4) | pair.get(1).copied().unwrap_or(0))
        .collect()
}

fn code_of(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .take(4)
        .fold(0u32, |acc, b| (acc << 8) | u32::from(*b))
}

fn utf16_units(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks(2)
        .map(|c| (u16::from(c[0]) << 8) | u16::from(c.get(1).copied().unwrap_or(0)))
        .collect()
}

fn utf16(bytes: &[u8]) -> String {
    String::from_utf16_lossy(&utf16_units(bytes))
}

/// Состояние графики, которое сохраняют `q` и `Q`.
#[derive(Debug, Clone, Copy)]
struct State {
    ctm: Matrix,
    char_spacing: f32,
    word_spacing: f32,
    /// Горизонтальное сжатие, доля (Tz / 100).
    scale: f32,
    leading: f32,
    rise: f32,
    size: f32,
    invisible: bool,
}

impl State {
    fn new(ctm: Matrix) -> State {
        State {
            ctm,
            char_spacing: 0.0,
            word_spacing: 0.0,
            scale: 1.0,
            leading: 0.0,
            rise: 0.0,
            size: 0.0,
            invisible: false,
        }
    }
}

/// Проигрыватель содержимого страницы.
struct Player<'a> {
    doc: &'a Document,
    /// Загруженные шрифты по номеру объекта (или по имени для встроенных
    /// словарей — тогда без кеша).
    fonts: HashMap<lopdf::ObjectId, std::rc::Rc<Font>>,
    glyphs: Vec<Glyph>,
}

impl<'a> Player<'a> {
    fn new(doc: &'a Document) -> Player<'a> {
        Player {
            doc,
            fonts: HashMap::new(),
            glyphs: Vec::new(),
        }
    }

    fn font(&mut self, resources: &Dictionary, name: &[u8]) -> Option<std::rc::Rc<Font>> {
        let fonts = resources
            .get(b"Font")
            .ok()
            .and_then(|o| dict(self.doc, o))?;
        let entry = fonts.get(name).ok()?;
        if let Object::Reference(id) = entry
            && let Some(font) = self.fonts.get(id)
        {
            return Some(font.clone());
        }
        let font = std::rc::Rc::new(Font::load(self.doc, dict(self.doc, entry)?));
        if let Object::Reference(id) = entry {
            self.fonts.insert(*id, font.clone());
        }
        Some(font)
    }

    fn play(&mut self, content: &[u8], resources: &Dictionary, ctm: Matrix, depth: usize) {
        let Ok(content) = Content::decode(content) else {
            return;
        };
        let mut state = State::new(ctm);
        let mut stack = Vec::new();
        let mut font: Option<std::rc::Rc<Font>> = None;
        let mut text_matrix = IDENTITY;
        let mut line_matrix = IDENTITY;
        for op in &content.operations {
            let n = |i: usize| op.operands.get(i).and_then(|o| number(self.doc, o));
            match op.operator.as_str() {
                "q" => stack.push((state, font.clone())),
                "Q" => {
                    if let Some((s, f)) = stack.pop() {
                        state = s;
                        font = f;
                    }
                }
                "cm" => {
                    if let Some(m) = matrix(&op.operands, self.doc) {
                        state.ctm = multiply(m, state.ctm);
                    }
                }
                "BT" => {
                    text_matrix = IDENTITY;
                    line_matrix = IDENTITY;
                }
                "Tc" => state.char_spacing = n(0).unwrap_or(0.0),
                "Tw" => state.word_spacing = n(0).unwrap_or(0.0),
                "Tz" => state.scale = n(0).unwrap_or(100.0) / 100.0,
                "TL" => state.leading = n(0).unwrap_or(0.0),
                "Ts" => state.rise = n(0).unwrap_or(0.0),
                "Tr" => state.invisible = n(0) == Some(3.0),
                "Tf" => {
                    state.size = n(1).unwrap_or(0.0);
                    font = op
                        .operands
                        .first()
                        .and_then(|o| o.as_name().ok())
                        .and_then(|name| self.font(resources, name));
                }
                "Tm" => {
                    if let Some(m) = matrix(&op.operands, self.doc) {
                        text_matrix = m;
                        line_matrix = m;
                    }
                }
                "Td" | "TD" => {
                    let (tx, ty) = (n(0).unwrap_or(0.0), n(1).unwrap_or(0.0));
                    if op.operator == "TD" {
                        state.leading = -ty;
                    }
                    line_matrix = multiply([1.0, 0.0, 0.0, 1.0, tx, ty], line_matrix);
                    text_matrix = line_matrix;
                }
                "T*" => {
                    line_matrix = multiply([1.0, 0.0, 0.0, 1.0, 0.0, -state.leading], line_matrix);
                    text_matrix = line_matrix;
                }
                "Tj" | "'" | "\"" | "TJ" => {
                    if op.operator == "\"" {
                        state.word_spacing = n(0).unwrap_or(0.0);
                        state.char_spacing = n(1).unwrap_or(0.0);
                    }
                    if op.operator == "'" || op.operator == "\"" {
                        line_matrix =
                            multiply([1.0, 0.0, 0.0, 1.0, 0.0, -state.leading], line_matrix);
                        text_matrix = line_matrix;
                    }
                    let Some(font) = font.as_deref() else {
                        continue;
                    };
                    let parts: &[Object] = match op.operator.as_str() {
                        "TJ" => op
                            .operands
                            .first()
                            .and_then(|o| array(self.doc, o))
                            .unwrap_or(&[]),
                        _ => op.operands.last().map(std::slice::from_ref).unwrap_or(&[]),
                    };
                    for part in parts {
                        match part {
                            Object::String(bytes, _) => {
                                self.show(font, &state, &mut text_matrix, bytes);
                            }
                            other => {
                                if let Some(shift) = number(self.doc, other) {
                                    let tx = -shift / 1000.0 * state.size * state.scale;
                                    text_matrix =
                                        multiply([1.0, 0.0, 0.0, 1.0, tx, 0.0], text_matrix);
                                }
                            }
                        }
                    }
                }
                "Do" if depth < MAX_FORM_DEPTH => {
                    if let Some(name) = op.operands.first().and_then(|o| o.as_name().ok()) {
                        self.form(resources, name, state.ctm, depth);
                    }
                }
                _ => {}
            }
        }
    }

    /// Рисует форму (`Do`), если это форма, а не картинка.
    fn form(&mut self, resources: &Dictionary, name: &[u8], ctm: Matrix, depth: usize) {
        let doc = self.doc;
        let Some(Object::Stream(stream)) = resources
            .get(b"XObject")
            .ok()
            .and_then(|o| dict(doc, o))
            .and_then(|x| x.get(name).ok())
            .and_then(|o| doc.dereference(o).ok())
            .map(|(_, o)| o)
        else {
            return;
        };
        if stream.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Form") {
            return;
        }
        let form_matrix = stream
            .dict
            .get(b"Matrix")
            .ok()
            .and_then(|o| array(doc, o))
            .and_then(|a| matrix(a, doc))
            .unwrap_or(IDENTITY);
        let form_resources = stream
            .dict
            .get(b"Resources")
            .ok()
            .and_then(|o| dict(doc, o))
            .unwrap_or(resources);
        let content = stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone());
        self.play(
            &content,
            form_resources,
            multiply(form_matrix, ctm),
            depth + 1,
        );
    }

    /// Показывает строку букв и сдвигает матрицу текста.
    fn show(&mut self, font: &Font, state: &State, text_matrix: &mut Matrix, bytes: &[u8]) {
        for code in font.codes(bytes) {
            let advance = font.width(code) * state.size;
            let mut tx = advance + state.char_spacing;
            if code == 32 && !font.two_bytes {
                tx += state.word_spacing;
            }
            tx *= state.scale;
            let m = multiply(*text_matrix, state.ctm);
            let (x, y) = apply(m, 0.0, state.rise);
            let (right, _) = apply(m, advance * state.scale, state.rise);
            let size = state.size * (m[2] * m[2] + m[3] * m[3]).sqrt();
            if !state.invisible {
                self.glyphs.push(Glyph {
                    text: font.text(code),
                    x: x.min(right),
                    right: x.max(right),
                    y,
                    size,
                });
            }
            *text_matrix = multiply([1.0, 0.0, 0.0, 1.0, tx, 0.0], *text_matrix);
        }
    }
}

/// Одна ли строка у двух букв: базовая линия меньшей буквы не дальше
/// чем на 0,6 размера большей вверх (верхний индекс) и на 0,35 вниз
/// (нижний индекс).
fn same_line(a: &Glyph, b: &Glyph) -> bool {
    let (big, small) = if a.size >= b.size { (a, b) } else { (b, a) };
    let up = small.y - big.y;
    (-0.35 * big.size..=0.6 * big.size).contains(&up)
}

/// Собирает буквы в строки (см. [`same_line`]). Базовая линия строки — та, на которой
/// стоит больше всего букв (индексы и сноски её не сдвигают).
fn group_lines(mut glyphs: Vec<Glyph>, page_height: f32) -> Vec<PdfLine> {
    glyphs.retain(|g| !g.text.is_empty() && g.size > 0.0);
    glyphs.sort_by(|a, b| b.y.total_cmp(&a.y));
    let mut groups: Vec<Vec<Glyph>> = Vec::new();
    for glyph in glyphs {
        match groups.last_mut() {
            Some(group) if group.iter().any(|g| same_line(g, &glyph)) => {
                group.push(glyph);
            }
            _ => groups.push(vec![glyph]),
        }
    }
    groups
        .into_iter()
        .filter_map(|mut group| {
            if group.iter().all(|g| g.text.trim().is_empty()) {
                return None;
            }
            let mut counts: HashMap<i32, usize> = HashMap::new();
            for g in &group {
                *counts.entry((g.y * 100.0).round() as i32).or_default() += 1;
            }
            let baseline = counts
                .into_iter()
                .max_by_key(|&(y, count)| (count, -y))
                .map_or(0.0, |(y, _)| y as f32 / 100.0);
            group.sort_by(|a, b| a.x.total_cmp(&b.x));
            let x = group
                .iter()
                .find(|g| !g.text.trim().is_empty())
                .map_or(0.0, |g| g.x);
            let mut text = String::new();
            let mut previous: Option<&Glyph> = None;
            for g in &group {
                if let Some(p) = previous
                    && g.x - p.right > 0.15 * g.size.min(p.size)
                    && !text.ends_with(char::is_whitespace)
                    && !g.text.starts_with(char::is_whitespace)
                {
                    text.push(' ');
                }
                text.push_str(&g.text);
                previous = Some(g);
            }
            Some(PdfLine {
                x,
                baseline: page_height - baseline,
                text: text.trim().to_string(),
            })
        })
        .collect()
}

/// Значение ключа страницы с учётом наследования от родителей.
fn inherited<'a>(doc: &'a Document, page: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    let mut node = page;
    for _ in 0..32 {
        if let Ok(value) = node.get(key) {
            return Some(value);
        }
        node = node.get(b"Parent").ok().and_then(|o| dict(doc, o))?;
    }
    None
}

fn media_box(doc: &Document, page: &Dictionary) -> [f32; 4] {
    let letter = [0.0, 0.0, 612.0, 792.0];
    let Some(values) = inherited(doc, page, b"MediaBox").and_then(|o| array(doc, o)) else {
        return letter;
    };
    let numbers: Vec<f32> = values.iter().filter_map(|o| number(doc, o)).collect();
    match numbers.as_slice() {
        [x0, y0, x1, y1] => [x0.min(*x1), y0.min(*y1), x0.max(*x1), y0.max(*y1)],
        _ => letter,
    }
}

fn matrix(operands: &[Object], doc: &Document) -> Option<Matrix> {
    let values: Vec<f32> = operands.iter().filter_map(|o| number(doc, o)).collect();
    values.try_into().ok()
}

fn number(doc: &Document, object: &Object) -> Option<f32> {
    match doc.dereference(object).ok()?.1 {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

fn array<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a [Object]> {
    doc.dereference(object)
        .ok()?
        .1
        .as_array()
        .ok()
        .map(Vec::as_slice)
}

fn dict<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Dictionary> {
    match doc.dereference(object).ok()?.1 {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmap_ranges_and_chars() {
        let data = b"2 beginbfchar\n<0003> <0020>\n<0011> <0041>\nendbfchar\n\
            1 beginbfrange\n<0020> <0022> <0430>\nendbfrange\n\
            1 beginbfrange\n<0030> <0031> [<0066006C> <0078>]\nendbfrange";
        let tokens = cmap_tokens(data);
        assert!(matches!(tokens[0], CmapToken::Word(b"beginbfchar")));
        let mut font = Dictionary::new();
        let doc = Document::with_version("1.5");
        let stream = lopdf::Stream::new(Dictionary::new(), data.to_vec());
        font.set("ToUnicode", Object::Stream(stream));
        let map = to_unicode(&doc, &font);
        assert_eq!(map[&0x03], " ");
        assert_eq!(map[&0x11], "A");
        assert_eq!(map[&0x21], "б");
        assert_eq!(map[&0x30], "fl");
        assert_eq!(map[&0x31], "x");
    }

    #[test]
    fn subscript_stays_on_its_line() {
        let glyph = |text: &str, x: f32, y: f32| Glyph {
            text: text.into(),
            x,
            right: x + 5.0,
            y,
            size: 11.0,
        };
        let lines = group_lines(
            vec![
                glyph("H", 72.0, 700.0),
                glyph("2", 77.0, 697.0),
                glyph("O", 82.0, 700.0),
                glyph("N", 72.0, 686.0),
            ],
            792.0,
        );
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "H2O");
        assert_eq!(lines[0].baseline, 92.0);
        assert_eq!(lines[1].text, "N");
    }

    #[test]
    fn broken_pdf_is_an_error() {
        assert!(read_pdf_lines(b"not a pdf").is_err());
    }
}
