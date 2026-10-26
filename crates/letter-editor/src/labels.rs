//! Наклейки для рассылки: лист наклеек — таблица на всю страницу, ячейка
//! на наклейку. Во всех наклейках, кроме первой, в начале стоит поле
//! `NEXT` («Следующая запись»): при слиянии каждая наклейка берёт свою
//! запись. «Обновить наклейки» повторяет первую наклейку во всех
//! остальных.

use letter_model::{
    Block, Document, LineSpacing, Orientation, PageMargins, PageSize, Paragraph, RowHeight,
    RowProps, Section, SectionProps, Table, TableCell, TableLayout, TableProps, TableRow,
    TableWidth, Twips, fields,
};

use crate::edit::insert_items;
use crate::merge::field_items;
use crate::position::Position;

/// Размеры листа наклеек (как в окне «Параметры наклейки» Word).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LabelSpec {
    /// Название (марка и номер листа).
    pub name: &'static str,
    /// Лист.
    pub page: PageSize,
    /// Наклеек в строке.
    pub columns: u32,
    /// Строк наклеек на листе.
    pub rows: u32,
    /// Ширина наклейки.
    pub width: Twips,
    /// Высота наклейки.
    pub height: Twips,
    /// Шаг по горизонтали: от левого края наклейки до левого края
    /// следующей.
    pub pitch_x: Twips,
    /// Шаг по вертикали.
    pub pitch_y: Twips,
    /// Отступ первой строки от верхнего края листа.
    pub top: Twips,
    /// Отступ первого столбца от левого края листа.
    pub side: Twips,
}

/// Сотые доли миллиметра в твипах.
const fn mm(hundredths: i32) -> Twips {
    Twips((hundredths * 1440 + 1270) / 2540)
}

/// Распространённые листы наклеек.
pub const LABELS: [LabelSpec; 6] = [
    LabelSpec {
        name: "Avery L7160",
        page: PageSize::A4,
        columns: 3,
        rows: 7,
        width: mm(6350),
        height: mm(3810),
        pitch_x: mm(6604),
        pitch_y: mm(3810),
        top: mm(1515),
        side: mm(721),
    },
    LabelSpec {
        name: "Avery L7163",
        page: PageSize::A4,
        columns: 2,
        rows: 7,
        width: mm(9910),
        height: mm(3810),
        pitch_x: mm(10160),
        pitch_y: mm(3810),
        top: mm(1515),
        side: mm(465),
    },
    LabelSpec {
        name: "Avery L7173",
        page: PageSize::A4,
        columns: 2,
        rows: 5,
        width: mm(9910),
        height: mm(5700),
        pitch_x: mm(10160),
        pitch_y: mm(5700),
        top: mm(600),
        side: mm(465),
    },
    LabelSpec {
        name: "Avery L7651",
        page: PageSize::A4,
        columns: 5,
        rows: 13,
        width: mm(3810),
        height: mm(2120),
        pitch_x: mm(4064),
        pitch_y: mm(2120),
        top: mm(1070),
        side: mm(467),
    },
    LabelSpec {
        name: "Avery 5160",
        page: PageSize::LETTER,
        columns: 3,
        rows: 10,
        width: Twips(3780),
        height: Twips(1440),
        pitch_x: Twips(3960),
        pitch_y: Twips(1440),
        top: Twips(720),
        side: Twips(270),
    },
    LabelSpec {
        name: "Avery 5163",
        page: PageSize::LETTER,
        columns: 2,
        rows: 5,
        width: Twips(5760),
        height: Twips(2880),
        pitch_x: Twips(6030),
        pitch_y: Twips(2880),
        top: Twips(720),
        side: Twips(225),
    },
];

/// Код поля «Следующая запись».
const NEXT_CODE: &str = " NEXT ";

/// Что поле «Следующая запись» показывает вне просмотра.
pub(crate) const NEXT_SHOWN: &str = "«Next Record»";

/// Абзац внутри наклейки: без интервалов, с небольшими отступами от
/// краёв (как у Word).
fn label_paragraph() -> Paragraph {
    let mut paragraph = Paragraph::default();
    paragraph.props.indentation.left = Some(Twips(95));
    paragraph.props.indentation.right = Some(Twips(95));
    paragraph.props.spacing.before = Some(Twips(0));
    paragraph.props.spacing.after = Some(Twips(0));
    paragraph.props.spacing.line = Some(LineSpacing::SINGLE);
    paragraph
}

/// Ставит поле «Следующая запись» в начало ячейки.
fn add_next(cell: &mut TableCell) {
    if !matches!(cell.blocks.first(), Some(Block::Paragraph(_))) {
        cell.blocks.insert(0, label_paragraph().into());
    }
    if let Some(Block::Paragraph(paragraph)) = cell.blocks.first_mut() {
        insert_items(paragraph, 0, field_items(NEXT_CODE.into(), NEXT_SHOWN));
    }
}

/// Заменяет документ листом наклеек `spec`: таблица на всю страницу,
/// между наклейками — пустые столбцы-промежутки. Возвращает место в
/// первой наклейке.
pub fn make_labels(doc: &mut Document, spec: &LabelSpec) -> Position {
    let columns = spec.columns.max(1) as usize;
    let gap = Twips((spec.pitch_x.0 - spec.width.0).max(0));
    let mut grid = Vec::new();
    for c in 0..columns {
        grid.push(spec.width);
        if gap.0 > 0 && c + 1 < columns {
            grid.push(gap);
        }
    }
    let total: i32 = grid.iter().map(|w| w.0).sum();
    let mut rows = Vec::new();
    for r in 0..spec.rows.max(1) {
        let cells = grid
            .iter()
            .enumerate()
            .map(|(k, width)| {
                let mut cell = TableCell {
                    blocks: vec![label_paragraph().into()],
                    ..TableCell::default()
                };
                cell.props.width = Some(TableWidth::Twips(*width));
                // Промежутки — нечётные столбцы, если они есть.
                let label = gap.0 == 0 || k % 2 == 0;
                if label && (r > 0 || k > 0) {
                    add_next(&mut cell);
                }
                cell
            })
            .collect();
        rows.push(TableRow {
            props: RowProps {
                height: Some(RowHeight {
                    value: spec.pitch_y,
                    rule: letter_model::HeightRule::Exact,
                }),
                cant_split: Some(true),
                ..RowProps::default()
            },
            cells,
            ..TableRow::default()
        });
    }
    let table = Table {
        props: Box::new(TableProps {
            width: Some(TableWidth::Twips(Twips(total))),
            layout: Some(TableLayout::Fixed),
            cell_margins: Some(letter_model::CellMargins {
                left: Some(TableWidth::Twips(Twips(15))),
                right: Some(TableWidth::Twips(Twips(15))),
                ..Default::default()
            }),
            ..TableProps::default()
        }),
        grid,
        rows,
        ..Table::default()
    };
    // Абзац после таблицы — крошечный, чтобы не уйти на новую страницу.
    let mut last = label_paragraph();
    last.props.spacing.line = Some(LineSpacing::Exact(Twips(20)));
    let page = PageSize {
        orientation: Orientation::Portrait,
        ..spec.page
    };
    let props = SectionProps {
        page_size: Some(page),
        margins: Some(PageMargins {
            top: spec.top,
            right: Twips((page.width.0 - spec.side.0 - total).max(0)),
            bottom: Twips(0),
            left: spec.side,
            header: Twips(0),
            footer: Twips(0),
            gutter: Twips(0),
        }),
        ..SectionProps::default()
    };
    doc.sections = vec![Section {
        props,
        blocks: vec![table.into(), last.into()],
    }];
    Position::new(0, 0)
}

/// Первая таблица документа (лист наклеек).
fn first_table(doc: &mut Document) -> Option<&mut Table> {
    doc.sections
        .iter_mut()
        .flat_map(|s| s.blocks.iter_mut())
        .find_map(|block| match block {
            Block::Table(table) => Some(table),
            _ => None,
        })
}

/// «Обновить наклейки»: содержимое первой наклейки (первой ячейки
/// первой таблицы) повторяется во всех наклейках — ячейках той же
/// ширины — после поля «Следующая запись». Возвращает, поменялось ли
/// что-нибудь.
pub fn update_labels(doc: &mut Document) -> bool {
    let Some(table) = first_table(doc) else {
        return false;
    };
    let Some(first) = table.rows.first().and_then(|row| row.cells.first()) else {
        return false;
    };
    let width = first.props.width;
    let template = first.blocks.clone();
    let mut changed = false;
    for (r, row) in table.rows.iter_mut().enumerate() {
        for (k, cell) in row.cells.iter_mut().enumerate() {
            if (r == 0 && k == 0) || cell.props.width != width {
                continue;
            }
            let mut copy = TableCell {
                blocks: template.clone(),
                ..cell.clone()
            };
            add_next(&mut copy);
            if copy != *cell {
                *cell = copy;
                changed = true;
            }
        }
    }
    changed
}

/// Есть ли в документе поле «Следующая запись» (документ — лист
/// наклеек или каталог).
pub fn has_next_records(doc: &Document) -> bool {
    (0..crate::edit::paragraph_count(doc)).any(|index| {
        crate::edit::paragraph_ref(doc, index)
            .is_some_and(|p| fields(p).iter().any(|f| f.name() == "NEXT"))
    })
}

/// Сколько полей «Следующая запись» верхнего уровня в абзаце.
pub(crate) fn next_count(paragraph: &Paragraph) -> usize {
    fields(paragraph)
        .iter()
        .filter(|f| f.depth == 0 && f.name() == "NEXT")
        .count()
}

/// Есть ли в абзаце начало поля.
#[cfg(test)]
fn has_field_marks(paragraph: &Paragraph) -> bool {
    paragraph.content.iter().any(|inline| {
        matches!(inline, letter_model::Inline::Run(run) if run.content.iter().any(|c| matches!(
            c,
            letter_model::RunContent::FieldChar(f) if f.kind == letter_model::FieldCharKind::Begin
        )))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merge::{MergeData, insert_merge_field, merge_all, show_record};

    fn cell_text(doc: &Document, row: usize, cell: usize) -> String {
        match &doc.sections[0].blocks[0] {
            Block::Table(table) => table.rows[row].cells[cell].plain_text(),
            _ => String::new(),
        }
    }

    #[test]
    fn sheet_has_a_cell_per_label() {
        let mut doc = Document::default();
        make_labels(&mut doc, &LABELS[0]);
        let Block::Table(table) = &doc.sections[0].blocks[0] else {
            panic!("нет таблицы");
        };
        assert_eq!(table.rows.len(), 7);
        // Три наклейки и два промежутка.
        assert_eq!(table.grid.len(), 5);
        assert_eq!(cell_text(&doc, 0, 0), "");
        assert_eq!(cell_text(&doc, 0, 1), "");
        assert_eq!(cell_text(&doc, 0, 2), NEXT_SHOWN);
        assert!(has_next_records(&doc));
        let Block::Paragraph(first) = &table.rows[0].cells[0].blocks[0] else {
            panic!("нет абзаца");
        };
        assert!(!has_field_marks(first));
        let margins = doc.sections[0].props.margins.expect("поля");
        assert_eq!(margins.top, mm(1515));
        assert!(margins.right.0 >= 0);
    }

    #[test]
    fn labels_take_one_record_each() {
        let mut doc = Document::default();
        make_labels(&mut doc, &LABELS[4]);
        // Первая ячейка — первый абзац документа.
        insert_merge_field(&mut doc, Position::new(0, 0), "Имя");
        assert!(update_labels(&mut doc));
        assert!(!update_labels(&mut doc), "второй раз ничего не меняется");
        assert_eq!(cell_text(&doc, 0, 2), format!("{NEXT_SHOWN}«Имя»"));
        // Промежуток остаётся пустым.
        assert_eq!(cell_text(&doc, 0, 1), "");
        let data = MergeData::from_csv("Имя\nВера\nАнна\nОля\nИра\n");
        show_record(&mut doc, Some((&data, 0)));
        assert_eq!(cell_text(&doc, 0, 0), "Вера");
        assert_eq!(cell_text(&doc, 0, 2), "Анна");
        assert_eq!(cell_text(&doc, 0, 4), "Оля");
        assert_eq!(cell_text(&doc, 1, 0), "Ира");
        // Записи кончились — наклейки пустые.
        assert_eq!(cell_text(&doc, 1, 2), "");
        // 30 наклеек на листе: четыре записи — один лист.
        let merged = merge_all(&doc, &data);
        assert_eq!(merged.sections.len(), 1);
        assert!(!has_next_records(&merged));
        let more: String = (0..35).map(|n| format!("{n}\n")).collect();
        let data = MergeData::from_csv(&format!("Имя\n{more}"));
        let merged = merge_all(&doc, &data);
        assert_eq!(merged.sections.len(), 2);
        let Block::Table(table) = &merged.sections[1].blocks[0] else {
            panic!("нет таблицы");
        };
        assert_eq!(table.rows[0].cells[0].plain_text(), "30");
        assert_eq!(table.rows[1].cells[2].plain_text(), "34");
        assert_eq!(table.rows[1].cells[4].plain_text(), "");
    }
}
