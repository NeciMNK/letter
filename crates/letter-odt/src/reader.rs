//! Чтение ODT в модель документа.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use letter_model::{
    Block, BreakKind, Document, HalfPoints, Inline, Orientation, PageMargins, PageSize, Paragraph,
    ParagraphProps, RawXml, Run, RunContent, RunFonts, RunProps, Section, SectionProps, Style,
    StyleKind, Table, TableCell, TableRow, Twips,
};
use zip::ZipArchive;

use crate::ReadError;
use crate::names::{STANDARD, display_name, style_id};
use crate::props::{FontFaces, read_paragraph, read_text};
use crate::toggles;
use crate::units::parse_length;
use crate::xml::{self, Element, Node};

/// Самая большая часть, которую мы согласны распаковать (512 МБ).
const MAX_PART_SIZE: u64 = 512 * 1024 * 1024;
/// Больше стольких повторов строки или ячейки (`number-*-repeated`) не
/// разворачиваем: защита от файлов, раздувающихся в памяти.
const MAX_REPEAT: usize = 1000;
/// Пространство имён WordprocessingML (для свойств ячеек).
const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
/// Глубина вложенности элементов, которую обходим.
const MAX_DEPTH: usize = 200;

/// Части документа: разобранные XML.
struct Parts {
    content: Element,
    styles: Option<Element>,
    meta: Option<Element>,
    settings: Option<Element>,
}

pub(crate) fn read(data: &[u8]) -> Result<Document, ReadError> {
    let parts = if data.starts_with(b"PK") {
        zip_parts(data)?
    } else {
        // Плоский ODT (.fodt): всё в одном XML-файле.
        let root = xml::parse(data).map_err(|_| ReadError::NotOdt(String::new()))?;
        if root.name != "office:document" {
            return Err(ReadError::NotOdt(String::new()));
        }
        Parts {
            content: root,
            styles: None,
            meta: None,
            settings: None,
        }
    };
    Ok(Reader::new(&parts).document(&parts))
}

fn zip_parts(data: &[u8]) -> Result<Parts, ReadError> {
    let mut archive = ZipArchive::new(Cursor::new(data))?;
    if let Some(mime) = read_part(&mut archive, "mimetype")? {
        let mime = String::from_utf8_lossy(&mime).trim().to_string();
        if !mime.starts_with("application/vnd.oasis.opendocument.text") {
            return Err(ReadError::NotOdt(mime));
        }
    }
    let content = read_part(&mut archive, "content.xml")?
        .ok_or_else(|| ReadError::MissingPart("content.xml".into()))?;
    let content = xml::parse(&content)?;
    // Без остальных частей документ всё равно можно показать.
    let mut optional = |name: &str| {
        read_part(&mut archive, name)
            .ok()
            .flatten()
            .and_then(|bytes| xml::parse(&bytes).ok())
    };
    Ok(Parts {
        styles: optional("styles.xml"),
        meta: optional("meta.xml"),
        settings: optional("settings.xml"),
        content,
    })
}

fn read_part(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Option<Vec<u8>>, ReadError> {
    let Some(index) = archive.index_for_name(name) else {
        return Ok(None);
    };
    let file = archive.by_index(index)?;
    if file.size() > MAX_PART_SIZE {
        return Err(ReadError::PartTooLarge(name.into()));
    }
    let mut bytes = Vec::new();
    // `take` — на случай, если размер в заголовке архива подделан.
    file.take(MAX_PART_SIZE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PART_SIZE {
        return Err(ReadError::PartTooLarge(name.into()));
    }
    Ok(Some(bytes))
}

/// Стиль из файла, как он записан.
#[derive(Debug, Clone, Default)]
struct OdfStyle {
    name: String,
    display: Option<String>,
    family: String,
    parent: Option<String>,
    next: Option<String>,
    paragraph: ParagraphProps,
    run: RunProps,
    /// Размер шрифта в процентах от родителя.
    size_percent: Option<f64>,
    /// Абзац с этим стилем начинает страницу с таким шаблоном.
    master_page: Option<String>,
    /// Шаг табуляции по умолчанию (только у стиля по умолчанию).
    tab_stop_distance: Option<Twips>,
}

struct Reader<'a> {
    fonts: FontFaces,
    /// Стили из `office:styles`, по порядку.
    named: Vec<OdfStyle>,
    default_paragraph: Option<OdfStyle>,
    /// Автоматические стили содержимого: (семейство, имя) → стиль.
    auto: HashMap<(String, String), OdfStyle>,
    /// Параметры страницы: имя шаблона страницы → параметры.
    masters: HashMap<String, SectionProps>,
    default_master: Option<String>,
    body: Option<&'a Element>,
}

impl<'a> Reader<'a> {
    fn new(parts: &'a Parts) -> Reader<'a> {
        let roots: Vec<&Element> = parts.styles.iter().chain([&parts.content]).collect();
        let mut fonts = FontFaces::new();
        for decls in roots
            .iter()
            .filter_map(|r| r.child("office:font-face-decls"))
        {
            for face in decls.elements().filter(|e| e.name == "style:font-face") {
                if let Some(name) = face.attr("style:name") {
                    let family = face
                        .attr("svg:font-family")
                        .map(|f| f.trim().trim_matches(|c| c == '\'' || c == '"'))
                        .filter(|f| !f.is_empty())
                        .unwrap_or(name);
                    fonts.insert(name.to_string(), family.to_string());
                }
            }
        }

        let mut reader = Reader {
            fonts,
            named: Vec::new(),
            default_paragraph: None,
            auto: HashMap::new(),
            masters: HashMap::new(),
            default_master: None,
            body: parts
                .content
                .child("office:body")
                .and_then(|b| b.child("office:text")),
        };

        let mut layouts: HashMap<String, SectionProps> = HashMap::new();
        for root in &roots {
            if let Some(styles) = root.child("office:styles") {
                for el in styles.elements() {
                    match el.name.as_str() {
                        "style:style" => reader.named.push(reader.style(el)),
                        "style:default-style" if el.attr("style:family") == Some("paragraph") => {
                            reader.default_paragraph = Some(reader.style(el));
                        }
                        _ => {}
                    }
                }
            }
            if let Some(auto) = root.child("office:automatic-styles") {
                for el in auto.elements() {
                    if el.name == "style:page-layout"
                        && let Some(name) = el.attr("style:name")
                    {
                        layouts.insert(name.to_string(), page_layout(el));
                    }
                }
            }
        }
        if let Some(auto) = parts.content.child("office:automatic-styles") {
            for el in auto.elements().filter(|e| e.name == "style:style") {
                let style = reader.style(el);
                reader
                    .auto
                    .insert((style.family.clone(), style.name.clone()), style);
            }
        }
        for root in &roots {
            let Some(masters) = root.child("office:master-styles") else {
                continue;
            };
            for el in masters.elements().filter(|e| e.name == "style:master-page") {
                let Some(name) = el.attr("style:name") else {
                    continue;
                };
                let props = el
                    .attr("style:page-layout-name")
                    .and_then(|l| layouts.get(l))
                    .cloned()
                    .unwrap_or_default();
                if reader.default_master.is_none() || name == STANDARD {
                    reader.default_master = Some(name.to_string());
                }
                reader.masters.insert(name.to_string(), props);
            }
        }
        reader
    }

    fn style(&self, el: &Element) -> OdfStyle {
        let mut style = OdfStyle {
            name: el.attr("style:name").unwrap_or_default().to_string(),
            display: el.attr("style:display-name").map(str::to_string),
            family: el.attr("style:family").unwrap_or_default().to_string(),
            parent: el.attr("style:parent-style-name").map(str::to_string),
            next: el.attr("style:next-style-name").map(str::to_string),
            master_page: el
                .attr("style:master-page-name")
                .filter(|m| !m.is_empty())
                .map(str::to_string),
            ..OdfStyle::default()
        };
        if let Some(p) = el.child("style:paragraph-properties") {
            style.paragraph = read_paragraph(p);
            style.tab_stop_distance = p
                .attr("style:tab-stop-distance")
                .and_then(parse_length)
                .filter(|t| t.0 > 0);
        }
        if let Some(t) = el.child("style:text-properties") {
            (style.run, style.size_percent) = read_text(t, &self.fonts);
        }
        if let Some(t) = el.child("style:table-properties")
            && t.attr("fo:break-before") == Some("page")
        {
            style.paragraph.page_break_before = Some(true);
        }
        style
    }

    fn document(&self, parts: &Parts) -> Document {
        let mut doc = Document::new();
        doc.sections.clear();
        self.styles(&mut doc);
        settings(&mut doc, parts);
        if let Some(meta) = parts
            .meta
            .as_ref()
            .unwrap_or(&parts.content)
            .child("office:meta")
        {
            properties(&mut doc, meta);
        }

        let first = self
            .default_master
            .as_ref()
            .and_then(|m| self.masters.get(m))
            .cloned()
            .unwrap_or_default();
        let mut sections = vec![Section {
            props: first,
            blocks: Vec::new(),
        }];
        if let Some(body) = self.body {
            let mut sink = |block: Block, master: Option<&str>| {
                if let Some(props) = master.and_then(|m| self.masters.get(m)) {
                    let current = sections.last_mut();
                    match current {
                        Some(s) if s.blocks.is_empty() => s.props = props.clone(),
                        _ => sections.push(Section {
                            props: props.clone(),
                            blocks: Vec::new(),
                        }),
                    }
                }
                if let Some(s) = sections.last_mut() {
                    s.blocks.push(block);
                }
            };
            self.blocks(body, &doc, &mut sink, 0);
        }
        if sections.iter().all(|s| s.blocks.is_empty()) {
            sections.truncate(1);
            if let Some(s) = sections.first_mut() {
                s.blocks.push(Paragraph::default().into());
            }
        }
        doc.sections = sections;
        if self.tabs_relative(parts) {
            make_tabs_absolute(&mut doc);
        }
        doc
    }

    /// Табуляции LibreOffice по умолчанию отсчитываются от отступа абзаца,
    /// а в Word — от поля.
    fn tabs_relative(&self, parts: &Parts) -> bool {
        config_item(parts, "TabsRelativeToIndent").is_none_or(|v| v)
    }

    fn styles(&self, doc: &mut Document) {
        if let Some(defaults) = &self.default_paragraph {
            doc.styles.defaults.paragraph = defaults.paragraph.clone();
            doc.styles.defaults.run = defaults.run.clone();
            if let Some(distance) = defaults.tab_stop_distance {
                doc.settings.default_tab_stop = distance;
            }
            if let Some(percent) = defaults.size_percent {
                doc.styles.defaults.run.size = Some(scale(HalfPoints(24), percent));
            }
        }
        doc.styles.styles.clear();
        let kind = |family: &str| match family {
            "paragraph" => Some(StyleKind::Paragraph),
            "text" => Some(StyleKind::Character),
            _ => None,
        };
        for odf in &self.named {
            let Some(kind) = kind(&odf.family) else {
                continue;
            };
            let id = style_id(&odf.name);
            if doc
                .styles
                .styles
                .iter()
                .any(|s| s.id == id && s.kind == kind)
            {
                continue;
            }
            let mut style = Style::new(id, kind);
            style.name = Some(display_name(&odf.name, odf.display.as_deref()));
            style.is_default = kind == StyleKind::Paragraph && odf.name == STANDARD;
            style.based_on = odf.parent.as_deref().map(style_id);
            style.next = odf.next.as_deref().map(style_id);
            if kind == StyleKind::Paragraph {
                style.paragraph = odf.paragraph.clone();
                style.paragraph.style = None;
            }
            style.run = odf.run.clone();
            doc.styles.styles.push(style);
        }
        // Размеры в процентах: от размера родителя, по порядку стилей.
        for odf in &self.named {
            let Some(percent) = odf.size_percent else {
                continue;
            };
            let id = style_id(&odf.name);
            let base = match kind(&odf.family) {
                Some(StyleKind::Paragraph) => {
                    let parent = ParagraphProps {
                        style: odf.parent.as_deref().map(style_id),
                        ..ParagraphProps::default()
                    };
                    doc.resolve_run(&parent, &RunProps::default()).size
                }
                _ => {
                    doc.resolve_run(&ParagraphProps::default(), &RunProps::default())
                        .size
                }
            };
            if let Some(style) = doc.styles.styles.iter_mut().find(|s| s.id == id) {
                style.run.size = Some(scale(base, percent));
            }
        }
    }

    /// Обходит блочные элементы и отдаёт блоки в `sink` вместе с именем
    /// шаблона страницы, если блок начинает новый раздел.
    fn blocks(
        &self,
        parent: &Element,
        doc: &Document,
        sink: &mut dyn FnMut(Block, Option<&str>),
        depth: usize,
    ) {
        if depth > MAX_DEPTH {
            return;
        }
        for el in parent.elements() {
            match el.name.as_str() {
                "text:p" | "text:h" => {
                    let (paragraph, master) = self.paragraph(el, doc);
                    sink(paragraph.into(), master);
                }
                "table:table" => {
                    let master = el
                        .attr("table:style-name")
                        .and_then(|n| self.auto.get(&("table".into(), n.into())))
                        .and_then(|s| s.master_page.as_deref());
                    sink(self.table(el, doc, depth).into(), master);
                }
                "text:sequence-decls"
                | "text:variable-decls"
                | "text:user-field-decls"
                | "text:dde-connection-decls"
                | "text:tracked-changes"
                | "text:soft-page-break" => {}
                name if name.ends_with("-source") => {}
                // Списки, разделы, оглавления и т. п.: берём абзацы изнутри.
                name if name.starts_with("text:") => self.blocks(el, doc, sink, depth + 1),
                _ => {}
            }
        }
    }

    fn table(&self, el: &Element, doc: &Document, depth: usize) -> Table {
        let mut table = Table::default();
        self.rows(el, doc, &mut table, depth);
        table
    }

    fn rows(&self, parent: &Element, doc: &Document, table: &mut Table, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        for el in parent.elements() {
            match el.name.as_str() {
                "table:table-row" => {
                    let row = self.row(el, doc, depth);
                    for _ in 0..repeat(el, "table:number-rows-repeated") {
                        if table.rows.len() >= MAX_REPEAT * 10 {
                            return;
                        }
                        table.rows.push(row.clone());
                    }
                }
                "table:table-header-rows" | "table:table-rows" | "table:table-row-group" => {
                    self.rows(el, doc, table, depth + 1);
                }
                _ => {}
            }
        }
    }

    fn row(&self, el: &Element, doc: &Document, depth: usize) -> TableRow {
        let mut row = TableRow::default();
        for cell_el in el.elements().filter(|e| e.name == "table:table-cell") {
            let mut blocks = Vec::new();
            self.blocks(cell_el, doc, &mut |b, _| blocks.push(b), depth + 1);
            if blocks.is_empty() {
                blocks.push(Paragraph::default().into());
            }
            // Объединение по горизонтали хранится, как в .docx, пока
            // свойства ячеек не разбираются (задача F5.2).
            let span = repeat(cell_el, "table:number-columns-spanned");
            let raw_props = if span > 1 {
                vec![RawXml::new(format!(
                    "<w:tcPr xmlns:w=\"{WORD_NS}\"><w:gridSpan w:val=\"{}\"/></w:tcPr>",
                    span.min(63)
                ))]
            } else {
                Vec::new()
            };
            let cell = TableCell { raw_props, blocks };
            for _ in 0..repeat(cell_el, "table:number-columns-repeated") {
                row.cells.push(cell.clone());
            }
        }
        row
    }

    fn paragraph(&self, el: &Element, doc: &Document) -> (Paragraph, Option<&str>) {
        let mut props = ParagraphProps::default();
        let mut base = RunProps::default();
        let mut master = None;
        if let Some(name) = el.attr("text:style-name") {
            match self.auto.get(&("paragraph".into(), name.into())) {
                Some(auto) => {
                    props = auto.paragraph.clone();
                    props.style = auto.parent.as_deref().map(style_id);
                    base = auto.run.clone();
                    if let Some(percent) = auto.size_percent {
                        let size = doc.resolve_run(&props, &RunProps::default()).size;
                        base.size = Some(scale(size, percent));
                    }
                    master = auto.master_page.as_deref();
                }
                None => props.style = Some(style_id(name)),
            }
        }
        let mut builder = Runs {
            runs: Vec::new(),
            last_space: true,
        };
        self.inline(el, doc, &props, &base, &mut builder, 0);
        let mut paragraph = Paragraph {
            props,
            content: Vec::new(),
        };
        for mut run in builder.runs {
            toggles::fix_from_odf(doc, &paragraph.props, &mut run.props);
            paragraph.content.push(Inline::Run(run));
        }
        (paragraph, master)
    }

    fn inline(
        &self,
        el: &Element,
        doc: &Document,
        paragraph: &ParagraphProps,
        props: &RunProps,
        out: &mut Runs,
        depth: usize,
    ) {
        if depth > MAX_DEPTH {
            return;
        }
        for node in &el.children {
            let child = match node {
                Node::Text(text) => {
                    out.text(text, props, true);
                    continue;
                }
                Node::Element(child) => child,
            };
            match child.name.as_str() {
                "text:s" => {
                    let n = child
                        .attr("text:c")
                        .and_then(|c| c.trim().parse::<usize>().ok())
                        .unwrap_or(1)
                        .clamp(1, MAX_REPEAT);
                    out.text(&" ".repeat(n), props, false);
                    out.last_space = true;
                }
                "text:tab" => out.push(RunContent::Tab, props),
                "text:line-break" => out.push(RunContent::Break(BreakKind::Line), props),
                "text:note"
                | "text:ruby-text"
                | "text:tracked-changes"
                | "text:soft-page-break"
                | "text:bookmark"
                | "text:bookmark-start"
                | "text:bookmark-end" => {}
                name if name.starts_with("text:") => {
                    let inner = self.span_props(child, doc, paragraph, props);
                    self.inline(child, doc, paragraph, &inner, out, depth + 1);
                }
                // Примечания, рисунки и прочее пока не переносятся.
                _ => {}
            }
        }
    }

    /// Оформление внутри `text:span` (и других элементов со стилем).
    fn span_props(
        &self,
        el: &Element,
        doc: &Document,
        paragraph: &ParagraphProps,
        outer: &RunProps,
    ) -> RunProps {
        let Some(name) = el.attr("text:style-name") else {
            return outer.clone();
        };
        let mut props = outer.clone();
        match self.auto.get(&("text".into(), name.into())) {
            Some(auto) => {
                if let Some(parent) = &auto.parent {
                    props.style = Some(style_id(parent));
                }
                overlay(&mut props, &auto.run);
                if let Some(percent) = auto.size_percent {
                    let size = doc.resolve_run(paragraph, outer).size;
                    props.size = Some(scale(size, percent));
                }
            }
            None => {
                let id = style_id(name);
                let known = doc
                    .styles
                    .get(&id)
                    .is_some_and(|s| s.kind == StyleKind::Character);
                if known {
                    props.style = Some(id);
                }
            }
        }
        props
    }
}

/// Сборщик фрагментов абзаца.
struct Runs {
    runs: Vec<Run>,
    /// Последний знак — пробел (или начало абзаца): следующие пробелы
    /// в тексте XML не значимы.
    last_space: bool,
}

impl Runs {
    /// Добавляет текст. Если `collapse`, пробелы схлопываются, как
    /// требует ODF для текста в XML; иначе (`text:s`) берутся как есть.
    fn text(&mut self, text: &str, props: &RunProps, collapse: bool) {
        let mut out = String::new();
        for c in text.chars() {
            let c = if collapse && matches!(c, '\t' | '\n' | '\r') {
                ' '
            } else {
                c
            };
            if c == ' ' {
                if collapse && self.last_space {
                    continue;
                }
                self.last_space = true;
            } else {
                self.last_space = false;
            }
            out.push(c);
        }
        if !out.is_empty() {
            self.append(RunContent::Text(out), props);
        }
    }

    /// Добавляет табуляцию или разрыв.
    fn push(&mut self, item: RunContent, props: &RunProps) {
        self.append(item, props);
        self.last_space = false;
    }

    fn append(&mut self, item: RunContent, props: &RunProps) {
        match self.runs.last_mut().filter(|r| r.props == *props) {
            Some(run) => match (run.content.last_mut(), item) {
                (Some(RunContent::Text(last)), RunContent::Text(text)) => last.push_str(&text),
                (_, item) => run.content.push(item),
            },
            None => self.runs.push(Run {
                props: props.clone(),
                content: vec![item],
            }),
        }
    }
}

/// Накладывает заданные в `layer` свойства на `acc`.
fn overlay(acc: &mut RunProps, layer: &RunProps) {
    fn set<T: Clone>(acc: &mut Option<T>, layer: &Option<T>) {
        if layer.is_some() {
            acc.clone_from(layer);
        }
    }
    let RunFonts {
        ascii,
        h_ansi,
        east_asia,
        complex_script,
        ..
    } = &layer.fonts;
    set(&mut acc.fonts.ascii, ascii);
    set(&mut acc.fonts.h_ansi, h_ansi);
    set(&mut acc.fonts.east_asia, east_asia);
    set(&mut acc.fonts.complex_script, complex_script);
    set(&mut acc.size, &layer.size);
    set(&mut acc.bold, &layer.bold);
    set(&mut acc.italic, &layer.italic);
    set(&mut acc.underline, &layer.underline);
    set(&mut acc.strike, &layer.strike);
    set(&mut acc.double_strike, &layer.double_strike);
    set(&mut acc.color, &layer.color);
    set(&mut acc.highlight, &layer.highlight);
    set(&mut acc.vertical_align, &layer.vertical_align);
}

fn scale(size: HalfPoints, percent: f64) -> HalfPoints {
    HalfPoints::from_points(size.to_points() * percent / 100.0)
}

fn repeat(el: &Element, attr: &str) -> usize {
    el.attr(attr)
        .and_then(|n| n.trim().parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, MAX_REPEAT)
}

/// Параметры страницы из `style:page-layout`.
fn page_layout(el: &Element) -> SectionProps {
    let Some(p) = el.child("style:page-layout-properties") else {
        return SectionProps::default();
    };
    let len = |e: &Element, name: &str| e.attr(name).and_then(parse_length);
    let default = PageSize::default();
    let width = len(p, "fo:page-width").unwrap_or(default.width);
    let height = len(p, "fo:page-height").unwrap_or(default.height);
    let orientation = match p.attr("style:print-orientation") {
        Some("landscape") => Orientation::Landscape,
        Some("portrait") => Orientation::Portrait,
        _ if width > height => Orientation::Landscape,
        _ => Orientation::Portrait,
    };
    let all = len(p, "fo:margin");
    let margin = |name: &str| len(p, name).or(all).unwrap_or(Twips::ZERO);
    let defaults = PageMargins::default();
    let mut margins = PageMargins {
        top: margin("fo:margin-top"),
        right: margin("fo:margin-right"),
        bottom: margin("fo:margin-bottom"),
        left: margin("fo:margin-left"),
        header: defaults.header,
        footer: defaults.footer,
        gutter: len(p, "loext:margin-gutter").unwrap_or(Twips::ZERO),
    };
    // С колонтитулом поле ODF — это расстояние до колонтитула, а текст
    // начинается ниже на высоту колонтитула вместе с отступом.
    let extra = |style: &str, gap: &str| {
        let props = el.child(style)?.child("style:header-footer-properties")?;
        len(props, "fo:min-height")
            .or_else(|| len(props, gap))
            .map(|t| t.0.max(0))
    };
    if let Some(height) = extra("style:header-style", "fo:margin-bottom") {
        margins.header = margins.top;
        margins.top = Twips(margins.top.0.saturating_add(height));
    }
    if let Some(height) = extra("style:footer-style", "fo:margin-top") {
        margins.footer = margins.bottom;
        margins.bottom = Twips(margins.bottom.0.saturating_add(height));
    }
    SectionProps {
        page_size: Some(PageSize {
            width,
            height,
            orientation,
        }),
        margins: Some(margins),
        unknown: Vec::new(),
    }
}

/// Значение логической настройки из `settings.xml`.
fn config_item(parts: &Parts, name: &str) -> Option<bool> {
    fn find<'e>(el: &'e Element, name: &str) -> Option<&'e Element> {
        if el.name == "config:config-item" && el.attr("config:name") == Some(name) {
            return Some(el);
        }
        el.elements().find_map(|e| find(e, name))
    }
    let root = parts.settings.as_ref().unwrap_or(&parts.content);
    let settings = root.child("office:settings")?;
    match find(settings, name)?.text().trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn settings(doc: &mut Document, parts: &Parts) {
    // В LibreOffice эта настройка называется «Добавлять интервал между
    // абзацами и таблицами»; по умолчанию интервалы складываются.
    doc.settings.add_paragraph_spacing = config_item(parts, "AddParaTableSpacing").unwrap_or(true);
}

/// Свойства документа из `office:meta`.
fn properties(doc: &mut Document, meta: &Element) {
    let core = &mut doc.properties.core;
    let app = &mut doc.properties.app;
    let mut keywords = Vec::new();
    for el in meta.elements() {
        let text = || Some(el.text());
        match el.name.as_str() {
            "dc:title" => core.title = text(),
            "dc:subject" => core.subject = text(),
            "dc:description" => core.description = text(),
            "meta:keyword" => keywords.push(el.text()),
            "meta:initial-creator" => core.creator = text(),
            "dc:creator" => core.last_modified_by = text(),
            "meta:creation-date" => core.created = text(),
            "dc:date" => core.modified = text(),
            "meta:print-date" => core.last_printed = text(),
            "meta:editing-cycles" => core.revision = text(),
            "dc:language" => core.language = text(),
            "meta:generator" => app.application = text(),
            "meta:editing-duration" => app.total_time = duration_minutes(&el.text()),
            "meta:template" => app.template = el.attr("xlink:title").map(str::to_string),
            "meta:document-statistic" => {
                let n = |name: &str| el.attr(name).and_then(|v| v.trim().parse().ok());
                app.pages = n("meta:page-count");
                app.words = n("meta:word-count");
                app.characters_with_spaces = n("meta:character-count");
                app.characters = n("meta:non-whitespace-character-count");
                app.paragraphs = n("meta:paragraph-count");
            }
            _ => {}
        }
    }
    if !keywords.is_empty() {
        core.keywords = Some(keywords.join(", "));
    }
}

/// Длительность ISO 8601 (`P1DT2H30M`) в минутах.
fn duration_minutes(value: &str) -> Option<u32> {
    let rest = value.trim().strip_prefix('P')?;
    let (date, time) = rest.split_once('T').unwrap_or((rest, ""));
    let mut minutes = 0.0;
    let mut add = |part: &str, units: &[(char, f64)]| -> Option<()> {
        let mut number = String::new();
        for c in part.chars() {
            if c.is_ascii_digit() || c == '.' {
                number.push(c);
            } else {
                let factor = units.iter().find(|(u, _)| *u == c)?.1;
                minutes += number.parse::<f64>().ok()? * factor;
                number.clear();
            }
        }
        number.is_empty().then_some(())
    };
    add(
        date,
        &[
            ('Y', 525_600.0),
            ('M', 43_200.0),
            ('W', 10_080.0),
            ('D', 1440.0),
        ],
    )?;
    add(time, &[('H', 60.0), ('M', 1.0), ('S', 1.0 / 60.0)])?;
    (minutes.is_finite() && minutes < f64::from(u32::MAX)).then_some(minutes.round() as u32)
}

/// Переводит позиции табуляции из отсчёта от отступа абзаца в отсчёт
/// от поля, как в Word.
fn make_tabs_absolute(doc: &mut Document) {
    let shifts: Vec<(usize, Twips)> = doc
        .styles
        .styles
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind == StyleKind::Paragraph && !s.paragraph.tabs.is_empty())
        .map(|(i, s)| {
            let props = ParagraphProps {
                style: Some(s.id.clone()),
                ..ParagraphProps::default()
            };
            (i, doc.resolve_paragraph(&props).indent_left)
        })
        .collect();
    let mut sections = std::mem::take(&mut doc.sections);
    for section in &mut sections {
        shift_blocks(doc, &mut section.blocks);
    }
    doc.sections = sections;
    for (i, shift) in shifts {
        if let Some(style) = doc.styles.styles.get_mut(i) {
            for tab in &mut style.paragraph.tabs {
                tab.position = Twips(tab.position.0.saturating_add(shift.0));
            }
        }
    }
}

fn shift_blocks(doc: &Document, blocks: &mut [Block]) {
    for block in blocks {
        match block {
            Block::Paragraph(p) if !p.props.tabs.is_empty() => {
                let shift = doc.resolve_paragraph(&p.props).indent_left;
                for tab in &mut p.props.tabs {
                    tab.position = Twips(tab.position.0.saturating_add(shift.0));
                }
            }
            Block::Table(t) => {
                for cell in t.rows.iter_mut().flat_map(|r| &mut r.cells) {
                    shift_blocks(doc, &mut cell.blocks);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(duration_minutes("PT1H30M"), Some(90));
        assert_eq!(duration_minutes("P1DT0H0M30S"), Some(1441));
        assert_eq!(duration_minutes("P0D"), Some(0));
        assert_eq!(duration_minutes("1H"), None);
        assert_eq!(duration_minutes("PT1X"), None);
    }

    #[test]
    fn whitespace_collapses_like_odf() {
        let mut runs = Runs {
            runs: Vec::new(),
            last_space: true,
        };
        let props = RunProps::default();
        runs.text("  a \n  b ", &props, true);
        runs.text("  ", &props, false);
        runs.text(" c", &props, true);
        assert_eq!(runs.runs.len(), 1);
        assert_eq!(runs.runs[0].plain_text(), "a b   c");
    }
}
