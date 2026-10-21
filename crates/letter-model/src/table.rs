//! Блоки раздела: абзацы и таблицы.
//!
//! Таблица пока хранит только структуру (строки и ячейки). Ширины, границы,
//! объединение ячеек и прочее добавит задача F5.2.

use crate::{Paragraph, RawXml};

/// Элемент раздела или ячейки таблицы.
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
    /// Свойства таблицы и сетка столбцов (`w:tblPr`, `w:tblGrid`) как есть.
    /// Разбирать их начнёт задача F5.2.
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
    /// Свойства строки (`w:trPr`, `w:tblPrEx`) как есть.
    pub raw_props: Vec<RawXml>,
    /// Ячейки слева направо.
    pub cells: Vec<TableCell>,
}

/// Ячейка таблицы. Внутри — те же блоки, что и в разделе, поэтому
/// таблицы могут быть вложенными.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableCell {
    /// Свойства ячейки (`w:tcPr`) как есть.
    pub raw_props: Vec<RawXml>,
    /// Содержимое ячейки. Word всегда записывает хотя бы один абзац, но
    /// в битом файле список может быть пустым: вёрстка обязана это выдерживать.
    pub blocks: Vec<Block>,
}

impl TableCell {
    /// Ячейка с одним абзацем текста.
    pub fn text(text: impl Into<String>) -> TableCell {
        TableCell {
            raw_props: Vec::new(),
            blocks: vec![Paragraph::text(text).into()],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_plain_text() {
        let table = Table {
            raw_props: Vec::new(),
            rows: vec![
                TableRow {
                    raw_props: Vec::new(),
                    cells: vec![TableCell::text("A1"), TableCell::text("B1")],
                },
                TableRow {
                    raw_props: Vec::new(),
                    cells: vec![TableCell::text("A2"), TableCell::text("B2")],
                },
            ],
        };
        assert_eq!(Block::from(table).plain_text(), "A1\tB1\nA2\tB2");
    }

    #[test]
    fn nested_table() {
        let inner = Table {
            raw_props: Vec::new(),
            rows: vec![TableRow {
                raw_props: Vec::new(),
                cells: vec![TableCell::text("in")],
            }],
        };
        let cell = TableCell {
            raw_props: Vec::new(),
            blocks: vec![Paragraph::text("out").into(), inner.into()],
        };
        assert_eq!(cell.plain_text(), "out\nin");
    }
}
