//! Запись модели в XML: главная часть документа и стили.
//!
//! Word строго следит за порядком дочерних элементов (его задаёт схема
//! OOXML). Понятые свойства и сохранённые без разбора ([`RawXml`])
//! перемешиваются, поэтому все дочерние элементы сначала собираются
//! вместе, а потом упорядочиваются по таблицам ниже.

use letter_model::{
    Block, BreakKind, Color, Document, FirstLineIndent, Inline, LineSpacing, Orientation,
    Paragraph, ParagraphProps, RawXml, Run, RunContent, RunFonts, RunProps, Section, SectionProps,
    Style, StyleKind, Styles, Table, VerticalAlign,
};

use crate::document::{W_NS, is_w_ns};
use crate::names::{
    ALIGNMENTS, HIGHLIGHTS, TAB_KINDS, TAB_LEADERS, THEME_FONTS, UNDERLINES, name_of,
};
use crate::xml;

/// Пространство имён связей (`r:id`).
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// Порядок дочерних элементов `w:document`.
const DOCUMENT_ORDER: &[&str] = &["background", "body"];

/// Порядок дочерних элементов `w:pPr` (CT_PPr).
const PPR_ORDER: &[&str] = &[
    "pStyle",
    "keepNext",
    "keepLines",
    "pageBreakBefore",
    "framePr",
    "widowControl",
    "numPr",
    "suppressLineNumbers",
    "pBdr",
    "shd",
    "tabs",
    "suppressAutoHyphens",
    "kinsoku",
    "wordWrap",
    "overflowPunct",
    "topLinePunct",
    "autoSpaceDE",
    "autoSpaceDN",
    "bidi",
    "adjustRightInd",
    "snapToGrid",
    "spacing",
    "ind",
    "contextualSpacing",
    "mirrorIndents",
    "suppressOverlap",
    "jc",
    "textDirection",
    "textAlignment",
    "textboxTightWrap",
    "outlineLvl",
    "divId",
    "cnfStyle",
    "rPr",
    "sectPr",
    "pPrChange",
];

/// Порядок дочерних элементов `w:rPr` (CT_RPr).
const RPR_ORDER: &[&str] = &[
    "rStyle",
    "rFonts",
    "b",
    "bCs",
    "i",
    "iCs",
    "caps",
    "smallCaps",
    "strike",
    "dstrike",
    "outline",
    "shadow",
    "emboss",
    "imprint",
    "noProof",
    "snapToGrid",
    "vanish",
    "webHidden",
    "color",
    "spacing",
    "w",
    "kern",
    "position",
    "sz",
    "szCs",
    "highlight",
    "u",
    "effect",
    "bdr",
    "shd",
    "fitText",
    "vertAlign",
    "rtl",
    "cs",
    "em",
    "lang",
    "eastAsianLayout",
    "specVanish",
    "oMath",
    "rPrChange",
];

/// Порядок дочерних элементов `w:sectPr` (CT_SectPr). Ссылки на
/// колонтитулы идут первыми в любом порядке.
const SECTPR_ORDER: &[&str] = &[
    "headerReference",
    "footerReference",
    "footnotePr",
    "endnotePr",
    "type",
    "pgSz",
    "pgMar",
    "paperSrc",
    "pgBorders",
    "lnNumType",
    "pgNumType",
    "cols",
    "formProt",
    "vAlign",
    "noEndnote",
    "titlePg",
    "textDirection",
    "bidi",
    "rtlGutter",
    "docGrid",
    "printerSettings",
    "sectPrChange",
];

/// Порядок дочерних элементов `w:styles`.
const STYLES_ORDER: &[&str] = &["docDefaults", "latentStyles", "style"];

/// Порядок дочерних элементов `w:style` (CT_Style).
const STYLE_ORDER: &[&str] = &[
    "name",
    "aliases",
    "basedOn",
    "next",
    "link",
    "autoRedefine",
    "hidden",
    "uiPriority",
    "semiHidden",
    "unhideWhenUsed",
    "qFormat",
    "locked",
    "personal",
    "personalCompose",
    "personalReply",
    "rsid",
    "pPr",
    "rPr",
    "tblPr",
    "trPr",
    "tcPr",
    "tblStylePr",
];

/// Атрибуты корневого элемента: объявления пространств имён и
/// `mc:Ignorable`. Префикс `w` всегда указывает на обычный WordprocessingML,
/// потому что именно его пишет запись.
pub(crate) fn root_attributes(original: Option<&[(String, String)]>) -> Vec<(String, String)> {
    let mut attrs: Vec<(String, String)> = match original {
        Some(attrs) => attrs.to_vec(),
        None => vec![("xmlns:r".into(), R_NS.into())],
    };
    match attrs.iter_mut().find(|(name, _)| name == "xmlns:w") {
        Some((_, uri)) => *uri = W_NS.into(),
        None => attrs.insert(0, ("xmlns:w".into(), W_NS.into())),
    }
    attrs
}

/// Главная часть документа (`word/document.xml`).
pub(crate) fn document_xml(doc: &Document) -> String {
    let attrs = root_attributes(
        doc.source
            .as_ref()
            .map(|s| s.main_root_attributes.as_slice()),
    );
    let mut children = Children::new(DOCUMENT_ORDER);
    children.raw(&doc.unknown);
    children.push("body", body(&doc.sections));
    xml_part(&element_with("w:document", &attrs, &children.finish()))
}

/// Тело документа. Параметры каждого раздела, кроме последнего, пишутся
/// в свойства его последнего абзаца, параметры последнего — в конец тела.
fn body(sections: &[Section]) -> String {
    let mut out = String::new();
    for (i, section) in sections.iter().enumerate() {
        let is_last = i + 1 == sections.len();
        let blocks = &section.blocks;
        if is_last {
            for block in blocks {
                out.push_str(&block_xml(block));
            }
            if section.props != SectionProps::default() || blocks.is_empty() {
                out.push_str(&section_props(&section.props));
            }
            continue;
        }
        match blocks.split_last() {
            Some((Block::Paragraph(last), rest)) => {
                for block in rest {
                    out.push_str(&block_xml(block));
                }
                out.push_str(&paragraph(last, Some(&section.props)));
            }
            // Раздел кончается не абзацем (или пуст): параметры раздела
            // некуда положить, поэтому добавляем пустой абзац, как Word.
            _ => {
                for block in blocks {
                    out.push_str(&block_xml(block));
                }
                out.push_str(&paragraph(&Paragraph::default(), Some(&section.props)));
            }
        }
    }
    element("w:body", &out)
}

fn block_xml(block: &Block) -> String {
    match block {
        Block::Paragraph(p) => paragraph(p, None),
        Block::Table(t) => table(t),
        Block::Unknown(raw) => raw.as_str().to_string(),
    }
}

fn paragraph(p: &Paragraph, section: Option<&SectionProps>) -> String {
    let mut out = paragraph_props(&p.props, section);
    for inline in &p.content {
        match inline {
            Inline::Run(run) => out.push_str(&run_xml(run)),
            Inline::Unknown(raw) => out.push_str(raw.as_str()),
        }
    }
    element("w:p", &out)
}

fn table(table: &Table) -> String {
    let mut out = raws(&table.raw_props);
    for row in &table.rows {
        let mut row_xml = raws(&row.raw_props);
        for cell in &row.cells {
            let mut cell_xml = raws(&cell.raw_props);
            for block in &cell.blocks {
                cell_xml.push_str(&block_xml(block));
            }
            row_xml.push_str(&element("w:tc", &cell_xml));
        }
        out.push_str(&element("w:tr", &row_xml));
    }
    element("w:tbl", &out)
}

/// `w:pPr`; пустая строка, если записывать нечего.
pub(crate) fn paragraph_props(props: &ParagraphProps, section: Option<&SectionProps>) -> String {
    let mut children = Children::new(PPR_ORDER);
    if let Some(style) = &props.style {
        children.push("pStyle", val("w:pStyle", style));
    }
    if let Some(numbering) = props.numbering {
        let mut inner = String::new();
        if let Some(level) = numbering.level {
            inner.push_str(&val("w:ilvl", &level.to_string()));
        }
        if let Some(id) = numbering.id {
            inner.push_str(&val("w:numId", &id.to_string()));
        }
        children.push("numPr", element("w:numPr", &inner));
    }
    let toggles = [
        ("keepNext", props.keep_next),
        ("keepLines", props.keep_lines),
        ("pageBreakBefore", props.page_break_before),
        ("widowControl", props.widow_control),
    ];
    for (name, value) in toggles {
        if let Some(on) = value {
            children.push(name, toggle(&format!("w:{name}"), on));
        }
    }
    if !props.tabs.is_empty() {
        let mut tabs = String::new();
        for stop in &props.tabs {
            let mut attrs = vec![(
                "w:val",
                name_of(TAB_KINDS, stop.kind).unwrap_or("left").to_string(),
            )];
            if stop.leader != Default::default() {
                attrs.push((
                    "w:leader",
                    name_of(TAB_LEADERS, stop.leader)
                        .unwrap_or("none")
                        .to_string(),
                ));
            }
            attrs.push(("w:pos", stop.position.0.to_string()));
            tabs.push_str(&empty("w:tab", &attrs));
        }
        children.push("tabs", element("w:tabs", &tabs));
    }
    let spacing = &props.spacing;
    let mut attrs = Vec::new();
    if let Some(before) = spacing.before {
        attrs.push(("w:before", before.0.to_string()));
    }
    if let Some(after) = spacing.after {
        attrs.push(("w:after", after.0.to_string()));
    }
    if let Some(line) = spacing.line {
        let (value, rule) = match line {
            LineSpacing::Multiple(n) => (n.to_string(), "auto"),
            LineSpacing::Exact(t) => (t.0.to_string(), "exact"),
            LineSpacing::AtLeast(t) => (t.0.to_string(), "atLeast"),
        };
        attrs.push(("w:line", value));
        attrs.push(("w:lineRule", rule.to_string()));
    }
    if !attrs.is_empty() {
        children.push("spacing", empty("w:spacing", &attrs));
    }
    let ind = &props.indentation;
    let mut attrs = Vec::new();
    if let Some(left) = ind.left {
        attrs.push(("w:left", left.0.to_string()));
    }
    if let Some(right) = ind.right {
        attrs.push(("w:right", right.0.to_string()));
    }
    match ind.first_line {
        Some(FirstLineIndent::FirstLine(t)) => attrs.push(("w:firstLine", t.0.to_string())),
        Some(FirstLineIndent::Hanging(t)) => attrs.push(("w:hanging", t.0.to_string())),
        None => {}
    }
    if !attrs.is_empty() {
        children.push("ind", empty("w:ind", &attrs));
    }
    if let Some(on) = props.contextual_spacing {
        children.push("contextualSpacing", toggle("w:contextualSpacing", on));
    }
    if let Some(name) = props.alignment.and_then(|a| name_of(ALIGNMENTS, a)) {
        children.push("jc", val("w:jc", name));
    }
    if let Some(section) = section {
        children.push("sectPr", section_props(section));
    }
    children.raw(&props.unknown);
    optional_element("w:pPr", &children.finish())
}

fn run_xml(run: &Run) -> String {
    let mut out = run_props(&run.props);
    for item in &run.content {
        match item {
            RunContent::Text(text) => out.push_str(&text_element(text)),
            RunContent::Tab => out.push_str("<w:tab/>"),
            RunContent::Break(BreakKind::Line) => out.push_str("<w:br/>"),
            RunContent::Break(BreakKind::Page) => out.push_str(r#"<w:br w:type="page"/>"#),
            RunContent::Break(BreakKind::Column) => out.push_str(r#"<w:br w:type="column"/>"#),
            RunContent::Unknown(raw) => out.push_str(raw.as_str()),
        }
    }
    element("w:r", &out)
}

/// `w:t`. Пробелы по краям и подряд Word сохраняет только с `xml:space`.
fn text_element(text: &str) -> String {
    let needs_preserve = text.starts_with(char::is_whitespace)
        || text.ends_with(char::is_whitespace)
        || text.contains("  ")
        || text.contains(['\t', '\n', '\r']);
    let text = escape_text(text);
    if needs_preserve {
        format!(r#"<w:t xml:space="preserve">{text}</w:t>"#)
    } else {
        format!("<w:t>{text}</w:t>")
    }
}

/// `w:rPr`; пустая строка, если записывать нечего.
pub(crate) fn run_props(props: &RunProps) -> String {
    let mut children = Children::new(RPR_ORDER);
    if let Some(style) = &props.style {
        children.push("rStyle", val("w:rStyle", style));
    }
    if props.fonts != RunFonts::default() {
        children.push("rFonts", fonts(&props.fonts));
    }
    let toggles = [
        ("b", props.bold),
        ("i", props.italic),
        ("caps", props.caps),
        ("smallCaps", props.small_caps),
        ("strike", props.strike),
        ("dstrike", props.double_strike),
        ("vanish", props.hidden),
    ];
    for (name, value) in toggles {
        if let Some(on) = value {
            children.push(name, toggle(&format!("w:{name}"), on));
        }
    }
    if let Some(color) = props.color {
        let mut attrs = vec![("w:val", hex_color(color))];
        if let Some(theme) = &props.theme_color {
            attrs.push(("w:themeColor", theme.name.clone()));
            if let Some(shade) = theme.shade {
                attrs.push(("w:themeShade", format!("{shade:02X}")));
            }
            if let Some(tint) = theme.tint {
                attrs.push(("w:themeTint", format!("{tint:02X}")));
            }
        }
        children.push("color", empty("w:color", &attrs));
    }
    if let Some(spacing) = props.spacing {
        children.push("spacing", val("w:spacing", &spacing.0.to_string()));
    }
    if let Some(scale) = props.scale {
        children.push("w", val("w:w", &scale.to_string()));
    }
    if let Some(kerning) = props.kerning {
        children.push("kern", val("w:kern", &kerning.0.to_string()));
    }
    if let Some(position) = props.position {
        children.push("position", val("w:position", &position.to_string()));
    }
    if let Some(size) = props.size {
        children.push("sz", val("w:sz", &size.0.to_string()));
    }
    if let Some(name) = props.highlight.and_then(|h| name_of(HIGHLIGHTS, h)) {
        children.push("highlight", val("w:highlight", name));
    }
    if let Some(name) = props.underline.and_then(|u| name_of(UNDERLINES, u)) {
        let mut attrs = vec![("w:val", name.to_string())];
        if let Some(color) = props.underline_color {
            attrs.push(("w:color", hex_color(color)));
        }
        children.push("u", empty("w:u", &attrs));
    }
    if let Some(align) = props.vertical_align {
        let name = match align {
            VerticalAlign::Baseline => "baseline",
            VerticalAlign::Superscript => "superscript",
            VerticalAlign::Subscript => "subscript",
        };
        children.push("vertAlign", val("w:vertAlign", name));
    }
    children.raw(&props.unknown);
    optional_element("w:rPr", &children.finish())
}

/// Цвет как в атрибутах Word: `auto` или `RRGGBB`.
fn hex_color(color: Color) -> String {
    match color {
        Color::Auto => "auto".to_string(),
        Color::Rgb(r, g, b) => format!("{r:02X}{g:02X}{b:02X}"),
    }
}

fn fonts(fonts: &RunFonts) -> String {
    let mut attrs = Vec::new();
    let names = [
        ("w:ascii", &fonts.ascii),
        ("w:hAnsi", &fonts.h_ansi),
        ("w:eastAsia", &fonts.east_asia),
        ("w:cs", &fonts.complex_script),
        ("w:hint", &fonts.hint),
    ];
    for (attr, value) in names {
        if let Some(value) = value {
            attrs.push((attr, value.clone()));
        }
    }
    let themes = [
        ("w:asciiTheme", fonts.ascii_theme),
        ("w:hAnsiTheme", fonts.h_ansi_theme),
        ("w:eastAsiaTheme", fonts.east_asia_theme),
        ("w:cstheme", fonts.complex_script_theme),
    ];
    for (attr, value) in themes {
        if let Some(name) = value.and_then(|t| name_of(THEME_FONTS, t)) {
            attrs.push((attr, name.to_string()));
        }
    }
    empty("w:rFonts", &attrs)
}

fn section_props(props: &SectionProps) -> String {
    let mut children = Children::new(SECTPR_ORDER);
    if let Some(size) = props.page_size {
        let mut attrs = vec![
            ("w:w", size.width.0.to_string()),
            ("w:h", size.height.0.to_string()),
        ];
        if size.orientation == Orientation::Landscape {
            attrs.push(("w:orient", "landscape".to_string()));
        }
        children.push("pgSz", empty("w:pgSz", &attrs));
    }
    if let Some(m) = props.margins {
        let attrs = [
            ("w:top", m.top),
            ("w:right", m.right),
            ("w:bottom", m.bottom),
            ("w:left", m.left),
            ("w:header", m.header),
            ("w:footer", m.footer),
            ("w:gutter", m.gutter),
        ]
        .map(|(name, value)| (name, value.0.to_string()));
        children.push("pgMar", empty("w:pgMar", &attrs));
    }
    children.raw(&props.unknown);
    element("w:sectPr", &children.finish())
}

/// Часть стилей (`word/styles.xml`).
pub(crate) fn styles_xml(styles: &Styles, original_root: Option<&[(String, String)]>) -> String {
    let mut children = Children::new(STYLES_ORDER);
    let defaults = &styles.defaults;
    let run = run_props(&defaults.run);
    let paragraph = paragraph_props(&defaults.paragraph, None);
    if !run.is_empty() || !paragraph.is_empty() {
        let mut out = String::new();
        if !run.is_empty() {
            out.push_str(&element("w:rPrDefault", &run));
        }
        if !paragraph.is_empty() {
            out.push_str(&element("w:pPrDefault", &paragraph));
        }
        children.push("docDefaults", element("w:docDefaults", &out));
    }
    for style in &styles.styles {
        children.push("style", style_xml(style));
    }
    children.raw(&styles.unknown);
    let attrs = root_attributes(original_root);
    xml_part(&element_with("w:styles", &attrs, &children.finish()))
}

fn style_xml(style: &Style) -> String {
    let kind = match style.kind {
        StyleKind::Paragraph => "paragraph",
        StyleKind::Character => "character",
        StyleKind::Table => "table",
        StyleKind::Numbering => "numbering",
    };
    let mut attrs = vec![("w:type", kind.to_string())];
    if style.is_default {
        attrs.push(("w:default", "1".to_string()));
    }
    if style.custom {
        attrs.push(("w:customStyle", "1".to_string()));
    }
    attrs.push(("w:styleId", style.id.clone()));

    let mut children = Children::new(STYLE_ORDER);
    if let Some(name) = &style.name {
        children.push("name", val("w:name", name));
    }
    if let Some(based_on) = &style.based_on {
        children.push("basedOn", val("w:basedOn", based_on));
    }
    if let Some(next) = &style.next {
        children.push("next", val("w:next", next));
    }
    let paragraph = paragraph_props(&style.paragraph, None);
    if !paragraph.is_empty() {
        children.push("pPr", paragraph);
    }
    let run = run_props(&style.run);
    if !run.is_empty() {
        children.push("rPr", run);
    }
    children.raw(&style.unknown);
    let attrs: Vec<(String, String)> = attrs
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect();
    element_with("w:style", &attrs, &children.finish())
}

/// Дочерние элементы, которые нужно записать в порядке схемы.
pub(crate) struct Children {
    order: &'static [&'static str],
    items: Vec<(usize, String)>,
}

impl Children {
    pub(crate) fn new(order: &'static [&'static str]) -> Children {
        Children {
            order,
            items: Vec::new(),
        }
    }

    /// Место элемента по таблице; неизвестные — в конец.
    fn rank(&self, local: &str) -> usize {
        self.order
            .iter()
            .position(|name| *name == local)
            .unwrap_or(self.order.len())
    }

    pub(crate) fn push(&mut self, local: &str, xml: String) {
        self.items.push((self.rank(local), xml));
    }

    /// Добавляет сохранённые элементы. Их место определяется по имени,
    /// если они из пространства имён WordprocessingML; элементы из других
    /// пространств (расширения новых версий Word) идут в конец.
    pub(crate) fn raw(&mut self, raws: &[RawXml]) {
        for raw in raws {
            let rank = match xml::parse(raw.as_str().as_bytes()) {
                Ok(e) if is_w_ns(e.ns.as_deref()) => self.rank(&e.local),
                _ => self.order.len(),
            };
            self.items.push((rank, raw.as_str().to_string()));
        }
    }

    pub(crate) fn finish(mut self) -> String {
        // Сортировка устойчивая: одинаковые по месту элементы
        // сохраняют исходный порядок.
        self.items.sort_by_key(|(rank, _)| *rank);
        self.items.into_iter().map(|(_, xml)| xml).collect()
    }
}

fn raws(raws: &[RawXml]) -> String {
    raws.iter().map(RawXml::as_str).collect()
}

/// Добавляет к корневому элементу заголовок XML.
pub(crate) fn xml_part(root: &str) -> String {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n{root}")
}

pub(crate) fn element(name: &str, content: &str) -> String {
    if content.is_empty() {
        format!("<{name}/>")
    } else {
        format!("<{name}>{content}</{name}>")
    }
}

/// Элемент, который не пишется вовсе, если он пуст.
fn optional_element(name: &str, content: &str) -> String {
    if content.is_empty() {
        String::new()
    } else {
        element(name, content)
    }
}

pub(crate) fn element_with(name: &str, attrs: &[(String, String)], content: &str) -> String {
    let mut out = format!("<{name}");
    for (attr, value) in attrs {
        out.push_str(&format!(" {attr}=\"{}\"", escape_attr(value)));
    }
    if content.is_empty() {
        out.push_str("/>");
    } else {
        out.push_str(&format!(">{content}</{name}>"));
    }
    out
}

pub(crate) fn empty(name: &str, attrs: &[(&str, String)]) -> String {
    let mut out = format!("<{name}");
    for (attr, value) in attrs {
        out.push_str(&format!(" {attr}=\"{}\"", escape_attr(value)));
    }
    out.push_str("/>");
    out
}

pub(crate) fn val(name: &str, value: &str) -> String {
    empty(name, &[("w:val", value.to_string())])
}

/// Переключатель: `<w:b/>` или `<w:b w:val="0"/>`.
pub(crate) fn toggle(name: &str, on: bool) -> String {
    if on {
        format!("<{name}/>")
    } else {
        val(name, "0")
    }
}

/// Символы, которые нельзя записать в XML 1.0 даже ссылкой (`&#1;` и т. п.).
fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

/// Экранирует текст элемента. Недопустимые в XML символы выбрасываются;
/// `\r` пишется ссылкой, иначе при чтении он превратится в `\n`.
pub(crate) fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().filter(|&c| is_xml_char(c)) {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
    out
}

/// Экранирует значение атрибута. Табуляции и переводы строк пишутся
/// ссылками, иначе при чтении они превратятся в пробелы.
pub(crate) fn escape_attr(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars().filter(|&c| is_xml_char(c)) {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::{Spacing, Twips};

    const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

    #[test]
    fn known_and_unknown_properties_follow_schema_order() {
        let props = ParagraphProps {
            alignment: Some(letter_model::Alignment::Center),
            spacing: Spacing {
                after: Some(Twips(0)),
                ..Spacing::default()
            },
            unknown: vec![
                RawXml::new(
                    r#"<w14:ext xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"/>"#,
                ),
                RawXml::new(format!(r#"<w:rPr {W}><w:b/></w:rPr>"#)),
                RawXml::new(format!(r#"<w:keepNext {W}/>"#)),
            ],
            ..ParagraphProps::default()
        };
        let xml = paragraph_props(&props, Some(&SectionProps::default()));
        let order: Vec<_> = [
            "keepNext",
            "w:spacing",
            "w:jc",
            "w:rPr",
            "w:sectPr",
            "w14:ext",
        ]
        .iter()
        .map(|name| xml.find(name).unwrap_or_else(|| panic!("{name} missing")))
        .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{xml}");
    }

    #[test]
    fn empty_properties_are_not_written() {
        assert_eq!(paragraph_props(&ParagraphProps::default(), None), "");
        assert_eq!(run_props(&RunProps::default()), "");
        assert_eq!(section_props(&SectionProps::default()), "<w:sectPr/>");
    }

    #[test]
    fn text_is_escaped() {
        assert_eq!(text_element("a<b>&c"), "<w:t>a&lt;b&gt;&amp;c</w:t>");
        assert_eq!(text_element(" x"), r#"<w:t xml:space="preserve"> x</w:t>"#);
        // Управляющие символы запрещены в XML — их выбрасываем.
        assert_eq!(text_element("a\u{1}b"), "<w:t>ab</w:t>");
        assert_eq!(escape_attr("\"a\"\tb\n"), "&quot;a&quot;&#9;b&#10;");
        assert_eq!(escape_text("\r"), "&#13;");
    }

    #[test]
    fn written_document_parses() {
        let doc = Document::new();
        let xml = document_xml(&doc);
        assert!(xml.starts_with("<?xml"));
        let root = xml::parse(xml.as_bytes()).unwrap();
        assert!(root.is_w("document"));
    }
}
