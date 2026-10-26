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
    /// Какие части оформления из стиля таблицы включены (`w:tblLook`):
    /// строка заголовка, итоговая строка, первый и последний столбцы,
    /// чередование.
    pub look: Option<TableLook>,
    /// Сколько строк в одной полосе чередования (`w:tblStyleRowBandSize`).
    pub row_band_size: Option<u32>,
    /// Сколько столбцов в одной полосе (`w:tblStyleColBandSize`).
    pub column_band_size: Option<u32>,
    /// Элементы управления содержимым (`w:sdt`), внутри которых лежит
    /// таблица, от внешнего к внутреннему. В `w:tblPr` не пишутся.
    pub controls: Vec<crate::ContentControlRef>,
    /// Непонятые свойства как есть.
    pub unknown: Vec<RawXml>,
}

/// Параметры стиля таблицы (`w:tblLook`, «Конструктор таблиц» в Word):
/// какие части условного оформления стиля действуют.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TableLook {
    /// Строка заголовка.
    pub first_row: bool,
    /// Итоговая строка.
    pub last_row: bool,
    /// Первый столбец.
    pub first_column: bool,
    /// Последний столбец.
    pub last_column: bool,
    /// Без чередования строк.
    pub no_h_band: bool,
    /// Без чередования столбцов.
    pub no_v_band: bool,
}

impl TableLook {
    /// Как у новой таблицы в Word (`04A0`): заголовок, первый столбец,
    /// чередование строк.
    pub const WORD: TableLook = TableLook {
        first_row: true,
        last_row: false,
        first_column: true,
        last_column: false,
        no_h_band: false,
        no_v_band: true,
    };

    /// Старая запись числом (`w:val="04A0"`).
    pub fn from_bits(bits: u16) -> TableLook {
        TableLook {
            first_row: bits & 0x0020 != 0,
            last_row: bits & 0x0040 != 0,
            first_column: bits & 0x0080 != 0,
            last_column: bits & 0x0100 != 0,
            no_h_band: bits & 0x0200 != 0,
            no_v_band: bits & 0x0400 != 0,
        }
    }

    /// Запись числом (см. [`TableLook::from_bits`]).
    pub fn bits(self) -> u16 {
        [
            (self.first_row, 0x0020),
            (self.last_row, 0x0040),
            (self.first_column, 0x0080),
            (self.last_column, 0x0100),
            (self.no_h_band, 0x0200),
            (self.no_v_band, 0x0400),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, bit)| bit)
        .sum()
    }
}

/// Часть таблицы, у которой своё оформление в стиле таблицы
/// (`w:tblStylePr w:type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TableArea {
    /// Вся таблица.
    WholeTable,
    /// Нечётные полосы столбцов.
    Band1Vertical,
    /// Чётные полосы столбцов.
    Band2Vertical,
    /// Нечётные полосы строк.
    Band1Horizontal,
    /// Чётные полосы строк.
    Band2Horizontal,
    /// Первый столбец.
    FirstColumn,
    /// Последний столбец.
    LastColumn,
    /// Первая строка (заголовок).
    FirstRow,
    /// Последняя строка (итоги).
    LastRow,
    /// Правая верхняя ячейка.
    TopRightCell,
    /// Левая верхняя ячейка.
    TopLeftCell,
    /// Правая нижняя ячейка.
    BottomRightCell,
    /// Левая нижняя ячейка.
    BottomLeftCell,
}

impl TableArea {
    /// Все части в том порядке, в каком Word накладывает их оформление:
    /// следующая перекрывает предыдущие.
    pub const ORDER: [TableArea; 13] = [
        TableArea::WholeTable,
        TableArea::Band1Vertical,
        TableArea::Band2Vertical,
        TableArea::Band1Horizontal,
        TableArea::Band2Horizontal,
        TableArea::FirstColumn,
        TableArea::LastColumn,
        TableArea::FirstRow,
        TableArea::LastRow,
        TableArea::TopRightCell,
        TableArea::TopLeftCell,
        TableArea::BottomRightCell,
        TableArea::BottomLeftCell,
    ];

    fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// Набор частей таблицы, к которым относится ячейка.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TableAreas(u16);

impl TableAreas {
    /// Относится ли ячейка к части `area`.
    pub fn contains(self, area: TableArea) -> bool {
        self.0 & area.bit() != 0
    }

    /// Добавляет часть.
    pub fn insert(&mut self, area: TableArea) {
        self.0 |= area.bit();
    }

    /// Части ячейки в строке `row` из `rows`, занимающей столбцы сетки
    /// `columns` (начало и конец, не включая) из `grid`, при параметрах
    /// `look` и полосах по `bands` строк и столбцов.
    pub fn of_cell(
        look: TableLook,
        (row, rows): (usize, usize),
        (columns, grid): ((usize, usize), usize),
        bands: (u32, u32),
    ) -> TableAreas {
        let mut areas = TableAreas::default();
        areas.insert(TableArea::WholeTable);
        let first_row = look.first_row && row == 0;
        let last_row = look.last_row && rows > 0 && row + 1 == rows;
        let first_column = look.first_column && columns.0 == 0;
        let last_column = look.last_column && grid > 0 && columns.1 >= grid;
        // Полосы считаются без строки заголовка и первого столбца.
        if !look.no_h_band && !first_row && !last_row {
            let index = row.saturating_sub(usize::from(look.first_row));
            let band = index / bands.0.max(1) as usize;
            areas.insert(if band.is_multiple_of(2) {
                TableArea::Band1Horizontal
            } else {
                TableArea::Band2Horizontal
            });
        }
        if !look.no_v_band && !first_column && !last_column {
            let index = columns.0.saturating_sub(usize::from(look.first_column));
            let band = index / bands.1.max(1) as usize;
            areas.insert(if band.is_multiple_of(2) {
                TableArea::Band1Vertical
            } else {
                TableArea::Band2Vertical
            });
        }
        for (on, area) in [
            (first_row, TableArea::FirstRow),
            (last_row, TableArea::LastRow),
            (first_column, TableArea::FirstColumn),
            (last_column, TableArea::LastColumn),
            (first_row && first_column, TableArea::TopLeftCell),
            (first_row && last_column, TableArea::TopRightCell),
            (last_row && first_column, TableArea::BottomLeftCell),
            (last_row && last_column, TableArea::BottomRightCell),
        ] {
            if on {
                areas.insert(area);
            }
        }
        areas
    }
}

/// Оформление части таблицы в стиле таблицы (`w:tblStylePr`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableAreaStyle {
    /// Какая часть.
    pub area: TableArea,
    /// Оформление абзацев.
    pub paragraph: crate::ParagraphProps,
    /// Оформление символов.
    pub run: crate::RunProps,
    /// Свойства таблицы.
    pub table: TableProps,
    /// Свойства ячеек: границы, заливка.
    pub cell: CellProps,
    /// Непонятые элементы (`w:trPr` и т. п.) как есть.
    pub unknown: Vec<RawXml>,
}

impl TableAreaStyle {
    /// Пустое оформление части `area`.
    pub fn new(area: TableArea) -> TableAreaStyle {
        TableAreaStyle {
            area,
            paragraph: crate::ParagraphProps::default(),
            run: crate::RunProps::default(),
            table: TableProps::default(),
            cell: CellProps::default(),
            unknown: Vec::new(),
        }
    }
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

    #[test]
    fn look_bits() {
        assert_eq!(TableLook::from_bits(0x04A0), TableLook::WORD);
        assert_eq!(TableLook::WORD.bits(), 0x04A0);
    }

    #[test]
    fn areas_of_cells() {
        let areas =
            |row, columns| TableAreas::of_cell(TableLook::WORD, (row, 5), (columns, 3), (1, 1));
        let top_left = areas(0, (0, 1));
        assert!(top_left.contains(TableArea::FirstRow));
        assert!(top_left.contains(TableArea::FirstColumn));
        assert!(top_left.contains(TableArea::TopLeftCell));
        assert!(!top_left.contains(TableArea::Band1Horizontal));
        // Полосы: первая после заголовка — нечётная.
        assert!(areas(1, (1, 2)).contains(TableArea::Band1Horizontal));
        assert!(areas(2, (1, 2)).contains(TableArea::Band2Horizontal));
        assert!(areas(3, (1, 2)).contains(TableArea::Band1Horizontal));
        // Итоговой строки и чередования столбцов в WORD нет.
        let last = areas(4, (2, 3));
        assert!(!last.contains(TableArea::LastRow));
        assert!(!last.contains(TableArea::LastColumn));
        assert!(!last.contains(TableArea::Band1Vertical));
        assert!(last.contains(TableArea::WholeTable));
        // Полосы по две строки.
        let wide = |row| TableAreas::of_cell(TableLook::WORD, (row, 9), ((1, 2), 3), (2, 1));
        assert!(wide(2).contains(TableArea::Band1Horizontal));
        assert!(wide(3).contains(TableArea::Band2Horizontal));
    }
}
