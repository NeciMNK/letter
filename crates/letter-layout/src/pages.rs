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
//! - колонтитулы (верхний и нижний; первой страницы раздела, чётных
//!   страниц) верстаются отдельно и ставятся на расстоянии от края листа;
//!   высокий колонтитул отодвигает текст.
//!
//! - сноски: текст сноски стоит внизу той страницы, где ссылка на неё,
//!   под короткой чертой (место под него отнимается у текста); концевые
//!   сноски — после текста документа.
//!
//! - границы и заливка абзацев: линии стоят снаружи текста на расстоянии
//!   `w:space`, место под верхнюю и нижнюю линии добавляется к абзацу;
//!   подряд идущие абзацы с одинаковыми границами обводятся одной рамкой,
//!   между ними — линия `w:between`.
//!
//! Пока не учитываются: деление длинной сноски между страницами и
//! непонятые элементы внутри абзаца.

use std::collections::HashMap;
use std::sync::Arc;

use letter_model::{
    Alignment, Anchor, AnchorAlign, Block, Border, Document, EMU_PER_POINT, FirstLineIndent,
    HeaderFooter, HeaderFooterKind, HorizontalFrom, ImageData, LevelSuffix, LineSpacing,
    ListCounters, ListLabel, NoteKind, Numbering, PageBorderDisplay, PageBorders, PageSize,
    Paragraph, ParagraphBorders, Placement, ResolvedFonts, ResolvedParagraphProps,
    ResolvedRunProps, RunContent, RunFonts, RunProps, Section, SectionProps, SectionStart,
    Settings, Styles, TabStopKind, Theme, Twips, VerticalAlign, VerticalFrom, Watermark, Wrap,
    find_watermark,
};

use crate::fields;
use crate::fonts::FontLibrary;
use crate::lines::{
    Align, Hyphenation, InlineObject, Line, LineEnd, LineItem, LineRoom, ParagraphFormat, TabKind,
    TabLeader, TabStop, TextSpan, break_lines, break_lines_around,
};
use crate::notes::NoteNumbers;
use crate::shaping::{ShapedRun, Shaper, TextStyle};
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
    /// Заливка ячеек и абзацев, линии границ таблиц и абзацев.
    pub shapes: Vec<Shape>,
    /// Плавающие рисунки в порядке привязки.
    pub floats: Vec<Float>,
    /// Раздел, в котором началась страница (номер по порядку).
    pub section: usize,
    /// Какие колонтитулы раздела у страницы.
    pub kind: HeaderFooterKind,
    /// Верхний колонтитул.
    pub header: PageStory,
    /// Нижний колонтитул.
    pub footer: PageStory,
    /// Тексты сносок на странице: обычных — внизу, концевых — после
    /// текста документа.
    pub notes: Vec<PageNote>,
    /// Подложка из верхнего колонтитула страницы.
    pub watermark: Option<PageWatermark>,
}

/// Подложка на странице: надпись, повёрнутая вокруг середины области
/// текста.
#[derive(Debug, Clone, PartialEq)]
pub struct PageWatermark {
    /// Глифы надписи; ширины уже растянуты на ширину подложки.
    pub runs: Vec<ShapedRun>,
    /// Цвет.
    pub color: (u8, u8, u8),
    /// Непрозрачность, 0–255.
    pub alpha: u8,
    /// Начало базовой линии, пт от левого верхнего угла листа.
    pub x: f32,
    /// Начало базовой линии по вертикали.
    pub y: f32,
    /// Поворот по часовой стрелке, градусы.
    pub rotation: f32,
}

/// Подложка `mark` по середине области текста страницы.
fn page_watermark(
    mark: &Watermark,
    page: &Page,
    shaper: &mut Shaper,
    library: &FontLibrary,
) -> Option<PageWatermark> {
    if mark.text.trim().is_empty() || mark.width <= 0.0 || mark.height <= 0.0 {
        return None;
    }
    let probe = shaper.shape(
        library,
        &mark.text,
        &TextStyle::new(mark.font.clone(), 100.0),
    );
    let font = probe.first()?.font.id;
    let metrics = shaper.metrics(library, font)?;
    let em = (metrics.ascent + metrics.descent).max(0.1);
    // Как в Word, текст растягивается на прямоугольник подложки: по
    // высоте — кегль, по ширине — масштаб глифов.
    let size = mark.height / em;
    let mut runs = shaper.shape(
        library,
        &mark.text,
        &TextStyle::new(mark.font.clone(), size),
    );
    let advance: f32 = runs.iter().map(ShapedRun::width).sum();
    if advance <= 0.0 {
        return None;
    }
    let stretch = mark.width / advance;
    for run in &mut runs {
        run.width_scale *= stretch;
        for glyph in &mut run.glyphs {
            glyph.x_advance *= stretch;
            glyph.x_offset *= stretch;
        }
    }
    let m = page.margins;
    let cx = m.left + (page.width - m.left - m.right) / 2.0;
    let cy = m.top + (page.height - m.top - m.bottom) / 2.0;
    // Начало базовой линии в осях надписи (от её середины) и поворот.
    let (lx, ly) = (
        -mark.width / 2.0,
        -mark.height / 2.0 + metrics.ascent * size,
    );
    let (sin, cos) = mark.rotation.to_radians().sin_cos();
    Some(PageWatermark {
        runs,
        color: mark.color,
        alpha: (mark.opacity * 255.0).round().clamp(0.0, 255.0) as u8,
        x: cx + lx * cos - ly * sin,
        y: cy + lx * sin + ly * cos,
        rotation: mark.rotation,
    })
}

/// Текст сноски на странице. Координаты — как у текста страницы;
/// [`PageLine::paragraph`] — номер абзаца в тексте сноски.
#[derive(Debug, Clone, PartialEq)]
pub struct PageNote {
    /// Обычная или концевая.
    pub kind: NoteKind,
    /// Номер сноски в документе ([`letter_model::Note::id`]).
    pub id: String,
    /// Строки текста сноски.
    pub story: PageStory,
}

/// Колонтитул на странице. Координаты, как у текста страницы, — пт
/// от левого верхнего угла листа; [`PageLine::paragraph`] — номер абзаца
/// в колонтитуле.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PageStory {
    /// Строки.
    pub lines: Vec<PageLine>,
    /// Заливка ячеек и линии границ таблиц.
    pub shapes: Vec<Shape>,
    /// Плавающие рисунки.
    pub floats: Vec<Float>,
}

impl PageStory {
    /// Колонтитул пуст: рисовать нечего.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.shapes.is_empty() && self.floats.is_empty()
    }

    /// Колонтитул, сдвинутый вниз на `dy`.
    fn moved(self, dy: f32) -> PageStory {
        self.shifted(0.0, dy)
    }

    /// Всё содержимое, сдвинутое вправо на `dx` и вниз на `dy`.
    pub fn shifted(mut self, dx: f32, dy: f32) -> PageStory {
        for line in &mut self.lines {
            line.x += dx;
            line.top += dy;
            line.baseline += dy;
            line.cell = line.cell.map(|c| c.moved(dx, dy));
        }
        for shape in &mut self.shapes {
            shape.x += dx;
            shape.y += dy;
        }
        for float in &mut self.floats {
            float.x += dx;
            float.y += dy;
            float.paragraph_top += dy;
        }
        self
    }
}

/// Плавающий рисунок на странице. Координаты — пт от левого верхнего
/// угла листа.
#[derive(Debug, Clone, PartialEq)]
pub struct Float {
    /// Левый край.
    pub x: f32,
    /// Верхний край.
    pub y: f32,
    /// Ширина.
    pub width: f32,
    /// Высота.
    pub height: f32,
    /// Картинка (`None` — заглушка).
    pub image: Option<Arc<ImageData>>,
    /// Фигура вместо картинки.
    pub shape: Option<Arc<letter_model::Shape>>,
    /// Текст надписей фигуры от левого верхнего угла рисунка.
    pub text: Option<Arc<PageStory>>,
    /// Обтекание текстом.
    pub wrap: Wrap,
    /// За текстом (рисуется под ним).
    pub behind: bool,
    /// Расстояние до текста с каждой стороны, пт.
    pub distance: Margins,
    /// Абзац привязки (номер по порядку в документе).
    pub paragraph: usize,
    /// Где знак привязки в тексте абзаца, байты.
    pub at: usize,
    /// Верх абзаца привязки на этой странице (от него отсчитывается
    /// положение «от абзаца»).
    pub paragraph_top: f32,
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
    /// Номера сносок в абзаце: тоже зависят от текста выше.
    marks: Vec<String>,
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
    // С какого абзаца начинается каждый раздел.
    let mut section_starts = Vec::with_capacity(doc.sections.len() + 1);
    for section in &doc.sections {
        let width = section_columns(section, &Geometry::of(section))
            .first()
            .map_or(0.0, |c| c.1);
        section_starts.push(all.len());
        collect_paragraphs(doc, &section.blocks, width, None, &mut all);
    }
    section_starts.push(all.len());
    let labels = list_labels(doc, &all);
    let numbers = NoteNumbers::of(doc);
    let marks: Vec<Vec<String>> = all
        .iter()
        .map(|item| numbers.marks(item.paragraph, None))
        .collect();

    // Совпадающие начало и конец со старой вёрсткой.
    let old = std::mem::take(&mut cache.entries);
    let same = |entry: &Entry, k: usize| {
        let item = &all[k];
        entry.width == item.width.to_bits()
            && entry.paragraph == *item.paragraph
            && entry.label == labels[k]
            && entry.marks == marks[k]
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
                &marks[k],
                table,
                item.width,
                shaper,
                library,
            );
            Entry {
                paragraph: item.paragraph.clone(),
                width: item.width.to_bits(),
                label,
                marks: marks[k].clone(),
                table: item.table.clone(),
                prepared: Arc::new(prepared),
            }
        });
        entries.push(entry);
    }

    let mut pager = Pager {
        add_spacing: doc.settings.add_paragraph_spacing,
        paragraph_notes: entries.iter().map(|e| e.prepared.notes.clone()).collect(),
        ..Pager::default()
    };
    let prepared: Vec<Arc<Prepared>> = entries.iter().map(|e| Arc::clone(&e.prepared)).collect();
    let mut ctx = Ctx {
        doc,
        prepared: &prepared,
        next: 0,
        add_spacing: doc.settings.add_paragraph_spacing,
    };
    // Колонтитулы каждого раздела (свои или от предыдущего раздела),
    // свёрстанные от верха колонтитула.
    let stories = section_stories(doc, shaper, library);
    for (index, section) in doc.sections.iter().enumerate() {
        let geometry = Geometry::of(section);
        let columns = section_columns(section, &geometry);
        let column_width = columns.first().map_or(0.0, |c| c.1);
        pager.section = index;
        // Сноски, ссылки на которые в разделе, — по ширине его текста.
        let paragraphs = section_starts[index]..section_starts[index + 1];
        let ids = pager.paragraph_notes[paragraphs]
            .iter()
            .flatten()
            .map(|(_, id)| id.clone())
            .collect::<Vec<_>>();
        let (notes, separator) = lay_notes(
            doc,
            NoteKind::Footnote,
            ids.iter().map(String::as_str),
            &numbers,
            &geometry,
            shaper,
            library,
        );
        pager.notes = notes.into_iter().collect();
        pager.note_separator = separator;
        pager.bands = bands(
            section,
            &geometry,
            &stories[index],
            doc.settings.even_and_odd_headers,
        );
        pager.start_section(geometry, &section.props, columns);
        flow(
            &mut pager,
            &mut ctx,
            &section.blocks,
            column_width,
            shaper,
            library,
        );
    }
    if pager.pages.is_empty() {
        let section = Section::default();
        let geometry = Geometry::of(&section);
        let columns = section_columns(&section, &geometry);
        pager.start_section(geometry, &section.props, columns);
    }
    if let Some(geometry) = pager.geometry {
        let (endnotes, separator) = lay_notes(
            doc,
            NoteKind::Endnote,
            numbers.referenced(NoteKind::Endnote),
            &numbers,
            &geometry,
            shaper,
            library,
        );
        pager.place_endnotes(endnotes, separator);
    }
    pager.close_columns();
    pager.finish_notes();
    cache.entries = entries;
    cache.rebuilt = rebuilt;
    let mut pages = pager.pages;
    place_stories(doc, &mut pages, &stories, shaper, library);
    page_background(doc, &mut pages);
    DocumentLayout { pages }
}

/// Цвет страницы (заливка всего листа под всем остальным) и границы
/// страниц раздела.
fn page_background(doc: &Document, pages: &mut [Page]) {
    let mut previous_section = None;
    for page in pages {
        let first = previous_section != Some(page.section);
        previous_section = Some(page.section);
        if let Some(letter_model::Color::Rgb(r, g, b)) = doc.background {
            page.shapes.insert(
                0,
                Shape {
                    x: 0.0,
                    y: 0.0,
                    width: page.width,
                    height: page.height,
                    color: (r, g, b),
                    kind: ShapeKind::Fill,
                },
            );
        }
        let borders = doc
            .sections
            .get(page.section)
            .and_then(|s| s.props.page_borders.as_ref());
        if let Some(borders) = borders {
            let shown = match borders.display {
                PageBorderDisplay::AllPages => true,
                PageBorderDisplay::FirstPage => first,
                PageBorderDisplay::NotFirstPage => !first,
            };
            if shown {
                page_borders(page, borders);
            }
        }
    }
}

/// Линии границ страницы: на расстоянии `w:space` пт от края листа или
/// снаружи от текста.
fn page_borders(page: &mut Page, borders: &PageBorders) {
    let line = |b: &Option<Border>| ParagraphBorders::visible(b).cloned();
    let (top, left, bottom, right) = (
        line(&borders.top),
        line(&borders.left),
        line(&borders.bottom),
        line(&borders.right),
    );
    let width = |b: &Option<Border>| b.as_ref().map_or(0.0, Border::width_points);
    let space = |b: &Option<Border>| b.as_ref().map_or(0.0, |b| b.space.unwrap_or(0) as f32);
    let m = page.margins;
    // Внешние края рамки.
    let (x0, y0, x1, y1) = if borders.from_page_edge {
        (
            space(&left),
            space(&top),
            page.width - space(&right),
            page.height - space(&bottom),
        )
    } else {
        (
            m.left - space(&left) - width(&left),
            m.top - space(&top) - width(&top),
            page.width - m.right + space(&right) + width(&right),
            page.height - m.bottom + space(&bottom) + width(&bottom),
        )
    };
    let rule = |b: &Border, x: f32, y: f32, w: f32, h: f32| Shape {
        x,
        y,
        width: w,
        height: h,
        color: match b.color {
            Some(letter_model::Color::Rgb(r, g, b)) => (r, g, b),
            _ => (0, 0, 0),
        },
        kind: ShapeKind::Rule,
    };
    if let Some(b) = &top {
        page.shapes.push(rule(b, x0, y0, x1 - x0, b.width_points()));
    }
    if let Some(b) = &bottom {
        let w = b.width_points();
        page.shapes.push(rule(b, x0, y1 - w, x1 - x0, w));
    }
    if let Some(b) = &left {
        page.shapes.push(rule(b, x0, y0, b.width_points(), y1 - y0));
    }
    if let Some(b) = &right {
        let w = b.width_points();
        page.shapes.push(rule(b, x1 - w, y0, w, y1 - y0));
    }
}

/// Раскладывает блоки раздела или колонтитула: подряд идущие абзацы
/// вместе (правила «не отрывать» и «одного стиля» смотрят на соседей),
/// таблицы по строкам.
fn flow(
    pager: &mut Pager,
    ctx: &mut Ctx<'_>,
    blocks: &[Block],
    column_width: f32,
    shaper: &mut Shaper,
    library: &FontLibrary,
) {
    let prepared = ctx.prepared;
    let mut i = 0;
    while i < blocks.len() {
        match &blocks[i] {
            Block::Paragraph(_) => {
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
                let group = &prepared[first.min(prepared.len())..(first + run).min(prepared.len())];
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
                    pager.place(group, &spacing, j, first + j, shaper, library);
                }
                i += span;
            }
            Block::Table(table) => {
                let rows = build_rows(ctx, table, 0.0, column_width);
                pager.place_table(rows);
                i += 1;
            }
            Block::Unknown(_) => i += 1,
        }
    }
    pager.flush_frame();
}

/// Колонтитул, свёрстанный от своего верха.
#[derive(Debug, Clone)]
struct StoryBox<'d> {
    story: PageStory,
    height: f32,
    /// Содержимое колонтитула (для номеров страниц).
    blocks: &'d [Block],
}

/// Колонтитулы раздела: верхние и нижние для каждого вида страниц (см.
/// [`kind_index`]); `None` — колонтитула нет.
type SectionStories<'d> = [[Option<StoryBox<'d>>; 3]; 2];

/// Колонтитулы всех разделов. Раздел, у которого нет своего колонтитула
/// какого-то вида, берёт его у предыдущего раздела.
fn section_stories<'d>(
    doc: &'d Document,
    shaper: &mut Shaper,
    library: &FontLibrary,
) -> Vec<SectionStories<'d>> {
    let mut current: [[Option<&HeaderFooter>; 3]; 2] = Default::default();
    let mut out = Vec::with_capacity(doc.sections.len());
    for section in &doc.sections {
        let geometry = Geometry::of(section);
        let mut boxes: SectionStories = Default::default();
        for (side, set) in [&section.props.headers, &section.props.footers]
            .into_iter()
            .enumerate()
        {
            for kind in HeaderFooterKind::ALL {
                let k = kind_index(kind);
                if let Some(story) = set.get(kind) {
                    current[side][k] = Some(story);
                }
                boxes[side][k] = current[side][k].map(|story| {
                    let (laid, height) = lay_story(doc, &story.blocks, &geometry, shaper, library);
                    StoryBox {
                        story: laid,
                        height,
                        blocks: &story.blocks,
                    }
                });
            }
        }
        out.push(boxes);
    }
    out
}

/// Высота листа для вёрстки колонтитула: колонтитул не делится на
/// страницы.
const STORY_HEIGHT: f32 = 100_000.0;

/// Верстает колонтитул по ширине текста раздела.
fn lay_story(
    doc: &Document,
    blocks: &[Block],
    g: &Geometry,
    shaper: &mut Shaper,
    library: &FontLibrary,
) -> (PageStory, f32) {
    lay_story_with(
        doc,
        blocks,
        g,
        (&NoteNumbers::default(), None),
        shaper,
        library,
    )
}

/// Верстает колонтитул или текст сноски по ширине текста раздела;
/// `notes` — номера сносок и номер самой сноски (для её знака в тексте).
fn lay_story_with(
    doc: &Document,
    blocks: &[Block],
    g: &Geometry,
    notes: (&NoteNumbers, Option<&str>),
    shaper: &mut Shaper,
    library: &FontLibrary,
) -> (PageStory, f32) {
    let width = g.text_width();
    let mut items = Vec::new();
    collect_paragraphs(doc, blocks, width, None, &mut items);
    let labels = list_labels(doc, &items);
    let prepared: Vec<Arc<Prepared>> = items
        .iter()
        .zip(&labels)
        .map(|(item, label)| {
            Arc::new(prepare(
                doc,
                item.paragraph,
                label.as_ref(),
                &notes.0.marks(item.paragraph, notes.1),
                item.table.as_deref(),
                item.width,
                shaper,
                library,
            ))
        })
        .collect();
    let geometry = Geometry {
        width: g.width,
        height: STORY_HEIGHT,
        margins: Margins {
            top: 0.0,
            bottom: 0.0,
            ..g.margins
        },
    };
    let mut pager = Pager {
        add_spacing: doc.settings.add_paragraph_spacing,
        ..Pager::default()
    };
    let mut ctx = Ctx {
        doc,
        prepared: &prepared,
        next: 0,
        add_spacing: doc.settings.add_paragraph_spacing,
    };
    pager.start_section(geometry, &SectionProps::default(), vec![(0.0, width)]);
    flow(&mut pager, &mut ctx, blocks, width, shaper, library);
    pager.close_columns();
    let page = pager.pages.into_iter().next();
    let story = page.map_or_else(PageStory::default, |page| PageStory {
        lines: page.lines,
        shapes: page.shapes,
        floats: page.floats,
    });
    let height = story
        .lines
        .iter()
        .map(|l| l.top + l.height)
        .chain(story.shapes.iter().map(|s| s.y + s.height))
        .fold(0.0, f32::max);
    (story, height)
}

/// Текст надписей фигуры (и фигур её группы) размером `width` ×
/// `height` пт, свёрстанный от левого верхнего угла фигуры. `None`, если
/// текста нет. Поворот фигуры к тексту пока не применяется.
fn text_boxes(
    doc: &Document,
    shape: &letter_model::Shape,
    width: f32,
    height: f32,
    shaper: &mut Shaper,
    library: &FontLibrary,
) -> Option<Arc<PageStory>> {
    let mut out = PageStory::default();
    add_text_box(
        &mut out,
        doc,
        shape,
        [0.0, 0.0, width, height],
        shaper,
        library,
        0,
    );
    (!out.is_empty()).then(|| Arc::new(out))
}

/// Самая глубокая вложенность групп фигур, в которой ищется текст.
const MAX_SHAPE_DEPTH: usize = 16;

fn add_text_box(
    out: &mut PageStory,
    doc: &Document,
    shape: &letter_model::Shape,
    [x, y, w, h]: [f32; 4],
    shaper: &mut Shaper,
    library: &FontLibrary,
    depth: usize,
) {
    if depth > MAX_SHAPE_DEPTH || !(w.is_finite() && h.is_finite()) {
        return;
    }
    if let letter_model::Geometry::Group(group) = &shape.geometry {
        let frame = group.frame;
        if frame.width <= 0 || frame.height <= 0 {
            return;
        }
        let (sx, sy) = (w / frame.width as f32, h / frame.height as f32);
        for child in &group.children {
            let f = child.frame;
            let rect = [
                x + (f.x - frame.x) as f32 * sx,
                y + (f.y - frame.y) as f32 * sy,
                f.width as f32 * sx,
                f.height as f32 * sy,
            ];
            add_text_box(out, doc, &child.shape, rect, shaper, library, depth + 1);
        }
        return;
    }
    if shape.text.is_empty() {
        return;
    }
    let [left, top, right, bottom] = shape.text_insets.map(|v| v as f32 / EMU_PER_POINT as f32);
    let geometry = Geometry {
        width: w,
        height: STORY_HEIGHT,
        margins: Margins {
            top: 0.0,
            right,
            bottom: 0.0,
            left,
        },
    };
    if geometry.text_width() < 1.0 {
        return;
    }
    let (story, text_height) = lay_story(doc, &shape.text, &geometry, shaper, library);
    // Текст, который не влез, выходит за фигуру: при выравнивании
    // посередине — в обе стороны, как в Word.
    let free = h - top - bottom - text_height;
    let dy = top
        + match shape.text_anchor {
            letter_model::TextAnchor::Top => 0.0,
            letter_model::TextAnchor::Center => free / 2.0,
            letter_model::TextAnchor::Bottom => free,
        };
    let mut story = story.shifted(x, y + dy);
    if let Some(color) = &shape.text_color {
        let [r, g, b, _] = color.rgba(&letter_model::ColorScheme::default());
        for line in &mut story.lines {
            line.spans = line
                .spans
                .iter()
                .cloned()
                .map(|mut props| {
                    if props.color == letter_model::Color::Auto {
                        props.color = letter_model::Color::Rgb(r, g, b);
                    }
                    props
                })
                .collect();
        }
    }
    out.lines.append(&mut story.lines);
    out.shapes.append(&mut story.shapes);
    out.floats.append(&mut story.floats);
}

/// Текст сноски, свёрстанный от своего верха.
#[derive(Debug, Clone, Default)]
struct NoteBox {
    story: PageStory,
    height: f32,
}

/// Сноски вида `kind` с номерами `ids`, свёрстанные по ширине текста
/// раздела, и высота черты над ними (служебная сноска-черта документа
/// или строка по умолчанию).
fn lay_notes<'i>(
    doc: &Document,
    kind: NoteKind,
    ids: impl IntoIterator<Item = &'i str>,
    numbers: &NoteNumbers,
    g: &Geometry,
    shaper: &mut Shaper,
    library: &FontLibrary,
) -> (Vec<(String, NoteBox)>, f32) {
    let all = match kind {
        NoteKind::Footnote => &doc.footnotes,
        NoteKind::Endnote => &doc.endnotes,
    };
    let mut out: Vec<(String, NoteBox)> = Vec::new();
    for id in ids {
        if out.iter().any(|(known, _)| known == id) {
            continue;
        }
        let Some(note) = all.iter().find(|n| n.id == id && n.is_regular()) else {
            continue;
        };
        let own = numbers.number(kind, id).unwrap_or("");
        let (story, height) =
            lay_story_with(doc, &note.blocks, g, (numbers, Some(own)), shaper, library);
        out.push((id.to_string(), NoteBox { story, height }));
    }
    let separator = all
        .iter()
        .find(|n| n.special.as_deref() == Some("separator"))
        .map(|n| lay_story(doc, &n.blocks, g, shaper, library).1)
        .filter(|h| *h > EPSILON)
        .unwrap_or(DEFAULT_NOTE_SEPARATOR);
    (out, separator)
}

/// Высота строки с чертой над сносками, если в документе её нет, пт.
const DEFAULT_NOTE_SEPARATOR: f32 = 12.0;
/// Длина черты над сносками, пт (как в Word — 2 дюйма).
const NOTE_RULE_LENGTH: f32 = 144.0;

/// Место колонтитулов раздела. Колонтитул, который не влезает в поле,
/// отодвигает текст (если поле не отрицательное).
fn bands(section: &Section, g: &Geometry, stories: &SectionStories, even_and_odd: bool) -> Bands {
    let m = section.props.margins.unwrap_or_default();
    let header = points(m.header).max(0.0);
    let footer = points(m.footer).max(0.0);
    let mut bands = Bands {
        title_page: section.props.title_page,
        even_and_odd,
        ..Bands::default()
    };
    let [headers, footers] = stories;
    for (top, story) in bands.top.iter_mut().zip(headers) {
        if let Some(story) = story
            && m.top.0 >= 0
        {
            *top = header + story.height;
        }
    }
    for (bottom, story) in bands.bottom.iter_mut().zip(footers) {
        if let Some(story) = story
            && m.bottom.0 >= 0
        {
            *bottom = g.height - footer - story.height;
        }
    }
    bands
}

/// Ставит колонтитулы на страницы: верхний — на расстоянии от верха
/// листа, нижний — так, чтобы его низ был на расстоянии от низа листа.
fn place_stories(
    doc: &Document,
    pages: &mut [Page],
    stories: &[SectionStories<'_>],
    shaper: &mut Shaper,
    library: &FontLibrary,
) {
    let count = pages.len();
    for (index, page) in pages.iter_mut().enumerate() {
        let Some(section) = doc.sections.get(page.section) else {
            continue;
        };
        let Some(boxes) = stories.get(page.section) else {
            continue;
        };
        let m = section.props.margins.unwrap_or_default();
        let k = kind_index(page.kind);
        // Колонтитул с номером страницы верстается для каждой страницы.
        let mut on_page = |story: &StoryBox<'_>| match fields::with_page_numbers(
            story.blocks,
            index + 1,
            count,
        ) {
            Some(blocks) => lay_story(doc, &blocks, &Geometry::of(section), shaper, library),
            None => (story.story.clone(), story.height),
        };
        if let Some(story) = &boxes[0][k] {
            let (laid, _) = on_page(story);
            page.header = laid.moved(points(m.header).max(0.0));
        }
        if let Some(story) = &boxes[1][k] {
            let (laid, height) = on_page(story);
            let top = page.height - points(m.footer).max(0.0) - height;
            page.footer = laid.moved(top);
        }
        let mark = boxes[0][k].as_ref().and_then(|s| find_watermark(s.blocks));
        page.watermark = mark.and_then(|mark| page_watermark(&mark, page, shaper, library));
    }
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

/// Где в строке знак с этим смещением, пт от левого поля.
fn anchor_x(line: &Line, at: usize) -> f32 {
    line.items
        .iter()
        .find_map(|item| match item {
            LineItem::Object(o) if o.at == at => Some(o.x),
            _ => None,
        })
        .unwrap_or(line.x)
}

/// Место плавающего рисунка на странице: `line_top` — верх строки
/// с привязкой, `paragraph_top` — верх абзаца, `char_x` — знак привязки
/// (пт от края листа).
fn place_float(
    spec: &FloatSpec,
    g: &Geometry,
    line_top: f32,
    paragraph_top: f32,
    char_x: f32,
    paragraph: usize,
) -> Float {
    let m = g.margins;
    let (h_start, h_size) = match spec.anchor.horizontal.relative {
        HorizontalFrom::Margin | HorizontalFrom::Column => (m.left, g.text_width()),
        HorizontalFrom::Page => (0.0, g.width),
        HorizontalFrom::LeftMargin | HorizontalFrom::InsideMargin => (0.0, m.left),
        HorizontalFrom::RightMargin | HorizontalFrom::OutsideMargin => (g.width - m.right, m.right),
        HorizontalFrom::Character => (char_x, 0.0),
    };
    let (v_start, v_size) = match spec.anchor.vertical.relative {
        VerticalFrom::Margin => (m.top, g.height - m.top - m.bottom),
        VerticalFrom::Page => (0.0, g.height),
        VerticalFrom::Paragraph => (paragraph_top, 0.0),
        VerticalFrom::Line => (line_top, 0.0),
        VerticalFrom::TopMargin | VerticalFrom::InsideMargin => (0.0, m.top),
        VerticalFrom::BottomMargin | VerticalFrom::OutsideMargin => (g.height - m.bottom, m.bottom),
    };
    let points = |emu: i64| emu as f32 / EMU_PER_POINT as f32;
    let place = |placement: Placement, start: f32, size: f32, length: f32| match placement {
        Placement::Offset(emu) => start + points(emu),
        Placement::Align(AnchorAlign::Start | AnchorAlign::Inside) => start,
        Placement::Align(AnchorAlign::Center) => start + (size - length) / 2.0,
        Placement::Align(AnchorAlign::End | AnchorAlign::Outside) => start + size - length,
    };
    let anchor = &spec.anchor;
    Float {
        x: place(anchor.horizontal.placement, h_start, h_size, spec.width),
        y: place(anchor.vertical.placement, v_start, v_size, spec.height),
        width: spec.width,
        height: spec.height,
        image: spec.image.clone(),
        shape: spec.shape.clone(),
        text: spec.text.clone(),
        wrap: anchor.wrap,
        behind: anchor.wrap == Wrap::None && anchor.behind_text,
        distance: Margins {
            top: points(anchor.distance.top),
            right: points(anchor.distance.right),
            bottom: points(anchor.distance.bottom),
            left: points(anchor.distance.left),
        },
        paragraph,
        at: spec.at,
        paragraph_top,
    }
}

/// Колонки раздела: левый край от левого поля и ширина, пт.
fn section_columns(section: &Section, g: &Geometry) -> Vec<(f32, f32)> {
    let width = g.text_width().max(1.0);
    match &section.props.columns {
        Some(columns) if columns.count > 1 || !columns.widths.is_empty() => columns
            .layout(Twips((width * 20.0).round() as i32))
            .into_iter()
            .map(|(x, w)| (points(x), points(w)))
            .collect(),
        _ => vec![(0.0, width)],
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
#[derive(Clone)]
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
    /// Плавающие рисунки абзаца.
    floats: Vec<FloatSpec>,
    /// Из чего заново разбить абзац на строки вокруг плавающих рисунков.
    source: Arc<LineSource>,
    /// Пропуск над каждой строкой, пт (строка ушла ниже рисунка, рядом
    /// с которым ей не хватило места). Пусто — пропусков нет.
    gaps: Vec<f32>,
    /// Ссылки на обычные сноски: место в тексте (байты) и номер сноски.
    notes: Vec<(usize, String)>,
    /// Границы и заливка абзаца (вне таблиц).
    frame: Option<Arc<Frame>>,
}

/// Границы и заливка абзаца: видимые линии, цвет заливки и отступы
/// текста от краёв колонки, пт. Подряд идущие абзацы с одинаковыми
/// границами обводятся одной рамкой.
#[derive(Debug, Clone, PartialEq)]
struct Frame {
    top: Option<Border>,
    left: Option<Border>,
    bottom: Option<Border>,
    right: Option<Border>,
    between: Option<Border>,
    fill: Option<(u8, u8, u8)>,
    indent_left: f32,
    indent_right: f32,
}

impl Frame {
    /// Рамка абзаца с оформлением `props`; `None`, если нет ни линий,
    /// ни заливки.
    fn of(props: &ResolvedParagraphProps) -> Option<Frame> {
        let b = &props.borders;
        let visible = |side: &Option<Border>| ParagraphBorders::visible(side).cloned();
        let frame = Frame {
            top: visible(&b.top),
            left: visible(&b.left),
            bottom: visible(&b.bottom),
            right: visible(&b.right),
            between: visible(&b.between),
            fill: props.shading.as_ref().and_then(|s| s.background()),
            indent_left: points(props.indent_left),
            indent_right: points(props.indent_right),
        };
        let lines = [&frame.top, &frame.left, &frame.bottom, &frame.right];
        (lines.iter().any(|l| l.is_some()) || frame.fill.is_some()).then_some(frame)
    }

    /// Толщина линии вместе с её отступом от текста, пт.
    fn pad(border: &Option<Border>) -> f32 {
        border
            .as_ref()
            .map_or(0.0, |b| b.width_points() + b.space.unwrap_or(0) as f32)
    }
}

/// Рамка абзацев, начатая на странице: собирается, пока следующие
/// абзацы с той же рамкой идут подряд, и рисуется целиком.
struct FrameRun {
    frame: Arc<Frame>,
    page: usize,
    /// Края текста, пт от угла листа.
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    /// Рисовать линию сверху и снизу (рамка не продолжается с прошлой
    /// страницы и не уходит на следующую).
    top_edge: bool,
    bottom_edge: bool,
    /// Где линии между абзацами.
    between: Vec<f32>,
}

/// Абзац `p` с местом под линии рамки: `top` пт над первой строкой
/// и `bottom` под строкой `last`.
fn padded(p: Arc<Prepared>, top: f32, bottom: f32, last: usize) -> Arc<Prepared> {
    if top <= 0.0 && bottom <= 0.0 || p.lines.is_empty() {
        return p;
    }
    let mut p = Arc::unwrap_or_clone(p);
    let last = last.min(p.lines.len() - 1);
    p.gaps.resize(p.lines.len(), 0.0);
    if let (Some(gap), Some(height)) = (p.gaps.first_mut(), p.heights.first_mut()) {
        *gap += top;
        *height += top;
    }
    if let Some(height) = p.heights.get_mut(last) {
        *height += bottom;
    }
    Arc::new(p)
}

/// Текст и оформление абзаца для разбиения на строки.
#[derive(Debug)]
struct LineSource {
    spans: Vec<TextSpan>,
    mark_style: TextStyle,
    format: ParagraphFormat,
    /// Высота строки номера списка: подъём, спуск и интерлиньяж.
    label: Option<(f32, f32, f32)>,
}

/// Место, которое занимает плавающий рисунок с обтеканием, вместе
/// с расстоянием до текста; пт от угла листа.
#[derive(Debug, Clone, Copy)]
struct Obstacle {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    /// Обтекание «сверху и снизу»: рядом с рисунком текста нет.
    whole_width: bool,
}

impl Obstacle {
    fn of(float: &Float) -> Option<Obstacle> {
        let d = float.distance;
        (float.wrap != Wrap::None).then_some(Obstacle {
            left: float.x - d.left,
            right: float.x + float.width + d.right,
            top: float.y - d.top,
            bottom: float.y + float.height + d.bottom,
            whole_width: float.wrap == Wrap::TopAndBottom,
        })
    }
}

/// Номер колонки, в которой стоит последняя из строк `lines`.
fn column_of_last(lines: &[PageLine], columns: &[(f32, f32)], left: f32) -> usize {
    let x = lines.last().map_or(left, |l| l.x);
    columns
        .iter()
        .position(|c| (left + c.0 - x).abs() < EPSILON)
        .unwrap_or(0)
}

/// Толщина линии между колонками, пт.
const SEPARATOR_WIDTH: f32 = 0.5;

/// Самое узкое место рядом с рисунком, в которое ещё ставится текст, пт.
const MIN_ROOM: f32 = 18.0;

/// Самый широкий свободный от `obstacles` отрезок внутри `from..to`.
fn widest_gap(from: f32, to: f32, obstacles: &[&Obstacle]) -> (f32, f32) {
    let mut blocked: Vec<(f32, f32)> = obstacles
        .iter()
        .map(|o| {
            if o.whole_width {
                (from, to)
            } else {
                (o.left, o.right)
            }
        })
        .collect();
    blocked.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut best = (from, from);
    let mut x = from;
    for (left, right) in blocked {
        if left > x && left.min(to) - x > best.1 - best.0 {
            best = (x, left.min(to));
        }
        x = x.max(right);
    }
    if to - x > best.1 - best.0 {
        best = (x, to);
    }
    best
}

/// Плавающий рисунок абзаца до раскладки по страницам.
#[derive(Debug, Clone)]
struct FloatSpec {
    /// Где знак привязки в тексте абзаца, байты.
    at: usize,
    width: f32,
    height: f32,
    image: Option<Arc<ImageData>>,
    shape: Option<Arc<letter_model::Shape>>,
    text: Option<Arc<PageStory>>,
    anchor: Anchor,
}

/// Добавляет кусок текста фрагмента: с учётом «все прописные» и разных
/// шрифтов для разных видов письменности.
fn push_text(
    spans: &mut Vec<TextSpan>,
    span_props: &mut Vec<ResolvedRunProps>,
    text: &str,
    resolved: &ResolvedRunProps,
) {
    for (shown, small) in letter_case(text, resolved) {
        for (piece, family) in split_by_font_slot(&shown, &resolved.fonts) {
            let mut style = text_style(resolved, family);
            if small {
                style.size *= SMALL_CAPS_SCALE;
            }
            spans.push(TextSpan::new(piece, style));
            span_props.push(resolved.clone());
        }
    }
}

/// Переводит абзац модели в типы вёрстки и разбивает на строки.
#[allow(clippy::too_many_arguments)]
fn prepare(
    doc: &Document,
    paragraph: &Paragraph,
    label: Option<&ListLabel>,
    marks: &[String],
    table: Option<&str>,
    width: f32,
    shaper: &mut Shaper,
    library: &FontLibrary,
) -> Prepared {
    let mut marks = marks.iter();
    let props = doc.resolve_paragraph_in(&paragraph.props, table);
    let mut spans = Vec::new();
    let mut span_props = Vec::new();
    let mut floats = Vec::new();
    let mut notes = Vec::new();
    for run in paragraph.runs() {
        let resolved = doc.resolve_run_in(&paragraph.props, &run.props, table);
        let mut text = String::new();
        for item in &run.content {
            match item {
                RunContent::Text(t) => text.push_str(t),
                RunContent::Tab => text.push('\t'),
                RunContent::Break(letter_model::BreakKind::Line) => text.push('\n'),
                RunContent::Break(letter_model::BreakKind::Column) => text.push('\u{b}'),
                RunContent::Break(_) => text.push('\u{c}'),
                RunContent::Picture(picture) => {
                    push_text(
                        &mut spans,
                        &mut span_props,
                        &std::mem::take(&mut text),
                        &resolved,
                    );
                    let boxes = picture.shape.as_deref().and_then(|shape| {
                        text_boxes(
                            doc,
                            shape,
                            picture.width_points(),
                            picture.height_points(),
                            shaper,
                            library,
                        )
                    });
                    if let Some(anchor) = &picture.anchor {
                        floats.push(FloatSpec {
                            at: spans.iter().map(|s: &TextSpan| s.text.len()).sum(),
                            width: picture.width_points(),
                            height: picture.height_points(),
                            image: picture.image.clone(),
                            shape: picture.shape.clone(),
                            text: boxes.clone(),
                            anchor: (**anchor).clone(),
                        });
                    }
                    let object = InlineObject {
                        width: picture.width_points(),
                        height: picture.height_points(),
                        image: picture.image.clone(),
                        shape: picture.shape.clone(),
                        text: boxes,
                        floating: picture.anchor.is_some(),
                    };
                    let style = text_style(&resolved, &resolved.fonts.h_ansi);
                    spans.push(TextSpan::object(object, style));
                    span_props.push(resolved.clone());
                }
                // Номер сноски — надпись вместо знака в тексте.
                RunContent::NoteReference(letter_model::NoteReference {
                    custom_mark: false,
                    ..
                })
                | RunContent::NoteMark(_) => {
                    push_text(
                        &mut spans,
                        &mut span_props,
                        &std::mem::take(&mut text),
                        &resolved,
                    );
                    if let RunContent::NoteReference(r) = item
                        && r.kind == NoteKind::Footnote
                    {
                        notes.push((spans.iter().map(|s| s.text.len()).sum(), r.id.clone()));
                    }
                    let number = marks.next().map_or("?", String::as_str);
                    let style = text_style(&resolved, &resolved.fonts.h_ansi);
                    spans.push(TextSpan::label(number, style));
                    span_props.push(resolved.clone());
                }
                // Свой знак сноски — обычный текст после ссылки.
                RunContent::NoteReference(r) => {
                    if r.kind == NoteKind::Footnote {
                        push_text(
                            &mut spans,
                            &mut span_props,
                            &std::mem::take(&mut text),
                            &resolved,
                        );
                        notes.push((spans.iter().map(|s| s.text.len()).sum(), r.id.clone()));
                    }
                }
                // Знак из шрифта — надпись похожим знаком Юникода.
                RunContent::Symbol(symbol) => {
                    push_text(
                        &mut spans,
                        &mut span_props,
                        &std::mem::take(&mut text),
                        &resolved,
                    );
                    let style = text_style(&resolved, &resolved.fonts.h_ansi);
                    spans.push(TextSpan::label(symbol.unicode().to_string(), style));
                    span_props.push(resolved.clone());
                }
                // Код поля не виден, видно только его результат.
                RunContent::FieldChar(_)
                | RunContent::FieldCode(_)
                | RunContent::CommentReference(_)
                | RunContent::Unknown(_) => {}
            }
        }
        push_text(&mut spans, &mut span_props, &text, &resolved);
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
    let settings = &doc.settings;
    format.hyphenation =
        (settings.auto_hyphenation && !props.suppress_auto_hyphens).then(|| Hyphenation {
            zone: points(settings.hyphenation_zone),
            limit: settings.consecutive_hyphen_limit,
            caps: !settings.do_not_hyphenate_caps,
        });
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
    let label_height = label
        .as_ref()
        .map(|(_, l)| (l.ascent, l.descent, l.leading));
    raise_first_line(&mut broken.lines, label_height);
    let frame = if table.is_none() {
        Frame::of(&props).map(Arc::new)
    } else {
        None
    };
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
        floats,
        source: Arc::new(LineSource {
            spans,
            mark_style,
            format,
            label: label_height,
        }),
        gaps: Vec::new(),
        notes,
        frame,
    }
}

/// Номер другим шрифтом может сделать первую строку выше.
fn raise_first_line(lines: &mut [Line], label: Option<(f32, f32, f32)>) {
    if let (Some((ascent, descent, leading)), Some(first)) = (label, lines.first_mut()) {
        first.ascent = first.ascent.max(ascent);
        first.descent = first.descent.max(descent);
        first.leading = first.leading.max(leading);
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
    let symbols =
        |family: &Option<String>| family.clone().filter(|f| letter_model::is_symbol_font(f));
    let font = symbols(&run.fonts.ascii).or_else(|| symbols(&run.fonts.h_ansi));
    let text = match font {
        // Знаки из шрифтов Symbol и Wingdings заменяются похожими знаками
        // Юникода обычным шрифтом: самих этих шрифтов обычно нет.
        Some(font) => {
            run.fonts = RunFonts::default();
            label
                .text
                .chars()
                .map(|c| letter_model::symbol_char(&font, c))
                .collect()
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
            object_height: 0.0,
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
            LineItem::Object(o) => {
                o.x += x;
                o.span = 0;
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
    /// Верх абзаца на странице (для рисунков «от абзаца»): номер абзаца,
    /// номер страницы и верх его первой строки на ней.
    paragraph_top: Option<(usize, usize, f32)>,
    /// Колонки текущего раздела: левый край от поля и ширина, пт.
    columns: Vec<(f32, f32)>,
    /// Колонка, в которой идёт текст.
    column: usize,
    /// Верх колонок на странице (ниже верхнего поля, если раздел начался
    /// посреди страницы).
    column_top: f32,
    /// Самый низ текста в колонках на странице.
    column_bottom: f32,
    /// Линия между колонками.
    separator: bool,
    /// Предыдущий абзац кончился разрывом колонки: следующий начнёт
    /// новую колонку.
    column_pending: bool,
    /// С какой строки и какой фигуры страницы начались колонки на ней.
    region_start: (usize, usize),
    /// Номер текущего раздела.
    section: usize,
    /// Сколько страниц у текущего раздела.
    section_pages: usize,
    /// Место колонтитулов текущего раздела.
    bands: Bands,
    /// Верх и низ текста на текущей странице (с колонтитулами).
    page_top: Option<f32>,
    page_bottom: Option<f32>,
    /// Сноски текущего раздела, свёрстанные по ширине его текста.
    notes: HashMap<String, NoteBox>,
    /// Высота строки с чертой над сносками текущего раздела.
    note_separator: f32,
    /// Ссылки на сноски в абзацах документа (по номеру абзаца).
    paragraph_notes: Vec<Vec<(usize, String)>>,
    /// Сноски, ссылки на которые уже стоят на текущей странице.
    page_notes: Vec<(String, NoteBox)>,
    /// Место под сноски внизу текущей страницы (с чертой), пт.
    notes_height: f32,
    /// Рамка абзацев, которая ещё может продолжиться.
    frame_run: Option<FrameRun>,
}

/// Сколько места занимают колонтитулы раздела: для каждого вида страниц
/// (см. [`kind_index`]) верх и низ текста, пт от верха листа.
#[derive(Debug, Clone, Copy)]
struct Bands {
    title_page: bool,
    even_and_odd: bool,
    top: [f32; 3],
    bottom: [f32; 3],
}

impl Default for Bands {
    fn default() -> Bands {
        Bands {
            title_page: false,
            even_and_odd: false,
            top: [0.0; 3],
            bottom: [f32::INFINITY; 3],
        }
    }
}

/// Номер вида колонтитула в массивах.
fn kind_index(kind: HeaderFooterKind) -> usize {
    match kind {
        HeaderFooterKind::Default => 0,
        HeaderFooterKind::First => 1,
        HeaderFooterKind::Even => 2,
    }
}

impl Pager {
    /// Начинает раздел: с новой страницы (чётной или нечётной — тогда,
    /// может быть, после пустой) или на текущей, ниже текста предыдущего
    /// раздела («на текущей странице» при том же размере листа).
    fn start_section(
        &mut self,
        geometry: Geometry,
        props: &SectionProps,
        columns: Vec<(f32, f32)>,
    ) {
        let start = props.start.unwrap_or_default();
        let same_sheet = self.pages.last().is_some_and(|p| {
            (p.width - geometry.width).abs() < EPSILON
                && (p.height - geometry.height).abs() < EPSILON
        });
        let separator = props.columns.as_ref().is_some_and(|c| c.separator);
        match start {
            SectionStart::Continuous | SectionStart::NextColumn if same_sheet => {
                self.balance_columns();
                self.close_columns();
                let top = self.column_bottom.max(self.y);
                self.geometry = Some(geometry);
                self.columns = columns;
                self.separator = separator;
                self.column = 0;
                self.column_top = top;
                self.column_bottom = top;
                self.y = top;
                self.region_start = self
                    .pages
                    .last()
                    .map_or((0, 0), |p| (p.lines.len(), p.shapes.len()));
            }
            _ => {
                self.close_columns();
                self.geometry = Some(geometry);
                self.columns = columns;
                self.separator = separator;
                self.section_pages = 0;
                self.new_page(false);
                let even = self.pages.len().is_multiple_of(2);
                let wrong = match start {
                    SectionStart::EvenPage => !even,
                    SectionStart::OddPage => even,
                    _ => false,
                };
                // Чётность не та: эта страница остаётся пустой.
                if wrong {
                    self.new_page(false);
                }
            }
        }
    }

    /// Левый край текущей колонки, пт от края листа.
    fn column_x(&self) -> f32 {
        self.geometry.map_or(0.0, |g| g.margins.left)
            + self.columns.get(self.column).map_or(0.0, |c| c.0)
    }

    /// Ширина текущей колонки, пт.
    fn column_width(&self) -> f32 {
        self.columns
            .get(self.column)
            .map(|c| c.1)
            .or_else(|| self.geometry.map(|g| g.text_width()))
            .unwrap_or(0.0)
    }

    /// Следующая колонка, а после последней — новая страница.
    fn next_column_or_page(&mut self, soft: bool) {
        if self.column + 1 < self.columns.len() {
            self.column_bottom = self.column_bottom.max(self.y);
            self.column += 1;
            self.y = self.column_top;
            self.has_content = false;
            self.soft_top = soft;
            self.break_pending = false;
            self.column_pending = false;
            self.previous_after = 0.0;
        } else {
            self.new_page(soft);
        }
    }

    /// Выравнивает колонки по высоте перед разделом «на текущей
    /// странице», как Word: строки текущей страницы, начиная с начала
    /// колонок, раскладываются заново так, чтобы колонки вышли примерно
    /// одной высоты. Колонки с таблицами и рисунками остаются как есть.
    fn balance_columns(&mut self) {
        let count = self.columns.len();
        let (first_line, first_shape) = self.region_start;
        let left = self.geometry.map_or(0.0, |g| g.margins.left);
        let (top, bottom) = (self.column_top, self.bottom());
        let columns = self.columns.clone();
        let Some(page) = self.pages.last_mut() else {
            return;
        };
        if count < 2
            || self.column == 0 && self.y <= top + EPSILON
            || page.shapes.len() > first_shape
            || !page.floats.is_empty()
            || page.lines.len() <= first_line
        {
            return;
        }
        // Место каждой строки по порядку: промежуток над ней в её колонке
        // и высота.
        let lines = &mut page.lines[first_line..];
        let mut slots = Vec::with_capacity(lines.len());
        let mut previous: Option<(f32, f32)> = None;
        for line in lines.iter() {
            let gap = match previous {
                Some((x, bottom)) if (x - line.x).abs() < EPSILON => (line.top - bottom).max(0.0),
                _ => 0.0,
            };
            slots.push((gap, line.height));
            previous = Some((line.x, line.top + line.height));
        }
        let total: f32 = slots.iter().map(|(g, h)| g + h).sum();
        let tallest = slots.iter().map(|s| s.1).fold(0.0, f32::max);
        let target = (total / count as f32).max(tallest);
        let mut placed = Vec::with_capacity(slots.len());
        let (mut column, mut y) = (0, 0.0);
        for (i, &(gap, height)) in slots.iter().enumerate() {
            let gap = if y == 0.0 { 0.0 } else { gap };
            if i > 0 && y + gap + height > target + EPSILON && column + 1 < count {
                column += 1;
                y = 0.0;
            }
            let gap = if y == 0.0 { 0.0 } else { gap };
            placed.push((column, top + y + gap));
            y += gap + height;
        }
        let lowest = placed
            .iter()
            .zip(&slots)
            .map(|((_, t), (_, h))| t + h)
            .fold(top, f32::max);
        if lowest > bottom + EPSILON {
            return;
        }
        for (line, (column, new_top)) in lines.iter_mut().zip(placed) {
            let shift = new_top - line.top;
            line.top = new_top;
            line.baseline += shift;
            line.x = left + columns.get(column).map_or(0.0, |c| c.0);
        }
        self.column = column_of_last(&page.lines[first_line..], &columns, left);
        self.column_bottom = lowest;
        self.y = lowest;
    }

    /// Линии между заполненными колонками на текущей странице. После
    /// этого текст снова идёт с первой колонки.
    fn close_columns(&mut self) {
        let bottom = self.column_bottom.max(self.y);
        let used = std::mem::take(&mut self.column);
        if !self.separator || used == 0 || bottom <= self.column_top + EPSILON {
            return;
        }
        let left = self.geometry.map_or(0.0, |g| g.margins.left);
        let rules: Vec<Shape> = self
            .columns
            .windows(2)
            .take(used)
            .map(|pair| {
                let (a, b) = (pair[0], pair[1]);
                let x = left + (a.0 + a.1 + b.0) / 2.0;
                Shape {
                    x: x - SEPARATOR_WIDTH / 2.0,
                    y: self.column_top,
                    width: SEPARATOR_WIDTH,
                    height: bottom - self.column_top,
                    color: (0, 0, 0),
                    kind: ShapeKind::Rule,
                }
            })
            .collect();
        if let Some(page) = self.pages.last_mut() {
            page.shapes.extend(rules);
        }
    }

    fn new_page(&mut self, soft: bool) {
        let Some(g) = self.geometry else {
            return;
        };
        self.close_columns();
        self.finish_notes();
        self.column = 0;
        self.column_top = g.margins.top;
        self.column_bottom = g.margins.top;
        self.column_pending = false;
        self.region_start = (0, 0);
        // Какие колонтитулы у страницы и сколько места они оставляют тексту.
        let number = self.pages.len() + 1;
        let kind = if self.section_pages == 0 && self.bands.title_page {
            HeaderFooterKind::First
        } else if self.bands.even_and_odd && number.is_multiple_of(2) {
            HeaderFooterKind::Even
        } else {
            HeaderFooterKind::Default
        };
        self.section_pages += 1;
        let k = kind_index(kind);
        self.page_top = Some(g.margins.top.max(self.bands.top[k]));
        self.page_bottom = Some((g.height - g.margins.bottom).min(self.bands.bottom[k]));
        let top = self.top();
        self.column_top = top;
        self.column_bottom = top;
        self.pages.push(Page {
            width: g.width,
            height: g.height,
            margins: g.margins,
            lines: Vec::new(),
            shapes: Vec::new(),
            floats: Vec::new(),
            section: self.section,
            kind,
            header: PageStory::default(),
            footer: PageStory::default(),
            notes: Vec::new(),
            watermark: None,
        });
        self.y = top;
        self.has_content = false;
        self.soft_top = soft;
        self.break_pending = false;
        self.previous_after = 0.0;
    }

    /// Верх текста на странице: верхнее поле или низ колонтитула.
    fn top(&self) -> f32 {
        self.page_top
            .or(self.geometry.map(|g| g.margins.top))
            .unwrap_or(0.0)
    }

    /// Низ текста на странице: нижнее поле или верх колонтитула, выше
    /// — сноски страницы.
    fn bottom(&self) -> f32 {
        self.area_bottom() - self.notes_height
    }

    /// Низ места для текста и сносок: нижнее поле или верх колонтитула.
    fn area_bottom(&self) -> f32 {
        self.page_bottom
            .or(self.geometry.map(|g| g.height - g.margins.bottom))
            .unwrap_or(f32::INFINITY)
    }

    /// Номера сносок, ссылки на которые в строке `k` абзаца `notes`.
    fn line_notes<'n>(
        notes: &'n [(usize, String)],
        lines: &[Arc<Line>],
        k: usize,
    ) -> impl Iterator<Item = &'n str> {
        let range = lines.get(k).map(|l| l.range.clone());
        let last = k + 1 == lines.len();
        notes
            .iter()
            .filter(move |(at, _)| {
                range
                    .as_ref()
                    .is_some_and(|r| r.contains(at) || (last && *at >= r.end))
            })
            .map(|(_, id)| id.as_str())
    }

    /// Сколько места внизу страницы займут новые сноски строки `k`
    /// абзаца `p` (те, что ещё не на странице и не в `seen`).
    fn note_room(&self, p: &Prepared, k: usize, seen: &mut Vec<String>) -> f32 {
        let mut room = 0.0;
        for id in Self::line_notes(&p.notes, &p.lines, k) {
            let Some(note) = self.notes.get(id) else {
                continue;
            };
            if self.page_notes.iter().any(|(known, _)| known == id)
                || seen.iter().any(|known| known == id)
            {
                continue;
            }
            if self.page_notes.is_empty() && seen.is_empty() {
                room += self.note_separator;
            }
            room += note.height;
            seen.push(id.to_string());
        }
        room
    }

    /// Ставит на текущую страницу сноски строк, стоящих на ней с номера
    /// `from`.
    fn add_line_notes(&mut self, from: usize) {
        let Some(page) = self.pages.last() else {
            return;
        };
        let mut ids: Vec<String> = Vec::new();
        for line in &page.lines[from.min(page.lines.len())..] {
            let Some(notes) = self.paragraph_notes.get(line.paragraph) else {
                continue;
            };
            let range = &line.line.range;
            for (at, id) in notes {
                if range.contains(at) && !ids.contains(id) {
                    ids.push(id.clone());
                }
            }
        }
        self.add_notes(ids);
    }

    /// Ставит сноски `ids` на текущую страницу (если их там ещё нет).
    fn add_notes(&mut self, ids: impl IntoIterator<Item = String>) {
        for id in ids {
            if self.page_notes.iter().any(|(known, _)| *known == id) {
                continue;
            }
            let Some(note) = self.notes.get(&id).cloned() else {
                continue;
            };
            if self.page_notes.is_empty() {
                self.notes_height += self.note_separator;
            }
            self.notes_height += note.height;
            self.page_notes.push((id, note));
        }
    }

    /// Черта над сносками на высоте `top` (верх её строки высотой
    /// `height`).
    fn note_rule(&self, top: f32, height: f32) -> Shape {
        Shape {
            x: self.column_x(),
            y: top + height / 2.0 - SEPARATOR_WIDTH / 2.0,
            width: NOTE_RULE_LENGTH.min(self.column_width()),
            height: SEPARATOR_WIDTH,
            color: (0, 0, 0),
            kind: ShapeKind::Rule,
        }
    }

    /// Выводит сноски текущей страницы внизу неё: черта, под ней тексты
    /// сносок по порядку ссылок; низ последней — у нижнего поля.
    fn finish_notes(&mut self) {
        let notes = std::mem::take(&mut self.page_notes);
        let height = std::mem::take(&mut self.notes_height);
        if notes.is_empty() {
            return;
        }
        let mut y = self.area_bottom() - height;
        let rule = self.note_rule(y, self.note_separator);
        y += self.note_separator;
        let Some(page) = self.pages.last_mut() else {
            return;
        };
        page.shapes.push(rule);
        for (id, note) in notes {
            page.notes.push(PageNote {
                kind: NoteKind::Footnote,
                id,
                story: note.story.moved(y),
            });
            y += note.height;
        }
    }

    /// Ставит концевые сноски после текста документа: черта, под ней
    /// сноски целиком; сноска, которая не влезает, начинает новую
    /// страницу.
    fn place_endnotes(&mut self, notes: Vec<(String, NoteBox)>, separator: f32) {
        if notes.is_empty() {
            return;
        }
        if (self.break_pending || self.column_pending) && self.has_content {
            self.new_page(true);
        }
        let mut first = true;
        for (id, note) in notes {
            let need = note.height + if first { separator } else { 0.0 };
            if self.y + need > self.bottom() + EPSILON && self.has_content {
                self.next_column_or_page(true);
            }
            if first {
                let rule = self.note_rule(self.y, separator);
                if let Some(page) = self.pages.last_mut() {
                    page.shapes.push(rule);
                }
                self.y += separator;
                first = false;
            }
            let dx = self.column_x() - self.geometry.map_or(0.0, |g| g.margins.left);
            let mut story = note.story.moved(self.y);
            for line in &mut story.lines {
                line.x += dx;
            }
            if let Some(page) = self.pages.last_mut() {
                page.notes.push(PageNote {
                    kind: NoteKind::Endnote,
                    id,
                    story,
                });
            }
            self.y += note.height;
            self.has_content = true;
        }
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
        shaper: &mut Shaper,
        library: &FontLibrary,
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
        if self.column_pending && self.has_content {
            self.next_column_or_page(true);
        }
        if p.page_break_before && self.has_content {
            self.new_page(false);
        }
        if p.keep_next && self.has_content {
            let chain = self.keep_chain_height(prepared, blocks, i);
            if self.y + chain > self.bottom() + EPSILON && chain <= self.page_height() {
                self.next_column_or_page(true);
            }
        }

        // Куски абзаца между разрывами страницы и колонки.
        let mut segments = Vec::new();
        let mut start = 0;
        for (k, line) in p.lines.iter().enumerate() {
            if matches!(line.end, LineEnd::Page | LineEnd::Column) {
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
        let mut ends_with_column = false;
        if segments.len() > 1
            && segments.last().is_some_and(|last| {
                last.len() == 1 && p.lines.get(last.start).is_some_and(|l| l.range.is_empty())
            })
        {
            segments.pop();
            let column = segments
                .last()
                .and_then(|s| p.lines.get(s.end.wrapping_sub(1)))
                .is_some_and(|l| l.end == LineEnd::Column);
            ends_with_break = !column;
            ends_with_column = column;
        }
        // Чем кончается кусок: разрывом колонки или страницы.
        let column_break: Vec<bool> = segments
            .iter()
            .map(|s| {
                p.lines
                    .get(s.end.wrapping_sub(1))
                    .is_some_and(|l| l.end == LineEnd::Column)
            })
            .collect();

        // Место под линии рамки: сверху — у первого абзаца рамки (у
        // следующих — под линию между абзацами), снизу — у последнего.
        let frame = p.frame.clone();
        let same = |j: Option<usize>| {
            frame.is_some()
                && j.and_then(|j| prepared.get(j))
                    .is_some_and(|q| q.frame == frame)
        };
        let joins = (same(i.checked_sub(1)), same(Some(i + 1)));
        let (top_pad, bottom_pad) = match &frame {
            Some(f) => (
                Frame::pad(if joins.0 { &f.between } else { &f.top }),
                if joins.1 { 0.0 } else { Frame::pad(&f.bottom) },
            ),
            None => (0.0, 0.0),
        };
        let mut last = segments.last().map_or(0, |s| s.end.saturating_sub(1));

        // Обтекание рисунков — только у абзацев без разрывов страницы.
        let wraps = segments.len() == 1 && !ends_with_break && !ends_with_column;
        let mut current = padded(Arc::clone(p), top_pad, bottom_pad, last);
        let mut first_segment = true;
        for (s, segment) in segments.iter().enumerate() {
            if s > 0 {
                if column_break.get(s - 1).copied().unwrap_or(false) {
                    self.next_column_or_page(false);
                } else {
                    self.new_page(false);
                }
            }
            let mut segment = segment.clone();
            let mut next = segment.start;
            while next < segment.end {
                let before_now = if first_segment && next == segment.start {
                    self.space_before(before)
                } else {
                    0.0
                };
                if wraps && next == segment.start {
                    let top = self.y + before_now;
                    let wrapped = self
                        .wrap(p, top, paragraph_index, shaper, library)
                        .map_or_else(|| Arc::clone(p), Arc::new);
                    last = wrapped.lines.len().saturating_sub(1);
                    current = padded(wrapped, top_pad, bottom_pad, last);
                    segment.end = current.lines.len();
                }
                let heights = &current.heights;
                let available = self.bottom() - self.y - before_now;
                let remaining = segment.end - next;
                let mut fit = 0;
                let mut used = 0.0;
                // Сноски строк, которые встанут на страницу, занимают
                // место внизу неё.
                let mut seen = Vec::new();
                for (k, h) in heights[next..segment.end].iter().enumerate() {
                    let notes = self.note_room(&current, next + k, &mut seen);
                    if used + h + notes > available + EPSILON {
                        break;
                    }
                    used += h + notes;
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
                        self.next_column_or_page(true);
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
                            self.next_column_or_page(true);
                            continue;
                        }
                        // На пустой странице — хоть одна строка.
                        fit = 1;
                    }
                }
                self.y += before_now;
                let chunk_top = self.y;
                for k in next..next + fit {
                    self.push_line(&current, k, paragraph_index);
                }
                if let Some(frame) = &frame {
                    let edges = (next == 0, (next..next + fit).contains(&last));
                    self.frame_chunk(frame, (chunk_top, self.y), edges, joins);
                }
                next += fit;
                if next < segment.end {
                    self.next_column_or_page(true);
                }
            }
            first_segment = false;
        }
        self.y += after;
        self.previous_after = after;
        self.has_content = true;
        self.break_pending = ends_with_break;
        self.column_pending = ends_with_column;
    }

    /// Абзац `p`, разбитый на строки заново вокруг плавающих рисунков
    /// с обтеканием, если он начнётся на высоте `top`; `None` — рисунки
    /// ему не мешают. Мешают рисунки, уже стоящие на странице, и рисунки
    /// самого абзаца. Строки, ушедшие на следующую страницу, рисунки не
    /// сужают.
    fn wrap(
        &self,
        p: &Prepared,
        top: f32,
        paragraph: usize,
        shaper: &mut Shaper,
        library: &FontLibrary,
    ) -> Option<Prepared> {
        let g = self.geometry?;
        let mut obstacles: Vec<Obstacle> = self
            .pages
            .last()
            .map(|page| page.floats.iter().filter_map(Obstacle::of).collect())
            .unwrap_or_default();
        obstacles.extend(p.floats.iter().filter_map(|f| {
            Obstacle::of(&place_float(f, &g, top, top, self.column_x(), paragraph))
        }));
        let bottom = self.bottom();
        obstacles.retain(|o| o.bottom > top + EPSILON && o.top < bottom);
        if obstacles.is_empty() {
            return None;
        }
        let source = &p.source;
        let rule = p.spacing.line;
        let from = self.column_x();
        let to = from + self.column_width();
        let fallback = source.mark_style.size * 1.2;
        let mut gaps: Vec<f32> = Vec::new();
        let mut y = top;
        let mut room = |lines: &[Line]| -> Option<LineRoom> {
            if let Some(last) = lines.last() {
                y += gaps.last().copied().unwrap_or(0.0) + rule.height(last);
            }
            let estimate = lines.last().map_or(fallback, |l| rule.height(l));
            let mut gap = 0.0;
            loop {
                let (line_top, line_bottom) = (y + gap, y + gap + estimate);
                let hits: Vec<&Obstacle> = obstacles
                    .iter()
                    .filter(|o| o.top < line_bottom && o.bottom > line_top)
                    .collect();
                if hits.is_empty() || line_bottom > bottom + EPSILON {
                    gaps.push(gap);
                    return None;
                }
                let (left, right) = widest_gap(from, to, &hits);
                if right - left >= MIN_ROOM {
                    gaps.push(gap);
                    return Some(LineRoom {
                        left: left - from,
                        right: right - from,
                    });
                }
                // Рядом с рисунком места нет: строка уходит под него.
                let below = hits.iter().map(|o| o.bottom).fold(line_top, f32::max);
                gap = below - y;
            }
        };
        let mut broken = break_lines_around(
            shaper,
            library,
            &source.spans,
            &source.mark_style,
            &source.format,
            &mut room,
        );
        raise_first_line(&mut broken.lines, source.label);
        let heights = broken
            .lines
            .iter()
            .zip(&gaps)
            .map(|(line, gap)| gap + rule.height(line))
            .collect();
        Some(Prepared {
            text: broken.text.into(),
            lines: broken.lines.into_iter().map(Arc::new).collect(),
            heights,
            spans: p.spans.clone(),
            spacing: p.spacing,
            style: p.style.clone(),
            keep_next: p.keep_next,
            keep_lines: p.keep_lines,
            page_break_before: p.page_break_before,
            widow_control: p.widow_control,
            label: p.label.clone(),
            floats: p.floats.clone(),
            source: Arc::clone(&p.source),
            gaps,
            notes: p.notes.clone(),
            frame: p.frame.clone(),
        })
    }

    /// Строки абзаца с рамкой `frame` от `top` до `bottom` встали на
    /// страницу; `edges` — среди них первая и последняя строки абзаца,
    /// `joins` — у предыдущего и следующего абзацев та же рамка.
    fn frame_chunk(
        &mut self,
        frame: &Arc<Frame>,
        (top, bottom): (f32, f32),
        (first, last): (bool, bool),
        joins: (bool, bool),
    ) {
        let page = self.pages.len().saturating_sub(1);
        let x = self.column_x();
        let (left, right) = (
            x + frame.indent_left,
            x + self.column_width() - frame.indent_right,
        );
        if let Some(run) = &mut self.frame_run
            && first
            && joins.0
            && run.page == page
            && (run.left - left).abs() < EPSILON
        {
            if frame.between.is_some() {
                run.between.push(top);
            }
            run.bottom = bottom;
            run.bottom_edge = last && !joins.1;
            return;
        }
        self.flush_frame();
        self.frame_run = Some(FrameRun {
            frame: Arc::clone(frame),
            page,
            left,
            right,
            top,
            bottom,
            top_edge: first && !joins.0,
            bottom_edge: last && !joins.1,
            between: Vec::new(),
        });
    }

    /// Рисует собранную рамку: заливку под текстом и линии вокруг него
    /// (снаружи, на расстоянии `w:space` от текста).
    fn flush_frame(&mut self) {
        let Some(run) = self.frame_run.take() else {
            return;
        };
        let Some(page) = self.pages.get_mut(run.page) else {
            return;
        };
        let f = &run.frame;
        let width = |b: &Option<Border>| b.as_ref().map_or(0.0, Border::width_points);
        let space = |b: &Option<Border>| b.as_ref().map_or(0.0, |b| b.space.unwrap_or(0) as f32);
        let (inner_left, inner_right) = (run.left - space(&f.left), run.right + space(&f.right));
        let (outer_left, outer_right) =
            (inner_left - width(&f.left), inner_right + width(&f.right));
        let rule = |b: &Border, x: f32, y: f32, w: f32, h: f32| Shape {
            x,
            y,
            width: w,
            height: h,
            color: match b.color {
                Some(letter_model::Color::Rgb(r, g, b)) => (r, g, b),
                _ => (0, 0, 0),
            },
            kind: ShapeKind::Rule,
        };
        if let Some(color) = f.fill {
            page.shapes.push(Shape {
                x: inner_left,
                y: run.top,
                width: inner_right - inner_left,
                height: run.bottom - run.top,
                color,
                kind: ShapeKind::Fill,
            });
        }
        let across = outer_right - outer_left;
        if let (true, Some(b)) = (run.top_edge, &f.top) {
            page.shapes
                .push(rule(b, outer_left, run.top, across, b.width_points()));
        }
        if let (true, Some(b)) = (run.bottom_edge, &f.bottom) {
            let w = b.width_points();
            page.shapes
                .push(rule(b, outer_left, run.bottom - w, across, w));
        }
        if let Some(b) = &f.between {
            for &y in &run.between {
                page.shapes
                    .push(rule(b, outer_left, y, across, b.width_points()));
            }
        }
        let height = run.bottom - run.top;
        if let Some(b) = &f.left {
            page.shapes
                .push(rule(b, outer_left, run.top, b.width_points(), height));
        }
        if let Some(b) = &f.right {
            page.shapes
                .push(rule(b, inner_right, run.top, b.width_points(), height));
        }
    }

    fn push_line(&mut self, p: &Prepared, k: usize, paragraph: usize) {
        let (Some(line), Some(&height), Some(g)) =
            (p.lines.get(k), p.heights.get(k), self.geometry)
        else {
            return;
        };
        let gap = p.gaps.get(k).copied().unwrap_or(0.0);
        let (above, top, full_height) = (self.y, self.y + gap, height);
        // Под последней строкой может быть место под линию рамки.
        let height = (full_height - gap).min(p.spacing.line.height(line));
        let column_x = self.column_x();
        let placed = PageLine {
            line: Arc::clone(line),
            x: column_x,
            top,
            height,
            baseline: top + p.spacing.line.baseline_offset(line, height),
            paragraph,
            text: p.text.clone(),
            spans: p.spans.clone(),
            label: if k == 0 { p.label.clone() } else { None },
            cell: None,
        };
        let page_index = self.pages.len().saturating_sub(1);
        if self
            .paragraph_top
            .is_none_or(|(p, page, _)| p != paragraph || page != page_index)
        {
            self.paragraph_top = Some((paragraph, page_index, above));
        }
        let paragraph_top = self.paragraph_top.map_or(top, |(_, _, t)| t);
        let last = k + 1 == p.lines.len();
        let anchors: Vec<Float> = p
            .floats
            .iter()
            .filter(|f| line.range.contains(&f.at) || (last && f.at >= line.range.end))
            .map(|f| {
                let char_x = column_x + anchor_x(line, f.at);
                place_float(f, &g, top, paragraph_top, char_x, paragraph)
            })
            .collect();
        if let Some(page) = self.pages.last_mut() {
            page.lines.push(placed);
            page.floats.extend(anchors);
        }
        let notes: Vec<String> = Self::line_notes(&p.notes, &p.lines, k)
            .map(str::to_string)
            .collect();
        self.add_notes(notes);
        self.y += full_height;
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
        if self.column_pending && self.has_content {
            self.next_column_or_page(true);
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
        self.column_pending = false;
    }

    /// Новая страница посреди таблицы, с повторёнными заголовками.
    fn table_page(&mut self, headers: &[RowBox], repeat: bool) {
        self.next_column_or_page(true);
        if repeat {
            for header in headers {
                self.draw(header, true, true);
            }
        }
    }

    /// Рисует строку таблицы с текущего места и сдвигает его вниз.
    fn draw(&mut self, row: &RowBox, top_edge: bool, bottom_edge: bool) {
        let left = self.column_x();
        let top = self.y;
        let from = self.pages.last().map_or(0, |p| p.lines.len());
        let mut canvas = PageCanvas {
            page: self.pages.last_mut(),
        };
        draw_row(row, left, top, top_edge, bottom_edge, &mut canvas);
        self.add_line_notes(from);
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
    use letter_model::{AnchorPosition, BreakKind, PageMargins, Run, Section, SectionProps};

    /// Длина знака объекта в байтах.
    const OBJECT_LEN: usize = letter_model::OBJECT_CHAR.len_utf8();

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
                ..SectionProps::default()
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

    /// Абзац `text` со ссылкой на сноску `id` в конце.
    fn noted(text: &str, kind: letter_model::NoteKind, id: &str) -> Paragraph {
        let reference = Run {
            content: vec![RunContent::NoteReference(letter_model::NoteReference {
                kind,
                id: id.into(),
                custom_mark: false,
            })],
            ..Run::default()
        };
        let text = Run {
            content: vec![RunContent::Text(text.into())],
            ..Run::default()
        };
        Paragraph::from_runs([text, reference])
    }

    fn note(id: &str, text: &str) -> letter_model::Note {
        letter_model::Note {
            id: id.into(),
            special: None,
            blocks: vec![para(text).into()],
        }
    }

    #[test]
    fn footnotes_stand_at_the_bottom_of_their_page() {
        let mut paragraphs = singles(12);
        paragraphs[1] = noted("p1", NoteKind::Footnote, "7");
        let mut doc = doc(paragraphs);
        // Лист на 10 строк.
        if let Some(size) = &mut doc.sections[0].props.page_size {
            size.height = Twips(2304);
        }
        doc.footnotes = vec![note("7", "note")];
        let layout = lay(&doc);
        // Сноска и строка с чертой занимают место двух строк с лишним.
        assert_eq!(pages(&layout)[0], [0, 1, 2, 3, 4, 5, 6]);
        let page = &layout.pages[0];
        assert_eq!(page.notes.len(), 1);
        assert_eq!(page.notes[0].id, "7");
        let line = &page.notes[0].story.lines[0];
        assert!((line.top + line.height - page.height).abs() < 1e-3);
        assert!(line.text.contains("note"));
        let rule = page.shapes.last().map(|s| s.y);
        assert!(rule.is_some_and(|y| y < line.top && y > page.lines[6].top));
        assert!(layout.pages[1].notes.is_empty());
    }

    #[test]
    fn endnotes_follow_the_text() {
        let mut doc = doc(vec![noted("p0", NoteKind::Endnote, "1"), para("p1")]);
        doc.endnotes = vec![note("1", "end"), note("2", "unused")];
        let layout = lay(&doc);
        let page = &layout.pages[0];
        assert_eq!(page.notes.len(), 1);
        assert_eq!(page.notes[0].kind, NoteKind::Endnote);
        let line = &page.notes[0].story.lines[0];
        // Под текстом: строка с чертой, потом сноска.
        assert!((line.top - (2.0 * LINE + DEFAULT_NOTE_SEPARATOR)).abs() < 1e-3);
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

    fn floating(
        horizontal: AnchorPosition<HorizontalFrom>,
        vertical: AnchorPosition<VerticalFrom>,
        behind_text: bool,
    ) -> RunContent {
        RunContent::Picture(letter_model::Picture {
            width: 20 * EMU_PER_POINT,
            height: 10 * EMU_PER_POINT,
            image: None,
            relationship: None,
            raw: None,
            anchor: Some(Box::new(Anchor {
                horizontal,
                vertical,
                wrap: Wrap::None,
                behind_text,
                distance: letter_model::Distance::default(),
            })),
            shape: None,
        })
    }

    /// Рисунок `width` × `height` пт с обтеканием `wrap` в точке `x`, `y`
    /// от угла листа.
    fn wrapped(wrap: Wrap, x: i64, y: i64, width: i64, height: i64) -> RunContent {
        let at = |pt: i64| Placement::Offset(pt * EMU_PER_POINT);
        RunContent::Picture(letter_model::Picture {
            width: width * EMU_PER_POINT,
            height: height * EMU_PER_POINT,
            image: None,
            relationship: None,
            raw: None,
            anchor: Some(Box::new(Anchor {
                horizontal: AnchorPosition {
                    relative: HorizontalFrom::Page,
                    placement: at(x),
                },
                vertical: AnchorPosition {
                    relative: VerticalFrom::Page,
                    placement: at(y),
                },
                wrap,
                behind_text: false,
                distance: letter_model::Distance::default(),
            })),
            shape: None,
        })
    }

    /// Лист 200 × 200 пт без полей; первый абзац привязывает `picture`,
    /// дальше — длинный текст.
    fn around(picture: RunContent) -> DocumentLayout {
        let mut anchor = para("");
        if let Some(letter_model::Inline::Run(run)) = anchor.content.first_mut() {
            run.content.push(picture);
        }
        let words = vec!["word"; 60].join(" ");
        let mut d = doc(vec![anchor, para(&words)]);
        d.sections[0].props.page_size = Some(PageSize {
            width: Twips(4000),
            height: Twips(4000),
            orientation: Default::default(),
        });
        lay(&d)
    }

    #[test]
    fn text_wraps_around_a_square_picture() {
        let layout = around(wrapped(Wrap::Square, 0, 0, 60, 30));
        let page = &layout.pages[0];
        let text: Vec<&PageLine> = page.lines.iter().filter(|l| l.paragraph == 1).collect();
        assert!(text.len() > 3);
        for line in &text {
            let left = line.x + line.line.x;
            if line.top < 30.0 {
                assert!(left >= 60.0 - 1e-3, "{} {}", line.top, left);
                assert!(line.line.width <= 140.0 + 1e-3);
            } else {
                assert!(left.abs() < 1e-3, "{} {}", line.top, left);
            }
        }
        assert!(text.iter().any(|l| l.top < 30.0));
        // Рисунок справа: строки рядом с ним кончаются до него.
        let layout = around(wrapped(Wrap::Square, 140, 0, 60, 30));
        let line = layout.pages[0]
            .lines
            .iter()
            .find(|l| l.paragraph == 1)
            .unwrap();
        assert!(line.line.x.abs() < 1e-3);
        assert!(line.line.x + line.line.width <= 140.0 + 1e-3);
    }

    #[test]
    fn text_goes_below_a_top_and_bottom_picture() {
        let layout = around(wrapped(Wrap::TopAndBottom, 50, 0, 60, 30));
        let page = &layout.pages[0];
        let first = page.lines.iter().find(|l| l.paragraph == 1).unwrap();
        assert!(first.top >= 30.0 - 1e-3, "{}", first.top);
        assert!(first.line.x.abs() < 1e-3);
        // Без обтекания рисунок тексту не мешает.
        let layout = around(wrapped(Wrap::None, 50, 0, 60, 30));
        let first = layout.pages[0]
            .lines
            .iter()
            .find(|l| l.paragraph == 1)
            .unwrap();
        assert!(first.top < 30.0);
    }

    #[test]
    fn widest_gap_is_found() {
        let o = |left, right| Obstacle {
            left,
            right,
            top: 0.0,
            bottom: 1.0,
            whole_width: false,
        };
        assert_eq!(widest_gap(0.0, 100.0, &[&o(10.0, 30.0)]), (30.0, 100.0));
        assert_eq!(widest_gap(0.0, 100.0, &[&o(60.0, 120.0)]), (0.0, 60.0));
        assert_eq!(
            widest_gap(0.0, 100.0, &[&o(70.0, 90.0), &o(10.0, 20.0)]),
            (20.0, 70.0)
        );
        let all = Obstacle {
            whole_width: true,
            ..o(40.0, 50.0)
        };
        let (left, right) = widest_gap(0.0, 100.0, &[&all]);
        assert!(right - left < 1e-3);
    }

    #[test]
    fn floating_pictures_are_placed_on_the_page() {
        let mut anchored = para("ab");
        if let Some(letter_model::Inline::Run(run)) = anchored.content.first_mut() {
            run.content.push(floating(
                AnchorPosition {
                    relative: HorizontalFrom::Page,
                    placement: Placement::Offset(50 * EMU_PER_POINT),
                },
                AnchorPosition {
                    relative: VerticalFrom::Paragraph,
                    placement: Placement::Offset(5 * EMU_PER_POINT),
                },
                true,
            ));
            run.content.push(RunContent::Text("c".into()));
            run.content.push(floating(
                AnchorPosition {
                    relative: HorizontalFrom::Margin,
                    placement: Placement::Align(AnchorAlign::Center),
                },
                AnchorPosition {
                    relative: VerticalFrom::Page,
                    placement: Placement::Align(AnchorAlign::End),
                },
                false,
            ));
        }
        let layout = lay(&doc(vec![para("p0"), anchored]));
        let page = &layout.pages[0];
        // Плавающий рисунок не занимает места в строке.
        assert_eq!(pages(&layout), [vec![0, 1]]);
        assert!((page.lines[1].top - LINE).abs() < 1e-3);
        let close = |a: f32, b: f32| (a - b).abs() < 1e-3;
        let [behind, front] = &page.floats[..] else {
            panic!("{:?}", page.floats);
        };
        assert!(close(behind.x, 50.0) && close(behind.y, LINE + 5.0));
        assert!(behind.behind && !front.behind);
        assert_eq!((behind.paragraph, behind.at), (1, 2));
        assert_eq!(front.at, 2 + OBJECT_LEN + 1);
        // По центру листа 200 пт и по нижнему краю листа 57,6 пт.
        assert!(close(front.x, 90.0) && close(front.y, 47.6));
        assert!(close(front.width, 20.0) && close(front.height, 10.0));
    }

    /// Колонки без промежутка.
    fn columns(count: u16, separator: bool) -> Option<letter_model::Columns> {
        Some(letter_model::Columns {
            count,
            space: Twips(0),
            separator,
            widths: Vec::new(),
        })
    }

    /// Левые края и номера абзацев строк каждой страницы.
    fn places(layout: &DocumentLayout) -> Vec<Vec<(f32, usize)>> {
        layout
            .pages
            .iter()
            .map(|p| p.lines.iter().map(|l| (l.x, l.paragraph)).collect())
            .collect()
    }

    #[test]
    fn text_flows_through_columns() {
        let mut d = doc(singles(12));
        d.sections[0].props.columns = columns(2, true);
        let layout = lay(&d);
        let pages = places(&layout);
        assert_eq!(pages.len(), 2);
        // Пять строк в первой колонке, пять во второй (на 100 пт правее).
        let xs: Vec<f32> = pages[0].iter().map(|l| l.0).collect();
        assert_eq!(
            xs,
            [0.0, 0.0, 0.0, 0.0, 0.0, 100.0, 100.0, 100.0, 100.0, 100.0]
        );
        assert!((layout.pages[0].lines[5].top).abs() < 1e-3);
        assert_eq!(pages[1], [(0.0, 10), (0.0, 11)]);
        // Строки ужаты до ширины колонки.
        assert!(
            layout.pages[0]
                .lines
                .iter()
                .all(|l| l.line.x + l.line.width <= 100.0 + 1e-3)
        );
        // Линия между колонками — только на первой странице (на второй
        // текст в одной колонке).
        let rules = |page: &Page| {
            page.shapes
                .iter()
                .filter(|s| s.kind == ShapeKind::Rule)
                .map(|s| (s.x + s.width / 2.0, s.height))
                .collect::<Vec<_>>()
        };
        let first = rules(&layout.pages[0]);
        assert_eq!(first.len(), 1);
        assert!((first[0].0 - 100.0).abs() < 1e-3 && (first[0].1 - 5.0 * LINE).abs() < 1e-3);
        assert!(rules(&layout.pages[1]).is_empty());
    }

    #[test]
    fn column_break_starts_the_next_column() {
        let mut broken = para("a");
        broken.content.push(letter_model::Inline::Run(Run {
            props: RunProps::default(),
            content: vec![
                RunContent::Break(BreakKind::Column),
                RunContent::Text("b".into()),
            ],
        }));
        let mut d = doc(vec![broken, para("c")]);
        d.sections[0].props.columns = columns(2, false);
        assert_eq!(places(&lay(&d)), [vec![(0.0, 0), (100.0, 0), (100.0, 1)]]);
        // Без колонок разрыв колонки — новая страница.
        d.sections[0].props.columns = None;
        assert_eq!(pages(&lay(&d)), [vec![0], vec![0, 1]]);
    }

    #[test]
    fn columns_are_balanced_before_a_continuous_section() {
        let mut d = doc(Vec::new());
        let mut first = doc(singles(4)).sections.remove(0);
        first.props.columns = columns(2, true);
        let mut second = doc(vec![para("after")]).sections.remove(0);
        second.props.start = Some(SectionStart::Continuous);
        d.sections = vec![first, second];
        let layout = lay(&d);
        let page = &layout.pages[0];
        let found: Vec<(f32, f32)> = page.lines.iter().map(|l| (l.x, l.top)).collect();
        let close =
            |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3;
        let expected = [
            (0.0, 0.0),
            (0.0, LINE),
            (100.0, 0.0),
            (100.0, LINE),
            // Новый раздел — под колонками, во всю ширину.
            (0.0, 2.0 * LINE),
        ];
        assert_eq!(found.len(), expected.len());
        assert!(
            found.iter().zip(expected).all(|(a, b)| close(*a, b)),
            "{found:?}"
        );
        let rule = page
            .shapes
            .iter()
            .find(|s| s.kind == ShapeKind::Rule)
            .unwrap();
        assert!((rule.height - 2.0 * LINE).abs() < 1e-3);
    }

    #[test]
    fn sections_start_where_they_say() {
        let section = |text: &str, start: Option<SectionStart>| {
            let mut s = doc(vec![para(text)]).sections.remove(0);
            s.props.start = start;
            s
        };
        let mut d = doc(Vec::new());
        d.sections = vec![
            section("a", None),
            section("b", Some(SectionStart::Continuous)),
            section("c", Some(SectionStart::OddPage)),
            section("d", Some(SectionStart::EvenPage)),
            section("e", Some(SectionStart::NextPage)),
        ];
        let layout = lay(&d);
        // «b» — на той же странице под «a»; «c» — на третьей (вторая
        // пустая); «d» — на четвёртой; «e» — на пятой.
        assert_eq!(
            pages(&layout),
            [vec![0, 1], vec![], vec![2], vec![3], vec![4]]
        );
        assert!((layout.pages[0].lines[1].top - LINE).abs() < 1e-3);
        // «На текущей странице» с другим размером листа — новая страница.
        d.sections[1].props.page_size = Some(PageSize::A4);
        assert_eq!(pages(&lay(&d))[..2], [vec![0], vec![1]]);
    }

    #[test]
    fn watermark_stands_in_the_middle_of_the_page() {
        let mark = r##"<w:pict xmlns:w="w" xmlns:v="v"><v:shape id="PowerPlusWaterMarkObject1" style="position:absolute;width:200pt;height:50pt;rotation:315" fillcolor="#ff0000"><v:fill opacity=".5"/><v:textpath style="font-family:&quot;Liberation Sans&quot;" string="DRAFT"/></v:shape></w:pict>"##;
        let mut header = para("");
        if let Some(letter_model::Inline::Run(run)) = header.content.first_mut() {
            run.content
                .push(RunContent::Unknown(letter_model::RawXml::new(mark)));
        }
        let mut d = doc(singles(40));
        d.sections[0].props.headers.default = Some(letter_model::HeaderFooter {
            blocks: vec![header.into()],
            relationship: None,
        });
        let layout = lay(&d);
        assert!(layout.pages.len() > 1);
        for page in &layout.pages {
            let mark = page.watermark.as_ref().expect("подложка");
            assert_eq!((mark.color, mark.alpha), ((255, 0, 0), 128));
            // Надпись растянута на 200 пт.
            let width: f32 = mark.runs.iter().map(ShapedRun::width).sum();
            assert!((width - 200.0).abs() < 0.1, "{width}");
            // Начало надписи — левее и ниже середины листа (надпись
            // идёт вверх вправо), середина надписи — в середине листа.
            let (sin, cos) = 315f32.to_radians().sin_cos();
            let mid = (mark.x + 100.0 * cos, mark.y + 100.0 * sin);
            assert!(mark.x < page.width / 2.0 && mark.y > page.height / 2.0);
            assert!((mid.0 - page.width / 2.0).abs() < 30.0, "{mid:?}");
            assert!((mid.1 - page.height / 2.0).abs() < 30.0, "{mid:?}");
        }
    }

    #[test]
    fn headers_and_footers_take_their_place() {
        let story = |text: &str| letter_model::HeaderFooter {
            blocks: vec![para(text).into()],
            relationship: None,
        };
        let mut d = doc(singles(8));
        let props = &mut d.sections[0].props;
        props.margins = props.margins.map(|m| PageMargins {
            header: Twips(0),
            footer: Twips(0),
            ..m
        });
        props.title_page = true;
        props.headers.first = Some(story("first"));
        props.headers.default = Some(story("header"));
        props.footers.default = Some(story("footer"));
        let mut second = doc(vec![para("second section")]).sections.remove(0);
        second.props.margins = props.margins;
        d.sections.push(second);
        let layout = lay(&d);
        // Колонтитулы отодвигают текст; у первой страницы нижнего нет.
        assert_eq!(
            pages(&layout),
            [vec![0, 1, 2, 3], vec![4, 5, 6], vec![7], vec![8]]
        );
        let text = |story: &PageStory| {
            story
                .lines
                .iter()
                .map(|l| l.text[l.line.range.clone()].to_string())
                .collect::<Vec<_>>()
        };
        let p = &layout.pages;
        assert_eq!(text(&p[0].header), ["first"]);
        assert!(p[0].footer.is_empty());
        assert_eq!(text(&p[1].header), ["header"]);
        assert_eq!(text(&p[1].footer), ["footer"]);
        assert!(p[1].header.lines[0].top.abs() < 1e-3);
        assert!((p[1].footer.lines[0].top - (57.6 - LINE)).abs() < 1e-3);
        assert!((p[1].lines[0].top - LINE).abs() < 1e-3);
        // Второй раздел берёт колонтитулы первого.
        assert_eq!(p[3].section, 1);
        assert_eq!(text(&p[3].header), ["header"]);
        assert_eq!(text(&p[3].footer), ["footer"]);

        // Чётные страницы — со своим колонтитулом; нижнего чётного нет,
        // поэтому на второй странице текст доходит до низа.
        d.settings.even_and_odd_headers = true;
        d.sections[0].props.headers.even = Some(story("even"));
        let layout = lay(&d);
        let kinds: Vec<_> = layout.pages.iter().map(|p| text(&p.header)).collect();
        assert_eq!(kinds, [["first"], ["even"], ["header"]]);
        assert_eq!(
            pages(&layout),
            [vec![0, 1, 2, 3], vec![4, 5, 6, 7], vec![8]]
        );
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
    fn paragraph_borders_and_shading() {
        use letter_model::{Border, Color, ParagraphBorders, Shading};
        let line = Border {
            space: Some(1),
            ..Border::single(8)
        };
        let mut boxed = ParagraphBorders::frame(line.clone());
        boxed.between = Some(line);
        let framed = |text: &str| {
            let mut p = para(text);
            p.props.borders = Some(boxed.clone());
            p.props.shading = Some(Shading::fill(Color::Rgb(0, 200, 0)));
            p
        };
        let layout = lay(&doc(vec![framed("a"), framed("b"), para("c")]));
        let page = &layout.pages[0];
        let tops: Vec<f32> = page.lines.iter().map(|l| l.top).collect();
        // Над рамкой и между абзацами — линия 1 пт и отступ 1 пт, под
        // рамкой — так же; у строк своя высота.
        let expected = [2.0, 2.0 + LINE + 2.0, 2.0 + 2.0 * LINE + 4.0];
        for (top, want) in tops.iter().zip(expected) {
            assert!((top - want).abs() < 1e-3, "{tops:?}");
        }
        assert!(page.lines.iter().all(|l| (l.height - LINE).abs() < 1e-3));
        let rules: Vec<&Shape> = page
            .shapes
            .iter()
            .filter(|s| s.kind == ShapeKind::Rule)
            .collect();
        // Сверху, снизу, между абзацами, слева и справа — одна рамка.
        assert_eq!(rules.len(), 5);
        let fills: Vec<&Shape> = page
            .shapes
            .iter()
            .filter(|s| s.kind == ShapeKind::Fill)
            .collect();
        assert_eq!(fills.len(), 1);
        assert!((fills[0].y).abs() < 1e-3);
        assert!((fills[0].height - (2.0 * LINE + 6.0)).abs() < 1e-3);
        // Абзац без рамки её не получает.
        let mut plain = doc(vec![para("a")]);
        plain.sections[0].blocks.push(framed("b").into());
        let layout = lay(&plain);
        assert!(layout.pages[0].lines[0].top.abs() < 1e-3);
        assert_eq!(layout.pages[0].shapes.len(), 5);
    }

    #[test]
    fn page_color_and_page_borders() {
        use letter_model::{Border, Color, PageBorderDisplay, PageBorders};
        let mut d = doc(singles(40));
        d.background = Some(Color::Rgb(1, 2, 3));
        let line = Border {
            space: Some(24),
            ..Border::single(8)
        };
        d.sections[0].props.page_borders = Some(PageBorders {
            top: Some(line.clone()),
            left: Some(line),
            from_page_edge: true,
            display: PageBorderDisplay::NotFirstPage,
            ..PageBorders::default()
        });
        let layout = lay(&d);
        assert!(layout.pages.len() > 1);
        for (i, page) in layout.pages.iter().enumerate() {
            let fill = page.shapes.first().copied();
            assert_eq!(
                fill.map(|s| (s.kind, s.color)),
                Some((ShapeKind::Fill, (1, 2, 3)))
            );
            assert_eq!(
                fill.map(|s| (s.width, s.height)),
                Some((page.width, page.height))
            );
            let rules: Vec<&Shape> = page
                .shapes
                .iter()
                .filter(|s| s.kind == ShapeKind::Rule)
                .collect();
            if i == 0 {
                assert!(rules.is_empty());
            } else {
                // Сверху и слева, в 24 пт от края листа.
                assert_eq!(rules.len(), 2);
                assert!(rules.iter().all(|r| (r.x - 24.0).abs() < 1e-3));
                assert!(rules.iter().all(|r| (r.y - 24.0).abs() < 1e-3));
            }
        }
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
    fn symbol_from_a_font_is_shown_as_unicode() {
        let run = Run {
            content: vec![
                RunContent::Text("a".into()),
                RunContent::Symbol(letter_model::Symbol {
                    font: "Symbol".into(),
                    code: 0xF070,
                }),
            ],
            ..Run::default()
        };
        let layout = lay(&doc(vec![Paragraph::from_runs([run])]));
        let line = &layout.pages[0].lines[0];
        assert_eq!(&*line.text, format!("a{}", letter_model::OBJECT_CHAR));
        let glyphs: usize = line
            .line
            .items
            .iter()
            .map(|item| match item {
                crate::LineItem::Text(t) => t.run.glyphs.len(),
                _ => 0,
            })
            .sum();
        assert_eq!(glyphs, 2);
        assert!(line.line.width > 0.0);
    }
}
