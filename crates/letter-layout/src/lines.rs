//! Разбиение абзаца на строки.
//!
//! Шаги:
//!
//! 1. Текст абзаца собирается из кусков с разным оформлением, для всего
//!    абзаца считаются направления письма (Unicode Bidi), и каждый кусок
//!    превращается в глифы ([`Shaper`]).
//! 2. Глифы группируются в «кластеры» — неделимые части текста (буква
//!    с ударением, лигатура, табуляция, разрыв строки).
//! 3. Строки заполняются жадно, как в Word: слова добавляются, пока
//!    влезают; переносить строку можно только там, где разрешают правила
//!    Unicode (UAX #14). Пробелы в конце строки не считаются — они
//!    «висят» за правым краем. Слово длиннее строки режется по буквам.
//! 4. Каждая строка выравнивается (влево, вправо, по центру, по ширине),
//!    куски, которые пишутся справа налево, переставляются для показа.
//!
//! Все размеры — в пунктах, отсчёт по горизонтали — от левого поля
//! страницы (как позиции табуляций в Word).

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use letter_model::{ImageData, OBJECT_CHAR, Shape};

use unicode_bidi::{BidiInfo, Level};
use unicode_linebreak::{BreakOpportunity, linebreaks};

use crate::fonts::{FontLibrary, FontMatch};
use crate::shaping::{ShapedGlyph, ShapedRun, Shaper, TextStyle};

/// Допуск при сравнении ширин, пт: строка «влезает», если вылезает
/// меньше чем на эту величину.
const EPSILON: f32 = 0.01;

/// Кусок текста абзаца с одним оформлением. `'\t'` в тексте —
/// табуляция, `'\n'` — разрыв строки (Shift+Enter), `'\u{c}'` — разрыв
/// страницы (Ctrl+Enter), `'\u{b}'` — разрыв колонки.
#[derive(Debug, Clone, PartialEq)]
pub struct TextSpan {
    /// Текст.
    pub text: String,
    /// Оформление.
    pub style: TextStyle,
    /// Объект в строке (рисунок): тогда текст — один знак
    /// [`OBJECT_CHAR`], а место в строке задаёт объект.
    pub object: Option<InlineObject>,
    /// Надпись вместо текста (номер сноски): тогда текст — один знак
    /// [`OBJECT_CHAR`], а видна надпись — неделимо, как одна буква.
    pub label: Option<String>,
}

impl TextSpan {
    /// Кусок текста с заданным оформлением.
    pub fn new(text: impl Into<String>, style: TextStyle) -> TextSpan {
        TextSpan {
            text: text.into(),
            style,
            object: None,
            label: None,
        }
    }

    /// Надпись `label` (номер сноски), которая в тексте абзаца — один
    /// знак [`OBJECT_CHAR`].
    pub fn label(label: impl Into<String>, style: TextStyle) -> TextSpan {
        TextSpan {
            text: OBJECT_CHAR.to_string(),
            style,
            object: None,
            label: Some(label.into()),
        }
    }

    /// Объект в строке (рисунок); `style` — оформление знака, по нему
    /// считается глубина строки под базовой линией.
    pub fn object(object: InlineObject, style: TextStyle) -> TextSpan {
        TextSpan {
            text: OBJECT_CHAR.to_string(),
            style,
            object: Some(object),
            label: None,
        }
    }
}

/// Объект, который стоит в строке как большая буква: низ — на базовой
/// линии, ширина и высота заданы.
#[derive(Debug, Clone, PartialEq)]
pub struct InlineObject {
    /// Ширина, пт.
    pub width: f32,
    /// Высота, пт.
    pub height: f32,
    /// Картинка рисунка (`None` — рисовать заглушку).
    pub image: Option<Arc<ImageData>>,
    /// Фигура вместо картинки.
    pub shape: Option<Arc<Shape>>,
    /// Плавающий объект: в строке от него только знак привязки без
    /// ширины и высоты, сам объект ставит раскладка по страницам.
    pub floating: bool,
}

/// Выравнивание строк абзаца.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// По левому краю.
    #[default]
    Left,
    /// По центру.
    Center,
    /// По правому краю.
    Right,
    /// По ширине: пробелы растягиваются, кроме последней строки абзаца.
    Justify,
    /// Распределённое: растягиваются промежутки между всеми буквами,
    /// включая последнюю строку.
    Distribute,
}

/// Вид позиции табуляции.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabKind {
    /// Текст начинается от позиции.
    #[default]
    Left,
    /// Текст центрируется по позиции.
    Center,
    /// Текст заканчивается на позиции.
    Right,
    /// По позиции выравнивается десятичный разделитель.
    Decimal,
    /// Вертикальная черта: рисуется в позиции, но табуляцию не
    /// останавливает.
    Bar,
}

/// Заполнитель пустого места перед позицией табуляции.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabLeader {
    /// Пусто.
    #[default]
    None,
    /// Точки: `.......`.
    Dot,
    /// Дефисы: `-------`.
    Hyphen,
    /// Сплошная линия внизу: `_______`.
    Underscore,
    /// Толстая линия.
    Heavy,
    /// Точки по центру строки: `·······`.
    MiddleDot,
}

/// Позиция табуляции, заданная в абзаце.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TabStop {
    /// Расстояние от левого поля, пт.
    pub position: f32,
    /// Вид.
    pub kind: TabKind,
    /// Заполнитель.
    pub leader: TabLeader,
}

/// Оформление абзаца, нужное для разбиения на строки.
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphFormat {
    /// Ширина между полями страницы (или ячейки), пт.
    pub width: f32,
    /// Отступ слева, пт.
    pub left_indent: f32,
    /// Отступ справа, пт.
    pub right_indent: f32,
    /// Отступ первой строки, пт: положительный — красная строка,
    /// отрицательный — выступ.
    pub first_line_indent: f32,
    /// Выравнивание.
    pub align: Align,
    /// Позиции табуляции абзаца (порядок не важен).
    pub tab_stops: Vec<TabStop>,
    /// Шаг табуляции по умолчанию, пт (в Word — 36 пт, 1,25 см).
    pub default_tab: f32,
    /// Десятичный разделитель для табуляции «по разделителю».
    pub decimal_separator: char,
    /// Автоматическая расстановка переносов (`None` — выключена).
    pub hyphenation: Option<Hyphenation>,
}

/// Настройки автоматической расстановки переносов.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hyphenation {
    /// Ширина зоны переноса, пт: слово переносится, только если без
    /// переноса справа в строке осталось бы не меньше места.
    pub zone: f32,
    /// Не больше стольких строк подряд кончаются переносом; 0 — без
    /// ограничения.
    pub limit: u32,
    /// Переносить слова из прописных букв.
    pub caps: bool,
}

impl Default for Hyphenation {
    /// Как в Word: зона 0,25 дюйма, без ограничения, прописные
    /// переносятся.
    fn default() -> Hyphenation {
        Hyphenation {
            zone: 18.0,
            limit: 0,
            caps: true,
        }
    }
}

impl ParagraphFormat {
    /// Абзац заданной ширины без отступов, выровненный влево.
    pub fn new(width: f32) -> ParagraphFormat {
        ParagraphFormat {
            width,
            left_indent: 0.0,
            right_indent: 0.0,
            first_line_indent: 0.0,
            align: Align::Left,
            tab_stops: Vec::new(),
            default_tab: 36.0,
            decimal_separator: '.',
            hyphenation: None,
        }
    }

    /// Где начинается строка (первая или следующие).
    fn line_start(&self, first: bool) -> f32 {
        if first {
            self.left_indent + self.first_line_indent
        } else {
            self.left_indent
        }
    }

    /// Где кончается место для текста.
    fn line_end(&self) -> f32 {
        self.width - self.right_indent
    }
}

/// Чем кончается строка.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnd {
    /// Текст не влез, продолжение на следующей строке.
    Wrap,
    /// Разрыв строки (Shift+Enter).
    Break,
    /// Разрыв страницы (Ctrl+Enter): следующая строка — на новой странице.
    Page,
    /// Разрыв колонки (Ctrl+Shift+Enter): следующая строка — в следующей
    /// колонке (или на новой странице, если колонка последняя).
    Column,
    /// Конец абзаца.
    Paragraph,
}

/// Часть строки, готовая к рисованию.
#[derive(Debug, Clone, PartialEq)]
pub enum LineItem {
    /// Глифы одним шрифтом.
    Text(TextItem),
    /// Пустое место табуляции (возможно, с заполнителем).
    Tab(TabItem),
    /// Объект в строке (рисунок).
    Object(ObjectItem),
}

impl LineItem {
    /// Левый край части, пт от левого поля.
    pub fn x(&self) -> f32 {
        match self {
            LineItem::Text(t) => t.x,
            LineItem::Tab(t) => t.x,
            LineItem::Object(o) => o.x,
        }
    }

    /// Ширина части, пт.
    pub fn width(&self) -> f32 {
        match self {
            LineItem::Text(t) => t.run.width(),
            LineItem::Tab(t) => t.width,
            LineItem::Object(o) => o.width,
        }
    }
}

/// Глифы одним шрифтом на своём месте в строке.
#[derive(Debug, Clone, PartialEq)]
pub struct TextItem {
    /// Левый край, пт от левого поля.
    pub x: f32,
    /// Номер куска [`TextSpan`], откуда текст (для цвета, подчёркивания).
    pub span: usize,
    /// Глифы в порядке показа. При выравнивании по ширине растяжка уже
    /// добавлена к `x_advance` глифов.
    pub run: ShapedRun,
}

/// Табуляция на своём месте в строке.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabItem {
    /// Где знак табуляции в тексте абзаца, байты.
    pub at: usize,
    /// Левый край, пт от левого поля.
    pub x: f32,
    /// Ширина, пт.
    pub width: f32,
    /// Заполнитель.
    pub leader: TabLeader,
    /// Номер куска [`TextSpan`], где стоит табуляция.
    pub span: usize,
}

/// Объект (рисунок) на своём месте в строке. Низ объекта — на базовой
/// линии строки.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectItem {
    /// Где знак объекта в тексте абзаца, байты.
    pub at: usize,
    /// Левый край, пт от левого поля.
    pub x: f32,
    /// Ширина, пт.
    pub width: f32,
    /// Высота, пт (у знака привязки плавающего объекта — 0).
    pub height: f32,
    /// Номер куска [`TextSpan`] с объектом.
    pub span: usize,
    /// Картинка.
    pub image: Option<Arc<ImageData>>,
    /// Фигура вместо картинки.
    pub shape: Option<Arc<Shape>>,
}

/// Одна строка абзаца.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Какой кусок текста абзаца, в байтах (с пробелами в конце).
    pub range: Range<usize>,
    /// Левый край текста после выравнивания, пт от левого поля.
    pub x: f32,
    /// Ширина текста без пробелов в конце, пт.
    pub width: f32,
    /// Высота над базовой линией: самая большая у шрифтов строки, пт.
    pub ascent: f32,
    /// Глубина под базовой линией, пт.
    pub descent: f32,
    /// Дополнительный промежуток над строкой (внешний интерлиньяж), пт.
    pub leading: f32,
    /// Высота самого высокого рисунка в строке, пт (0 — рисунков нет).
    /// Рисунок стоит на базовой линии, поэтому `ascent` не меньше его.
    pub object_height: f32,
    /// Части строки в порядке показа слева направо.
    pub items: Vec<LineItem>,
    /// Чем кончается строка.
    pub end: LineEnd,
}

impl Line {
    /// Высота строки при одинарном интервале, пт: как в Word, высота
    /// шрифта по размерам Windows плюс внешний интерлиньяж.
    pub fn natural_height(&self) -> f32 {
        self.ascent + self.descent + self.leading
    }
}

/// Абзац, разбитый на строки.
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphLines {
    /// Весь текст абзаца (куски подряд).
    pub text: String,
    /// Строки сверху вниз. Всегда есть хотя бы одна.
    pub lines: Vec<Line>,
}

/// Вид кластера.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    /// Буквы из куска `runs[run]`; `space` — пробел, который может
    /// висеть в конце строки и растягиваться при выравнивании по ширине.
    Text { run: usize, space: bool },
    /// Табуляция.
    Tab,
    /// Разрыв строки или страницы.
    Break(LineEnd),
    /// Объект в строке (рисунок) из куска с этим номером.
    Object,
}

/// Неделимая часть текста.
#[derive(Debug, Clone)]
struct Cluster {
    range: Range<usize>,
    width: f32,
    span: usize,
    level: u8,
    kind: Kind,
}

impl Cluster {
    fn is_space(&self) -> bool {
        matches!(self.kind, Kind::Text { space: true, .. })
    }
}

/// Найденная позиция табуляции.
#[derive(Debug, Clone, Copy)]
struct Stop {
    position: f32,
    kind: TabKind,
    leader: TabLeader,
    /// Позиция по умолчанию (шаг табуляции), а не заданная в абзаце.
    default: bool,
}

/// Положение в строке с учётом табуляции, которая ещё ждёт текста:
/// для табуляции вправо, по центру и по разделителю её ширина зависит
/// от текста после неё.
#[derive(Debug, Clone, Copy)]
struct TabState {
    /// Где стоит последняя табуляция (или начало строки).
    origin: f32,
    /// Позиция последней табуляции.
    stop: Option<Stop>,
    /// Ширина текста после неё.
    segment: f32,
    /// Ширина текста до десятичного разделителя.
    before_separator: Option<f32>,
}

impl TabState {
    fn new(origin: f32) -> TabState {
        TabState {
            origin,
            stop: None,
            segment: 0.0,
            before_separator: None,
        }
    }

    /// Ширина последней табуляции.
    fn tab_width(&self) -> f32 {
        let Some(stop) = self.stop else {
            return 0.0;
        };
        let anchor = match stop.kind {
            TabKind::Left | TabKind::Bar => 0.0,
            TabKind::Center => self.segment / 2.0,
            TabKind::Right => self.segment,
            TabKind::Decimal => self.before_separator.unwrap_or(self.segment),
        };
        (stop.position - self.origin - anchor).max(0.0)
    }

    /// Где кончается текст.
    fn x(&self) -> f32 {
        self.origin + self.tab_width() + self.segment
    }

    fn add_text(&mut self, width: f32, has_separator: bool) {
        if has_separator && self.before_separator.is_none() {
            self.before_separator = Some(self.segment);
        }
        self.segment += width;
    }

    /// Новая табуляция: прошлая получает окончательную ширину.
    fn start_tab(&mut self, stop: Stop) {
        *self = TabState {
            origin: self.x(),
            stop: Some(stop),
            segment: 0.0,
            before_separator: None,
        };
    }
}

/// Разбивает абзац на строки. `mark_style` — оформление знака абзаца:
/// по нему считается высота пустой строки.
pub fn break_lines(
    shaper: &mut Shaper,
    library: &FontLibrary,
    spans: &[TextSpan],
    mark_style: &TextStyle,
    format: &ParagraphFormat,
) -> ParagraphLines {
    break_lines_around(shaper, library, spans, mark_style, format, &mut |_| None)
}

/// Свободное место для строки рядом с плавающим рисунком: края, пт от
/// левого края области абзаца (как [`Line::x`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineRoom {
    /// Левый край.
    pub left: f32,
    /// Правый край.
    pub right: f32,
}

/// Как [`break_lines`], но место для каждой строки может быть уже:
/// перед каждой строкой вызывается `room` с уже готовыми строками
/// и возвращает место для следующей (`None` — вся ширина).
pub fn break_lines_around(
    shaper: &mut Shaper,
    library: &FontLibrary,
    spans: &[TextSpan],
    mark_style: &TextStyle,
    format: &ParagraphFormat,
    room: &mut dyn FnMut(&[Line]) -> Option<LineRoom>,
) -> ParagraphLines {
    let mut builder = Builder::new(shaper, library, spans, format);
    let mut lines = Vec::new();
    let mut start = 0;
    let mut first = true;
    loop {
        builder.room = room(&lines);
        let (end, kind) = builder.fit(start, first);
        lines.push(builder.place(start, end, first, kind, mark_style));
        start = end;
        first = false;
        let finished = start >= builder.clusters.len();
        // После разрыва в самом конце абзаца — ещё одна пустая строка.
        if finished && !matches!(kind, LineEnd::Break | LineEnd::Page | LineEnd::Column) {
            break;
        }
        if finished {
            builder.room = room(&lines);
            lines.push(builder.place(start, start, first, LineEnd::Paragraph, mark_style));
            break;
        }
    }
    ParagraphLines {
        text: builder.text,
        lines,
    }
}

struct Builder<'a> {
    shaper: &'a mut Shaper,
    library: &'a FontLibrary,
    format: &'a ParagraphFormat,
    text: String,
    runs: Vec<ShapedRun>,
    clusters: Vec<Cluster>,
    /// Можно ли перенести строку после байта с этим номером.
    breaks: Vec<bool>,
    /// Объекты кусков (`None` у обычного текста).
    objects: Vec<Option<InlineObject>>,
    /// Заданные позиции табуляции по возрастанию, без черт.
    stops: Vec<TabStop>,
    /// Место для текущей строки, если его сужает плавающий рисунок.
    room: Option<LineRoom>,
    /// Дефисы переноса по номерам кластеров, после которых можно
    /// перенести строку с дефисом: мягкий перенос (U+00AD) или место
    /// автоматического переноса в слове. Дефис виден, только если строка
    /// перенесена на этом месте.
    hyphens: HashMap<usize, Hyphen>,
    /// Где в тексте можно автоматически перенести слово, байты.
    auto_points: Vec<bool>,
    /// Сколько строк подряд перед текущей кончаются переносом.
    hyphen_streak: u32,
}

/// Дефис переноса после кластера.
#[derive(Debug, Clone, Copy)]
struct Hyphen {
    glyph: ShapedGlyph,
    /// Место автоматического переноса (а не мягкий перенос).
    auto: bool,
}

impl<'a> Builder<'a> {
    fn new(
        shaper: &'a mut Shaper,
        library: &'a FontLibrary,
        spans: &[TextSpan],
        format: &'a ParagraphFormat,
    ) -> Builder<'a> {
        let text: String = spans.iter().map(|s| s.text.as_str()).collect();
        let mut breaks = vec![false; text.len() + 1];
        for (at, opportunity) in linebreaks(&text) {
            if at < text.len() || opportunity == BreakOpportunity::Allowed {
                breaks[at] = true;
            }
        }
        let auto_points = match &format.hyphenation {
            Some(h) => hyphenation_points(&text, h.caps),
            None => Vec::new(),
        };
        let mut stops: Vec<TabStop> = format
            .tab_stops
            .iter()
            .filter(|s| s.kind != TabKind::Bar && s.position.is_finite())
            .copied()
            .collect();
        stops.sort_by(|a, b| a.position.total_cmp(&b.position));
        let mut builder = Builder {
            shaper,
            library,
            format,
            text,
            runs: Vec::new(),
            clusters: Vec::new(),
            breaks,
            stops,
            objects: spans.iter().map(|s| s.object.clone()).collect(),
            room: None,
            hyphens: HashMap::new(),
            auto_points,
            hyphen_streak: 0,
        };
        builder.shape(spans);
        builder
    }

    /// Превращает текст в кластеры.
    fn shape(&mut self, spans: &[TextSpan]) {
        let levels = BidiInfo::new(&self.text, Some(Level::ltr())).levels;
        let mut offset = 0;
        for (index, span) in spans.iter().enumerate() {
            let span_range = offset..offset + span.text.len();
            offset = span_range.end;
            if let Some(object) = &span.object {
                // Объект — один неделимый кластер на весь кусок.
                if !span_range.is_empty() {
                    self.clusters.push(Cluster {
                        level: levels[span_range.start].number(),
                        range: span_range,
                        width: if object.floating { 0.0 } else { object.width },
                        span: index,
                        kind: Kind::Object,
                    });
                }
                continue;
            }
            if let Some(label) = &span.label {
                let level = levels[span_range.start].number();
                self.shape_label(span_range, level, index, label, span);
                continue;
            }
            // Куски одного направления между табуляциями и разрывами.
            let mut piece_start = span_range.start;
            for (i, c) in span.text.char_indices() {
                let at = span_range.start + i;
                let special = match c {
                    '\t' => Some(Kind::Tab),
                    '\n' => Some(Kind::Break(LineEnd::Break)),
                    '\u{c}' => Some(Kind::Break(LineEnd::Page)),
                    '\u{b}' => Some(Kind::Break(LineEnd::Column)),
                    _ => None,
                };
                let level_changes = at > piece_start && levels[at] != levels[at - 1];
                if special.is_some() || level_changes {
                    self.shape_piece(piece_start..at, levels[piece_start].number(), index, span);
                    piece_start = at;
                }
                if let Some(kind) = special {
                    self.clusters.push(Cluster {
                        range: at..at + 1,
                        width: 0.0,
                        span: index,
                        level: 0,
                        kind,
                    });
                    piece_start = at + 1;
                }
            }
            if piece_start < span_range.end {
                self.shape_piece(
                    piece_start..span_range.end,
                    levels[piece_start].number(),
                    index,
                    span,
                );
            }
        }
    }

    /// Надпись куска `span` вместо его знака `range`: один неделимый
    /// кластер с глифами надписи.
    fn shape_label(
        &mut self,
        range: Range<usize>,
        level: u8,
        span: usize,
        label: &str,
        source: &TextSpan,
    ) {
        if range.is_empty() {
            return;
        }
        let runs =
            self.shaper
                .shape_range(self.library, label, 0..label.len(), level, &source.style);
        let mut runs = runs.into_iter();
        let Some(mut run) = runs.next() else {
            return;
        };
        for extra in runs {
            run.glyphs.extend(extra.glyphs);
        }
        for glyph in &mut run.glyphs {
            glyph.cluster = range.start;
        }
        run.range = range.clone();
        let width = run.width();
        let index = self.runs.len();
        self.runs.push(run);
        self.clusters.push(Cluster {
            range,
            width,
            span,
            level,
            kind: Kind::Text {
                run: index,
                space: false,
            },
        });
    }

    fn shape_piece(&mut self, range: Range<usize>, level: u8, span: usize, source: &TextSpan) {
        if range.is_empty() {
            return;
        }
        let runs = self
            .shaper
            .shape_range(self.library, &self.text, range, level, &source.style);
        for run in runs {
            let mut starts: Vec<usize> = run.glyphs.iter().map(|g| g.cluster).collect();
            starts.sort_unstable();
            starts.dedup();
            let Some(first) = starts.first_mut() else {
                continue;
            };
            *first = run.range.start;
            let index = self.runs.len();
            for (k, &start) in starts.iter().enumerate() {
                let end = starts.get(k + 1).copied().unwrap_or(run.range.end);
                let range = start..end;
                let width = run
                    .glyphs
                    .iter()
                    .filter(|g| range.contains(&g.cluster) || (k == 0 && g.cluster < start))
                    .map(|g| g.x_advance)
                    .sum();
                let space = self.text[range.clone()].chars().all(is_hanging_space);
                let soft = self.text[range.clone()].ends_with(SOFT_HYPHEN);
                let auto = !soft && self.auto_points.get(range.end).copied().unwrap_or(false);
                if (soft || auto)
                    && run.level % 2 == 0
                    && let Some(glyph) = self.hyphen_glyph(&run, &source.style)
                {
                    let cluster = range.start;
                    let glyph = ShapedGlyph { cluster, ..glyph };
                    self.hyphens
                        .insert(self.clusters.len(), Hyphen { glyph, auto });
                }
                self.clusters.push(Cluster {
                    range,
                    width,
                    span,
                    level: run.level,
                    kind: Kind::Text { run: index, space },
                });
            }
            self.runs.push(run);
        }
    }

    /// Глиф дефиса тем же шрифтом, что и кусок `run` (если кусок набран
    /// запасным шрифтом, например кириллица — шрифтом с кириллицей, то
    /// и дефис им же).
    fn hyphen_glyph(&mut self, run: &ShapedRun, style: &TextStyle) -> Option<ShapedGlyph> {
        let fallback = TextStyle {
            family: run.font.family.clone(),
            ..style.clone()
        };
        for style in [style, &fallback] {
            let runs = self.shaper.shape_range(self.library, "-", 0..1, 0, style);
            if let [hyphen] = runs.as_slice()
                && hyphen.font.id == run.font.id
                && hyphen.glyphs.len() == 1
            {
                return hyphen.glyphs.first().copied();
            }
        }
        None
    }

    /// Ширина дефиса, который появится при переносе после кластера `i`.
    fn hyphen_width(&self, i: usize) -> f32 {
        self.hyphens.get(&i).map_or(0.0, |h| h.glyph.x_advance)
    }

    /// Следующая позиция табуляции после `x`.
    fn next_stop(&self, x: f32, first: bool) -> Stop {
        let format = self.format;
        let custom = self
            .stops
            .iter()
            .find(|s| s.position > x + EPSILON)
            .map(|s| Stop {
                position: s.position,
                kind: s.kind,
                leader: s.leader,
                default: false,
            });
        // На первой строке с выступом есть скрытая позиция на отступе.
        let hanging = (first && format.first_line_indent < 0.0 && format.left_indent > x + EPSILON)
            .then_some(Stop {
                position: format.left_indent,
                kind: TabKind::Left,
                leader: TabLeader::None,
                default: false,
            });
        let found = match (custom, hanging) {
            (Some(c), Some(h)) if h.position < c.position => Some(h),
            (Some(c), _) => Some(c),
            (None, h) => h,
        };
        if let Some(stop) = found {
            return stop;
        }
        // Шаг по умолчанию — только правее последней заданной позиции.
        let step = if format.default_tab > EPSILON {
            format.default_tab
        } else {
            36.0
        };
        let from = self.stops.last().map_or(x, |s| s.position.max(x));
        Stop {
            position: ((from + EPSILON) / step).floor() * step + step,
            kind: TabKind::Left,
            leader: TabLeader::None,
            default: true,
        }
    }

    /// Где начинается текущая строка с учётом [`Builder::room`].
    fn line_start(&self, first: bool) -> f32 {
        let start = self.format.line_start(first);
        self.room.map_or(start, |r| start.max(r.left))
    }

    /// Где кончается место для текста текущей строки.
    fn line_end(&self) -> f32 {
        let end = self.format.line_end();
        self.room.map_or(end, |r| end.min(r.right))
    }

    /// Ищет, где кончится строка, начатая с кластера `start`.
    fn fit(&mut self, start: usize, first: bool) -> (usize, LineEnd) {
        let line_end = self.line_end();
        let mut state = TabState::new(self.line_start(first));
        // Последнее место переноса и последнее — без автоматического
        // переноса слова (с местом, где кончается текст).
        let mut last_break = None;
        let mut last_plain: Option<(usize, f32)> = None;
        let mut has_content = false;
        let hyphenation = self.format.hyphenation;
        let auto_allowed =
            hyphenation.is_some_and(|h| h.limit == 0 || self.hyphen_streak < h.limit);
        // Где перенести строку, если кластер `i` не влез. Слово
        // переносится, только если без этого справа осталось бы не
        // меньше зоны переноса.
        let wrap_at = |i: usize, last_break: Option<usize>, last_plain: Option<(usize, f32)>| {
            let end = last_break.filter(|&b| b > start);
            let plain = last_plain.filter(|&(b, _)| b > start);
            match (end, plain, hyphenation) {
                (Some(b), Some((p, x)), Some(h)) if b != p && line_end - x < h.zone => p,
                _ => end.unwrap_or(i),
            }
        };
        for (i, cluster) in self.clusters.iter().enumerate().skip(start) {
            match cluster.kind {
                Kind::Break(end) => return (i + 1, end),
                Kind::Tab => {
                    let stop = self.next_stop(state.x(), first);
                    // Табуляция по умолчанию за правым краем переносит
                    // текст на следующую строку.
                    if stop.default && stop.position > line_end + EPSILON && i > start {
                        return (i, LineEnd::Wrap);
                    }
                    state.start_tab(stop);
                    // Табуляция, упёршаяся в правый край, — не текст:
                    // после неё не переносим.
                    has_content |= !stop.default || stop.position <= line_end + EPSILON;
                }
                Kind::Object => {
                    state.add_text(cluster.width, false);
                    if has_content && state.x() > line_end + EPSILON {
                        return (wrap_at(i, last_break, last_plain), LineEnd::Wrap);
                    }
                    has_content = true;
                }
                Kind::Text { space, .. } => {
                    let separator =
                        self.text[cluster.range.clone()].contains(self.format.decimal_separator);
                    state.add_text(cluster.width, separator);
                    if !space {
                        if has_content && state.x() > line_end + EPSILON {
                            return (wrap_at(i, last_break, last_plain), LineEnd::Wrap);
                        }
                        has_content = true;
                    }
                }
            }
            // На переносе с дефисом строку можно разорвать, только если
            // влезет и появившийся дефис.
            let hyphen = self.hyphens.get(&i);
            let auto = hyphen.is_some_and(|h| h.auto);
            if (self.breaks[cluster.range.end] || (auto && auto_allowed))
                && hyphen.is_none_or(|h| state.x() + h.glyph.x_advance <= line_end + EPSILON)
            {
                last_break = Some(i + 1);
                if !auto {
                    last_plain = Some((i + 1, state.x()));
                }
            }
        }
        (self.clusters.len(), LineEnd::Paragraph)
    }

    /// Расставляет кластеры `start..end` по местам.
    fn place(
        &mut self,
        start: usize,
        end: usize,
        first: bool,
        kind: LineEnd,
        mark_style: &TextStyle,
    ) -> Line {
        let format = self.format;
        let line_start = self.line_start(first);
        let line_end = self.line_end();
        let clusters = &self.clusters[start..end];
        // Пробелы и разрыв в конце строки не считаются в ширину.
        let content_end = clusters
            .iter()
            .rposition(|c| !c.is_space() && !matches!(c.kind, Kind::Break(_)))
            .map_or(0, |i| i + 1);

        // Ширины табуляций.
        let mut widths: Vec<f32> = clusters.iter().map(|c| c.width).collect();
        // Перенос на мягком переносе: в конце строки виден дефис.
        let hyphen = (kind == LineEnd::Wrap && content_end > 0)
            .then(|| content_end - 1)
            .filter(|&i| self.hyphens.contains_key(&(start + i)));
        if let Some(i) = hyphen {
            widths[i] += self.hyphen_width(start + i);
        }
        self.hyphen_streak = if hyphen.is_some() {
            self.hyphen_streak + 1
        } else {
            0
        };
        let mut state = TabState::new(line_start);
        let mut leaders = vec![TabLeader::None; clusters.len()];
        let mut pending: Option<usize> = None;
        for (i, cluster) in clusters[..content_end].iter().enumerate() {
            match cluster.kind {
                Kind::Tab => {
                    if let Some(p) = pending {
                        widths[p] = state.tab_width();
                    }
                    let stop = self.next_stop(state.x(), first);
                    // Табуляция не уходит за правый край, если это не
                    // заданная позиция.
                    let stop = if stop.default {
                        Stop {
                            position: stop.position.min(line_end.max(state.x())),
                            ..stop
                        }
                    } else {
                        stop
                    };
                    state.start_tab(stop);
                    leaders[i] = stop.leader;
                    pending = Some(i);
                }
                Kind::Text { .. } => state.add_text(
                    cluster.width,
                    self.text[cluster.range.clone()].contains(format.decimal_separator),
                ),
                Kind::Object => state.add_text(cluster.width, false),
                Kind::Break(_) => {}
            }
        }
        if let Some(p) = pending {
            widths[p] = state.tab_width();
        }

        // Выравнивание.
        let content_width: f32 = widths[..content_end].iter().sum();
        let extra = line_end - line_start - content_width;
        let after_tab = clusters[..content_end]
            .iter()
            .rposition(|c| c.kind == Kind::Tab)
            .map_or(0, |i| i + 1);
        let mut offset = 0.0;
        match format.align {
            Align::Left => {}
            Align::Center => offset = (extra / 2.0).max(0.0),
            Align::Right => offset = extra.max(0.0),
            Align::Justify if kind != LineEnd::Paragraph && extra > 0.0 => {
                let spaces: Vec<usize> = (after_tab..content_end)
                    .filter(|&i| clusters[i].is_space())
                    .collect();
                for &i in &spaces {
                    widths[i] += extra / spaces.len() as f32;
                }
            }
            Align::Distribute if extra > 0.0 => {
                let gaps: Vec<usize> = (after_tab..content_end.saturating_sub(1))
                    .filter(|&i| matches!(clusters[i].kind, Kind::Text { .. } | Kind::Object))
                    .collect();
                for &i in &gaps {
                    widths[i] += extra / gaps.len() as f32;
                }
            }
            Align::Justify | Align::Distribute => {}
        }
        let stretched: Vec<f32> = widths
            .iter()
            .zip(clusters)
            .map(|(w, c)| w - c.width)
            .collect();

        // Порядок показа.
        let mut levels: Vec<u8> = clusters.iter().map(|c| c.level).collect();
        for (i, level) in levels.iter_mut().enumerate() {
            if i >= content_end {
                *level = 0;
            }
        }
        // Пробелы перед табуляцией — на уровне абзаца (правило L1).
        for i in (0..content_end).rev() {
            if clusters[i].kind == Kind::Tab {
                let mut j = i;
                while j > 0 && clusters[j - 1].is_space() {
                    j -= 1;
                    levels[j] = 0;
                }
            }
        }
        let order = visual_order(&levels);

        // Части строки.
        let mut items: Vec<LineItem> = Vec::new();
        let mut group: Vec<usize> = Vec::new();
        let mut x = line_start + offset;
        let mut group_x = x;
        for &i in &order {
            let cluster = &clusters[i];
            let same_run = match (cluster.kind, group.first().map(|&g| clusters[g].kind)) {
                (Kind::Text { run: a, .. }, Some(Kind::Text { run: b, .. })) => a == b,
                _ => false,
            };
            if !same_run && !group.is_empty() {
                items.extend(self.text_item(group_x, start, &group, &stretched, hyphen));
                group.clear();
            }
            match cluster.kind {
                Kind::Text { .. } => {
                    if group.is_empty() {
                        group_x = x;
                    }
                    group.push(i);
                }
                Kind::Tab => {
                    items.push(LineItem::Tab(TabItem {
                        at: cluster.range.start,
                        x,
                        width: widths[i],
                        leader: leaders[i],
                        span: cluster.span,
                    }));
                }
                Kind::Object => {
                    let object = self
                        .objects
                        .get(cluster.span)
                        .cloned()
                        .flatten()
                        .filter(|o| !o.floating);
                    items.push(LineItem::Object(ObjectItem {
                        at: cluster.range.start,
                        x,
                        width: cluster.width,
                        height: object.as_ref().map_or(0.0, |o| o.height),
                        span: cluster.span,
                        image: object.as_ref().and_then(|o| o.image.clone()),
                        shape: object.and_then(|o| o.shape),
                    }));
                }
                Kind::Break(_) => {}
            }
            x += widths[i];
        }
        if !group.is_empty() {
            items.extend(self.text_item(group_x, start, &group, &stretched, hyphen));
        }

        let range = match (clusters.first(), clusters.last()) {
            (Some(a), Some(b)) => a.range.start..b.range.end,
            _ => self.text.len()..self.text.len(),
        };
        let (ascent, descent, leading) = self.line_height(start..end, mark_style);
        // Рисунок стоит на базовой линии и поднимает строку.
        let object_height = items
            .iter()
            .filter_map(|item| match item {
                LineItem::Object(object) => Some(object.height),
                _ => None,
            })
            .fold(0.0, f32::max);
        Line {
            range,
            x: line_start + offset,
            width: widths[..content_end].iter().sum(),
            ascent: ascent.max(object_height),
            descent,
            leading,
            object_height,
            items,
            end: kind,
        }
    }

    /// Глифы кластеров `group` (подряд в порядке показа, из одного куска;
    /// номера — от кластера `start`). `hyphen` — кластер, после которого
    /// дорисовывается дефис мягкого переноса.
    fn text_item(
        &self,
        x: f32,
        start: usize,
        group: &[usize],
        stretched: &[f32],
        hyphen: Option<usize>,
    ) -> Option<LineItem> {
        let clusters = &self.clusters[start..];
        let first = clusters.get(*group.first()?)?;
        let Kind::Text { run, .. } = first.kind else {
            return None;
        };
        let source = self.runs.get(run)?;
        let mut glyphs = Vec::new();
        let from = group.iter().map(|&i| clusters[i].range.start).min()?;
        let end = group.iter().map(|&i| clusters[i].range.end).max()?;
        for glyph in &source.glyphs {
            let owner = group
                .iter()
                .find(|&&i| clusters[i].range.contains(&glyph.cluster))
                .copied();
            if owner.is_some() {
                glyphs.push((owner, *glyph));
            }
        }
        if let Some(i) = hyphen.filter(|i| group.contains(i))
            && let Some(hyphen) = self.hyphens.get(&(start + i))
        {
            glyphs.push((Some(i), hyphen.glyph));
        }
        // Растяжку получает последний глиф кластера.
        for &i in group {
            if stretched[i] != 0.0
                && let Some((_, glyph)) = glyphs.iter_mut().rev().find(|(o, _)| *o == Some(i))
            {
                glyph.x_advance += stretched[i];
            }
        }
        Some(LineItem::Text(TextItem {
            x,
            span: first.span,
            run: ShapedRun {
                range: from..end,
                font: source.font.clone(),
                size: source.size,
                level: source.level,
                glyphs: glyphs.into_iter().map(|(_, g)| g).collect(),
                width_scale: source.width_scale,
                hidden: source.hidden,
            },
        }))
    }

    /// Высота строки по шрифтам её букв (по размерам Windows, как в Word).
    fn line_height(&mut self, clusters: Range<usize>, mark_style: &TextStyle) -> (f32, f32, f32) {
        let mut fonts: Vec<(&FontMatch, f32)> = Vec::new();
        for cluster in &self.clusters[clusters] {
            if let Kind::Text { run, .. } = cluster.kind
                && let Some(r) = self.runs.get(run)
                && !r.hidden
                && !fonts.iter().any(|(f, s)| f.id == r.font.id && *s == r.size)
            {
                fonts.push((&r.font, r.size));
            }
        }
        let mark = self
            .library
            .find(&mark_style.family, mark_style.bold, mark_style.italic);
        if fonts.is_empty()
            && let Some(m) = &mark
        {
            fonts.push((m, mark_style.size));
        }
        let mut sizes = Vec::new();
        for (font, size) in fonts {
            if let Some(m) = self.shaper.metrics(self.library, font.id) {
                sizes.push((
                    m.win_ascent * size,
                    m.win_descent * size,
                    m.external_leading * size,
                ));
            }
        }
        sizes
            .into_iter()
            .fold((0.0, 0.0, 0.0), |(a, d, l), (a2, d2, l2)| {
                (a.max(a2), d.max(d2), l.max(l2))
            })
    }
}

/// Пробел, который может висеть в конце строки. Неразрывные пробелы
/// ведут себя как буквы.
/// Места автоматического переноса в тексте (байты): между слогами слов
/// русского и английского языка. Слова с мягким переносом не трогаются,
/// как в Word; слова из прописных букв — только если `caps`.
fn hyphenation_points(text: &str, caps: bool) -> Vec<bool> {
    let mut points = vec![false; text.len() + 1];
    // Слова — куски из букв; слово, в котором есть мягкий перенос, не
    // переносится автоматически.
    let mut words: Vec<(usize, usize, bool)> = Vec::new();
    for (at, c) in text.char_indices() {
        let end = at + c.len_utf8();
        match words.last_mut() {
            Some((_, e, soft)) if *e == at && (c.is_alphabetic() || c == SOFT_HYPHEN) => {
                *e = end;
                *soft |= c == SOFT_HYPHEN;
            }
            _ if c.is_alphabetic() => words.push((at, end, false)),
            _ => {}
        }
    }
    for (from, to, soft) in words {
        let word = &text[from..to];
        if soft || (!caps && word.chars().all(|c| !c.is_lowercase())) {
            continue;
        }
        let lang = if word.chars().any(|c| matches!(c, '\u{400}'..='\u{4ff}')) {
            hypher::Lang::Russian
        } else if word.chars().all(|c| c.is_ascii_alphabetic()) {
            hypher::Lang::English
        } else {
            continue;
        };
        let mut offset = from;
        let mut syllables = hypher::hyphenate(word, lang).peekable();
        while let Some(syllable) = syllables.next() {
            offset += syllable.len();
            if syllables.peek().is_some() {
                points[offset] = true;
            }
        }
    }
    points
}

/// Мягкий перенос: невидим, но строку можно перенести на нём с дефисом.
const SOFT_HYPHEN: char = '\u{ad}';

fn is_hanging_space(c: char) -> bool {
    c.is_whitespace()
        && !matches!(
            c,
            '\u{a0}' | '\u{202f}' | '\u{2007}' | '\t' | '\n' | '\u{c}' | '\u{b}'
        )
}

/// Порядок показа элементов с уровнями Bidi (правило L2): куски
/// с нечётным уровнем переворачиваются, от самого высокого уровня
/// к самому низкому нечётному.
fn visual_order(levels: &[u8]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..levels.len()).collect();
    let Some(&highest) = levels.iter().max() else {
        return order;
    };
    let lowest_odd = levels
        .iter()
        .copied()
        .filter(|l| l % 2 == 1)
        .min()
        .unwrap_or(highest + 1);
    let mut level = highest;
    while level >= lowest_odd && level > 0 {
        let mut i = 0;
        while i < order.len() {
            if levels[order[i]] >= level {
                let start = i;
                while i < order.len() && levels[order[i]] >= level {
                    i += 1;
                }
                order[start..i].reverse();
            } else {
                i += 1;
            }
        }
        level -= 1;
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ширина буквы Liberation Mono в 10 пт.
    const MONO: f32 = 1229.0 / 2048.0 * 10.0;

    fn mono() -> TextStyle {
        TextStyle::new("Liberation Mono", 10.0)
    }

    fn lay(spans: &[TextSpan], format: &ParagraphFormat) -> ParagraphLines {
        let library = FontLibrary::embedded_only();
        let mut shaper = Shaper::new();
        break_lines(&mut shaper, &library, spans, &mono(), format)
    }

    fn lay_text(text: &str, format: &ParagraphFormat) -> ParagraphLines {
        lay(&[TextSpan::new(text, mono())], format)
    }

    fn line_texts(p: &ParagraphLines) -> Vec<&str> {
        p.lines.iter().map(|l| &p.text[l.range.clone()]).collect()
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn words_wrap_and_spaces_hang() {
        // Влезает 10 букв.
        let format = ParagraphFormat::new(MONO * 10.0 + 0.001);
        let p = lay_text("one two three fours", &format);
        assert_eq!(line_texts(&p), ["one two ", "three ", "fours"]);
        let ends: Vec<_> = p.lines.iter().map(|l| l.end).collect();
        assert_eq!(ends, [LineEnd::Wrap, LineEnd::Wrap, LineEnd::Paragraph]);
        // Пробел в конце строки не считается в ширину.
        assert!(close(p.lines[0].width, MONO * 7.0));
        // Пробел-хвост в «three » не заставил перенести «three».
        let q = lay_text("three four", &ParagraphFormat::new(MONO * 5.0 + 0.001));
        assert_eq!(line_texts(&q), ["three ", "four"]);
        let ascent = p.lines[0].ascent;
        assert!(ascent > 8.0 && ascent < 10.0, "{ascent}");
    }

    #[test]
    fn label_takes_its_width() {
        let spans = [
            TextSpan::new("ab", mono()),
            TextSpan::label("12", mono()),
            TextSpan::new("c", mono()),
        ];
        let p = lay(&spans, &ParagraphFormat::new(MONO * 20.0));
        let line = &p.lines[0];
        // Надпись из двух знаков стоит на месте одного знака текста.
        assert_eq!(p.text, format!("ab{OBJECT_CHAR}c"));
        assert!(close(line.width, MONO * 5.0), "{}", line.width);
        let label = line
            .items
            .iter()
            .find_map(|item| match item {
                LineItem::Text(t) if t.span == 1 => Some(t),
                _ => None,
            })
            .expect("нет надписи");
        assert_eq!(label.run.range, 2..2 + OBJECT_CHAR.len_utf8());
        assert_eq!(label.run.glyphs.len(), 2);
    }

    #[test]
    fn long_word_is_cut() {
        let p = lay_text("abcdefgh", &ParagraphFormat::new(MONO * 3.0 + 0.001));
        assert_eq!(line_texts(&p), ["abc", "def", "gh"]);
        // Даже если не влезает ни одной буквы, строка не пустая.
        let p = lay_text("ab", &ParagraphFormat::new(1.0));
        assert_eq!(line_texts(&p), ["a", "b"]);
    }

    #[test]
    fn hyphen_allows_a_break() {
        let p = lay_text("well-known", &ParagraphFormat::new(MONO * 7.0));
        assert_eq!(line_texts(&p), ["well-", "known"]);
        // Неразрывный пробел не даёт переноса.
        let p = lay_text("ab\u{a0}cd", &ParagraphFormat::new(MONO * 3.0 + 0.001));
        assert_eq!(line_texts(&p), ["ab\u{a0}", "cd"]);
    }

    /// Видимые глифы строки.
    fn visible_glyphs(line: &Line) -> usize {
        line.items
            .iter()
            .map(|item| match item {
                LineItem::Text(t) => t.run.glyphs.iter().filter(|g| g.x_advance > 0.0).count(),
                _ => 0,
            })
            .sum()
    }

    /// Самое длинное начало слова `word` до места переноса, которое
    /// с дефисом занимает не больше `room` знаков.
    fn best_prefix(word: &str, lang: hypher::Lang, room: usize) -> String {
        let mut prefix = String::new();
        let mut best = String::new();
        let syllables: Vec<&str> = hypher::hyphenate(word, lang).collect();
        for syllable in &syllables[..syllables.len() - 1] {
            prefix.push_str(syllable);
            if prefix.chars().count() < room {
                best = prefix.clone();
            }
        }
        best
    }

    #[test]
    fn words_are_hyphenated_automatically() {
        let mut format = ParagraphFormat::new(MONO * 10.0 + 0.001);
        format.hyphenation = Some(Hyphenation {
            zone: 0.0,
            ..Hyphenation::default()
        });
        let p = lay_text("aa hyphenation", &format);
        let prefix = best_prefix("hyphenation", hypher::Lang::English, 10 - 3);
        assert!(!prefix.is_empty());
        let first = format!("aa {prefix}");
        assert_eq!(line_texts(&p)[0], first);
        // Дефис виден в конце строки и считается в ширину.
        assert_eq!(visible_glyphs(&p.lines[0]), first.chars().count() + 1);
        assert!(close(
            p.lines[0].width,
            MONO * (first.chars().count() + 1) as f32
        ));
        // Русский.
        let p = lay_text("ну переносы", &format);
        let prefix = best_prefix("переносы", hypher::Lang::Russian, 10 - 3);
        assert_eq!(line_texts(&p)[0], format!("ну {prefix}"));
        // Широкая зона: перенос не нужен, слово целиком на новой строке.
        format.hyphenation = Some(Hyphenation {
            zone: MONO * 20.0,
            ..Hyphenation::default()
        });
        let p = lay_text("aa hyphenation", &format);
        assert_eq!(line_texts(&p)[0], "aa ");
        // Без настройки переносов нет.
        let p = lay_text("aa hyphenation", &ParagraphFormat::new(MONO * 10.0 + 0.001));
        assert_eq!(line_texts(&p)[0], "aa ");
        // Слова из прописных — только если разрешено.
        format.hyphenation = Some(Hyphenation {
            zone: 0.0,
            caps: false,
            ..Hyphenation::default()
        });
        let p = lay_text("aa HYPHENATION", &format);
        assert_eq!(line_texts(&p)[0], "aa ");
    }

    #[test]
    fn consecutive_hyphens_are_limited() {
        let text = "hyphenation ".repeat(12);
        let mut format = ParagraphFormat::new(MONO * 16.0 + 0.001);
        format.hyphenation = Some(Hyphenation {
            zone: 0.0,
            ..Hyphenation::default()
        });
        let ends = |p: &ParagraphLines| -> Vec<bool> {
            p.lines
                .iter()
                .map(|l| {
                    let t = &p.text[l.range.clone()];
                    !t.ends_with(' ') && l.end == LineEnd::Wrap
                })
                .collect()
        };
        let free = ends(&lay_text(&text, &format));
        assert!(free.windows(2).any(|w| w[0] && w[1]), "{free:?}");
        format.hyphenation = Some(Hyphenation {
            zone: 0.0,
            limit: 1,
            ..Hyphenation::default()
        });
        let limited = ends(&lay_text(&text, &format));
        assert!(limited.iter().any(|&h| h));
        assert!(!limited.windows(2).any(|w| w[0] && w[1]), "{limited:?}");
    }

    #[test]
    fn soft_hyphen_shows_only_at_line_end() {
        let glyphs = |line: &Line| -> usize {
            line.items
                .iter()
                .map(|item| match item {
                    LineItem::Text(t) => t.run.glyphs.iter().filter(|g| g.x_advance > 0.0).count(),
                    _ => 0,
                })
                .sum()
        };
        // Влезает: мягкий перенос не виден и не занимает места.
        let p = lay_text("ab\u{ad}cd", &ParagraphFormat::new(MONO * 10.0));
        assert_eq!(line_texts(&p), ["ab\u{ad}cd"]);
        assert!(close(p.lines[0].width, MONO * 4.0), "{}", p.lines[0].width);
        assert_eq!(glyphs(&p.lines[0]), 4);
        // Не влезает: перенос на мягком переносе, в конце строки — дефис.
        let p = lay_text("ab\u{ad}cdef", &ParagraphFormat::new(MONO * 4.0 + 0.001));
        assert_eq!(line_texts(&p), ["ab\u{ad}", "cdef"]);
        assert!(close(p.lines[0].width, MONO * 3.0), "{}", p.lines[0].width);
        assert_eq!(glyphs(&p.lines[0]), 3);
        // Дефис тоже должен влезть, иначе перенос — на прошлом пробеле.
        let p = lay_text("ab cd\u{ad}ef", &ParagraphFormat::new(MONO * 5.0 + 0.001));
        assert_eq!(line_texts(&p), ["ab ", "cd\u{ad}ef"]);
        // Неразрывный дефис виден всегда и не даёт переноса.
        let p = lay_text("ab\u{2011}cd", &ParagraphFormat::new(MONO * 4.0 + 0.001));
        assert_eq!(line_texts(&p), ["ab\u{2011}c", "d"]);
        assert_eq!(glyphs(&p.lines[0]), 4);
    }

    #[test]
    fn alignment() {
        let width = MONO * 20.0;
        let mut format = ParagraphFormat::new(width);
        format.right_indent = MONO * 2.0;
        format.align = Align::Right;
        let p = lay_text("abcd ", &format);
        let line = &p.lines[0];
        assert!(close(line.x + line.width, width - MONO * 2.0));
        assert!(close(line.items[0].x(), line.x));

        format.align = Align::Center;
        let p = lay_text("abcd", &format);
        assert!(close(p.lines[0].x, MONO * 7.0));
    }

    #[test]
    fn justify_stretches_spaces_except_last_line() {
        let mut format = ParagraphFormat::new(MONO * 10.0 + 0.001);
        format.align = Align::Justify;
        let p = lay_text("a b c d e f g h\ni j", &format);
        assert_eq!(line_texts(&p), ["a b c d e ", "f g h\n", "i j"]);
        for line in &p.lines[..2] {
            assert!(close(line.width, format.width), "{}", line.width);
            let sum: f32 = line.items.iter().map(LineItem::width).sum();
            // Растяжка добавлена к глифам: сумма частей = ширина
            // вместе с висящим пробелом.
            assert!(sum >= line.width - 1e-3);
        }
        // Разрыв строки (Shift+Enter) тоже растягивает, последняя — нет.
        assert!(close(p.lines[2].width, MONO * 3.0));
        // Позиции букв: «e» на правом краю.
        let first = &p.lines[0];
        let LineItem::Text(item) = &first.items[0] else {
            panic!("{:?}", first.items);
        };
        let e_left = item.x
            + item.run.glyphs[..8]
                .iter()
                .map(|g| g.x_advance)
                .sum::<f32>();
        assert!(close(e_left + MONO, format.width), "{e_left}");
    }

    #[test]
    fn distribute_stretches_every_letter() {
        let mut format = ParagraphFormat::new(MONO * 10.0);
        format.align = Align::Distribute;
        let p = lay_text("abc", &format);
        assert!(close(p.lines[0].width, format.width));
        let LineItem::Text(item) = &p.lines[0].items[0] else {
            panic!();
        };
        assert!(close(item.run.glyphs[0].x_advance, MONO * 4.5));
        assert!(close(item.run.glyphs[2].x_advance, MONO));
    }

    #[test]
    fn indents() {
        let mut format = ParagraphFormat::new(MONO * 8.0 + 0.001);
        format.left_indent = MONO * 2.0;
        format.first_line_indent = MONO * 2.0;
        let p = lay_text("aaaa bbbb cccc", &format);
        assert_eq!(line_texts(&p), ["aaaa ", "bbbb ", "cccc"]);
        assert!(close(p.lines[0].x, MONO * 4.0));
        assert!(close(p.lines[1].x, MONO * 2.0));

        format.first_line_indent = -MONO * 2.0;
        let p = lay_text("aaaa bb cccc", &format);
        assert_eq!(line_texts(&p), ["aaaa bb ", "cccc"]);
        assert!(close(p.lines[0].x, 0.0));
    }

    #[test]
    fn line_breaks_and_empty_lines() {
        let p = lay_text("ab\n", &ParagraphFormat::new(100.0));
        assert_eq!(line_texts(&p), ["ab\n", ""]);
        assert_eq!(p.lines[0].end, LineEnd::Break);
        assert_eq!(p.lines[1].end, LineEnd::Paragraph);
        // Пустая строка берёт высоту у знака абзаца.
        assert!(close(p.lines[1].ascent, p.lines[0].ascent));

        let p = lay_text("ab\u{c}cd", &ParagraphFormat::new(100.0));
        assert_eq!(line_texts(&p), ["ab\u{c}", "cd"]);
        assert_eq!(p.lines[0].end, LineEnd::Page);
        assert!(close(p.lines[0].width, MONO * 2.0));

        let p = lay(&[], &ParagraphFormat::new(100.0));
        assert_eq!(p.lines.len(), 1);
        assert!(p.lines[0].items.is_empty() && p.lines[0].ascent > 0.0);
    }

    fn item_x(p: &ParagraphLines, line: usize, text: &str) -> f32 {
        p.lines[line]
            .items
            .iter()
            .find_map(|item| match item {
                LineItem::Text(t) if &p.text[t.run.range.clone()] == text => Some(t.x),
                _ => None,
            })
            .unwrap_or_else(|| panic!("нет {text:?} в {:?}", p.lines[line].items))
    }

    #[test]
    fn default_tabs() {
        let format = ParagraphFormat::new(200.0);
        let p = lay_text("a\tb\tc", &format);
        assert!(close(item_x(&p, 0, "b"), 36.0));
        assert!(close(item_x(&p, 0, "c"), 72.0));
        let LineItem::Tab(tab) = p.lines[0].items[1] else {
            panic!();
        };
        assert!(close(tab.x, MONO) && close(tab.width, 36.0 - MONO));
        assert_eq!(tab.at, 1);
        // Табуляция по умолчанию за правым краем — перенос.
        let p = lay_text("a\tb", &ParagraphFormat::new(30.0));
        assert_eq!(line_texts(&p), ["a", "\tb"]);
    }

    #[test]
    fn custom_tabs() {
        let stop = |position, kind| TabStop {
            position,
            kind,
            leader: TabLeader::None,
        };
        let mut format = ParagraphFormat::new(400.0);
        format.tab_stops = vec![
            stop(300.0, TabKind::Decimal),
            stop(100.0, TabKind::Left),
            stop(150.0, TabKind::Bar),
            stop(200.0, TabKind::Center),
        ];
        let p = lay_text("a\tb\tcc\t12.5", &format);
        // Заданная позиция убирает позиции по умолчанию левее себя.
        assert!(close(item_x(&p, 0, "b"), 100.0));
        // Черта не останавливает табуляцию.
        assert!(close(item_x(&p, 0, "cc"), 200.0 - MONO));
        assert!(close(item_x(&p, 0, "12.5"), 300.0 - 2.0 * MONO));
        // После последней заданной позиции — снова шаг по умолчанию.
        let p = lay_text("a\tb\tc\td\te", &format);
        assert!(close(item_x(&p, 0, "e"), 324.0));

        format.tab_stops = vec![TabStop {
            position: 300.0,
            kind: TabKind::Right,
            leader: TabLeader::Dot,
        }];
        let p = lay_text("Глава 1\t15", &format);
        let line = &p.lines[0];
        let last = line.items.last().unwrap();
        assert!(close(last.x() + last.width(), 300.0));
        let LineItem::Tab(tab) = line.items[1] else {
            panic!("{:?}", line.items);
        };
        assert_eq!(tab.leader, TabLeader::Dot);
        // Если текст не влезает до позиции, табуляция нулевой ширины.
        let p = lay_text(&format!("\t{}", "x".repeat(60)), &format);
        let LineItem::Tab(tab) = p.lines[0].items[0] else {
            panic!();
        };
        assert!(close(tab.width, 0.0));
    }

    #[test]
    fn hanging_indent_has_a_hidden_tab_stop() {
        let mut format = ParagraphFormat::new(300.0);
        format.left_indent = 18.0;
        format.first_line_indent = -18.0;
        let p = lay_text("1.\tПункт", &format);
        assert!(close(item_x(&p, 0, "Пункт"), 18.0));
    }

    #[test]
    fn right_to_left_words_are_reordered() {
        let format = ParagraphFormat::new(400.0);
        let text = "abc אבג דהו def";
        let p = lay_text(text, &format);
        let items = &p.lines[0].items;
        let words: Vec<_> = items
            .iter()
            .map(|item| match item {
                LineItem::Text(t) => &p.text[t.run.range.clone()],
                LineItem::Tab(_) => "\t",
                LineItem::Object(_) => "\u{fffc}",
            })
            .collect();
        // Слева направо: «abc », иврит, « def».
        assert_eq!(words, ["abc ", "אבג דהו", " def"]);
        // В иврите слова идут справа налево: левее всех — последняя
        // буква второго слова, правее всех — первая буква первого.
        let LineItem::Text(hebrew) = &items[1] else {
            panic!();
        };
        let clusters: Vec<_> = hebrew.run.glyphs.iter().map(|g| g.cluster).collect();
        assert_eq!(clusters.first(), text.rfind('ו').as_ref());
        assert_eq!(clusters.last(), text.find('א').as_ref());
        // Части не налезают друг на друга.
        for pair in items.windows(2) {
            assert!(close(pair[0].x() + pair[0].width(), pair[1].x()));
        }
    }

    #[test]
    fn spans_keep_their_style() {
        let bold = TextStyle {
            bold: true,
            ..TextStyle::new("Liberation Sans", 20.0)
        };
        let p = lay(
            &[TextSpan::new("ab ", mono()), TextSpan::new("cd", bold)],
            &ParagraphFormat::new(200.0),
        );
        let spans: Vec<_> = p.lines[0]
            .items
            .iter()
            .map(|item| match item {
                LineItem::Text(t) => (t.span, t.run.size),
                LineItem::Tab(t) => (t.span, 0.0),
                LineItem::Object(o) => (o.span, 0.0),
            })
            .collect();
        assert_eq!(spans, [(0, 10.0), (1, 20.0)]);
        // Высота строки — по самому крупному шрифту.
        assert!(p.lines[0].ascent > 15.0);
    }

    #[test]
    fn pictures_stand_in_the_line_like_big_letters() {
        let picture = |width| {
            TextSpan::object(
                InlineObject {
                    width,
                    height: 50.0,
                    image: None,
                    shape: None,
                    floating: false,
                },
                mono(),
            )
        };
        let p = lay(
            &[
                TextSpan::new("ab ", mono()),
                picture(60.0),
                TextSpan::new(" cd ", mono()),
                picture(120.0),
            ],
            &ParagraphFormat::new(150.0),
        );
        // Второй рисунок не влез и ушёл на новую строку.
        assert_eq!(p.lines.len(), 2, "{:?}", p.lines);
        let LineItem::Object(first) = &p.lines[0].items[1] else {
            panic!("{:?}", p.lines[0].items);
        };
        assert_eq!(first.at, 3);
        assert!((first.x - 18.0).abs() < 0.1, "{}", first.x);
        assert!(close(first.width, 60.0));
        // Строка поднялась до верха рисунка.
        assert!(close(p.lines[0].ascent, 50.0));
        let LineItem::Object(second) = &p.lines[1].items[0] else {
            panic!("{:?}", p.lines[1].items);
        };
        assert!(close(second.x, 0.0));
        assert_eq!(&p.text[second.at..], "\u{fffc}");
    }

    #[test]
    fn bidi_levels_reorder() {
        assert_eq!(visual_order(&[0, 1, 1, 0]), [0, 2, 1, 3]);
        assert_eq!(visual_order(&[1, 2, 2, 1]), [3, 1, 2, 0]);
        assert_eq!(visual_order(&[0, 0]), [0, 1]);
        assert!(visual_order(&[]).is_empty());
    }
}
