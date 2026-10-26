//! Разделы: разрывы страницы, колонки и раздела; ориентация, размер,
//! поля и колонки разделов (кнопки вкладки «Макет»).

use letter_model::{
    Block, Border, BreakKind, Columns, Document, FieldChar, FieldCharKind, HeaderFooter,
    HeaderFooterKind, Orientation, PageBorders, PageMargins, PageSize, Paragraph, Run, RunContent,
    Section, SectionProps, SectionStart, Twips,
};

use crate::edit::{blocks_mut, clamp_offset, find, insert_items, split_content, top_level};
use crate::position::Position;

/// Что поменять в параметрах страницы.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSetup {
    /// Ориентация листа. Поля поворачиваются вместе с листом, как в Word.
    Orientation(Orientation),
    /// Размер листа (`orientation` не важна: ориентация остаётся прежней).
    Size(PageSize),
    /// Поля страницы; расстояния до колонтитулов и переплёт остаются
    /// прежними.
    Margins(PageMargins),
    /// Колонки текста.
    Columns(ColumnPreset),
    /// Границы страниц — во всём документе, как по умолчанию в Word.
    Borders(PageBorderPreset),
    /// Всё из окна «Параметры страницы»: лист с ориентацией и все поля
    /// (с колонтитулами и переплётом); `whole` — во всём документе, иначе
    /// в разделах выделения.
    Page {
        /// Лист.
        size: PageSize,
        /// Поля.
        margins: PageMargins,
        /// Во всём документе.
        whole: bool,
    },
}

/// Готовые варианты кнопки «Границы страниц».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageBorderPreset {
    /// Без рамки.
    None,
    /// Рамка: сплошная линия 0,5 пт в 24 пт от края листа.
    Box,
    /// Толстая рамка: 3 пт.
    Thick,
}

impl PageBorderPreset {
    /// Все варианты по порядку меню.
    pub const ALL: [PageBorderPreset; 3] = [
        PageBorderPreset::None,
        PageBorderPreset::Box,
        PageBorderPreset::Thick,
    ];

    /// Рамка варианта; `None` — без рамки.
    pub fn borders(self) -> Option<PageBorders> {
        let eighths = match self {
            PageBorderPreset::None => return None,
            PageBorderPreset::Box => 4,
            PageBorderPreset::Thick => 24,
        };
        let line = Border {
            space: Some(24),
            ..Border::single(eighths)
        };
        Some(PageBorders {
            top: Some(line.clone()),
            left: Some(line.clone()),
            bottom: Some(line.clone()),
            right: Some(line),
            from_page_edge: true,
            ..PageBorders::default()
        })
    }
}

/// Готовые варианты кнопки «Колонки».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnPreset {
    /// Одна колонка.
    One,
    /// Две одинаковые.
    Two,
    /// Три одинаковые.
    Three,
    /// Две: узкая слева.
    Left,
    /// Две: узкая справа.
    Right,
}

impl ColumnPreset {
    /// Все варианты по порядку меню.
    pub const ALL: [ColumnPreset; 5] = [
        ColumnPreset::One,
        ColumnPreset::Two,
        ColumnPreset::Three,
        ColumnPreset::Left,
        ColumnPreset::Right,
    ];

    /// Какой вариант у колонок `columns` (`None` — свои, не из меню).
    pub fn of(columns: Option<&Columns>) -> Option<ColumnPreset> {
        let Some(columns) = columns else {
            return Some(ColumnPreset::One);
        };
        if let [(first, _), (second, _)] = columns.widths[..] {
            return match first.0.cmp(&second.0) {
                std::cmp::Ordering::Less => Some(ColumnPreset::Left),
                std::cmp::Ordering::Greater => Some(ColumnPreset::Right),
                std::cmp::Ordering::Equal => Some(ColumnPreset::Two),
            };
        }
        if !columns.widths.is_empty() {
            return None;
        }
        match columns.count {
            0 | 1 => Some(ColumnPreset::One),
            2 => Some(ColumnPreset::Two),
            3 => Some(ColumnPreset::Three),
            _ => None,
        }
    }
}

/// Вставляет разрыв страницы или колонки (`kind`) у курсора и возвращает
/// позицию за ним.
pub fn insert_break(doc: &mut Document, at: Position, kind: BreakKind) -> Position {
    let Some(paragraph) = crate::edit::paragraph_mut(doc, at.paragraph) else {
        return at;
    };
    let offset = clamp_offset(paragraph, at.offset);
    insert_items(paragraph, offset, vec![RunContent::Break(kind)]);
    Position::new(at.paragraph, offset + 1)
}

/// Вставляет разрыв раздела у курсора: абзац делится, и всё после курсора
/// становится новым разделом с теми же параметрами страницы, который
/// начинается так, как сказано в `start`. В таблице ничего не делает.
/// Возвращает начало нового раздела.
pub fn insert_section_break(doc: &mut Document, at: Position, start: SectionStart) -> Position {
    let Some(path) = find(doc, at.paragraph) else {
        return at;
    };
    if !path.cells.is_empty() {
        return at;
    }
    let Some(blocks) = blocks_mut(doc, &path) else {
        return at;
    };
    let Some(Block::Paragraph(paragraph)) = blocks.get_mut(path.block) else {
        return at;
    };
    let offset = clamp_offset(paragraph, at.offset);
    let mut tail = Paragraph {
        props: paragraph.props.clone(),
        content: split_content(paragraph, offset),
    };
    tail.props.page_break_before = None;
    let Some(section) = doc.sections.get_mut(path.section) else {
        return at;
    };
    let mut rest = section.blocks.split_off(path.block + 1);
    rest.insert(0, tail.into());
    let next = Section {
        props: SectionProps {
            start: Some(start),
            ..section.props.clone()
        },
        blocks: rest,
    };
    doc.sections.insert(path.section + 1, next);
    Position::new(at.paragraph + 1, 0)
}

/// Раздел, чей колонтитул вида `kind` (`header` — верхний) виден
/// в разделе `section`: сам раздел или ближайший предыдущий, у которого
/// такой колонтитул задан. `None` — такого колонтитула нет.
pub fn header_footer_source(
    doc: &Document,
    section: usize,
    header: bool,
    kind: HeaderFooterKind,
) -> Option<usize> {
    let last = section.min(doc.sections.len().checked_sub(1)?);
    (0..=last).rev().find(|&s| {
        let props = &doc.sections[s].props;
        let set = if header {
            &props.headers
        } else {
            &props.footers
        };
        set.get(kind).is_some()
    })
}

/// Ставит разделу `section` колонтитул `story` вида `kind` (`header` —
/// верхний); `None` убирает его (тогда виден колонтитул предыдущего
/// раздела).
pub fn set_header_footer(
    doc: &mut Document,
    section: usize,
    header: bool,
    kind: HeaderFooterKind,
    story: Option<HeaderFooter>,
) {
    let Some(section) = doc.sections.get_mut(section) else {
        return;
    };
    let props = &mut section.props;
    let set = if header {
        &mut props.headers
    } else {
        &mut props.footers
    };
    *set.get_mut(kind) = story;
}

/// Включает или выключает у раздела `section` особый колонтитул первой
/// страницы.
pub fn set_title_page(doc: &mut Document, section: usize, on: bool) {
    if let Some(section) = doc.sections.get_mut(section) {
        section.props.title_page = on;
    }
}

/// Поле «номер страницы» (PAGE) из знаков поля: начало, код,
/// разделитель, результат «1», конец. При вёрстке колонтитула результат
/// заменяется номером страницы.
pub fn page_number_runs() -> Vec<Run> {
    let item = |content: RunContent| Run {
        content: vec![content],
        ..Run::default()
    };
    let mark = |kind| item(RunContent::FieldChar(FieldChar::new(kind)));
    vec![
        mark(FieldCharKind::Begin),
        item(RunContent::FieldCode(" PAGE ".into())),
        mark(FieldCharKind::Separate),
        Run::text("1"),
        mark(FieldCharKind::End),
    ]
}

/// Параметры страницы раздела, в котором абзац `paragraph`.
pub fn section_props(doc: &Document, paragraph: usize) -> Option<&SectionProps> {
    let (section, _) = top_level(doc, paragraph)?;
    doc.sections.get(section).map(|s| &s.props)
}

/// Меняет параметры страницы разделов, в которых лежат абзацы с `first`
/// по `last`.
pub fn set_page_setup(doc: &mut Document, first: usize, last: usize, setup: PageSetup) {
    if let PageSetup::Borders(preset) = setup {
        for section in &mut doc.sections {
            section.props.page_borders = preset.borders();
        }
        return;
    }
    if let PageSetup::Page { whole: true, .. } = setup {
        for section in &mut doc.sections {
            apply(&mut section.props, setup);
        }
        return;
    }
    let (Some((from, _)), Some((to, _))) = (top_level(doc, first), top_level(doc, last)) else {
        return;
    };
    for section in doc.sections.iter_mut().take(to + 1).skip(from) {
        apply(&mut section.props, setup);
    }
}

fn apply(props: &mut SectionProps, setup: PageSetup) {
    let size = props
        .page_size
        .filter(|s| s.width.0 > 0 && s.height.0 > 0)
        .unwrap_or(PageSize::A4);
    let margins = props.margins.unwrap_or_default();
    match setup {
        PageSetup::Orientation(orientation) => {
            if size.orientation != orientation {
                props.page_size = Some(size.rotated());
                props.margins = Some(rotate_margins(margins, orientation));
            }
        }
        PageSetup::Size(new) => {
            let (short, long) = if new.width.0 <= new.height.0 {
                (new.width, new.height)
            } else {
                (new.height, new.width)
            };
            let (width, height) = match size.orientation {
                Orientation::Portrait => (short, long),
                Orientation::Landscape => (long, short),
            };
            props.page_size = Some(PageSize {
                width,
                height,
                orientation: size.orientation,
            });
        }
        PageSetup::Margins(new) => {
            props.margins = Some(PageMargins {
                header: margins.header,
                footer: margins.footer,
                gutter: margins.gutter,
                ..new
            });
        }
        PageSetup::Columns(preset) => {
            let width = Twips(
                size.width.0 - margins.left.0.max(0) - margins.right.0.max(0) - margins.gutter.0,
            );
            props.columns = preset_columns(preset, props.columns.as_ref(), width);
        }
        PageSetup::Borders(preset) => props.page_borders = preset.borders(),
        PageSetup::Page {
            size: new, margins, ..
        } => {
            props.page_size = Some(new);
            props.margins = Some(margins);
        }
    }
}

/// Поля после поворота листа: при повороте в альбомную левое поле
/// становится верхним, верхнее — правым и так далее; обратно — наоборот.
fn rotate_margins(m: PageMargins, to: Orientation) -> PageMargins {
    let (top, right, bottom, left) = match to {
        Orientation::Landscape => (m.left, m.top, m.right, m.bottom),
        Orientation::Portrait => (m.right, m.bottom, m.left, m.top),
    };
    PageMargins {
        top,
        right,
        bottom,
        left,
        ..m
    }
}

/// Колонки варианта `preset` при ширине текста `width`. Промежуток
/// и линия между колонками берутся от прежних колонок.
fn preset_columns(preset: ColumnPreset, old: Option<&Columns>, width: Twips) -> Option<Columns> {
    let base = Columns {
        widths: Vec::new(),
        count: 1,
        ..old.cloned().unwrap_or_default()
    };
    let count = match preset {
        ColumnPreset::One => return None,
        ColumnPreset::Two => 2,
        ColumnPreset::Three => 3,
        ColumnPreset::Left | ColumnPreset::Right => {
            // Узкая колонка — около трети, как в Word.
            let free = (width.0 - base.space.0).max(2);
            let narrow = free * 305 / 1000;
            let wide = free - narrow;
            let (a, b) = if preset == ColumnPreset::Left {
                (narrow, wide)
            } else {
                (wide, narrow)
            };
            return Some(Columns {
                count: 2,
                widths: vec![(Twips(a), base.space), (Twips(b), Twips::ZERO)],
                ..base
            });
        }
    };
    Some(Columns { count, ..base })
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::Inline;

    fn doc(texts: &[&str]) -> Document {
        Document {
            sections: vec![Section {
                props: SectionProps {
                    page_size: Some(PageSize::A4),
                    margins: Some(PageMargins::default()),
                    ..SectionProps::default()
                },
                blocks: texts.iter().map(|t| Paragraph::text(*t).into()).collect(),
            }],
            ..Document::default()
        }
    }

    fn text(section: &Section) -> String {
        section.plain_text()
    }

    #[test]
    fn section_break_splits_the_section() {
        let mut d = doc(&["один два", "три"]);
        let at = insert_section_break(&mut d, Position::new(0, 8), SectionStart::Continuous);
        assert_eq!(at, Position::new(1, 0));
        assert_eq!(d.sections.len(), 2);
        assert_eq!(text(&d.sections[0]), "один");
        assert_eq!(text(&d.sections[1]), " два\nтри");
        assert_eq!(d.sections[0].props.start, None);
        assert_eq!(d.sections[1].props.start, Some(SectionStart::Continuous));
        assert_eq!(d.sections[1].props.page_size, Some(PageSize::A4));
    }

    #[test]
    fn no_section_break_in_a_table() {
        let mut d = doc(&["x"]);
        let at = crate::table::insert_table(&mut d, Position::new(0, 1), 1, 1);
        let before = d.clone();
        assert_eq!(insert_section_break(&mut d, at, SectionStart::NextPage), at);
        assert_eq!(d, before);
    }

    #[test]
    fn breaks_go_into_the_text() {
        let mut d = doc(&["ab"]);
        let at = insert_break(&mut d, Position::new(0, 1), BreakKind::Column);
        assert_eq!(at, Position::new(0, 2));
        let Some(Block::Paragraph(p)) = d.sections[0].blocks.first() else {
            panic!("нет абзаца");
        };
        let breaks: Vec<_> = p
            .content
            .iter()
            .filter_map(|i| match i {
                Inline::Run(r) => Some(r),
                _ => None,
            })
            .flat_map(|r| &r.content)
            .filter(|c| matches!(c, RunContent::Break(BreakKind::Column)))
            .collect();
        assert_eq!(breaks.len(), 1);
    }

    #[test]
    fn orientation_rotates_page_and_margins() {
        let mut d = doc(&["a"]);
        set_page_setup(&mut d, 0, 0, PageSetup::Orientation(Orientation::Landscape));
        let props = &d.sections[0].props;
        assert_eq!(props.page_size, Some(PageSize::A4.rotated()));
        let m = props.margins.unwrap_or_default();
        assert_eq!(m.top, PageMargins::default().left);
        assert_eq!(m.bottom, PageMargins::default().right);
        set_page_setup(&mut d, 0, 0, PageSetup::Orientation(Orientation::Portrait));
        assert_eq!(d.sections[0].props.page_size, Some(PageSize::A4));
        assert_eq!(d.sections[0].props.margins, Some(PageMargins::default()));
    }

    #[test]
    fn page_setup_window_sets_section_or_document() {
        let mut d = doc(&["a"]);
        let second = d.sections[0].clone();
        d.sections.push(second);
        let size = PageSize {
            width: Twips(10000),
            height: Twips(14000),
            orientation: Orientation::Portrait,
        };
        let margins = PageMargins {
            header: Twips(500),
            gutter: Twips(300),
            ..PageMargins::default()
        };
        let page = |whole| PageSetup::Page {
            size,
            margins,
            whole,
        };
        set_page_setup(&mut d, 0, 0, page(false));
        assert_eq!(d.sections[0].props.page_size, Some(size));
        assert_eq!(d.sections[0].props.margins, Some(margins));
        assert_ne!(d.sections[1].props.page_size, Some(size));
        set_page_setup(&mut d, 0, 0, page(true));
        assert_eq!(d.sections[1].props.margins, Some(margins));
    }

    #[test]
    fn size_keeps_orientation_and_margins_keep_header() {
        let mut d = doc(&["a"]);
        set_page_setup(&mut d, 0, 0, PageSetup::Orientation(Orientation::Landscape));
        set_page_setup(&mut d, 0, 0, PageSetup::Size(PageSize::LETTER));
        assert_eq!(
            d.sections[0].props.page_size,
            Some(PageSize::LETTER.rotated())
        );
        let narrow = PageMargins {
            top: Twips(720),
            right: Twips(720),
            bottom: Twips(720),
            left: Twips(720),
            header: Twips::ZERO,
            footer: Twips::ZERO,
            gutter: Twips::ZERO,
        };
        set_page_setup(&mut d, 0, 0, PageSetup::Margins(narrow));
        let m = d.sections[0].props.margins.unwrap_or_default();
        assert_eq!(m.left, Twips(720));
        assert_eq!(m.header, PageMargins::default().header);
    }

    #[test]
    fn column_presets() {
        let mut d = doc(&["a"]);
        for preset in ColumnPreset::ALL {
            set_page_setup(&mut d, 0, 0, PageSetup::Columns(preset));
            let columns = d.sections[0].props.columns.as_ref();
            assert_eq!(ColumnPreset::of(columns), Some(preset));
        }
        set_page_setup(&mut d, 0, 0, PageSetup::Columns(ColumnPreset::Left));
        let Some(columns) = &d.sections[0].props.columns else {
            panic!("нет колонок");
        };
        let total: i32 = columns.widths.iter().map(|(w, s)| w.0 + s.0).sum();
        assert_eq!(total, 11906 - 1701 - 850);
    }

    #[test]
    fn headers_come_from_earlier_sections() {
        let mut d = doc(&["a", "b"]);
        insert_section_break(&mut d, Position::new(1, 0), SectionStart::NextPage);
        let kind = HeaderFooterKind::Default;
        assert_eq!(header_footer_source(&d, 1, true, kind), None);
        let story = HeaderFooter {
            blocks: vec![Paragraph::text("h").into()],
            relationship: None,
        };
        set_header_footer(&mut d, 0, true, kind, Some(story));
        assert_eq!(header_footer_source(&d, 1, true, kind), Some(0));
        assert_eq!(header_footer_source(&d, 1, false, kind), None);
        set_header_footer(&mut d, 0, true, kind, None);
        assert_eq!(header_footer_source(&d, 1, true, kind), None);
    }

    #[test]
    fn page_number_field_shows_one() {
        let p = Paragraph::from_runs(page_number_runs());
        assert_eq!(p.plain_text(), "1");
        let mut d = doc(&["a"]);
        set_title_page(&mut d, 0, true);
        assert!(d.sections[0].props.title_page);
    }

    #[test]
    fn setup_changes_only_the_selected_sections() {
        let mut d = doc(&["a", "b"]);
        insert_section_break(&mut d, Position::new(1, 0), SectionStart::NextPage);
        set_page_setup(&mut d, 2, 2, PageSetup::Columns(ColumnPreset::Two));
        assert_eq!(d.sections[0].props.columns, None);
        assert_eq!(
            section_props(&d, 2)
                .and_then(|p| p.columns.as_ref())
                .map(|c| c.count),
            Some(2)
        );
    }
}
