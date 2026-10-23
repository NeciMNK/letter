//! Сборка модели документа из потоков файла .doc.

use std::io::{Cursor, Read};

use letter_model::{
    Block, BreakKind, CellProps, Columns, Document, FieldChar, FieldCharKind, HalfPoints, Inline,
    PageMargins, PageSize, Paragraph, ParagraphProps, Run, RunContent, RunFonts, RunProps, Section,
    SectionProps, Style, StyleKind, Table, TableCell, TableRow, TableWidth, Twips,
};

use crate::ReadError;
use crate::fib::{Fib, index, plc, u16_at, u32_at};
use crate::sprm::{Sep, TableMarks, apply_chp, apply_pap, apply_sep, sprms};
use crate::styles::{ISTD_NIL, StyleSheet, builtin_name, fonts, style_id};

/// Самый большой поток, который мы согласны прочитать (512 МБ).
const MAX_STREAM_SIZE: u64 = 512 * 1024 * 1024;

/// Размер страницы оформления (`FKP`) в потоке `WordDocument`.
const PAGE: usize = 512;

/// Номер «стиля по умолчанию» для знаков (`Default Paragraph Font`).
const STI_DEFAULT_FONT: u16 = 65;

/// Открывает составной файл.
pub(crate) fn open(data: &[u8]) -> std::io::Result<cfb::CompoundFile<Cursor<&[u8]>>> {
    cfb::CompoundFile::open(Cursor::new(data))
}

fn stream(file: &mut cfb::CompoundFile<Cursor<&[u8]>>, name: &str) -> Result<Vec<u8>, ReadError> {
    let stream = file.open_stream(name).map_err(|_| ReadError::NotADoc)?;
    let mut bytes = Vec::new();
    stream.take(MAX_STREAM_SIZE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_STREAM_SIZE {
        return Err(ReadError::Corrupt(format!("поток {name} слишком большой")));
    }
    Ok(bytes)
}

/// Кусок текста из таблицы кусков (`Pcd`).
#[derive(Debug, Clone, Copy)]
struct Piece {
    /// Позиции знаков (`CP`) куска: от и до.
    cp_start: u32,
    cp_end: u32,
    /// Смещение текста в потоке `WordDocument`.
    fc: u32,
    /// Один байт на знак (Windows-1252), иначе UTF-16.
    compressed: bool,
    /// Номер добавочного оформления (`Prc`), если оно есть.
    prc: Option<usize>,
}

/// Знак основного текста: код UTF-16, смещение в потоке и кусок.
#[derive(Debug, Clone, Copy)]
struct Char {
    code: u16,
    fc: u32,
    piece: usize,
}

/// Оформление знаков или абзацев на отрезке смещений потока.
#[derive(Debug, Clone)]
struct Range {
    start: u32,
    end: u32,
    /// Для абзаца — номер стиля.
    istd: u16,
    grpprl: Vec<u8>,
}

pub(crate) fn read(data: &[u8]) -> Result<Document, ReadError> {
    let mut file = open(data).map_err(|_| ReadError::NotADoc)?;
    let word = stream(&mut file, "/WordDocument")?;
    let fib = Fib::read(&word)?;
    let table = stream(&mut file, if fib.table_1 { "/1Table" } else { "/0Table" })?;

    let fonts = fib
        .part(&table, index::STTBF_FFN)
        .map(fonts)
        .unwrap_or_default();
    let sheet = fib
        .part(&table, index::STSHF)
        .map(StyleSheet::read)
        .unwrap_or_default();
    let clx = fib
        .part(&table, index::CLX)
        .ok_or_else(|| ReadError::Corrupt("нет таблицы кусков текста".into()))?;
    let (pieces, prcs) = read_clx(clx)?;
    let chars = read_chars(&word, &pieces, fib.ccp_text);
    let chpx = fib
        .part(&table, index::PLCF_BTE_CHPX)
        .map(|plc| read_fkps(&word, plc, false))
        .unwrap_or_default();
    let papx = fib
        .part(&table, index::PLCF_BTE_PAPX)
        .map(|plc| read_fkps(&word, plc, true))
        .unwrap_or_default();
    let sections = fib
        .part(&table, index::PLCF_SED)
        .map(|plc| read_sections(&word, plc))
        .unwrap_or_default();

    let ids = style_ids(&sheet);
    let mut doc = Document::new();
    fill_styles(&mut doc, &sheet, &ids, &fonts);
    let builder = Builder {
        chars: &chars,
        pieces: &pieces,
        prcs: &prcs,
        chpx: &chpx,
        papx: &papx,
        sections: &sections,
        fonts: &fonts,
        ids: &ids,
    };
    doc.sections = builder.build();
    Ok(doc)
}

/// Таблица кусков (`Clx`): добавочное оформление (`Prc`) и куски
/// (`Pcdt`).
fn read_clx(clx: &[u8]) -> Result<(Vec<Piece>, Vec<Vec<u8>>), ReadError> {
    let corrupt = || ReadError::Corrupt("испорчена таблица кусков текста".into());
    let mut prcs = Vec::new();
    let mut at = 0;
    while clx.get(at) == Some(&1) {
        let cb = usize::from(u16_at(clx, at + 1).ok_or_else(corrupt)?);
        prcs.push(clx.get(at + 3..at + 3 + cb).ok_or_else(corrupt)?.to_vec());
        at += 3 + cb;
    }
    if clx.get(at) != Some(&2) {
        return Err(corrupt());
    }
    let lcb = u32_at(clx, at + 1).ok_or_else(corrupt)? as usize;
    let plc_bytes = clx.get(at + 5..at + 5 + lcb).ok_or_else(corrupt)?;
    let (cps, pcds) = plc(plc_bytes, 8).ok_or_else(corrupt)?;
    let mut pieces = Vec::new();
    for (i, pcd) in pcds.iter().enumerate() {
        let fc = u32_at(pcd, 2).ok_or_else(corrupt)?;
        let prm = u16_at(pcd, 6).ok_or_else(corrupt)?;
        let compressed = fc & 0x4000_0000 != 0;
        pieces.push(Piece {
            cp_start: cps[i],
            cp_end: cps[i + 1],
            fc: if compressed {
                (fc & !0x4000_0000) / 2
            } else {
                fc
            },
            compressed,
            prc: (prm & 1 == 1).then_some(usize::from(prm >> 1)),
        });
    }
    Ok((pieces, prcs))
}

/// Знаки основного текста (первые `ccp_text`).
fn read_chars(word: &[u8], pieces: &[Piece], ccp_text: u32) -> Vec<Char> {
    let mut chars = Vec::new();
    for (index, piece) in pieces.iter().enumerate() {
        if piece.cp_start >= ccp_text {
            break;
        }
        let count = piece.cp_end.min(ccp_text).saturating_sub(piece.cp_start);
        for i in 0..count {
            let (code, fc) = if piece.compressed {
                let fc = piece.fc + i;
                let Some(&byte) = word.get(fc as usize) else {
                    break;
                };
                (cp1252(byte), fc)
            } else {
                let fc = piece.fc + i * 2;
                let Some(code) = u16_at(word, fc as usize) else {
                    break;
                };
                (code, fc)
            };
            chars.push(Char {
                code,
                fc,
                piece: index,
            });
        }
    }
    chars
}

/// Знак Windows-1252 в UTF-16.
fn cp1252(byte: u8) -> u16 {
    let bytes = [byte];
    let (text, _) = encoding_rs::WINDOWS_1252.decode_without_bom_handling(&bytes);
    text.encode_utf16().next().unwrap_or(u16::from(byte))
}

/// Страницы оформления (`FKP`) знаков или абзацев по их таблице
/// (`PlcBteChpx`/`PlcBtePapx`). Отрезки упорядочены по смещению.
fn read_fkps(word: &[u8], plc_bytes: &[u8], paragraphs: bool) -> Vec<Range> {
    let Some((_, pages)) = plc(plc_bytes, 4) else {
        return Vec::new();
    };
    let mut ranges = Vec::new();
    for page in pages {
        let pn = u32_at(page, 0).unwrap_or(0) & 0x003F_FFFF;
        let start = pn as usize * PAGE;
        let Some(fkp) = word.get(start..start + PAGE) else {
            continue;
        };
        let count = usize::from(fkp[PAGE - 1]);
        let entry_size = if paragraphs { 13 } else { 1 };
        let entries = 4 * (count + 1);
        for i in 0..count {
            let (Some(fc_start), Some(fc_end), Some(&offset)) = (
                u32_at(fkp, i * 4),
                u32_at(fkp, (i + 1) * 4),
                fkp.get(entries + i * entry_size),
            ) else {
                break;
            };
            let at = usize::from(offset) * 2;
            let (istd, grpprl) = if offset == 0 {
                (0, Vec::new())
            } else if paragraphs {
                read_papx(fkp, at)
            } else {
                let cb = usize::from(*fkp.get(at).unwrap_or(&0));
                (0, fkp.get(at + 1..at + 1 + cb).unwrap_or_default().to_vec())
            };
            ranges.push(Range {
                start: fc_start,
                end: fc_end,
                istd,
                grpprl,
            });
        }
    }
    ranges.sort_by_key(|r| r.start);
    ranges
}

/// Оформление абзаца в странице: номер стиля и свойства.
fn read_papx(fkp: &[u8], at: usize) -> (u16, Vec<u8>) {
    let cb = usize::from(*fkp.get(at).unwrap_or(&0));
    let (start, len) = if cb == 0 {
        let cb = usize::from(*fkp.get(at + 1).unwrap_or(&0));
        (at + 2, cb * 2)
    } else {
        (at + 1, (cb * 2).saturating_sub(1))
    };
    let Some(data) = fkp.get(start..start + len) else {
        return (0, Vec::new());
    };
    (
        u16_at(data, 0).unwrap_or(0),
        data.get(2..).unwrap_or_default().to_vec(),
    )
}

/// Отрезок, в который попадает смещение `fc`.
fn find(ranges: &[Range], fc: u32) -> Option<&Range> {
    let i = ranges.partition_point(|r| r.start <= fc);
    ranges[..i].last().filter(|r| fc < r.end)
}

/// Разделы: последний знак каждого и его свойства.
fn read_sections(word: &[u8], plc_bytes: &[u8]) -> Vec<(u32, Sep)> {
    let Some((cps, seds)) = plc(plc_bytes, 12) else {
        return Vec::new();
    };
    seds.iter()
        .enumerate()
        .map(|(i, sed)| {
            let mut sep = Sep::default();
            if let Some(fc) = u32_at(sed, 2).filter(|&fc| fc != u32::MAX)
                && let Some(cb) = u16_at(word, fc as usize)
            {
                let start = fc as usize + 2;
                if let Some(grpprl) = word.get(start..start + usize::from(cb)) {
                    for (sprm, op) in sprms(grpprl) {
                        apply_sep(&mut sep, sprm, op);
                    }
                }
            }
            (cps[i + 1], sep)
        })
        .collect()
}

/// Идентификаторы стилей по номерам. Повторы получают номер в конце.
fn style_ids(sheet: &StyleSheet) -> Vec<Option<String>> {
    let mut ids: Vec<Option<String>> = Vec::new();
    for entry in &sheet.styles {
        let id = entry.as_ref().filter(|e| e.kind.is_some()).map(|entry| {
            let name = builtin_name(entry.sti).unwrap_or(&entry.name);
            let base = style_id(name);
            let mut id = base.clone();
            let mut n = 1;
            while ids
                .iter()
                .flatten()
                .any(|known| known.eq_ignore_ascii_case(&id))
            {
                n += 1;
                id = format!("{base}{n}");
            }
            id
        });
        ids.push(id);
    }
    ids
}

/// Стиль знаков для фрагмента: «Шрифт абзаца по умолчанию» — это
/// отсутствие стиля.
fn char_style(sheet_ids: &[Option<String>], istd: u16) -> Option<String> {
    sheet_ids.get(usize::from(istd))?.clone()
}

/// Стили и оформление по умолчанию.
fn fill_styles(doc: &mut Document, sheet: &StyleSheet, ids: &[Option<String>], fonts: &[String]) {
    let font = |i: u16| fonts.get(usize::from(i)).cloned();
    let defaults = &mut doc.styles.defaults;
    defaults.run = RunProps {
        fonts: RunFonts {
            ascii: font(sheet.default_fonts[0]),
            east_asia: font(sheet.default_fonts[1]),
            h_ansi: font(sheet.default_fonts[2]),
            complex_script: font(sheet.default_fonts[2]),
            ..RunFonts::default()
        },
        // В .doc размер по умолчанию — 10 пт.
        size: Some(HalfPoints(20)),
        ..RunProps::default()
    };
    defaults.paragraph = ParagraphProps::default();
    doc.styles.styles.clear();

    let style_of = |istd: u16| -> Option<String> {
        let entry = sheet.get(istd)?;
        if entry.sti == STI_DEFAULT_FONT {
            return None;
        }
        char_style(ids, istd)
    };
    for (istd, entry) in sheet.styles.iter().enumerate() {
        let (Some(entry), Some(id)) = (entry, ids.get(istd).cloned().flatten()) else {
            continue;
        };
        let Some(kind) = entry
            .kind
            .filter(|k| matches!(k, StyleKind::Paragraph | StyleKind::Character))
        else {
            continue;
        };
        let mut style = Style::new(id, kind);
        style.name = Some(builtin_name(entry.sti).unwrap_or(&entry.name).to_string());
        style.is_default = (kind == StyleKind::Paragraph && istd == 0)
            || (kind == StyleKind::Character && entry.sti == STI_DEFAULT_FONT);
        style.custom = entry.sti == 0x0FFE;
        let link = |i: u16| {
            (i != ISTD_NIL && usize::from(i) != istd)
                .then(|| char_style(ids, i))
                .flatten()
        };
        style.based_on = link(entry.based_on);
        style.next = link(entry.next);
        let mut marks = TableMarks::default();
        for (sprm, op) in sprms(&entry.papx) {
            apply_pap(&mut style.paragraph, &mut marks, sprm, op);
        }
        for (sprm, op) in sprms(&entry.chpx) {
            apply_chp(&mut style.run, sprm, op, fonts, &style_of);
        }
        style.run.style = None;
        doc.styles.styles.push(style);
    }
}

/// Сборщик разделов из знаков и оформления.
struct Builder<'a> {
    chars: &'a [Char],
    pieces: &'a [Piece],
    prcs: &'a [Vec<u8>],
    chpx: &'a [Range],
    papx: &'a [Range],
    sections: &'a [(u32, Sep)],
    fonts: &'a [String],
    ids: &'a [Option<String>],
}

/// Таблица, которая сейчас собирается.
#[derive(Default)]
struct TableState {
    rows: Vec<(Vec<TableCell>, Vec<i16>)>,
    cells: Vec<TableCell>,
    blocks: Vec<Block>,
}

impl TableState {
    fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.cells.is_empty() && self.blocks.is_empty()
    }
}

impl Builder<'_> {
    fn build(&self) -> Vec<Section> {
        let mut sections = Vec::new();
        let mut blocks: Vec<Block> = Vec::new();
        let mut table = TableState::default();
        let mut runs: Vec<Run> = Vec::new();
        let mut run_key = None;
        // Вложенные поля: `true` — сейчас код поля.
        let mut fields: Vec<bool> = Vec::new();
        let mut section = 0;

        for (cp, ch) in self.chars.iter().enumerate() {
            let cp = cp as u32;
            let key = (find(self.chpx, ch.fc).map(|r| r.start), ch.piece);
            if run_key != Some(key) || runs.is_empty() {
                run_key = Some(key);
                runs.push(Run {
                    props: self.run_props(ch),
                    content: Vec::new(),
                });
            }
            let in_code = fields.last() == Some(&true);
            let content = match ch.code {
                0x0D | 0x07 => None,
                0x0C if self.section_ends_at(cp) => None,
                0x13 => {
                    fields.push(true);
                    Some(RunContent::FieldChar(FieldChar::new(FieldCharKind::Begin)))
                }
                0x14 => {
                    if let Some(last) = fields.last_mut() {
                        *last = false;
                    }
                    Some(RunContent::FieldChar(FieldChar::new(
                        FieldCharKind::Separate,
                    )))
                }
                0x15 => {
                    fields.pop();
                    Some(RunContent::FieldChar(FieldChar::new(FieldCharKind::End)))
                }
                0x0C => Some(RunContent::Break(BreakKind::Page)),
                0x0B => Some(RunContent::Break(BreakKind::Line)),
                0x0E => Some(RunContent::Break(BreakKind::Column)),
                0x09 if !in_code => Some(RunContent::Tab),
                0x1E => Some(text("\u{2011}")),
                0x1F => Some(text("\u{ad}")),
                // Картинки, сноски и прочие особые знаки — позже.
                code if code < 0x20 && code != 0x09 => continue,
                code => {
                    let s = String::from_utf16_lossy(&[code]);
                    Some(if in_code {
                        RunContent::FieldCode(s)
                    } else {
                        text(&s)
                    })
                }
            };
            if let Some(content) = content {
                if let Some(run) = runs.last_mut() {
                    push_content(&mut run.content, content);
                }
                continue;
            }

            // Конец абзаца, ячейки, строки таблицы или раздела.
            let (props, marks) = self.paragraph_props(ch);
            let paragraph = Paragraph {
                props,
                content: std::mem::take(&mut runs)
                    .into_iter()
                    .filter(|r| !r.content.is_empty())
                    .map(Inline::Run)
                    .collect(),
            };
            run_key = None;
            if ch.code == 0x07 && marks.row_end {
                if !table.blocks.is_empty() {
                    let blocks = std::mem::take(&mut table.blocks);
                    table.cells.push(cell(blocks));
                }
                let cells = std::mem::take(&mut table.cells);
                table.rows.push((cells, marks.cell_edges));
            } else if marks.in_table || ch.code == 0x07 {
                table.blocks.push(paragraph.into());
                if ch.code == 0x07 {
                    let blocks = std::mem::take(&mut table.blocks);
                    table.cells.push(cell(blocks));
                }
            } else {
                flush_table(&mut table, &mut blocks);
                blocks.push(paragraph.into());
            }
            if ch.code == 0x0C && self.section_ends_at(cp) {
                flush_table(&mut table, &mut blocks);
                sections.push(self.section(section, std::mem::take(&mut blocks)));
                section += 1;
            }
        }
        if runs.iter().any(|r| !r.content.is_empty()) {
            blocks.push(
                Paragraph {
                    props: ParagraphProps::default(),
                    content: runs.into_iter().map(Inline::Run).collect(),
                }
                .into(),
            );
        }
        flush_table(&mut table, &mut blocks);
        if !blocks.is_empty() || sections.is_empty() {
            sections.push(self.section(section, blocks));
        }
        sections
    }

    /// Заканчивается ли раздел знаком `cp`.
    fn section_ends_at(&self, cp: u32) -> bool {
        self.sections.iter().any(|(end, _)| *end == cp + 1)
    }

    fn section(&self, index: usize, mut blocks: Vec<Block>) -> Section {
        if blocks.is_empty() {
            blocks.push(Paragraph::default().into());
        }
        let sep = self
            .sections
            .get(index)
            .or(self.sections.last())
            .map(|(_, sep)| *sep)
            .unwrap_or_default();
        Section {
            props: section_props(&sep),
            blocks,
        }
    }

    /// Добавочное оформление куска (после «быстрого сохранения»).
    fn prc(&self, ch: &Char) -> &[u8] {
        self.pieces
            .get(ch.piece)
            .and_then(|p| p.prc)
            .and_then(|i| self.prcs.get(i))
            .map_or(&[], Vec::as_slice)
    }

    fn run_props(&self, ch: &Char) -> RunProps {
        let mut props = RunProps::default();
        let ids = self.ids;
        let style_of = |istd: u16| char_style(ids, istd);
        let direct = find(self.chpx, ch.fc).map_or(&[][..], |r| r.grpprl.as_slice());
        for grpprl in [direct, self.prc(ch)] {
            for (sprm, op) in sprms(grpprl) {
                apply_chp(&mut props, sprm, op, self.fonts, &style_of);
            }
        }
        // «Шрифт абзаца по умолчанию» — то же, что без стиля.
        if props.style.as_deref() == Some("DefaultParagraphFont") {
            props.style = None;
        }
        props
    }

    fn paragraph_props(&self, ch: &Char) -> (ParagraphProps, TableMarks) {
        let mut props = ParagraphProps::default();
        let mut marks = TableMarks::default();
        let range = find(self.papx, ch.fc);
        if let Some(range) = range {
            if range.istd != 0 {
                props.style = char_style(self.ids, range.istd);
            }
            for (sprm, op) in sprms(&range.grpprl) {
                apply_pap(&mut props, &mut marks, sprm, op);
            }
        }
        for (sprm, op) in sprms(self.prc(ch)) {
            apply_pap(&mut props, &mut marks, sprm, op);
        }
        (props, marks)
    }
}

fn text(s: &str) -> RunContent {
    RunContent::Text(s.to_string())
}

/// Добавляет содержимое во фрагмент, склеивая текст с текстом.
fn push_content(content: &mut Vec<RunContent>, item: RunContent) {
    match (content.last_mut(), item) {
        (Some(RunContent::Text(last)), RunContent::Text(more)) => last.push_str(&more),
        (Some(RunContent::FieldCode(last)), RunContent::FieldCode(more)) => last.push_str(&more),
        (_, item) => content.push(item),
    }
}

fn cell(mut blocks: Vec<Block>) -> TableCell {
    if blocks.is_empty() {
        blocks.push(Paragraph::default().into());
    }
    TableCell {
        props: CellProps::default(),
        raw_props: Vec::new(),
        blocks,
    }
}

/// Дописывает собранную таблицу в блоки. Сетка столбцов — все границы
/// ячеек всех строк; ячейка занимает столбцы между своими границами.
fn flush_table(state: &mut TableState, blocks: &mut Vec<Block>) {
    if state.is_empty() {
        return;
    }
    if !state.blocks.is_empty() {
        let cell_blocks = std::mem::take(&mut state.blocks);
        state.cells.push(cell(cell_blocks));
    }
    if !state.cells.is_empty() {
        let cells = std::mem::take(&mut state.cells);
        state.rows.push((cells, Vec::new()));
    }
    let mut edges: Vec<i16> = state
        .rows
        .iter()
        .flat_map(|(_, e)| e.iter().copied())
        .collect();
    edges.sort_unstable();
    edges.dedup();
    let mut table = Table {
        grid: edges
            .windows(2)
            .map(|w| Twips(i32::from(w[1]) - i32::from(w[0])))
            .collect(),
        ..Table::default()
    };
    for (cells, row_edges) in std::mem::take(&mut state.rows) {
        let mut cells = cells;
        for (i, cell) in cells.iter_mut().enumerate() {
            let (Some(&left), Some(&right)) = (row_edges.get(i), row_edges.get(i + 1)) else {
                continue;
            };
            cell.props.width = Some(TableWidth::Twips(Twips(i32::from(right) - i32::from(left))));
            let span = edges.iter().filter(|&&e| e > left && e <= right).count();
            if span > 1 {
                cell.props.grid_span = Some(span as u32);
            }
        }
        table.rows.push(TableRow {
            props: Default::default(),
            raw_props: Vec::new(),
            cells,
        });
    }
    blocks.push(table.into());
}

fn section_props(sep: &Sep) -> SectionProps {
    SectionProps {
        page_size: Some(PageSize {
            width: Twips(sep.width),
            height: Twips(sep.height),
            orientation: sep.orientation(),
        }),
        margins: Some(PageMargins {
            top: Twips(sep.top),
            right: Twips(sep.right),
            bottom: Twips(sep.bottom),
            left: Twips(sep.left),
            header: Twips(sep.header),
            footer: Twips(sep.footer),
            gutter: Twips(sep.gutter),
        }),
        start: Some(sep.start),
        columns: Some(Columns {
            count: sep.columns,
            space: Twips(sep.column_space),
            separator: sep.column_line,
            widths: Vec::new(),
        }),
        title_page: sep.title_page,
        ..SectionProps::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_are_found_by_offset() {
        let range = |start, end| Range {
            start,
            end,
            istd: 0,
            grpprl: Vec::new(),
        };
        let ranges = [range(10, 20), range(20, 30), range(40, 50)];
        assert_eq!(find(&ranges, 10).map(|r| r.start), Some(10));
        assert_eq!(find(&ranges, 29).map(|r| r.start), Some(20));
        assert!(find(&ranges, 35).is_none());
        assert!(find(&ranges, 5).is_none());
    }

    #[test]
    fn windows_1252_letters() {
        assert_eq!(cp1252(b'A'), u16::from(b'A'));
        assert_eq!(cp1252(0x93), 0x201C);
        assert_eq!(cp1252(0xE9), 0xE9);
    }
}
