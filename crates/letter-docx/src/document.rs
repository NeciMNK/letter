//! Разбор главной части документа (`word/document.xml`) в модель.
//!
//! Правило: элемент, который понят полностью (имя и все атрибуты),
//! превращается в поля модели. Всё остальное сохраняется целиком как
//! [`RawXml`], чтобы при записи вернуть его на место без потерь.

use std::sync::Arc;

use letter_model::{
    Block, Bookmark, BreakKind, Color, Columns, ContentControl, ContentControlRef, Document,
    FieldChar, FieldCharKind, FirstLineIndent, HalfPoints, HeaderFooter, HeaderFooterKind,
    HeaderFooters, Hyperlink, Indentation, Inline, LineSpacing, Note, NoteKind, NoteReference,
    NumberingRef, Orientation, PageBorderDisplay, PageBorders, PageMargins, PageSize, Paragraph,
    ParagraphProps, RawXml, Run, RunContent, RunFonts, RunProps, Section, SectionProps,
    SectionStart, Spacing, Symbol, TabStop, ThemeColor, Twips, Underline, VerticalAlign,
};

use crate::ReadError;
use crate::names::{
    ALIGNMENTS, HIGHLIGHTS, TAB_KINDS, TAB_LEADERS, THEME_FONTS, UNDERLINES, lookup,
};
use crate::serialize::R_NS;
use crate::table::{read_border, read_paragraph_borders, read_shading};
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
        } else if child.is_w("background")
            && doc.background.is_none()
            && child.only_attrs(&["color"])
            && child.elements().next().is_none()
            && let Some(color) = child.w_attr("color").and_then(parse_color)
        {
            doc.background = Some(color);
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
    let mut children = Vec::new();
    for child in body.elements() {
        flatten_block_controls(child, &[], &mut children);
    }
    for (child, controls) in children {
        if child.is_w("p") {
            let (mut paragraph, section_props) = read_paragraph(child, true);
            paragraph.props.controls = controls;
            current.blocks.push(paragraph.into());
            if let Some(props) = section_props {
                current.props = props;
                sections.push(std::mem::take(&mut current));
            }
        } else if child.is_w("sectPr") {
            current.props = read_section_props(child);
            has_final_props = true;
        } else {
            current
                .blocks
                .push(with_controls(read_block(child), controls));
        }
    }
    if has_final_props || !current.blocks.is_empty() || sections.is_empty() {
        sections.push(current);
    }
    sections
}

/// Блоки из элемента содержимого (ячейки, колонтитула, сноски): обычно
/// один блок, а у элемента управления содержимым — все блоки внутри него.
pub(crate) fn read_blocks(element: &Element, out: &mut Vec<Block>) {
    let mut children = Vec::new();
    flatten_block_controls(element, &[], &mut children);
    for (child, controls) in children {
        out.push(with_controls(read_block(child), controls));
    }
}

/// Все блоки дочерних элементов `parent` (см. [`read_blocks`]).
pub(crate) fn read_child_blocks(parent: &Element) -> Vec<Block> {
    let mut blocks = Vec::new();
    for child in parent.elements() {
        read_blocks(child, &mut blocks);
    }
    blocks
}

/// Ставит абзацу или таблице ссылки на элементы управления содержимым.
fn with_controls(mut block: Block, controls: Vec<ContentControlRef>) -> Block {
    match &mut block {
        Block::Paragraph(p) => p.props.controls = controls,
        Block::Table(t) => t.props.controls = controls,
        Block::Unknown(_) => {}
    }
    block
}

/// Раскрывает блочный элемент управления содержимым (`w:sdt`): кладёт
/// в `out` его абзацы и таблицы со ссылками на элемент (и на внешние
/// элементы `outer`). Остальные элементы и `w:sdt`, которые нельзя
/// раскрыть без потерь, кладутся как есть.
fn flatten_block_controls<'e>(
    element: &'e Element,
    outer: &[ContentControlRef],
    out: &mut Vec<(&'e Element, Vec<ContentControlRef>)>,
) {
    let Some((control, content)) = block_control(element) else {
        out.push((element, outer.to_vec()));
        return;
    };
    let mut controls = outer.to_vec();
    controls.push(control);
    for child in content.elements() {
        flatten_block_controls(child, &controls, out);
    }
}

/// Блочный `w:sdt`, который можно раскрыть: свойства и `w:sdtContent`,
/// внутри только абзацы, таблицы и такие же `w:sdt`, и хотя бы один из них.
fn block_control(sdt: &Element) -> Option<(ContentControlRef, &Element)> {
    let (control, content) = control_parts(sdt)?;
    let mut any = false;
    for child in content.elements() {
        if !(child.is_w("p") || child.is_w("tbl") || block_control(child).is_some()) {
            return None;
        }
        any = true;
    }
    any.then_some((control, content))
}

/// Части `w:sdt`: свойства элемента и `w:sdtContent`. `None`, если
/// у `w:sdt` атрибуты, лишние или повторённые части.
fn control_parts(sdt: &Element) -> Option<(ContentControlRef, &Element)> {
    if !sdt.is_w("sdt") || !sdt.attrs.is_empty() {
        return None;
    }
    let mut control = ContentControl::default();
    let mut content = None;
    for child in sdt.elements() {
        if child.is_w("sdtPr") && control.props.is_none() && content.is_none() {
            control.props = Some(child.raw());
        } else if child.is_w("sdtEndPr") && control.end_props.is_none() && content.is_none() {
            control.end_props = Some(child.raw());
        } else if child.is_w("sdtContent") && content.is_none() && child.attrs.is_empty() {
            content = Some(child);
        } else {
            return None;
        }
    }
    Some((Arc::new(control), content?))
}

/// Элемент управления содержимым внутри абзаца (`w:sdt` с фрагментами
/// текста): фрагменты со ссылкой на элемент. `None`, если внутри есть
/// что-то, кроме фрагментов, или он пуст.
fn read_inline_control(sdt: &Element) -> Option<Vec<Inline>> {
    let (control, content) = control_parts(sdt)?;
    let mut items = Vec::new();
    for child in content.elements() {
        if !child.is_w("r") {
            return None;
        }
        let mut run = read_run(child);
        run.props.control = Some(control.clone());
        items.push(Inline::Run(run));
    }
    (!items.is_empty()).then_some(items)
}

/// Блок внутри ячейки таблицы или любой блок, кроме абзаца, в теле.
fn read_block(element: &Element) -> Block {
    if element.is_w("p") {
        read_paragraph(element, false).0.into()
    } else if element.is_w("tbl") {
        crate::table::read_table(element)
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
        } else if let Some(runs) = simple_field(child) {
            paragraph.content.extend(runs.into_iter().map(Inline::Run));
        } else if let Some(items) = read_hyperlink(child) {
            paragraph.content.extend(items);
        } else if let Some(mark) = read_bookmark(child) {
            paragraph.content.push(mark);
        } else if let Some(items) = read_inline_control(child) {
            paragraph.content.extend(items);
        } else if let Some(mark) = read_comment_mark(child) {
            paragraph.content.push(mark);
        } else {
            paragraph.content.push(Inline::Unknown(child.raw()));
        }
    }
    (paragraph, section)
}

/// Начало или конец закладки; с другими атрибутами (закладка столбцов
/// таблицы и т. п.) — `None`.
fn read_bookmark(element: &Element) -> Option<Inline> {
    if element.elements().next().is_some() {
        return None;
    }
    if element.is_w("bookmarkStart") && element.only_attrs(&["id", "name"]) {
        Some(Inline::BookmarkStart(Bookmark {
            id: element.w_attr("id")?.to_string(),
            name: element.w_attr("name")?.to_string(),
        }))
    } else if element.is_w("bookmarkEnd") && element.only_attrs(&["id"]) {
        Some(Inline::BookmarkEnd(element.w_attr("id")?.to_string()))
    } else {
        None
    }
}

/// Начало или конец текста примечания; с другими атрибутами — `None`.
fn read_comment_mark(element: &Element) -> Option<Inline> {
    if element.elements().next().is_some() || !element.only_attrs(&["id"]) {
        return None;
    }
    let id = element.w_attr("id")?.to_string();
    if element.is_w("commentRangeStart") {
        Some(Inline::CommentStart(id))
    } else if element.is_w("commentRangeEnd") {
        Some(Inline::CommentEnd(id))
    } else {
        None
    }
}

/// Гиперссылка `w:hyperlink`: её фрагменты получают ссылку в оформлении
/// (адрес по связи `r:id` подставляется после чтения связей). Ссылка
/// с чем-то, кроме фрагментов, простых полей и закладок, остаётся как есть.
fn read_hyperlink(element: &Element) -> Option<Vec<Inline>> {
    if !element.is_w("hyperlink") {
        return None;
    }
    let mut link = Hyperlink::default();
    for attribute in &element.attrs {
        let value = Some(attribute.value.clone());
        if attribute.ns.as_deref() == Some(crate::serialize::R_NS) && attribute.local == "id" {
            link.relationship = value;
            continue;
        }
        if attribute.ns.is_some() && !is_w_ns(attribute.ns.as_deref()) {
            return None;
        }
        match attribute.local.as_str() {
            "anchor" => link.anchor = value,
            "tooltip" => link.tooltip = value,
            "tgtFrame" => link.frame = value,
            "history" => {
                link.history = match attribute.value.as_str() {
                    "1" | "true" | "on" => true,
                    "0" | "false" | "off" => false,
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
    let mut items = Vec::new();
    let with_link = |mut run: Run| {
        run.props.link = Some(link.clone());
        Inline::Run(run)
    };
    for child in element.elements() {
        if child.is_w("r") {
            items.push(with_link(read_run(child)));
        } else if let Some(runs) = simple_field(child) {
            items.extend(runs.into_iter().map(with_link));
        } else {
            items.push(read_bookmark(child).or_else(|| read_comment_mark(child))?);
        }
    }
    Some(items)
}

/// Простое поле `w:fldSimple` превращается в такое же поле из знаков
/// поля (`w:fldChar`): начало, код, разделитель, результат, конец —
/// так его результат виден и правится как обычный текст, а номер
/// страницы подставляется при вёрстке. Поле с чем-то, кроме фрагментов
/// текста, остаётся как есть.
fn simple_field(element: &Element) -> Option<Vec<Run>> {
    if !element.is_w("fldSimple") || !element.only_attrs(&["instr"]) {
        return None;
    }
    let instruction = element.w_attr("instr")?;
    let mut result = Vec::new();
    for child in element.elements() {
        if !child.is_w("r") {
            return None;
        }
        result.push(read_run(child));
    }
    let props = result.first().map(|r| r.props.clone()).unwrap_or_default();
    let marker = |item: RunContent| Run {
        props: props.clone(),
        content: vec![item],
    };
    let mark = |kind| marker(RunContent::FieldChar(FieldChar::new(kind)));
    let mut runs = vec![
        mark(FieldCharKind::Begin),
        marker(RunContent::FieldCode(instruction.to_string())),
        mark(FieldCharKind::Separate),
    ];
    runs.extend(result);
    runs.push(mark(FieldCharKind::End));
    Some(runs)
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
                "numPr" => set(&mut props.numbering, read_numbering_ref(child)),
                "jc" => set(&mut props.alignment, enum_val(child, ALIGNMENTS)),
                "ind" => read_indentation(child)
                    .map(|i| props.indentation = i)
                    .is_some(),
                "spacing" => read_spacing(child).map(|s| props.spacing = s).is_some(),
                "contextualSpacing" => set(&mut props.contextual_spacing, toggle(child)),
                "keepNext" => set(&mut props.keep_next, toggle(child)),
                "keepLines" => set(&mut props.keep_lines, toggle(child)),
                "suppressAutoHyphens" => set(&mut props.suppress_auto_hyphens, toggle(child)),
                "pageBreakBefore" => set(&mut props.page_break_before, toggle(child)),
                "widowControl" => set(&mut props.widow_control, toggle(child)),
                "tabs" if props.tabs.is_empty() => {
                    read_tabs(child).map(|t| props.tabs = t).is_some()
                }
                "pBdr" => set(&mut props.borders, read_paragraph_borders(child)),
                "shd" => set(&mut props.shading, read_shading(child)),
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

/// `w:numPr`: номер списка и уровень. Если внутри есть что-то ещё
/// (например, правка `w:ins`), элемент сохраняется как есть.
fn read_numbering_ref(element: &Element) -> Option<NumberingRef> {
    if !element.attrs.is_empty() {
        return None;
    }
    let mut numbering = NumberingRef::default();
    for child in element.elements() {
        let number = || {
            child
                .only_attrs(&["val"])
                .then(|| child.w_attr("val"))
                .flatten()
                .and_then(|v| v.trim().parse::<u32>().ok())
        };
        if child.is_w("numId") && numbering.id.is_none() {
            numbering.id = Some(number()?);
        } else if child.is_w("ilvl") && numbering.level.is_none() {
            numbering.level = Some(u8::try_from(number()?).ok()?);
        } else {
            return None;
        }
    }
    Some(numbering)
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
                "noBreakHyphen" if child.attrs.is_empty() => RunContent::Text("\u{2011}".into()),
                "softHyphen" if child.attrs.is_empty() => RunContent::Text("\u{ad}".into()),
                "sym" => match read_symbol(child) {
                    Some(s) => RunContent::Symbol(s),
                    None => RunContent::Unknown(child.raw()),
                },
                "cr" if child.attrs.is_empty() => RunContent::Break(BreakKind::Line),
                "drawing" => match crate::picture::read_picture(child) {
                    Some(picture) => RunContent::Picture(picture),
                    None => RunContent::Unknown(child.raw()),
                },
                "br" if child.only_attrs(&["type"]) => match child.w_attr("type") {
                    None | Some("textWrapping") => RunContent::Break(BreakKind::Line),
                    Some("page") => RunContent::Break(BreakKind::Page),
                    Some("column") => RunContent::Break(BreakKind::Column),
                    Some(_) => RunContent::Unknown(child.raw()),
                },
                "footnoteReference" | "endnoteReference"
                    if child.only_attrs(&["id", "customMarkFollows"])
                        && child.elements().next().is_none() =>
                {
                    match read_note_reference(child) {
                        Some(r) => RunContent::NoteReference(r),
                        None => RunContent::Unknown(child.raw()),
                    }
                }
                "commentReference"
                    if child.only_attrs(&["id"]) && child.elements().next().is_none() =>
                {
                    match child.w_attr("id") {
                        Some(id) => RunContent::CommentReference(id.to_string()),
                        None => RunContent::Unknown(child.raw()),
                    }
                }
                "footnoteRef" if child.attrs.is_empty() => RunContent::NoteMark(NoteKind::Footnote),
                "endnoteRef" if child.attrs.is_empty() => RunContent::NoteMark(NoteKind::Endnote),
                "fldChar" => match read_field_char(child) {
                    Some(c) => RunContent::FieldChar(c),
                    None => RunContent::Unknown(child.raw()),
                },
                "instrText"
                    if child.elements().next().is_none()
                        && child.attrs.iter().all(|a| a.local == "space") =>
                {
                    RunContent::FieldCode(child.text())
                }
                _ => RunContent::Unknown(child.raw()),
            }
        };
        run.content.push(item);
    }
    run
}

/// Знак из шрифта `w:sym`, если у него понятные шрифт и номер и нет
/// ничего лишнего.
fn read_symbol(element: &Element) -> Option<Symbol> {
    if !element.only_attrs(&["font", "char"]) || element.elements().next().is_some() {
        return None;
    }
    let font = element.w_attr("font")?.to_string();
    let code = u32::from_str_radix(element.w_attr("char")?, 16).ok()?;
    char::from_u32(code)?;
    Some(Symbol { font, code })
}

/// Ссылка на сноску или концевую сноску.
fn read_note_reference(element: &Element) -> Option<NoteReference> {
    let kind = if element.local == "footnoteReference" {
        NoteKind::Footnote
    } else {
        NoteKind::Endnote
    };
    let custom_mark = match element.w_attr("customMarkFollows") {
        None | Some("0" | "false" | "off") => false,
        Some("1" | "true" | "on") => true,
        Some(_) => return None,
    };
    Some(NoteReference {
        kind,
        id: element.w_attr("id")?.to_string(),
        custom_mark,
    })
}

/// Сноски из части `w:footnotes` или `w:endnotes`; `None`, если в части
/// есть что-то непонятное (тогда часть сохраняется как есть).
pub(crate) fn read_notes(root: &Element, kind: NoteKind) -> Option<Vec<Note>> {
    let (part, item) = match kind {
        NoteKind::Footnote => ("footnotes", "footnote"),
        NoteKind::Endnote => ("endnotes", "endnote"),
    };
    if !root.is_w(part) {
        return None;
    }
    let mut notes = Vec::new();
    for child in root.elements() {
        if !child.is_w(item) || !child.only_attrs(&["id", "type"]) {
            return None;
        }
        notes.push(Note {
            id: child.w_attr("id")?.to_string(),
            special: child.w_attr("type").map(str::to_string),
            blocks: read_child_blocks(child),
        });
    }
    Some(notes)
}

/// Знак поля `w:fldChar`; с данными поля формы (`w:ffData`) и другими
/// детьми — `None`.
fn read_field_char(element: &Element) -> Option<FieldChar> {
    if !element.only_attrs(&["fldCharType", "dirty", "fldLock"])
        || element.elements().next().is_some()
    {
        return None;
    }
    let kind = match element.w_attr("fldCharType")? {
        "begin" => FieldCharKind::Begin,
        "separate" => FieldCharKind::Separate,
        "end" => FieldCharKind::End,
        _ => return None,
    };
    let flag = |name: &str| match element.w_attr(name) {
        None | Some("false" | "0" | "off") => Some(false),
        Some("true" | "1" | "on") => Some(true),
        Some(_) => None,
    };
    Some(FieldChar {
        kind,
        dirty: flag("dirty")?,
        locked: flag("fldLock")?,
    })
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
                "caps" => set(&mut props.caps, toggle(child)),
                "smallCaps" => set(&mut props.small_caps, toggle(child)),
                "vanish" => set(&mut props.hidden, toggle(child)),
                "u" => read_underline(child)
                    .map(|(underline, color)| {
                        props.underline = Some(underline);
                        props.underline_color = color;
                    })
                    .is_some(),
                "spacing" => set(
                    &mut props.spacing,
                    single_val(child)
                        .and_then(|v| v.parse::<i32>().ok())
                        .map(Twips),
                ),
                "w" => set(
                    &mut props.scale,
                    single_val(child)
                        .and_then(|v| v.trim_end_matches('%').parse::<u16>().ok())
                        .filter(|w| (1..=600).contains(w)),
                ),
                "position" => set(
                    &mut props.position,
                    single_val(child).and_then(|v| v.parse::<i32>().ok()),
                ),
                "kern" => set(
                    &mut props.kerning,
                    single_val(child).and_then(parse_half_points),
                ),
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

fn read_section_props(sect: &Element) -> SectionProps {
    let mut props = SectionProps::default();
    for child in sect.elements() {
        let understood = is_w_ns(child.ns.as_deref())
            && match child.local.as_str() {
                "pgSz" => set(&mut props.page_size, read_page_size(child)),
                "pgMar" => set(&mut props.margins, read_margins(child)),
                "type" => set(&mut props.start, read_section_start(child)),
                "cols" => set(&mut props.columns, read_columns(child)),
                "headerReference" => read_reference(child, &mut props.headers),
                "footerReference" => read_reference(child, &mut props.footers),
                "titlePg" => toggle(child).map(|on| props.title_page = on).is_some(),
                "pgBorders" => set(&mut props.page_borders, read_page_borders(child)),
                _ => false,
            };
        if !understood {
            props.unknown.push(child.raw());
        }
    }
    props
}

/// `w:pgBorders`: границы страниц. `None`, если внутри непонятное
/// (например, рисунчатая граница) — тогда элемент сохраняется как есть.
fn read_page_borders(element: &Element) -> Option<PageBorders> {
    if !element.only_attrs(&["offsetFrom", "display", "zOrder"]) {
        return None;
    }
    let mut borders = PageBorders {
        from_page_edge: match element.w_attr("offsetFrom") {
            None | Some("text") => false,
            Some("page") => true,
            Some(_) => return None,
        },
        display: match element.w_attr("display") {
            None | Some("allPages") => PageBorderDisplay::AllPages,
            Some("firstPage") => PageBorderDisplay::FirstPage,
            Some("notFirstPage") => PageBorderDisplay::NotFirstPage,
            Some(_) => return None,
        },
        behind_text: match element.w_attr("zOrder") {
            None | Some("front") => false,
            Some("back") => true,
            Some(_) => return None,
        },
        ..PageBorders::default()
    };
    for child in element.elements() {
        if !is_w_ns(child.ns.as_deref()) {
            return None;
        }
        let slot = match child.local.as_str() {
            "top" => &mut borders.top,
            "left" => &mut borders.left,
            "bottom" => &mut borders.bottom,
            "right" => &mut borders.right,
            _ => return None,
        };
        if slot.is_some() {
            return None;
        }
        *slot = Some(read_border(child)?);
    }
    Some(borders)
}

/// Ссылка на колонтитул (`w:headerReference`, `w:footerReference`):
/// запоминает связь, а содержимое читается потом из части колонтитула.
/// `false`, если ссылка непонятна или такой колонтитул уже есть.
fn read_reference(element: &Element, set: &mut HeaderFooters) -> bool {
    let mut kind = Some(HeaderFooterKind::Default);
    let mut id = None;
    for a in &element.attrs {
        match a.local.as_str() {
            "type" if is_w_ns(a.ns.as_deref()) => {
                kind = match a.value.as_str() {
                    "default" => Some(HeaderFooterKind::Default),
                    "first" => Some(HeaderFooterKind::First),
                    "even" => Some(HeaderFooterKind::Even),
                    _ => None,
                }
            }
            "id" if a.ns.as_deref() == Some(R_NS) => id = Some(a.value.clone()),
            _ => return false,
        }
    }
    let (Some(kind), Some(id)) = (kind, id) else {
        return false;
    };
    let slot = set.get_mut(kind);
    if slot.is_some() {
        return false;
    }
    *slot = Some(HeaderFooter {
        blocks: Vec::new(),
        relationship: Some(id),
    });
    true
}

/// Содержимое части колонтитула (`w:hdr` или `w:ftr`); `None`, если
/// корень другой.
pub(crate) fn read_header_footer(root: &Element) -> Option<Vec<Block>> {
    if !root.is_w("hdr") && !root.is_w("ftr") {
        return None;
    }
    Some(read_child_blocks(root))
}

/// Начало раздела `w:type`.
fn read_section_start(element: &Element) -> Option<SectionStart> {
    if !element.only_attrs(&["val"]) {
        return None;
    }
    Some(match element.w_attr("val") {
        None | Some("nextPage") => SectionStart::NextPage,
        Some("continuous") => SectionStart::Continuous,
        Some("evenPage") => SectionStart::EvenPage,
        Some("oddPage") => SectionStart::OddPage,
        Some("nextColumn") => SectionStart::NextColumn,
        Some(_) => return None,
    })
}

/// Колонки `w:cols`. Чего не понимаем — `None`, элемент остаётся как есть.
fn read_columns(cols: &Element) -> Option<Columns> {
    if !cols.only_attrs(&["num", "space", "sep", "equalWidth"]) {
        return None;
    }
    let flag = |name: &str| match cols.w_attr(name) {
        None => Some(None),
        Some("true" | "1" | "on") => Some(Some(true)),
        Some("false" | "0" | "off") => Some(Some(false)),
        Some(_) => None,
    };
    let count = match cols.w_attr("num") {
        Some(n) => n.parse::<u16>().ok()?.max(1),
        None => 1,
    };
    let space = match cols.w_attr("space") {
        Some(v) => parse_twips(v)?,
        // Значение по умолчанию по стандарту — полдюйма.
        None => Twips(720),
    };
    let separator = flag("sep")?.unwrap_or(false);
    let equal = flag("equalWidth")?;
    let mut widths = Vec::new();
    for col in cols.elements() {
        if !col.is_w("col") || !col.only_attrs(&["w", "space"]) {
            return None;
        }
        let width = parse_twips(col.w_attr("w")?)?;
        let after = match col.w_attr("space") {
            Some(v) => parse_twips(v)?,
            None => Twips::ZERO,
        };
        widths.push((width, after));
    }
    // Ширины колонок без `equalWidth="0"` Word не читает: сохраняем как есть.
    if !widths.is_empty() && equal != Some(false) {
        return None;
    }
    if equal == Some(false) && widths.is_empty() {
        return None;
    }
    Some(Columns {
        count,
        space,
        separator,
        widths,
    })
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
pub(crate) fn set<T>(slot: &mut Option<T>, value: Option<T>) -> bool {
    match value {
        Some(v) => {
            *slot = Some(v);
            true
        }
        None => false,
    }
}

/// Значение `w:val` у элемента, у которого других атрибутов нет.
pub(crate) fn single_val(element: &Element) -> Option<&str> {
    if element.only_attrs(&["val"]) {
        element.w_attr("val")
    } else {
        None
    }
}

pub(crate) fn string_val(element: &Element) -> Option<String> {
    single_val(element).map(str::to_string)
}

pub(crate) fn enum_val<T: Copy>(element: &Element, table: &[(&str, T)]) -> Option<T> {
    single_val(element).and_then(|v| lookup(table, v))
}

/// Переключатель (`ST_OnOff`): без `w:val` означает «включено».
pub(crate) fn toggle(element: &Element) -> Option<bool> {
    if !element.only_attrs(&["val"]) {
        return None;
    }
    match element.w_attr("val") {
        None | Some("true" | "1" | "on") => Some(true),
        Some("false" | "0" | "off") => Some(false),
        Some(_) => None,
    }
}

/// Подчёркивание `w:u`: вид и цвет (`w:color`, без ссылок на тему —
/// с ними элемент сохраняется как есть).
fn read_underline(element: &Element) -> Option<(Underline, Option<Color>)> {
    if !element.only_attrs(&["val", "color"]) {
        return None;
    }
    let underline = lookup(UNDERLINES, element.w_attr("val")?)?;
    let color = match element.w_attr("color") {
        None => None,
        Some(value) => Some(parse_color(value)?),
    };
    Some((underline, color))
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

pub(crate) fn parse_color(value: &str) -> Option<Color> {
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
                <w:pageBreakBefore/><w:widowControl w:val="false"/><w:suppressAutoHyphens/>
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
        assert_eq!(p.suppress_auto_hyphens, Some(true));
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
            r#"<w:p w14:paraId="1"><w:permStart w:id="0" w:edGrp="everyone"/><w:r><w:t>x</w:t></w:r><w:permEnd w:id="0"/></w:p>
               <w:sdt><w:sdtContent><w:bookmarkStart w:id="1" w:name="a"/><w:p/></w:sdtContent></w:sdt>"#,
        );
        let p = first_paragraph(&doc);
        assert_eq!(p.content.len(), 3);
        assert!(matches!(&p.content[0], Inline::Unknown(r) if r.local_name() == "permStart"));
        let blocks: Vec<_> = doc.blocks().collect();
        assert!(matches!(blocks[1], Block::Unknown(r) if r.local_name() == "sdt"));
        // Фрагмент несёт объявление пространства имён и снова читается.
        if let Inline::Unknown(raw) = &p.content[0] {
            assert!(raw.as_str().contains("xmlns:w="));
            assert!(xml::parse(raw.as_str().as_bytes()).is_ok());
        }
    }

    #[test]
    fn simple_field_becomes_field_characters() {
        let doc = read_body_xml(
            r#"<w:p><w:fldSimple w:instr=" PAGE "><w:r><w:rPr><w:b/></w:rPr><w:t>7</w:t></w:r></w:fldSimple></w:p>"#,
        );
        let p = first_paragraph(&doc);
        assert_eq!(p.plain_text(), "7");
        let names: Vec<_> = p
            .runs()
            .flat_map(|r| &r.content)
            .map(|c| match c {
                RunContent::FieldChar(c) => format!("{:?}", c.kind),
                RunContent::FieldCode(code) => code.clone(),
                RunContent::Text(t) => t.clone(),
                _ => String::new(),
            })
            .collect();
        assert_eq!(names, ["Begin", " PAGE ", "Separate", "7", "End"]);
        // Оформление результата есть и у знаков поля.
        assert!(p.runs().all(|r| r.props.bold == Some(true)));
        // Записанное поле читается так же.
        let xml = crate::serialize::document_xml(&doc);
        let again = read_document(&xml::parse(xml.as_bytes()).unwrap()).unwrap();
        assert_eq!(again.sections[0].blocks, doc.sections[0].blocks);
    }

    #[test]
    fn complex_field_is_read() {
        let doc = read_body_xml(
            r#"<w:p><w:r><w:fldChar w:fldCharType="begin" w:dirty="true"/></w:r><w:r><w:instrText xml:space="preserve"> DATE \@ "d.M.yyyy" </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>1.2.2026</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#,
        );
        let p = first_paragraph(&doc);
        let found = letter_model::fields(p);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name(), "DATE");
        assert_eq!(found[0].switch("\\@").as_deref(), Some("d.M.yyyy"));
        assert!(found[0].dirty);
        assert_eq!(&p.plain_text()[found[0].result.clone()], "1.2.2026");
        let xml = crate::serialize::document_xml(&doc);
        assert!(xml.contains(r#"<w:fldChar w:fldCharType="begin" w:dirty="1"/>"#));
        let again = read_document(&xml::parse(xml.as_bytes()).unwrap()).unwrap();
        assert_eq!(again.sections[0].blocks, doc.sections[0].blocks);
    }

    #[test]
    fn sections() {
        let doc = read_body_xml(
            r#"<w:p><w:r><w:t>one</w:t></w:r></w:p>
               <w:p><w:pPr><w:sectPr><w:pgSz w:w="16838" w:h="11906" w:orient="landscape"/></w:sectPr></w:pPr></w:p>
               <w:p><w:r><w:t>two</w:t></w:r></w:p>
               <w:sectPr>
                 <w:headerReference w:type="default" r:id="rId1" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"/>
                 <w:footerReference w:type="first" r:id="rId2" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"/>
                 <w:footerReference w:type="odd" r:id="rId3" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"/>
                 <w:titlePg/>
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
        // Непонятный вид колонтитула остаётся как есть.
        assert_eq!(names, ["footerReference"]);
        assert_eq!(
            last.headers
                .default
                .as_ref()
                .and_then(|h| h.relationship.as_deref()),
            Some("rId1")
        );
        assert_eq!(
            last.footers
                .first
                .as_ref()
                .and_then(|h| h.relationship.as_deref()),
            Some("rId2")
        );
        assert!(last.title_page);
        assert_eq!(
            last.columns,
            Some(Columns {
                space: Twips(708),
                ..Columns::default()
            })
        );
        assert_eq!(doc.plain_text(), "one\n\ntwo");
    }

    #[test]
    fn section_start_and_columns() {
        let read = |sect: &str| {
            read_body_xml(&format!("<w:sectPr>{sect}</w:sectPr>")).sections[0]
                .props
                .clone()
        };
        let props =
            read(r#"<w:type w:val="continuous"/><w:cols w:num="3" w:space="400" w:sep="1"/>"#);
        assert_eq!(props.start, Some(SectionStart::Continuous));
        assert_eq!(
            props.columns,
            Some(Columns {
                count: 3,
                space: Twips(400),
                separator: true,
                widths: Vec::new(),
            })
        );
        let uneven = read(
            r#"<w:cols w:num="2" w:equalWidth="0"><w:col w:w="3000" w:space="500"/><w:col w:w="4000"/></w:cols>"#,
        );
        let columns = uneven.columns.unwrap();
        assert_eq!(
            columns.widths,
            [(Twips(3000), Twips(500)), (Twips(4000), Twips::ZERO)]
        );
        assert_eq!(columns.space, Twips(720));
        // Непонятное остаётся как есть.
        let odd = read(r#"<w:type w:val="weird"/><w:cols w:num="2"><w:col w:w="1"/></w:cols>"#);
        assert_eq!((odd.start, odd.columns), (None, None));
        assert_eq!(odd.unknown.len(), 2);
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
        assert!(table.raw_props.is_empty() && table.grid.is_empty());
        assert!(table.rows[0].raw_props.is_empty());
        assert!(table.rows[0].cells[0].raw_props.is_empty());
        assert_eq!(blocks[0].plain_text(), "a\tb");
        assert!(matches!(blocks[1], Block::Unknown(_)));
    }

    #[test]
    fn document_level_unknowns_and_strict_namespace() {
        let xml = r#"<w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"><w:background w:color="FFFF00"/><w:background w:color="00FF00"/><w:body><w:p><w:r><w:t>s</w:t></w:r></w:p></w:body></w:document>"#;
        let doc = read_document(&xml::parse(xml.as_bytes()).unwrap()).unwrap();
        assert_eq!(doc.plain_text(), "s");
        // Цвет страницы понят, повтор сохранён как есть.
        assert_eq!(doc.background, Some(Color::Rgb(0xFF, 0xFF, 0)));
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

    #[test]
    fn content_controls_show_their_content_and_survive_round_trip() {
        let body = r#"<w:sdt><w:sdtPr><w:alias w:val="Титул"/><w:tag w:val="cover"/></w:sdtPr><w:sdtContent>
                 <w:p><w:r><w:t>Заголовок</w:t></w:r></w:p>
                 <w:sdt><w:sdtPr><w:tag w:val="inner"/></w:sdtPr><w:sdtContent>
                   <w:tbl><w:tr><w:tc><w:sdt><w:sdtPr><w:tag w:val="cell"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>ячейка</w:t></w:r></w:p></w:sdtContent></w:sdt></w:tc></w:tr></w:tbl>
                 </w:sdtContent></w:sdt>
                 <w:p><w:r><w:t>Подзаголовок</w:t></w:r></w:p>
               </w:sdtContent></w:sdt>
               <w:p><w:r><w:t>Имя: </w:t></w:r><w:sdt><w:sdtPr><w:tag w:val="name"/></w:sdtPr><w:sdtEndPr><w:rPr><w:b/></w:rPr></w:sdtEndPr><w:sdtContent><w:r><w:t>Иван</w:t></w:r><w:r><w:t> Петров</w:t></w:r></w:sdtContent></w:sdt><w:r><w:t>.</w:t></w:r></w:p>
               <w:sdt><w:sdtPr><w:tag w:val="a"/></w:sdtPr><w:sdtContent><w:p/></w:sdtContent></w:sdt>
               <w:sdt><w:sdtPr><w:tag w:val="a"/></w:sdtPr><w:sdtContent><w:p/></w:sdtContent></w:sdt>"#;
        let doc = read_body_xml(body);
        assert_eq!(
            doc.plain_text(),
            "Заголовок\nячейка\nПодзаголовок\nИмя: Иван Петров.\n\n"
        );
        let blocks: Vec<_> = doc.blocks().collect();
        let Block::Paragraph(title) = blocks[0] else {
            panic!("expected paragraph")
        };
        assert_eq!(title.props.controls.len(), 1);
        assert_eq!(title.props.controls[0].tag().as_deref(), Some("cover"));
        assert_eq!(title.props.controls[0].title().as_deref(), Some("Титул"));
        let Block::Table(table) = blocks[1] else {
            panic!("expected table")
        };
        assert_eq!(table.props.controls.len(), 2);
        assert!(Arc::ptr_eq(
            &table.props.controls[0],
            &title.props.controls[0]
        ));
        let Block::Paragraph(cell) = &table.rows[0].cells[0].blocks[0] else {
            panic!("expected paragraph")
        };
        assert_eq!(cell.props.controls[0].tag().as_deref(), Some("cell"));
        let Block::Paragraph(form) = blocks[3] else {
            panic!("expected paragraph")
        };
        assert!(form.props.controls.is_empty());
        let runs: Vec<_> = form.runs().collect();
        assert!(runs[0].props.control.is_none() && runs[3].props.control.is_none());
        let name = runs[1].props.control.as_ref().expect("control");
        assert_eq!(name.tag().as_deref(), Some("name"));
        assert!(name.end_props.is_some());
        assert!(Arc::ptr_eq(
            name,
            runs[2].props.control.as_ref().expect("control")
        ));

        // Запись возвращает те же элементы: снова читается в ту же модель,
        // и соседние одинаковые элементы не слились.
        let written = crate::serialize::document_xml(&doc);
        assert_eq!(written.matches("<w:sdt>").count(), 6);
        let again = read_document(&xml::parse(written.as_bytes()).unwrap()).unwrap();
        assert_eq!(again.sections, doc.sections);
    }

    #[test]
    fn unusual_content_controls_stay_as_they_are() {
        let doc = read_body_xml(
            r#"<w:sdt><w:sdtPr/><w:sdtContent/></w:sdt>
               <w:sdt><w:sdtContent><w:customXml/></w:sdtContent></w:sdt>
               <w:p><w:sdt><w:sdtContent><w:hyperlink/></w:sdtContent></w:sdt></w:p>"#,
        );
        let blocks: Vec<_> = doc.blocks().collect();
        assert!(matches!(blocks[0], Block::Unknown(_)));
        assert!(matches!(blocks[1], Block::Unknown(_)));
        let Block::Paragraph(p) = blocks[2] else {
            panic!("expected paragraph")
        };
        assert!(matches!(&p.content[0], Inline::Unknown(r) if r.local_name() == "sdt"));
    }
}
