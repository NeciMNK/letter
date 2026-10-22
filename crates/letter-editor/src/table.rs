//! Таблицы в редакторе: вставка таблицы, переход между ячейками по Tab,
//! добавление и удаление строк и столбцов, удаление таблицы.
//!
//! Новая таблица — как в Word: стиль «Сетка таблицы» (`TableGrid`,
//! тонкие линии со всех сторон, абзацы без интервала после), столбцы
//! одинаковой ширины во всю ширину текста. Если в документе нет этого
//! стиля, он добавляется (вместе со стилем таблиц по умолчанию).

use letter_model::{
    Block, Border, Borders, CellMargins, Color, Document, LineSpacing, PageSize, Paragraph,
    Shading, Style, StyleKind, Table, TableCell, TableProps, TableRow, TableWidth, Twips,
    VerticalMerge,
};

use crate::edit::{
    Path, blocks_mut, find, paragraph_count, paragraph_ref, split_paragraph, text_len,
};
use crate::position::Position;
use crate::selection::Selection;

/// Стиль новых таблиц.
const GRID_STYLE: &str = "TableGrid";
/// Поле ячейки слева и справа у стиля таблиц по умолчанию, твипы.
const SIDE_MARGIN: i32 = 108;

/// Где стоит ячейка: путь к таблице, строка и номер ячейки в строке.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CellRef {
    /// Путь к таблице: раздел, ячейки внешних таблиц, номер блока.
    table: Path,
    row: usize,
    cell: usize,
}

/// Ячейка (самой внутренней таблицы), в которой стоит абзац `paragraph`.
pub(crate) fn cell_of(doc: &Document, paragraph: usize) -> Option<CellRef> {
    let path = find(doc, paragraph)?;
    let (&(block, row, cell), outer) = path.cells.split_last()?;
    Some(CellRef {
        table: Path {
            section: path.section,
            cells: outer.to_vec(),
            block,
        },
        row,
        cell,
    })
}

/// Стоит ли абзац `paragraph` в таблице.
pub fn in_table(doc: &Document, paragraph: usize) -> bool {
    cell_of(doc, paragraph).is_some()
}

fn table_ref<'a>(doc: &'a Document, path: &Path) -> Option<&'a Table> {
    let mut blocks = &doc.sections.get(path.section)?.blocks;
    for &(b, r, c) in &path.cells {
        let Some(Block::Table(table)) = blocks.get(b) else {
            return None;
        };
        blocks = &table.rows.get(r)?.cells.get(c)?.blocks;
    }
    match blocks.get(path.block)? {
        Block::Table(table) => Some(table),
        _ => None,
    }
}

fn table_mut<'a>(doc: &'a mut Document, path: &Path) -> Option<&'a mut Table> {
    match blocks_mut(doc, path)?.get_mut(path.block)? {
        Block::Table(table) => Some(table),
        _ => None,
    }
}

/// Сколько абзацев в блоках (с ячейками таблиц).
fn count(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|b| match b {
            Block::Paragraph(_) => 1,
            Block::Table(t) => t
                .rows
                .iter()
                .flat_map(|r| &r.cells)
                .map(|c| count(&c.blocks))
                .sum(),
            Block::Unknown(_) => 0,
        })
        .sum()
}

/// Номер первого абзаца блока по пути `path` (или абзаца, который
/// встанет на это место).
fn first_paragraph(doc: &Document, path: &Path) -> usize {
    let mut index: usize = doc
        .sections
        .iter()
        .take(path.section)
        .map(|s| count(&s.blocks))
        .sum();
    let Some(section) = doc.sections.get(path.section) else {
        return index;
    };
    let mut blocks = &section.blocks;
    for &(b, r, c) in &path.cells {
        index += count(&blocks[..b.min(blocks.len())]);
        let Some(Block::Table(table)) = blocks.get(b) else {
            return index;
        };
        for row in table.rows.iter().take(r) {
            index += row.cells.iter().map(|c| count(&c.blocks)).sum::<usize>();
        }
        let Some(row) = table.rows.get(r) else {
            return index;
        };
        index += row
            .cells
            .iter()
            .take(c)
            .map(|c| count(&c.blocks))
            .sum::<usize>();
        let Some(cell) = row.cells.get(c) else {
            return index;
        };
        blocks = &cell.blocks;
    }
    index + count(&blocks[..path.block.min(blocks.len())])
}

/// Номер первого абзаца ячейки.
fn cell_start(doc: &Document, table: &Path, row: usize, cell: usize) -> usize {
    let mut cells = table.cells.clone();
    cells.push((table.block, row, cell));
    first_paragraph(
        doc,
        &Path {
            section: table.section,
            cells,
            block: 0,
        },
    )
}

/// Ширина, в которую встаёт таблица в месте `path`, твипы: ширина
/// текста раздела или ячейки.
fn available_width(doc: &Document, path: &Path) -> i32 {
    let Some(section) = doc.sections.get(path.section) else {
        return 9000;
    };
    let size = section
        .props
        .page_size
        .filter(|s| s.width.0 > 0)
        .unwrap_or(PageSize::A4);
    let m = section.props.margins.unwrap_or_default();
    let mut width = size.width.0 - m.left.0.max(0) - m.right.0.max(0) - m.gutter.0.max(0);
    // Во вложенной таблице — ширина ячейки за вычетом полей.
    let mut blocks = &section.blocks;
    for &(b, r, c) in &path.cells {
        let Some(Block::Table(table)) = blocks.get(b) else {
            break;
        };
        let start: usize = table
            .rows
            .get(r)
            .map_or(0, |row| row.cells.iter().take(c).map(TableCell::span).sum());
        let Some(cell) = table.rows.get(r).and_then(|row| row.cells.get(c)) else {
            break;
        };
        let grid: i32 = table
            .grid
            .iter()
            .skip(start)
            .take(cell.span())
            .map(|t| t.0)
            .sum();
        if grid > 0 {
            width = grid - 2 * SIDE_MARGIN;
        }
        blocks = &cell.blocks;
    }
    width.max(720)
}

/// Добавляет стили «Обычная таблица» (если нет стиля таблиц по
/// умолчанию) и «Сетка таблицы» (если нет).
fn ensure_grid_style(doc: &mut Document) {
    if doc.styles.default_for(StyleKind::Table).is_none() && doc.styles.get("TableNormal").is_none()
    {
        let mut normal = Style::new("TableNormal", StyleKind::Table);
        normal.name = Some("Normal Table".into());
        normal.is_default = true;
        normal.table.indent = Some(TableWidth::Twips(Twips(0)));
        normal.table.cell_margins = Some(CellMargins {
            top: Some(TableWidth::Twips(Twips(0))),
            left: Some(TableWidth::Twips(Twips(SIDE_MARGIN))),
            bottom: Some(TableWidth::Twips(Twips(0))),
            right: Some(TableWidth::Twips(Twips(SIDE_MARGIN))),
        });
        doc.styles.styles.push(normal);
    }
    if doc.styles.get(GRID_STYLE).is_none() {
        let mut grid = Style::new(GRID_STYLE, StyleKind::Table);
        grid.name = Some("Table Grid".into());
        grid.based_on = doc
            .styles
            .default_for(StyleKind::Table)
            .map(|s| s.id.clone());
        grid.paragraph.spacing.after = Some(Twips(0));
        grid.paragraph.spacing.line = Some(LineSpacing::SINGLE);
        grid.table.borders = Some(Borders::all(Border::single(4)));
        doc.styles.styles.push(grid);
    }
}

/// Вставляет таблицу `rows` × `columns` у позиции `at`: абзац делится
/// в этом месте (в начале абзаца таблица встаёт перед ним). Возвращает
/// позицию в первой ячейке.
pub fn insert_table(doc: &mut Document, at: Position, rows: usize, columns: usize) -> Position {
    let rows = rows.clamp(1, 63);
    let columns = columns.clamp(1, 63);
    let Some(paragraph) = paragraph_ref(doc, at.paragraph) else {
        return at;
    };
    let at = if at.offset > 0 && text_len(paragraph) > 0 {
        split_paragraph(doc, at)
    } else {
        Position::new(at.paragraph, 0)
    };
    let Some(path) = find(doc, at.paragraph) else {
        return at;
    };
    ensure_grid_style(doc);
    let width = available_width(doc, &path) / columns as i32;
    let row = TableRow {
        cells: (0..columns)
            .map(|_| TableCell {
                props: letter_model::CellProps {
                    width: Some(TableWidth::Twips(Twips(width))),
                    ..Default::default()
                },
                blocks: vec![Paragraph::default().into()],
                ..TableCell::default()
            })
            .collect(),
        ..TableRow::default()
    };
    let table = Table {
        props: Box::new(TableProps {
            style: Some(GRID_STYLE.into()),
            width: Some(TableWidth::Auto),
            ..TableProps::default()
        }),
        grid: vec![Twips(width); columns],
        raw_props: Vec::new(),
        rows: vec![row; rows],
    };
    let Some(blocks) = blocks_mut(doc, &path) else {
        return at;
    };
    blocks.insert(path.block, table.into());
    Position::new(at.paragraph, 0)
}

/// Tab в таблице: выделяет содержимое следующей (`back` — предыдущей)
/// ячейки. `None`, если абзац не в таблице или ячейки дальше нет.
pub fn next_cell(doc: &Document, paragraph: usize, back: bool) -> Option<Selection> {
    let at = cell_of(doc, paragraph)?;
    let table = table_ref(doc, &at.table)?;
    let (row, cell) = if back {
        if at.cell > 0 {
            (at.row, at.cell - 1)
        } else {
            let row = at.row.checked_sub(1)?;
            (row, table.rows.get(row)?.cells.len().checked_sub(1)?)
        }
    } else if at.cell + 1 < table.rows.get(at.row)?.cells.len() {
        (at.row, at.cell + 1)
    } else {
        let row = at.row + 1;
        table.rows.get(row)?.cells.first()?;
        (row, 0)
    };
    Some(cell_selection(doc, &at.table, row, cell))
}

/// Всё содержимое ячейки.
fn cell_selection(doc: &Document, table: &Path, row: usize, cell: usize) -> Selection {
    let first = cell_start(doc, table, row, cell);
    let paragraphs = table_ref(doc, table)
        .and_then(|t| t.rows.get(row)?.cells.get(cell))
        .map_or(0, |c| count(&c.blocks));
    let last = first + paragraphs.saturating_sub(1);
    let end = paragraph_ref(doc, last).map_or(0, text_len);
    Selection::new(Position::new(first, 0), Position::new(last, end))
}

/// Строки таблицы с ячейками выделенного (`start`, `end`): номера первой
/// и последней. `None`, если начало не в таблице.
fn row_range(doc: &Document, start: usize, end: usize) -> Option<(CellRef, usize, usize)> {
    let first = cell_of(doc, start)?;
    let last = cell_of(doc, end)
        .filter(|c| c.table == first.table)
        .map_or(first.row, |c| c.row);
    let (a, b) = (first.row.min(last), first.row.max(last));
    Some((first, a, b))
}

/// Пустая копия строки: те же ячейки с теми же свойствами, в каждой —
/// пустой абзац с оформлением первого абзаца ячейки.
fn empty_like(row: &TableRow) -> TableRow {
    let mut new = row.clone();
    new.props.header = None;
    for cell in &mut new.cells {
        let props = cell
            .blocks
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) => Some(p.props.clone()),
                _ => None,
            })
            .unwrap_or_default();
        cell.blocks = vec![
            Paragraph {
                props,
                content: Vec::new(),
            }
            .into(),
        ];
        cell.props.v_merge = None;
    }
    new
}

/// Вставляет над (`below == false`) или под строками выделенного
/// столько же новых пустых строк. Возвращает позицию в первой ячейке
/// первой новой строки.
pub fn insert_rows(doc: &mut Document, start: usize, end: usize, below: bool) -> Option<Position> {
    let (at, first, last) = row_range(doc, start, end)?;
    let table = table_mut(doc, &at.table)?;
    let source = if below { last } else { first };
    let template = empty_like(table.rows.get(source)?);
    let place = if below { last + 1 } else { first };
    let count = last - first + 1;
    for _ in 0..count {
        table.rows.insert(place, template.clone());
    }
    Some(Position::new(cell_start(doc, &at.table, place, 0), 0))
}

/// Столбцы сетки, которые занимает ячейка: начало и конец (не включая).
fn grid_span(row: &TableRow, cell: usize) -> (usize, usize) {
    let start: usize = row.cells.iter().take(cell).map(TableCell::span).sum();
    let span = row.cells.get(cell).map_or(1, TableCell::span);
    (start, start + span)
}

/// Столбцы сетки выделенного: от ячейки начала до ячейки конца.
fn column_range(doc: &Document, start: usize, end: usize) -> Option<(CellRef, usize, usize)> {
    let first = cell_of(doc, start)?;
    let table = table_ref(doc, &first.table)?;
    let (a0, a1) = grid_span(table.rows.get(first.row)?, first.cell);
    let (b0, b1) = match cell_of(doc, end).filter(|c| c.table == first.table) {
        Some(last) => grid_span(table.rows.get(last.row)?, last.cell),
        None => (a0, a1),
    };
    Some((first, a0.min(b0), a1.max(b1)))
}

/// Ширины ячеек — по сетке.
fn fit_cells_to_grid(table: &mut Table) {
    for row in &mut table.rows {
        let mut start = 0;
        for cell in &mut row.cells {
            let span = cell.span();
            let width: i32 = table.grid.iter().skip(start).take(span).map(|t| t.0).sum();
            if width > 0 {
                cell.props.width = Some(TableWidth::Twips(Twips(width)));
            }
            start += span;
        }
    }
}

/// Вставляет слева (`right == false`) или справа от столбцов выделенного
/// столько же новых столбцов. Ширина таблицы сохраняется: все столбцы
/// становятся уже. Возвращает позицию в новой ячейке строки с курсором.
pub fn insert_columns(
    doc: &mut Document,
    start: usize,
    end: usize,
    right: bool,
) -> Option<Position> {
    let (at, g0, g1) = column_range(doc, start, end)?;
    let table = table_mut(doc, &at.table)?;
    let count = g1 - g0;
    let place = if right { g1 } else { g0 };
    let columns: usize = table
        .rows
        .iter()
        .map(|r| r.cells.iter().map(TableCell::span).sum::<usize>())
        .max()
        .unwrap_or(0);
    if table.grid.len() < columns {
        let each = table.grid.last().map_or(1440, |t| t.0);
        table.grid.resize(columns, Twips(each));
    }
    let total: i64 = table.grid.iter().map(|t| i64::from(t.0)).sum();
    let new_widths: Vec<i32> = table.grid[g0..g1.min(table.grid.len())]
        .iter()
        .map(|t| t.0)
        .collect();
    for (k, &w) in new_widths.iter().enumerate() {
        table.grid.insert(place + k, Twips(w));
    }
    // Прежняя ширина таблицы.
    let wider: i64 = table.grid.iter().map(|t| i64::from(t.0)).sum();
    if wider > 0 && total > 0 {
        for t in &mut table.grid {
            t.0 = (i64::from(t.0) * total / wider) as i32;
        }
    }
    let mut new_cell_index = 0;
    for (r, row) in table.rows.iter_mut().enumerate() {
        // Ячейка, перед которой встают новые: первая с началом ≥ place.
        let mut start = 0;
        let mut index = row.cells.len();
        let mut widen = None;
        for (c, cell) in row.cells.iter().enumerate() {
            let span = cell.span();
            if start >= place {
                index = c;
                break;
            }
            if start + span > place {
                widen = Some(c);
                break;
            }
            start += span;
        }
        if let Some(c) = widen {
            // Новый столбец проходит через объединённую ячейку — она шире.
            let cell = &mut row.cells[c];
            cell.props.grid_span = Some((cell.span() + count) as u32);
            continue;
        }
        // Образец — соседняя ячейка.
        let neighbour = if right {
            index.checked_sub(1)
        } else {
            Some(index.min(row.cells.len().saturating_sub(1)))
        };
        let template = neighbour
            .and_then(|n| row.cells.get(n))
            .map(|c| {
                let mut cell = empty_like(&TableRow {
                    cells: vec![c.clone()],
                    ..TableRow::default()
                })
                .cells
                .remove(0);
                cell.props.grid_span = None;
                cell
            })
            .unwrap_or_else(|| TableCell {
                blocks: vec![Paragraph::default().into()],
                ..TableCell::default()
            });
        for k in 0..count {
            row.cells.insert(index + k, template.clone());
        }
        if r == at.row {
            new_cell_index = index;
        }
    }
    fit_cells_to_grid(table);
    Some(Position::new(
        cell_start(doc, &at.table, at.row, new_cell_index),
        0,
    ))
}

/// Удаляет таблицу по пути `path`; после неё всегда остаётся абзац.
/// Возвращает позицию на месте таблицы.
fn remove_table(doc: &mut Document, path: &Path) -> Position {
    let first = first_paragraph(doc, path);
    if let Some(blocks) = blocks_mut(doc, path) {
        if path.block < blocks.len() {
            blocks.remove(path.block);
        }
        if !blocks
            .get(path.block)
            .is_some_and(|b| matches!(b, Block::Paragraph(_)))
        {
            blocks.insert(path.block, Paragraph::default().into());
        }
    }
    Position::new(first.min(paragraph_count(doc).saturating_sub(1)), 0)
}

/// Удаляет таблицу с абзацем `paragraph`.
pub fn delete_table(doc: &mut Document, paragraph: usize) -> Option<Position> {
    let at = cell_of(doc, paragraph)?;
    Some(remove_table(doc, &at.table))
}

/// Удаляет строки выделенного. Если строк не осталось — таблицу.
pub fn delete_rows(doc: &mut Document, start: usize, end: usize) -> Option<Position> {
    let (at, first, last) = row_range(doc, start, end)?;
    let table = table_mut(doc, &at.table)?;
    if first == 0 && last + 1 >= table.rows.len() {
        return Some(remove_table(doc, &at.table));
    }
    // Объединение по вертикали, начатое в удаляемых строках,
    // начинается в следующей.
    if let Some(next) = table.rows.get(last + 1) {
        let above = first.checked_sub(1).and_then(|r| table.rows.get(r));
        let restart: Vec<usize> = (0..next.cells.len())
            .filter(|&c| next.cells[c].props.v_merge == Some(VerticalMerge::Continue))
            .filter(|&c| {
                let s = grid_span(next, c).0;
                !above.is_some_and(|row| {
                    (0..row.cells.len())
                        .any(|k| grid_span(row, k).0 == s && row.cells[k].props.v_merge.is_some())
                })
            })
            .collect();
        for c in restart {
            table.rows[last + 1].cells[c].props.v_merge = Some(VerticalMerge::Restart);
        }
    }
    table.rows.drain(first..=last);
    let row = first.min(table.rows.len().saturating_sub(1));
    Some(Position::new(cell_start(doc, &at.table, row, 0), 0))
}

/// Удаляет столбцы выделенного. Если столбцов не осталось — таблицу.
pub fn delete_columns(doc: &mut Document, start: usize, end: usize) -> Option<Position> {
    let (at, g0, g1) = column_range(doc, start, end)?;
    let table = table_mut(doc, &at.table)?;
    let count = g1 - g0;
    for row in &mut table.rows {
        let mut start = 0;
        let mut keep = Vec::with_capacity(row.cells.len());
        for cell in row.cells.drain(..) {
            let span = cell.span();
            let (a, b) = (start, start + span);
            start = b;
            let overlap = b.min(g1).saturating_sub(a.max(g0));
            if overlap == 0 {
                keep.push(cell);
            } else if overlap < span {
                let mut cell = cell;
                cell.props.grid_span = Some((span - overlap) as u32);
                keep.push(cell);
            }
        }
        row.cells = keep;
    }
    table.rows.retain(|r| !r.cells.is_empty());
    if g0 < table.grid.len() {
        let to = (g0 + count).min(table.grid.len());
        table.grid.drain(g0..to);
    }
    if table.rows.is_empty() {
        return Some(remove_table(doc, &at.table));
    }
    fit_cells_to_grid(table);
    let row = at.row.min(table.rows.len() - 1);
    let cell = table.rows[row]
        .cells
        .iter()
        .scan(0, |s, c| {
            let start = *s;
            *s += c.span();
            Some(start)
        })
        .position(|s| s >= g0)
        .unwrap_or(table.rows[row].cells.len() - 1);
    Some(Position::new(cell_start(doc, &at.table, row, cell), 0))
}

/// Какие границы поставить или убрать у выделенных ячеек (меню
/// «Границы»).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderPreset {
    /// Все границы.
    All,
    /// Нет границ.
    None,
    /// Внешние границы.
    Outside,
    /// Внутренние границы.
    Inside,
    /// Верхняя граница.
    Top,
    /// Нижняя граница.
    Bottom,
    /// Левая граница.
    Left,
    /// Правая граница.
    Right,
}

/// Прямоугольник ячеек: строки `rows` и столбцы сетки `g0..g1`.
struct CellBlock {
    table: Path,
    rows: (usize, usize),
    columns: (usize, usize),
}

impl CellBlock {
    /// Ячейки прямоугольника: строка, номер ячейки, столбцы сетки.
    fn cells(&self, table: &Table) -> Vec<(usize, usize, usize, usize)> {
        let (g0, g1) = self.columns;
        let mut out = Vec::new();
        for r in self.rows.0..=self.rows.1 {
            let Some(row) = table.rows.get(r) else {
                continue;
            };
            for c in 0..row.cells.len() {
                let (a, b) = grid_span(row, c);
                if a < g1 && b > g0 {
                    out.push((r, c, a, b));
                }
            }
        }
        out
    }
}

/// Выделенные ячейки: от ячейки начала до ячейки конца выделения
/// (прямоугольником, расширенным до целых объединённых ячеек). Если
/// конец в другой таблице — только ячейка начала.
fn cell_block(doc: &Document, start: usize, end: usize) -> Option<CellBlock> {
    let first = cell_of(doc, start)?;
    let table = table_ref(doc, &first.table)?;
    let last = cell_of(doc, end)
        .filter(|c| c.table == first.table)
        .unwrap_or_else(|| first.clone());
    let (a0, a1) = grid_span(table.rows.get(first.row)?, first.cell);
    let (b0, b1) = grid_span(table.rows.get(last.row)?, last.cell);
    let mut block = CellBlock {
        table: first.table.clone(),
        rows: (first.row.min(last.row), first.row.max(last.row)),
        columns: (a0.min(b0), a1.max(b1)),
    };
    // Объединённые ячейки на краю входят целиком.
    loop {
        let (mut g0, mut g1) = block.columns;
        for (_, _, a, b) in block.cells(table) {
            g0 = g0.min(a);
            g1 = g1.max(b);
        }
        if (g0, g1) == block.columns {
            break;
        }
        block.columns = (g0, g1);
    }
    Some(block)
}

/// Выделено больше одной ячейки — их можно объединить.
pub fn can_merge(doc: &Document, start: usize, end: usize) -> bool {
    cell_block(doc, start, end)
        .and_then(|b| Some(b.cells(table_ref(doc, &b.table)?).len() > 1))
        .unwrap_or(false)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

fn side_mut(borders: &mut Borders, side: Side) -> &mut Option<Border> {
    match side {
        Side::Top => &mut borders.top,
        Side::Bottom => &mut borders.bottom,
        Side::Left => &mut borders.left,
        Side::Right => &mut borders.right,
    }
}

/// Ставит линию `border` на край `side` ячейки (`row`, `cell`) и на тот
/// же край соседней ячейки по другую сторону: вёрстка берёт линию
/// у любой из них.
fn set_edge(table: &mut Table, row: usize, cell: usize, side: Side, border: &Border) {
    let Some(r) = table.rows.get(row) else {
        return;
    };
    let (a, b) = grid_span(r, cell);
    let mut targets = vec![(row, cell, side)];
    match side {
        Side::Left if cell > 0 => targets.push((row, cell - 1, Side::Right)),
        Side::Right if cell + 1 < r.cells.len() => targets.push((row, cell + 1, Side::Left)),
        Side::Top | Side::Bottom => {
            let other = if side == Side::Top {
                row.checked_sub(1)
            } else {
                Some(row + 1)
            };
            if let Some(o) = other
                && let Some(neighbour) = table.rows.get(o)
            {
                let opposite = if side == Side::Top {
                    Side::Bottom
                } else {
                    Side::Top
                };
                for c in 0..neighbour.cells.len() {
                    let (x, y) = grid_span(neighbour, c);
                    if x < b && y > a {
                        targets.push((o, c, opposite));
                    }
                }
            }
        }
        _ => {}
    }
    for (r, c, side) in targets {
        if let Some(cell) = table.rows.get_mut(r).and_then(|r| r.cells.get_mut(c)) {
            let borders = cell.props.borders.get_or_insert_with(Borders::default);
            *side_mut(borders, side) = Some(border.clone());
        }
    }
}

/// Меню «Границы»: ставит или убирает линии у выделенных ячеек.
/// Возвращает, были ли ячейки.
pub fn set_cell_borders(
    doc: &mut Document,
    start: usize,
    end: usize,
    preset: BorderPreset,
) -> bool {
    let Some(block) = cell_block(doc, start, end) else {
        return false;
    };
    let Some(table) = table_mut(doc, &block.table) else {
        return false;
    };
    let line = Border::single(4);
    let none = Border {
        style: "nil".into(),
        size: None,
        space: None,
        color: None,
    };
    for (r, c, a, b) in block.cells(table) {
        let edges = [
            (Side::Top, r == block.rows.0),
            (Side::Bottom, r == block.rows.1),
            (Side::Left, a == block.columns.0),
            (Side::Right, b == block.columns.1),
        ];
        for (side, outer) in edges {
            let border = match preset {
                BorderPreset::All => Some(&line),
                BorderPreset::None => Some(&none),
                BorderPreset::Outside => outer.then_some(&line),
                BorderPreset::Inside => (!outer).then_some(&line),
                BorderPreset::Top => (outer && side == Side::Top).then_some(&line),
                BorderPreset::Bottom => (outer && side == Side::Bottom).then_some(&line),
                BorderPreset::Left => (outer && side == Side::Left).then_some(&line),
                BorderPreset::Right => (outer && side == Side::Right).then_some(&line),
            };
            if let Some(border) = border {
                set_edge(table, r, c, side, border);
            }
        }
    }
    true
}

/// «Заливка»: цвет фона выделенных ячеек; `None` — без заливки.
pub fn set_cell_shading(
    doc: &mut Document,
    start: usize,
    end: usize,
    color: Option<Color>,
) -> bool {
    let Some(block) = cell_block(doc, start, end) else {
        return false;
    };
    let Some(table) = table_mut(doc, &block.table) else {
        return false;
    };
    for (r, c, _, _) in block.cells(table) {
        if let Some(cell) = table.rows.get_mut(r).and_then(|r| r.cells.get_mut(c)) {
            cell.props.shading = color.map(Shading::fill);
        }
    }
    true
}

/// «Объединить ячейки»: выделенные ячейки становятся одной. Текст всех
/// ячеек (кроме пустых абзацев) собирается в левую верхнюю. Возвращает
/// позицию в ней.
pub fn merge_cells(doc: &mut Document, start: usize, end: usize) -> Option<Position> {
    let block = cell_block(doc, start, end)?;
    let table = table_mut(doc, &block.table)?;
    let cells = block.cells(table);
    if cells.len() < 2 {
        return None;
    }
    let (g0, g1) = block.columns;
    let width: i32 = table.grid.iter().skip(g0).take(g1 - g0).map(|t| t.0).sum();
    let mut content: Vec<Block> = Vec::new();
    for &(r, c, _, _) in &cells {
        if let Some(cell) = table.rows.get(r).and_then(|row| row.cells.get(c)) {
            content.extend(
                cell.blocks
                    .iter()
                    .filter(|b| match b {
                        Block::Paragraph(p) => !p.plain_text().is_empty(),
                        _ => true,
                    })
                    .cloned(),
            );
        }
    }
    let first_row = block.rows.0;
    let tall = block.rows.1 > block.rows.0;
    for r in (block.rows.0..=block.rows.1).rev() {
        let Some(row) = table.rows.get_mut(r) else {
            continue;
        };
        let indices: Vec<usize> = cells
            .iter()
            .filter(|&&(cr, ..)| cr == r)
            .map(|&(_, c, _, _)| c)
            .collect();
        let (Some(&first), Some(&last)) = (indices.first(), indices.last()) else {
            continue;
        };
        let mut merged = row.cells[first].clone();
        merged.props.grid_span = (g1 - g0 > 1).then_some((g1 - g0) as u32);
        if width > 0 {
            merged.props.width = Some(TableWidth::Twips(Twips(width)));
        }
        merged.props.v_merge = match (tall, r == first_row) {
            (false, _) => None,
            (true, true) => Some(VerticalMerge::Restart),
            (true, false) => Some(VerticalMerge::Continue),
        };
        let paragraph_props = merged.blocks.iter().find_map(|b| match b {
            Block::Paragraph(p) => Some(p.props.clone()),
            _ => None,
        });
        merged.blocks = if r == first_row && !content.is_empty() {
            std::mem::take(&mut content)
        } else {
            vec![
                Paragraph {
                    props: paragraph_props.unwrap_or_default(),
                    content: Vec::new(),
                }
                .into(),
            ]
        };
        row.cells.splice(first..=last, [merged]);
    }
    let first = cells.first().map_or(0, |&(_, c, _, _)| c);
    Some(Position::new(
        cell_start(doc, &block.table, first_row, first),
        0,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::insert_text;

    fn doc_with(texts: &[&str]) -> Document {
        let mut doc = Document::new();
        doc.sections[0].blocks = texts.iter().map(|t| Paragraph::text(*t).into()).collect();
        doc
    }

    fn table(doc: &Document) -> &Table {
        doc.sections[0]
            .blocks
            .iter()
            .find_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .expect("таблица")
    }

    #[test]
    fn insert_splits_the_paragraph() {
        let mut doc = doc_with(&["abcd"]);
        let at = insert_table(&mut doc, Position::new(0, 2), 2, 3);
        // «ab», таблица (6 пустых абзацев), «cd».
        assert_eq!(at, Position::new(1, 0));
        assert_eq!(doc.plain_text(), "ab\n\t\t\n\t\t\ncd");
        let t = table(&doc);
        assert_eq!(t.grid.len(), 3);
        assert_eq!(t.props.style.as_deref(), Some(GRID_STYLE));
        assert!(doc.styles.get(GRID_STYLE).is_some());
        assert!(doc.styles.default_for(StyleKind::Table).is_some());
        // В начале абзаца — перед ним.
        let mut doc = doc_with(&["x"]);
        insert_table(&mut doc, Position::new(0, 0), 1, 1);
        assert!(matches!(doc.sections[0].blocks[0], Block::Table(_)));
        assert_eq!(paragraph_count(&doc), 2);
        // Стиль не добавляется второй раз.
        let styles = doc.styles.styles.len();
        insert_table(&mut doc, Position::new(1, 0), 1, 1);
        assert_eq!(doc.styles.styles.len(), styles);
    }

    #[test]
    fn tab_walks_the_cells() {
        let mut doc = doc_with(&["x"]);
        insert_table(&mut doc, Position::new(0, 0), 2, 2);
        insert_text(&mut doc, Position::new(1, 0), "bb");
        let next = next_cell(&doc, 0, false).unwrap();
        assert_eq!(
            (next.anchor, next.focus),
            (Position::new(1, 0), Position::new(1, 2))
        );
        assert_eq!(next_cell(&doc, 1, false).unwrap().focus.paragraph, 2);
        assert_eq!(next_cell(&doc, 2, true).unwrap().focus.paragraph, 1);
        assert_eq!(next_cell(&doc, 3, false), None);
        assert_eq!(next_cell(&doc, 0, true), None);
        assert_eq!(next_cell(&doc, 4, false), None); // абзац после таблицы
        assert!(in_table(&doc, 3) && !in_table(&doc, 4));
    }

    #[test]
    fn rows_and_columns() {
        let mut doc = doc_with(&["x"]);
        insert_table(&mut doc, Position::new(0, 0), 2, 2);
        insert_text(&mut doc, Position::new(2, 0), "c");
        // Строка под второй: курсор в её первой ячейке.
        let at = insert_rows(&mut doc, 2, 2, true).unwrap();
        assert_eq!(at, Position::new(4, 0));
        assert_eq!(table(&doc).rows.len(), 3);
        // Две строки над выделенными двумя.
        insert_rows(&mut doc, 0, 2, false).unwrap();
        assert_eq!(table(&doc).rows.len(), 5);
        assert_eq!(doc.plain_text(), "\t\n\t\n\t\nc\t\n\t\nx");

        let width: i32 = table(&doc).grid.iter().map(|t| t.0).sum();
        let at = insert_columns(&mut doc, 6, 6, true).unwrap();
        let t = table(&doc);
        assert_eq!(t.grid.len(), 3);
        assert!(t.rows.iter().all(|r| r.cells.len() == 3));
        // Ширина таблицы та же (с точностью до округления).
        let after: i32 = t.grid.iter().map(|t| t.0).sum();
        assert!((width - after).abs() <= 3);
        assert_eq!(cell_of(&doc, at.paragraph).map(|c| c.cell), Some(1));

        delete_columns(&mut doc, 1, 1).unwrap();
        assert!(table(&doc).rows.iter().all(|r| r.cells.len() == 2));
        assert_eq!(table(&doc).grid.len(), 2);
        delete_rows(&mut doc, 0, 3).unwrap();
        assert_eq!(table(&doc).rows.len(), 3);
        assert_eq!(doc.plain_text(), "\t\nc\t\n\t\nx");
        // Все строки — таблица целиком, абзац после неё остаётся.
        let at = delete_rows(&mut doc, 0, 5).unwrap();
        assert_eq!(doc.plain_text(), "x");
        assert_eq!(at, Position::new(0, 0));
    }

    #[test]
    fn delete_table_keeps_a_paragraph() {
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![
            Paragraph::text("a").into(),
            Table {
                rows: vec![TableRow {
                    cells: vec![TableCell::text("t")],
                    ..TableRow::default()
                }],
                ..Table::default()
            }
            .into(),
        ];
        let at = delete_table(&mut doc, 1).unwrap();
        assert_eq!(at, Position::new(1, 0));
        assert_eq!(paragraph_count(&doc), 2);
        assert_eq!(delete_table(&mut doc, 0), None);
    }

    /// Таблица 3×3 с текстом «r,c» в ячейках сразу после абзаца «x».
    fn grid3() -> Document {
        let mut doc = doc_with(&["x"]);
        insert_table(&mut doc, Position::new(0, 0), 3, 3);
        for i in 0..9 {
            insert_text(
                &mut doc,
                Position::new(i, 0),
                &format!("{},{}", i / 3, i % 3),
            );
        }
        doc
    }

    fn borders(doc: &Document, row: usize, cell: usize) -> Borders {
        table(doc).rows[row].cells[cell]
            .props
            .borders
            .clone()
            .unwrap_or_default()
    }

    #[test]
    fn borders_and_shading_of_cells() {
        let mut doc = grid3();
        // Средняя ячейка: только внешние линии — и у соседей тоже.
        assert!(set_cell_borders(&mut doc, 4, 4, BorderPreset::None));
        let nil = |b: &Option<Border>| b.as_ref().is_some_and(Border::is_none);
        let middle = borders(&doc, 1, 1);
        assert!(nil(&middle.top) && nil(&middle.left) && nil(&middle.bottom) && nil(&middle.right));
        assert!(nil(&borders(&doc, 0, 1).bottom));
        assert!(nil(&borders(&doc, 1, 0).right));
        assert!(nil(&borders(&doc, 2, 1).top));
        assert!(borders(&doc, 0, 0).top.is_none());
        // Внешние линии прямоугольника 2×2 сверху слева.
        set_cell_borders(&mut doc, 0, 4, BorderPreset::Outside);
        let single = |b: &Option<Border>| b.as_ref().is_some_and(|b| b.style == "single");
        assert!(single(&borders(&doc, 0, 0).top) && single(&borders(&doc, 0, 1).right));
        assert!(single(&borders(&doc, 1, 1).bottom) && single(&borders(&doc, 1, 1).right));
        assert!(nil(&borders(&doc, 1, 1).top));
        // Заливка и её снятие.
        set_cell_shading(&mut doc, 0, 1, Some(Color::Rgb(1, 2, 3)));
        assert!(table(&doc).rows[0].cells[1].props.shading.is_some());
        assert!(table(&doc).rows[0].cells[2].props.shading.is_none());
        set_cell_shading(&mut doc, 1, 1, None);
        assert!(table(&doc).rows[0].cells[1].props.shading.is_none());
        assert!(!set_cell_shading(&mut doc, 9, 9, None));
    }

    #[test]
    fn merge_cells_across_and_down() {
        let mut doc = grid3();
        assert!(!can_merge(&doc, 0, 0));
        assert!(can_merge(&doc, 0, 4));
        // Первая строка: 0,0 и 0,1 — в одну ячейку на два столбца.
        let at = merge_cells(&mut doc, 0, 1).unwrap();
        assert_eq!(at, Position::new(0, 0));
        let t = table(&doc);
        assert_eq!(t.rows[0].cells.len(), 2);
        assert_eq!(t.rows[0].cells[0].span(), 2);
        assert_eq!(t.rows[0].cells[0].plain_text(), "0,0\n0,1");
        // Затем прямоугольник: объединённая ячейка и строка под ней.
        merge_cells(&mut doc, 0, 4).unwrap();
        let t = table(&doc);
        assert_eq!(
            t.rows[0].cells[0].props.v_merge,
            Some(VerticalMerge::Restart)
        );
        assert_eq!(
            t.rows[1].cells[0].props.v_merge,
            Some(VerticalMerge::Continue)
        );
        assert_eq!(t.rows[1].cells.len(), 2);
        assert_eq!(t.rows[0].cells[0].plain_text(), "0,0\n0,1\n1,0\n1,1");
        assert_eq!(t.rows[1].cells[0].plain_text(), "");
        assert_eq!(merge_cells(&mut doc, 0, 0), None);
    }

    #[test]
    fn nested_table_fits_its_cell() {
        let mut doc = doc_with(&["x"]);
        insert_table(&mut doc, Position::new(0, 0), 1, 2);
        let outer = table(&doc).grid[0].0;
        insert_table(&mut doc, Position::new(0, 0), 1, 2);
        let Some(Block::Table(t)) = doc.sections[0].blocks.first() else {
            panic!("таблица");
        };
        let Some(Block::Table(inner)) = t.rows[0].cells[0].blocks.first() else {
            panic!("вложенная таблица");
        };
        assert_eq!(inner.grid[0].0, (outer - 2 * SIDE_MARGIN) / 2);
    }
}
