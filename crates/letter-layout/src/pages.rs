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
//!   внизу страницы, последняя — одна вверху следующей;
//! - у пунктов списка перед первой строкой стоит номер или маркер
//!   ([`ListMark`]), текст начинается после него — за табуляцией,
//!   пробелом или сразу;
//! - таблицы верстаются по строкам (см. [`crate::table`]): строка,
//!   которая не влезает, переносится или делится между страницами,
//!   строки-заголовки повторяются на каждой странице.
//!
//! Пока не учитываются: колонтитулы, сноски, колонки, рисунки,
//! гиперссылки и другие непонятые элементы внутри абзаца.

use std::sync::Arc;

use letter_model::{
    Alignment, Block, Document, FirstLineIndent, LevelSuffix, LineSpacing, ListCounters, ListLabel,
    Numbering, PageSize, Paragraph, ResolvedFonts, ResolvedRunProps, RunContent, RunFonts,
    RunProps, Section, Settings, Styles, TabStopKind, Theme, Twips, VerticalAlign,
};

use crate::fonts::FontLibrary;
use crate::lines::{
    Align, Line, LineEnd, LineItem, ParagraphFormat, TabKind, TabLeader, TabStop, TextSpan,
    break_lines,
};
use crate::shaping::{Shaper, TextStyle};
use crate::spacing::{LineRule, ParagraphBlock, ParagraphSpacing, effective_spacing, gap_between};
use crate::table::{BoxLine, Canvas, Ctx, RowBox, TableGeometry, build_rows, draw_row, split_row};

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
    /// Строки текста сверху вниз (в таблице — по ячейкам).
    pub lines: Vec<PageLine>,
    /// Заливка ячеек и линии границ таблиц.
    pub shapes: Vec<Shape>,
}

/// Прямоугольник на странице: заливка или линия границы. Координаты —
/// пт от левого верхнего угла листа.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    /// Левый край.
    pub x: f32,
    /// Верхний край.
    pub y: f32,
    /// Ширина.
    pub width: f32,
    /// Высота.
    pub height: f32,
    /// Цвет.
    pub color: (u8, u8, u8),
    /// Заливка (рисуется под текстом) или линия (над ним).
    pub kind: ShapeKind,
}

/// Вид прямоугольника.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    /// Заливка ячейки.
    Fill,
    /// Линия границы.
    Rule,
}

/// Ячейка таблицы на странице, пт от левого верхнего угла листа.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellArea {
    /// Левый край.
    pub left: f32,
    /// Верхний край.
    pub top: f32,
    /// Правый край.
    pub right: f32,
    /// Нижний край.
    pub bottom: f32,
}

impl CellArea {
    /// Ячейка, сдвинутая на `dx`, `dy`.
    pub fn moved(self, dx: f32, dy: f32) -> CellArea {
        CellArea {
            left: self.left + dx,
            top: self.top + dy,
            right: self.right + dx,
            bottom: self.bottom + dy,
        }
    }

    /// Точка внутри ячейки.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        (self.left..self.right).contains(&x) && (self.top..self.bottom).contains(&y)
    }
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
    /// Номер или маркер пункта списка — только у первой строки абзаца.
    pub label: Option<Arc<ListMark>>,
    /// Ячейка таблицы, в которой стоит строка (самая внутренняя).
    pub cell: Option<CellArea>,
}

/// Номер или маркер пункта списка, готовый к рисованию.
#[derive(Debug, Clone, PartialEq)]
pub struct ListMark {
    /// Текст номера («1.», «•»).
    pub text: String,
    /// Глифы; горизонтальные координаты — от левого поля, как у строки.
    /// `span` у кусков — 0.
    pub items: Vec<LineItem>,
    /// Оформление номера.
    pub props: ResolvedRunProps,
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
    context: Option<(Styles, Settings, Theme, Numbering)>,
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
    /// Номер пункта списка: зависит от абзацев выше.
    label: Option<ListLabel>,
    /// Стиль таблицы, если абзац в ячейке.
    table: Option<String>,
    prepared: Arc<Prepared>,
}

/// Абзац документа и где он стоит: ширина текста и стиль таблицы
/// (если абзац в ячейке; `""` — стиль таблиц по умолчанию).
struct Item<'a> {
    paragraph: &'a Paragraph,
    width: f32,
    table: Option<String>,
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
    let context_same =
        cache
            .context
            .as_ref()
            .is_some_and(|(styles, settings, theme, numbering)| {
                *styles == doc.styles
                    && *settings == doc.settings
                    && *theme == doc.theme
                    && *numbering == doc.numbering
            });
    if !context_same {
        cache.entries.clear();
        cache.context = Some((
            doc.styles.clone(),
            doc.settings.clone(),
            doc.theme.clone(),
            doc.numbering.clone(),
        ));
    }

    // Все абзацы документа с шириной текста: раздела или ячейки.
    let mut all: Vec<Item<'_>> = Vec::new();
    for section in &doc.sections {
        let width = Geometry::of(section).text_width();
        collect_paragraphs(doc, &section.blocks, width, None, &mut all);
    }
    let labels = list_labels(doc, &all);

    // Совпадающие начало и конец со старой вёрсткой.
    let old = std::mem::take(&mut cache.entries);
    let same = |entry: &Entry, k: usize| {
        let item = &all[k];
        entry.width == item.width.to_bits()
            && entry.paragraph == *item.paragraph
            && entry.label == labels[k]
            && entry.table == item.table
    };
    let prefix = old
        .iter()
        .enumerate()
        .take_while(|&(k, entry)| k < all.len() && same(entry, k))
        .count();
    let limit = old.len().min(all.len()) - prefix;
    let suffix = old
        .iter()
        .rev()
        .zip((0..all.len()).rev())
        .take(limit)
        .take_while(|&(entry, k)| same(entry, k))
        .count();
    let mut old: Vec<Option<Entry>> = old.into_iter().map(Some).collect();
    let old_len = old.len();
    let mut rebuilt = 0;
    let mut entries = Vec::with_capacity(all.len());
    for (k, item) in all.iter().enumerate() {
        let reused = if k < prefix {
            old[k].take()
        } else if k >= all.len() - suffix {
            old[old_len - (all.len() - k)].take()
        } else {
            None
        };
        let entry = reused.unwrap_or_else(|| {
            rebuilt += 1;
            let label = labels[k].clone();
            let table = item.table.as_deref();
            let prepared = prepare(
                doc,
                item.paragraph,
                label.as_ref(),
                table,
                item.width,
                shaper,
                library,
            );
            Entry {
                paragraph: item.paragraph.clone(),
                width: item.width.to_bits(),
                label,
                table: item.table.clone(),
                prepared: Arc::new(prepared),
            }
        });
        entries.push(entry);
    }

    let mut pager = Pager {
        add_spacing: doc.settings.add_paragraph_spacing,
        ..Pager::default()
    };
    let prepared: Vec<Arc<Prepared>> = entries.iter().map(|e| Arc::clone(&e.prepared)).collect();
    let mut ctx = Ctx {
        doc,
        prepared: &prepared,
        next: 0,
        add_spacing: doc.settings.add_paragraph_spacing,
    };
    for section in &doc.sections {
        let geometry = Geometry::of(section);
        pager.start_section(geometry);
        let mut i = 0;
        let blocks = &section.blocks;
        while i < blocks.len() {
            match &blocks[i] {
                Block::Paragraph(_) => {
                    // Подряд идущие абзацы раскладываются вместе: правила
                    // «не отрывать» и «одного стиля» смотрят на соседей.
                    let run = blocks[i..]
                        .iter()
                        .take_while(|b| !matches!(b, Block::Table(_)))
                        .filter(|b| matches!(b, Block::Paragraph(_)))
                        .count();
                    let span = blocks[i..]
                        .iter()
                        .take_while(|b| !matches!(b, Block::Table(_)))
                        .count();
                    let first = ctx.next;
                    let group =
                        &prepared[first.min(prepared.len())..(first + run).min(prepared.len())];
                    ctx.next += run;
                    let spacing: Vec<ParagraphBlock<'_>> = group
                        .iter()
                        .map(|p| ParagraphBlock {
                            // Раскладке по страницам нужны только интервалы
                            // и стиль (`effective_spacing`), строки она
                            // берёт из `Prepared`.
                            lines: &[],
                            spacing: p.spacing,
                            style: p.style.as_deref(),
                        })
                        .collect();
                    for j in 0..group.len() {
                        pager.place(group, &spacing, j, first + j);
                    }
                    i += span;
                }
                Block::Table(table) => {
                    let rows = build_rows(&mut ctx, table, 0.0, geometry.text_width());
                    pager.place_table(rows);
                    i += 1;
                }
                Block::Unknown(_) => i += 1,
            }
        }
    }
    if pager.pages.is_empty() {
        pager.start_section(Geometry::of(&Section::default()));
    }
    cache.entries = entries;
    cache.rebuilt = rebuilt;
    DocumentLayout { pages: pager.pages }
}

/// Номера пунктов списков у абзацев по порядку.
fn list_labels(doc: &Document, all: &[Item<'_>]) -> Vec<Option<ListLabel>> {
    if doc.numbering.is_empty() {
        return vec![None; all.len()];
    }
    let mut counters = ListCounters::default();
    all.iter()
        .map(|item| {
            let (id, level) = doc.resolve_paragraph(&item.paragraph.props).numbering?;
            counters.next(&doc.numbering, id, level)
        })
        .collect()
}

/// Абзацы по порядку с шириной текста; абзацы таблиц — по строкам
/// и ячейкам, с шириной текста своей ячейки. `table` — стиль таблицы,
/// в ячейке которой стоят `blocks`.
fn collect_paragraphs<'a>(
    doc: &Document,
    blocks: &'a [Block],
    width: f32,
    table: Option<&str>,
    out: &mut Vec<Item<'a>>,
) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => out.push(Item {
                paragraph,
                width,
                table: table.map(str::to_string),
            }),
            Block::Table(t) => {
                let geometry = TableGeometry::of(doc, t, width);
                let style = geometry.style();
                for row in &t.rows {
                    let mut start = 0;
                    for cell in &row.cells {
                        let (_, text_width) = geometry.text_area(start, cell);
                        collect_paragraphs(doc, &cell.blocks, text_width, Some(&style), out);
                        start += cell.span();
                    }
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
pub(crate) struct Prepared {
    text: Arc<str>,
    /// Строки общие с [`PageLine::line`]: страницы не копируют глифы.
    lines: Vec<Arc<Line>>,
    /// Высота каждой строки с учётом межстрочного интервала, пт.
    pub(crate) heights: Vec<f32>,
    spans: Arc<[ResolvedRunProps]>,
    pub(crate) spacing: ParagraphSpacing,
    pub(crate) style: Option<String>,
    keep_next: bool,
    keep_lines: bool,
    page_break_before: bool,
    widow_control: bool,
    label: Option<Arc<ListMark>>,
}

/// Переводит абзац модели в типы вёрстки и разбивает на строки.
fn prepare(
    doc: &Document,
    paragraph: &Paragraph,
    label: Option<&ListLabel>,
    table: Option<&str>,
    width: f32,
    shaper: &mut Shaper,
    library: &FontLibrary,
) -> Prepared {
    let props = doc.resolve_paragraph_in(&paragraph.props, table);
    let mut spans = Vec::new();
    let mut span_props = Vec::new();
    for run in paragraph.runs() {
        let resolved = doc.resolve_run_in(&paragraph.props, &run.props, table);
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
    let mark = doc.resolve_run_in(&paragraph.props, &RunProps::default(), table);
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
    let label =
        label.map(|label| list_mark(doc, paragraph, label, table, &mut format, shaper, library));
    let mut broken = break_lines(shaper, library, &spans, &mark_style, &format);
    // Номер другим шрифтом может сделать первую строку выше.
    if let (Some((_, height)), Some(first)) = (&label, broken.lines.first_mut()) {
        first.ascent = first.ascent.max(height.ascent);
        first.descent = first.descent.max(height.descent);
        first.leading = first.leading.max(height.leading);
    }
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
        label: label.map(|(mark, _)| Arc::new(mark)),
    }
}

/// Номер пункта списка: глифы на своём месте и высота его строки.
/// Начало первой строки абзаца в `format` сдвигается за номер.
fn list_mark(
    doc: &Document,
    paragraph: &Paragraph,
    label: &ListLabel,
    table: Option<&str>,
    format: &mut ParagraphFormat,
    shaper: &mut Shaper,
    library: &FontLibrary,
) -> (ListMark, Line) {
    let level = &label.level;
    let mut run = level.run.clone();
    let symbols = |family: &Option<String>| family.as_deref().and_then(symbol_font);
    let font = symbols(&run.fonts.ascii).or_else(|| symbols(&run.fonts.h_ansi));
    let text = match font {
        // Знаки из шрифтов Symbol и Wingdings заменяются похожими знаками
        // Юникода обычным шрифтом: самих этих шрифтов обычно нет.
        Some(font) => {
            run.fonts = RunFonts::default();
            label.text.chars().map(|c| symbol_char(font, c)).collect()
        }
        None => label.text.clone(),
    };
    let props = doc.resolve_run_in(&paragraph.props, &run, table);
    let mut shown = text;
    if level.suffix == Some(LevelSuffix::Space) {
        // Неразрывный пробел: обычный «висел» бы и не считался в ширину.
        shown.push('\u{a0}');
    }
    let spans: Vec<TextSpan> = split_by_font_slot(&shown, &props.fonts)
        .into_iter()
        .map(|(piece, family)| TextSpan::new(piece, text_style(&props, family)))
        .collect();
    let mark_style = text_style(&props, &props.fonts.h_ansi);
    let wide = ParagraphFormat::new(1.0e6);
    let mut line = break_lines(shaper, library, &spans, &mark_style, &wide)
        .lines
        .into_iter()
        .next()
        .unwrap_or_else(|| Line {
            range: 0..0,
            x: 0.0,
            width: 0.0,
            ascent: 0.0,
            descent: 0.0,
            leading: 0.0,
            items: Vec::new(),
            end: LineEnd::Paragraph,
        });
    let width: f32 = line.items.iter().map(LineItem::width).sum();
    let start = format.left_indent + format.first_line_indent;
    let x = match level.alignment {
        Some(Alignment::Center) => start - width / 2.0,
        Some(Alignment::Right) => start - width,
        _ => start,
    };
    let end = x + width;
    let text_start = match level.suffix.unwrap_or(LevelSuffix::Tab) {
        LevelSuffix::Tab => {
            // Табуляция до ближайшей позиции: заданной в абзаце, отступа
            // выступа или шага по умолчанию.
            let set = format
                .tab_stops
                .iter()
                .filter(|t| t.kind != TabKind::Bar)
                .map(|t| t.position);
            let hanging = (format.first_line_indent < 0.0).then_some(format.left_indent);
            let next = set
                .chain(hanging)
                .filter(|&p| p > end + 0.01)
                .min_by(f32::total_cmp);
            next.unwrap_or_else(|| {
                let step = format.default_tab.max(1.0);
                ((end / step).floor() + 1.0) * step
            })
        }
        LevelSuffix::Space | LevelSuffix::Nothing => end,
    };
    format.first_line_indent = text_start - format.left_indent;
    for item in &mut line.items {
        match item {
            LineItem::Text(t) => {
                t.x += x;
                t.span = 0;
            }
            LineItem::Tab(t) => {
                t.x += x;
                t.span = 0;
            }
        }
    }
    let mark = ListMark {
        text: shown.trim_end_matches('\u{a0}').to_string(),
        items: std::mem::take(&mut line.items),
        props,
    };
    (mark, line)
}

/// Шрифт со знаками на месте букв, у которого знаки маркеров лежат
/// в области `U+F000`–`U+F0FF`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SymbolFont {
    Symbol,
    Wingdings,
}

fn symbol_font(family: &str) -> Option<SymbolFont> {
    let family = family.to_lowercase();
    if family == "symbol" {
        Some(SymbolFont::Symbol)
    } else if family.starts_with("wingdings") {
        Some(SymbolFont::Wingdings)
    } else {
        None
    }
}

/// Знак Юникода, похожий на знак шрифта Symbol или Wingdings.
fn symbol_char(font: SymbolFont, c: char) -> char {
    let code = c as u32;
    let byte = if (0xF020..=0xF0FF).contains(&code) {
        code - 0xF000
    } else if (0x20..=0xFF).contains(&code) {
        code
    } else {
        return c;
    };
    match (font, byte) {
        (SymbolFont::Symbol, 0xB7) => '•',
        (SymbolFont::Symbol, 0xA8) => '♦',
        (SymbolFont::Symbol, 0xA7) => '♣',
        (SymbolFont::Symbol, 0xA9) => '♥',
        (SymbolFont::Symbol, 0xAA) => '♠',
        (SymbolFont::Symbol, 0xAE) => '→',
        (SymbolFont::Symbol, 0x2D) => '−',
        (SymbolFont::Wingdings, 0x6C) => '●',
        (SymbolFont::Wingdings, 0x6E) => '■',
        (SymbolFont::Wingdings, 0x71) => '❑',
        (SymbolFont::Wingdings, 0x76) => '❖',
        (SymbolFont::Wingdings, 0x9F) => '•',
        (SymbolFont::Wingdings, 0xA1) => '○',
        (SymbolFont::Wingdings, 0xA7) => '▪',
        (SymbolFont::Wingdings, 0xA8) => '◻',
        (SymbolFont::Wingdings, 0xD8) => '➢',
        (SymbolFont::Wingdings, 0xE0) => '➢',
        (SymbolFont::Wingdings, 0xFC) => '✓',
        (SymbolFont::Wingdings, 0xFE) => '☑',
        // Остальные знаки — обычный маркер.
        _ => '•',
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
            shapes: Vec::new(),
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
            label: if k == 0 { p.label.clone() } else { None },
            cell: None,
        };
        if let Some(page) = self.pages.last_mut() {
            page.lines.push(placed);
        }
        self.y += height;
        self.has_content = true;
    }

    /// Раскладывает строки таблицы: строка, которая не влезает,
    /// переносится на следующую страницу целиком (если запрещено её
    /// разрывать или ни одна строка текста не влезает) или делится;
    /// строки-заголовки повторяются наверху каждой следующей страницы.
    fn place_table(&mut self, rows: Vec<RowBox>) {
        if self.break_pending && self.has_content {
            self.new_page(true);
        }
        let headers: Vec<RowBox> = rows.iter().take_while(|r| r.header).cloned().collect();
        let header_height: f32 = headers.iter().map(|r| r.height).sum();
        // Повторять заголовки, только если после них остаётся место.
        let repeat = !headers.is_empty() && header_height < self.page_height() / 2.0;
        // На странице только повторённые заголовки (или ничего).
        let mut fresh = !self.has_content;
        for (index, row) in rows.into_iter().enumerate() {
            let mut row = row;
            let mut continued = false;
            loop {
                let available = self.bottom() - self.y;
                if row.height <= available + EPSILON {
                    self.draw(&row, !continued, true);
                    fresh = false;
                    break;
                }
                let whole_fits = row.height <= self.page_height() - header_height;
                if !fresh && (row.cant_split || row.header) && whole_fits {
                    self.table_page(&headers, repeat && index >= headers.len());
                    fresh = true;
                    continue;
                }
                match split_row(&row, available) {
                    Some((first, rest)) => {
                        self.draw(&first, !continued, false);
                        row = rest;
                        continued = true;
                        self.table_page(&headers, repeat && index >= headers.len());
                        fresh = true;
                    }
                    None if !fresh => {
                        self.table_page(&headers, repeat && index >= headers.len());
                        fresh = true;
                    }
                    None => {
                        // На пустой странице не влезает ни одна строка
                        // текста — строка таблицы остаётся как есть.
                        self.draw(&row, !continued, true);
                        fresh = false;
                        break;
                    }
                }
            }
        }
        self.has_content = true;
        self.previous_after = 0.0;
        self.break_pending = false;
    }

    /// Новая страница посреди таблицы, с повторёнными заголовками.
    fn table_page(&mut self, headers: &[RowBox], repeat: bool) {
        self.new_page(true);
        if repeat {
            for header in headers {
                self.draw(header, true, true);
            }
        }
    }

    /// Рисует строку таблицы с текущего места и сдвигает его вниз.
    fn draw(&mut self, row: &RowBox, top_edge: bool, bottom_edge: bool) {
        let Some(g) = self.geometry else {
            return;
        };
        let top = self.y;
        let mut canvas = PageCanvas {
            page: self.pages.last_mut(),
        };
        draw_row(row, g.margins.left, top, top_edge, bottom_edge, &mut canvas);
        self.y += row.height;
        self.has_content = true;
    }
}

/// Рисование строки таблицы прямо на страницу.
struct PageCanvas<'a> {
    page: Option<&'a mut Page>,
}

impl Canvas for PageCanvas<'_> {
    fn line(&mut self, line: &BoxLine, dx: f32, dy: f32, area: CellArea) {
        let p = &line.prepared;
        let (Some(page), Some(l)) = (self.page.as_deref_mut(), p.lines.get(line.k)) else {
            return;
        };
        let top = line.y + dy;
        page.lines.push(PageLine {
            line: Arc::clone(l),
            x: line.x + dx,
            top,
            height: line.height,
            baseline: top + p.spacing.line.baseline_offset(l, line.height),
            paragraph: line.paragraph,
            text: p.text.clone(),
            spans: p.spans.clone(),
            label: if line.k == 0 { p.label.clone() } else { None },
            cell: Some(match line.cell {
                Some(inner) => inner.moved(dx, dy),
                None => area,
            }),
        });
    }

    fn shape(&mut self, shape: Shape) {
        if let Some(page) = self.page.as_deref_mut() {
            page.shapes.push(shape);
        }
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

    /// Таблица: строки по ячейкам, текст ячеек через `|`, ширина
    /// столбцов 50 пт, сплошные границы.
    fn table(rows: &[&[&str]]) -> letter_model::Table {
        use letter_model::{Border, Borders, Table, TableCell, TableProps, TableRow};
        let columns = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        Table {
            props: Box::new(TableProps {
                borders: Some(Borders::all(Border::single(8))),
                ..TableProps::default()
            }),
            grid: vec![Twips(1000); columns],
            raw_props: Vec::new(),
            rows: rows
                .iter()
                .map(|cells| TableRow {
                    cells: cells
                        .iter()
                        .map(|text| TableCell {
                            blocks: vec![para(text).into()],
                            ..TableCell::default()
                        })
                        .collect(),
                    ..TableRow::default()
                })
                .collect(),
        }
    }

    fn doc_with(blocks: Vec<Block>) -> Document {
        let mut d = doc(Vec::new());
        d.sections[0].blocks = blocks;
        d
    }

    #[test]
    fn table_cells_stand_side_by_side() {
        let d = doc_with(vec![
            para("before").into(),
            table(&[&["a", "b|c"], &["d", "e"]]).into(),
            para("after").into(),
        ]);
        let layout = lay(&d);
        let lines = &layout.pages[0].lines;
        let found: Vec<(usize, f32, f32)> =
            lines.iter().map(|l| (l.paragraph, l.x, l.top)).collect();
        // Текст первого столбца — на поле (Word 2010 и старше), второго —
        // через 50 пт; строка таблицы высотой с самую высокую ячейку.
        let close = |a: f32, b: f32| (a - b).abs() < 1e-3;
        assert_eq!(found.len(), 7);
        assert!(close(found[1].1, 0.0) && close(found[1].2, LINE));
        assert!(close(found[2].1, 50.0) && close(found[2].2, LINE));
        assert!(close(found[3].2, 2.0 * LINE)); // «c» — вторая строка ячейки
        assert!(close(found[4].2, 3.0 * LINE) && found[4].0 == 3);
        assert!(close(found[6].2, 4.0 * LINE) && found[6].0 == 5);
        // Строки ячеек знают свою ячейку; у строки вне таблицы её нет.
        let cell = lines[2].cell.expect("ячейка");
        assert!(close(cell.left, 50.0 - 5.4) && close(cell.right, 100.0 - 5.4));
        assert!(lines[0].cell.is_none());
        // Границы: 2×2 ячейки, у каждой линия сверху и слева, у правых —
        // справа, у нижних — снизу.
        let rules = layout.pages[0]
            .shapes
            .iter()
            .filter(|s| s.kind == ShapeKind::Rule)
            .count();
        assert_eq!(rules, 4 * 2 + 2 + 2);
        // В новом режиме Word граница — на поле, текст правее.
        let mut d = d;
        d.settings.compatibility_mode = 15;
        let layout = lay(&d);
        assert!(close(layout.pages[0].lines[1].x, 5.4));
    }

    #[test]
    fn table_rows_flow_onto_new_pages_with_headers() {
        let mut t = table(&[&["H"], &["1|1"], &["2|2"], &["3|3"]]);
        t.rows[0].props.header = Some(true);
        let layout = lay(&doc_with(vec![t.clone().into()]));
        // На странице 5 строк: заголовок и две строки таблицы, на
        // следующей — заголовок ещё раз и последняя строка.
        assert_eq!(pages(&layout), [vec![0, 1, 1, 2, 2], vec![0, 3, 3]]);
        // Без заголовка длинная строка делится между страницами.
        t.rows[0].props.header = None;
        t.rows[3].cells[0].blocks = vec![para("a|b|c").into()];
        let layout = lay(&doc_with(vec![t.clone().into()]));
        assert_eq!(pages(&layout), [vec![0, 1, 1, 2, 2], vec![3, 3, 3]]);
        let mut first = singles(3);
        first.push(para("x|x"));
        let mut blocks: Vec<Block> = first.into_iter().map(Block::from).collect();
        let mut short = table(&[&["a|b|c"]]);
        blocks.push(short.clone().into());
        // Строку можно разорвать: часть остаётся внизу.
        assert_eq!(
            pages(&lay(&doc_with(blocks.clone()))),
            [vec![0, 1, 2, 3, 3], vec![4, 4, 4]]
        );
        // «Не разрывать строку»: строка целиком на следующей странице.
        short.rows[0].props.cant_split = Some(true);
        blocks.pop();
        blocks.pop();
        blocks.push(para("x").into());
        blocks.push(short.into());
        assert_eq!(
            pages(&lay(&doc_with(blocks))),
            [vec![0, 1, 2, 3], vec![4, 4, 4]]
        );
    }

    #[test]
    fn row_height_rules_and_merged_cells() {
        use letter_model::{HeightRule, RowHeight, VerticalMerge};
        let mut t = table(&[&["a", "b"], &["c", "d"]]);
        t.rows[0].props.height = Some(RowHeight {
            value: Twips(600), // 30 пт
            rule: HeightRule::AtLeast,
        });
        let layout = lay(&doc_with(vec![t.clone().into()]));
        assert!((layout.pages[0].lines[2].top - 30.0).abs() < 1e-3);
        // Объединённая по вертикали ячейка: её текст выше двух строк —
        // последняя строка объединения растягивается.
        t.rows[0].props.height = None;
        t.rows[0].cells[0].props.v_merge = Some(VerticalMerge::Restart);
        t.rows[0].cells[0].blocks = vec![para("1|2|3").into()];
        t.rows[1].cells[0].props.v_merge = Some(VerticalMerge::Continue);
        t.rows[1].cells[0].blocks = vec![Paragraph::default().into()];
        let d = doc_with(vec![t.into(), para("after").into()]);
        let layout = lay(&d);
        let after = layout.pages[0]
            .lines
            .iter()
            .find(|l| l.paragraph == 4)
            .expect("абзац после таблицы");
        assert!((after.top - 3.0 * LINE).abs() < 1e-3);
    }

    #[test]
    fn nested_tables_and_shading() {
        use letter_model::{Color, Shading, TableCell};
        let inner = table(&[&["x", "y"]]);
        let mut outer = table(&[&["outer"]]);
        outer.grid = vec![Twips(3000)];
        outer.rows[0].cells[0] = TableCell {
            blocks: vec![para("outer").into(), inner.into()],
            ..TableCell::default()
        };
        outer.rows[0].cells[0].props.shading = Some(Shading::fill(Color::Rgb(200, 0, 0)));
        let layout = lay(&doc_with(vec![outer.into()]));
        let page = &layout.pages[0];
        assert_eq!(page.lines.len(), 3);
        // Вложенная таблица — под абзацем; её текст, как и у внешней,
        // начинается на краю текста ячейки.
        let x = &page.lines[1];
        let y = &page.lines[2];
        assert!((y.top - LINE).abs() < 1e-3);
        assert!(x.x.abs() < 1e-3 && (y.x - 50.0).abs() < 1e-3, "{}", y.x);
        let inner_cell = y.cell.expect("ячейка");
        assert!(inner_cell.right - inner_cell.left < 51.0);
        assert!(
            page.shapes
                .iter()
                .any(|s| s.kind == ShapeKind::Fill && s.color == (200, 0, 0))
        );
    }

    #[test]
    fn table_style_formats_cell_paragraphs() {
        use letter_model::{Style, StyleKind};
        let mut d = doc_with(vec![table(&[&["a"]]).into()]);
        let mut grid = Style::new("Grid", StyleKind::Table);
        grid.paragraph.spacing.before = Some(Twips(200));
        d.styles.styles.push(grid);
        if let Some(Block::Table(t)) = d.sections[0].blocks.first_mut() {
            t.props.style = Some("Grid".into());
        }
        let layout = lay(&d);
        assert!((layout.pages[0].lines[0].top - 10.0).abs() < 1e-3);
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

    /// Документ со списком «%1.» с отступом 36 пт и выступом 18 пт.
    fn list_doc(paragraphs: Vec<Paragraph>, suffix: Option<LevelSuffix>) -> Document {
        use letter_model::{
            AbstractNumbering, Indentation, ListLevel, NumberFormat, NumberingInstance,
        };
        let mut doc = doc(paragraphs);
        doc.numbering = Numbering {
            abstracts: vec![AbstractNumbering {
                id: 0,
                levels: vec![ListLevel {
                    start: Some(1),
                    format: Some(NumberFormat::Decimal),
                    text: Some("%1.".into()),
                    suffix,
                    paragraph: letter_model::ParagraphProps {
                        indentation: Indentation {
                            left: Some(Twips(720)),
                            first_line: Some(FirstLineIndent::Hanging(Twips(360))),
                            ..Indentation::default()
                        },
                        ..Default::default()
                    },
                    ..ListLevel::default()
                }],
                ..Default::default()
            }],
            instances: vec![NumberingInstance {
                id: 1,
                abstract_id: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        doc
    }

    fn item(text: &str) -> Paragraph {
        let mut p = para(text);
        p.props.numbering = Some(letter_model::NumberingRef {
            id: Some(1),
            level: Some(0),
        });
        p
    }

    #[test]
    fn list_items_get_numbers() {
        let doc = list_doc(vec![item("a"), para("b"), item("c")], None);
        let layout = lay(&doc);
        let lines = &layout.pages[0].lines;
        let label = |i: usize| lines[i].label.as_ref().map(|l| l.text.clone());
        assert_eq!(label(0).as_deref(), Some("1."));
        assert_eq!(label(1), None);
        assert_eq!(label(2).as_deref(), Some("2."));
        // Номер — в начале выступа, текст — за табуляцией на отступе.
        let mark = lines[0].label.as_ref().map(|l| l.items[0].x());
        assert_eq!(mark, Some(18.0));
        assert_eq!(lines[0].line.x, 36.0);
        assert_eq!(lines[1].line.x, 0.0);
    }

    #[test]
    fn number_followed_by_space_or_nothing() {
        let x = |suffix| {
            let layout = lay(&list_doc(vec![item("a")], Some(suffix)));
            let line = &layout.pages[0].lines[0];
            let mark = line
                .label
                .as_ref()
                .map(|l| l.items.iter().map(LineItem::width).sum::<f32>());
            (line.line.x - 18.0, mark.unwrap_or_default())
        };
        let (nothing, width) = x(LevelSuffix::Nothing);
        assert!((nothing - width).abs() < 0.01, "{nothing} {width}");
        let (space, wider) = x(LevelSuffix::Space);
        assert!(
            wider > width && (space - wider).abs() < 0.01,
            "{space} {wider}"
        );
    }

    #[test]
    fn numbers_below_a_change_are_relaid() {
        let mut document = list_doc(vec![item("a"), item("b"), item("c")], None);
        let library = FontLibrary::embedded_only();
        let mut shaper = Shaper::new();
        let mut cache = LayoutCache::default();
        layout_document_cached(&document, &mut shaper, &library, &mut cache);
        // Первый пункт больше не в списке: номера ниже меняются.
        document.sections[0].blocks[0] = Block::from(para("a"));
        let cached = layout_document_cached(&document, &mut shaper, &library, &mut cache);
        assert_eq!(cache.rebuilt(), 3);
        assert_eq!(cached, lay(&document));
        let labels: Vec<_> = cached.pages[0]
            .lines
            .iter()
            .map(|l| l.label.as_ref().map(|l| l.text.clone()))
            .collect();
        assert_eq!(labels, [None, Some("1.".into()), Some("2.".into())]);
    }

    #[test]
    fn symbol_bullets_become_unicode() {
        assert_eq!(symbol_char(SymbolFont::Symbol, '\u{f0b7}'), '•');
        assert_eq!(symbol_char(SymbolFont::Wingdings, '\u{f0a7}'), '▪');
        assert_eq!(symbol_font("Wingdings 2"), Some(SymbolFont::Wingdings));
        assert_eq!(symbol_font("Courier New"), None);
    }
}
