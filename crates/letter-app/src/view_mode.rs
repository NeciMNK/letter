//! Режимы просмотра, как в Word («Вид → Режимы» и кнопки в строке
//! состояния):
//!
//! - «Разметка страницы» — листы как на бумаге (обычный режим);
//! - «Режим чтения» — без ленты и линейки, лист целиком, правка
//!   выключена; выход — Esc;
//! - «Веб-документ» — текст во всю ширину окна, без листов, полей и
//!   колонтитулов;
//! - «Черновик» — текст с шириной строк как на бумаге, но без разбиения
//!   на листы, колонтитулов и плавающих полей сверху и снизу.
//!
//! Веб-документ и черновик верстаются из копии документа с другими
//! параметрами страницы ([`view_document`]): у копии те же абзацы, что
//! у документа, поэтому курсор, выделение и правка работают как обычно.
//! Сам документ (и то, что сохраняется и печатается) не меняется.

use letter_layout::DocumentLayout;
use letter_model::{
    Document, HeaderFooter, HeaderFooters, Orientation, PageMargins, PageSize, Twips,
};

/// Режим просмотра.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// Разметка страницы.
    #[default]
    Print,
    /// Режим чтения.
    Read,
    /// Веб-документ.
    Web,
    /// Черновик.
    Draft,
}

impl ViewMode {
    /// Верстается ли документ без листов (из копии с другими
    /// параметрами страницы).
    pub fn is_continuous(self) -> bool {
        matches!(self, ViewMode::Web | ViewMode::Draft)
    }

    /// Можно ли править документ.
    pub fn editable(self) -> bool {
        self != ViewMode::Read
    }
}

/// Высота «бесконечного» листа веб-документа и черновика, твипы
/// (около 14 м: текст обычного документа помещается на один лист,
/// лишнее потом обрезается, см. [`trim_pages`]).
const ENDLESS: i32 = 800_000;

/// Поля веб-документа слева и справа, твипы (0,25 дюйма).
const WEB_SIDE: i32 = 360;

/// Поля сверху и снизу у веб-документа и черновика, твипы.
const CONTINUOUS_TOP: i32 = 360;

/// Самая узкая строка веб-документа, пт: в очень узком окне текст не
/// сжимается в столбик по букве.
const MIN_WEB_WIDTH: f32 = 144.0;

/// Копия документа для вёрстки в режиме `mode`; `None` — верстается сам
/// документ. `web_width` — ширина окна в пунктах документа (для
/// веб-документа).
pub fn view_document(doc: &Document, mode: ViewMode, web_width: f32) -> Option<Document> {
    if !mode.is_continuous() {
        return None;
    }
    let mut copy = doc.clone();
    for section in &mut copy.sections {
        let props = &mut section.props;
        let page = props.page_size.unwrap_or(PageSize::A4);
        let mut margins = props.margins.unwrap_or_default();
        let width = match mode {
            ViewMode::Web => {
                margins.left = Twips(WEB_SIDE);
                margins.right = Twips(WEB_SIDE);
                margins.gutter = Twips(0);
                let width = web_width.max(MIN_WEB_WIDTH) * 20.0;
                Twips(width.round() as i32)
            }
            _ => page.width,
        };
        margins.top = Twips(CONTINUOUS_TOP);
        margins.bottom = Twips(CONTINUOUS_TOP);
        margins.header = Twips(0);
        margins.footer = Twips(0);
        props.margins = Some(PageMargins { ..margins });
        props.page_size = Some(PageSize {
            width,
            height: Twips(ENDLESS),
            orientation: Orientation::Portrait,
        });
        // Без колонок, колонтитулов и границ страниц.
        props.columns = None;
        props.page_borders = None;
        props.title_page = false;
        let empty = || HeaderFooters {
            default: Some(HeaderFooter::default()),
            first: Some(HeaderFooter::default()),
            even: Some(HeaderFooter::default()),
        };
        props.headers = empty();
        props.footers = empty();
    }
    Some(copy)
}

/// Обрезает «бесконечные» листы веб-документа и черновика по нижнему
/// краю содержимого (с полем снизу).
pub fn trim_pages(layout: &mut DocumentLayout) {
    let bottom_margin = CONTINUOUS_TOP as f32 / 20.0;
    for page in &mut layout.pages {
        let mut bottom: f32 = 0.0;
        for line in page
            .lines
            .iter()
            .chain(page.notes.iter().flat_map(|n| &n.story.lines))
        {
            bottom = bottom.max(line.top + line.height);
        }
        for shape in &page.shapes {
            bottom = bottom.max(shape.y + shape.height);
        }
        for float in &page.floats {
            bottom = bottom.max(float.y + float.height);
        }
        page.height = page.height.min(bottom + bottom_margin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_layout::{FontLibrary, Shaper, layout_document};
    use letter_model::Paragraph;

    fn doc() -> Document {
        let mut doc = Document::new();
        doc.sections[0].blocks = (0..80)
            .map(|i| Paragraph::text(format!("Абзац {i} с текстом подлиннее.")).into())
            .collect();
        doc.sections[0].props.headers.default = Some(HeaderFooter {
            blocks: vec![Paragraph::text("Колонтитул").into()],
            relationship: None,
        });
        doc
    }

    fn layout(doc: &Document) -> DocumentLayout {
        layout_document(doc, &mut Shaper::new(), &FontLibrary::embedded_only())
    }

    #[test]
    fn print_layout_uses_the_document_itself() {
        assert!(view_document(&doc(), ViewMode::Print, 500.0).is_none());
        assert!(view_document(&doc(), ViewMode::Read, 500.0).is_none());
        assert!(!ViewMode::Read.editable() && ViewMode::Draft.editable());
    }

    #[test]
    fn web_layout_fills_the_window_on_one_sheet() {
        let original = doc();
        let paged = layout(&original);
        assert!(paged.pages.len() > 1);
        assert!(!paged.pages[0].header.lines.is_empty());

        let web = view_document(&original, ViewMode::Web, 900.0).unwrap();
        // Абзацы те же: курсор и правка работают по тем же номерам.
        assert_eq!(web.sections[0].blocks, original.sections[0].blocks);
        let mut continuous = layout(&web);
        trim_pages(&mut continuous);
        assert_eq!(continuous.pages.len(), 1);
        let page = &continuous.pages[0];
        assert!((page.width - 900.0).abs() < 0.5);
        assert!(page.header.lines.is_empty());
        // Лист обрезан по тексту.
        let last = page.lines.last().unwrap();
        assert!(page.height < last.top + last.height + 30.0);
        // Узкое окно не сжимает текст в столбик.
        let narrow = view_document(&original, ViewMode::Web, 10.0).unwrap();
        assert!(layout(&narrow).pages[0].width >= MIN_WEB_WIDTH);
    }

    #[test]
    fn draft_keeps_line_width() {
        let original = doc();
        let paged = layout(&original);
        let draft = view_document(&original, ViewMode::Draft, 900.0).unwrap();
        let mut continuous = layout(&draft);
        trim_pages(&mut continuous);
        assert_eq!(continuous.pages.len(), 1);
        assert_eq!(continuous.pages[0].width, paged.pages[0].width);
        let text = |l: &DocumentLayout| -> Vec<String> {
            l.pages
                .iter()
                .flat_map(|p| p.lines.iter().map(|line| line.text.to_string()))
                .collect()
        };
        // Строки переносятся так же, как на бумаге.
        assert_eq!(text(&continuous), text(&paged));
    }
}
