//! Правка колонтитулов. Двойной щелчок по колонтитулу (или кнопка на
//! ленте) открывает его для правки: на время правки редактор работает
//! с отдельным документом из одного раздела, где блоки — содержимое
//! колонтитула, а сам документ откладывается. После каждой правки
//! колонтитул записывается в отложенный документ и тот верстается заново,
//! поэтому новый колонтитул сразу виден на всех страницах. Когда правка
//! кончается (Esc, двойной щелчок по тексту), изменение записывается
//! в историю документа одним шагом отмены.

use letter_editor::{History, Position, header_footer_source, set_header_footer};
use letter_layout::{DocumentLayout, PageStory};
use letter_model::{
    Block, Document, HeaderFooter, HeaderFooterKind, Paragraph, ParagraphProps, Section,
};

/// Колонтитул, открытый для правки.
pub struct StoryEdit {
    /// Сам документ (с колонтитулом после последней правки).
    pub body: Document,
    /// История правок документа (у колонтитула во время правки — своя).
    pub history: History,
    /// Курсор и якорь выделения в документе до правки колонтитула.
    pub cursor: Position,
    pub anchor: Position,
    /// Раздел, чей колонтитул правится.
    pub section: usize,
    /// Верхний колонтитул (иначе нижний).
    pub header: bool,
    /// Вид колонтитула.
    pub kind: HeaderFooterKind,
    /// Страница, на которой идёт правка.
    pub page: usize,
    /// Колонтитул до правки (`None` — его не было).
    original: Option<HeaderFooter>,
    /// Блоки, с которыми правка началась.
    start: Vec<Block>,
    /// Страницы документа, как их видно (курсор и выделение считаются
    /// по [`edit_layout`]).
    pub display: DocumentLayout,
}

impl StoryEdit {
    /// Начинает правку верхнего (`header`) или нижнего колонтитула,
    /// который виден на странице `page` раздела `section` с колонтитулами
    /// вида `kind`. Возвращает правку и документ колонтитула для редактора.
    #[allow(clippy::too_many_arguments)]
    pub fn begin(
        body: Document,
        history: History,
        cursor: Position,
        anchor: Position,
        display: DocumentLayout,
        page: usize,
        section: usize,
        kind: HeaderFooterKind,
        header: bool,
    ) -> (StoryEdit, Document) {
        // Колонтитул «как в предыдущем разделе» правится там, где задан.
        // Если его нет нигде, он появляется в первом разделе и виден во
        // всех, как в Word.
        let source = header_footer_source(&body, section, header, kind).unwrap_or(0);
        let props = &body.sections.get(source).map(|s| &s.props);
        let original = props
            .map(|p| if header { &p.headers } else { &p.footers })
            .and_then(|set| set.get(kind))
            .cloned();
        let start = match &original {
            Some(story) if !story.blocks.is_empty() => story.blocks.clone(),
            _ => vec![empty_paragraph(&body, header).into()],
        };
        let story = Document {
            sections: vec![Section {
                props: body
                    .sections
                    .get(section)
                    .map(|s| s.props.clone())
                    .unwrap_or_default(),
                blocks: start.clone(),
            }],
            styles: body.styles.clone(),
            numbering: body.numbering.clone(),
            theme: body.theme.clone(),
            settings: body.settings.clone(),
            ..Document::new()
        };
        let edit = StoryEdit {
            body,
            history,
            cursor,
            anchor,
            section: source,
            header,
            kind,
            page,
            original,
            start,
            display,
        };
        (edit, story)
    }

    /// Колонтитул после правки: `None` — не изменился.
    fn edited(&self, story: &Document) -> Option<Option<HeaderFooter>> {
        let blocks = story
            .sections
            .iter()
            .flat_map(|s| s.blocks.iter().cloned())
            .collect::<Vec<_>>();
        if blocks == self.start {
            return None;
        }
        let mut relationship = self.original.as_ref().and_then(|o| o.relationship.clone());
        // Часть файла, общая с колонтитулом другого раздела, не трогаем:
        // изменённый колонтитул получит свою.
        if relationship.as_ref().is_some_and(|r| self.shared(r)) {
            relationship = None;
        }
        Some(Some(HeaderFooter {
            blocks,
            relationship,
        }))
    }

    /// Ссылается ли на часть `relationship` ещё какой-то колонтитул.
    fn shared(&self, relationship: &str) -> bool {
        let mut count = 0;
        for section in &self.body.sections {
            for set in [&section.props.headers, &section.props.footers] {
                for (_, story) in set.iter() {
                    if story.relationship.as_deref() == Some(relationship) {
                        count += 1;
                    }
                }
            }
        }
        count > 1
    }

    /// Записывает колонтитул из `story` в документ (для показа).
    pub fn write_back(&mut self, story: &Document) {
        let new = self.edited(story).unwrap_or_else(|| self.original.clone());
        set_header_footer(&mut self.body, self.section, self.header, self.kind, new);
    }

    /// Кончает правку: документ с колонтитулом как до правки и сам новый
    /// колонтитул, если он изменился.
    pub fn finish(mut self, story: &Document) -> (Document, History, Option<Option<HeaderFooter>>) {
        let edited = self.edited(story);
        let original = self.original.clone();
        set_header_footer(
            &mut self.body,
            self.section,
            self.header,
            self.kind,
            original,
        );
        (self.body, self.history, edited)
    }
}

/// Пустой абзац нового колонтитула: со стилем «Верхний колонтитул» или
/// «Нижний колонтитул», если он есть в документе.
fn empty_paragraph(doc: &Document, header: bool) -> Paragraph {
    let name = if header { "header" } else { "footer" };
    Paragraph {
        props: ParagraphProps {
            style: crate::styles::built_in_id(doc, name),
            ..ParagraphProps::default()
        },
        content: Vec::new(),
    }
}

/// Страницы для курсора, выделения и щелчков во время правки
/// колонтитула: как `display`, но строки и рисунки страницы `page` —
/// колонтитула, а у остальных страниц их нет.
pub fn edit_layout(display: &DocumentLayout, page: usize, header: bool) -> DocumentLayout {
    let pages = display
        .pages
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut copy = p.clone();
            let story: PageStory = if i == page {
                if header {
                    p.header.clone()
                } else {
                    p.footer.clone()
                }
            } else {
                PageStory::default()
            };
            copy.lines = story.lines;
            copy.floats = story.floats;
            copy.shapes = Vec::new();
            copy.header = PageStory::default();
            copy.footer = PageStory::default();
            copy
        })
        .collect();
    DocumentLayout { pages }
}

/// Где на листе колонтитул: `Some(true)` — верхний, `Some(false)` —
/// нижний, `None` — текст. Граница — поле страницы или край колонтитула,
/// если он заходит за поле.
pub fn zone(page: &letter_layout::Page, y: f32) -> Option<bool> {
    let (header_end, footer_start) = bounds(page);
    if y < header_end {
        Some(true)
    } else if y > footer_start {
        Some(false)
    } else {
        None
    }
}

/// Где кончается место верхнего колонтитула и начинается место нижнего,
/// пт от верха листа.
pub fn bounds(page: &letter_layout::Page) -> (f32, f32) {
    let bottom = |story: &PageStory| {
        story
            .lines
            .iter()
            .map(|l| l.top + l.height)
            .fold(0.0, f32::max)
    };
    let top_of = |story: &PageStory| {
        story
            .lines
            .iter()
            .map(|l| l.top)
            .fold(f32::INFINITY, f32::min)
    };
    let header_end = page.margins.top.max(bottom(&page.header));
    let footer_start = (page.height - page.margins.bottom).min(top_of(&page.footer));
    (header_end, footer_start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::SectionProps;

    fn doc() -> Document {
        let mut doc = Document::new();
        doc.sections = vec![
            Section {
                props: SectionProps::default(),
                blocks: vec![Paragraph::text("one").into()],
            },
            Section {
                props: SectionProps::default(),
                blocks: vec![Paragraph::text("two").into()],
            },
        ];
        doc
    }

    fn begin(body: Document, section: usize) -> (StoryEdit, Document) {
        StoryEdit::begin(
            body,
            History::new(),
            Position::default(),
            Position::default(),
            DocumentLayout { pages: Vec::new() },
            0,
            section,
            HeaderFooterKind::Default,
            true,
        )
    }

    #[test]
    fn new_header_goes_to_the_first_section() {
        let (mut edit, mut story) = begin(doc(), 1);
        assert_eq!(edit.section, 0);
        story.sections[0].blocks = vec![Paragraph::text("top").into()];
        edit.write_back(&story);
        assert!(edit.body.sections[0].props.headers.default.is_some());
        let (body, _, edited) = edit.finish(&story);
        // Документ — как до правки; изменение — отдельно, для истории.
        assert_eq!(body, doc());
        let edited = edited.flatten().unwrap();
        assert_eq!(edited.blocks[0].plain_text(), "top");
    }

    #[test]
    fn untouched_header_is_not_a_change() {
        let (edit, story) = begin(doc(), 0);
        let (_, _, edited) = edit.finish(&story);
        assert_eq!(edited, None);
    }

    #[test]
    fn shared_part_is_not_overwritten() {
        let mut body = doc();
        let shared = HeaderFooter {
            blocks: vec![Paragraph::text("old").into()],
            relationship: Some("rId7".into()),
        };
        body.sections[0].props.headers.default = Some(shared.clone());
        body.sections[1].props.headers.default = Some(shared);
        let (edit, mut story) = begin(body, 1);
        assert_eq!(edit.section, 1);
        story.sections[0].blocks = vec![Paragraph::text("new").into()];
        let (_, _, edited) = edit.finish(&story);
        assert_eq!(edited.flatten().unwrap().relationship, None);
    }
}
