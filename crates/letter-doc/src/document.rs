//! Сборка модели документа из текста .doc.

use std::collections::HashMap;

use letter_model::{
    Block, BreakKind, Document, FieldChar, FieldCharKind, HeaderFooter, HeaderFooterKind, Inline,
    Paragraph, ParagraphProps, Run, RunContent, RunProps, Section, Twips,
};

use crate::ReadError;
use crate::fib::{Fib, Table, u16_at, u32_at};
use crate::fkp::{Span, chpx, find, papx};
use crate::section::{section_props, seds};
use crate::sprm::{Names, apply_paragraph, apply_run, sprms};
use crate::styles::{fonts, style_sheet};
use crate::table::{Place, RowDef, place, row_def, table};
use crate::text::{Piece, pieces, text};

/// Таблица, которая собирается: готовые строки, ячейки текущей строки
/// и содержимое текущей ячейки.
#[derive(Default)]
struct TableBuild {
    rows: Vec<(Vec<Vec<Block>>, RowDef)>,
    cells: Vec<Vec<Block>>,
    blocks: Vec<Block>,
}

/// Абзац с его местом в таблицах.
struct ParagraphEnd {
    props: ParagraphProps,
    place: Place,
    /// Описание строки, если абзац — конец строки таблицы.
    row: Option<RowDef>,
}

/// Абзацы и разделы по мере чтения текста.
struct Builder {
    sections: Vec<Section>,
    blocks: Vec<Block>,
    /// Открытые таблицы, от внешней к вложенным.
    tables: Vec<TableBuild>,
    paragraph: Paragraph,
    /// Набираемый текст текущего фрагмента и его свойства.
    pending: Vec<RunContent>,
    props: RunProps,
    text: String,
    /// Поля: внутри кода поля (до разделителя) текст — код.
    fields: Vec<bool>,
}

impl Builder {
    fn new() -> Builder {
        Builder {
            sections: Vec::new(),
            blocks: Vec::new(),
            tables: Vec::new(),
            paragraph: Paragraph::default(),
            pending: Vec::new(),
            props: RunProps::default(),
            text: String::new(),
            fields: Vec::new(),
        }
    }

    /// В коде поля (а не в его результате).
    fn in_code(&self) -> bool {
        self.fields.last() == Some(&true)
    }

    fn flush_text(&mut self) {
        if self.text.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.text);
        self.pending.push(if self.in_code() {
            RunContent::FieldCode(text)
        } else {
            RunContent::Text(text)
        });
    }

    fn push(&mut self, item: RunContent) {
        self.flush_text();
        self.pending.push(item);
    }

    /// Закрывает фрагмент: дальше текст с другими свойствами.
    fn end_run(&mut self) {
        self.flush_text();
        if !self.pending.is_empty() {
            let content = std::mem::take(&mut self.pending);
            self.paragraph.content.push(Inline::Run(Run {
                props: self.props.clone(),
                content,
            }));
        }
    }

    /// Свойства следующего текста.
    fn set_props(&mut self, props: &RunProps) {
        if *props != self.props {
            self.end_run();
            self.props = props.clone();
        }
    }

    /// Куда идут абзацы: в ячейку самой вложенной таблицы или в текст.
    fn target(&mut self) -> &mut Vec<Block> {
        match self.tables.last_mut() {
            Some(table) => &mut table.blocks,
            None => &mut self.blocks,
        }
    }

    /// Закрывает или открывает таблицы до глубины `depth`.
    fn set_depth(&mut self, depth: usize) {
        while self.tables.len() > depth {
            let Some(mut build) = self.tables.pop() else {
                break;
            };
            // Недописанная строка (файл оборван) — без описания.
            if !build.blocks.is_empty() {
                build.cells.push(std::mem::take(&mut build.blocks));
            }
            if !build.cells.is_empty() {
                build.rows.push((build.cells, RowDef::default()));
            }
            if !build.rows.is_empty() {
                let table = table(build.rows);
                self.target().push(table.into());
            }
        }
        while self.tables.len() < depth {
            self.tables.push(TableBuild::default());
        }
    }

    /// Закрывает абзац. `unit` — знак его конца (13 или 7 — конец ячейки).
    fn end_paragraph(&mut self, end: ParagraphEnd, unit: u16) {
        self.end_run();
        let mut paragraph = std::mem::take(&mut self.paragraph);
        let place = end.place;
        self.set_depth(place.depth);
        let row_end = place.depth > 0 && (place.inner_row_end || (place.row_end && unit == 7));
        let cell_end = place.depth > 0 && !row_end && (unit == 7 || place.inner_cell_end);
        if let (true, Some(build)) = (row_end, self.tables.last_mut()) {
            let cells = std::mem::take(&mut build.cells);
            build.rows.push((cells, end.row.unwrap_or_default()));
            return;
        }
        paragraph.props = end.props;
        self.target().push(paragraph.into());
        if let (true, Some(build)) = (cell_end, self.tables.last_mut()) {
            let blocks = std::mem::take(&mut build.blocks);
            build.cells.push(blocks);
        }
    }

    /// Закрывает всё и отдаёт блоки (для колонтитулов).
    fn finish(mut self) -> Vec<Block> {
        if !self.pending.is_empty() || !self.text.is_empty() {
            self.end_paragraph(ParagraphEnd::plain(), 13);
        }
        self.set_depth(0);
        self.blocks
    }

    fn end_section(&mut self) {
        self.set_depth(0);
        let mut blocks = std::mem::take(&mut self.blocks);
        if blocks.is_empty() {
            blocks.push(Paragraph::default().into());
        }
        self.sections.push(Section {
            props: Default::default(),
            blocks,
        });
    }

    fn field(&mut self, kind: FieldCharKind) {
        self.flush_text();
        match kind {
            FieldCharKind::Begin => {
                self.pending
                    .push(RunContent::FieldChar(FieldChar::new(kind)));
                self.fields.push(true);
            }
            FieldCharKind::Separate => {
                if let Some(code) = self.fields.last_mut() {
                    *code = false;
                    self.pending
                        .push(RunContent::FieldChar(FieldChar::new(kind)));
                }
            }
            FieldCharKind::End => {
                if self.fields.pop().is_some() {
                    self.pending
                        .push(RunContent::FieldChar(FieldChar::new(kind)));
                }
            }
        }
    }
}

/// Оформление по местам в потоке: отрезки и разобранные свойства.
struct Formatting<'a> {
    chpx: Vec<Span<'a>>,
    papx: Vec<Span<'a>>,
    names: Names<'a>,
    /// Разобранные свойства знаков по номеру отрезка.
    runs: HashMap<usize, RunProps>,
}

impl Formatting<'_> {
    /// Свойства знака в месте `fc`.
    fn run(&mut self, fc: u32) -> RunProps {
        let i = self.chpx.partition_point(|s| s.fc_end <= fc);
        if self.chpx.get(i).is_none_or(|s| s.fc_start > fc) {
            return RunProps::default();
        }
        if let Some(props) = self.runs.get(&i) {
            return props.clone();
        }
        let mut props = RunProps::default();
        apply_run(&mut props, &sprms(self.chpx[i].grpprl), &self.names);
        self.runs.insert(i, props.clone());
        props
    }

    /// Свойства абзаца, чей знак конца — в месте `fc`.
    fn paragraph(&self, fc: u32) -> ParagraphEnd {
        let mut end = ParagraphEnd::plain();
        if let Some(span) = find(&self.papx, fc) {
            let list = sprms(span.grpprl);
            end.props.style = self.names.style(span.istd).filter(|id| id != "Normal");
            apply_paragraph(&mut end.props, &list);
            end.place = place(&list);
            if end.place.row_end || end.place.inner_row_end {
                end.row = Some(row_def(&list, &self.names));
            }
        }
        end
    }
}

impl ParagraphEnd {
    /// Обычный абзац без свойств.
    fn plain() -> ParagraphEnd {
        ParagraphEnd {
            props: ParagraphProps::default(),
            place: Place::default(),
            row: None,
        }
    }
}

/// Переводит знаки текста в абзацы. `ends` — концы разделов (CP после
/// последнего знака).
fn feed(
    builder: &mut Builder,
    units: &[(u32, u32, u16)],
    formatting: &mut Formatting,
    ends: &[u32],
) {
    let mut i = 0;
    while i < units.len() {
        let (cp, fc, unit) = units[i];
        i += 1;
        let props = formatting.run(fc);
        builder.set_props(&props);
        match unit {
            0x0D | 0x07 => builder.end_paragraph(formatting.paragraph(fc), unit),
            0x0C if ends.contains(&(cp + 1)) => {
                builder.end_paragraph(formatting.paragraph(fc), 13);
                builder.end_section();
            }
            0x0C => builder.push(RunContent::Break(BreakKind::Page)),
            0x0E => builder.push(RunContent::Break(BreakKind::Column)),
            0x0B => builder.push(RunContent::Break(BreakKind::Line)),
            0x09 => builder.push(RunContent::Tab),
            0x13 => builder.field(FieldCharKind::Begin),
            0x14 => builder.field(FieldCharKind::Separate),
            0x15 => builder.field(FieldCharKind::End),
            0x1E => builder.text.push('\u{2011}'),
            0x1F => builder.text.push('\u{AD}'),
            // Значки рисунков, сносок, примечаний и прочие служебные знаки.
            0x00..=0x1F => {}
            // Знак вне основной плоскости — пара суррогатов.
            0xD800..=0xDBFF => {
                let low = units.get(i).map(|u| u.2);
                if let Some(low @ 0xDC00..=0xDFFF) = low {
                    i += 1;
                    let code =
                        0x10000 + ((u32::from(unit) - 0xD800) << 10) + (u32::from(low) - 0xDC00);
                    builder.text.extend(char::from_u32(code));
                }
            }
            _ => builder.text.extend(char::from_u32(u32::from(unit))),
        }
    }
}

/// Колонтитулы разделов из `PlcfHdd`. Первые 6 частей — разделители
/// сносок, дальше по 6 на раздел: чётный и нечётный верхние, чётный и
/// нечётный нижние, верхний и нижний первой страницы. Пустая часть —
/// колонтитул как у предыдущего раздела.
fn headers(
    sections: &mut [Section],
    plc: &[u8],
    from: u32,
    pieces: &[Piece],
    word: &[u8],
    formatting: &mut Formatting,
) {
    let cps: Vec<u32> = (0..plc.len() / 4)
        .filter_map(|i| u32_at(plc, 4 * i))
        .collect();
    for (index, section) in sections.iter_mut().enumerate() {
        for (j, (footer, kind)) in [
            (false, HeaderFooterKind::Even),
            (false, HeaderFooterKind::Default),
            (true, HeaderFooterKind::Even),
            (true, HeaderFooterKind::Default),
            (false, HeaderFooterKind::First),
            (true, HeaderFooterKind::First),
        ]
        .into_iter()
        .enumerate()
        {
            let story = 6 + 6 * index + j;
            let (Some(&start), Some(&end)) = (cps.get(story), cps.get(story + 1)) else {
                return;
            };
            if end <= start {
                continue;
            }
            let units = text(pieces, word, from + start, from + end);
            let mut builder = Builder::new();
            feed(&mut builder, &units, formatting, &[]);
            let mut blocks = builder.finish();
            // Последний абзац части — служебный, не показывается.
            if blocks.len() > 1
                && matches!(blocks.last(), Some(Block::Paragraph(p)) if p.content.is_empty())
            {
                blocks.pop();
            }
            let place = if footer {
                &mut section.props.footers
            } else {
                &mut section.props.headers
            };
            *place.get_mut(kind) = Some(HeaderFooter {
                blocks,
                relationship: None,
            });
        }
    }
}

/// Собирает документ.
pub(crate) fn build(fib: &Fib, word: &[u8], table: &[u8]) -> Result<Document, ReadError> {
    let pieces = pieces(fib.table(table, Table::Clx))?;
    let sections_info = seds(fib.table(table, Table::PlcfSed));
    let ends: Vec<u32> = sections_info.iter().map(|s| s.end).collect();
    let fonts = fonts(fib.table(table, Table::SttbfFfn));
    let sheet = style_sheet(fib.table(table, Table::Stshf), &fonts);
    let mut formatting = Formatting {
        chpx: chpx(fib.table(table, Table::PlcfBteChpx), word),
        papx: papx(fib.table(table, Table::PlcfBtePapx), word),
        names: Names {
            fonts: &fonts,
            styles: &sheet.ids,
        },
        runs: HashMap::new(),
    };
    let units = text(&pieces, word, 0, fib.ccp_text);
    let mut builder = Builder::new();
    feed(&mut builder, &units, &mut formatting, &ends);
    // Последний знак текста — конец абзаца; если нет, абзац всё равно
    // закрывается.
    if !builder.pending.is_empty() || !builder.text.is_empty() {
        builder.end_paragraph(ParagraphEnd::plain(), 13);
    }
    builder.end_section();
    let mut sections = builder.sections;
    for (section, sed) in sections.iter_mut().zip(&sections_info) {
        section.props = section_props(word, sed.sepx);
    }
    headers(
        &mut sections,
        fib.table(table, Table::PlcfHdd),
        fib.ccp_text + fib.ccp_ftn,
        &pieces,
        word,
        &mut formatting,
    );
    let mut doc = Document::new();
    doc.sections = sections;
    doc.styles = sheet.styles;
    // Параметры документа: чётные и нечётные колонтитулы, шаг табуляции.
    let dop = fib.table(table, Table::Dop);
    if let Some(flags) = dop.first() {
        doc.settings.even_and_odd_headers = flags & 1 != 0;
    }
    if let Some(tab) = u16_at(dop, 10).filter(|&t| t > 0) {
        doc.settings.default_tab_stop = Twips(i32::from(tab));
    }
    Ok(doc)
}
