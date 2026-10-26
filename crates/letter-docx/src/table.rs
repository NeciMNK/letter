//! Таблицы: чтение и запись `w:tbl` со свойствами таблицы (`w:tblPr`),
//! сеткой (`w:tblGrid`), строк (`w:trPr`) и ячеек (`w:tcPr`).
//!
//! Правило то же, что и для абзацев: понятое превращается в поля модели,
//! остальное хранится как есть и записывается обратно.

use letter_model::{
    Block, Border, Borders, CellMargins, CellProps, CellVerticalAlign, HeightRule,
    ParagraphBorders, RowHeight, RowProps, Shading, Table, TableArea, TableAreaStyle, TableCell,
    TableLayout, TableLook, TableProps, TableRow, TableWidth, Twips, VerticalMerge,
};

use crate::document::{enum_val, is_w_ns, parse_color, read_blocks, set, string_val, toggle};
use crate::names::{ALIGNMENTS, name_of};
use crate::serialize::{
    Children, blocks_xml, element, empty, hex_color, raws, toggle as toggle_xml, val,
};
use crate::xml::Element;

/// Порядок детей `w:tblPr` по схеме.
const TBLPR_ORDER: &[&str] = &[
    "tblStyle",
    "tblpPr",
    "tblOverlap",
    "bidiVisual",
    "tblStyleRowBandSize",
    "tblStyleColBandSize",
    "tblW",
    "jc",
    "tblCellSpacing",
    "tblInd",
    "tblBorders",
    "shd",
    "tblLayout",
    "tblCellMar",
    "tblLook",
    "tblCaption",
    "tblDescription",
    "tblPrChange",
];

/// Порядок детей `w:trPr` по схеме.
const TRPR_ORDER: &[&str] = &[
    "cnfStyle",
    "divId",
    "gridBefore",
    "gridAfter",
    "wBefore",
    "wAfter",
    "cantSplit",
    "trHeight",
    "tblHeader",
    "tblCellSpacing",
    "jc",
    "hidden",
    "ins",
    "del",
    "trPrChange",
];

/// Порядок детей `w:tcPr` по схеме.
const TCPR_ORDER: &[&str] = &[
    "cnfStyle",
    "tcW",
    "gridSpan",
    "hMerge",
    "vMerge",
    "tcBorders",
    "shd",
    "noWrap",
    "tcMar",
    "textDirection",
    "tcFitText",
    "vAlign",
    "hideMark",
    "headers",
    "cellIns",
    "cellDel",
    "cellMerge",
    "tcPrChange",
];

/// Порядок детей `w:tblStylePr` по схеме.
const AREA_ORDER: &[&str] = &["pPr", "rPr", "tblPr", "trPr", "tcPr"];

/// Части таблицы в стиле (`w:tblStylePr w:type`).
const AREAS: &[(&str, TableArea)] = &[
    ("wholeTable", TableArea::WholeTable),
    ("band1Vert", TableArea::Band1Vertical),
    ("band2Vert", TableArea::Band2Vertical),
    ("band1Horz", TableArea::Band1Horizontal),
    ("band2Horz", TableArea::Band2Horizontal),
    ("firstCol", TableArea::FirstColumn),
    ("lastCol", TableArea::LastColumn),
    ("firstRow", TableArea::FirstRow),
    ("lastRow", TableArea::LastRow),
    ("neCell", TableArea::TopRightCell),
    ("nwCell", TableArea::TopLeftCell),
    ("seCell", TableArea::BottomRightCell),
    ("swCell", TableArea::BottomLeftCell),
];

const LAYOUTS: &[(&str, TableLayout)] = &[
    ("autofit", TableLayout::Auto),
    ("fixed", TableLayout::Fixed),
];

const HEIGHT_RULES: &[(&str, HeightRule)] = &[
    ("auto", HeightRule::Auto),
    ("atLeast", HeightRule::AtLeast),
    ("exact", HeightRule::Exact),
];

const V_ALIGNS: &[(&str, CellVerticalAlign)] = &[
    ("top", CellVerticalAlign::Top),
    ("center", CellVerticalAlign::Center),
    ("bottom", CellVerticalAlign::Bottom),
];

/// Читает `w:tbl`. Таблица с чем-то непонятным между строками
/// (закладки, элементы управления содержимым) сохраняется целиком.
pub(crate) fn read_table(tbl: &Element) -> Block {
    let mut table = Table::default();
    for child in tbl.elements() {
        if child.is_w("tblPr") && child.attrs.is_empty() {
            table.props = Box::new(read_table_props(child));
        } else if child.is_w("tblGrid") && table.grid.is_empty() {
            match read_grid(child) {
                Some(grid) => table.grid = grid,
                None => table.raw_props.push(child.raw()),
            }
        } else if child.is_w("tblPr") || child.is_w("tblGrid") {
            table.raw_props.push(child.raw());
        } else if child.is_w("tr") {
            match read_row(child) {
                Some(row) => table.rows.push(row),
                None => return Block::Unknown(tbl.raw()),
            }
        } else {
            return Block::Unknown(tbl.raw());
        }
    }
    Block::Table(table)
}

fn read_grid(grid: &Element) -> Option<Vec<Twips>> {
    if !grid.attrs.is_empty() {
        return None;
    }
    grid.elements()
        .map(|col| {
            if col.is_w("gridCol") && col.only_attrs(&["w"]) {
                // Ширина может отсутствовать: тогда столбец подбирается.
                match col.w_attr("w") {
                    Some(w) => crate::document::parse_twips(w),
                    None => Some(Twips(0)),
                }
            } else {
                None
            }
        })
        .collect()
}

fn read_row(tr: &Element) -> Option<TableRow> {
    let mut row = TableRow::default();
    let mut has_props = false;
    for child in tr.elements() {
        if child.is_w("trPr") && !has_props && child.attrs.is_empty() {
            row.props = read_row_props(child);
            has_props = true;
        } else if child.is_w("trPr") || child.is_w("tblPrEx") {
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
    let mut has_props = false;
    for child in tc.elements() {
        if child.is_w("tcPr") && !has_props && child.attrs.is_empty() {
            cell.props = read_cell_props(child);
            has_props = true;
        } else if child.is_w("tcPr") {
            cell.raw_props.push(child.raw());
        } else {
            read_blocks(child, &mut cell.blocks);
        }
    }
    cell
}

pub(crate) fn read_table_props(tbl_pr: &Element) -> TableProps {
    let mut props = TableProps::default();
    for child in tbl_pr.elements() {
        let understood = is_w_ns(child.ns.as_deref())
            && match child.local.as_str() {
                "tblStyle" => set(&mut props.style, string_val(child)),
                "tblW" => set(&mut props.width, read_width(child)),
                "jc" => set(&mut props.alignment, enum_val(child, ALIGNMENTS)),
                "tblInd" => set(&mut props.indent, read_width(child)),
                "tblBorders" => set(&mut props.borders, read_borders(child)),
                "shd" => set(&mut props.shading, read_shading(child)),
                "tblLayout" => set(&mut props.layout, read_layout(child)),
                "tblCellMar" => set(&mut props.cell_margins, read_margins(child)),
                "tblLook" => set(&mut props.look, read_look(child)),
                "tblStyleRowBandSize" => set(&mut props.row_band_size, read_span(child)),
                "tblStyleColBandSize" => set(&mut props.column_band_size, read_span(child)),
                _ => false,
            };
        if !understood {
            props.unknown.push(child.raw());
        }
    }
    props
}

/// `w:tblLook`: старая запись числом (`w:val`) и новая — атрибутами;
/// атрибуты важнее числа.
fn read_look(element: &Element) -> Option<TableLook> {
    const NAMES: [&str; 6] = [
        "firstRow",
        "lastRow",
        "firstColumn",
        "lastColumn",
        "noHBand",
        "noVBand",
    ];
    let mut allowed = vec!["val"];
    allowed.extend(NAMES);
    if !element.only_attrs(&allowed) {
        return None;
    }
    let mut look = match element.w_attr("val") {
        Some(v) => TableLook::from_bits(u16::from_str_radix(v.trim(), 16).ok()?),
        None => TableLook::default(),
    };
    for name in NAMES {
        let Some(value) = element.w_attr(name) else {
            continue;
        };
        let on = match value {
            "1" | "true" | "on" => true,
            "0" | "false" | "off" => false,
            _ => return None,
        };
        *match name {
            "firstRow" => &mut look.first_row,
            "lastRow" => &mut look.last_row,
            "firstColumn" => &mut look.first_column,
            "lastColumn" => &mut look.last_column,
            "noHBand" => &mut look.no_h_band,
            _ => &mut look.no_v_band,
        } = on;
    }
    Some(look)
}

/// `w:tblStylePr` стиля таблицы; `None` — если что-то непонятно (тогда
/// элемент хранится как есть).
pub(crate) fn read_table_area(element: &Element) -> Option<TableAreaStyle> {
    if !element.only_attrs(&["type"]) {
        return None;
    }
    let area = crate::names::lookup(AREAS, element.w_attr("type")?)?;
    let mut style = TableAreaStyle::new(area);
    for child in element.elements() {
        let understood = is_w_ns(child.ns.as_deref())
            && child.attrs.is_empty()
            && match child.local.as_str() {
                "pPr" => {
                    style.paragraph = crate::document::read_paragraph_props(child, false).0;
                    true
                }
                "rPr" => {
                    style.run = crate::document::read_run_props(child);
                    true
                }
                "tblPr" => {
                    style.table = read_table_props(child);
                    true
                }
                "tcPr" => {
                    style.cell = read_cell_props(child);
                    true
                }
                _ => false,
            };
        if !understood {
            style.unknown.push(child.raw());
        }
    }
    Some(style)
}

/// Записывает `w:tblStylePr`.
pub(crate) fn table_area_xml(style: &TableAreaStyle) -> String {
    let mut children = Children::new(AREA_ORDER);
    children.push(
        "pPr",
        crate::serialize::paragraph_props(&style.paragraph, None),
    );
    children.push("rPr", crate::serialize::run_props(&style.run));
    let table = table_props(&style.table);
    if !table.is_empty() {
        children.push("tblPr", element("w:tblPr", &table));
    }
    let cell = cell_props(&style.cell);
    if !cell.is_empty() {
        children.push("tcPr", element("w:tcPr", &cell));
    }
    children.raw(&style.unknown);
    let name = name_of(AREAS, style.area).unwrap_or("wholeTable");
    crate::serialize::element_with(
        "w:tblStylePr",
        &[("w:type".to_string(), name.to_string())],
        &children.finish(),
    )
}

fn read_row_props(tr_pr: &Element) -> RowProps {
    let mut props = RowProps::default();
    for child in tr_pr.elements() {
        let understood = is_w_ns(child.ns.as_deref())
            && match child.local.as_str() {
                "trHeight" => set(&mut props.height, read_height(child)),
                "tblHeader" => set(&mut props.header, toggle(child)),
                "cantSplit" => set(&mut props.cant_split, toggle(child)),
                _ => false,
            };
        if !understood {
            props.unknown.push(child.raw());
        }
    }
    props
}

fn read_cell_props(tc_pr: &Element) -> CellProps {
    let mut props = CellProps::default();
    for child in tc_pr.elements() {
        let understood = is_w_ns(child.ns.as_deref())
            && match child.local.as_str() {
                "tcW" => set(&mut props.width, read_width(child)),
                "gridSpan" => set(&mut props.grid_span, read_span(child)),
                "vMerge" => set(&mut props.v_merge, read_v_merge(child)),
                "tcBorders" => set(&mut props.borders, read_borders(child)),
                "shd" => set(&mut props.shading, read_shading(child)),
                "tcMar" => set(&mut props.margins, read_margins(child)),
                "vAlign" => set(&mut props.v_align, enum_val(child, V_ALIGNS)),
                _ => false,
            };
        if !understood {
            props.unknown.push(child.raw());
        }
    }
    props
}

/// Ширина `w:w` + `w:type` (по умолчанию — твипы).
fn read_width(element: &Element) -> Option<TableWidth> {
    if !element.only_attrs(&["w", "type"]) {
        return None;
    }
    let number = |value: Option<&str>| -> Option<i32> { value?.trim().parse().ok() };
    match element.w_attr("type") {
        Some("auto") => Some(TableWidth::Auto),
        Some("nil") => Some(TableWidth::Nil),
        Some("pct") => {
            let value = element.w_attr("w")?.trim();
            match value.strip_suffix('%') {
                // «50%» — новая запись; в модели — пятидесятые доли процента.
                Some(percent) => {
                    let percent: f64 = percent.trim().parse().ok()?;
                    let fiftieths = (percent * 50.0).round();
                    (fiftieths.abs() <= 1e9).then_some(TableWidth::Percent(fiftieths as i32))
                }
                None => number(Some(value)).map(TableWidth::Percent),
            }
        }
        None | Some("dxa") => {
            crate::document::parse_twips(element.w_attr("w")?).map(TableWidth::Twips)
        }
        Some(_) => None,
    }
}

fn read_span(element: &Element) -> Option<u32> {
    crate::document::single_val(element)?.trim().parse().ok()
}

fn read_v_merge(element: &Element) -> Option<VerticalMerge> {
    if !element.only_attrs(&["val"]) {
        return None;
    }
    match element.w_attr("val") {
        None | Some("continue") => Some(VerticalMerge::Continue),
        Some("restart") => Some(VerticalMerge::Restart),
        Some(_) => None,
    }
}

fn read_layout(element: &Element) -> Option<TableLayout> {
    if !element.only_attrs(&["type"]) {
        return None;
    }
    // Без `w:type` — автоподбор.
    match element.w_attr("type") {
        None => Some(TableLayout::Auto),
        Some(v) => crate::names::lookup(LAYOUTS, v),
    }
}

fn read_height(element: &Element) -> Option<RowHeight> {
    if !element.only_attrs(&["val", "hRule"]) {
        return None;
    }
    let value = match element.w_attr("val") {
        Some(v) => crate::document::parse_twips(v)?,
        None => Twips(0),
    };
    let rule = match element.w_attr("hRule") {
        None => HeightRule::AtLeast,
        Some(v) => crate::names::lookup(HEIGHT_RULES, v)?,
    };
    Some(RowHeight { value, rule })
}

/// Набор границ (`w:tblBorders`, `w:tcBorders`). Если внутри есть что-то
/// кроме шести понятных линий, элемент сохраняется как есть.
fn read_borders(element: &Element) -> Option<Borders> {
    if !element.attrs.is_empty() {
        return None;
    }
    let mut borders = Borders::default();
    for child in element.elements() {
        if !is_w_ns(child.ns.as_deref()) {
            return None;
        }
        let slot = match child.local.as_str() {
            "top" => &mut borders.top,
            "left" | "start" => &mut borders.left,
            "bottom" => &mut borders.bottom,
            "right" | "end" => &mut borders.right,
            "insideH" => &mut borders.inside_h,
            "insideV" => &mut borders.inside_v,
            _ => return None,
        };
        if slot.is_some() {
            return None;
        }
        *slot = Some(read_border(child)?);
    }
    Some(borders)
}

/// `w:pBdr`: границы абзаца. `None`, если внутри есть что-то
/// непонятное — тогда элемент сохраняется как есть.
pub(crate) fn read_paragraph_borders(element: &Element) -> Option<ParagraphBorders> {
    if !element.attrs.is_empty() {
        return None;
    }
    let mut borders = ParagraphBorders::default();
    for child in element.elements() {
        if !is_w_ns(child.ns.as_deref()) {
            return None;
        }
        let slot = match child.local.as_str() {
            "top" => &mut borders.top,
            "left" | "start" => &mut borders.left,
            "bottom" => &mut borders.bottom,
            "right" | "end" => &mut borders.right,
            "between" => &mut borders.between,
            "bar" => &mut borders.bar,
            _ => return None,
        };
        if slot.is_some() {
            return None;
        }
        *slot = Some(read_border(child)?);
    }
    Some(borders)
}

pub(crate) fn read_border(element: &Element) -> Option<Border> {
    if !element.only_attrs(&[
        "val",
        "sz",
        "space",
        "color",
        "themeColor",
        "themeShade",
        "themeTint",
    ]) {
        return None;
    }
    let number = |name: &str| -> Result<Option<u32>, ()> {
        match element.w_attr(name) {
            Some(v) => v.trim().parse().map(Some).map_err(|_| ()),
            None => Ok(None),
        }
    };
    Some(Border {
        style: element.w_attr("val")?.to_string(),
        size: number("sz").ok()?,
        space: number("space").ok()?,
        color: match element.w_attr("color") {
            Some(v) => Some(parse_color(v)?),
            None => None,
        },
        theme_color: crate::document::read_theme_color(element)?,
    })
}

pub(crate) fn read_shading(element: &Element) -> Option<Shading> {
    if !element.only_attrs(&[
        "val",
        "color",
        "fill",
        "themeColor",
        "themeShade",
        "themeTint",
        "themeFill",
        "themeFillShade",
        "themeFillTint",
    ]) {
        return None;
    }
    let color = |name: &str| -> Result<Option<_>, ()> {
        match element.w_attr(name) {
            Some(v) => parse_color(v).map(Some).ok_or(()),
            None => Ok(None),
        }
    };
    Some(Shading {
        pattern: element.w_attr("val")?.to_string(),
        color: color("color").ok()?,
        fill: color("fill").ok()?,
        theme_color: crate::document::read_theme_color(element)?,
        theme_fill: crate::document::read_theme_attrs(
            element,
            ["themeFill", "themeFillShade", "themeFillTint"],
        )?,
    })
}

fn read_margins(element: &Element) -> Option<CellMargins> {
    if !element.attrs.is_empty() {
        return None;
    }
    let mut margins = CellMargins::default();
    for child in element.elements() {
        if !is_w_ns(child.ns.as_deref()) {
            return None;
        }
        let slot = match child.local.as_str() {
            "top" => &mut margins.top,
            "left" | "start" => &mut margins.left,
            "bottom" => &mut margins.bottom,
            "right" | "end" => &mut margins.right,
            _ => return None,
        };
        if slot.is_some() {
            return None;
        }
        *slot = Some(read_width(child)?);
    }
    Some(margins)
}

/// Записывает `w:tbl`.
pub(crate) fn table_xml(table: &Table) -> String {
    let mut out = element("w:tblPr", &table_props(&table.props));
    // Сетка обязательна; если она не понята при чтении, она лежит
    // в `raw_props` и пишется оттуда.
    if !table.grid.is_empty() || table.raw_props.is_empty() {
        let cols: String = table
            .grid
            .iter()
            .map(|w| empty("w:gridCol", &[("w:w", w.0.to_string())]))
            .collect();
        out.push_str(&element("w:tblGrid", &cols));
    }
    out.push_str(&raws(&table.raw_props));
    for row in &table.rows {
        let mut row_xml = raws(&row.raw_props);
        let props = row_props(&row.props);
        if !props.is_empty() {
            row_xml.push_str(&element("w:trPr", &props));
        }
        for cell in &row.cells {
            let mut cell_xml = String::new();
            let props = cell_props(&cell.props);
            if !props.is_empty() {
                cell_xml.push_str(&element("w:tcPr", &props));
            }
            cell_xml.push_str(&raws(&cell.raw_props));
            cell_xml.push_str(&blocks_xml(&cell.blocks));
            if cell.blocks.is_empty() {
                // Word не открывает ячейку без абзаца.
                cell_xml.push_str("<w:p/>");
            }
            row_xml.push_str(&element("w:tc", &cell_xml));
        }
        out.push_str(&element("w:tr", &row_xml));
    }
    element("w:tbl", &out)
}

pub(crate) fn table_props(props: &TableProps) -> String {
    let mut children = Children::new(TBLPR_ORDER);
    if let Some(style) = &props.style {
        children.push("tblStyle", val("w:tblStyle", style));
    }
    if let Some(size) = props.row_band_size {
        children.push(
            "tblStyleRowBandSize",
            val("w:tblStyleRowBandSize", &size.to_string()),
        );
    }
    if let Some(size) = props.column_band_size {
        children.push(
            "tblStyleColBandSize",
            val("w:tblStyleColBandSize", &size.to_string()),
        );
    }
    if let Some(width) = props.width {
        children.push("tblW", width_xml("w:tblW", width));
    }
    if let Some(alignment) = props.alignment
        && let Some(name) = name_of(ALIGNMENTS, alignment)
    {
        children.push("jc", val("w:jc", name));
    }
    if let Some(indent) = props.indent {
        children.push("tblInd", width_xml("w:tblInd", indent));
    }
    if let Some(borders) = &props.borders {
        children.push("tblBorders", borders_xml("w:tblBorders", borders));
    }
    if let Some(shading) = &props.shading {
        children.push("shd", shading_xml(shading));
    }
    if let Some(layout) = props.layout
        && let Some(name) = name_of(LAYOUTS, layout)
    {
        children.push(
            "tblLayout",
            empty("w:tblLayout", &[("w:type", name.into())]),
        );
    }
    if let Some(margins) = &props.cell_margins {
        children.push("tblCellMar", margins_xml("w:tblCellMar", margins));
    }
    if let Some(look) = props.look {
        let flag = |on: bool| if on { "1" } else { "0" }.to_string();
        children.push(
            "tblLook",
            empty(
                "w:tblLook",
                &[
                    ("w:val", format!("{:04X}", look.bits())),
                    ("w:firstRow", flag(look.first_row)),
                    ("w:lastRow", flag(look.last_row)),
                    ("w:firstColumn", flag(look.first_column)),
                    ("w:lastColumn", flag(look.last_column)),
                    ("w:noHBand", flag(look.no_h_band)),
                    ("w:noVBand", flag(look.no_v_band)),
                ],
            ),
        );
    }
    children.raw(&props.unknown);
    children.finish()
}

fn row_props(props: &RowProps) -> String {
    let mut children = Children::new(TRPR_ORDER);
    if let Some(on) = props.cant_split {
        children.push("cantSplit", toggle_xml("w:cantSplit", on));
    }
    if let Some(height) = props.height {
        let mut attrs = vec![("w:val", height.value.0.to_string())];
        if height.rule != HeightRule::AtLeast
            && let Some(rule) = name_of(HEIGHT_RULES, height.rule)
        {
            attrs.push(("w:hRule", rule.to_string()));
        }
        children.push("trHeight", empty("w:trHeight", &attrs));
    }
    if let Some(on) = props.header {
        children.push("tblHeader", toggle_xml("w:tblHeader", on));
    }
    children.raw(&props.unknown);
    children.finish()
}

fn cell_props(props: &CellProps) -> String {
    let mut children = Children::new(TCPR_ORDER);
    if let Some(width) = props.width {
        children.push("tcW", width_xml("w:tcW", width));
    }
    if let Some(span) = props.grid_span {
        children.push("gridSpan", val("w:gridSpan", &span.to_string()));
    }
    if let Some(merge) = props.v_merge {
        let xml = match merge {
            VerticalMerge::Restart => val("w:vMerge", "restart"),
            VerticalMerge::Continue => "<w:vMerge/>".to_string(),
        };
        children.push("vMerge", xml);
    }
    if let Some(borders) = &props.borders {
        children.push("tcBorders", borders_xml("w:tcBorders", borders));
    }
    if let Some(shading) = &props.shading {
        children.push("shd", shading_xml(shading));
    }
    if let Some(margins) = &props.margins {
        children.push("tcMar", margins_xml("w:tcMar", margins));
    }
    if let Some(align) = props.v_align
        && let Some(name) = name_of(V_ALIGNS, align)
    {
        children.push("vAlign", val("w:vAlign", name));
    }
    children.raw(&props.unknown);
    children.finish()
}

fn width_xml(name: &str, width: TableWidth) -> String {
    let (w, kind) = match width {
        TableWidth::Auto => (0, "auto"),
        TableWidth::Nil => (0, "nil"),
        TableWidth::Twips(t) => (t.0, "dxa"),
        TableWidth::Percent(p) => (p, "pct"),
    };
    empty(
        name,
        &[("w:w", w.to_string()), ("w:type", kind.to_string())],
    )
}

fn borders_xml(name: &str, borders: &Borders) -> String {
    sides_xml(
        name,
        [
            ("w:top", &borders.top),
            ("w:left", &borders.left),
            ("w:bottom", &borders.bottom),
            ("w:right", &borders.right),
            ("w:insideH", &borders.inside_h),
            ("w:insideV", &borders.inside_v),
        ],
    )
}

/// `w:pBdr`.
pub(crate) fn paragraph_borders_xml(borders: &ParagraphBorders) -> String {
    sides_xml(
        "w:pBdr",
        [
            ("w:top", &borders.top),
            ("w:left", &borders.left),
            ("w:bottom", &borders.bottom),
            ("w:right", &borders.right),
            ("w:between", &borders.between),
            ("w:bar", &borders.bar),
        ],
    )
}

fn sides_xml<const N: usize>(name: &str, sides: [(&str, &Option<Border>); N]) -> String {
    element(name, &border_lines(sides))
}

/// Линии границ по сторонам: `<w:top …/>` и т. п.
pub(crate) fn border_lines<const N: usize>(sides: [(&str, &Option<Border>); N]) -> String {
    let mut inner = String::new();
    for (side, border) in sides {
        if let Some(border) = border {
            let mut attrs = vec![("w:val", border.style.clone())];
            if let Some(size) = border.size {
                attrs.push(("w:sz", size.to_string()));
            }
            if let Some(space) = border.space {
                attrs.push(("w:space", space.to_string()));
            }
            if let Some(color) = border.color {
                attrs.push(("w:color", hex_color(color)));
            }
            crate::serialize::theme_color_attrs(&mut attrs, border.theme_color.as_ref());
            inner.push_str(&empty(side, &attrs));
        }
    }
    inner
}

pub(crate) fn shading_xml(shading: &Shading) -> String {
    let mut attrs = vec![("w:val", shading.pattern.clone())];
    if let Some(color) = shading.color {
        attrs.push(("w:color", hex_color(color)));
    }
    if let Some(fill) = shading.fill {
        attrs.push(("w:fill", hex_color(fill)));
    }
    crate::serialize::theme_color_attrs(&mut attrs, shading.theme_color.as_ref());
    if let Some(theme) = &shading.theme_fill {
        attrs.push(("w:themeFill", theme.name.clone()));
        if let Some(shade) = theme.shade {
            attrs.push(("w:themeFillShade", format!("{shade:02X}")));
        }
        if let Some(tint) = theme.tint {
            attrs.push(("w:themeFillTint", format!("{tint:02X}")));
        }
    }
    empty("w:shd", &attrs)
}

fn margins_xml(name: &str, margins: &CellMargins) -> String {
    let sides = [
        ("w:top", margins.top),
        ("w:left", margins.left),
        ("w:bottom", margins.bottom),
        ("w:right", margins.right),
    ];
    let inner: String = sides
        .into_iter()
        .filter_map(|(side, width)| width.map(|w| width_xml(side, w)))
        .collect();
    element(name, &inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml;

    fn parse(xml_text: &str) -> Element {
        let text = format!(
            "<w:root xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">{xml_text}</w:root>"
        );
        match xml::parse(text.as_bytes()) {
            Ok(root) => match root.elements().next() {
                Some(e) => e.clone(),
                None => panic!("нет элемента"),
            },
            Err(e) => panic!("{e:?}"),
        }
    }

    #[test]
    fn widths() {
        let w = |s: &str| read_width(&parse(s));
        assert_eq!(
            w("<w:tblW w:w=\"5000\" w:type=\"pct\"/>"),
            Some(TableWidth::Percent(5000))
        );
        assert_eq!(
            w("<w:tblW w:w=\"50%\" w:type=\"pct\"/>"),
            Some(TableWidth::Percent(2500))
        );
        assert_eq!(
            w("<w:tblW w:w=\"0\" w:type=\"auto\"/>"),
            Some(TableWidth::Auto)
        );
        assert_eq!(
            w("<w:tcW w:w=\"1440\"/>"),
            Some(TableWidth::Twips(Twips(1440)))
        );
        assert_eq!(w("<w:tcW w:w=\"1\" w:type=\"bogus\"/>"), None);
    }

    #[test]
    fn borders_with_theme_colors_are_read_and_written() {
        let source = "<w:tblPr><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\" w:color=\"4F81BD\" w:themeColor=\"accent1\" w:themeShade=\"BF\"/></w:tblBorders></w:tblPr>";
        let props = read_table_props(&parse(source));
        assert!(props.unknown.is_empty());
        let top = props
            .borders
            .as_ref()
            .and_then(|b| b.top.clone())
            .expect("линия сверху");
        let theme = top.theme_color.as_ref().expect("цвет темы");
        assert_eq!((theme.name.as_str(), theme.shade), ("accent1", Some(0xBF)));
        let written = element("w:tblPr", &table_props(&props));
        assert_eq!(read_table_props(&parse(&written)), props);
        // Оттенок без цвета темы непонятен — элемент сохраняется как есть.
        let odd = read_table_props(&parse(
            "<w:tblPr><w:tblBorders><w:top w:val=\"single\" w:themeShade=\"BF\"/></w:tblBorders></w:tblPr>",
        ));
        assert_eq!(odd.borders, None);
        assert_eq!(odd.unknown.len(), 1);
    }

    #[test]
    fn cell_props_round_trip() {
        let source = "<w:tcPr><w:tcW w:w=\"2000\" w:type=\"dxa\"/><w:gridSpan w:val=\"2\"/><w:vMerge w:val=\"restart\"/><w:tcBorders><w:top w:val=\"double\" w:sz=\"6\" w:space=\"0\" w:color=\"FF0000\"/></w:tcBorders><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"D9E2F3\"/><w:noWrap/><w:vAlign w:val=\"center\"/></w:tcPr>";
        let props = read_cell_props(&parse(source));
        assert_eq!(props.grid_span, Some(2));
        assert_eq!(props.v_merge, Some(VerticalMerge::Restart));
        assert_eq!(props.v_align, Some(CellVerticalAlign::Center));
        assert_eq!(props.unknown.len(), 1);
        let written = element("w:tcPr", &cell_props(&props));
        assert_eq!(read_cell_props(&parse(&written)), props);
        // noWrap остаётся между shd и vAlign, как требует схема.
        let no_wrap = written.find("noWrap").unwrap_or(0);
        assert!(written.find("w:shd").unwrap_or(usize::MAX) < no_wrap);
        assert!(no_wrap < written.find("vAlign").unwrap_or(0));
    }
}
