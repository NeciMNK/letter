//! Таблицы. В .doc таблица — это абзацы с пометкой «в таблице»: ячейка
//! кончается особым знаком, строка — ещё одним знаком конца строки, в
//! свойствах которого лежит описание строки (`sprmTDefTable`: границы
//! ячеек, объединения, границы и заливка).

use letter_model::{
    Alignment, Block, Border, Borders, CellMargins, CellProps, CellVerticalAlign, HeightRule,
    RowHeight, RowProps, Shading, Table, TableCell, TableLayout, TableProps, TableRow, TableWidth,
    Twips, VerticalMerge,
};

use crate::border::{brc, brc80, shd, shd80};
use crate::fib::u16_at;
use crate::sprm::{Names, Sprm, code};

/// Место абзаца в таблицах (из его свойств).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Place {
    /// Глубина вложенности: 0 — не в таблице.
    pub depth: usize,
    /// Знак конца строки таблицы (у вложенных — абзац с такой пометкой).
    pub row_end: bool,
    /// Абзац вложенной таблицы, которым кончается ячейка.
    pub inner_cell_end: bool,
    /// Абзац вложенной таблицы — конец строки.
    pub inner_row_end: bool,
}

/// Место абзаца по его свойствам.
pub(crate) fn place(list: &[Sprm]) -> Place {
    let mut place = Place::default();
    let mut in_table = false;
    let mut depth = None;
    for sprm in list {
        match sprm.code {
            0x2416 => in_table = sprm.byte() != 0,
            0x2417 => place.row_end = sprm.byte() != 0,
            0x244B => place.inner_cell_end = sprm.byte() != 0,
            0x244C => place.inner_row_end = sprm.byte() != 0,
            0x6649 => {
                depth = Some(
                    u16_at(sprm.operand, 0)
                        .map(usize::from)
                        .unwrap_or(0)
                        .min(64),
                )
            }
            _ => {}
        }
    }
    place.depth = match depth {
        Some(depth) => depth,
        None if in_table => 1,
        None => 0,
    };
    place
}

/// Описание ячейки из строки.
#[derive(Debug, Clone, Default)]
struct CellDef {
    /// Флаги `TC80`: объединения, выравнивание по вертикали.
    flags: u16,
    borders: Borders,
    shading: Option<Shading>,
    v_align: Option<CellVerticalAlign>,
}

/// Описание строки таблицы (свойства знака конца строки).
#[derive(Debug, Clone, Default)]
pub(crate) struct RowDef {
    /// Границы ячеек по горизонтали: (n + 1) мест.
    centers: Vec<i32>,
    cells: Vec<CellDef>,
    row: RowProps,
    table: TableProps,
    /// Половина промежутка между ячейками (поле ячейки слева и справа).
    gap_half: i32,
}

/// Ширина с единицей: 1 — авто, 2 — пятидесятые доли процента, 3 — твипы.
fn width(fts: u8, value: u16) -> TableWidth {
    match fts {
        1 => TableWidth::Auto,
        2 => TableWidth::Percent(i32::from(value)),
        3 => TableWidth::Twips(Twips(i32::from(value))),
        _ => TableWidth::Nil,
    }
}

/// Стороны по битам: 1 — верх, 2 — слева, 4 — низ, 8 — справа.
fn sides(borders: &mut Borders, bits: u8, line: &Option<Border>) {
    for (bit, side) in [
        (1, &mut borders.top),
        (2, &mut borders.left),
        (4, &mut borders.bottom),
        (8, &mut borders.right),
    ] {
        if bits & bit != 0 {
            *side = line.clone();
        }
    }
}

/// Разбирает описание строки.
pub(crate) fn row_def(list: &[Sprm], names: &Names) -> RowDef {
    let mut def = RowDef::default();
    for sprm in list {
        // Значение после байта длины у свойств переменной длины.
        let data = sprm.operand.get(1..).unwrap_or(&[]);
        match sprm.code {
            code::T_DEF_TABLE => {
                let data = sprm.operand.get(2..).unwrap_or(&[]);
                let count = usize::from(data.first().copied().unwrap_or(0));
                def.centers = (0..=count)
                    .map_while(|i| u16_at(data, 1 + 2 * i))
                    .map(|v| i32::from(v as i16))
                    .collect();
                let tcs = 1 + 2 * (count + 1);
                def.cells = (0..count)
                    .map(|i| {
                        let tc = data.get(tcs + 20 * i..tcs + 20 * (i + 1)).unwrap_or(&[]);
                        let flags = u16_at(tc, 0).unwrap_or(0);
                        let line = |at: usize| tc.get(at..at + 4).and_then(brc80);
                        CellDef {
                            flags,
                            borders: Borders {
                                top: line(4),
                                left: line(8),
                                bottom: line(12),
                                right: line(16),
                                ..Borders::default()
                            },
                            shading: None,
                            v_align: v_align((flags >> 7) & 3),
                        }
                    })
                    .collect();
            }
            0xD605 | 0xD613 => {
                let size = if sprm.code == 0xD605 { 4 } else { 8 };
                let line = |i: usize| {
                    let bytes = data.get(i * size..(i + 1) * size)?;
                    if size == 4 { brc80(bytes) } else { brc(bytes) }
                };
                let borders = Borders {
                    top: line(0),
                    left: line(1),
                    bottom: line(2),
                    right: line(3),
                    inside_h: line(4),
                    inside_v: line(5),
                };
                def.table.borders = Some(borders);
            }
            0xD609 => {
                for (cell, bytes) in def.cells.iter_mut().zip(data.chunks(2)) {
                    cell.shading = shd80(bytes);
                }
            }
            0xD612 | 0xD616 | 0xD60C => {
                let first = match sprm.code {
                    0xD612 => 0,
                    0xD616 => 22,
                    _ => 44,
                };
                for (cell, bytes) in def.cells.iter_mut().skip(first).zip(data.chunks(10)) {
                    cell.shading = shd(bytes);
                }
            }
            // Выравнивание по вертикали у ячеек [первая, последняя).
            0xD62C => {
                if let [first, last, align, ..] = *data {
                    for cell in cells(&mut def.cells, first, last) {
                        cell.v_align = v_align(u16::from(align));
                    }
                }
            }
            // Границы ячеек [первая, последняя) по сторонам.
            0xD620 | 0xD62F => {
                if let [first, last, bits, ref rest @ ..] = *data {
                    let line = if sprm.code == 0xD620 {
                        brc80(rest)
                    } else {
                        brc(rest)
                    };
                    for cell in cells(&mut def.cells, first, last) {
                        sides(&mut cell.borders, bits, &line);
                    }
                }
            }
            // Поля ячеек таблицы по умолчанию.
            0xD634 => {
                if let [_, _, bits, fts, ..] = *data {
                    let value = width(fts, u16_at(data, 4).unwrap_or(0));
                    let margins = def.table.cell_margins.get_or_insert_with(Default::default);
                    for (bit, side) in [
                        (1, &mut margins.top),
                        (2, &mut margins.left),
                        (4, &mut margins.bottom),
                        (8, &mut margins.right),
                    ] {
                        if bits & bit != 0 {
                            *side = Some(value);
                        }
                    }
                }
            }
            0x3403 | 0x3466 => def.row.cant_split = Some(sprm.byte() != 0),
            0x3404 => def.row.header = Some(sprm.byte() != 0),
            0x9407 => {
                let value = i32::from(sprm.signed());
                def.row.height = (value != 0).then(|| RowHeight {
                    value: Twips(value.abs()),
                    rule: if value < 0 {
                        HeightRule::Exact
                    } else {
                        HeightRule::AtLeast
                    },
                });
            }
            0x5400 | 0x548A => {
                def.table.alignment = Some(match sprm.word() {
                    1 => Alignment::Center,
                    2 => Alignment::Right,
                    _ => Alignment::Left,
                });
            }
            0x9602 => def.gap_half = i32::from(sprm.signed()),
            0xF614 => {
                def.table.width = Some(width(sprm.byte(), u16_at(sprm.operand, 1).unwrap_or(0)));
            }
            0x563A => def.table.style = names.style(sprm.word()),
            0x3615 => {
                def.table.layout = Some(if sprm.byte() != 0 {
                    TableLayout::Auto
                } else {
                    TableLayout::Fixed
                });
            }
            _ => {}
        }
    }
    def
}

/// Ячейки с номерами [first, last).
fn cells(cells: &mut [CellDef], first: u8, last: u8) -> &mut [CellDef] {
    let end = usize::from(last).min(cells.len());
    let start = usize::from(first).min(end);
    &mut cells[start..end]
}

fn v_align(value: u16) -> Option<CellVerticalAlign> {
    match value {
        1 => Some(CellVerticalAlign::Center),
        2 => Some(CellVerticalAlign::Bottom),
        _ => None,
    }
}

/// Флаги ячейки `TC80`.
const MERGED: u16 = 0x0002;
const VERT_MERGE: u16 = 0x0020;
const VERT_RESTART: u16 = 0x0040;

/// Собирает таблицу из строк: ячейки (с содержимым) и описание строки.
pub(crate) fn table(rows: Vec<(Vec<Vec<Block>>, RowDef)>) -> Table {
    // Сетка — все границы ячеек всех строк.
    let mut edges: Vec<i32> = rows
        .iter()
        .flat_map(|(_, def)| def.centers.iter().copied())
        .collect();
    edges.sort_unstable();
    edges.dedup();
    let mut table = Table {
        grid: edges.windows(2).map(|w| Twips(w[1] - w[0])).collect(),
        ..Table::default()
    };
    if let Some((_, def)) = rows.first() {
        let mut props = def.table.clone();
        // Отступ таблицы в модели — до текста первой ячейки (её граница
        // левее на поле ячейки), а в .doc задана сама граница.
        let margin = match props.cell_margins.and_then(|m| m.left) {
            Some(TableWidth::Twips(margin)) => margin.0,
            _ => def.gap_half,
        };
        let left = def.centers.first().copied().unwrap_or(0) + margin;
        props.indent = (left != 0).then_some(TableWidth::Twips(Twips(left)));
        if props.cell_margins.is_none() && def.gap_half != 0 {
            let gap = Some(TableWidth::Twips(Twips(def.gap_half)));
            props.cell_margins = Some(CellMargins {
                left: gap,
                right: gap,
                ..CellMargins::default()
            });
        }
        table.props = Box::new(props);
    }
    for (contents, def) in rows {
        let mut row = TableRow {
            props: def.row.clone(),
            ..TableRow::default()
        };
        for (i, blocks) in contents.into_iter().enumerate() {
            let cell_def = def.cells.get(i).cloned().unwrap_or_default();
            let bounds = (def.centers.get(i), def.centers.get(i + 1));
            let (span, width) = match bounds {
                (Some(&left), Some(&right)) => (
                    edges
                        .iter()
                        .filter(|&&e| e > left && e <= right)
                        .count()
                        .max(1),
                    Some(TableWidth::Twips(Twips(right - left))),
                ),
                _ => (1, None),
            };
            // Старое объединение по горизонтали: ячейка присоединяется к
            // предыдущей.
            if cell_def.flags & MERGED != 0
                && let Some(previous) = row.cells.last_mut()
            {
                let total = previous.span() + span;
                previous.props.grid_span = Some(total as u32);
                if let (Some(TableWidth::Twips(a)), Some(TableWidth::Twips(b))) =
                    (previous.props.width, width)
                {
                    previous.props.width = Some(TableWidth::Twips(Twips(a.0 + b.0)));
                }
                continue;
            }
            let props = CellProps {
                width,
                grid_span: (span > 1).then_some(span as u32),
                v_merge: if cell_def.flags & VERT_RESTART != 0 {
                    Some(VerticalMerge::Restart)
                } else if cell_def.flags & VERT_MERGE != 0 {
                    Some(VerticalMerge::Continue)
                } else {
                    None
                },
                borders: (!cell_def.borders.is_empty()).then_some(cell_def.borders),
                shading: cell_def.shading,
                v_align: cell_def.v_align,
                ..CellProps::default()
            };
            let mut blocks = blocks;
            if blocks.is_empty() {
                blocks.push(letter_model::Paragraph::default().into());
            }
            row.cells.push(TableCell {
                props,
                raw_props: Vec::new(),
                blocks,
            });
        }
        table.rows.push(row);
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sprm::sprms;

    /// `sprmTDefTable` для ячеек с границами `centers` и флагами `flags`.
    fn def_table(centers: &[i16], flags: &[u16]) -> Vec<u8> {
        let mut data = vec![flags.len() as u8];
        for c in centers {
            data.extend(c.to_le_bytes());
        }
        for f in flags {
            data.extend(f.to_le_bytes());
            data.extend([0u8; 18]);
        }
        let mut out = vec![0x08, 0xD6];
        out.extend((data.len() as u16 + 1).to_le_bytes());
        out.extend(data);
        out
    }

    #[test]
    fn place_of_paragraphs() {
        assert_eq!(place(&sprms(&[0x16, 0x24, 1])).depth, 1);
        let end = place(&sprms(&[0x16, 0x24, 1, 0x17, 0x24, 1]));
        assert!(end.row_end);
        assert_eq!(place(&sprms(&[0x49, 0x66, 2, 0, 0, 0])).depth, 2);
        assert_eq!(place(&[]).depth, 0);
    }

    #[test]
    fn rows_make_a_grid_with_merges() {
        let names = Names {
            fonts: &[],
            styles: &[],
        };
        let full = def_table(&[0, 1000, 2000, 3000], &[0, 0, 0]);
        // Вторая строка: первые две ячейки объединены.
        let merged = def_table(&[0, 1000, 2000, 3000], &[0x0001, MERGED, 0]);
        // Третья: две ячейки, граница не совпадает с сеткой выше.
        let other = def_table(&[0, 1500, 3000], &[0, 0]);
        let cell = || vec![Block::from(letter_model::Paragraph::default())];
        let rows = vec![
            (vec![cell(), cell(), cell()], row_def(&sprms(&full), &names)),
            (
                vec![cell(), cell(), cell()],
                row_def(&sprms(&merged), &names),
            ),
            (vec![cell(), cell()], row_def(&sprms(&other), &names)),
        ];
        let table = table(rows);
        assert_eq!(
            table.grid,
            [Twips(1000), Twips(500), Twips(500), Twips(1000)]
        );
        assert_eq!(table.rows[0].cells.len(), 3);
        assert_eq!(table.rows[0].cells[1].span(), 2);
        assert_eq!(table.rows[1].cells.len(), 2);
        assert_eq!(table.rows[1].cells[0].span(), 3);
        assert_eq!(
            table.rows[1].cells[0].props.width,
            Some(TableWidth::Twips(Twips(2000)))
        );
        assert_eq!(table.rows[2].cells[0].span(), 2);
    }
}
