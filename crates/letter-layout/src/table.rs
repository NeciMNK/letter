//! Вёрстка таблиц: ширины столбцов, ячейки, высота строк, границы
//! и заливка.
//!
//! - Ширины столбцов берутся из сетки таблицы (`w:tblGrid`) — Word
//!   записывает туда уже подобранные ширины. Если сетки нет, ширина
//!   таблицы (или ширина текста) делится поровну.
//! - В режиме совместимости Word 2010 и старше текст первого столбца
//!   стоит на левом поле, а граница таблицы — левее на поле ячейки;
//!   в Word 2013 и новее граница стоит на поле.
//! - Ячейка — те же абзацы (и вложенные таблицы) друг под другом,
//!   с полями ячейки со всех сторон. Высота строки — самая высокая
//!   ячейка, если высота не задана «точно» или «не меньше».
//! - Объединённая по вертикали ячейка верстается в первой строке
//!   объединения; если не помещается — последняя строка выше.
//! - Границы: у ячейки свои, иначе внешние границы таблицы по краям
//!   и внутренние между ячейками. Линия рисуется по центру границы
//!   ячейки; двойные, пунктирные и прочие виды — пока сплошной линией.

use std::sync::Arc;

use letter_model::{
    Alignment, Block, Border, CellVerticalAlign, Color, Document, HeightRule, Table, TableCell,
    TableProps, TableWidth, VerticalMerge,
};

use crate::pages::{CellArea, Prepared, Shape, ShapeKind};
use crate::spacing::{ParagraphBlock, effective_spacing, gap_between};

/// Поле ячейки слева и справа, если его не задали ни таблица, ни стиль:
/// 0,08″, как в Word.
const DEFAULT_SIDE_MARGIN: f32 = 5.4;

/// Столбцы таблицы.
pub(crate) struct TableGeometry {
    /// Свойства таблицы со стилем.
    pub props: TableProps,
    /// Края столбцов сетки, пт от левого края области, где стоит таблица.
    pub edges: Vec<f32>,
    /// Поля ячеек по умолчанию: сверху, слева, снизу, справа, пт.
    margins: [f32; 4],
}

impl TableGeometry {
    /// Столбцы таблицы `table` в области шириной `width` пт.
    pub fn of(doc: &Document, table: &Table, width: f32) -> TableGeometry {
        let props = doc.resolve_table(&table.props);
        let side = |w: Option<TableWidth>, default: f32| match w {
            Some(TableWidth::Twips(t)) => t.to_points() as f32,
            Some(_) => 0.0,
            None => default,
        };
        let m = props.cell_margins.unwrap_or_default();
        let margins = [
            side(m.top, 0.0),
            side(m.left, DEFAULT_SIDE_MARGIN),
            side(m.bottom, 0.0),
            side(m.right, DEFAULT_SIDE_MARGIN),
        ];

        let columns = table
            .rows
            .iter()
            .map(|r| r.cells.iter().map(TableCell::span).sum::<usize>())
            .max()
            .unwrap_or(0)
            .max(table.grid.len());
        let mut grid: Vec<f32> = table
            .grid
            .iter()
            .map(|t| (t.to_points() as f32).max(0.0))
            .collect();
        let total_width = match props.width {
            Some(TableWidth::Twips(t)) if t.0 > 0 => t.to_points() as f32,
            Some(TableWidth::Percent(p)) if p > 0 => width * p as f32 / 5000.0,
            _ => width,
        };
        if grid.iter().sum::<f32>() <= 0.0 {
            grid.clear();
        }
        if grid.len() < columns {
            // Недостающие столбцы: остаток ширины поровну, а если его
            // нет — средняя ширина известных.
            let known: f32 = grid.iter().sum();
            let missing = (columns - grid.len()) as f32;
            let each = if total_width - known > missing {
                (total_width - known) / missing
            } else if grid.is_empty() {
                total_width / missing
            } else {
                known / grid.len() as f32
            };
            grid.resize(columns, each);
        }
        let total: f32 = grid.iter().sum();

        let first_margin = table
            .rows
            .first()
            .and_then(|r| r.cells.first())
            .map_or(margins[1], |c| cell_margins(c, margins)[1]);
        let left = match props.alignment {
            Some(Alignment::Center) => (width - total) / 2.0,
            Some(Alignment::Right) => width - total,
            _ => {
                let indent = match props.indent {
                    Some(TableWidth::Twips(t)) => t.to_points() as f32,
                    _ => 0.0,
                };
                if doc.settings.compatibility_mode < 15 {
                    indent - first_margin
                } else {
                    indent
                }
            }
        };
        let mut edges = Vec::with_capacity(grid.len() + 1);
        let mut x = left;
        edges.push(x);
        for w in grid {
            x += w;
            edges.push(x);
        }
        TableGeometry {
            props,
            edges,
            margins,
        }
    }

    /// Стиль таблицы для оформления абзацев в ячейках (`""` — стиль
    /// таблиц по умолчанию).
    pub fn style(&self) -> String {
        self.props.style.clone().unwrap_or_default()
    }

    /// Левый и правый край ячейки, начинающейся в столбце `start`.
    pub fn cell_edges(&self, start: usize, cell: &TableCell) -> (f32, f32) {
        let last = self.edges.len().saturating_sub(1);
        let at = |i: usize| self.edges.get(i.min(last)).copied().unwrap_or(0.0);
        (at(start), at(start + cell.span()))
    }

    /// Поля ячейки: сверху, слева, снизу, справа.
    pub fn margins(&self, cell: &TableCell) -> [f32; 4] {
        cell_margins(cell, self.margins)
    }

    /// Начало текста ячейки и его ширина.
    pub fn text_area(&self, start: usize, cell: &TableCell) -> (f32, f32) {
        let (left, right) = self.cell_edges(start, cell);
        let m = self.margins(cell);
        let x = left + m[1];
        (x, (right - m[3] - x).max(1.0))
    }
}

fn cell_margins(cell: &TableCell, defaults: [f32; 4]) -> [f32; 4] {
    let Some(m) = cell.props.margins else {
        return defaults;
    };
    let side = |w: Option<TableWidth>, default: f32| match w {
        Some(TableWidth::Twips(t)) => t.to_points() as f32,
        Some(_) => 0.0,
        None => default,
    };
    [
        side(m.top, defaults[0]),
        side(m.left, defaults[1]),
        side(m.bottom, defaults[2]),
        side(m.right, defaults[3]),
    ]
}

/// Строка абзаца в ячейке; координаты — от левого края области текста
/// и от верха строки таблицы.
#[derive(Clone)]
pub(crate) struct BoxLine {
    pub prepared: Arc<Prepared>,
    /// Номер строки в абзаце.
    pub k: usize,
    /// Номер абзаца в документе.
    pub paragraph: usize,
    /// Левый край области, от которого отсчитаны координаты строки.
    pub x: f32,
    pub y: f32,
    pub height: f32,
    /// Ячейка вложенной таблицы, если строка в ней.
    pub cell: Option<CellArea>,
}

/// Кусок строки таблицы в одном столбце (ячейка).
#[derive(Clone)]
pub(crate) struct Segment {
    pub left: f32,
    pub right: f32,
    pub lines: Vec<BoxLine>,
    /// Заливка и линии вложенных таблиц.
    pub shapes: Vec<Shape>,
    pub top: Option<Border>,
    pub bottom: Option<Border>,
    pub left_border: Option<Border>,
    pub right_border: Option<Border>,
    pub fill: Option<(u8, u8, u8)>,
    pub margin_top: f32,
    pub margin_bottom: f32,
}

/// Строка таблицы, готовая к раскладке по страницам.
#[derive(Clone)]
pub(crate) struct RowBox {
    pub height: f32,
    /// Повторять наверху каждой страницы.
    pub header: bool,
    pub cant_split: bool,
    pub segments: Vec<Segment>,
    /// Линия там, где строка разорвана между страницами.
    pub split_border: Option<Border>,
}

/// Что нужно для вёрстки ячеек: подготовленные абзацы документа по
/// порядку и номер следующего.
pub(crate) struct Ctx<'a> {
    pub doc: &'a Document,
    pub prepared: &'a [Arc<Prepared>],
    pub next: usize,
    pub add_spacing: bool,
}

impl Ctx<'_> {
    fn take(&mut self) -> Option<(usize, Arc<Prepared>)> {
        let index = self.next;
        let prepared = self.prepared.get(index)?;
        self.next += 1;
        Some((index, Arc::clone(prepared)))
    }
}

/// Содержимое ячейки: строки, линии вложенных таблиц и высота.
struct Content {
    lines: Vec<BoxLine>,
    shapes: Vec<Shape>,
    height: f32,
}

/// Абзацы и таблицы ячейки друг под другом. `x` — начало текста.
fn stack(ctx: &mut Ctx<'_>, blocks: &[Block], x: f32, width: f32) -> Content {
    let mut content = Content {
        lines: Vec::new(),
        shapes: Vec::new(),
        height: 0.0,
    };
    let mut y = 0.0;
    let mut previous_after = 0.0;
    let mut first = true;
    let mut i = 0;
    while i < blocks.len() {
        match &blocks[i] {
            Block::Paragraph(_) => {
                // Подряд идущие абзацы — для правила «одного стиля».
                let run = blocks[i..]
                    .iter()
                    .take_while(|b| matches!(b, Block::Paragraph(_)))
                    .count();
                let mut group = Vec::with_capacity(run);
                for _ in 0..run {
                    if let Some(item) = ctx.take() {
                        group.push(item);
                    }
                }
                let spacing: Vec<ParagraphBlock<'_>> = group
                    .iter()
                    .map(|(_, p)| ParagraphBlock {
                        lines: &[],
                        spacing: p.spacing,
                        style: p.style.as_deref(),
                    })
                    .collect();
                for (j, (index, p)) in group.iter().enumerate() {
                    let (before, after) = effective_spacing(&spacing, j);
                    y += if first {
                        before
                    } else {
                        gap_between(previous_after, before, ctx.add_spacing) - previous_after
                    };
                    for (k, &height) in p.heights.iter().enumerate() {
                        content.lines.push(BoxLine {
                            prepared: Arc::clone(p),
                            k,
                            paragraph: *index,
                            x,
                            y,
                            height,
                            cell: None,
                        });
                        y += height;
                    }
                    y += after;
                    previous_after = after;
                    first = false;
                }
                i += run;
            }
            Block::Table(table) => {
                for row in build_rows(ctx, table, x, width) {
                    draw_row(&row, 0.0, y, true, true, &mut content);
                    y += row.height;
                }
                previous_after = 0.0;
                first = false;
                i += 1;
            }
            Block::Unknown(_) => i += 1,
        }
    }
    content.height = y;
    content
}

/// Строки таблицы `table` в области с левым краем `origin` и шириной
/// `width`.
pub(crate) fn build_rows(ctx: &mut Ctx<'_>, table: &Table, origin: f32, width: f32) -> Vec<RowBox> {
    let geometry = TableGeometry::of(ctx.doc, table, width);
    let borders = geometry.props.borders.clone().unwrap_or_default();
    let table_fill = geometry.props.shading.as_ref().and_then(|s| s.background());
    let row_count = table.rows.len();
    let mut rows: Vec<RowBox> = Vec::with_capacity(row_count);
    // Начало каждой ячейки в сетке и её содержимое (до раскладки по
    // строкам, с отступом поля сверху).
    let mut merges: Vec<Vec<(usize, Option<VerticalMerge>, f32)>> = Vec::new();
    for (r, row) in table.rows.iter().enumerate() {
        let mut start = 0;
        let mut segments = Vec::with_capacity(row.cells.len());
        let mut starts = Vec::with_capacity(row.cells.len());
        let mut needed: f32 = 0.0;
        let count = row.cells.len();
        for (c, cell) in row.cells.iter().enumerate() {
            let (left, right) = geometry.cell_edges(start, cell);
            let (text_x, text_width) = geometry.text_area(start, cell);
            let m = geometry.margins(cell);
            let content = stack(ctx, &cell.blocks, origin + text_x, text_width);
            let own = cell.props.borders.clone().unwrap_or_default();
            let first_col = start == 0;
            let last_col = c + 1 == count;
            let first_row = r == 0;
            let last_row = r + 1 == row_count;
            let merge = cell.props.v_merge;
            let top = if merge == Some(VerticalMerge::Continue) {
                None
            } else {
                own.top.clone().or(if first_row {
                    borders.top.clone()
                } else {
                    borders.inside_h.clone()
                })
            };
            let bottom = if last_row {
                own.bottom.clone().or(borders.bottom.clone())
            } else {
                own.bottom.clone()
            };
            let left_border = own.left.clone().or(if first_col {
                borders.left.clone()
            } else {
                borders.inside_v.clone()
            });
            let right_border = if last_col {
                own.right.clone().or(borders.right.clone())
            } else {
                own.right.clone()
            };
            let fill = cell
                .props
                .shading
                .as_ref()
                .map_or(table_fill, |s| s.background());
            let height = m[0] + content.height + m[2];
            if merge != Some(VerticalMerge::Restart) {
                needed = needed.max(height);
            }
            starts.push((start, merge, height));
            let mut lines = content.lines;
            let mut shapes = content.shapes;
            for line in &mut lines {
                line.y += m[0];
                if let Some(area) = &mut line.cell {
                    area.top += m[0];
                    area.bottom += m[0];
                }
            }
            for shape in &mut shapes {
                shape.y += m[0];
            }
            segments.push(Segment {
                left: origin + left,
                right: origin + right,
                lines,
                shapes,
                top,
                bottom,
                left_border,
                right_border,
                fill,
                margin_top: m[0],
                margin_bottom: m[2],
            });
            start += cell.span();
        }
        let height = match row.props.height {
            Some(h) => {
                let value = h.value.to_points() as f32;
                match h.rule {
                    HeightRule::Exact if value > 0.0 => value,
                    HeightRule::AtLeast | HeightRule::Exact => needed.max(value),
                    HeightRule::Auto => needed,
                }
            }
            None => needed,
        };
        // Выравнивание по вертикали в необъединённых ячейках.
        for ((cell, segment), &(_, merge, content)) in
            row.cells.iter().zip(&mut segments).zip(&starts)
        {
            if merge.is_some() {
                continue;
            }
            let free = (height - content).max(0.0);
            let shift = match cell.props.v_align {
                Some(CellVerticalAlign::Center) => free / 2.0,
                Some(CellVerticalAlign::Bottom) => free,
                _ => 0.0,
            };
            if shift > 0.0 {
                shift_segment(segment, shift);
            }
        }
        rows.push(RowBox {
            height,
            header: row.props.header == Some(true),
            cant_split: row.props.cant_split == Some(true),
            segments,
            split_border: borders.inside_h.clone().or(borders.bottom.clone()),
        });
        merges.push(starts);
    }
    spread_merged_cells(&mut rows, &merges);
    rows
}

fn shift_segment(segment: &mut Segment, dy: f32) {
    for line in &mut segment.lines {
        line.y += dy;
        if let Some(area) = &mut line.cell {
            area.top += dy;
            area.bottom += dy;
        }
    }
    for shape in &mut segment.shapes {
        shape.y += dy;
    }
}

/// Объединённые по вертикали ячейки: строки объединения растягиваются,
/// чтобы содержимое поместилось, а строки текста переходят в те строки
/// таблицы, на которые приходятся; внутренних линий нет.
fn spread_merged_cells(rows: &mut [RowBox], merges: &[Vec<(usize, Option<VerticalMerge>, f32)>]) {
    for r in 0..rows.len() {
        for (c, &(start, merge, needed)) in merges[r].iter().enumerate() {
            if merge != Some(VerticalMerge::Restart) {
                continue;
            }
            // Ячейки ниже, продолжающие эту: (строка, номер ячейки).
            let mut chain = vec![(r, c)];
            for (below, cells) in merges.iter().enumerate().skip(r + 1) {
                match cells
                    .iter()
                    .position(|&(s, m, _)| s == start && m == Some(VerticalMerge::Continue))
                {
                    Some(index) => chain.push((below, index)),
                    None => break,
                }
            }
            let total: f32 = chain.iter().map(|&(row, _)| rows[row].height).sum();
            if let Some(&(last, _)) = chain.last()
                && total < needed
            {
                rows[last].height += needed - total;
            }
            if chain.len() < 2 {
                continue;
            }
            // Строки текста — в строку таблицы, где начинаются.
            let lines = std::mem::take(&mut rows[r].segments[c].lines);
            let mut tops = Vec::with_capacity(chain.len());
            let mut y = 0.0;
            for &(row, _) in &chain {
                tops.push(y);
                y += rows[row].height;
            }
            for mut line in lines {
                let j = tops.iter().rposition(|&t| line.y >= t).unwrap_or(0);
                let (row, index) = chain[j];
                line.y -= tops[j];
                if let Some(area) = &mut line.cell {
                    area.top -= tops[j];
                    area.bottom -= tops[j];
                }
                if let Some(segment) = rows[row].segments.get_mut(index) {
                    segment.lines.push(line);
                }
            }
            // Нижняя линия — только у последней ячейки объединения.
            let bottom = rows[chain[chain.len() - 1].0]
                .segments
                .get(chain[chain.len() - 1].1)
                .and_then(|s| s.bottom.clone());
            for (n, &(row, index)) in chain.iter().enumerate() {
                if let Some(segment) = rows[row].segments.get_mut(index) {
                    segment.bottom = if n + 1 == chain.len() {
                        bottom.clone()
                    } else {
                        None
                    };
                }
            }
        }
    }
}

/// Куда рисовать строку таблицы: строки абзацев и прямоугольники.
pub(crate) trait Canvas {
    fn line(&mut self, line: &BoxLine, dx: f32, dy: f32, area: CellArea);
    fn shape(&mut self, shape: Shape);
}

impl Canvas for Content {
    fn line(&mut self, line: &BoxLine, dx: f32, dy: f32, area: CellArea) {
        let mut line = line.clone();
        line.x += dx;
        line.y += dy;
        line.cell = Some(match line.cell {
            Some(inner) => inner.moved(dx, dy),
            None => area,
        });
        self.lines.push(line);
    }

    fn shape(&mut self, shape: Shape) {
        self.shapes.push(shape);
    }
}

/// Рисует строку таблицы с верхом в `top` (по горизонтали — со сдвигом
/// `dx`). `top_edge` и `bottom_edge` — рисовать ли линии сверху и снизу
/// (у куска строки, разорванной между страницами, вместо них — линия
/// разрыва).
pub(crate) fn draw_row(
    row: &RowBox,
    dx: f32,
    top: f32,
    top_edge: bool,
    bottom_edge: bool,
    canvas: &mut impl Canvas,
) {
    let bottom = top + row.height;
    for segment in &row.segments {
        let (left, right) = (segment.left + dx, segment.right + dx);
        if let Some(color) = segment.fill {
            canvas.shape(Shape {
                x: left,
                y: top,
                width: right - left,
                height: row.height,
                color,
                kind: ShapeKind::Fill,
            });
        }
        let area = CellArea {
            left,
            top,
            right,
            bottom,
        };
        for line in &segment.lines {
            canvas.line(line, dx, top, area);
        }
        for shape in &segment.shapes {
            canvas.shape(Shape {
                x: shape.x + dx,
                y: shape.y + top,
                ..*shape
            });
        }
        let top_border = if top_edge {
            segment.top.as_ref()
        } else {
            row.split_border.as_ref()
        };
        let bottom_border = if bottom_edge {
            segment.bottom.as_ref()
        } else {
            row.split_border.as_ref()
        };
        if let Some(b) = top_border {
            horizontal(canvas, b, left, right, top);
        }
        if let Some(b) = bottom_border {
            horizontal(canvas, b, left, right, bottom);
        }
        if let Some(b) = &segment.left_border {
            vertical(canvas, b, left, top, bottom);
        }
        if let Some(b) = &segment.right_border {
            vertical(canvas, b, right, top, bottom);
        }
    }
}

fn border_color(border: &Border) -> (u8, u8, u8) {
    match border.color {
        Some(Color::Rgb(r, g, b)) => (r, g, b),
        _ => (0, 0, 0),
    }
}

fn horizontal(canvas: &mut impl Canvas, border: &Border, left: f32, right: f32, y: f32) {
    let w = border.width_points();
    if w <= 0.0 {
        return;
    }
    canvas.shape(Shape {
        x: left - w / 2.0,
        y: y - w / 2.0,
        width: right - left + w,
        height: w,
        color: border_color(border),
        kind: ShapeKind::Rule,
    });
}

fn vertical(canvas: &mut impl Canvas, border: &Border, x: f32, top: f32, bottom: f32) {
    let w = border.width_points();
    if w <= 0.0 {
        return;
    }
    canvas.shape(Shape {
        x: x - w / 2.0,
        y: top,
        width: w,
        height: bottom - top,
        color: border_color(border),
        kind: ShapeKind::Rule,
    });
}

/// Делит строку таблицы, не влезающую на страницу: в первую часть —
/// строки текста каждой ячейки, которые помещаются в `available` пт,
/// остальное — во вторую. `None`, если не помещается ни одна строка.
pub(crate) fn split_row(row: &RowBox, available: f32) -> Option<(RowBox, RowBox)> {
    let mut first = row.clone();
    let mut rest = row.clone();
    let mut any = false;
    let mut first_height: f32 = 0.0;
    let mut rest_height: f32 = 0.0;
    for (n, segment) in row.segments.iter().enumerate() {
        let limit = available - segment.margin_bottom;
        let cut = segment
            .lines
            .iter()
            .filter(|l| l.y + l.height > limit + 0.01)
            .map(|l| l.y)
            .fold(f32::INFINITY, f32::min);
        let (keep, moved): (Vec<BoxLine>, Vec<BoxLine>) =
            segment.lines.iter().cloned().partition(|l| l.y < cut);
        any |= !keep.is_empty();
        let kept_bottom = keep
            .iter()
            .map(|l| l.y + l.height)
            .fold(segment.margin_top, f32::max);
        first_height = first_height.max(kept_bottom + segment.margin_bottom);
        let shift = if cut.is_finite() {
            cut - segment.margin_top
        } else {
            0.0
        };
        let moved: Vec<BoxLine> = moved
            .into_iter()
            .map(|mut l| {
                l.y -= shift;
                if let Some(area) = &mut l.cell {
                    area.top -= shift;
                    area.bottom -= shift;
                }
                l
            })
            .collect();
        rest_height = rest_height.max(
            moved
                .iter()
                .map(|l| l.y + l.height)
                .fold(segment.margin_top, f32::max)
                + segment.margin_bottom,
        );
        let (shapes_first, shapes_rest): (Vec<Shape>, Vec<Shape>) =
            segment.shapes.iter().partition(|s| s.y < cut);
        first.segments[n].lines = keep;
        first.segments[n].shapes = shapes_first;
        rest.segments[n].lines = moved;
        rest.segments[n].shapes = shapes_rest
            .into_iter()
            .map(|s| Shape {
                y: s.y - shift,
                ..s
            })
            .collect();
    }
    if !any {
        return None;
    }
    first.height = first_height.min(available).max(0.0);
    rest.height = rest_height.max(row.height - first.height).max(0.0);
    rest.header = false;
    Some((first, rest))
}
