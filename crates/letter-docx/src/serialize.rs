//! Запись модели в XML: главная часть документа и стили.
//!
//! Word строго следит за порядком дочерних элементов (его задаёт схема
//! OOXML). Понятые свойства и сохранённые без разбора ([`RawXml`])
//! перемешиваются, поэтому все дочерние элементы сначала собираются
//! вместе, а потом упорядочиваются по таблицам ниже.

use letter_model::{
    Block, BreakKind, Color, Columns, Document, FieldChar, FieldCharKind, FirstLineIndent,
    HeaderFooterKind, Hyperlink, Inline, LineSpacing, Note, NoteKind, Orientation,
    PageBorderDisplay, PageBorders, Paragraph, ParagraphProps, RawXml, Run, RunContent, RunFonts,
    RunProps, Section, SectionProps, SectionStart, Style, StyleKind, Styles, VerticalAlign,
};

use crate::document::{W_NS, is_w_ns};
use crate::names::{
    ALIGNMENTS, HIGHLIGHTS, TAB_KINDS, TAB_LEADERS, THEME_FONTS, UNDERLINES, name_of,
};
use crate::table::{border_lines, paragraph_borders_xml, shading_xml};
use crate::xml;

/// Пространство имён связей (`r:id`).
pub(crate) const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

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
    // Ссылки на колонтитулы и рисунки пишутся с префиксом `r:`.
    if !attrs.iter().any(|(name, _)| name == "xmlns:r") {
        attrs.push(("xmlns:r".into(), R_NS.into()));
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
    NEW_PICTURES.with(|n| n.set(0));
    let mut children = Children::new(DOCUMENT_ORDER);
    if let Some(color) = doc.background {
        children.push(
            "background",
            empty("w:background", &[("w:color", hex_color(color))]),
        );
    }
    children.raw(&doc.unknown);
    children.push("body", body(&doc.sections));
    xml_part(&element_with("w:document", &attrs, &children.finish()))
}

/// Часть колонтитула (`w:hdr` или `w:ftr`) с атрибутами корня `root`
/// из исходного файла.
pub(crate) fn header_footer_xml(
    blocks: &[Block],
    header: bool,
    root: Option<&[(String, String)]>,
) -> String {
    let attrs = root_attributes(root);
    let mut content: String = blocks.iter().map(block_xml).collect();
    // Колонтитул обязан содержать хотя бы один абзац.
    if blocks.is_empty() {
        content = paragraph(&Paragraph::default(), None);
    }
    let name = if header { "w:hdr" } else { "w:ftr" };
    xml_part(&element_with(name, &attrs, &content))
}

/// Часть сносок (`w:footnotes`) или концевых сносок (`w:endnotes`);
/// `root` — атрибуты корня из исходного файла.
pub(crate) fn notes_xml(
    notes: &[Note],
    kind: NoteKind,
    root: Option<&[(String, String)]>,
) -> String {
    let attrs = root_attributes(root);
    let (part, item) = match kind {
        NoteKind::Footnote => ("w:footnotes", "w:footnote"),
        NoteKind::Endnote => ("w:endnotes", "w:endnote"),
    };
    let mut content = String::new();
    for note in notes {
        let special = note
            .special
            .as_ref()
            .map(|s| format!(r#" w:type="{}""#, escape_attr(s)))
            .unwrap_or_default();
        let mut inner: String = note.blocks.iter().map(block_xml).collect();
        // Сноска обязана содержать хотя бы один абзац.
        if note.blocks.is_empty() {
            inner = paragraph(&Paragraph::default(), None);
        }
        content.push_str(&format!(
            r#"<{item}{special} w:id="{}">{inner}</{item}>"#,
            escape_attr(&note.id)
        ));
    }
    xml_part(&element_with(part, &attrs, &content))
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

pub(crate) fn block_xml(block: &Block) -> String {
    match block {
        Block::Paragraph(p) => paragraph(p, None),
        Block::Table(t) => crate::table::table_xml(t),
        Block::Unknown(raw) => raw.as_str().to_string(),
    }
}

fn paragraph(p: &Paragraph, section: Option<&SectionProps>) -> String {
    let mut out = paragraph_props(&p.props, section);
    let mut i = 0;
    while i < p.content.len() {
        let link = match &p.content[i] {
            Inline::Run(run) => run.props.link.as_ref(),
            _ => None,
        };
        let Some(link) = link else {
            out.push_str(&inline_xml(&p.content[i]));
            i += 1;
            continue;
        };
        // Соседние фрагменты с той же ссылкой (и закладки между ними) —
        // внутри одного `w:hyperlink`.
        let mut end = i + 1;
        let mut last_run = i;
        while let Some(next) = p.content.get(end) {
            match next {
                Inline::Run(run) if run.props.link.as_ref() == Some(link) => last_run = end,
                Inline::BookmarkStart(_) | Inline::BookmarkEnd(_) => {}
                _ => break,
            }
            end += 1;
        }
        let inner: String = p.content[i..=last_run].iter().map(inline_xml).collect();
        out.push_str(&format!(
            "<w:hyperlink{}>{inner}</w:hyperlink>",
            hyperlink_attributes(link)
        ));
        i = last_run + 1;
    }
    element("w:p", &out)
}

fn inline_xml(inline: &Inline) -> String {
    match inline {
        Inline::Run(run) => run_xml(run),
        Inline::BookmarkStart(bookmark) => format!(
            r#"<w:bookmarkStart w:id="{}" w:name="{}"/>"#,
            escape_attr(&bookmark.id),
            escape_attr(&bookmark.name)
        ),
        Inline::BookmarkEnd(id) => format!(r#"<w:bookmarkEnd w:id="{}"/>"#, escape_attr(id)),
        Inline::Unknown(raw) => raw.as_str().to_string(),
    }
}

/// Атрибуты `w:hyperlink`. Адрес пишется связью `r:id`: её заранее
/// ставит запись пакета.
fn hyperlink_attributes(link: &Hyperlink) -> String {
    let mut out = String::new();
    if let Some(id) = &link.relationship {
        out.push_str(&format!(r#" r:id="{}""#, escape_attr(id)));
    }
    for (name, value) in [
        ("anchor", &link.anchor),
        ("tooltip", &link.tooltip),
        ("tgtFrame", &link.frame),
    ] {
        if let Some(value) = value {
            out.push_str(&format!(r#" w:{name}="{}""#, escape_attr(value)));
        }
    }
    if link.history {
        out.push_str(r#" w:history="1""#);
    }
    out
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
    if let Some(borders) = &props.borders {
        children.push("pBdr", paragraph_borders_xml(borders));
    }
    if let Some(shading) = &props.shading {
        children.push("shd", shading_xml(shading));
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

thread_local! {
    /// Сколько новых рисунков уже записано в документ: по этому счётчику
    /// им даются номера `wp:docPr`, которые не совпадают с номерами
    /// рисунков из исходного файла (Word нумерует их с 1).
    static NEW_PICTURES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn run_xml(run: &Run) -> String {
    let mut out = run_props(&run.props);
    for item in &run.content {
        match item {
            RunContent::Text(text) => out.push_str(&text_xml(text)),
            RunContent::Tab => out.push_str("<w:tab/>"),
            RunContent::Symbol(s) => out.push_str(&format!(
                r#"<w:sym w:font="{}" w:char="{:04X}"/>"#,
                escape_attr(&s.font),
                s.code
            )),
            RunContent::Break(BreakKind::Line) => out.push_str("<w:br/>"),
            RunContent::Break(BreakKind::Page) => out.push_str(r#"<w:br w:type="page"/>"#),
            RunContent::Break(BreakKind::Column) => out.push_str(r#"<w:br w:type="column"/>"#),
            RunContent::Picture(picture) => {
                let id = NEW_PICTURES.with(|n| {
                    n.set(n.get() + 1);
                    n.get()
                });
                out.push_str(&crate::picture::picture_xml(picture, 10_000 + id));
            }
            RunContent::NoteReference(r) => {
                let name = match r.kind {
                    NoteKind::Footnote => "w:footnoteReference",
                    NoteKind::Endnote => "w:endnoteReference",
                };
                let custom = if r.custom_mark {
                    r#" w:customMarkFollows="1""#
                } else {
                    ""
                };
                out.push_str(&format!(
                    r#"<{name}{custom} w:id="{}"/>"#,
                    escape_attr(&r.id)
                ));
            }
            RunContent::NoteMark(NoteKind::Footnote) => out.push_str("<w:footnoteRef/>"),
            RunContent::NoteMark(NoteKind::Endnote) => out.push_str("<w:endnoteRef/>"),
            RunContent::FieldChar(c) => out.push_str(&field_char(c)),
            RunContent::FieldCode(code) => out.push_str(&format!(
                r#"<w:instrText xml:space="preserve">{}</w:instrText>"#,
                escape_text(code)
            )),
            RunContent::Unknown(raw) => out.push_str(raw.as_str()),
        }
    }
    element("w:r", &out)
}

/// `w:fldChar`: знак начала, разделителя или конца поля.
fn field_char(c: &FieldChar) -> String {
    let kind = match c.kind {
        FieldCharKind::Begin => "begin",
        FieldCharKind::Separate => "separate",
        FieldCharKind::End => "end",
    };
    let mut out = format!(r#"<w:fldChar w:fldCharType="{kind}""#);
    if c.dirty {
        out.push_str(r#" w:dirty="1""#);
    }
    if c.locked {
        out.push_str(r#" w:fldLock="1""#);
    }
    out.push_str("/>");
    out
}

/// `w:t`. Пробелы по краям и подряд Word сохраняет только с `xml:space`.
/// Текст с неразрывными дефисами (U+2011) и мягкими переносами (U+00AD):
/// они записываются отдельными элементами `w:noBreakHyphen` и
/// `w:softHyphen`, как в Word.
fn text_xml(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find(['\u{2011}', '\u{ad}']) {
        if i > 0 {
            out.push_str(&text_element(&rest[..i]));
        }
        let c = rest[i..].chars().next().unwrap_or_default();
        out.push_str(if c == '\u{ad}' {
            "<w:softHyphen/>"
        } else {
            "<w:noBreakHyphen/>"
        });
        rest = &rest[i + c.len_utf8()..];
    }
    if !rest.is_empty() || out.is_empty() {
        out.push_str(&text_element(rest));
    }
    out
}

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
pub(crate) fn hex_color(color: Color) -> String {
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
    if let Some(start) = props.start {
        let name = match start {
            SectionStart::NextPage => "nextPage",
            SectionStart::Continuous => "continuous",
            SectionStart::EvenPage => "evenPage",
            SectionStart::OddPage => "oddPage",
            SectionStart::NextColumn => "nextColumn",
        };
        children.push("type", val("w:type", name));
    }
    if let Some(columns) = &props.columns {
        children.push("cols", columns_xml(columns));
    }
    for (set, name, element) in [
        (&props.headers, "headerReference", "w:headerReference"),
        (&props.footers, "footerReference", "w:footerReference"),
    ] {
        for (kind, story) in set.iter() {
            let Some(id) = &story.relationship else {
                continue;
            };
            let kind = match kind {
                HeaderFooterKind::Default => "default",
                HeaderFooterKind::First => "first",
                HeaderFooterKind::Even => "even",
            };
            let attrs = [("w:type", kind.to_string()), ("r:id", id.clone())];
            children.push(name, empty(element, &attrs));
        }
    }
    if props.title_page {
        children.push("titlePg", empty("w:titlePg", &[]));
    }
    if let Some(borders) = &props.page_borders {
        children.push("pgBorders", page_borders_xml(borders));
    }
    children.raw(&props.unknown);
    element("w:sectPr", &children.finish())
}

/// Границы страниц `w:pgBorders`.
fn page_borders_xml(borders: &PageBorders) -> String {
    let mut attrs = Vec::new();
    if borders.from_page_edge {
        attrs.push(("w:offsetFrom", "page".to_string()));
    }
    if borders.behind_text {
        attrs.push(("w:zOrder", "back".to_string()));
    }
    match borders.display {
        PageBorderDisplay::AllPages => {}
        PageBorderDisplay::FirstPage => attrs.push(("w:display", "firstPage".to_string())),
        PageBorderDisplay::NotFirstPage => attrs.push(("w:display", "notFirstPage".to_string())),
    }
    let attrs: Vec<(String, String)> = attrs
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect();
    let lines = border_lines([
        ("w:top", &borders.top),
        ("w:left", &borders.left),
        ("w:bottom", &borders.bottom),
        ("w:right", &borders.right),
    ]);
    element_with("w:pgBorders", &attrs, &lines)
}

/// Колонки раздела `w:cols`.
fn columns_xml(columns: &Columns) -> String {
    let mut attrs = vec![
        ("w:num".to_string(), columns.count.max(1).to_string()),
        ("w:space".to_string(), columns.space.0.to_string()),
    ];
    if columns.separator {
        attrs.push(("w:sep".to_string(), "1".to_string()));
    }
    if columns.widths.is_empty() {
        return element_with("w:cols", &attrs, "");
    }
    attrs.push(("w:equalWidth".to_string(), "0".to_string()));
    let content: String = columns
        .widths
        .iter()
        .map(|(width, space)| {
            empty(
                "w:col",
                &[
                    ("w:w", width.0.to_string()),
                    ("w:space", space.0.to_string()),
                ],
            )
        })
        .collect();
    element_with("w:cols", &attrs, &content)
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
    let table = crate::table::table_props(&style.table);
    if !table.is_empty() {
        children.push("tblPr", element("w:tblPr", &table));
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

pub(crate) fn raws(raws: &[RawXml]) -> String {
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
        assert_eq!(
            text_xml("a\u{2011}b\u{ad}"),
            "<w:t>a</w:t><w:noBreakHyphen/><w:t>b</w:t><w:softHyphen/>"
        );
        assert_eq!(text_xml(""), "<w:t></w:t>");
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
