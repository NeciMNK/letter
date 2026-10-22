//! Сборка документа по порядку: текст, абзацы, таблицы. Общая часть
//! читателей HTML и Markdown.

use letter_model::{
    Block, BreakKind, Document, Inline, Paragraph, ParagraphProps, Run, RunContent, RunProps,
    Table, TableCell, TableRow,
};

use crate::styles::base_document;

/// Наибольшая вложенность таблиц: глубже содержимое идёт в текущую ячейку.
const MAX_TABLE_DEPTH: usize = 16;

/// Открытая таблица.
struct OpenTable {
    table: Table,
    /// Ячейки текущей строки (`None` — строка не начата).
    row: Option<Vec<TableCell>>,
    /// Блоки текущей ячейки (`None` — ячейка не начата).
    cell: Option<Vec<Block>>,
}

/// Собирает документ.
pub struct Builder {
    doc: Document,
    body: Vec<Block>,
    tables: Vec<OpenTable>,
    /// Лишние (слишком глубокие) таблицы, которые не открывались.
    ignored_tables: usize,
    /// Оформление текущего абзаца.
    para: ParagraphProps,
    runs: Vec<Run>,
}

impl Builder {
    /// Пустой документ со стилями нового документа Word.
    pub fn new() -> Builder {
        Builder {
            doc: base_document(),
            body: Vec::new(),
            tables: Vec::new(),
            ignored_tables: 0,
            para: ParagraphProps::default(),
            runs: Vec::new(),
        }
    }

    /// Документ, в который потом попадут блоки.
    pub fn doc(&self) -> &Document {
        &self.doc
    }

    /// Документ, в который потом попадут блоки (для свойств и стилей).
    pub fn doc_mut(&mut self) -> &mut Document {
        &mut self.doc
    }

    /// Добавляет текст в текущий абзац.
    pub fn text(&mut self, text: &str, props: &RunProps) {
        if text.is_empty() {
            return;
        }
        match self.runs.last_mut() {
            Some(run) if run.props == *props => match run.content.last_mut() {
                Some(RunContent::Text(t)) => t.push_str(text),
                _ => run.content.push(RunContent::Text(text.to_owned())),
            },
            _ => self.runs.push(Run::text(text).with_props(props.clone())),
        }
    }

    /// Добавляет табуляцию или разрыв в текущий абзац.
    pub fn content(&mut self, content: RunContent, props: &RunProps) {
        match self.runs.last_mut() {
            Some(run) if run.props == *props => run.content.push(content),
            _ => self.runs.push(Run {
                props: props.clone(),
                content: vec![content],
            }),
        }
    }

    /// Разрыв строки.
    pub fn line_break(&mut self, props: &RunProps) {
        self.content(RunContent::Break(BreakKind::Line), props);
    }

    /// Последний знак текущего абзаца (для расстановки пробелов).
    pub fn last_char(&self) -> Option<char> {
        self.runs
            .iter()
            .flat_map(|r| &r.content)
            .next_back()
            .map(|c| match c {
                RunContent::Text(t) => t.chars().next_back().unwrap_or(' '),
                RunContent::Break(_) => '\n',
                _ => ' ',
            })
    }

    /// В текущем абзаце уже есть содержимое.
    pub fn has_content(&self) -> bool {
        !self.runs.is_empty()
    }

    /// Убирает разрыв строки в самом конце абзаца (браузер его не
    /// показывает). `true`, если в абзаце было содержимое.
    pub fn drop_trailing_line_break(&mut self) -> bool {
        let had = self.has_content();
        if let Some(run) = self.runs.last_mut()
            && run.content.last() == Some(&RunContent::Break(BreakKind::Line))
        {
            run.content.pop();
            if run.content.is_empty() {
                self.runs.pop();
            }
        }
        had
    }

    /// Начинает новый абзац с оформлением `props`. Начатый абзац с
    /// содержимым завершается.
    pub fn start_paragraph(&mut self, props: ParagraphProps) {
        self.end_paragraph();
        self.para = props;
    }

    /// Завершает абзац, если в нём что-то есть.
    pub fn end_paragraph(&mut self) {
        if self.has_content() {
            self.push_paragraph();
        }
    }

    /// Завершает абзац, даже пустой (пустая строка в тексте).
    pub fn empty_paragraph(&mut self) {
        self.push_paragraph();
    }

    fn push_paragraph(&mut self) {
        let mut runs = std::mem::take(&mut self.runs);
        // Пробелы в конце абзаца не видны и мешают сравнивать текст.
        if let Some(run) = runs.last_mut()
            && let Some(RunContent::Text(text)) = run.content.last_mut()
        {
            let trimmed = text.trim_end_matches(' ').len();
            text.truncate(trimmed);
            if text.is_empty() {
                run.content.pop();
            }
        }
        runs.retain(|r| !r.content.is_empty());
        let paragraph = Paragraph {
            props: std::mem::take(&mut self.para),
            content: runs.into_iter().map(Inline::Run).collect(),
        };
        self.push_block(paragraph.into());
    }

    fn push_block(&mut self, block: Block) {
        match self.tables.last_mut() {
            Some(table) => {
                if table.row.is_none() {
                    table.row = Some(Vec::new());
                }
                table.cell.get_or_insert_with(Vec::new).push(block);
            }
            None => self.body.push(block),
        }
    }

    /// Начинает таблицу.
    pub fn start_table(&mut self) {
        self.end_paragraph();
        if self.tables.len() >= MAX_TABLE_DEPTH {
            self.ignored_tables += 1;
            return;
        }
        self.tables.push(OpenTable {
            table: Table::default(),
            row: None,
            cell: None,
        });
    }

    /// Начинает строку таблицы.
    pub fn start_row(&mut self) {
        self.end_row();
        if let Some(table) = self.tables.last_mut() {
            table.row = Some(Vec::new());
        }
    }

    /// Начинает ячейку.
    pub fn start_cell(&mut self) {
        self.end_cell();
        if let Some(table) = self.tables.last_mut() {
            table.row.get_or_insert_with(Vec::new);
            table.cell = Some(Vec::new());
        }
    }

    /// Завершает ячейку.
    pub fn end_cell(&mut self) {
        if self.ignored_tables > 0 || self.tables.is_empty() {
            return;
        }
        self.end_paragraph();
        let Some(table) = self.tables.last_mut() else {
            return;
        };
        if let Some(mut blocks) = table.cell.take() {
            if !matches!(blocks.last(), Some(Block::Paragraph(_))) {
                // Word требует, чтобы ячейка кончалась абзацем.
                blocks.push(Paragraph::default().into());
            }
            table.row.get_or_insert_with(Vec::new).push(TableCell {
                blocks,
                ..TableCell::default()
            });
        }
    }

    /// Завершает строку таблицы.
    pub fn end_row(&mut self) {
        self.end_cell();
        if self.ignored_tables > 0 {
            return;
        }
        if let Some(table) = self.tables.last_mut()
            && let Some(cells) = table.row.take()
            && !cells.is_empty()
        {
            table.table.rows.push(TableRow {
                cells,
                ..TableRow::default()
            });
        }
    }

    /// Завершает таблицу.
    pub fn end_table(&mut self) {
        if self.ignored_tables > 0 {
            self.ignored_tables -= 1;
            return;
        }
        self.end_row();
        if let Some(open) = self.tables.pop()
            && !open.table.rows.is_empty()
        {
            self.push_block(open.table.into());
        }
    }

    /// Внутри таблицы.
    pub fn in_table(&self) -> bool {
        !self.tables.is_empty()
    }

    /// Готовый документ. В нём всегда есть хотя бы один абзац, а
    /// последний блок — абзац (как требует Word).
    pub fn finish(mut self) -> Document {
        self.end_paragraph();
        self.ignored_tables = 0;
        while !self.tables.is_empty() {
            self.end_table();
        }
        if !matches!(self.body.last(), Some(Block::Paragraph(_))) {
            self.body.push(Paragraph::default().into());
        }
        let mut doc = self.doc;
        doc.sections[0].blocks = self.body;
        doc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paragraphs_and_tables() {
        let mut b = Builder::new();
        let plain = RunProps::default();
        let bold = RunProps {
            bold: Some(true),
            ..RunProps::default()
        };
        b.text("a", &plain);
        b.text("b", &plain);
        b.text("c ", &bold);
        b.start_paragraph(ParagraphProps::default());
        b.start_table();
        b.start_row();
        b.start_cell();
        b.text("x", &plain);
        b.start_cell();
        b.start_row();
        b.text("y", &plain);
        // Таблица не закрыта: закроется в конце.
        let doc = b.finish();
        let blocks = &doc.sections[0].blocks;
        assert_eq!(blocks.len(), 3);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!()
        };
        assert_eq!(p.runs().count(), 2);
        assert_eq!(p.plain_text(), "abc");
        let Block::Table(t) = &blocks[1] else {
            panic!()
        };
        assert_eq!(t.plain_text(), "x\t\ny");
        assert_eq!(blocks[2], Paragraph::default().into());
    }

    #[test]
    fn empty_document_has_a_paragraph() {
        let doc = Builder::new().finish();
        assert_eq!(doc.sections[0].blocks, vec![Paragraph::default().into()]);
    }
}
