//! Запись документа в формат .doc (Word 97–2003, MS-DOC).
//!
//! Файл собирается так, как его читает [`crate::reader`]: весь текст —
//! один кусок в UTF-16 в потоке `WordDocument`, за ним страницы
//! оформления знаков и абзацев (`FKP`), в потоке `1Table` — стили,
//! шрифты, таблица кусков, разделы и свойства документа.
//!
//! Пишется: текст, оформление знаков и абзацев, стили, разделы
//! (лист, поля, колонки, начало), разрывы, таблицы (ширины, объединение
//! ячеек, границы, заливка, высота строк), поля, гиперссылки, закладки.

use std::collections::HashMap;
use std::io::Write as _;

use md5::Digest as _;

use letter_model::{
    Alignment, Block, Border, BreakKind, CellVerticalAlign, Color, Document, FieldCharKind,
    FirstLineIndent, Highlight, Hyperlink, Inline, LevelSuffix, LineSpacing, ListLevel, NoteKind,
    NumberFormat, Orientation, Paragraph, ParagraphBorders, ParagraphProps, Picture, RunContent,
    RunProps, Section, SectionStart, Shading, Style, StyleKind, TabLeader, TabStop, TabStopKind,
    Table, TableWidth, ThemeFont, Underline, VerticalAlign, VerticalMerge,
};

use crate::styles::builtin_name;

/// Почему не удалось записать файл .doc.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WriteError {
    /// Ошибка записи составного файла.
    #[error("не удалось записать файл .doc: {0}")]
    Io(#[from] std::io::Error),
    /// Документ слишком большой для формата .doc.
    #[error("документ слишком большой для формата .doc")]
    TooLarge,
}

/// Где в потоке `WordDocument` начинается текст.
const FC_TEXT: u32 = 0x0800;

/// Размер страницы оформления.
const PAGE: usize = 512;

/// Знаки, которые в .doc значат разметку.
const PARAGRAPH: u16 = 0x0D;
const CELL: u16 = 0x07;
const SECTION: u16 = 0x0C;

/// Номер встроенного стиля «Шрифт абзаца по умолчанию».
const STI_DEFAULT_FONT: u16 = 65;

/// Части текста по порядку: основной текст, сноски, колонтитулы,
/// примечания, концевые сноски (номера в [`Writer::stories`]).
const STORY_TEXT: usize = 0;
const STORY_FOOTNOTES: usize = 1;
const STORY_HEADERS: usize = 2;
const STORY_COMMENTS: usize = 3;
const STORY_ENDNOTES: usize = 4;

/// Особый знак в тексте (`sprmCFSpec`).
const SPEC: [u8; 3] = [0x55, 0x08, 1];

/// Шрифт, который стоит первым в таблице шрифтов.
const FIRST_FONT: &str = "Times New Roman";

/// Записывает документ в байты файла .doc; `template` — шаблон .dot.
pub(crate) fn write(doc: &Document, template: bool) -> Result<Vec<u8>, WriteError> {
    let mut writer = Writer::new(doc);
    for (i, section) in doc.sections.iter().enumerate() {
        writer.section(section, i + 1 == doc.sections.len());
    }
    if doc.sections.is_empty() {
        writer.mark(PARAGRAPH, 0, Vec::new(), &[]);
        writer.sections.push((writer.cp(), Vec::new()));
    }
    writer.stories[STORY_TEXT] = writer.cp();
    writer.notes(NoteKind::Footnote);
    writer.headers();
    writer.comments();
    writer.notes(NoteKind::Endnote);
    // После всех частей, кроме основного текста, — ещё знак абзаца.
    if writer.cp() > writer.stories[STORY_TEXT] {
        writer.mark(PARAGRAPH, 0, Vec::new(), &[]);
    }
    writer.finish(template)
}

/// Стиль в таблице стилей: его место (`istd`), номер встроенного стиля
/// (`sti`) и сам стиль.
struct StyleSlot<'a> {
    sti: u16,
    style: Option<&'a Style>,
    /// Стиль «Обычный»: в него же идут свойства документа по умолчанию.
    normal: bool,
    /// «Шрифт абзаца по умолчанию».
    default_font: bool,
}

struct Writer<'a> {
    doc: &'a Document,
    fonts: Vec<String>,
    /// Стили по местам (`istd`); `None` — пустое место.
    styles: Vec<Option<StyleSlot<'a>>>,
    style_index: HashMap<&'a str, u16>,
    /// Текст по знакам (`CP`).
    text: Vec<u16>,
    /// Оформление знаков: конец отрезка (`CP`) и свойства.
    chpx: Vec<(u32, Vec<u8>)>,
    /// Оформление абзацев: конец абзаца (`CP`), стиль и свойства.
    papx: Vec<(u32, u16, Vec<u8>)>,
    /// Разделы: конец (`CP`) и свойства.
    sections: Vec<(u32, Vec<u8>)>,
    /// Знаки полей: место и запись `FLD`.
    fields: Vec<(u32, [u8; 2])>,
    /// Закладки: имя, начало и конец.
    bookmarks: Vec<(String, u32, Option<u32>)>,
    bookmark_ids: HashMap<String, usize>,
    /// Длины частей текста (см. [`STORY_TEXT`] и дальше).
    stories: [u32; 5],
    /// Ссылки на сноски: место, вид, номер сноски и «свой знак».
    note_refs: Vec<(u32, NoteKind, String, bool)>,
    /// Начала сносок в их части текста (`PlcffndTxt`, `PlcfendTxt`).
    note_texts: [Vec<u32>; 2],
    /// Колонтитулы: начала частей (`PlcfHdd`).
    header_cps: Vec<u32>,
    /// Ссылки на примечания: место и номер.
    comment_refs: Vec<(u32, String)>,
    /// Отмеченный примечанием кусок: начало и конец по номеру.
    comment_ranges: HashMap<String, (Option<u32>, Option<u32>)>,
    /// Начала примечаний в их части текста (`PlcfandTxt`).
    comment_texts: Vec<u32>,
    /// Особый знак, с которого начнётся следующий абзац (знак
    /// примечания в его тексте).
    pending_mark: Option<u16>,
    /// Поток `Data`: картинки.
    data: Vec<u8>,
}

impl<'a> Writer<'a> {
    fn new(doc: &'a Document) -> Writer<'a> {
        let mut writer = Writer {
            doc,
            fonts: vec![FIRST_FONT.to_owned()],
            styles: Vec::new(),
            style_index: HashMap::new(),
            text: Vec::new(),
            chpx: Vec::new(),
            papx: Vec::new(),
            sections: Vec::new(),
            fields: Vec::new(),
            bookmarks: Vec::new(),
            bookmark_ids: HashMap::new(),
            stories: [0; 5],
            note_refs: Vec::new(),
            note_texts: [Vec::new(), Vec::new()],
            header_cps: Vec::new(),
            comment_refs: Vec::new(),
            comment_ranges: HashMap::new(),
            comment_texts: Vec::new(),
            pending_mark: None,
            data: Vec::new(),
        };
        writer.plan_styles();
        writer
    }

    fn cp(&self) -> u32 {
        self.text.len() as u32
    }

    /// Раскладывает стили по местам, как Word: 0 — «Обычный», 1–9 —
    /// заголовки, 10 — «Шрифт абзаца по умолчанию», с 15 — остальные.
    fn plan_styles(&mut self) {
        let doc = self.doc;
        let mut slots: Vec<Option<StyleSlot<'a>>> = (0..15).map(|_| None).collect();
        let normal = doc
            .styles
            .styles
            .iter()
            .find(|s| s.kind == StyleKind::Paragraph && s.is_default);
        slots[0] = Some(StyleSlot {
            sti: 0,
            style: normal,
            normal: true,
            default_font: false,
        });
        let default_font = doc
            .styles
            .styles
            .iter()
            .find(|s| s.kind == StyleKind::Character && s.is_default);
        slots[10] = Some(StyleSlot {
            sti: STI_DEFAULT_FONT,
            style: default_font,
            normal: false,
            default_font: true,
        });
        if let Some(style) = normal {
            self.style_index.insert(&style.id, 0);
        }
        if let Some(style) = default_font {
            self.style_index.insert(&style.id, 10);
        }
        for style in &doc.styles.styles {
            if self.style_index.contains_key(style.id.as_str())
                || !matches!(style.kind, StyleKind::Paragraph | StyleKind::Character)
            {
                continue;
            }
            let sti = sti_of(style);
            let istd = match sti {
                1..=9
                    if style.kind == StyleKind::Paragraph && slots[usize::from(sti)].is_none() =>
                {
                    usize::from(sti)
                }
                _ => {
                    slots.push(None);
                    slots.len() - 1
                }
            };
            slots[istd] = Some(StyleSlot {
                sti,
                style: Some(style),
                normal: false,
                default_font: false,
            });
            self.style_index.insert(&style.id, istd as u16);
        }
        self.styles = slots;
    }

    fn istd(&self, id: Option<&str>) -> Option<u16> {
        self.style_index.get(id?).copied()
    }

    fn font(&mut self, name: &str) -> u16 {
        if let Some(i) = self.fonts.iter().position(|f| f == name) {
            return i as u16;
        }
        self.fonts.push(name.to_owned());
        (self.fonts.len() - 1) as u16
    }

    /// Добавляет знаки с оформлением.
    fn push(&mut self, units: &[u16], chpx: &[u8]) {
        if units.is_empty() {
            return;
        }
        self.text.extend_from_slice(units);
        let end = self.cp();
        match self.chpx.last_mut() {
            Some((last_end, last)) if last.as_slice() == chpx => *last_end = end,
            _ => self.chpx.push((end, chpx.to_vec())),
        }
    }

    fn push_str(&mut self, text: &str, chpx: &[u8]) {
        let units: Vec<u16> = text
            .encode_utf16()
            .map(|u| match u {
                // Служебные знаки .doc в тексте не нужны.
                0x00..=0x08 | 0x0A..=0x1F => 0x20,
                u => u,
            })
            .collect();
        self.push(&units, chpx);
    }

    /// Знак конца абзаца (`mark`) и оформление абзаца.
    fn mark(&mut self, mark: u16, istd: u16, papx: Vec<u8>, chpx: &[u8]) {
        self.push(&[mark], chpx);
        self.papx.push((self.cp(), istd, papx));
    }

    /// Поле: знак начала, разделителя или конца. Возвращает номер
    /// записи поля.
    fn field_char(&mut self, kind: FieldCharKind, chpx: &[u8], has_separator: bool) -> usize {
        let (code, fld) = match kind {
            FieldCharKind::Begin => (0x13, [0x13, FLT_UNKNOWN]),
            FieldCharKind::Separate => (0x14, [0x14, 0xFF]),
            FieldCharKind::End => (0x15, [0x15, if has_separator { 0x80 } else { 0 }]),
        };
        self.fields.push((self.cp(), fld));
        self.special(code, chpx);
        self.fields.len() - 1
    }

    /// Особый знак `code` (сноска, примечание, поле) с оформлением.
    fn special(&mut self, code: u16, chpx: &[u8]) {
        let mut chpx = chpx.to_vec();
        chpx.extend(SPEC);
        self.push(&[code], &chpx);
    }

    /// Часть сносок или концевых сносок: тексты в порядке ссылок на них.
    fn notes(&mut self, kind: NoteKind) {
        let doc = self.doc;
        let start = self.cp();
        let notes = match kind {
            NoteKind::Footnote => &doc.footnotes,
            NoteKind::Endnote => &doc.endnotes,
        };
        let ids: Vec<String> = self
            .note_refs
            .iter()
            .filter(|r| r.1 == kind && r.0 < self.stories[STORY_TEXT])
            .map(|r| r.2.clone())
            .collect();
        let mut starts = Vec::new();
        for id in &ids {
            starts.push(self.cp() - start);
            match notes.iter().find(|n| &n.id == id) {
                Some(note) if !note.blocks.is_empty() => {
                    for block in &note.blocks {
                        self.block(block, PARAGRAPH, &[]);
                    }
                }
                _ => {
                    self.special(0x02, &[]);
                    self.mark(PARAGRAPH, 0, Vec::new(), &[]);
                }
            }
        }
        if starts.is_empty() {
            return;
        }
        // Последний знак абзаца части — ни к одной сноске.
        starts.push(self.cp() - start);
        self.mark(PARAGRAPH, 0, Vec::new(), &[]);
        starts.push(self.cp() - start);
        let (story, slot) = match kind {
            NoteKind::Footnote => (STORY_FOOTNOTES, 0),
            NoteKind::Endnote => (STORY_ENDNOTES, 1),
        };
        self.stories[story] = self.cp() - start;
        self.note_texts[slot] = starts;
    }

    /// Часть колонтитулов: шесть служебных частей (черты сносок; пустые
    /// — как обычно), потом по шесть на раздел: верхний чётных, верхний
    /// нечётных, нижний чётных, нижний нечётных, верхний и нижний первой
    /// страницы. Пустая часть — как в предыдущем разделе.
    fn headers(&mut self) {
        let doc = self.doc;
        let start = self.cp();
        let mut cps = vec![0; 6];
        for section in &doc.sections {
            let (h, f) = (&section.props.headers, &section.props.footers);
            for part in [&h.even, &h.default, &f.even, &f.default, &h.first, &f.first] {
                cps.push(self.cp() - start);
                let Some(part) = part else { continue };
                for block in &part.blocks {
                    self.block(block, PARAGRAPH, &[]);
                }
                if part.blocks.is_empty() {
                    self.mark(PARAGRAPH, 0, Vec::new(), &[]);
                }
            }
        }
        if self.cp() == start {
            return;
        }
        let end = self.cp() - start;
        self.mark(PARAGRAPH, 0, Vec::new(), &[]);
        // Как у Word: конец последней части и ещё одна позиция за
        // последним знаком абзаца.
        cps.extend([end, end + 2]);
        self.stories[STORY_HEADERS] = self.cp() - start;
        self.header_cps = cps;
    }

    /// Часть примечаний: тексты в порядке ссылок, каждый начинается
    /// знаком примечания.
    fn comments(&mut self) {
        let doc = self.doc;
        let start = self.cp();
        let ids: Vec<String> = self
            .comment_refs
            .iter()
            .filter(|r| r.0 < self.stories[STORY_TEXT])
            .map(|r| r.1.clone())
            .collect();
        let mut starts = Vec::new();
        for id in &ids {
            starts.push(self.cp() - start);
            let blocks = doc
                .comments
                .iter()
                .find(|c| &c.id == id)
                .map(|c| c.blocks.as_slice())
                .unwrap_or_default();
            self.pending_mark = Some(0x05);
            for block in blocks {
                self.block(block, PARAGRAPH, &[]);
            }
            if let Some(code) = self.pending_mark.take() {
                self.special(code, &[]);
                self.mark(PARAGRAPH, 0, Vec::new(), &[]);
            }
        }
        if starts.is_empty() {
            return;
        }
        starts.push(self.cp() - start);
        self.mark(PARAGRAPH, 0, Vec::new(), &[]);
        starts.push(self.cp() - start);
        self.stories[STORY_COMMENTS] = self.cp() - start;
        self.comment_texts = starts;
    }

    /// Вписывает в знак начала поля `begin` его тип — по коду поля,
    /// который записан сразу после знака.
    fn set_field_type(&mut self, begin: usize) {
        let Some(&(cp, _)) = self.fields.get(begin) else {
            return;
        };
        let start = (cp as usize + 1).min(self.text.len());
        let code = String::from_utf16_lossy(&self.text[start..]);
        if let Some(fld) = self.fields.get_mut(begin) {
            fld.1[1] = field_type(&code);
        }
    }

    fn section(&mut self, section: &Section, last: bool) {
        let blocks = &section.blocks;
        let sepx = sepx(section);
        let needs_paragraph = !matches!(blocks.last(), Some(Block::Paragraph(_)));
        for (i, block) in blocks.iter().enumerate() {
            let end = if !last && !needs_paragraph && i + 1 == blocks.len() {
                SECTION
            } else {
                PARAGRAPH
            };
            self.block(block, end, &[]);
        }
        if needs_paragraph {
            let mark = if last { PARAGRAPH } else { SECTION };
            self.mark(mark, 0, Vec::new(), &[]);
        }
        self.sections.push((self.cp(), sepx));
    }

    /// Блок; `end` — знак конца последнего абзаца, `table` — свойства
    /// «в таблице» для абзацев ячейки.
    fn block(&mut self, block: &Block, end: u16, table: &[u8]) {
        match block {
            Block::Paragraph(p) => self.paragraph(p, end, table),
            Block::Table(t) if table.is_empty() => self.table(t),
            // Таблица в таблице в .doc записывается текстом.
            Block::Table(t) => {
                let rows: Vec<&Paragraph> = t
                    .rows
                    .iter()
                    .flat_map(|r| &r.cells)
                    .flat_map(|c| &c.blocks)
                    .filter_map(|b| match b {
                        Block::Paragraph(p) => Some(p),
                        _ => None,
                    })
                    .collect();
                let count = rows.len();
                for (i, p) in rows.into_iter().enumerate() {
                    let mark = if i + 1 == count { end } else { PARAGRAPH };
                    self.paragraph(p, mark, table);
                }
                if count == 0 {
                    self.mark(end, 0, table.to_vec(), &[]);
                }
            }
            Block::Unknown(_) => self.mark(end, 0, table.to_vec(), &[]),
        }
    }

    fn paragraph(&mut self, p: &Paragraph, end: u16, table: &[u8]) {
        let istd = self.istd(p.props.style.as_deref()).unwrap_or(0);
        let mut link: Option<&Hyperlink> = None;
        // Открытые поля: номер знака начала и есть ли уже разделитель.
        let mut field_separators: Vec<(usize, bool)> = Vec::new();
        let mut mark_chpx = Vec::new();
        if let Some(code) = self.pending_mark.take() {
            self.special(code, &[]);
        }
        for inline in &p.content {
            match inline {
                Inline::Run(run) => {
                    if run.props.link.as_ref() != link {
                        if link.is_some() {
                            self.field_char(FieldCharKind::End, &[], true);
                        }
                        link = run.props.link.as_ref();
                        if let Some(target) = link {
                            let begin = self.field_char(FieldCharKind::Begin, &[], true);
                            self.push_str(&hyperlink_code(target), &[]);
                            self.set_field_type(begin);
                            self.field_char(FieldCharKind::Separate, &[], true);
                        }
                    }
                    let chpx = self.chpx(&run.props);
                    for content in &run.content {
                        self.content(content, &chpx, &mut field_separators);
                    }
                    mark_chpx = chpx;
                }
                Inline::BookmarkStart(b) => {
                    self.bookmark_ids.insert(b.id.clone(), self.bookmarks.len());
                    self.bookmarks.push((b.name.clone(), self.cp(), None));
                }
                Inline::BookmarkEnd(id) => {
                    if let Some(&i) = self.bookmark_ids.get(id) {
                        self.bookmarks[i].2 = Some(self.cp());
                    }
                }
                Inline::CommentStart(id) => {
                    let cp = self.cp();
                    self.comment_ranges.entry(id.clone()).or_default().0 = Some(cp);
                }
                Inline::CommentEnd(id) => {
                    let cp = self.cp();
                    self.comment_ranges.entry(id.clone()).or_default().1 = Some(cp);
                }
                _ => {}
            }
        }
        if link.is_some() {
            self.field_char(FieldCharKind::End, &[], true);
        }
        // Незакрытые поля закрываем, иначе Word считает файл испорченным.
        while let Some((begin, has_separator)) = field_separators.pop() {
            if !has_separator {
                self.set_field_type(begin);
            }
            self.field_char(FieldCharKind::End, &[], has_separator);
        }
        let mut papx = papx(&p.props);
        self.numbering_sprms(&p.props, &mut papx);
        papx.extend_from_slice(table);
        // Знак абзаца — с оформлением последнего фрагмента, без стиля
        // знаков и ссылки.
        self.mark(end, istd, papx, &mark_chpx);
    }

    fn content(&mut self, content: &RunContent, chpx: &[u8], fields: &mut Vec<(usize, bool)>) {
        match content {
            RunContent::Text(text) => self.push_str(text, chpx),
            RunContent::Tab => self.push(&[0x09], chpx),
            RunContent::Break(kind) => {
                let code = match kind {
                    BreakKind::Line => 0x0B,
                    BreakKind::Page => 0x0C,
                    BreakKind::Column => 0x0E,
                };
                self.push(&[code], chpx);
            }
            RunContent::FieldChar(c) => match c.kind {
                FieldCharKind::Begin => {
                    let begin = self.field_char(c.kind, chpx, false);
                    fields.push((begin, false));
                }
                FieldCharKind::Separate => {
                    if let Some((begin, separated)) = fields.last_mut()
                        && !*separated
                    {
                        *separated = true;
                        let begin = *begin;
                        self.set_field_type(begin);
                        self.field_char(c.kind, chpx, true);
                    }
                }
                FieldCharKind::End => {
                    if let Some((begin, has_separator)) = fields.pop() {
                        if !has_separator {
                            self.set_field_type(begin);
                        }
                        self.field_char(c.kind, chpx, has_separator);
                    }
                }
            },
            RunContent::FieldCode(code) => self.push_str(code, chpx),
            RunContent::NoteReference(r) => {
                let cp = self.cp();
                self.note_refs
                    .push((cp, r.kind, r.id.clone(), r.custom_mark));
                self.special(0x02, chpx);
            }
            RunContent::NoteMark(_) => self.special(0x02, chpx),
            RunContent::CommentReference(id) => {
                let cp = self.cp();
                self.comment_refs.push((cp, id.clone()));
                self.special(0x05, chpx);
            }
            RunContent::Picture(picture) => {
                if let Some(record) = picture_data(picture) {
                    let offset = self.data.len() as u32;
                    self.data.extend(record);
                    let mut chpx = chpx.to_vec();
                    push(&mut chpx, 0x6A03, &offset.to_le_bytes());
                    self.special(0x01, &chpx);
                }
            }
            RunContent::Symbol(symbol) => {
                let font = self.font(&symbol.font);
                let mut chpx = chpx.to_vec();
                for code in [0x4A4F, 0x4A51] {
                    push_u16(&mut chpx, code, font);
                }
                let unit = u16::try_from(symbol.code).unwrap_or(0x20);
                self.push(&[unit], &chpx);
            }
            _ => {}
        }
    }

    fn table(&mut self, table: &Table) {
        let cumulative = |grid: &[i32], upto: usize| grid.iter().take(upto).sum::<i32>();
        let grid: Vec<i32> = table.grid.iter().map(|t| t.0).collect();
        let in_table = [0x16, 0x24, 1];
        for row in &table.rows {
            let mut edges = vec![0i32];
            let mut column = 0usize;
            for cell in &row.cells {
                let span = cell.props.grid_span.unwrap_or(1).max(1) as usize;
                let width = match (&cell.props.width, grid.len() >= column + span) {
                    (_, true) if !grid.is_empty() => {
                        cumulative(&grid, column + span) - cumulative(&grid, column)
                    }
                    (Some(TableWidth::Twips(w)), _) => w.0,
                    _ => 1440,
                };
                column += span;
                let last = *edges.last().unwrap_or(&0);
                edges.push(last + width.max(1));
            }
            for cell in &row.cells {
                let count = cell.blocks.len();
                for (i, block) in cell.blocks.iter().enumerate() {
                    let end = if i + 1 == count { CELL } else { PARAGRAPH };
                    self.block(block, end, &in_table);
                }
                if count == 0 {
                    self.mark(CELL, 0, in_table.to_vec(), &[]);
                }
            }
            let tap = tap(table, row, &edges);
            let mut papx = vec![0x16, 0x24, 1, 0x17, 0x24, 1];
            papx.extend(tap);
            self.mark(CELL, 0, papx, &[]);
        }
    }

    /// Оформление знаков (`grpprl`).
    fn chpx(&mut self, props: &RunProps) -> Vec<u8> {
        let mut out = Vec::new();
        if let Some(istd) = self.istd(props.style.as_deref()) {
            push_u16(&mut out, 0x4A30, istd);
        }
        self.run_props(props, &mut out);
        out
    }

    fn run_props(&mut self, props: &RunProps, out: &mut Vec<u8>) {
        let fonts = &props.fonts;
        for (code, name, theme) in [
            (0x4A4F, &fonts.ascii, fonts.ascii_theme),
            (0x4A50, &fonts.east_asia, fonts.east_asia_theme),
            (0x4A51, &fonts.h_ansi, fonts.h_ansi_theme),
            (0x4A5E, &fonts.complex_script, fonts.complex_script_theme),
        ] {
            if let Some(name) = font_name(self.doc, name, theme) {
                let font = self.font(&name);
                push_u16(out, code, font);
            }
        }
        if let Some(size) = props.size {
            let size = u16::try_from(size.0).unwrap_or(u16::MAX);
            push_u16(out, 0x4A43, size);
            push_u16(out, 0x4A61, size);
        }
        for (code, value) in [
            (0x0835, props.bold),
            (0x0836, props.italic),
            (0x0837, props.strike),
            (0x2A53, props.double_strike),
            (0x083A, props.small_caps),
            (0x083B, props.caps),
            (0x083C, props.hidden),
        ] {
            if let Some(value) = value {
                push_u8(out, code, u8::from(value));
            }
        }
        if let Some(underline) = props.underline {
            push_u8(out, 0x2A3E, kul(underline));
        }
        match props.color {
            Some(Color::Auto) => {
                push_u8(out, 0x2A42, 0);
                push(out, 0x6870, &[0, 0, 0, 0xFF]);
            }
            Some(Color::Rgb(r, g, b)) => {
                push_u8(out, 0x2A42, nearest_ico(r, g, b));
                push(out, 0x6870, &[r, g, b, 0]);
            }
            None => {}
        }
        if let Some(highlight) = props.highlight {
            push_u8(out, 0x2A0C, highlight_ico(highlight));
        }
        if let Some(align) = props.vertical_align {
            let iss = match align {
                VerticalAlign::Superscript => 1,
                VerticalAlign::Subscript => 2,
                VerticalAlign::Baseline => 0,
            };
            push_u8(out, 0x2A48, iss);
        }
        if let Some(spacing) = props.spacing {
            push_i16(out, 0x8840, spacing.0);
        }
        if let Some(position) = props.position {
            push_i16(out, 0x4845, position);
        }
        if let Some(kerning) = props.kerning {
            push_u16(out, 0x484B, u16::try_from(kerning.0).unwrap_or(u16::MAX));
        }
        if let Some(scale) = props.scale {
            push_u16(out, 0x4852, scale);
        }
    }

    fn finish(mut self, template: bool) -> Result<Vec<u8>, WriteError> {
        let total = self.cp();
        if usize::try_from(total).map_or(true, |t| t > 0x0FFF_FFFF) {
            return Err(WriteError::TooLarge);
        }
        let fc = |cp: u32| FC_TEXT + cp * 2;

        // Поток `WordDocument`: заголовок, текст, разделы, страницы.
        let mut word = vec![0u8; FC_TEXT as usize];
        for unit in &self.text {
            word.extend(unit.to_le_bytes());
        }
        let mut seds = Vec::new();
        for (_, sepx) in &self.sections {
            seds.push(word.len() as u32);
            word.extend((sepx.len() as u16).to_le_bytes());
            word.extend(sepx);
        }
        pad_to_page(&mut word);
        let mut start = 0;
        let chp_runs: Vec<(u32, u32, Vec<u8>)> = self
            .chpx
            .iter()
            .map(|(end, grpprl)| {
                let run = (fc(start), fc(*end), chpx_entry(grpprl));
                start = *end;
                run
            })
            .collect();
        let (pages, chp_bte) = fkps(&chp_runs, false, (word.len() / PAGE) as u32);
        word.extend(pages);
        let mut start = 0;
        let pap_runs: Vec<(u32, u32, Vec<u8>)> = self
            .papx
            .iter()
            .map(|(end, istd, grpprl)| {
                let run = (fc(start), fc(*end), papx_entry(*istd, grpprl));
                start = *end;
                run
            })
            .collect();
        let (pages, pap_bte) = fkps(&pap_runs, true, (word.len() / PAGE) as u32);
        word.extend(pages);
        if word.len() < 4096 {
            word.resize(4096, 0);
        }

        // Поток `1Table`.
        let mut table = Vec::new();
        let mut pairs = vec![(0u32, 0u32); 93];
        let part = |table: &mut Vec<u8>, pairs: &mut [(u32, u32)], index: usize, bytes: Vec<u8>| {
            if !bytes.is_empty() {
                pairs[index] = (table.len() as u32, bytes.len() as u32);
                table.extend(bytes);
            }
        };
        let stsh = self.stsh();
        part(&mut table, &mut pairs, 1, stsh);
        pairs[0] = pairs[1];
        part(&mut table, &mut pairs, 6, {
            let cps: Vec<u32> = std::iter::once(0)
                .chain(self.sections.iter().map(|(end, _)| *end))
                .collect();
            let data: Vec<Vec<u8>> = seds
                .iter()
                .map(|fc| {
                    let mut sed = vec![0, 0];
                    sed.extend(fc.to_le_bytes());
                    sed.extend([0, 0, 0xFF, 0xFF, 0xFF, 0xFF]);
                    sed
                })
                .collect();
            plc(&cps, &data)
        });
        part(&mut table, &mut pairs, 12, bte(&chp_bte));
        part(&mut table, &mut pairs, 13, bte(&pap_bte));
        part(&mut table, &mut pairs, 15, sttbf_ffn(&self.fonts));
        // Поля — отдельно в каждой части текста, места от её начала.
        let text_end = self.stories[STORY_TEXT];
        let mut story_start = 0;
        for (story, index) in [
            (STORY_TEXT, 16),
            (STORY_FOOTNOTES, 18),
            (STORY_HEADERS, 17),
            (STORY_COMMENTS, 19),
            (STORY_ENDNOTES, 48),
        ] {
            let len = self.stories[story];
            let fields: Vec<&(u32, [u8; 2])> = self
                .fields
                .iter()
                .filter(|(cp, _)| (story_start..story_start + len).contains(cp))
                .collect();
            if !fields.is_empty() {
                let cps: Vec<u32> = fields
                    .iter()
                    .map(|(cp, _)| cp - story_start)
                    .chain(std::iter::once(len))
                    .collect();
                let data: Vec<Vec<u8>> = fields.iter().map(|(_, fld)| fld.to_vec()).collect();
                part(&mut table, &mut pairs, index, plc(&cps, &data));
            }
            story_start += len;
        }

        // Сноски: ссылки (`FRD` — номер по порядку, 0 у своего знака)
        // и тексты.
        for (kind, slot, refs_index, texts_index) in [
            (NoteKind::Footnote, 0, 2, 3),
            (NoteKind::Endnote, 1, 46, 47),
        ] {
            let refs: Vec<&(u32, NoteKind, String, bool)> = self
                .note_refs
                .iter()
                .filter(|r| r.1 == kind && r.0 < text_end)
                .collect();
            if refs.is_empty() {
                continue;
            }
            let cps: Vec<u32> = refs
                .iter()
                .map(|r| r.0)
                .chain(std::iter::once(text_end + 1))
                .collect();
            let mut number = 0i16;
            let data: Vec<Vec<u8>> = refs
                .iter()
                .map(|r| {
                    let frd = if r.3 {
                        0
                    } else {
                        number = number.saturating_add(1);
                        number
                    };
                    frd.to_le_bytes().to_vec()
                })
                .collect();
            part(&mut table, &mut pairs, refs_index, plc(&cps, &data));
            let texts = std::mem::take(&mut self.note_texts[slot]);
            part(&mut table, &mut pairs, texts_index, plc(&texts, &[]));
        }
        if !self.header_cps.is_empty() {
            part(&mut table, &mut pairs, 11, plc(&self.header_cps, &[]));
        }
        self.comment_tables(&mut table, &mut pairs);
        let bookmarks: Vec<(String, u32, u32)> = self
            .bookmarks
            .iter()
            .filter_map(|(name, start, end)| Some((name.clone(), *start, (*end)?)))
            .collect();
        if !bookmarks.is_empty() {
            part(&mut table, &mut pairs, 21, sttbf_bkmk(&bookmarks));
            let ranges: Vec<(u32, u32)> = bookmarks.iter().map(|b| (b.1, b.2)).collect();
            let (bkf, bkl) = bookmark_plcs(&ranges, text_end + 1);
            part(&mut table, &mut pairs, 22, bkf);
            part(&mut table, &mut pairs, 23, bkl);
        }
        let (lst, lst_len, lfo) = self.lists();
        part(&mut table, &mut pairs, 73, lst);
        // Уровни лежат сразу за `PlfLst`, но в его длину не входят.
        pairs[73].1 = pairs[73].1.min(lst_len as u32);
        part(&mut table, &mut pairs, 74, lfo);
        part(&mut table, &mut pairs, 31, dop(self.doc));
        part(&mut table, &mut pairs, 33, clx(total));

        write_fib(&mut word, &self.stories, &pairs, template);
        let summary = crate::properties::write(&self.doc.properties.core);
        compound(&word, &table, &self.data, summary.as_deref())
    }

    /// Таблицы примечаний: ссылки с автором (`ATRD`), авторы, тексты и
    /// отмеченные куски (закладки примечаний).
    fn comment_tables(&mut self, table: &mut Vec<u8>, pairs: &mut [(u32, u32)]) {
        let text_end = self.stories[STORY_TEXT];
        let refs: Vec<(u32, String)> = self
            .comment_refs
            .iter()
            .filter(|r| r.0 < text_end)
            .cloned()
            .collect();
        if refs.is_empty() {
            return;
        }
        let mut part = |index: usize, bytes: Vec<u8>| {
            pairs[index] = (table.len() as u32, bytes.len() as u32);
            table.extend(bytes);
        };
        let mut owners: Vec<&str> = Vec::new();
        let mut ranges: Vec<(u32, u32)> = Vec::new();
        let mut atrds = Vec::new();
        for (cp, id) in &refs {
            let comment = self.doc.comments.iter().find(|c| &c.id == id);
            let author = comment.map_or("", |c| c.author.as_str());
            let ibst = owners.iter().position(|a| *a == author).unwrap_or_else(|| {
                owners.push(author);
                owners.len() - 1
            });
            let initials = comment
                .and_then(|c| c.initials.clone())
                .unwrap_or_else(|| initials_of(author));
            let units: Vec<u16> = initials.encode_utf16().take(9).collect();
            let mut atrd = (units.len() as u16).to_le_bytes().to_vec();
            for i in 0..9 {
                atrd.extend(units.get(i).copied().unwrap_or(0).to_le_bytes());
            }
            atrd.extend((ibst as u16).to_le_bytes());
            atrd.extend([0; 4]);
            let tag = match self.comment_ranges.get(id) {
                Some((Some(start), end)) => {
                    let end = end.unwrap_or(*cp).max(*start);
                    ranges.push((*start, end));
                    (ranges.len() - 1) as i32
                }
                _ => -1,
            };
            atrd.extend(tag.to_le_bytes());
            atrds.push(atrd);
        }
        let cps: Vec<u32> = refs
            .iter()
            .map(|r| r.0)
            .chain(std::iter::once(text_end + 1))
            .collect();
        part(4, plc(&cps, &atrds));
        part(5, plc(&self.comment_texts, &[]));
        let mut names = Vec::new();
        for owner in &owners {
            let units: Vec<u16> = owner.encode_utf16().take(54).collect();
            names.extend((units.len() as u16).to_le_bytes());
            for unit in units {
                names.extend(unit.to_le_bytes());
            }
        }
        part(36, names);
        if ranges.is_empty() {
            return;
        }
        // Закладки примечаний: безымянные, с номером-меткой из `ATRD`.
        let mut sttb = 0xFFFFu16.to_le_bytes().to_vec();
        sttb.extend((ranges.len() as u16).to_le_bytes());
        sttb.extend(10u16.to_le_bytes());
        for tag in 0..ranges.len() as u32 {
            sttb.extend([0, 0, 0, 1]);
            sttb.extend(tag.to_le_bytes());
            sttb.extend(0xFFFF_FFFFu32.to_le_bytes());
        }
        part(37, sttb);
        let (bkf, bkl) = bookmark_plcs(&ranges, text_end + 1);
        part(42, bkf);
        part(43, bkl);
    }

    /// Список абзаца (`sprmPIlfo`: номер списка с 1, 0 — без списка)
    /// и уровень (`sprmPIlvl`).
    fn numbering_sprms(&self, props: &ParagraphProps, out: &mut Vec<u8>) {
        let Some(numbering) = props.numbering else {
            return;
        };
        if let Some(id) = numbering.id {
            let ilfo = self
                .doc
                .numbering
                .instances
                .iter()
                .position(|n| n.id == id)
                .map_or(0, |i| (i + 1).min(0x07FE) as u16);
            push_u16(out, 0x460B, ilfo);
        }
        if let Some(level) = numbering.level {
            push_u8(out, 0x260A, level.min(8));
        }
    }

    /// Списки: описания (`PlfLst` и сразу за ним уровни `LVL`) и
    /// списки абзацев (`PlfLfo`).
    /// Возвращает `PlfLst` с уровнями, длину самого `PlfLst` и `PlfLfo`.
    fn lists(&mut self) -> (Vec<u8>, usize, Vec<u8>) {
        let numbering = &self.doc.numbering;
        if numbering.instances.is_empty() {
            return (Vec::new(), 0, Vec::new());
        }
        let lsid = |i: usize| 0x0100_0000u32 + i as u32;
        let count = numbering.abstracts.len().min(0x7FFF);
        let mut lst = (count as u16).to_le_bytes().to_vec();
        let mut levels = Vec::new();
        for (i, list) in numbering.abstracts.iter().take(count).enumerate() {
            lst.extend(lsid(i).to_le_bytes());
            lst.extend(0u32.to_le_bytes());
            for level in 0..9u8 {
                let istd = list
                    .levels
                    .iter()
                    .find(|l| l.level == level)
                    .and_then(|l| self.istd(l.style.as_deref()))
                    .unwrap_or(0x0FFF);
                lst.extend(istd.to_le_bytes());
            }
            lst.extend([0, 0]);
            for level in 0..9u8 {
                let lvl = list.levels.iter().find(|l| l.level == level);
                levels.extend(self.lvl(lvl, level));
            }
        }
        let lst_len = lst.len();
        lst.extend(levels);

        let count = numbering.instances.len().min(0x07FE);
        let mut lfo = (count as u32).to_le_bytes().to_vec();
        let mut data = Vec::new();
        for instance in numbering.instances.iter().take(count) {
            let index = numbering
                .abstracts
                .iter()
                .position(|a| a.id == instance.abstract_id)
                .unwrap_or(0);
            lfo.extend(lsid(index).to_le_bytes());
            lfo.extend([0; 8]);
            let overrides: Vec<_> = instance.overrides.iter().take(9).collect();
            lfo.extend([overrides.len() as u8, 0, 0, 0]);
            data.extend(0xFFFF_FFFFu32.to_le_bytes());
            for change in overrides {
                let mut flags = u32::from(change.level.min(8));
                if change.start.is_some() {
                    flags |= 0x10;
                }
                if change.definition.is_some() {
                    flags |= 0x20;
                }
                data.extend(change.start.unwrap_or(0).to_le_bytes());
                data.extend(flags.to_le_bytes());
                if let Some(definition) = &change.definition {
                    data.extend(self.lvl(Some(definition), change.level.min(8)));
                }
            }
        }
        lfo.extend(data);
        (lst, lst_len, lfo)
    }

    /// Уровень списка (`LVL`): `LVLF`, оформление абзаца и номера,
    /// текст номера (на местах номеров — номера уровней 0–8).
    fn lvl(&mut self, level: Option<&ListLevel>, index: u8) -> Vec<u8> {
        let default = ListLevel {
            text: Some(format!("%{}.", index + 1)),
            ..ListLevel::default()
        };
        let level = level.unwrap_or(&default);
        let mut units: Vec<u16> = Vec::new();
        let mut places: Vec<u8> = Vec::new();
        let text: Vec<char> = level.text.as_deref().unwrap_or("").chars().collect();
        let mut i = 0;
        while i < text.len() {
            match (text[i], text.get(i + 1).and_then(|c| c.to_digit(10))) {
                ('%', Some(n @ 1..=9)) => {
                    units.push(n as u16 - 1);
                    if places.len() < 9 {
                        places.push(units.len().min(255) as u8);
                    }
                    i += 2;
                }
                (c, _) => {
                    let mut buf = [0u16; 2];
                    units.extend_from_slice(c.encode_utf16(&mut buf));
                    i += 1;
                }
            }
        }
        units.truncate(255);
        let nfc = match &level.format {
            None | Some(NumberFormat::Decimal) => 0,
            Some(NumberFormat::UpperRoman) => 1,
            Some(NumberFormat::LowerRoman) => 2,
            Some(NumberFormat::UpperLetter) => 3,
            Some(NumberFormat::LowerLetter) => 4,
            Some(NumberFormat::Ordinal) => 5,
            Some(NumberFormat::DecimalZero) => 22,
            Some(NumberFormat::Bullet) => 23,
            Some(NumberFormat::RussianLower) => 58,
            Some(NumberFormat::RussianUpper) => 59,
            Some(NumberFormat::None) => 255,
            Some(NumberFormat::Other(name)) => name
                .strip_prefix("nfc")
                .and_then(|n| n.parse().ok())
                .unwrap_or(0),
        };
        let mut flags = match level.alignment {
            Some(Alignment::Center) => 1,
            Some(Alignment::Right) => 2,
            _ => 0,
        };
        if level.legal == Some(true) {
            flags |= 4;
        }
        if level.restart.is_some() {
            flags |= 8;
        }
        let follow = match level.suffix {
            Some(LevelSuffix::Space) => 1,
            Some(LevelSuffix::Nothing) => 2,
            _ => 0,
        };
        let mut paragraph = level.paragraph.clone();
        paragraph.numbering = None;
        let papx = papx(&paragraph);
        let mut run = level.run.clone();
        run.style = None;
        run.link = None;
        let mut chpx = Vec::new();
        self.run_props(&run, &mut chpx);
        let (papx, chpx) = (truncate_list(papx), truncate_list(chpx));

        let mut out = level.start.unwrap_or(1).to_le_bytes().to_vec();
        out.push(nfc);
        out.push(flags);
        let mut rgbxch = [0u8; 9];
        rgbxch[..places.len()].copy_from_slice(&places);
        out.extend(rgbxch);
        out.push(follow);
        out.extend([0; 8]);
        out.push(chpx.len() as u8);
        out.push(papx.len() as u8);
        out.push(level.restart.unwrap_or(0));
        out.push(0);
        out.extend(papx);
        out.extend(chpx);
        out.extend((units.len() as u16).to_le_bytes());
        for unit in units {
            out.extend(unit.to_le_bytes());
        }
        out
    }

    /// Таблица стилей (`STSH`).
    fn stsh(&mut self) -> Vec<u8> {
        let defaults = &self.doc.styles.defaults;
        let fonts = &defaults.run.fonts;
        let default_font = font_name(self.doc, &fonts.ascii, fonts.ascii_theme)
            .unwrap_or_else(|| FIRST_FONT.to_owned());
        let east = font_name(self.doc, &fonts.east_asia, fonts.east_asia_theme);
        let other = font_name(self.doc, &fonts.h_ansi, fonts.h_ansi_theme);
        let ascii = self.font(&default_font);
        let east = east.map_or(ascii, |f| self.font(&f));
        let other = other.map_or(ascii, |f| self.font(&f));
        let mut stshi = Vec::new();
        for value in [
            self.styles.len() as u16,
            10,
            1,
            0x5B,
            15,
            0,
            ascii,
            east,
            other,
        ] {
            stshi.extend(value.to_le_bytes());
        }
        let mut out = (stshi.len() as u16).to_le_bytes().to_vec();
        out.extend(stshi);
        let slots = std::mem::take(&mut self.styles);
        for slot in &slots {
            let std = match slot {
                Some(slot) => self.std(slot),
                None => Vec::new(),
            };
            out.extend((std.len() as u16).to_le_bytes());
            out.extend(std);
        }
        self.styles = slots;
        out
    }

    /// Один стиль (`STD`).
    fn std(&mut self, slot: &StyleSlot<'_>) -> Vec<u8> {
        let style = slot.style;
        let kind = if slot.default_font {
            StyleKind::Character
        } else {
            style.map_or(StyleKind::Paragraph, |s| s.kind)
        };
        let stk: u16 = if kind == StyleKind::Character { 2 } else { 1 };
        let based_on = style
            .and_then(|s| self.istd(s.based_on.as_deref()))
            .unwrap_or(0x0FFF);
        let next = match kind {
            StyleKind::Paragraph => style
                .and_then(|s| self.istd(s.next.as_deref()))
                .unwrap_or(0),
            _ => 0,
        };
        let based_on = if slot.normal || slot.default_font {
            0x0FFF
        } else {
            based_on
        };
        let cupx: u16 = if kind == StyleKind::Character { 1 } else { 2 };
        let name = match (
            style.and_then(|s| s.name.as_deref()),
            builtin_name(slot.sti),
        ) {
            (_, Some(builtin)) if slot.sti != 0x0FFE => builtin.to_owned(),
            (Some(name), _) => name.to_owned(),
            _ => style.map(|s| s.id.clone()).unwrap_or_default(),
        };

        // Свойства: у «Обычного» — вместе со свойствами по умолчанию.
        let defaults = &self.doc.styles.defaults;
        let mut paragraph = ParagraphProps::default();
        let mut run = RunProps::default();
        if slot.normal {
            paragraph = defaults.paragraph.clone();
            run = defaults.run.clone();
        }
        if let Some(style) = style {
            merge_paragraph(&mut paragraph, &style.paragraph);
            merge_run(&mut run, &style.run);
        }
        run.style = None;
        run.link = None;
        let mut chpx = Vec::new();
        self.run_props(&run, &mut chpx);
        let mut papx = papx(&paragraph);
        self.numbering_sprms(&paragraph, &mut papx);

        let mut out = Vec::new();
        out.extend(slot.sti.min(0x0FFF).to_le_bytes());
        out.extend((stk | (based_on << 4)).to_le_bytes());
        out.extend((cupx | (next << 4)).to_le_bytes());
        out.extend(0u16.to_le_bytes()); // bchUpe — пересчитывается Word
        out.extend(0u16.to_le_bytes()); // grfstd
        let units: Vec<u16> = name.encode_utf16().collect();
        out.extend((units.len() as u16).to_le_bytes());
        for unit in units {
            out.extend(unit.to_le_bytes());
        }
        out.extend([0, 0]);
        let upx = |out: &mut Vec<u8>, bytes: &[u8]| {
            if out.len() % 2 == 1 {
                out.push(0);
            }
            out.extend((bytes.len() as u16).to_le_bytes());
            out.extend(bytes);
        };
        if kind == StyleKind::Paragraph {
            let mut bytes = 0u16.to_le_bytes().to_vec();
            bytes.extend(papx);
            upx(&mut out, &bytes);
        }
        upx(&mut out, &chpx);
        if out.len() % 2 == 1 {
            out.push(0);
        }
        out
    }
}

/// Номер встроенного стиля по имени; 0x0FFE — стиль пользователя.
fn sti_of(style: &Style) -> u16 {
    let name = style.name.as_deref().unwrap_or(&style.id);
    (0..=110)
        .find(|&sti| builtin_name(sti).is_some_and(|n| n.eq_ignore_ascii_case(name)))
        .unwrap_or(0x0FFE)
}

/// Свойства стиля поверх свойств по умолчанию (заданные — заменяют).
fn merge_paragraph(base: &mut ParagraphProps, over: &ParagraphProps) {
    macro_rules! take {
        ($($field:ident),*) => { $( if over.$field.is_some() { base.$field = over.$field.clone(); } )* };
    }
    take!(
        alignment,
        numbering,
        contextual_spacing,
        keep_next,
        keep_lines,
        page_break_before,
        widow_control,
        borders,
        shading,
        suppress_auto_hyphens
    );
    let (b, o) = (&mut base.indentation, &over.indentation);
    if o.left.is_some() {
        b.left = o.left;
    }
    if o.right.is_some() {
        b.right = o.right;
    }
    if o.first_line.is_some() {
        b.first_line = o.first_line;
    }
    let (b, o) = (&mut base.spacing, &over.spacing);
    if o.before.is_some() {
        b.before = o.before;
    }
    if o.after.is_some() {
        b.after = o.after;
    }
    if o.line.is_some() {
        b.line = o.line;
    }
    if !over.tabs.is_empty() {
        base.tabs.clone_from(&over.tabs);
    }
}

fn merge_run(base: &mut RunProps, over: &RunProps) {
    macro_rules! take {
        ($($field:ident),*) => { $( if over.$field.is_some() { base.$field = over.$field.clone(); } )* };
    }
    take!(
        size,
        bold,
        italic,
        underline,
        strike,
        double_strike,
        color,
        highlight,
        vertical_align,
        caps,
        small_caps,
        hidden,
        spacing,
        scale,
        position,
        kerning
    );
    let (b, o) = (&mut base.fonts, &over.fonts);
    if o.ascii.is_some() || o.ascii_theme.is_some() {
        b.ascii.clone_from(&o.ascii);
        b.ascii_theme = o.ascii_theme;
    }
    if o.h_ansi.is_some() || o.h_ansi_theme.is_some() {
        b.h_ansi.clone_from(&o.h_ansi);
        b.h_ansi_theme = o.h_ansi_theme;
    }
    if o.east_asia.is_some() || o.east_asia_theme.is_some() {
        b.east_asia.clone_from(&o.east_asia);
        b.east_asia_theme = o.east_asia_theme;
    }
    if o.complex_script.is_some() || o.complex_script_theme.is_some() {
        b.complex_script.clone_from(&o.complex_script);
        b.complex_script_theme = o.complex_script_theme;
    }
}

/// Имя шрифта: шрифт темы важнее имени, как в Word.
fn font_name(doc: &Document, name: &Option<String>, theme: Option<ThemeFont>) -> Option<String> {
    theme
        .and_then(|theme| doc.theme.font(theme))
        .map(str::to_owned)
        .or_else(|| name.clone())
}

fn push(out: &mut Vec<u8>, code: u16, op: &[u8]) {
    out.extend(code.to_le_bytes());
    out.extend(op);
}

fn push_u8(out: &mut Vec<u8>, code: u16, value: u8) {
    push(out, code, &[value]);
}

fn push_u16(out: &mut Vec<u8>, code: u16, value: u16) {
    push(out, code, &value.to_le_bytes());
}

fn push_i16(out: &mut Vec<u8>, code: u16, value: i32) {
    let value = value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
    push(out, code, &value.to_le_bytes());
}

/// Переменное свойство: длина байтом впереди.
fn push_var(out: &mut Vec<u8>, code: u16, op: &[u8]) {
    out.extend(code.to_le_bytes());
    out.push(op.len().min(255) as u8);
    out.extend(&op[..op.len().min(255)]);
}

/// Оформление абзаца (`grpprl` без номера стиля).
fn papx(props: &ParagraphProps) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(alignment) = props.alignment {
        let jc = match alignment {
            Alignment::Left => 0,
            Alignment::Center => 1,
            Alignment::Right => 2,
            Alignment::Justify => 3,
            Alignment::Distribute => 4,
        };
        push_u8(&mut out, 0x2403, jc);
        push_u8(&mut out, 0x2461, jc);
    }
    let indent = &props.indentation;
    if let Some(left) = indent.left {
        push_i16(&mut out, 0x840F, left.0);
        push_i16(&mut out, 0x845E, left.0);
    }
    if let Some(right) = indent.right {
        push_i16(&mut out, 0x840E, right.0);
        push_i16(&mut out, 0x845D, right.0);
    }
    if let Some(first) = indent.first_line {
        let value = match first {
            FirstLineIndent::FirstLine(t) => t.0,
            FirstLineIndent::Hanging(t) => -t.0,
        };
        push_i16(&mut out, 0x8411, value);
        push_i16(&mut out, 0x8460, value);
    }
    let spacing = &props.spacing;
    if let Some(before) = spacing.before {
        push_u16(&mut out, 0xA413, before.0.clamp(0, 0xFFFF) as u16);
    }
    if let Some(after) = spacing.after {
        push_u16(&mut out, 0xA414, after.0.clamp(0, 0xFFFF) as u16);
    }
    if let Some(line) = spacing.line {
        let (value, multiple) = match line {
            LineSpacing::Multiple(n) => (n.min(0x7FFF) as i16, 1i16),
            LineSpacing::Exact(t) => (-(t.0.clamp(0, 0x7FFF) as i16), 0),
            LineSpacing::AtLeast(t) => (t.0.clamp(0, 0x7FFF) as i16, 0),
        };
        let mut op = value.to_le_bytes().to_vec();
        op.extend(multiple.to_le_bytes());
        push(&mut out, 0x6412, &op);
    }
    for (code, value) in [
        (0x2405, props.keep_lines),
        (0x2406, props.keep_next),
        (0x2407, props.page_break_before),
        (0x2431, props.widow_control),
        (0x246D, props.contextual_spacing),
        (0x242A, props.suppress_auto_hyphens),
    ] {
        if let Some(value) = value {
            push_u8(&mut out, code, u8::from(value));
        }
    }
    if !props.tabs.is_empty() {
        out.extend(tabs(props));
    }
    if let Some(borders) = &props.borders {
        paragraph_borders(&mut out, borders);
    }
    if let Some(shading) = &props.shading {
        push_u16(&mut out, 0x442D, shd80(shading));
        push_var(&mut out, 0xC64D, &shd(shading));
    }
    out
}

/// Позиции табуляции (`sprmPChgTabsPapx`): снятые и добавленные.
fn tabs(props: &ParagraphProps) -> Vec<u8> {
    let (cleared, added): (Vec<&TabStop>, Vec<&TabStop>) = props
        .tabs
        .iter()
        .take(64)
        .partition(|t| t.kind == TabStopKind::Clear);
    let mut op = vec![cleared.len() as u8];
    for tab in &cleared {
        op.extend((tab.position.0.clamp(-0x7FFF, 0x7FFF) as i16).to_le_bytes());
    }
    op.push(added.len() as u8);
    for tab in &added {
        op.extend((tab.position.0.clamp(-0x7FFF, 0x7FFF) as i16).to_le_bytes());
    }
    for tab in &added {
        let kind = match tab.kind {
            TabStopKind::Center => 1,
            TabStopKind::Right => 2,
            TabStopKind::Decimal => 3,
            TabStopKind::Bar => 4,
            _ => 0,
        };
        let leader = match tab.leader {
            TabLeader::Dot => 1,
            TabLeader::Hyphen => 2,
            TabLeader::Underscore => 3,
            TabLeader::Heavy => 4,
            TabLeader::MiddleDot => 5,
            TabLeader::None => 0,
        };
        op.push(kind | (leader << 3));
    }
    let mut out = Vec::new();
    push_var(&mut out, 0xC60D, &op);
    out
}

fn paragraph_borders(out: &mut Vec<u8>, borders: &ParagraphBorders) {
    let sides = [
        &borders.top,
        &borders.left,
        &borders.bottom,
        &borders.right,
        &borders.between,
        &borders.bar,
    ];
    for (i, side) in sides.iter().enumerate() {
        let Some(border) = side else {
            continue;
        };
        let old = if i == 5 { 0x6629 } else { 0x6424 + i as u16 };
        push(out, old, &brc80(border));
        push_var(out, 0xC64E + i as u16, &brc(border));
    }
}

/// Номер вида линии (`brcType`) по имени.
fn brc_type(style: &str) -> u8 {
    match style {
        "nil" | "none" => 0xFF,
        "thick" => 2,
        "double" => 3,
        "dotted" => 6,
        "dashed" => 7,
        "dotDash" => 8,
        "dotDotDash" => 9,
        "triple" => 10,
        "thinThickSmallGap" => 11,
        "thickThinSmallGap" => 12,
        "thinThickThinSmallGap" => 13,
        "thinThickMediumGap" => 14,
        "thickThinMediumGap" => 15,
        "thinThickThinMediumGap" => 16,
        "thinThickLargeGap" => 17,
        "thickThinLargeGap" => 18,
        "thinThickThinLargeGap" => 19,
        "wave" => 20,
        "doubleWave" => 21,
        "dashSmallGap" => 22,
        "dashDotStroked" => 23,
        "threeDEmboss" => 24,
        "threeDEngrave" => 25,
        "outset" => 26,
        "inset" => 27,
        _ => 1,
    }
}

/// Граница `Brc` (8 байт).
fn brc(border: &Border) -> [u8; 8] {
    let kind = brc_type(&border.style);
    if kind == 0xFF {
        return [0xFF; 8];
    }
    let [r, g, b, auto] = colorref(border.color);
    let width = border.size.unwrap_or(4).clamp(1, 255) as u8;
    let space = border.space.unwrap_or(0).min(31) as u8;
    [r, g, b, auto, width, kind, space, 0]
}

/// Старая граница `Brc80` (4 байта).
fn brc80(border: &Border) -> [u8; 4] {
    let kind = brc_type(&border.style);
    if kind == 0xFF {
        return [0xFF; 4];
    }
    let width = border.size.unwrap_or(4).clamp(1, 255) as u8;
    let space = border.space.unwrap_or(0).min(31) as u8;
    [width, kind, color_ico(border.color), space]
}

/// Цвет `COLORREF`; «авто» — признак в четвёртом байте.
fn colorref(color: Option<Color>) -> [u8; 4] {
    match color {
        Some(Color::Rgb(r, g, b)) => [r, g, b, 0],
        _ => [0, 0, 0, 0xFF],
    }
}

fn color_ico(color: Option<Color>) -> u8 {
    match color {
        Some(Color::Rgb(r, g, b)) => nearest_ico(r, g, b),
        _ => 0,
    }
}

/// Номер узора заливки по имени.
fn ipat(pattern: &str) -> u16 {
    match pattern {
        "solid" => 1,
        "pct5" => 2,
        "pct10" => 3,
        "pct20" => 4,
        "pct25" => 5,
        "pct30" => 6,
        "pct40" => 7,
        "pct50" => 8,
        "pct60" => 9,
        "pct70" => 10,
        "pct75" => 11,
        "pct80" => 12,
        "pct90" => 13,
        "horzStripe" => 14,
        "vertStripe" => 15,
        "reverseDiagStripe" => 16,
        "diagStripe" => 17,
        "horzCross" => 18,
        "diagCross" => 19,
        "thinHorzStripe" => 20,
        "thinVertStripe" => 21,
        "thinReverseDiagStripe" => 22,
        "thinDiagStripe" => 23,
        "thinHorzCross" => 24,
        "thinDiagCross" => 25,
        "nil" => 0xFFFF,
        _ => 0,
    }
}

/// Заливка `Shd` (10 байт).
fn shd(shading: &Shading) -> [u8; 10] {
    let mut out = [0u8; 10];
    out[..4].copy_from_slice(&colorref(shading.color));
    out[4..8].copy_from_slice(&colorref(shading.fill));
    out[8..].copy_from_slice(&ipat(&shading.pattern).to_le_bytes());
    out
}

/// Старая заливка `Shd80` (цвета из 16).
fn shd80(shading: &Shading) -> u16 {
    let pattern = ipat(&shading.pattern);
    if pattern == 0xFFFF {
        return 0xFFFF;
    }
    u16::from(color_ico(shading.color))
        | (u16::from(color_ico(shading.fill)) << 5)
        | ((pattern & 0x3F) << 10)
}

/// Ближайший из 16 цветов Word (`Ico`).
fn nearest_ico(r: u8, g: u8, b: u8) -> u8 {
    (1..=16u8)
        .filter_map(|ico| Some((ico, ico_rgb(ico)?)))
        .min_by_key(|(_, (r2, g2, b2))| {
            let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
            d(r, *r2) + d(g, *g2) + d(b, *b2)
        })
        .map_or(1, |(ico, _)| ico)
}

fn ico_rgb(ico: u8) -> Option<(u8, u8, u8)> {
    ico_to_highlight(ico).rgb()
}

fn ico_to_highlight(ico: u8) -> Highlight {
    match ico {
        1 => Highlight::Black,
        2 => Highlight::Blue,
        3 => Highlight::Cyan,
        4 => Highlight::Green,
        5 => Highlight::Magenta,
        6 => Highlight::Red,
        7 => Highlight::Yellow,
        8 => Highlight::White,
        9 => Highlight::DarkBlue,
        10 => Highlight::DarkCyan,
        11 => Highlight::DarkGreen,
        12 => Highlight::DarkMagenta,
        13 => Highlight::DarkRed,
        14 => Highlight::DarkYellow,
        15 => Highlight::DarkGray,
        16 => Highlight::LightGray,
        _ => Highlight::None,
    }
}

fn highlight_ico(highlight: Highlight) -> u8 {
    (1..=16u8)
        .find(|&ico| ico_to_highlight(ico) == highlight)
        .unwrap_or(0)
}

/// Вид подчёркивания (`kul`).
fn kul(underline: Underline) -> u8 {
    match underline {
        Underline::None => 0,
        Underline::Single => 1,
        Underline::Words => 2,
        Underline::Double => 3,
        Underline::Dotted => 4,
        Underline::Thick => 6,
        Underline::Dash => 7,
        Underline::DotDash => 9,
        Underline::DotDotDash => 10,
        Underline::Wave => 11,
        Underline::DottedHeavy => 20,
        Underline::DashedHeavy => 23,
        Underline::DashDotHeavy => 25,
        Underline::DashDotDotHeavy => 26,
        Underline::WavyHeavy => 27,
        Underline::DashLong => 39,
        Underline::WavyDouble => 43,
        Underline::DashLongHeavy => 55,
    }
}

/// Код поля гиперссылки.
/// Тип поля, если он неизвестен.
const FLT_UNKNOWN: u8 = 1;

/// Тип поля (`flt`) по его коду, например `PAGE \* MERGEFORMAT`.
fn field_type(code: &str) -> u8 {
    let name = code
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    match name.as_str() {
        "REF" => 3,
        "FTNREF" => 5,
        "SET" => 6,
        "IF" => 7,
        "INDEX" => 8,
        "STYLEREF" => 10,
        "SEQ" => 12,
        "TOC" => 13,
        "INFO" => 14,
        "TITLE" => 15,
        "SUBJECT" => 16,
        "AUTHOR" => 17,
        "KEYWORDS" => 18,
        "COMMENTS" => 19,
        "LASTSAVEDBY" => 20,
        "CREATEDATE" => 21,
        "SAVEDATE" => 22,
        "PRINTDATE" => 23,
        "REVNUM" => 24,
        "EDITTIME" => 25,
        "NUMPAGES" => 26,
        "NUMWORDS" => 27,
        "NUMCHARS" => 28,
        "FILENAME" => 29,
        "TEMPLATE" => 30,
        "DATE" => 31,
        "TIME" => 32,
        "PAGE" => 33,
        "=" => 34,
        "QUOTE" => 35,
        "INCLUDE" => 36,
        "PAGEREF" => 37,
        "ASK" => 38,
        "FILLIN" => 39,
        "NEXT" => 41,
        "NEXTIF" => 42,
        "SKIPIF" => 43,
        "MERGEREC" => 44,
        "DDE" => 45,
        "DDEAUTO" => 46,
        "GLOSSARY" => 47,
        "PRINT" => 48,
        "EQ" => 49,
        "GOTOBUTTON" => 50,
        "MACROBUTTON" => 51,
        "AUTONUMOUT" => 52,
        "AUTONUMLGL" => 53,
        "AUTONUM" => 54,
        "IMPORT" => 55,
        "LINK" => 56,
        "SYMBOL" => 57,
        "EMBED" => 58,
        "MERGEFIELD" => 59,
        "USERNAME" => 60,
        "USERINITIALS" => 61,
        "USERADDRESS" => 62,
        "BARCODE" => 63,
        "DOCVARIABLE" => 64,
        "SECTION" => 65,
        "SECTIONPAGES" => 66,
        "INCLUDEPICTURE" => 67,
        "INCLUDETEXT" => 68,
        "FILESIZE" => 69,
        "FORMTEXT" => 70,
        "FORMCHECKBOX" => 71,
        "NOTEREF" => 72,
        "TOA" => 73,
        "TA" => 74,
        "MERGESEQ" => 75,
        "AUTOTEXT" => 79,
        "COMPARE" => 80,
        "ADDIN" => 81,
        "FORMDROPDOWN" => 83,
        "ADVANCE" => 84,
        "DOCPROPERTY" => 85,
        "CONTROL" => 87,
        "HYPERLINK" => 88,
        "AUTOTEXTLIST" => 89,
        "LISTNUM" => 90,
        "HTMLCONTROL" => 91,
        "BIDIOUTLINE" => 92,
        "ADDRESSBLOCK" => 93,
        "GREETINGLINE" => 94,
        "SHAPE" => 95,
        _ => FLT_UNKNOWN,
    }
}

fn hyperlink_code(link: &Hyperlink) -> String {
    let mut code = String::from(" HYPERLINK ");
    if let Some(target) = &link.target {
        code.push_str(&format!("\"{target}\" "));
    }
    if let Some(anchor) = &link.anchor {
        code.push_str(&format!("\\l \"{anchor}\" "));
    }
    if let Some(tooltip) = &link.tooltip {
        code.push_str(&format!("\\o \"{tooltip}\" "));
    }
    code
}

/// Свойства раздела (`grpprl`).
fn sepx(section: &Section) -> Vec<u8> {
    let props = &section.props;
    let mut out = Vec::new();
    let bkc = match props.start.unwrap_or_default() {
        SectionStart::Continuous => 0,
        SectionStart::NextColumn => 1,
        SectionStart::NextPage => 2,
        SectionStart::EvenPage => 3,
        SectionStart::OddPage => 4,
    };
    push_u8(&mut out, 0x3009, bkc);
    if let Some(page) = props.page_size {
        push_i16(&mut out, 0xB01F, page.width.0);
        push_i16(&mut out, 0xB020, page.height.0);
        // Книжная ориентация — по умолчанию; её не записываем: LibreOffice
        // принимает любое значение, кроме нуля, за альбомную.
        if page.orientation == Orientation::Landscape {
            push_u8(&mut out, 0x301D, 2);
        }
    }
    if let Some(m) = props.margins {
        push_i16(&mut out, 0xB021, m.left.0);
        push_i16(&mut out, 0xB022, m.right.0);
        push_i16(&mut out, 0x9023, m.top.0);
        push_i16(&mut out, 0x9024, m.bottom.0);
        push_i16(&mut out, 0xB017, m.header.0);
        push_i16(&mut out, 0xB018, m.footer.0);
        push_i16(&mut out, 0xB025, m.gutter.0);
    }
    if let Some(columns) = &props.columns {
        push_u16(&mut out, 0x500B, columns.count.max(1) - 1);
        push_i16(&mut out, 0x900C, columns.space.0);
        push_u8(&mut out, 0x3019, u8::from(columns.separator));
    }
    if props.title_page {
        push_u8(&mut out, 0x300A, 1);
    }
    if let Some(numbering) = &props.page_numbering
        && let Some(start) = numbering.start
    {
        push_u8(&mut out, 0x3011, 1);
        push_u16(&mut out, 0x501C, start.min(0xFFFF) as u16);
    }
    out
}

/// Свойства строки таблицы (`TAP`) в оформлении знака конца строки.
fn tap(table: &Table, row: &letter_model::TableRow, edges: &[i32]) -> Vec<u8> {
    let mut out = Vec::new();
    let count = row.cells.len().min(63);
    // Ширины и ячейки (`sprmTDefTable`): границы ячеек и `TC80`.
    let mut def = vec![count as u8];
    for edge in edges.iter().take(count + 1) {
        def.extend((edge.clamp(&-0x7FFF, &0x7FFF).to_owned() as i16).to_le_bytes());
    }
    for cell in row.cells.iter().take(count) {
        let mut flags = 0u16;
        match cell.props.v_merge {
            Some(VerticalMerge::Restart) => flags |= 0x0060,
            Some(VerticalMerge::Continue) => flags |= 0x0020,
            None => {}
        }
        flags |= match cell.props.v_align {
            Some(CellVerticalAlign::Center) => 1 << 7,
            Some(CellVerticalAlign::Bottom) => 2 << 7,
            _ => 0,
        };
        def.extend(flags.to_le_bytes());
        def.extend(0u16.to_le_bytes());
        let borders = cell.props.borders.clone().unwrap_or_default();
        for side in [&borders.top, &borders.left, &borders.bottom, &borders.right] {
            def.extend(side.as_ref().map_or([0; 4], brc80));
        }
    }
    out.extend(0xD608u16.to_le_bytes());
    out.extend(((def.len() + 1) as u16).to_le_bytes());
    out.extend(def);
    push_i16(&mut out, 0x9602, 108);
    if let Some(alignment) = table.props.alignment {
        let jc = match alignment {
            Alignment::Center => 1,
            Alignment::Right => 2,
            _ => 0,
        };
        push_u16(&mut out, 0x5400, jc);
    }
    if let Some(height) = row.props.height {
        let value = match height.rule {
            letter_model::HeightRule::Exact => -height.value.0,
            _ => height.value.0,
        };
        push_i16(&mut out, 0x9407, value);
    }
    if row.props.header == Some(true) {
        push_u8(&mut out, 0x3404, 1);
    }
    if row.props.cant_split == Some(true) {
        push_u8(&mut out, 0x3403, 1);
        push_u8(&mut out, 0x3466, 1);
    }
    if let Some(borders) = &table.props.borders {
        let sides = [
            &borders.top,
            &borders.left,
            &borders.bottom,
            &borders.right,
            &borders.inside_h,
            &borders.inside_v,
        ];
        let mut old = Vec::new();
        let mut new = Vec::new();
        for side in sides {
            old.extend(side.as_ref().map_or([0; 4], brc80));
            new.extend(side.as_ref().map_or([0; 8], brc));
        }
        push_var(&mut out, 0xD605, &old);
        push_var(&mut out, 0xD613, &new);
    }
    // Точные цвета границ ячеек (`sprmTSetBrc`).
    for (i, cell) in row.cells.iter().take(count).enumerate() {
        let Some(borders) = &cell.props.borders else {
            continue;
        };
        for (bit, side) in [
            (1u8, &borders.top),
            (2, &borders.left),
            (4, &borders.bottom),
            (8, &borders.right),
        ] {
            if let Some(border) = side {
                let mut op = vec![i as u8, i as u8 + 1, bit];
                op.extend(brc(border));
                push_var(&mut out, 0xD62F, &op);
            }
        }
    }
    // Заливка ячеек: новая по 22 ячейки и старая.
    if row.cells.iter().any(|c| c.props.shading.is_some()) {
        let mut old = Vec::new();
        for cell in row.cells.iter().take(count) {
            old.extend(cell.props.shading.as_ref().map_or(0, shd80).to_le_bytes());
        }
        push_var(&mut out, 0xD609, &old);
        for (part, code) in [(0, 0xD612u16), (1, 0xD616), (2, 0xD60C)] {
            let cells: Vec<_> = row
                .cells
                .iter()
                .take(count)
                .skip(part * 22)
                .take(22)
                .collect();
            if cells.is_empty() {
                break;
            }
            let mut op = Vec::new();
            for cell in cells {
                let shading = cell
                    .props
                    .shading
                    .as_ref()
                    .map_or([0, 0, 0, 0xFF, 0, 0, 0, 0xFF, 0, 0], shd);
                op.extend(shading);
            }
            push_var(&mut out, code, &op);
        }
    }
    out
}

/// Запись `CHPX` для страницы: длина и свойства.
fn chpx_entry(grpprl: &[u8]) -> Vec<u8> {
    if grpprl.is_empty() {
        return Vec::new();
    }
    let grpprl = &grpprl[..grpprl.len().min(255)];
    let mut out = vec![grpprl.len() as u8];
    out.extend(grpprl);
    out
}

/// Запись `PAPX` для страницы: длина в словах, номер стиля, свойства.
fn papx_entry(istd: u16, grpprl: &[u8]) -> Vec<u8> {
    let mut body = istd.to_le_bytes().to_vec();
    body.extend(grpprl);
    // В странице места мало: длинное оформление (очень широкие
    // таблицы) обрезается до целых свойств.
    if body.len() > 480 {
        body = truncate_sprms(&body, 480);
    }
    if body.len() % 2 == 1 {
        let mut out = vec![body.len().div_ceil(2) as u8];
        out.extend(body);
        out
    } else {
        let mut out = vec![0, (body.len() / 2) as u8];
        out.extend(body);
        out
    }
}

/// Оставляет номер стиля и столько целых свойств, сколько влезает.
fn truncate_sprms(body: &[u8], limit: usize) -> Vec<u8> {
    let mut out = body[..2].to_vec();
    for (sprm, op) in crate::sprm::sprms(&body[2..]) {
        let mut one = sprm.to_le_bytes().to_vec();
        match sprm {
            0xD608 | 0xD606 => one.extend(((op.len() + 1) as u16).to_le_bytes()),
            _ if sprm >> 13 == 6 => one.push(op.len() as u8),
            _ => {}
        }
        one.extend(op);
        if out.len() + one.len() > limit {
            continue;
        }
        out.extend(one);
    }
    out
}

/// Страницы оформления (`FKP`) из отрезков «начало, конец, запись».
/// Возвращает байты страниц и для таблицы страниц — начало каждой
/// страницы с её номером и конец последней.
fn fkps(
    runs: &[(u32, u32, Vec<u8>)],
    paragraphs: bool,
    first_pn: u32,
) -> (Vec<u8>, Vec<(u32, u32)>) {
    let entry = if paragraphs { 13 } else { 1 };
    let mut out = Vec::new();
    let mut bte = Vec::new();
    let mut i = 0;
    while i < runs.len() {
        let mut page = vec![0u8; PAGE];
        let mut top = PAGE - 1;
        let mut count = 0;
        let mut offsets: Vec<u8> = Vec::new();
        let mut known: HashMap<&[u8], u8> = HashMap::new();
        while i + count < runs.len() {
            let data = &runs[i + count].2;
            let reuse = known.get(data.as_slice()).copied();
            let need = if data.is_empty() || reuse.is_some() {
                0
            } else {
                data.len() + 1
            };
            let header = 4 * (count + 2) + entry * (count + 1);
            if header + need > top {
                break;
            }
            let offset = if data.is_empty() {
                0
            } else if let Some(offset) = reuse {
                offset
            } else {
                let mut at = top - data.len();
                at -= at % 2;
                page[at..at + data.len()].copy_from_slice(data);
                top = at;
                known.insert(data.as_slice(), (at / 2) as u8);
                (at / 2) as u8
            };
            offsets.push(offset);
            count += 1;
        }
        if count == 0 {
            // Запись не влезает даже одна: пишем страницу без свойств.
            offsets.push(0);
            count = 1;
        }
        for k in 0..count {
            let (start, _, _) = runs[i + k];
            page[k * 4..k * 4 + 4].copy_from_slice(&start.to_le_bytes());
        }
        let end = runs[i + count - 1].1;
        page[count * 4..count * 4 + 4].copy_from_slice(&end.to_le_bytes());
        let rgb = 4 * (count + 1);
        for (k, offset) in offsets.iter().enumerate() {
            page[rgb + k * entry] = *offset;
        }
        page[PAGE - 1] = count as u8;
        bte.push((runs[i].0, first_pn + (out.len() / PAGE) as u32));
        out.extend(page);
        i += count;
    }
    if let Some(last) = runs.last() {
        bte.push((last.1, 0));
    }
    (out, bte)
}

/// Таблица страниц оформления (`PlcBte`).
fn bte(pages: &[(u32, u32)]) -> Vec<u8> {
    if pages.len() < 2 {
        return Vec::new();
    }
    let cps: Vec<u32> = pages.iter().map(|(fc, _)| *fc).collect();
    let data: Vec<Vec<u8>> = pages[..pages.len() - 1]
        .iter()
        .map(|(_, pn)| pn.to_le_bytes().to_vec())
        .collect();
    plc(&cps, &data)
}

/// `PLC`: позиции и записи.
fn plc(positions: &[u32], data: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for p in positions {
        out.extend(p.to_le_bytes());
    }
    for d in data {
        out.extend(d);
    }
    out
}

/// Начала (`PlcfBkf`) и концы (`PlcfBkl`) закладок `ranges`, которые
/// идут по порядку начал. Концы — по порядку; начало хранит номер
/// своего конца.
fn bookmark_plcs(ranges: &[(u32, u32)], last: u32) -> (Vec<u8>, Vec<u8>) {
    let mut ends: Vec<(u32, usize)> = ranges.iter().enumerate().map(|(i, r)| (r.1, i)).collect();
    ends.sort();
    let mut end_index = vec![0u16; ranges.len()];
    for (k, (_, i)) in ends.iter().enumerate() {
        end_index[*i] = k as u16;
    }
    let starts: Vec<u32> = ranges
        .iter()
        .map(|r| r.0)
        .chain(std::iter::once(last))
        .collect();
    let data: Vec<Vec<u8>> = end_index
        .iter()
        .map(|k| {
            let mut bkf = k.to_le_bytes().to_vec();
            bkf.extend([0, 0]);
            bkf
        })
        .collect();
    let end_cps: Vec<u32> = ends
        .iter()
        .map(|(cp, _)| *cp)
        .chain(std::iter::once(last))
        .collect();
    (plc(&starts, &data), plc(&end_cps, &[]))
}

/// Оформление уровня списка — не длиннее 255 байт (целыми свойствами).
fn truncate_list(grpprl: Vec<u8>) -> Vec<u8> {
    if grpprl.len() <= 255 {
        return grpprl;
    }
    let mut out = Vec::new();
    for (sprm, op) in crate::sprm::sprms(&grpprl) {
        let mut one = sprm.to_le_bytes().to_vec();
        if sprm >> 13 == 6 {
            one.push(op.len() as u8);
        }
        one.extend(op);
        if out.len() + one.len() > 255 {
            break;
        }
        out.extend(one);
    }
    out
}

/// Инициалы по имени: первые буквы слов.
fn initials_of(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .collect()
}

/// Таблица кусков: один кусок UTF-16 на весь текст.
fn clx(total: u32) -> Vec<u8> {
    let mut pcdt = plc(&[0, total], &[]);
    pcdt.extend([0, 0]);
    pcdt.extend(FC_TEXT.to_le_bytes());
    pcdt.extend([0, 0]);
    let mut out = vec![2];
    out.extend((pcdt.len() as u32).to_le_bytes());
    out.extend(pcdt);
    out
}

/// Таблица шрифтов (`SttbfFfn`).
fn sttbf_ffn(fonts: &[String]) -> Vec<u8> {
    let mut out = (fonts.len() as u16).to_le_bytes().to_vec();
    out.extend(0u16.to_le_bytes());
    for name in fonts {
        let mut ffn = Vec::new();
        // Шрифт TrueType с переменной шириной, «roman».
        let symbol = letter_model::is_symbol_font(name);
        ffn.push(0x16);
        ffn.extend(400u16.to_le_bytes());
        ffn.push(if symbol { 2 } else { 0 });
        ffn.push(0);
        ffn.extend([0; 10]);
        ffn.extend([0; 24]);
        for unit in name.encode_utf16().take(31) {
            ffn.extend(unit.to_le_bytes());
        }
        ffn.extend([0, 0]);
        out.push(ffn.len() as u8);
        out.extend(ffn);
    }
    out
}

/// Имена закладок (`SttbfBkmk`).
fn sttbf_bkmk(bookmarks: &[(String, u32, u32)]) -> Vec<u8> {
    let mut out = 0xFFFFu16.to_le_bytes().to_vec();
    out.extend((bookmarks.len() as u16).to_le_bytes());
    out.extend(0u16.to_le_bytes());
    for (name, _, _) in bookmarks {
        let units: Vec<u16> = name.encode_utf16().take(40).collect();
        out.extend((units.len() as u16).to_le_bytes());
        for unit in units {
            out.extend(unit.to_le_bytes());
        }
    }
    out
}

/// Свойства документа (`Dop97`, 500 байт).
fn dop(doc: &Document) -> Vec<u8> {
    let mut out = vec![0u8; 500];
    // Чётные и нечётные колонтитулы; запрет висячих строк.
    out[0] = u8::from(doc.settings.even_and_odd_headers) | 0x02;
    // Позиции табуляции по умолчанию — 1,25 см.
    out[10..12].copy_from_slice(&720u16.to_le_bytes());
    out
}

/// Заголовок (FIB) Word 97 в начале потока `WordDocument`.
fn write_fib(word: &mut [u8], stories: &[u32; 5], pairs: &[(u32, u32)], template: bool) {
    let total: u32 = stories.iter().sum();
    let mut fib = Vec::new();
    // FibBase.
    fib.extend(0xA5ECu16.to_le_bytes());
    fib.extend(0x00C1u16.to_le_bytes());
    fib.extend(0u16.to_le_bytes());
    fib.extend(0x0419u16.to_le_bytes());
    fib.extend(0u16.to_le_bytes());
    // Таблицы в `1Table`, расширенные знаки; у шаблона — `fDot`.
    fib.extend((0x1200u16 | u16::from(template)).to_le_bytes());
    fib.extend(0x00BFu16.to_le_bytes());
    fib.extend(0u32.to_le_bytes());
    fib.extend([0, 0]);
    // Старые поля fcMin и fcMac: где начинается и кончается текст. Их
    // по-прежнему смотрят другие программы.
    fib.extend([0; 4]);
    fib.extend(FC_TEXT.to_le_bytes());
    fib.extend((FC_TEXT + total * 2).to_le_bytes());
    // FibRgW97.
    fib.extend(14u16.to_le_bytes());
    fib.extend([0; 26]);
    fib.extend(0x0419u16.to_le_bytes());
    // FibRgLw97.
    fib.extend(22u16.to_le_bytes());
    let mut lw = [0u32; 22];
    lw[0] = word.len() as u32;
    // ccpText, ccpFtn, ccpHdd, ccpMcr, ccpAtn, ccpEdn.
    lw[3] = stories[STORY_TEXT];
    lw[4] = stories[STORY_FOOTNOTES];
    lw[5] = stories[STORY_HEADERS];
    lw[7] = stories[STORY_COMMENTS];
    lw[8] = stories[STORY_ENDNOTES];
    for value in lw {
        fib.extend(value.to_le_bytes());
    }
    // FibRgFcLcb97.
    fib.extend((pairs.len() as u16).to_le_bytes());
    for (fc, lcb) in pairs {
        fib.extend(fc.to_le_bytes());
        fib.extend(lcb.to_le_bytes());
    }
    fib.extend(0u16.to_le_bytes());
    word[..fib.len()].copy_from_slice(&fib);
}

fn pad_to_page(bytes: &mut Vec<u8>) {
    let len = bytes.len().div_ceil(PAGE) * PAGE;
    bytes.resize(len, 0);
}

/// Картинка в строке для потока `Data`: заголовок `PICF` (размер) и
/// фигура OfficeArt с картинкой внутри. Плавающая картинка тоже
/// записывается в строку. Картинку вида, которого нет в .doc (GIF,
/// SVG), не записываем — `None`.
fn picture_data(picture: &Picture) -> Option<Vec<u8>> {
    let image = picture.image.as_ref()?;
    let blip = blip_record(&image.bytes)?;
    // Описание в хранилище (`OfficeArtFBSE`) с картинкой внутри.
    let mut bse = vec![blip.1, blip.1];
    bse.extend(md5::Md5::digest(&image.bytes));
    bse.extend(0xFFu16.to_le_bytes());
    bse.extend((blip.0.len() as u32).to_le_bytes());
    bse.extend(1u32.to_le_bytes());
    bse.extend(0u32.to_le_bytes());
    bse.extend([0, 0, 0, 0]);
    bse.extend(&blip.0);
    let bse = art_record(2, u16::from(blip.1), 0xF007, &bse);
    // Фигура «рамка картинки» (тип 75) с картинкой номер 1.
    let mut fsp = 1025u32.to_le_bytes().to_vec();
    fsp.extend(0x0A00u32.to_le_bytes());
    let fsp = art_record(2, 75, 0xF00A, &fsp);
    let mut opt = 0x4104u16.to_le_bytes().to_vec();
    opt.extend(1u32.to_le_bytes());
    let opt = art_record(3, 1, 0xF00B, &opt);
    let shape = art_record(0xF, 0, 0xF004, &[fsp, opt].concat());

    let twips = |emu: i64| (emu / 635).clamp(1, 0x7FFF) as i16;
    let mut picf = vec![0u8; 68];
    let size = (68 + shape.len() + bse.len()) as u32;
    picf[0..4].copy_from_slice(&size.to_le_bytes());
    picf[4..6].copy_from_slice(&68u16.to_le_bytes());
    // MM_SHAPE: дальше фигура OfficeArt.
    picf[6..8].copy_from_slice(&0x64u16.to_le_bytes());
    picf[28..30].copy_from_slice(&twips(picture.width).to_le_bytes());
    picf[30..32].copy_from_slice(&twips(picture.height).to_le_bytes());
    picf[32..34].copy_from_slice(&1000u16.to_le_bytes());
    picf[34..36].copy_from_slice(&1000u16.to_le_bytes());
    picf.extend(shape);
    picf.extend(bse);
    Some(picf)
}

/// Запись OfficeArt: версия, вариант, вид, данные.
fn art_record(version: u16, instance: u16, kind: u16, data: &[u8]) -> Vec<u8> {
    let mut out = (version | (instance << 4)).to_le_bytes().to_vec();
    out.extend(kind.to_le_bytes());
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    out
}

/// Запись картинки (`OfficeArtBlip*`) по байтам файла и вид картинки
/// в хранилище (`btWin32`): PNG, JPEG, TIFF, BMP (без заголовка файла),
/// EMF и WMF (без сжатия).
fn blip_record(bytes: &[u8]) -> Option<(Vec<u8>, u8)> {
    let uid = md5::Md5::digest(bytes);
    let raster = |kind: u16, instance: u16, bt: u8, body: &[u8]| {
        let mut data = uid.to_vec();
        data.push(0xFF);
        data.extend(body);
        Some((art_record(0, instance, kind, &data), bt))
    };
    if bytes.starts_with(b"\x89PNG") {
        return raster(0xF01E, 0x6E0, 6, bytes);
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        return raster(0xF01D, 0x46A, 5, bytes);
    }
    if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        return raster(0xF029, 0x6E4, 0x11, bytes);
    }
    if bytes.starts_with(b"BM") {
        return raster(0xF01F, 0x7A8, 7, bytes.get(14..)?);
    }
    let emf = bytes.get(40..44) == Some(b" EMF");
    let placeable = bytes.starts_with(&[0xD7, 0xCD, 0xC6, 0x9A]);
    let wmf = placeable || bytes.starts_with(&[1, 0, 9, 0]) || bytes.starts_with(&[2, 0, 9, 0]);
    if !emf && !wmf {
        return None;
    }
    let body = if placeable { bytes.get(22..)? } else { bytes };
    // Рамка: у EMF — из заголовка, у WMF — из размещаемого заголовка.
    let side = |at: usize| {
        i32::from_le_bytes(
            bytes
                .get(at..at + 4)
                .and_then(|b| b.try_into().ok())
                .unwrap_or_default(),
        )
    };
    let side16 = |at: usize| i32::from(i16_le(bytes, at));
    let bounds = if emf {
        [side(8), side(12), side(16), side(20)]
    } else if placeable {
        [side16(6), side16(8), side16(10), side16(12)]
    } else {
        [0, 0, 1000, 1000]
    };
    let mut data = uid.to_vec();
    data.extend((body.len() as u32).to_le_bytes());
    for b in bounds {
        data.extend(b.to_le_bytes());
    }
    // Размер в EMU: считаем 1 единицу рамки за 1/1440 дюйма.
    let emu = |v: i32| (i64::from(v) * 635).clamp(0, i64::from(i32::MAX)) as i32;
    data.extend(emu(bounds[2] - bounds[0]).to_le_bytes());
    data.extend(emu(bounds[3] - bounds[1]).to_le_bytes());
    data.extend((body.len() as u32).to_le_bytes());
    // Без сжатия.
    data.extend([0xFE, 0xFE]);
    data.extend(body);
    let (kind, instance, bt) = if emf {
        (0xF01A, 0x3D4, 2)
    } else {
        (0xF01B, 0x216, 3)
    };
    Some((art_record(0, instance, kind, &data), bt))
}

fn i16_le(bytes: &[u8], at: usize) -> i16 {
    bytes
        .get(at..at + 2)
        .map_or(0, |b| i16::from_le_bytes([b[0], b[1]]))
}

/// Составной файл OLE с потоками документа Word.
fn compound(
    word: &[u8],
    table: &[u8],
    data: &[u8],
    summary: Option<&[u8]>,
) -> Result<Vec<u8>, WriteError> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut file = cfb::CompoundFile::create_with_version(cfb::Version::V3, cursor)?;
    // Тип хранилища — «документ Word».
    let clsid = uuid_word();
    file.set_storage_clsid("/", clsid)?;
    file.create_stream("/WordDocument")?.write_all(word)?;
    file.create_stream("/1Table")?.write_all(table)?;
    if !data.is_empty() {
        file.create_stream("/Data")?.write_all(data)?;
    }
    if let Some(summary) = summary {
        file.create_stream("/\u{5}SummaryInformation")?
            .write_all(summary)?;
    }
    file.create_stream("/\u{1}CompObj")?
        .write_all(&comp_obj())?;
    file.flush()?;
    Ok(file.into_inner().into_inner())
}

/// Идентификатор класса документа Word 97–2003.
fn uuid_word() -> uuid::Uuid {
    uuid::Uuid::from_u128(0x0002_0906_0000_0000_C000_0000_0000_0046)
}

/// Поток `\x01CompObj`: какой программой сделан документ.
fn comp_obj() -> Vec<u8> {
    let mut out = vec![1, 0, 0xFE, 0xFF, 3, 0x0A, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF];
    out.extend(uuid_word().to_bytes_le());
    let string = |out: &mut Vec<u8>, text: &str| {
        out.extend(((text.len() + 1) as u32).to_le_bytes());
        out.extend(text.as_bytes());
        out.push(0);
    };
    string(&mut out, "Microsoft Word 97-2003 Document");
    string(&mut out, "MSWordDoc");
    string(&mut out, "Word.Document.8");
    out.extend(0x71B2_39F4u32.to_le_bytes());
    out.extend([0; 12]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_types_by_code() {
        assert_eq!(field_type(" PAGE \\* MERGEFORMAT "), 33);
        assert_eq!(field_type(" hyperlink \"https://example.com\" "), 88);
        assert_eq!(field_type("НЕПОНЯТНОЕ"), FLT_UNKNOWN);
        assert_eq!(field_type(""), FLT_UNKNOWN);
    }

    #[test]
    fn paragraph_properties_entry_length() {
        // Нечётная длина: cb = половина с округлением вверх.
        assert_eq!(papx_entry(0, &[1]), vec![2, 0, 0, 1]);
        // Чётная: 0 и половина длины.
        assert_eq!(papx_entry(1, &[]), vec![0, 1, 1, 0]);
    }

    #[test]
    fn picture_kinds() {
        let png = b"\x89PNG\r\n\x1a\nrest";
        let (record, bt) = blip_record(png).unwrap();
        assert_eq!(bt, 6);
        // Заголовок записи, метка (16 байт), 0xFF, сам файл.
        assert_eq!(&record[2..4], &0xF01Eu16.to_le_bytes());
        assert_eq!(&record[8 + 17..], png);
        assert_eq!(blip_record(&[0xFF, 0xD8, 0]).map(|b| b.1), Some(5));
        assert!(blip_record(b"GIF89a").is_none());
        assert!(blip_record(b"").is_none());
    }

    #[test]
    fn initials() {
        assert_eq!(initials_of("Anna Reviewer"), "AR");
        assert_eq!(initials_of(""), "");
    }
}
