//! Разбор главной части документа (`word/document.xml`) в модель.
//!
//! Правило: элемент, который понят полностью (имя и все атрибуты),
//! превращается в поля модели. Всё остальное сохраняется целиком как
//! [`RawXml`], чтобы при записи вернуть его на место без потерь.

use letter_model::{
    Block, BreakKind, Color, Document, FirstLineIndent, HalfPoints, Indentation, Inline,
    LineSpacing, Orientation, PageMargins, PageSize, Paragraph, ParagraphProps, RawXml, Run,
    RunContent, RunFonts, RunProps, Section, SectionProps, Spacing, TabStop, Table, TableCell,
    TableRow, ThemeColor, Twips, VerticalAlign,
};

use crate::ReadError;
use crate::names::{
    ALIGNMENTS, HIGHLIGHTS, TAB_KINDS, TAB_LEADERS, THEME_FONTS, UNDERLINES, lookup,
};
use crate::xml::Element;

/// Пространство имён WordprocessingML (обычный вариант OOXML).
pub(crate) const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
/// Пространство имён WordprocessingML в строгом (Strict) варианте OOXML.
pub(crate) const W_NS_STRICT: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";

pub(crate) fn is_w_ns(ns: Option<&str>) -> bool {
    matches!(ns, Some(W_NS | W_NS_STRICT))
}

impl Element {
    /// Элемент `w:<local>`.
    pub(crate) fn is_w(&self, local: &str) -> bool {
        self.local == local && is_w_ns(self.ns.as_deref())
    }

    /// Атрибут `w:<local>` (или без префикса — так пишут некоторые программы).
    pub(crate) fn w_attr(&self, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.local == local && (a.ns.is_none() || is_w_ns(a.ns.as_deref())))
            .map(|a| a.value.as_str())
    }

    /// Все атрибуты элемента входят в список понятных.
    pub(crate) fn only_attrs(&self, allowed: &[&str]) -> bool {
        self.attrs.iter().all(|a| {
            (a.ns.is_none() || is_w_ns(a.ns.as_deref())) && allowed.contains(&a.local.as_str())
        })
    }

    pub(crate) fn raw(&self) -> RawXml {
        RawXml::new(self.to_xml_fragment())
    }
}

/// Превращает корневой элемент `w:document` в модель.
pub(crate) fn read_document(root: &Element) -> Result<Document, ReadError> {
    if !root.is_w("document") {
        return Err(ReadError::xml(format!(
            "main part starts with <{}> instead of <w:document>",
            root.qname
        )));
    }
    let mut doc = Document::new();
    doc.sections.clear();
    for child in root.elements() {
        if child.is_w("body") {
            doc.sections = read_body(child);
        } else {
            doc.unknown.push(child.raw());
        }
    }
    if doc.sections.is_empty() {
        doc.sections.push(Section::default());
    }
    Ok(doc)
}

/// Тело документа. Разделы в .docx записаны необычно: параметры каждого
/// раздела, кроме последнего, лежат в `w:pPr/w:sectPr` последнего абзаца
/// раздела, а параметры последнего — в `w:sectPr` в конце тела.
fn read_body(body: &Element) -> Vec<Section> {
    let mut sections = Vec::new();
    let mut current = Section::default();
    let mut has_final_props = false;
    for child in body.elements() {
        if child.is_w("p") {
            let (paragraph, section_props) = read_paragraph(child, true);
            current.blocks.push(paragraph.into());
            if let Some(props) = section_props {
                current.props = props;
                sections.push(std::mem::take(&mut current));
            }
        } else if child.is_w("sectPr") {
            current.props = read_section_props(child);
            has_final_props = true;
        } else {
            current.blocks.push(read_block(child));
        }
    }
    if has_final_props || !current.blocks.is_empty() || sections.is_empty() {
        sections.push(current);
    }
    sections
}

/// Блок внутри ячейки таблицы или любой блок, кроме абзаца, в теле.
fn read_block(element: &Element) -> Block {
    if element.is_w("p") {
        read_paragraph(element, false).0.into()
    } else if element.is_w("tbl") {
        read_table(element)
    } else {
        Block::Unknown(element.raw())
    }
}

/// Абзац. Если `allow_section`, параметры раздела из `w:pPr/w:sectPr`
/// возвращаются отдельно; иначе (внутри таблицы) сохраняются как непонятые.
fn read_paragraph(p: &Element, allow_section: bool) -> (Paragraph, Option<SectionProps>) {
    let mut paragraph = Paragraph::default();
    let mut section = None;
    for child in p.elements() {
        if child.is_w("pPr") {
            let (props, section_props) = read_paragraph_props(child, allow_section);
            paragraph.props = props;
            section = section_props;
        } else if child.is_w("r") {
            paragraph.content.push(Inline::Run(read_run(child)));
        } else {
            paragraph.content.push(Inline::Unknown(child.raw()));
        }
    }
    (paragraph, section)
}

pub(crate) fn read_paragraph_props(
    ppr: &Element,
    allow_section: bool,
) -> (ParagraphProps, Option<SectionProps>) {
    let mut props = ParagraphProps::default();
    let mut section = None;
    for child in ppr.elements() {
        let understood = is_w_ns(child.ns.as_deref())
            && match child.local.as_str() {
                "pStyle" => set(&mut props.style, string_val(child)),
                "jc" => set(&mut props.alignment, enum_val(child, ALIGNMENTS)),
                "ind" => read_indentation(child)
                    .map(|i| props.indentation = i)
                    .is_some(),
                "spacing" => read_spacing(child).map(|s| props.spacing = s).is_some(),
                "contextualSpacing" => set(&mut props.contextual_spacing, toggle(child)),
                "keepNext" => set(&mut props.keep_next, toggle(child)),
                "keepLines" => set(&mut props.keep_lines, toggle(child)),
                "pageBreakBefore" => set(&mut props.page_break_before, toggle(child)),
                "widowControl" => set(&mut props.widow_control, toggle(child)),
                "tabs" if props.tabs.is_empty() => {
                    read_tabs(child).map(|t| props.tabs = t).is_some()
                }
                "sectPr" if allow_section => {
                    section = Some(read_section_props(child));
                    true
                }
                _ => false,
            };
        if !understood {
            props.unknown.push(child.raw());
        }
    }
    (props, section)
}

fn read_indentation(ind: &Element) -> Option<Indentation> {
    if !ind.only_attrs(&["left", "start", "right", "end", "firstLine", "hanging"]) {
        return None;
    }
    let measure = |names: &[&str]| -> Result<Option<Twips>, ()> {
        match names.iter().find_map(|n| ind.w_attr(n)) {
            Some(value) => parse_twips(value).map(Some).ok_or(()),
            None => Ok(None),
        }
    };
    let left = measure(&["left", "start"]).ok()?;
    let right = measure(&["right", "end"]).ok()?;
    // Если заданы оба, Word использует выступ.
    let first_line = match (measure(&["hanging"]).ok()?, measure(&["firstLine"]).ok()?) {
        (Some(h), _) => Some(FirstLineIndent::Hanging(h)),
        (None, Some(f)) => Some(FirstLineIndent::FirstLine(f)),
        (None, None) => None,
    };
    Some(Indentation {
        left,
        right,
        first_line,
    })
}

/// `w:tabs`. `None`, если список пуст или в нём что-то непонятное:
/// тогда он сохраняется как есть.
fn read_tabs(tabs: &Element) -> Option<Vec<TabStop>> {
    if !tabs.attrs.is_empty() {
        return None;
    }
    let mut stops = Vec::new();
    for tab in tabs.elements() {
        if !tab.is_w("tab") || !tab.only_attrs(&["val", "pos", "leader"]) {
            return None;
        }
        stops.push(TabStop {
            position: parse_twips(tab.w_attr("pos")?)?,
            kind: lookup(TAB_KINDS, tab.w_attr("val")?)?,
            leader: match tab.w_attr("leader") {
                Some(leader) => lookup(TAB_LEADERS, leader)?,
                None => Default::default(),
            },
        });
    }
    (!stops.is_empty()).then_some(stops)
}

fn read_spacing(spacing: &Element) -> Option<Spacing> {
    if !spacing.only_attrs(&["before", "after", "line", "lineRule"]) {
        return None;
    }
    let measure = |name: &str| -> Result<Option<Twips>, ()> {
        match spacing.w_attr(name) {
            Some(value) => parse_twips(value).map(Some).ok_or(()),
            None => Ok(None),
        }
    };
    let line = match (spacing.w_attr("line"), spacing.w_attr("lineRule")) {
        (None, None) => None,
        (None, Some(_)) => return None,
        (Some(value), rule) => {
            let twips = parse_twips(value)?;
            Some(match rule.unwrap_or("auto") {
                "auto" => LineSpacing::Multiple(u32::try_from(twips.0).ok()?),
                "exact" => LineSpacing::Exact(twips),
                "atLeast" => LineSpacing::AtLeast(twips),
                _ => return None,
            })
        }
    };
    Some(Spacing {
        before: measure("before").ok()?,
        after: measure("after").ok()?,
        line,
    })
}

fn read_run(r: &Element) -> Run {
    let mut run = Run::default();
    for child in r.elements() {
        let item = if !is_w_ns(child.ns.as_deref()) {
            RunContent::Unknown(child.raw())
        } else {
            match child.local.as_str() {
                "rPr" => {
                    run.props = read_run_props(child);
                    continue;
                }
                "t" => RunContent::Text(child.text()),
                "tab" if child.attrs.is_empty() => RunContent::Tab,
                "cr" if child.attrs.is_empty() => RunContent::Break(BreakKind::Line),
                "br" if child.only_attrs(&["type"]) => match child.w_attr("type") {
                    None | Some("textWrapping") => RunContent::Break(BreakKind::Line),
                    Some("page") => RunContent::Break(BreakKind::Page),
                    Some("column") => RunContent::Break(BreakKind::Column),
                    Some(_) => RunContent::Unknown(child.raw()),
                },
                _ => RunContent::Unknown(child.raw()),
            }
        };
        run.content.push(item);
    }
    run
}

pub(crate) fn read_run_props(rpr: &Element) -> RunProps {
    let mut props = RunProps::default();
    for child in rpr.elements() {
        let understood = is_w_ns(child.ns.as_deref())
            && match child.local.as_str() {
                "rStyle" => set(&mut props.style, string_val(child)),
                "rFonts" => read_fonts(child).map(|f| props.fonts = f).is_some(),
                "b" => set(&mut props.bold, toggle(child)),
                "i" => set(&mut props.italic, toggle(child)),
                "strike" => set(&mut props.strike, toggle(child)),
                "dstrike" => set(&mut props.double_strike, toggle(child)),
                "u" => set(&mut props.underline, enum_val(child, UNDERLINES)),
                "color" => read_color(child)
                    .map(|(color, theme)| {
                        props.color = Some(color);
                        props.theme_color = theme;
                    })
                    .is_some(),
                "highlight" => set(&mut props.highlight, enum_val(child, HIGHLIGHTS)),
                "sz" => set(
                    &mut props.size,
                    single_val(child).and_then(parse_half_points),
                ),
                "vertAlign" => set(
                    &mut props.vertical_align,
                    single_val(child).and_then(|v| match v {
                        "baseline" => Some(VerticalAlign::Baseline),
                        "superscript" => Some(VerticalAlign::Superscript),
                        "subscript" => Some(VerticalAlign::Subscript),
                        _ => None,
                    }),
                ),
                _ => false,
            };
        if !understood {
            props.unknown.push(child.raw());
        }
    }
    props
}

/// Шрифты: имена, ссылки на шрифты темы и подсказка `w:hint`.
fn read_fonts(fonts: &Element) -> Option<RunFonts> {
    let names = [
        "ascii",
        "hAnsi",
        "eastAsia",
        "cs",
        "asciiTheme",
        "hAnsiTheme",
        "eastAsiaTheme",
        "cstheme",
        "hint",
    ];
    if !fonts.only_attrs(&names) {
        return None;
    }
    let get = |name: &str| fonts.w_attr(name).map(str::to_string);
    // Неизвестное имя шрифта темы — повод сохранить элемент целиком.
    let theme = |name: &str| -> Result<Option<_>, ()> {
        match fonts.w_attr(name) {
            Some(value) => lookup(THEME_FONTS, value).map(Some).ok_or(()),
            None => Ok(None),
        }
    };
    Some(RunFonts {
        ascii: get("ascii"),
        h_ansi: get("hAnsi"),
        east_asia: get("eastAsia"),
        complex_script: get("cs"),
        ascii_theme: theme("asciiTheme").ok()?,
        h_ansi_theme: theme("hAnsiTheme").ok()?,
        east_asia_theme: theme("eastAsiaTheme").ok()?,
        complex_script_theme: theme("cstheme").ok()?,
        hint: get("hint"),
    })
}

fn read_table(tbl: &Element) -> Block {
    let mut table = Table::default();
    for child in tbl.elements() {
        if child.is_w("tblPr") || child.is_w("tblGrid") {
            table.raw_props.push(child.raw());
        } else if child.is_w("tr") {
            match read_row(child) {
                Some(row) => table.rows.push(row),
                None => return Block::Unknown(tbl.raw()),
            }
        } else {
            // Закладки, элементы управления и прочее между строками —
            // пока сохраняем таблицу целиком (таблицы — задача F5.2).
            return Block::Unknown(tbl.raw());
        }
    }
    Block::Table(table)
}

fn read_row(tr: &Element) -> Option<TableRow> {
    let mut row = TableRow::default();
    for child in tr.elements() {
        if child.is_w("trPr") || child.is_w("tblPrEx") {
            row.raw_props.push(child.raw());
        } else if child.is_w("tc") {
            row.cells.push(read_cell(child));
        } else {
            return None;
        }
    }
    Some(row)
}

fn read_cell(tc: &Element) -> TableCell {
    let mut cell = TableCell::default();
    for child in tc.elements() {
        if child.is_w("tcPr") {
            cell.raw_props.push(child.raw());
        } else {
            cell.blocks.push(read_block(child));
        }
    }
    cell
}

fn read_section_props(sect: &Element) -> SectionProps {
    let mut props = SectionProps::default();
    for child in sect.elements() {
        let understood = is_w_ns(child.ns.as_deref())
            && match child.local.as_str() {
                "pgSz" => set(&mut props.page_size, read_page_size(child)),
                "pgMar" => set(&mut props.margins, read_margins(child)),
                _ => false,
            };
        if !understood {
            props.unknown.push(child.raw());
        }
    }
    props
}

fn read_page_size(size: &Element) -> Option<PageSize> {
    if !size.only_attrs(&["w", "h", "orient"]) {
        return None;
    }
    Some(PageSize {
        width: parse_twips(size.w_attr("w")?)?,
        height: parse_twips(size.w_attr("h")?)?,
        orientation: match size.w_attr("orient") {
            None | Some("portrait") => Orientation::Portrait,
            Some("landscape") => Orientation::Landscape,
            Some(_) => return None,
        },
    })
}

fn read_margins(margins: &Element) -> Option<PageMargins> {
    let names = [
        "top", "right", "bottom", "left", "header", "footer", "gutter",
    ];
    if !margins.only_attrs(&names) {
        return None;
    }
    let get = |name: &str| margins.w_attr(name).and_then(parse_twips);
    // Стандарт требует все семь атрибутов, но некоторые программы не пишут
    // колонтитулы и переплёт; тогда считаем их нулевыми.
    let optional = |name: &str| match margins.w_attr(name) {
        Some(value) => parse_twips(value),
        None => Some(Twips::ZERO),
    };
    Some(PageMargins {
        top: get("top")?,
        right: get("right")?,
        bottom: get("bottom")?,
        left: get("left")?,
        header: optional("header")?,
        footer: optional("footer")?,
        gutter: optional("gutter")?,
    })
}

/// Записывает значение, если оно понято. Возвращает, понято ли.
fn set<T>(slot: &mut Option<T>, value: Option<T>) -> bool {
    match value {
        Some(v) => {
            *slot = Some(v);
            true
        }
        None => false,
    }
}

/// Значение `w:val` у элемента, у которого других атрибутов нет.
fn single_val(element: &Element) -> Option<&str> {
    if element.only_attrs(&["val"]) {
        element.w_attr("val")
    } else {
        None
    }
}

fn string_val(element: &Element) -> Option<String> {
    single_val(element).map(str::to_string)
}

fn enum_val<T: Copy>(element: &Element, table: &[(&str, T)]) -> Option<T> {
    single_val(element).and_then(|v| lookup(table, v))
}

/// Переключатель (`ST_OnOff`): без `w:val` означает «включено».
fn toggle(element: &Element) -> Option<bool> {
    if !element.only_attrs(&["val"]) {
        return None;
    }
    match element.w_attr("val") {
        None | Some("true" | "1" | "on") => Some(true),
        Some("false" | "0" | "off") => Some(false),
        Some(_) => None,
    }
}

/// Цвет `w:color`: значение и, возможно, ссылка на цвет темы.
fn read_color(element: &Element) -> Option<(Color, Option<ThemeColor>)> {
    if !element.only_attrs(&["val", "themeColor", "themeShade", "themeTint"]) {
        return None;
    }
    let color = parse_color(element.w_attr("val")?)?;
    let byte = |name: &str| -> Result<Option<u8>, ()> {
        match element.w_attr(name) {
            Some(v) => u8::from_str_radix(v, 16).map(Some).map_err(|_| ()),
            None => Ok(None),
        }
    };
    let theme = match element.w_attr("themeColor") {
        Some(name) => Some(ThemeColor {
            name: name.to_string(),
            shade: byte("themeShade").ok()?,
            tint: byte("themeTint").ok()?,
        }),
        None if element.w_attr("themeShade").is_some() || element.w_attr("themeTint").is_some() => {
            return None;
        }
        None => None,
    };
    Some((color, theme))
}

fn parse_color(value: &str) -> Option<Color> {
    match value {
        "auto" => Some(Color::Auto),
        hex if hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) => {
            let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
            Some(Color::Rgb(byte(0)?, byte(2)?, byte(4)?))
        }
        _ => None,
    }
}

/// Расстояние в твипах. Новая редакция стандарта разрешает и единицы:
/// `12pt`, `1in`, `2.5cm`, `10mm`, `1pc`/`1pi` (пика = 12 пт).
pub(crate) fn parse_twips(value: &str) -> Option<Twips> {
    let value = value.trim();
    if let Ok(n) = value.parse::<i32>() {
        return Some(Twips(n));
    }
    let points = universal_measure_points(value)?;
    let twips = (points * f64::from(Twips::PER_POINT)).round();
    (f64::from(i32::MIN)..=f64::from(i32::MAX))
        .contains(&twips)
        .then_some(Twips(twips as i32))
}

/// Размер шрифта в полупунктах (или с единицами, см. [`parse_twips`]).
fn parse_half_points(value: &str) -> Option<HalfPoints> {
    let value = value.trim();
    if let Ok(n) = value.parse::<u32>() {
        return Some(HalfPoints(n));
    }
    let half_points = (universal_measure_points(value)? * 2.0).round();
    (0.0..=f64::from(u32::MAX))
        .contains(&half_points)
        .then_some(HalfPoints(half_points as u32))
}

fn universal_measure_points(value: &str) -> Option<f64> {
    let units: [(&str, f64); 6] = [
        ("pt", 1.0),
        ("in", 72.0),
        ("cm", 72.0 / 2.54),
        ("mm", 72.0 / 25.4),
        ("pc", 12.0),
        ("pi", 12.0),
    ];
    let (number, factor) = units
        .iter()
        .find_map(|(suffix, factor)| value.strip_suffix(suffix).map(|n| (n, *factor)))?;
    let number: f64 = number.parse().ok()?;
    number.is_finite().then_some(number * factor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml;
    use letter_model::{Alignment, Highlight, TabLeader, TabStopKind, Underline};

    const NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml""#;

    fn read_body_xml(body: &str) -> Document {
        let xml = format!(r#"<w:document {NS}><w:body>{body}</w:body></w:document>"#);
        read_document(&xml::parse(xml.as_bytes()).unwrap()).unwrap()
    }

    fn first_paragraph(doc: &Document) -> &Paragraph {
        match doc.blocks().next() {
            Some(Block::Paragraph(p)) => p,
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn run_properties() {
        let doc = read_body_xml(
            r#"<w:p><w:r><w:rPr>
                <w:rStyle w:val="Strong"/>
                <w:rFonts w:ascii="Arial" w:hAnsi="Arial"/>
                <w:b/><w:i w:val="0"/><w:strike w:val="true"/><w:dstrike w:val="off"/>
                <w:u w:val="wave"/><w:color w:val="C00000"/><w:highlight w:val="yellow"/>
                <w:sz w:val="32"/><w:vertAlign w:val="superscript"/>
                <w:lang w:val="ru-RU"/>
            </w:rPr><w:t>x</w:t></w:r></w:p>"#,
        );
        let run = first_paragraph(&doc).runs().next().unwrap();
        let p = &run.props;
        assert_eq!(p.style.as_deref(), Some("Strong"));
        assert_eq!(p.fonts, RunFonts::new("Arial"));
        assert_eq!(p.bold, Some(true));
        assert_eq!(p.italic, Some(false));
        assert_eq!(p.strike, Some(true));
        assert_eq!(p.double_strike, Some(false));
        assert_eq!(p.underline, Some(Underline::Wave));
        assert_eq!(p.color, Some(Color::Rgb(0xC0, 0, 0)));
        assert_eq!(p.highlight, Some(Highlight::Yellow));
        assert_eq!(p.size, Some(HalfPoints(32)));
        assert_eq!(p.vertical_align, Some(VerticalAlign::Superscript));
        assert_eq!(p.unknown.len(), 1);
        assert_eq!(p.unknown[0].local_name(), "lang");
    }

    #[test]
    fn partially_understood_properties_are_kept_whole() {
        let doc = read_body_xml(
            r#"<w:p><w:r><w:rPr>
                <w:rFonts w:asciiTheme="bogusTheme" w:ascii="Calibri"/>
                <w:color w:val="FF0000" w:themeShade="80"/>
                <w:u w:val="bogus"/>
            </w:rPr></w:r></w:p>"#,
        );
        let p = &first_paragraph(&doc).runs().next().unwrap().props;
        assert_eq!(p.fonts, RunFonts::default());
        assert_eq!(p.color, None);
        assert_eq!(p.underline, None);
        let names: Vec<_> = p.unknown.iter().map(RawXml::local_name).collect();
        assert_eq!(names, ["rFonts", "color", "u"]);
    }

    #[test]
    fn theme_fonts_and_colors() {
        let doc = read_body_xml(
            r#"<w:p><w:r><w:rPr>
                <w:rFonts w:asciiTheme="majorHAnsi" w:hAnsiTheme="majorHAnsi" w:cstheme="minorBidi" w:hint="eastAsia"/>
                <w:color w:val="365F91" w:themeColor="accent1" w:themeShade="BF"/>
            </w:rPr></w:r></w:p>"#,
        );
        let p = &first_paragraph(&doc).runs().next().unwrap().props;
        assert_eq!(
            p.fonts.ascii_theme,
            Some(letter_model::ThemeFont::MajorHAnsi)
        );
        assert_eq!(
            p.fonts.complex_script_theme,
            Some(letter_model::ThemeFont::MinorBidi)
        );
        assert_eq!(p.fonts.hint.as_deref(), Some("eastAsia"));
        assert_eq!(p.color, Some(Color::Rgb(0x36, 0x5F, 0x91)));
        let theme = p.theme_color.as_ref().unwrap();
        assert_eq!(
            (theme.name.as_str(), theme.shade, theme.tint),
            ("accent1", Some(0xBF), None)
        );
        assert!(p.unknown.is_empty());
    }

    #[test]
    fn run_content() {
        let doc = read_body_xml(
            r#"<w:p><w:r><w:t xml:space="preserve"> a </w:t><w:tab/><w:br/><w:br w:type="page"/><w:cr/><w:br w:type="column"/><w:br w:clear="all"/><w:drawing/><w:t>b&amp;c</w:t></w:r></w:p>"#,
        );
        let run = first_paragraph(&doc).runs().next().unwrap();
        assert_eq!(run.plain_text(), " a \t\n\u{c}\n\u{c}b&c");
        assert!(matches!(&run.content[6], RunContent::Unknown(r) if r.local_name() == "br"));
        assert!(matches!(&run.content[7], RunContent::Unknown(r) if r.local_name() == "drawing"));
    }

    #[test]
    fn paragraph_properties() {
        let doc = read_body_xml(
            r#"<w:p><w:pPr>
                <w:pStyle w:val="Heading1"/><w:jc w:val="both"/>
                <w:ind w:left="720" w:right="360" w:hanging="360" w:firstLine="100"/>
                <w:spacing w:before="240" w:after="120" w:line="276" w:lineRule="auto"/>
                <w:contextualSpacing/><w:keepNext/><w:keepLines w:val="0"/>
                <w:pageBreakBefore/><w:widowControl w:val="false"/>
                <w:tabs><w:tab w:val="clear" w:pos="720"/><w:tab w:val="end" w:leader="dot" w:pos="9350"/></w:tabs>
                <w:framePr/>
            </w:pPr></w:p>"#,
        );
        let p = &first_paragraph(&doc).props;
        assert_eq!(p.style.as_deref(), Some("Heading1"));
        assert_eq!(p.alignment, Some(Alignment::Justify));
        assert_eq!(p.indentation.left, Some(Twips(720)));
        assert_eq!(p.indentation.right, Some(Twips(360)));
        assert_eq!(
            p.indentation.first_line,
            Some(FirstLineIndent::Hanging(Twips(360)))
        );
        assert_eq!(p.spacing.before, Some(Twips(240)));
        assert_eq!(p.spacing.after, Some(Twips(120)));
        assert_eq!(p.spacing.line, Some(LineSpacing::Multiple(276)));
        assert_eq!(p.contextual_spacing, Some(true));
        assert_eq!(p.keep_next, Some(true));
        assert_eq!(p.keep_lines, Some(false));
        assert_eq!(p.page_break_before, Some(true));
        assert_eq!(p.widow_control, Some(false));
        assert_eq!(
            p.tabs,
            [
                TabStop {
                    position: Twips(720),
                    kind: TabStopKind::Clear,
                    leader: TabLeader::None,
                },
                TabStop {
                    position: Twips(9350),
                    kind: TabStopKind::Right,
                    leader: TabLeader::Dot,
                },
            ]
        );
        assert_eq!(p.unknown.len(), 1);
    }

    #[test]
    fn odd_tab_lists_are_kept_as_is() {
        for tabs in [
            "<w:tabs/>",
            r#"<w:tabs><w:tab w:val="num" w:pos="1"/></w:tabs>"#,
            r#"<w:tabs><w:tab w:val="left"/></w:tabs>"#,
            r#"<w:tabs><w:tab w:val="left" w:pos="1" w:leader="stars"/></w:tabs>"#,
        ] {
            let doc = read_body_xml(&format!("<w:p><w:pPr>{tabs}</w:pPr></w:p>"));
            let p = &first_paragraph(&doc).props;
            assert!(p.tabs.is_empty(), "{tabs}");
            assert_eq!(p.unknown.len(), 1, "{tabs}");
        }
    }

    #[test]
    fn line_spacing_rules() {
        let line = |attrs: &str| {
            let doc = read_body_xml(&format!(
                r#"<w:p><w:pPr><w:spacing {attrs}/></w:pPr></w:p>"#
            ));
            first_paragraph(&doc).props.spacing.line
        };
        assert_eq!(line(r#"w:line="240""#), Some(LineSpacing::SINGLE));
        assert_eq!(
            line(r#"w:line="300" w:lineRule="exact""#),
            Some(LineSpacing::Exact(Twips(300)))
        );
        assert_eq!(
            line(r#"w:line="12pt" w:lineRule="atLeast""#),
            Some(LineSpacing::AtLeast(Twips(240)))
        );
        assert_eq!(line(r#"w:line="-5""#), None);
    }

    #[test]
    fn unknown_inlines_and_blocks() {
        let doc = read_body_xml(
            r#"<w:p w14:paraId="1"><w:bookmarkStart w:id="0" w:name="b"/><w:r><w:t>x</w:t></w:r><w:bookmarkEnd w:id="0"/></w:p>
               <w:sdt><w:sdtContent><w:p/></w:sdtContent></w:sdt>"#,
        );
        let p = first_paragraph(&doc);
        assert_eq!(p.content.len(), 3);
        assert!(matches!(&p.content[0], Inline::Unknown(r) if r.local_name() == "bookmarkStart"));
        let blocks: Vec<_> = doc.blocks().collect();
        assert!(matches!(blocks[1], Block::Unknown(r) if r.local_name() == "sdt"));
        // Фрагмент несёт объявление пространства имён и снова читается.
        if let Inline::Unknown(raw) = &p.content[0] {
            assert!(raw.as_str().contains("xmlns:w="));
            assert!(xml::parse(raw.as_str().as_bytes()).is_ok());
        }
    }

    #[test]
    fn sections() {
        let doc = read_body_xml(
            r#"<w:p><w:r><w:t>one</w:t></w:r></w:p>
               <w:p><w:pPr><w:sectPr><w:pgSz w:w="16838" w:h="11906" w:orient="landscape"/></w:sectPr></w:pPr></w:p>
               <w:p><w:r><w:t>two</w:t></w:r></w:p>
               <w:sectPr>
                 <w:headerReference w:type="default" r:id="rId1" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"/>
                 <w:pgSz w:w="11906" w:h="16838"/>
                 <w:pgMar w:top="1134" w:right="850" w:bottom="1134" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/>
                 <w:cols w:space="708"/>
               </w:sectPr>"#,
        );
        assert_eq!(doc.sections.len(), 2);
        assert_eq!(doc.sections[0].blocks.len(), 2);
        assert_eq!(
            doc.sections[0].props.page_size,
            Some(PageSize::A4.rotated())
        );
        let last = &doc.sections[1].props;
        assert_eq!(last.page_size, Some(PageSize::A4));
        assert_eq!(last.margins, Some(PageMargins::default()));
        let names: Vec<_> = last.unknown.iter().map(RawXml::local_name).collect();
        assert_eq!(names, ["headerReference", "cols"]);
        assert_eq!(doc.plain_text(), "one\n\ntwo");
    }

    #[test]
    fn margins_without_header_footer_gutter() {
        let doc = read_body_xml(
            r#"<w:sectPr><w:pgMar w:top="1" w:right="2" w:bottom="3" w:left="4"/></w:sectPr>"#,
        );
        let margins = doc.sections[0].props.margins.unwrap();
        assert_eq!(margins.left, Twips(4));
        assert_eq!(margins.gutter, Twips::ZERO);
        let broken = read_body_xml(r#"<w:sectPr><w:pgMar w:top="1" w:right="2"/></w:sectPr>"#);
        assert_eq!(broken.sections[0].props.margins, None);
    }

    #[test]
    fn tables() {
        let doc = read_body_xml(
            r#"<w:tbl><w:tblPr/><w:tblGrid/>
                 <w:tr><w:trPr/><w:tc><w:tcPr/><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr>
               </w:tbl>
               <w:tbl><w:tr><w:tc><w:p/></w:tc></w:tr><w:bookmarkEnd w:id="1"/></w:tbl>"#,
        );
        let blocks: Vec<_> = doc.blocks().collect();
        let Block::Table(table) = blocks[0] else {
            panic!("expected table");
        };
        assert_eq!(table.raw_props.len(), 2);
        assert_eq!(table.rows[0].raw_props.len(), 1);
        assert_eq!(table.rows[0].cells[0].raw_props.len(), 1);
        assert_eq!(blocks[0].plain_text(), "a\tb");
        assert!(matches!(blocks[1], Block::Unknown(_)));
    }

    #[test]
    fn document_level_unknowns_and_strict_namespace() {
        let xml = r#"<w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"><w:background w:color="FFFF00"/><w:body><w:p><w:r><w:t>s</w:t></w:r></w:p></w:body></w:document>"#;
        let doc = read_document(&xml::parse(xml.as_bytes()).unwrap()).unwrap();
        assert_eq!(doc.plain_text(), "s");
        assert_eq!(doc.unknown.len(), 1);
    }

    #[test]
    fn empty_or_wrong_root() {
        let empty = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"/>"#;
        let doc = read_document(&xml::parse(empty.as_bytes()).unwrap()).unwrap();
        assert_eq!(doc.sections.len(), 1);
        assert!(read_document(&xml::parse(b"<x/>").unwrap()).is_err());
    }

    #[test]
    fn measures() {
        assert_eq!(parse_twips("1440"), Some(Twips(1440)));
        assert_eq!(parse_twips("-720"), Some(Twips(-720)));
        assert_eq!(parse_twips("1in"), Some(Twips(1440)));
        assert_eq!(parse_twips("2.54cm"), Some(Twips(1440)));
        assert_eq!(parse_twips("1pc"), Some(Twips(240)));
        assert_eq!(parse_twips("abc"), None);
        assert_eq!(parse_twips("1e300pt"), None);
        assert_eq!(parse_half_points("11pt"), Some(HalfPoints(22)));
        assert_eq!(parse_half_points("-1pt"), None);
    }
}
