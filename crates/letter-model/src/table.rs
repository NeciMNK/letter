//! Блоки раздела: абзацы и таблицы.
//!
//! Таблица устроена как в Word: сетка столбцов ([`Table::grid`]), строки,
//! в строках ячейки. Ячейка может занимать несколько столбцов сетки
//! (`grid_span`) и продолжать ячейку сверху (`v_merge`) — так получаются
//! объединённые ячейки.

use crate::{Alignment, Borders, Paragraph, RawXml, Shading, Twips};

/// Элемент раздела или ячейки таблицы.
// Почти все блоки — абзацы, поэтому прятать абзац в `Box` невыгодно.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// Абзац.
    Paragraph(Paragraph),
    /// Таблица.
    Table(Table),
    /// Непонятый элемент (элемент управления содержимым, закладка и т. п.),
    /// сохранённый как есть.
    Unknown(RawXml),
}

impl Block {
    /// Текст блока без оформления. Ячейки таблицы разделяются `\t`,
    /// строки и абзацы — `\n`.
    pub fn plain_text(&self) -> String {
        match self {
            Block::Paragraph(p) => p.plain_text(),
            Block::Table(t) => t.plain_text(),
            Block::Unknown(_) => String::new(),
        }
    }
}

impl From<Paragraph> for Block {
    fn from(p: Paragraph) -> Block {
        Block::Paragraph(p)
    }
}

impl From<Table> for Block {
    fn from(t: Table) -> Block {
        Block::Table(t)
    }
}

/// Таблица.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Table {
    /// Свойства таблицы (`w:tblPr`). В коробке, чтобы [`Block`] оставался
    /// небольшим.
    pub props: Box<TableProps>,
    /// Ширины столбцов сетки (`w:tblGrid`).
    pub grid: Vec<Twips>,
    /// Непонятые части таблицы перед строками (например, `w:tblGrid`
    /// с правками), как есть.
    pub raw_props: Vec<RawXml>,
    /// Строки сверху вниз.
    pub rows: Vec<TableRow>,
}

impl Table {
    /// Текст таблицы (см. [`Block::plain_text`]).
    pub fn plain_text(&self) -> String {
        self.rows
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(TableCell::plain_text)
                    .collect::<Vec<_>>()
                    .join("\t")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Строка таблицы.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableRow {
    /// Свойства строки (`w:trPr`).
    pub props: RowProps,
    /// Непонятые свойства строки (`w:tblPrEx`) как есть.
    pub raw_props: Vec<RawXml>,
    /// Ячейки слева направо.
    pub cells: Vec<TableCell>,
}

/// Ячейка таблицы. Внутри — те же блоки, что и в разделе, поэтому
/// таблицы могут быть вложенными.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableCell {
    /// Свойства ячейки (`w:tcPr`).
    pub props: CellProps,
    /// Непонятые части ячейки перед содержимым, как есть.
    pub raw_props: Vec<RawXml>,
    /// Содержимое ячейки. Word всегда записывает хотя бы один абзац, но
    /// в битом файле список может быть пустым: вёрстка обязана это выдерживать.
    pub blocks: Vec<Block>,
}

impl TableCell {
    /// Ячейка с одним абзацем текста.
    pub fn text(text: impl Into<String>) -> TableCell {
        TableCell {
            blocks: vec![Paragraph::text(text).into()],
            ..TableCell::default()
        }
    }

    /// Текст ячейки: абзацы через `\n`.
    pub fn plain_text(&self) -> String {
        self.blocks
            .iter()
            .map(Block::plain_text)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Ширина или отступ в таблице (`w:tblW`, `w:tcW`, `w:tblInd`…).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableWidth {
    /// Подбирается по содержимому (`auto`).
    Auto,
    /// Не задана (`nil`).
    Nil,
    /// В твипах (`dxa`).
    Twips(Twips),
    /// В пятидесятых долях процента (`pct`): 5000 — 100 %.
    Percent(i32),
}

/// Как подбираются ширины столбцов.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableLayout {
    /// По содержимому (автоподбор, `autofit`).
    Auto,
    /// Фиксированные (`fixed`).
    Fixed,
}

/// Поля ячеек: расстояние от границ до текста.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellMargins {
    /// Сверху.
    pub top: Option<TableWidth>,
    /// Слева.
    pub left: Option<TableWidth>,
    /// Снизу.
    pub bottom: Option<TableWidth>,
    /// Справа.
    pub right: Option<TableWidth>,
}

/// Свойства таблицы (`w:tblPr`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableProps {
    /// Стиль таблицы (`w:tblStyle`).
    pub style: Option<String>,
    /// Ширина таблицы.
    pub width: Option<TableWidth>,
    /// Выравнивание таблицы на странице (`w:jc`).
    pub alignment: Option<Alignment>,
    /// Отступ слева (`w:tblInd`).
    pub indent: Option<TableWidth>,
    /// Границы.
    pub borders: Option<Borders>,
    /// Заливка.
    pub shading: Option<Shading>,
    /// Подбор ширин столбцов.
    pub layout: Option<TableLayout>,
    /// Поля ячеек по умолчанию.
    pub cell_margins: Option<CellMargins>,
    /// Элементы управления содержимым (`w:sdt`), внутри которых лежит
    /// таблица, от внешнего к внутреннему. В `w:tblPr` не пишутся.
    pub controls: Vec<crate::ContentControlRef>,
    /// Непонятые свойства как есть.
    pub unknown: Vec<RawXml>,
}

/// Правило высоты строки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeightRule {
    /// По содержимому (значение не используется).
    Auto,
    /// Не меньше значения.
    AtLeast,
    /// Ровно значение.
    Exact,
}

/// Высота строки (`w:trHeight`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowHeight {
    /// Высота.
    pub value: Twips,
    /// Правило.
    pub rule: HeightRule,
}

/// Свойства строки (`w:trPr`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RowProps {
    /// Высота строки.
    pub height: Option<RowHeight>,
    /// Повторять как заголовок на каждой странице (`w:tblHeader`).
    pub header: Option<bool>,
    /// Не разрывать строку между страницами (`w:cantSplit`).
    pub cant_split: Option<bool>,
    /// Непонятые свойства как есть.
    pub unknown: Vec<RawXml>,
}

/// Объединение ячеек по вертикали (`w:vMerge`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalMerge {
    /// Первая ячейка объединения.
    Restart,
    /// Продолжение ячейки сверху.
    Continue,
}

/// Выравнивание текста в ячейке по вертикали (`w:vAlign`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellVerticalAlign {
    /// По верху.
    Top,
    /// По центру.
    Center,
    /// По низу.
    Bottom,
}

/// Свойства ячейки (`w:tcPr`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CellProps {
    /// Ширина ячейки (`w:tcW`).
    pub width: Option<TableWidth>,
    /// Сколько столбцов сетки занимает (`w:gridSpan`).
    pub grid_span: Option<u32>,
    /// Объединение с ячейками сверху и снизу.
    pub v_merge: Option<VerticalMerge>,
    /// Границы ячейки.
    pub borders: Option<Borders>,
    /// Заливка.
    pub shading: Option<Shading>,
    /// Поля ячейки (`w:tcMar`).
    pub margins: Option<CellMargins>,
    /// Выравнивание по вертикали.
    pub v_align: Option<CellVerticalAlign>,
    /// Непонятые свойства как есть.
    pub unknown: Vec<RawXml>,
}

impl TableCell {
    /// Сколько столбцов сетки занимает ячейка (не меньше 1).
    pub fn span(&self) -> usize {
        self.props.grid_span.unwrap_or(1).max(1) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_plain_text() {
        let table = Table {
            rows: vec![
                TableRow {
                    cells: vec![TableCell::text("A1"), TableCell::text("B1")],
                    ..TableRow::default()
                },
                TableRow {
                    cells: vec![TableCell::text("A2"), TableCell::text("B2")],
                    ..TableRow::default()
                },
            ],
            ..Table::default()
        };
        assert_eq!(Block::from(table).plain_text(), "A1\tB1\nA2\tB2");
    }

    #[test]
    fn nested_table() {
        let inner = Table {
            rows: vec![TableRow {
                cells: vec![TableCell::text("in")],
                ..TableRow::default()
            }],
            ..Table::default()
        };
        let cell = TableCell {
            blocks: vec![Paragraph::text("out").into(), inner.into()],
            ..TableCell::default()
        };
        assert_eq!(cell.span(), 1);
        assert_eq!(cell.plain_text(), "out\nin");
    }
}
