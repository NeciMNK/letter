//! Документ для вёрстки в режимах «Веб-документ» и «Структура».
//!
//! В этих режимах нет листов: текст идёт по ширине окна. Вёрстка же
//! умеет только листы, поэтому верстается копия документа, где у каждого
//! раздела лист шириной в окно и очень длинный, поля маленькие,
//! колонтитулов и колонок нет. Абзацы и их номера те же, что в самом
//! документе, так что курсор, выделение и правка работают как обычно —
//! правится сам документ, а копия делается заново при вёрстке.

use std::collections::BTreeSet;

use letter_editor::outline_level;
use letter_model::{Block, Document, LineSpacing, NumberingRef, PageMargins, PageSize, Twips};

/// Как верстать документ.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum LayoutView {
    /// Как есть, по листам (разметка страницы, черновик, чтение).
    #[default]
    Pages,
    /// Веб-документ: ширина текста — `width` пт.
    Web {
        /// Ширина окна, пт документа.
        width: f32,
    },
    /// Структура: как веб-документ, плюс отступ по уровню заголовка.
    Outline {
        /// Ширина окна, пт документа.
        width: f32,
    },
}

impl LayoutView {
    /// Ширина окна в режимах без листов.
    pub fn width(self) -> Option<f32> {
        match self {
            LayoutView::Pages => None,
            LayoutView::Web { width } | LayoutView::Outline { width } => Some(width),
        }
    }
}

/// Что видно в режиме структуры.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutlineView {
    /// «Показать уровень»: заголовки до этого уровня без основного
    /// текста; `None` — «Все уровни».
    pub show_level: Option<u8>,
    /// Свёрнутые заголовки (номера абзацев): всё под ними скрыто.
    pub collapsed: BTreeSet<usize>,
}

/// Скрыт ли каждый абзац (по порядку, как в [`outline_levels`]) в режиме
/// структуры `view`.
pub fn outline_hidden(levels: &[Option<u8>], view: &OutlineView) -> Vec<bool> {
    let mut hidden = Vec::with_capacity(levels.len());
    // Уровень свёрнутого заголовка, под которым мы сейчас.
    let mut under: Option<u8> = None;
    for (index, level) in levels.iter().enumerate() {
        if let (Some(level), Some(top)) = (level, under)
            && *level <= top
        {
            under = None;
        }
        let by_level = match (view.show_level, level) {
            (None, _) => false,
            (Some(_), None) => true,
            (Some(show), Some(level)) => *level > show,
        };
        hidden.push(under.is_some() || by_level);
        if under.is_none()
            && let Some(level) = level
            && view.collapsed.contains(&index)
        {
            under = Some(*level);
        }
    }
    hidden
}

/// Поле вокруг текста в режимах без листов, пт.
pub const WEB_MARGIN: f32 = 12.0;

/// Высота «листа» без листов, пт: больше любого документа.
const ENDLESS: f32 = 2_000_000.0;

/// Шаг отступа уровня в режиме структуры (полдюйма, как в Word), пт.
pub const OUTLINE_STEP: f32 = 36.0;

/// Самая узкая ширина текста, пт: в совсем узком окне текст не
/// сжимается в столбик по букве.
const NARROWEST: f32 = 144.0;

fn twips(points: f32) -> Twips {
    Twips((points * Twips::PER_POINT as f32).round() as i32)
}

/// Копия документа `doc` для вёрстки в режиме `view`; `None` — верстать
/// сам документ.
/// Скрытые абзацы режима структуры — по `outline`.
pub fn view_document(doc: &Document, view: LayoutView, outline: &OutlineView) -> Option<Document> {
    let width = view.width()?;
    let mut copy = doc.clone();
    let page = (width.max(NARROWEST + 2.0 * WEB_MARGIN), ENDLESS);
    for section in &mut copy.sections {
        let props = &mut section.props;
        props.page_size = Some(PageSize {
            width: twips(page.0),
            height: twips(page.1),
            ..props.page_size.unwrap_or(PageSize::A4)
        });
        let margin = twips(WEB_MARGIN);
        props.margins = Some(PageMargins {
            top: margin,
            right: margin,
            bottom: margin,
            left: margin,
            header: Twips(0),
            footer: Twips(0),
            gutter: Twips(0),
        });
        props.columns = None;
        props.headers = Default::default();
        props.footers = Default::default();
        props.title_page = false;
        props.page_borders = None;
    }
    if let LayoutView::Outline { .. } = view {
        let levels = outline_levels(doc);
        let hidden = outline_hidden(&levels, outline);
        let mut index = 0;
        for section in &mut copy.sections {
            indent_outline(&mut section.blocks, &levels, &hidden, &mut index);
        }
    }
    Some(copy)
}

/// Уровень каждого абзаца в режиме структуры, по порядку, как в вёрстке
/// (абзацы ячеек таблиц тоже считаются): у заголовка — его уровень 1–9,
/// у основного текста — `None`.
pub fn outline_levels(doc: &Document) -> Vec<Option<u8>> {
    fn walk(doc: &Document, blocks: &[Block], out: &mut Vec<Option<u8>>) {
        for block in blocks {
            match block {
                Block::Paragraph(p) => out.push(outline_level(doc, &p.props)),
                Block::Table(t) => {
                    for cell in t.rows.iter().flat_map(|r| &r.cells) {
                        walk(doc, &cell.blocks, out);
                    }
                }
                Block::Unknown(_) => {}
            }
        }
    }
    let mut out = Vec::new();
    for section in &doc.sections {
        walk(doc, &section.blocks, &mut out);
    }
    out
}

/// Отступ абзаца в режиме структуры, пт: заголовок уровня N — на N − 1
/// шагов, основной текст — на шаг глубже заголовка над ним.
pub fn outline_indent(levels: &[Option<u8>], index: usize) -> f32 {
    let depth = match levels.get(index).copied().flatten() {
        Some(level) => level.saturating_sub(1),
        None => levels
            .get(..index)
            .unwrap_or_default()
            .iter()
            .rev()
            .find_map(|l| *l)
            .unwrap_or(0),
    };
    f32::from(depth) * OUTLINE_STEP
}

/// Ставит абзацам верхнего уровня отступ структуры; таблицы не трогает,
/// но их абзацы считает.
fn indent_outline(blocks: &mut [Block], levels: &[Option<u8>], hidden: &[bool], index: &mut usize) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                let indent = &mut p.props.indentation;
                indent.left = Some(twips(outline_indent(levels, *index)));
                indent.right = Some(Twips(0));
                indent.first_line = None;
                if hidden.get(*index).copied().unwrap_or(false) {
                    collapse(p);
                }
                *index += 1;
            }
            Block::Table(t) => {
                for cell in t.rows.iter_mut().flat_map(|r| &mut r.cells) {
                    let mut skip = *index;
                    count(&cell.blocks, &mut skip);
                    *index = skip;
                }
            }
            Block::Unknown(_) => {}
        }
    }
}

/// Скрытый абзац: строка почти нулевой высоты без текста, номера,
/// интервалов и рамок. Абзац остаётся на месте, чтобы номера абзацев
/// совпадали с документом.
fn collapse(p: &mut letter_model::Paragraph) {
    let props = &mut p.props;
    props.spacing.before = Some(Twips(0));
    props.spacing.after = Some(Twips(0));
    props.spacing.line = Some(LineSpacing::Exact(Twips(1)));
    props.numbering = Some(NumberingRef {
        id: Some(0),
        level: None,
    });
    props.borders = None;
    props.shading = None;
    props.page_break_before = Some(false);
    for inline in &mut p.content {
        if let letter_model::Inline::Run(run) = inline {
            run.props.hidden = Some(true);
        }
    }
}

fn count(blocks: &[Block], index: &mut usize) {
    for block in blocks {
        match block {
            Block::Paragraph(_) => *index += 1,
            Block::Table(t) => {
                for cell in t.rows.iter().flat_map(|r| &r.cells) {
                    count(&cell.blocks, index);
                }
            }
            Block::Unknown(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_layout::{FontLibrary, Shaper, layout_document};
    use letter_model::{Paragraph, ParagraphProps, Table, TableCell, TableRow};

    fn heading(level: u8, text: &str) -> Block {
        Block::Paragraph(Paragraph::text(text).with_props(ParagraphProps {
            style: Some(format!("Heading{level}")),
            ..ParagraphProps::default()
        }))
    }

    fn doc() -> Document {
        // Стили заголовков — из настоящего файла Word.
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/docx/headings.docx"
        );
        let mut doc = letter_docx::read_file(path).unwrap();
        doc.sections[0].blocks = vec![
            heading(1, "Глава"),
            Block::Paragraph(Paragraph::text("Текст главы")),
            heading(2, "Раздел"),
            Block::Table(Table {
                rows: vec![TableRow {
                    cells: vec![TableCell::text("ячейка")],
                    ..TableRow::default()
                }],
                ..Table::default()
            }),
            Block::Paragraph(Paragraph::text("Текст раздела")),
        ];
        doc
    }

    #[test]
    fn pages_view_keeps_the_document() {
        assert!(
            view_document(&Document::new(), LayoutView::Pages, &OutlineView::default()).is_none()
        );
    }

    #[test]
    fn web_text_follows_the_window() {
        let doc = Document::new();
        let copy = view_document(
            &doc,
            LayoutView::Web { width: 500.0 },
            &OutlineView::default(),
        )
        .unwrap();
        let layout = layout_document(&copy, &mut Shaper::new(), &FontLibrary::embedded_only());
        let page = &layout.pages[0];
        assert!((page.width - 500.0).abs() < 0.1);
        assert!((page.margins.left - WEB_MARGIN).abs() < 0.1);
        // Узкое окно не делает текст уже предела.
        let narrow = view_document(
            &doc,
            LayoutView::Web { width: 10.0 },
            &OutlineView::default(),
        )
        .unwrap();
        let width = narrow.sections[0].props.page_size.unwrap().width.0;
        assert!(width >= twips(NARROWEST).0);
    }

    #[test]
    fn outline_indents_by_level() {
        let doc = doc();
        let levels = outline_levels(&doc);
        // Глава, текст, раздел, ячейка, текст.
        assert_eq!(levels.len(), 5);
        assert_eq!(&levels[..3], &[Some(1), None, Some(2)]);
        assert_eq!(outline_indent(&levels, 0), 0.0);
        assert_eq!(outline_indent(&levels, 1), OUTLINE_STEP);
        assert_eq!(outline_indent(&levels, 2), OUTLINE_STEP);
        assert_eq!(outline_indent(&levels, 4), 2.0 * OUTLINE_STEP);

        let copy = view_document(
            &doc,
            LayoutView::Outline { width: 600.0 },
            &OutlineView::default(),
        )
        .unwrap();
        let left = |i: usize| match &copy.sections[0].blocks[i] {
            Block::Paragraph(p) => p.props.indentation.left,
            _ => None,
        };
        assert_eq!(left(1), Some(twips(OUTLINE_STEP)));
        // После таблицы счёт абзацев не сбился.
        assert_eq!(left(4), Some(twips(2.0 * OUTLINE_STEP)));
        // Сам документ не изменился.
        assert_eq!(doc.sections[0].blocks.len(), copy.sections[0].blocks.len());
        assert!(matches!(&doc.sections[0].blocks[1],
            Block::Paragraph(p) if p.props.indentation.left.is_none()));
    }

    #[test]
    fn outline_hides_by_level_and_collapse() {
        // Глава 1, текст, раздел 2, текст, глава 1.
        let levels = [Some(1), None, Some(2), None, Some(1)];
        let all = OutlineView::default();
        assert_eq!(outline_hidden(&levels, &all), [false; 5]);
        let level1 = OutlineView {
            show_level: Some(1),
            ..OutlineView::default()
        };
        assert_eq!(
            outline_hidden(&levels, &level1),
            [false, true, true, true, false]
        );
        let level2 = OutlineView {
            show_level: Some(2),
            ..OutlineView::default()
        };
        assert_eq!(
            outline_hidden(&levels, &level2),
            [false, true, false, true, false]
        );
        let collapsed = OutlineView {
            show_level: None,
            collapsed: [2].into(),
        };
        assert_eq!(
            outline_hidden(&levels, &collapsed),
            [false, false, false, true, false]
        );
        let collapsed = OutlineView {
            show_level: None,
            collapsed: [0, 2].into(),
        };
        assert_eq!(
            outline_hidden(&levels, &collapsed),
            [false, true, true, true, false]
        );

        // Скрытый абзац почти не занимает места.
        let doc = doc();
        let view = LayoutView::Outline { width: 600.0 };
        let open = view_document(&doc, view, &OutlineView::default()).unwrap();
        let closed = view_document(
            &doc,
            view,
            &OutlineView {
                show_level: None,
                collapsed: [0].into(),
            },
        )
        .unwrap();
        let fonts = FontLibrary::embedded_only();
        let open = layout_document(&open, &mut Shaper::new(), &fonts);
        let closed = layout_document(&closed, &mut Shaper::new(), &fonts);
        let bottom = |l: &letter_layout::DocumentLayout| {
            l.pages[0]
                .lines
                .iter()
                .map(|line| line.baseline)
                .fold(0.0, f32::max)
        };
        assert!(bottom(&closed) < bottom(&open) - 30.0);
    }
}
