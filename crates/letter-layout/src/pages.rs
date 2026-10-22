//! Вёрстка документа на страницы.
//!
//! Модель документа переводится в простые типы вёрстки (куски текста,
//! оформление абзаца, интервалы) с учётом стилей, абзацы разбиваются на
//! строки, а строки раскладываются по страницам по правилам Word:
//!
//! - каждый раздел начинается с новой страницы, размер листа и поля
//!   берутся из раздела;
//! - разрыв страницы (Ctrl+Enter) и «с новой страницы» начинают новую
//!   страницу;
//! - между абзацами — больший из интервалов «после» и «перед» (или их
//!   сумма, если так велят настройки документа, см. [`gap_between`]);
//! - интервал перед абзацем не добавляется вверху страницы, если
//!   страница началась сама (текст не влез), а не из-за разрыва;
//! - интервал после абзаца может уходить за нижнее поле;
//! - «не разрывать абзац»: абзац целиком переносится на следующую
//!   страницу, если не влезает (и помещается на странице целиком);
//! - «не отрывать от следующего»: цепочка таких абзацев и начало
//!   следующего абзаца переносятся вместе;
//! - «запрет висячих строк»: первая строка абзаца не остаётся одна
//!   внизу страницы, последняя — одна вверху следующей.
//!
//! Пока не учитываются: колонтитулы, сноски, колонки, таблицы (их
//! абзацы идут подряд на всю ширину), рисунки, гиперссылки и другие
//! непонятые элементы внутри абзаца, верхний/нижний индекс.

use std::sync::Arc;

use letter_model::{
    Alignment, Block, Document, FirstLineIndent, LineSpacing, PageSize, Paragraph, ResolvedFonts,
    ResolvedRunProps, RunContent, RunProps, Section, Settings, Styles, TabStopKind, Theme, Twips,
    VerticalAlign,
};

use crate::fonts::FontLibrary;
use crate::lines::{
    Align, Line, LineEnd, ParagraphFormat, TabKind, TabLeader, TabStop, TextSpan, break_lines,
};
use crate::shaping::{Shaper, TextStyle};
use crate::spacing::{LineRule, ParagraphBlock, ParagraphSpacing, effective_spacing, gap_between};

/// Строки этой высоты и меньше не считаются при переносе «висячих»
/// строк и при проверках «влезает ли» (защита от ошибок округления).
const EPSILON: f32 = 0.01;

/// Документ, разложенный по страницам.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentLayout {
    /// Страницы по порядку. Всегда есть хотя бы одна.
    pub pages: Vec<Page>,
}

/// Поля страницы, пт.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Margins {
    /// Сверху.
    pub top: f32,
    /// Справа.
    pub right: f32,
    /// Снизу.
    pub bottom: f32,
    /// Слева (вместе с полем под переплёт).
    pub left: f32,
}

/// Одна страница.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// Ширина листа, пт.
    pub width: f32,
    /// Высота листа, пт.
    pub height: f32,
    /// Поля.
    pub margins: Margins,
    /// Строки текста сверху вниз.
    pub lines: Vec<PageLine>,
}

/// Строка текста на странице.
#[derive(Debug, Clone, PartialEq)]
pub struct PageLine {
    /// Строка; горизонтальные координаты в ней — от левого поля.
    pub line: Arc<Line>,
    /// Левое поле страницы, пт от края листа: прибавить к координатам
    /// строки.
    pub x: f32,
    /// Верх строки, пт от верхнего края листа.
    pub top: f32,
    /// Высота строки, пт.
    pub height: f32,
    /// Базовая линия, пт от верхнего края листа.
    pub baseline: f32,
    /// Номер абзаца в документе (по порядку, абзацы таблиц тоже считаются).
    pub paragraph: usize,
    /// Весь текст абзаца: [`Line::range`] и диапазоны глифов — байты
    /// в нём.
    pub text: Arc<str>,
    /// Оформление кусков текста абзаца: [`crate::TextItem::span`] —
    /// номер в этом списке (цвет, подчёркивание и т. п.).
    pub spans: Arc<[ResolvedRunProps]>,
}

/// Раскладывает документ по страницам.
pub fn layout_document(
    doc: &Document,
    shaper: &mut Shaper,
    library: &FontLibrary,
) -> DocumentLayout {
    layout_document_cached(doc, shaper, library, &mut LayoutCache::default())
}

/// Абзацы прошлой вёрстки, разбитые на строки. Разбиение на строки
/// (подбор шрифтов, шейпинг, переносы) — самая дорогая часть вёрстки,
/// а при правке меняется обычно один абзац: остальные берутся отсюда.
#[derive(Default)]
pub struct LayoutCache {
    /// Стили, настройки и тема, с которыми свёрстаны абзацы: если они
    /// изменились, всё верстается заново.
    context: Option<(Styles, Settings, Theme)>,
    /// Абзацы по порядку в документе.
    entries: Vec<Entry>,
    /// Сколько абзацев разбито на строки заново в последний раз.
    rebuilt: usize,
}

impl LayoutCache {
    /// Сколько абзацев последняя вёрстка разбила на строки заново
    /// (остальные взяты из запомненных).
    pub fn rebuilt(&self) -> usize {
        self.rebuilt
    }
}

/// Запомненный абзац.
struct Entry {
    paragraph: Paragraph,
    /// Ширина текста, пт (`f32::to_bits`, чтобы сравнивать точно).
    width: u32,
    prepared: Arc<Prepared>,
}

/// Раскладывает документ по страницам, беря из `cache` абзацы, которые
/// не изменились с прошлого раза (с начала и с конца документа), и
/// запоминая в нём новые. Результат такой же, как у [`layout_document`].
pub fn layout_document_cached(
    doc: &Document,
    shaper: &mut Shaper,
    library: &FontLibrary,
    cache: &mut LayoutCache,
) -> DocumentLayout {
    let context_same = cache
        .context
        .as_ref()
        .is_some_and(|(styles, settings, theme)| {
            *styles == doc.styles && *settings == doc.settings && *theme == doc.theme
        });
    if !context_same {
        cache.entries.clear();
        cache.context = Some((doc.styles.clone(), doc.settings.clone(), doc.theme.clone()));
    }

    // Все абзацы документа с шириной текста своего раздела.
    let mut all: Vec<(&Paragraph, f32)> = Vec::new();
    let mut section_sizes = Vec::new();
    for section in &doc.sections {
        let width = Geometry::of(section).text_width();
        let mut paragraphs = Vec::new();
        collect_paragraphs(&section.blocks, &mut paragraphs);
        section_sizes.push(paragraphs.len());
        all.extend(paragraphs.into_iter().map(|p| (p, width)));
    }

    // Совпадающие начало и конец со старой вёрсткой.
    let old = std::mem::take(&mut cache.entries);
    let same = |entry: &Entry, (paragraph, width): (&Paragraph, f32)| {
        entry.width == width.to_bits() && entry.paragraph == *paragraph
    };
    let prefix = old
        .iter()
        .zip(&all)
        .take_while(|(entry, new)| same(entry, **new))
        .count();
    let limit = old.len().min(all.len()) - prefix;
    let suffix = old
        .iter()
        .rev()
        .zip(all.iter().rev())
        .take(limit)
        .take_while(|(entry, new)| same(entry, **new))
        .count();
    let mut old: Vec<Option<Entry>> = old.into_iter().map(Some).collect();
    let old_len = old.len();
    let mut rebuilt = 0;
    let mut entries = Vec::with_capacity(all.len());
    for (k, &(paragraph, width)) in all.iter().enumerate() {
        let reused = if k < prefix {
            old[k].take()
        } else if k >= all.len() - suffix {
            old[old_len - (all.len() - k)].take()
        } else {
            None
        };
        let entry = reused.unwrap_or_else(|| {
            rebuilt += 1;
            Entry {
                paragraph: paragraph.clone(),
                width: width.to_bits(),
                prepared: Arc::new(prepare(doc, paragraph, width, shaper, library)),
            }
        });
        entries.push(entry);
    }

    let mut pager = Pager {
        add_spacing: doc.settings.add_paragraph_spacing,
        ..Pager::default()
    };
    let mut paragraph_index = 0;
    let mut start = 0;
    for (section, &count) in doc.sections.iter().zip(&section_sizes) {
        pager.start_section(Geometry::of(section));
        let prepared: Vec<Arc<Prepared>> = entries[start..start + count]
            .iter()
            .map(|e| Arc::clone(&e.prepared))
            .collect();
        start += count;
        let blocks: Vec<ParagraphBlock<'_>> = prepared
            .iter()
            .map(|p| ParagraphBlock {
                // Раскладке по страницам нужны только интервалы и стиль
                // (`effective_spacing`), строки она берёт из `Prepared`.
                lines: &[],
                spacing: p.spacing,
                style: p.style.as_deref(),
            })
            .collect();
        for i in 0..prepared.len() {
            pager.place(&prepared, &blocks, i, paragraph_index);
            paragraph_index += 1;
        }
    }
    if pager.pages.is_empty() {
        pager.start_section(Geometry::of(&Section::default()));
    }
    cache.entries = entries;
    cache.rebuilt = rebuilt;
    DocumentLayout { pages: pager.pages }
}

/// Абзацы раздела по порядку; абзацы таблиц — подряд.
fn collect_paragraphs<'a>(blocks: &'a [Block], out: &mut Vec<&'a Paragraph>) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => out.push(p),
            Block::Table(table) => {
                for cell in table.rows.iter().flat_map(|r| &r.cells) {
                    collect_paragraphs(&cell.blocks, out);
                }
            }
            Block::Unknown(_) => {}
        }
    }
}

/// Размеры листа и полей раздела, пт.
#[derive(Debug, Clone, Copy)]
struct Geometry {
    width: f32,
    height: f32,
    margins: Margins,
}

impl Geometry {
    fn of(section: &Section) -> Geometry {
        let size = section
            .props
            .page_size
            .filter(|s| s.width.0 > 0 && s.height.0 > 0)
            .unwrap_or(PageSize::A4);
        let m = section.props.margins.unwrap_or_default();
        let width = points(size.width);
        let height = points(size.height);
        // Отрицательное поле значит «колонтитул не отодвигает текст»;
        // сам текст всё равно начинается на этом расстоянии.
        let mut margins = Margins {
            top: points(m.top).abs(),
            right: points(m.right).max(0.0),
            bottom: points(m.bottom).abs(),
            left: (points(m.left) + points(m.gutter)).max(0.0),
        };
        // Поля не могут съесть весь лист.
        if margins.left + margins.right > width - 1.0 {
            margins.left = 0.0;
            margins.right = 0.0;
        }
        if margins.top + margins.bottom > height - 1.0 {
            margins.top = 0.0;
            margins.bottom = 0.0;
        }
        Geometry {
            width,
            height,
            margins,
        }
    }

    fn text_width(&self) -> f32 {
        self.width - self.margins.left - self.margins.right
    }
}

fn points(t: Twips) -> f32 {
    t.to_points() as f32
}

/// Абзац, готовый к раскладке.
struct Prepared {
    text: Arc<str>,
    /// Строки общие с [`PageLine::line`]: страницы не копируют глифы.
    lines: Vec<Arc<Line>>,
    /// Высота каждой строки с учётом межстрочного интервала, пт.
    heights: Vec<f32>,
    spans: Arc<[ResolvedRunProps]>,
    spacing: ParagraphSpacing,
    style: Option<String>,
    keep_next: bool,
    keep_lines: bool,
    page_break_before: bool,
    widow_control: bool,
}

/// Переводит абзац модели в типы вёрстки и разбивает на строки.
fn prepare(
    doc: &Document,
    paragraph: &Paragraph,
    width: f32,
    shaper: &mut Shaper,
    library: &FontLibrary,
) -> Prepared {
    let props = doc.resolve_paragraph(&paragraph.props);
    let mut spans = Vec::new();
    let mut span_props = Vec::new();
    for run in paragraph.runs() {
        let resolved = doc.resolve_run(&paragraph.props, &run.props);
        let mut text = String::new();
        for item in &run.content {
            match item {
                RunContent::Text(t) => text.push_str(t),
                RunContent::Tab => text.push('\t'),
                RunContent::Break(letter_model::BreakKind::Line) => text.push('\n'),
                // Колонок пока нет: разрыв колонки — как разрыв страницы.
                RunContent::Break(_) => text.push('\u{c}'),
                RunContent::Unknown(_) => {}
            }
        }
        for (shown, small) in letter_case(&text, &resolved) {
            for (piece, family) in split_by_font_slot(&shown, &resolved.fonts) {
                let mut style = text_style(&resolved, family);
                if small {
                    style.size *= SMALL_CAPS_SCALE;
                }
                spans.push(TextSpan::new(piece, style));
                span_props.push(resolved.clone());
            }
        }
    }
    let mark = doc.resolve_run(&paragraph.props, &RunProps::default());
    let mark_style = text_style(&mark, &mark.fonts.h_ansi);

    let mut format = ParagraphFormat::new(width);
    format.left_indent = points(props.indent_left);
    format.right_indent = points(props.indent_right);
    format.first_line_indent = match props.first_line {
        Some(FirstLineIndent::FirstLine(t)) => points(t),
        Some(FirstLineIndent::Hanging(t)) => -points(t),
        None => 0.0,
    };
    format.align = match props.alignment {
        Alignment::Left => Align::Left,
        Alignment::Center => Align::Center,
        Alignment::Right => Align::Right,
        Alignment::Justify => Align::Justify,
        Alignment::Distribute => Align::Distribute,
    };
    format.default_tab = points(doc.settings.default_tab_stop);
    format.tab_stops = props
        .tabs
        .iter()
        .filter_map(|t| {
            let kind = match t.kind {
                TabStopKind::Left => TabKind::Left,
                TabStopKind::Center => TabKind::Center,
                TabStopKind::Right => TabKind::Right,
                TabStopKind::Decimal => TabKind::Decimal,
                TabStopKind::Bar => TabKind::Bar,
                TabStopKind::Clear => return None,
            };
            let leader = match t.leader {
                letter_model::TabLeader::None => TabLeader::None,
                letter_model::TabLeader::Dot => TabLeader::Dot,
                letter_model::TabLeader::Hyphen => TabLeader::Hyphen,
                letter_model::TabLeader::Underscore => TabLeader::Underscore,
                letter_model::TabLeader::Heavy => TabLeader::Heavy,
                letter_model::TabLeader::MiddleDot => TabLeader::MiddleDot,
            };
            Some(TabStop {
                position: points(t.position),
                kind,
                leader,
            })
        })
        .collect();

    let spacing = ParagraphSpacing {
        before: points(props.space_before),
        after: points(props.space_after),
        line: match props.line_spacing {
            LineSpacing::Multiple(n) => LineRule::Multiple(n as f32 / 240.0),
            LineSpacing::Exact(t) => LineRule::Exact(points(t)),
            LineSpacing::AtLeast(t) => LineRule::AtLeast(points(t)),
        },
        contextual: props.contextual_spacing,
    };
    let broken = break_lines(shaper, library, &spans, &mark_style, &format);
    Prepared {
        text: broken.text.into(),
        heights: broken
            .lines
            .iter()
            .map(|l| spacing.line.height(l))
            .collect(),
        lines: broken.lines.into_iter().map(Arc::new).collect(),
        spans: span_props.into(),
        spacing,
        style: props.style,
        keep_next: props.keep_next,
        keep_lines: props.keep_lines,
        page_break_before: props.page_break_before,
        widow_control: props.widow_control,
    }
}

/// Во сколько раз уменьшаются строчные буквы, показанные малыми
/// прописными.
const SMALL_CAPS_SCALE: f32 = 0.8;

/// Текст для показа с «Все прописные» и «Малые прописные»: куски
/// и признак «уменьшенные прописные». Буква меняется, только если
/// прописная занимает столько же байтов, — чтобы позиции курсора
/// в тексте не сдвинулись.
fn letter_case(text: &str, props: &ResolvedRunProps) -> Vec<(String, bool)> {
    if !props.caps && !props.small_caps {
        return vec![(text.to_string(), false)];
    }
    let mut pieces: Vec<(String, bool)> = Vec::new();
    for c in text.chars() {
        let mut upper = c.to_uppercase();
        let (shown, changed) = match (upper.next(), upper.next()) {
            (Some(u), None) if u != c && u.len_utf8() == c.len_utf8() => (u, true),
            _ => (c, false),
        };
        // «Все прописные» сильнее «Малых прописных», как в Word.
        let small = changed && !props.caps;
        match pieces.last_mut() {
            Some((piece, s)) if *s == small => piece.push(shown),
            _ => pieces.push((shown.to_string(), small)),
        }
    }
    pieces
}

/// Во сколько раз уменьшается надстрочный и подстрочный знак.
const SCRIPT_SCALE: f32 = 2.0 / 3.0;

/// Насколько поднят над базовой линией текст с оформлением `props`, пт
/// (подстрочный — отрицательное число): надстрочный — на треть кегля,
/// подстрочный опущен на 0,14 кегля. Кегль — до уменьшения.
pub fn baseline_rise(props: &ResolvedRunProps) -> f32 {
    let size = props.size.0 as f32 / 2.0;
    let script = match props.vertical_align {
        VerticalAlign::Baseline => 0.0,
        VerticalAlign::Superscript => size / 3.0,
        VerticalAlign::Subscript => -size * 0.14,
    };
    // «Смещение» из окна «Шрифт» — в полупунктах.
    script + props.position as f32 / 2.0
}

fn text_style(props: &ResolvedRunProps, family: &str) -> TextStyle {
    let scale = match props.vertical_align {
        VerticalAlign::Baseline => 1.0,
        _ => SCRIPT_SCALE,
    };
    TextStyle {
        family: family.to_string(),
        size: props.size.0 as f32 / 2.0 * scale,
        bold: props.bold,
        italic: props.italic,
        kerning: props.kerned(),
        ligatures: false,
        letter_spacing: props.spacing.0 as f32 / 20.0,
        width_scale: f32::from(props.scale.clamp(1, 600)) / 100.0,
        hidden: props.hidden,
    }
}

/// Делит текст по шрифтам, как Word: латиница ASCII — шрифтом `ascii`,
/// восточноазиатское письмо — `east_asia`, иврит, арабский, тайский
/// и индийские письменности — `complex_script`, остальное (кириллица,
/// греческий, латиница с диакритикой) — `h_ansi`. Табуляции, разрывы
/// и пробелы остаются в текущем куске.
fn split_by_font_slot<'a>(text: &str, fonts: &'a ResolvedFonts) -> Vec<(String, &'a str)> {
    let mut pieces: Vec<(String, &'a str)> = Vec::new();
    for c in text.chars() {
        let family = match c as u32 {
            0x00..=0x7F => fonts.ascii.as_str(),
            0x0590..=0x08FF
            | 0x0900..=0x0DFF
            | 0x0E00..=0x0E7F
            | 0xFB1D..=0xFDFF
            | 0xFE70..=0xFEFF => fonts.complex_script.as_str(),
            0x1100..=0x11FF
            | 0x2E80..=0x9FFF
            | 0xA960..=0xA97F
            | 0xAC00..=0xD7FF
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFFEF
            | 0x20000..=0x2FFFF => fonts.east_asia.as_str(),
            _ => fonts.h_ansi.as_str(),
        };
        let neutral = c.is_whitespace() || c.is_control();
        match pieces.last_mut() {
            Some((text, f)) if *f == family || neutral => text.push(c),
            _ => pieces.push((c.to_string(), family)),
        }
    }
    pieces
}

/// Раскладчик по страницам.
#[derive(Default)]
struct Pager {
    pages: Vec<Page>,
    geometry: Option<Geometry>,
    /// Где кончается текст на текущей странице, пт от верха листа.
    y: f32,
    /// На странице уже что-то есть.
    has_content: bool,
    /// Страница началась сама (текст не влез), а не из-за разрыва.
    soft_top: bool,
    /// Предыдущий абзац кончился разрывом страницы: следующий начнёт
    /// новую страницу.
    break_pending: bool,
    /// Интервал после последнего абзаца на странице.
    previous_after: f32,
    /// Складывать интервалы между абзацами (иначе — больший из двух).
    add_spacing: bool,
}

impl Pager {
    fn start_section(&mut self, geometry: Geometry) {
        self.geometry = Some(geometry);
        self.new_page(false);
    }

    fn new_page(&mut self, soft: bool) {
        let Some(g) = self.geometry else {
            return;
        };
        self.pages.push(Page {
            width: g.width,
            height: g.height,
            margins: g.margins,
            lines: Vec::new(),
        });
        self.y = g.margins.top;
        self.has_content = false;
        self.soft_top = soft;
        self.break_pending = false;
        self.previous_after = 0.0;
    }

    fn top(&self) -> f32 {
        self.geometry.map_or(0.0, |g| g.margins.top)
    }

    fn bottom(&self) -> f32 {
        self.geometry
            .map_or(f32::INFINITY, |g| g.height - g.margins.bottom)
    }

    /// Высота области текста.
    fn page_height(&self) -> f32 {
        self.bottom() - self.top()
    }

    /// Сколько добавить перед абзацем с интервалом `before`: вверху
    /// страницы, начатой самой, ничего, ниже предыдущего абзаца — с учётом
    /// его интервала после (он уже добавлен к `y`).
    fn space_before(&self, before: f32) -> f32 {
        if !self.has_content {
            return if self.soft_top { 0.0 } else { before };
        }
        gap_between(self.previous_after, before, self.add_spacing) - self.previous_after
    }

    /// Высота цепочки «не отрывать от следующего», начиная с абзаца `i`:
    /// сами абзацы и первые строки следующего за цепочкой.
    fn keep_chain_height(
        &self,
        prepared: &[Arc<Prepared>],
        blocks: &[ParagraphBlock<'_>],
        i: usize,
    ) -> f32 {
        let mut height = 0.0;
        let mut j = i;
        while let Some(p) = prepared.get(j) {
            let (before, after) = effective_spacing(blocks, j);
            let before = if j == i {
                self.space_before(before)
            } else {
                let (_, previous_after) = effective_spacing(blocks, j - 1);
                gap_between(previous_after, before, self.add_spacing) - previous_after
            };
            let heights = &p.heights;
            if !p.keep_next {
                let first = if p.widow_control && heights.len() > 1 {
                    2
                } else {
                    1
                };
                height += before + heights.iter().take(first).sum::<f32>();
                break;
            }
            height += before + heights.iter().sum::<f32>() + after;
            j += 1;
        }
        height
    }

    fn place(
        &mut self,
        prepared: &[Arc<Prepared>],
        blocks: &[ParagraphBlock<'_>],
        i: usize,
        paragraph_index: usize,
    ) {
        let Some(p) = prepared.get(i) else {
            return;
        };
        let (before, after) = effective_spacing(blocks, i);
        // Как в Word: страница после разрыва в конце абзаца начинается
        // без интервала перед абзацем, как будто текст просто не влез.
        if self.break_pending && self.has_content {
            self.new_page(true);
        }
        if p.page_break_before && self.has_content {
            self.new_page(false);
        }
        if p.keep_next && self.has_content {
            let chain = self.keep_chain_height(prepared, blocks, i);
            if self.y + chain > self.bottom() + EPSILON && chain <= self.page_height() {
                self.new_page(true);
            }
        }

        let heights = &p.heights;
        // Куски абзаца между разрывами страницы.
        let mut segments = Vec::new();
        let mut start = 0;
        for (k, line) in p.lines.iter().enumerate() {
            if line.end == LineEnd::Page {
                segments.push(start..k + 1);
                start = k + 1;
            }
        }
        if start < p.lines.len() || segments.is_empty() {
            segments.push(start..p.lines.len());
        }
        // Разрыв страницы в самом конце абзаца: знак абзаца остаётся на
        // странице с разрывом (Word не переносит пустую строку на новую
        // страницу), новую страницу начнёт следующий абзац.
        let mut ends_with_break = false;
        if segments.len() > 1
            && segments.last().is_some_and(|last| {
                last.len() == 1 && p.lines.get(last.start).is_some_and(|l| l.range.is_empty())
            })
        {
            segments.pop();
            ends_with_break = true;
        }

        let mut first_segment = true;
        for (s, segment) in segments.iter().enumerate() {
            if s > 0 {
                self.new_page(false);
            }
            let mut next = segment.start;
            while next < segment.end {
                let before_now = if first_segment && next == segment.start {
                    self.space_before(before)
                } else {
                    0.0
                };
                let available = self.bottom() - self.y - before_now;
                let remaining = segment.end - next;
                let mut fit = 0;
                let mut used = 0.0;
                for h in &heights[next..segment.end] {
                    if used + h > available + EPSILON {
                        break;
                    }
                    used += h;
                    fit += 1;
                }
                if fit < remaining {
                    let whole: f32 = heights[segment.clone()].iter().sum();
                    let at_start = next == segment.start;
                    if p.keep_lines
                        && at_start
                        && self.has_content
                        && whole + before <= self.page_height()
                    {
                        self.new_page(true);
                        continue;
                    }
                    if p.widow_control && remaining > 1 {
                        // Последняя строка не остаётся одна наверху.
                        if remaining - fit == 1 {
                            fit = fit.saturating_sub(1);
                        }
                        // Первая строка не остаётся одна внизу.
                        if at_start && fit == 1 {
                            fit = 0;
                        }
                    }
                    if fit == 0 {
                        if self.has_content {
                            self.new_page(true);
                            continue;
                        }
                        // На пустой странице — хоть одна строка.
                        fit = 1;
                    }
                }
                self.y += before_now;
                for k in next..next + fit {
                    self.push_line(p, k, paragraph_index);
                }
                next += fit;
                if next < segment.end {
                    self.new_page(true);
                }
            }
            first_segment = false;
        }
        self.y += after;
        self.previous_after = after;
        self.has_content = true;
        self.break_pending = ends_with_break;
    }

    fn push_line(&mut self, p: &Prepared, k: usize, paragraph: usize) {
        let (Some(line), Some(&height), Some(g)) =
            (p.lines.get(k), p.heights.get(k), self.geometry)
        else {
            return;
        };
        let top = self.y;
        let placed = PageLine {
            line: Arc::clone(line),
            x: g.margins.left,
            top,
            height,
            baseline: top + p.spacing.line.baseline_offset(line, height),
            paragraph,
            text: p.text.clone(),
            spans: p.spans.clone(),
        };
        if let Some(page) = self.pages.last_mut() {
            page.lines.push(placed);
        }
        self.y += height;
        self.has_content = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::{BreakKind, PageMargins, Run, Section, SectionProps};

    /// Высота строки Times New Roman 10 пт (шрифт по умолчанию).
    const LINE: f32 = 2355.0 / 2048.0 * 10.0;

    /// Документ с листом на 5 строк без полей.
    fn doc(paragraphs: Vec<Paragraph>) -> Document {
        let mut doc = Document::new();
        doc.sections = vec![Section {
            props: SectionProps {
                page_size: Some(PageSize {
                    width: Twips(4000),
                    height: Twips(1152),
                    orientation: Default::default(),
                }),
                margins: Some(PageMargins {
                    top: Twips(0),
                    right: Twips(0),
                    bottom: Twips(0),
                    left: Twips(0),
                    ..PageMargins::default()
                }),
                unknown: Vec::new(),
            },
            blocks: paragraphs.into_iter().map(Block::from).collect(),
        }];
        doc
    }

    fn para(text: &str) -> Paragraph {
        let mut run = Run::default();
        for (i, line) in text.split('|').enumerate() {
            if i > 0 {
                run.content.push(RunContent::Break(BreakKind::Line));
            }
            run.content.push(RunContent::Text(line.into()));
        }
        Paragraph::from_runs([run])
    }

    fn lay(doc: &Document) -> DocumentLayout {
        let library = FontLibrary::embedded_only();
        layout_document(doc, &mut Shaper::new(), &library)
    }

    /// Номера абзацев на каждой странице (по строкам).
    fn pages(layout: &DocumentLayout) -> Vec<Vec<usize>> {
        layout
            .pages
            .iter()
            .map(|p| p.lines.iter().map(|l| l.paragraph).collect())
            .collect()
    }

    fn singles(n: usize) -> Vec<Paragraph> {
        (0..n).map(|i| para(&format!("p{i}"))).collect()
    }

    #[test]
    fn lines_flow_onto_new_pages() {
        let layout = lay(&doc(singles(12)));
        assert_eq!(
            pages(&layout),
            [vec![0, 1, 2, 3, 4], vec![5, 6, 7, 8, 9], vec![10, 11]]
        );
        let first = &layout.pages[0];
        assert!((first.height - 57.6).abs() < 1e-3);
        assert!((first.lines[1].top - LINE).abs() < 1e-3);
        let line = &first.lines[0];
        assert!((line.baseline - (LINE - line.line.descent)).abs() < 1e-3);
        assert_eq!(line.spans.len(), 1);
    }

    #[test]
    fn page_breaks() {
        let mut with_break = para("a");
        with_break.content.push(letter_model::Inline::Run(Run {
            props: RunProps::default(),
            content: vec![
                RunContent::Break(BreakKind::Page),
                RunContent::Text("b".into()),
            ],
        }));
        let mut new_page = para("c");
        new_page.props.page_break_before = Some(true);
        let layout = lay(&doc(vec![with_break, new_page, para("d")]));
        assert_eq!(pages(&layout), [vec![0], vec![0], vec![1, 2]]);
        // «С новой страницы» у первого абзаца страницы ничего не делает.
        let mut first = para("x");
        first.props.page_break_before = Some(true);
        assert_eq!(pages(&lay(&doc(vec![first]))), [vec![0]]);
    }

    #[test]
    fn break_at_paragraph_end_starts_next_page_without_space_before() {
        let mut only_break = para("a");
        only_break.content.push(letter_model::Inline::Run(Run {
            props: RunProps::default(),
            content: vec![RunContent::Break(BreakKind::Page)],
        }));
        let mut next = para("b");
        next.props.spacing = letter_model::Spacing {
            before: Some(Twips(240)),
            ..Default::default()
        };
        let layout = lay(&doc(vec![only_break, next, para("c")]));
        // Пустая строка после разрыва не переносится на новую страницу.
        assert_eq!(pages(&layout), [vec![0], vec![1, 2]]);
        // И интервал перед абзацем вверху новой страницы не добавляется.
        assert!(layout.pages[1].lines[0].top.abs() < 1e-3);
    }

    #[test]
    fn widow_and_orphan_control() {
        // Первая строка не остаётся одна внизу: весь абзац переносится.
        let mut paragraphs = singles(4);
        paragraphs.push(para("a|b|c"));
        assert_eq!(
            pages(&lay(&doc(paragraphs))),
            [vec![0, 1, 2, 3], vec![4, 4, 4]]
        );
        // Последняя строка не остаётся одна наверху: с ней уходит ещё одна.
        let mut paragraphs = singles(2);
        paragraphs.push(para("a|b|c|d"));
        assert_eq!(
            pages(&lay(&doc(paragraphs.clone()))),
            [vec![0, 1, 2, 2], vec![2, 2]]
        );
        // Без запрета — как получится.
        paragraphs[2].props.widow_control = Some(false);
        assert_eq!(
            pages(&lay(&doc(paragraphs))),
            [vec![0, 1, 2, 2, 2], vec![2]]
        );
    }

    #[test]
    fn keep_lines_together() {
        let mut paragraphs = singles(2);
        let mut kept = para("a|b|c|d");
        kept.props.keep_lines = Some(true);
        paragraphs.push(kept);
        assert_eq!(
            pages(&lay(&doc(paragraphs))),
            [vec![0, 1], vec![2, 2, 2, 2]]
        );
        // Абзац длиннее страницы всё равно делится.
        let mut long = para("a|b|c|d|e|f|g");
        long.props.keep_lines = Some(true);
        let mut paragraphs = singles(1);
        paragraphs.push(long);
        assert_eq!(
            pages(&lay(&doc(paragraphs))),
            [vec![0, 1, 1, 1, 1], vec![1, 1, 1]]
        );
    }

    #[test]
    fn keep_with_next() {
        let mut paragraphs = singles(4);
        let mut heading = para("Заголовок");
        heading.props.keep_next = Some(true);
        paragraphs.push(heading);
        paragraphs.push(para("a|b"));
        assert_eq!(
            pages(&lay(&doc(paragraphs))),
            [vec![0, 1, 2, 3], vec![4, 5, 5]]
        );
    }

    #[test]
    fn space_before_at_the_top_of_a_page() {
        let spaced = |text| {
            let mut p = para(text);
            p.props.spacing.before = Some(Twips(100)); // 5 пт
            p
        };
        // Первая страница документа: интервал есть.
        let layout = lay(&doc(vec![spaced("a")]));
        assert!((layout.pages[0].lines[0].top - 5.0).abs() < 1e-3);
        // Страница началась сама: интервала нет.
        let mut paragraphs = singles(5);
        paragraphs.push(spaced("b"));
        let layout = lay(&doc(paragraphs));
        assert_eq!(layout.pages[1].lines[0].top, 0.0);
        // После разрыва страницы — есть.
        let mut after_break = spaced("c");
        after_break.props.page_break_before = Some(true);
        let layout = lay(&doc(vec![para("x"), after_break]));
        assert!((layout.pages[1].lines[0].top - 5.0).abs() < 1e-3);
    }

    #[test]
    fn spacing_between_paragraphs() {
        let mut first = para("a");
        first.props.spacing.after = Some(Twips(200)); // 10 пт
        let mut second = para("b");
        second.props.spacing.before = Some(Twips(120)); // 6 пт
        let mut document = doc(vec![first, second]);
        // По умолчанию — больший из двух интервалов.
        let layout = lay(&document);
        assert!((layout.pages[0].lines[1].top - (LINE + 10.0)).abs() < 1e-3);
        document.settings.add_paragraph_spacing = true;
        let layout = lay(&document);
        assert!((layout.pages[0].lines[1].top - (LINE + 16.0)).abs() < 1e-3);
    }

    #[test]
    fn sections_have_their_own_pages() {
        let mut document = doc(singles(1));
        let mut second = document.sections[0].clone();
        second.props.page_size = None;
        second.props.margins = None;
        document.sections.push(second);
        let layout = lay(&document);
        assert_eq!(layout.pages.len(), 2);
        let a4 = &layout.pages[1];
        assert!((a4.width - 595.3).abs() < 0.1 && (a4.height - 841.9).abs() < 0.1);
        // Поля по умолчанию: слева 3 см, сверху 2 см.
        assert!((a4.margins.left - 85.05).abs() < 0.01);
        assert!((a4.lines[0].x - a4.margins.left).abs() < 1e-3);
        assert!((a4.lines[0].top - 56.7).abs() < 1e-3);

        let mut empty = Document::new();
        empty.sections.clear();
        assert_eq!(lay(&empty).pages.len(), 1);
    }

    #[test]
    fn cached_layout_matches_a_fresh_one() {
        let library = FontLibrary::embedded_only();
        let mut shaper = Shaper::new();
        let mut cache = LayoutCache::default();
        let words = "word ".repeat(30);
        let mut d = doc((0..12).map(|i| para(&format!("{i} {words}"))).collect());
        let mut check = |d: &Document, rebuilt: Option<usize>| {
            let cached = layout_document_cached(d, &mut shaper, &library, &mut cache);
            assert_eq!(cached, lay(d));
            if let Some(n) = rebuilt {
                assert_eq!(cache.rebuilt(), n);
            }
        };
        check(&d, Some(12));
        // Ничего не менялось — ничего не верстается заново.
        check(&d, Some(0));
        // Правка в середине: заново только этот абзац.
        d.sections[0].blocks[5] = para("short").into();
        check(&d, Some(1));
        // Вставка и удаление абзацев.
        d.sections[0].blocks.insert(3, para("new|line").into());
        check(&d, Some(1));
        d.sections[0].blocks.remove(0);
        check(&d, Some(0));
        d.sections[0].blocks.truncate(2);
        check(&d, Some(0));
        // Интервал «с новой страницы» у абзаца сдвигает следующие страницы.
        if let Some(Block::Paragraph(p)) = d.sections[0].blocks.get_mut(1) {
            p.props.page_break_before = Some(true);
        }
        check(&d, Some(1));
        // Сменились стили — заново всё.
        d.styles.defaults.run.size = Some(letter_model::HalfPoints(30));
        check(&d, Some(2));
        // Другая ширина листа — абзацы раздела заново.
        if let Some(size) = &mut d.sections[0].props.page_size {
            size.width = Twips(5000);
        }
        check(&d, Some(2));
        // Пустой документ.
        d.sections.clear();
        check(&d, Some(0));
    }

    #[test]
    fn letters_use_the_font_of_their_script() {
        let fonts = ResolvedFonts {
            ascii: "A".into(),
            h_ansi: "H".into(),
            east_asia: "E".into(),
            complex_script: "C".into(),
            hint: None,
        };
        let pieces = split_by_font_slot("ab cd Жук\tשלום 中文", &fonts);
        let pieces: Vec<_> = pieces.iter().map(|(t, f)| (t.as_str(), *f)).collect();
        assert_eq!(
            pieces,
            [
                ("ab cd ", "A"),
                ("Жук\t", "H"),
                ("שלום ", "C"),
                ("中文", "E")
            ]
        );
    }

    #[test]
    fn font_effects_change_the_text() {
        let with = |props: RunProps| {
            let mut p = para("Абв abc");
            if let Some(letter_model::Inline::Run(run)) = p.content.first_mut() {
                run.props = props;
            }
            let layout = lay(&doc(vec![p]));
            let line = layout.pages[0].lines[0].clone();
            let width: f32 = line.line.items.iter().map(|i| i.width()).sum();
            let sizes: Vec<f32> = line
                .line
                .items
                .iter()
                .filter_map(|i| match i {
                    crate::LineItem::Text(t) => Some(t.run.size),
                    _ => None,
                })
                .collect();
            (layout, width, sizes)
        };
        let (plain, plain_width, _) = with(RunProps::default());
        // Все прописные: показан прописными, байты на тех же местах.
        let (caps, caps_width, _) = with(RunProps {
            caps: Some(true),
            ..RunProps::default()
        });
        assert_eq!(&*caps.pages[0].lines[0].text, "АБВ ABC");
        assert_eq!(
            caps.pages[0].lines[0].text.len(),
            plain.pages[0].lines[0].text.len()
        );
        assert!(caps_width > plain_width);
        // Малые прописные: строчные — меньшим кеглем.
        let (_, small_width, sizes) = with(RunProps {
            small_caps: Some(true),
            ..RunProps::default()
        });
        assert!(sizes.contains(&8.0) && sizes.contains(&10.0), "{sizes:?}");
        assert!(small_width > plain_width && small_width < caps_width);
        // Разреженный на 1 пт: на 7 знаков шире на 7 пт.
        let (_, spaced, _) = with(RunProps {
            spacing: Some(Twips(20)),
            ..RunProps::default()
        });
        assert!(
            (spaced - plain_width - 7.0).abs() < 0.01,
            "{spaced} {plain_width}"
        );
        let (_, wide, _) = with(RunProps {
            scale: Some(200),
            ..RunProps::default()
        });
        assert!((wide - 2.0 * plain_width).abs() < 0.01);
        let (_, hidden, _) = with(RunProps {
            hidden: Some(true),
            ..RunProps::default()
        });
        assert_eq!(hidden, 0.0);
        let raised = doc(vec![]).resolve_run(
            &letter_model::ParagraphProps::default(),
            &RunProps {
                position: Some(6),
                ..RunProps::default()
            },
        );
        assert_eq!(baseline_rise(&raised), 3.0);
    }
}
