//! Фигуры DrawingML: `wps:wsp` (фигура или надпись) и `wpg:wgp`
//! (группа) внутри `a:graphicData` рисунка.
//!
//! Из фигуры программа понимает контур (готовый или свой), заливку,
//! линию со стрелками, поворот, отражение и текст надписи. Весь элемент
//! рисунка при этом хранится как есть ([`letter_model::Picture::raw`]).
//! Фигура без исходного XML (например, из .doc) записывается заново
//! ([`graphic_data_xml`]).

use letter_model::{
    ColorBase, ColorMod, DEFAULT_LINE_WIDTH, DEFAULT_TEXT_INSETS, Frame, Geometry, Group,
    GroupChild, LineDash, LineEnd, PathCommand, Shape, ShapeColor, ShapeLine, ShapePath,
    TextAnchor,
};

use crate::xml::Element;

/// Самая большая координата внутри фигуры, EMU: больше не бывает
/// в разумном файле.
const MAX_COORD: i64 = 1 << 40;
/// Самая глубокая вложенность групп, которую читаем.
const MAX_DEPTH: usize = 16;

/// Фигура из `wps:wsp` или группа из `wpg:wgp`; `None` — не фигура.
pub(crate) fn read_graphic_shape(data: &Element) -> Option<Shape> {
    let element = data.elements().next()?;
    match element.local.as_str() {
        "wsp" => Some(read_wsp(element)),
        "wgp" => Some(read_group(element, 0)),
        _ => None,
    }
}

/// Шестнадцатеричный цвет `RRGGBB`.
pub(crate) fn parse_rgb(value: &str) -> Option<[u8; 3]> {
    if value.len() != 6 || !value.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&value[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

fn child<'a>(element: &'a Element, local: &str) -> Option<&'a Element> {
    element.elements().find(|e| e.local == local)
}

fn attr<'a>(element: &'a Element, local: &str) -> Option<&'a str> {
    element
        .attrs
        .iter()
        .find(|a| a.local == local)
        .map(|a| a.value.as_str())
}

fn number(element: &Element, local: &str) -> Option<i64> {
    attr(element, local)?
        .parse()
        .ok()
        .filter(|v: &i64| v.abs() <= MAX_COORD)
}

fn flag(element: &Element, local: &str) -> bool {
    matches!(attr(element, local), Some("1" | "true"))
}

/// Фигура `wps:wsp` (или `wps:wsp` внутри группы).
fn read_wsp(wsp: &Element) -> Shape {
    let mut shape = Shape {
        text_insets: DEFAULT_TEXT_INSETS,
        ..Shape::default()
    };
    let props = child(wsp, "spPr");
    if let Some(xfrm) = props.and_then(|p| child(p, "xfrm")) {
        read_transform(xfrm, &mut shape);
    }
    if let Some(geometry) = props.and_then(read_geometry) {
        shape.geometry = geometry;
    }
    let style = child(wsp, "style");
    shape.fill = read_shape_fill(props, style);
    shape.line = read_shape_line(props, style);
    shape.text_color = style
        .and_then(|s| child(s, "fontRef"))
        .and_then(|f| f.elements().find_map(read_color));
    if let Some(body) = child(wsp, "bodyPr") {
        shape.text_anchor = match attr(body, "anchor") {
            Some("ctr") => TextAnchor::Center,
            Some("b") => TextAnchor::Bottom,
            _ => TextAnchor::Top,
        };
        for (i, name) in ["lIns", "tIns", "rIns", "bIns"].into_iter().enumerate() {
            if let Some(v) = number(body, name).filter(|v| *v >= 0) {
                shape.text_insets[i] = v;
            }
        }
    }
    if let Some(content) = child(wsp, "txbx").and_then(|t| child(t, "txbxContent")) {
        shape.text = crate::document::read_child_blocks(content);
    }
    shape
}

/// Поворот и отражение из `a:xfrm`.
fn read_transform(xfrm: &Element, shape: &mut Shape) {
    shape.rotation = number(xfrm, "rot")
        .map_or(0, |r| r.rem_euclid(21_600_000))
        .try_into()
        .unwrap_or(0);
    shape.flip_horizontal = flag(xfrm, "flipH");
    shape.flip_vertical = flag(xfrm, "flipV");
}

/// Место из `a:off` и `a:ext` (или `a:chOff` и `a:chExt`).
fn read_frame(xfrm: &Element, offset: &str, extent: &str) -> Option<Frame> {
    let off = child(xfrm, offset)?;
    let ext = child(xfrm, extent)?;
    Some(Frame {
        x: number(off, "x")?,
        y: number(off, "y")?,
        width: number(ext, "cx").filter(|v| *v >= 0)?,
        height: number(ext, "cy").filter(|v| *v >= 0)?,
    })
}

/// Группа `wpg:wgp` или вложенная `wpg:grpSp`.
fn read_group(group: &Element, depth: usize) -> Shape {
    let mut shape = Shape::default();
    let mut frame = Frame::default();
    if let Some(xfrm) = child(group, "grpSpPr").and_then(|p| child(p, "xfrm")) {
        read_transform(xfrm, &mut shape);
        frame = read_frame(xfrm, "chOff", "chExt").unwrap_or_default();
    }
    let mut children = Vec::new();
    for element in group.elements() {
        let (child_shape, props) = match element.local.as_str() {
            "wsp" => (read_wsp(element), "spPr"),
            "grpSp" if depth < MAX_DEPTH => (read_group(element, depth + 1), "grpSpPr"),
            _ => continue,
        };
        let Some(place) = child(element, props)
            .and_then(|p| child(p, "xfrm"))
            .and_then(|x| read_frame(x, "off", "ext"))
        else {
            continue;
        };
        children.push(GroupChild {
            frame: place,
            shape: child_shape,
        });
    }
    shape.geometry = Geometry::Group(Group { frame, children });
    shape
}

/// Контур из `a:prstGeom` или `a:custGeom`.
fn read_geometry(props: &Element) -> Option<Geometry> {
    if let Some(preset) = child(props, "prstGeom") {
        let name = attr(preset, "prst")?.to_string();
        let adjust = child(preset, "avLst")
            .map(|list| {
                list.elements()
                    .filter(|g| g.local == "gd")
                    .filter_map(|g| {
                        let value = attr(g, "fmla")?.strip_prefix("val ")?.trim();
                        let value: i64 = value.parse().ok()?;
                        Some((
                            attr(g, "name")?.to_string(),
                            value.clamp(-MAX_COORD, MAX_COORD),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        return Some(Geometry::Preset { name, adjust });
    }
    let custom = child(props, "custGeom")?;
    let paths = child(custom, "pathLst")?
        .elements()
        .filter(|p| p.local == "path")
        .map(read_path)
        .collect();
    Some(Geometry::Custom(paths))
}

/// Одна линия `a:path` своего контура. Точки, заданные формулами
/// (по имени из `a:gdLst`), не понимаются: такая команда пропускается.
fn read_path(path: &Element) -> ShapePath {
    let point = |e: &Element| -> Option<(i64, i64)> { Some((number(e, "x")?, number(e, "y")?)) };
    let points = |e: &Element| -> Vec<(i64, i64)> {
        e.elements()
            .filter(|p| p.local == "pt")
            .filter_map(point)
            .collect()
    };
    let mut commands = Vec::new();
    for command in path.elements() {
        let item = match command.local.as_str() {
            "moveTo" => points(command)
                .first()
                .map(|&(x, y)| PathCommand::MoveTo(x, y)),
            "lnTo" => points(command)
                .first()
                .map(|&(x, y)| PathCommand::LineTo(x, y)),
            "cubicBezTo" => match points(command)[..] {
                [a, b, c] => Some(PathCommand::CubicTo([a, b, c])),
                _ => None,
            },
            "quadBezTo" => match points(command)[..] {
                [a, b] => Some(PathCommand::QuadTo([a, b])),
                _ => None,
            },
            "arcTo" => (|| {
                Some(PathCommand::ArcTo {
                    width_radius: number(command, "wR")?,
                    height_radius: number(command, "hR")?,
                    start: number(command, "stAng")?,
                    swing: number(command, "swAng")?,
                })
            })(),
            "close" => Some(PathCommand::Close),
            _ => None,
        };
        commands.extend(item);
    }
    ShapePath {
        width: number(path, "w").filter(|v| *v >= 0).unwrap_or(0),
        height: number(path, "h").filter(|v| *v >= 0).unwrap_or(0),
        fill: attr(path, "fill") != Some("none"),
        stroke: !matches!(attr(path, "stroke"), Some("0" | "false")),
        commands,
    }
}

/// Что сказано о заливке в свойствах фигуры.
enum FillSpec {
    /// Ничего: берётся из стиля фигуры.
    Unset,
    /// Без заливки.
    None,
    /// Цвет.
    Color(ShapeColor),
}

/// Заливка из элементов `a:noFill`, `a:solidFill`, `a:gradFill`,
/// `a:pattFill` среди детей `parent`.
fn read_fill(parent: &Element) -> FillSpec {
    for e in parent.elements() {
        match e.local.as_str() {
            "noFill" => return FillSpec::None,
            "solidFill" => {
                if let Some(color) = e.elements().find_map(read_color) {
                    return FillSpec::Color(color);
                }
            }
            // Градиент — пока цветом первой точки.
            "gradFill" => {
                let color = child(e, "gsLst")
                    .and_then(|l| l.elements().next())
                    .and_then(|gs| gs.elements().find_map(read_color));
                if let Some(color) = color {
                    return FillSpec::Color(color);
                }
            }
            // Узор — пока цветом переднего плана.
            "pattFill" => {
                if let Some(color) =
                    child(e, "fgClr").and_then(|c| c.elements().find_map(read_color))
                {
                    return FillSpec::Color(color);
                }
            }
            // Рисунок вместо заливки пока не показываем.
            "blipFill" => return FillSpec::Color(ShapeColor::rgb([0xF2, 0xF2, 0xF2])),
            _ => {}
        }
    }
    FillSpec::Unset
}

/// Ссылка стиля фигуры (`a:fillRef`, `a:lnRef`): номер и цвет.
fn style_ref(style: Option<&Element>, local: &str) -> Option<(i64, Option<ShapeColor>)> {
    let reference = child(style?, local)?;
    let idx = number(reference, "idx")?;
    Some((idx, reference.elements().find_map(read_color)))
}

fn read_shape_fill(props: Option<&Element>, style: Option<&Element>) -> Option<ShapeColor> {
    match props.map_or(FillSpec::Unset, read_fill) {
        FillSpec::None => None,
        FillSpec::Color(c) => Some(c),
        FillSpec::Unset => match style_ref(style, "fillRef") {
            Some((idx, color)) if idx > 0 => color,
            _ => None,
        },
    }
}

fn read_shape_line(props: Option<&Element>, style: Option<&Element>) -> Option<ShapeLine> {
    let ln = props.and_then(|p| child(p, "ln"));
    let style_line = style_ref(style, "lnRef").filter(|(idx, _)| *idx > 0);
    let color = match ln.map_or(FillSpec::Unset, read_fill) {
        FillSpec::None => return None,
        FillSpec::Color(c) => c,
        FillSpec::Unset => style_line.as_ref()?.1.clone()?,
    };
    // Толщина по умолчанию — из стиля линий темы Office (0,5; 1; 1,5 пт).
    let style_width = match style_line.map(|(idx, _)| idx) {
        Some(1) => 6_350,
        Some(2) => 12_700,
        Some(3..) => 19_050,
        _ => DEFAULT_LINE_WIDTH,
    };
    let width = ln
        .and_then(|l| number(l, "w"))
        .filter(|w| (0..=MAX_COORD).contains(w))
        .unwrap_or(style_width);
    let dash = ln
        .and_then(|l| child(l, "prstDash"))
        .and_then(|d| attr(d, "val"))
        .map_or(LineDash::Solid, |v| match v {
            "solid" => LineDash::Solid,
            "dot" | "sysDot" => LineDash::Dot,
            "dashDot" | "lgDashDot" | "lgDashDotDot" | "sysDashDot" | "sysDashDotDot" => {
                LineDash::DashDot
            }
            _ => LineDash::Dash,
        });
    let end = |local: &str| {
        ln.and_then(|l| child(l, local))
            .and_then(|e| attr(e, "type"))
            .map_or(LineEnd::None, |t| match t {
                "triangle" => LineEnd::Triangle,
                "stealth" => LineEnd::Stealth,
                "arrow" => LineEnd::Arrow,
                "diamond" => LineEnd::Diamond,
                "oval" => LineEnd::Oval,
                _ => LineEnd::None,
            })
    };
    Some(ShapeLine {
        color,
        width,
        dash,
        head: end("headEnd"),
        tail: end("tailEnd"),
    })
}

/// Цвет DrawingML (`a:srgbClr`, `a:schemeClr`, `a:sysClr`, `a:prstClr`,
/// `a:scrgbClr`) с поправками внутри.
fn read_color(e: &Element) -> Option<ShapeColor> {
    let base = match e.local.as_str() {
        "srgbClr" => ColorBase::Rgb(parse_rgb(attr(e, "val")?)?),
        "schemeClr" => ColorBase::Scheme(attr(e, "val")?.to_string()),
        "sysClr" => ColorBase::Rgb(attr(e, "lastClr").and_then(parse_rgb).unwrap_or(
            match attr(e, "val") {
                Some("window") => [255, 255, 255],
                _ => [0, 0, 0],
            },
        )),
        "prstClr" => ColorBase::Rgb(preset_color(attr(e, "val")?)?),
        "scrgbClr" => {
            let part = |n: &str| -> Option<u8> {
                let v = number(e, n)?.clamp(0, 100_000);
                Some((v * 255 / 100_000) as u8)
            };
            ColorBase::Rgb([part("r")?, part("g")?, part("b")?])
        }
        _ => return None,
    };
    let mods = e
        .elements()
        .filter_map(|m| {
            let value: i32 = attr(m, "val")?.parse().ok()?;
            Some(match m.local.as_str() {
                "lumMod" => ColorMod::LumMod(value),
                "lumOff" => ColorMod::LumOff(value),
                "shade" => ColorMod::Shade(value),
                "tint" => ColorMod::Tint(value),
                "alpha" => ColorMod::Alpha(value),
                _ => return None,
            })
        })
        .collect();
    Some(ShapeColor { base, mods })
}

const WPS_NS: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";
const WPG_NS: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup";

/// `a:graphicData` фигуры или группы размером `width` × `height` EMU —
/// для фигуры без исходного XML. Пространство имён `a:` должно быть
/// объявлено выше.
pub(crate) fn graphic_data_xml(shape: &Shape, width: i64, height: i64) -> String {
    let frame = Frame {
        x: 0,
        y: 0,
        width,
        height,
    };
    match &shape.geometry {
        Geometry::Group(group) => format!(
            "<a:graphicData uri=\"{WPG_NS}\"><wpg:wgp xmlns:wpg=\"{WPG_NS}\" \
             xmlns:wps=\"{WPS_NS}\"><wpg:cNvGrpSpPr/>{}</wpg:wgp></a:graphicData>",
            group_body(shape, group, frame, 0)
        ),
        _ => format!(
            "<a:graphicData uri=\"{WPS_NS}\">{}</a:graphicData>",
            wsp_xml(shape, frame, true)
        ),
    }
}

/// Свойства и состав группы (после `cNvGrpSpPr`).
fn group_body(shape: &Shape, group: &Group, frame: Frame, depth: usize) -> String {
    let inner = group.frame;
    let mut out = format!(
        "<wpg:grpSpPr><a:xfrm{}><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/>\
         <a:chOff x=\"{}\" y=\"{}\"/><a:chExt cx=\"{}\" cy=\"{}\"/></a:xfrm></wpg:grpSpPr>",
        transform_attrs(shape),
        frame.x,
        frame.y,
        frame.width,
        frame.height,
        inner.x,
        inner.y,
        inner.width,
        inner.height
    );
    for child in &group.children {
        match &child.shape.geometry {
            Geometry::Group(nested) if depth < MAX_DEPTH => out.push_str(&format!(
                "<wpg:grpSp><wpg:cNvGrpSpPr/>{}</wpg:grpSp>",
                group_body(&child.shape, nested, child.frame, depth + 1)
            )),
            Geometry::Group(_) => {}
            _ => out.push_str(&wsp_xml(&child.shape, child.frame, false)),
        }
    }
    out
}

/// Поворот и отражение (атрибуты `a:xfrm`).
fn transform_attrs(shape: &Shape) -> String {
    let mut out = String::new();
    if shape.rotation != 0 {
        out.push_str(&format!(" rot=\"{}\"", shape.rotation));
    }
    if shape.flip_horizontal {
        out.push_str(" flipH=\"1\"");
    }
    if shape.flip_vertical {
        out.push_str(" flipV=\"1\"");
    }
    out
}

/// Фигура `wps:wsp`; `top` — сама по себе (с объявлением пространства
/// имён), а не в группе.
fn wsp_xml(shape: &Shape, frame: Frame, top: bool) -> String {
    let ns = if top {
        format!(" xmlns:wps=\"{WPS_NS}\"")
    } else {
        String::new()
    };
    let text_box = if shape.text.is_empty() {
        ""
    } else {
        " txBox=\"1\""
    };
    let mut out = format!(
        "<wps:wsp{ns}><wps:cNvSpPr{text_box}/><wps:spPr><a:xfrm{}><a:off x=\"{}\" y=\"{}\"/>\
         <a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm>{}",
        transform_attrs(shape),
        frame.x,
        frame.y,
        frame.width,
        frame.height,
        geometry_xml(&shape.geometry)
    );
    match &shape.fill {
        Some(color) => out.push_str(&format!("<a:solidFill>{}</a:solidFill>", color_xml(color))),
        None => out.push_str("<a:noFill/>"),
    }
    out.push_str(&line_xml(shape.line.as_ref()));
    out.push_str("</wps:spPr>");
    if let Some(color) = &shape.text_color {
        let zero = "<a:srgbClr val=\"000000\"/>";
        out.push_str(&format!(
            "<wps:style><a:lnRef idx=\"0\">{zero}</a:lnRef><a:fillRef idx=\"0\">{zero}</a:fillRef>\
             <a:effectRef idx=\"0\">{zero}</a:effectRef><a:fontRef idx=\"minor\">{}</a:fontRef>\
             </wps:style>",
            color_xml(color)
        ));
    }
    if !shape.text.is_empty() {
        out.push_str(&format!(
            "<wps:txbx><w:txbxContent>{}</w:txbxContent></wps:txbx>",
            crate::serialize::blocks_xml(&shape.text)
        ));
    }
    let anchor = match shape.text_anchor {
        TextAnchor::Top => "t",
        TextAnchor::Center => "ctr",
        TextAnchor::Bottom => "b",
    };
    let [l, t, r, b] = shape.text_insets;
    out.push_str(&format!(
        "<wps:bodyPr lIns=\"{l}\" tIns=\"{t}\" rIns=\"{r}\" bIns=\"{b}\" anchor=\"{anchor}\"/>\
         </wps:wsp>"
    ));
    out
}

/// Контур: `a:prstGeom` или `a:custGeom`.
fn geometry_xml(geometry: &Geometry) -> String {
    match geometry {
        Geometry::Preset { name, adjust } => {
            let guides: String = adjust
                .iter()
                .map(|(name, value)| {
                    format!(
                        "<a:gd name=\"{}\" fmla=\"val {value}\"/>",
                        crate::serialize::escape_attr(name)
                    )
                })
                .collect();
            format!(
                "<a:prstGeom prst=\"{}\"><a:avLst>{guides}</a:avLst></a:prstGeom>",
                crate::serialize::escape_attr(name)
            )
        }
        Geometry::Custom(paths) => {
            let mut out = String::from(
                "<a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/>\
                 <a:rect l=\"l\" t=\"t\" r=\"r\" b=\"b\"/><a:pathLst>",
            );
            for path in paths {
                out.push_str(&format!(
                    "<a:path w=\"{}\" h=\"{}\"",
                    path.width, path.height
                ));
                if !path.fill {
                    out.push_str(" fill=\"none\"");
                }
                if !path.stroke {
                    out.push_str(" stroke=\"0\"");
                }
                out.push('>');
                let pt = |(x, y): (i64, i64)| format!("<a:pt x=\"{x}\" y=\"{y}\"/>");
                for command in &path.commands {
                    out.push_str(&match *command {
                        PathCommand::MoveTo(x, y) => format!("<a:moveTo>{}</a:moveTo>", pt((x, y))),
                        PathCommand::LineTo(x, y) => format!("<a:lnTo>{}</a:lnTo>", pt((x, y))),
                        PathCommand::CubicTo(points) => {
                            format!("<a:cubicBezTo>{}</a:cubicBezTo>", points.map(pt).concat())
                        }
                        PathCommand::QuadTo(points) => {
                            format!("<a:quadBezTo>{}</a:quadBezTo>", points.map(pt).concat())
                        }
                        PathCommand::ArcTo {
                            width_radius,
                            height_radius,
                            start,
                            swing,
                        } => format!(
                            "<a:arcTo wR=\"{width_radius}\" hR=\"{height_radius}\" \
                             stAng=\"{start}\" swAng=\"{swing}\"/>"
                        ),
                        PathCommand::Close => "<a:close/>".to_string(),
                    });
                }
                out.push_str("</a:path>");
            }
            out.push_str("</a:pathLst></a:custGeom>");
            out
        }
        Geometry::Group(_) => "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>".to_string(),
    }
}

/// Линия контура `a:ln`; `None` — без линии.
fn line_xml(line: Option<&ShapeLine>) -> String {
    let Some(line) = line else {
        return "<a:ln><a:noFill/></a:ln>".to_string();
    };
    let dash = match line.dash {
        LineDash::Solid => "solid",
        LineDash::Dot => "sysDot",
        LineDash::Dash => "dash",
        LineDash::DashDot => "dashDot",
    };
    let end = |name: &str, end: LineEnd| {
        let kind = match end {
            LineEnd::None => return String::new(),
            LineEnd::Triangle => "triangle",
            LineEnd::Stealth => "stealth",
            LineEnd::Arrow => "arrow",
            LineEnd::Diamond => "diamond",
            LineEnd::Oval => "oval",
        };
        format!("<a:{name} type=\"{kind}\"/>")
    };
    format!(
        "<a:ln w=\"{}\"><a:solidFill>{}</a:solidFill><a:prstDash val=\"{dash}\"/>{}{}</a:ln>",
        line.width,
        color_xml(&line.color),
        end("headEnd", line.head),
        end("tailEnd", line.tail)
    )
}

/// Цвет: `a:srgbClr` или `a:schemeClr` с поправками.
fn color_xml(color: &ShapeColor) -> String {
    let mods: String = color
        .mods
        .iter()
        .map(|m| {
            let (name, value) = match *m {
                ColorMod::LumMod(v) => ("lumMod", v),
                ColorMod::LumOff(v) => ("lumOff", v),
                ColorMod::Shade(v) => ("shade", v),
                ColorMod::Tint(v) => ("tint", v),
                ColorMod::Alpha(v) => ("alpha", v),
            };
            format!("<a:{name} val=\"{value}\"/>")
        })
        .collect();
    match &color.base {
        ColorBase::Rgb([r, g, b]) => {
            format!("<a:srgbClr val=\"{r:02X}{g:02X}{b:02X}\">{mods}</a:srgbClr>")
        }
        ColorBase::Scheme(name) => format!(
            "<a:schemeClr val=\"{}\">{mods}</a:schemeClr>",
            crate::serialize::escape_attr(name)
        ),
    }
}

/// Несколько самых частых именованных цветов (`a:prstClr`).
fn preset_color(name: &str) -> Option<[u8; 3]> {
    Some(match name {
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" => [0, 128, 0],
        "lime" => [0, 255, 0],
        "blue" => [0, 0, 255],
        "yellow" => [255, 255, 0],
        "cyan" | "aqua" => [0, 255, 255],
        "magenta" | "fuchsia" => [255, 0, 255],
        "gray" | "grey" => [128, 128, 128],
        "darkGray" | "dkGray" => [169, 169, 169],
        "lightGray" | "ltGray" => [211, 211, 211],
        "orange" => [255, 165, 0],
        "navy" => [0, 0, 128],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml;

    const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape" xmlns:wpg="http://schemas.microsoft.com/office/word/2010/wordprocessingGroup" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

    fn parse(inner: &str) -> Option<Shape> {
        let text = format!(r#"<a:graphicData {NS}>{inner}</a:graphicData>"#);
        read_graphic_shape(&xml::parse(text.as_bytes()).unwrap())
    }

    #[test]
    fn default_word_shape_takes_colors_from_style() {
        let shape = parse(
            r#"<wps:wsp><wps:cNvSpPr/><wps:spPr><a:xfrm rot="5400000" flipH="1"><a:off x="0" y="0"/><a:ext cx="100" cy="50"/></a:xfrm><a:prstGeom prst="roundRect"><a:avLst><a:gd name="adj" fmla="val 25000"/></a:avLst></a:prstGeom></wps:spPr><wps:style><a:lnRef idx="2"><a:schemeClr val="accent1"><a:shade val="15000"/></a:schemeClr></a:lnRef><a:fillRef idx="1"><a:schemeClr val="accent1"/></a:fillRef><a:effectRef idx="0"><a:schemeClr val="accent1"/></a:effectRef><a:fontRef idx="minor"><a:schemeClr val="lt1"/></a:fontRef></wps:style><wps:bodyPr lIns="0" anchor="ctr"/></wps:wsp>"#,
        )
        .unwrap();
        assert_eq!(
            shape.geometry,
            Geometry::Preset {
                name: "roundRect".into(),
                adjust: vec![("adj".into(), 25_000)],
            }
        );
        assert_eq!(shape.rotation, 5_400_000);
        assert!(shape.flip_horizontal && !shape.flip_vertical);
        let fill = shape.fill.unwrap();
        assert_eq!(fill.base, ColorBase::Scheme("accent1".into()));
        let line = shape.line.unwrap();
        assert_eq!(line.width, 12_700);
        assert_eq!(line.color.mods, vec![ColorMod::Shade(15_000)]);
        assert_eq!(shape.text_insets, [0, 45_720, 91_440, 45_720]);
        assert_eq!(shape.text_anchor, TextAnchor::Center);
        let text = shape.text_color.unwrap();
        assert_eq!(text.base, ColorBase::Scheme("lt1".into()));
    }

    #[test]
    fn explicit_fill_and_arrow_line() {
        let shape = parse(
            r#"<wps:wsp><wps:cNvCnPr/><wps:spPr><a:prstGeom prst="straightConnector1"><a:avLst/></a:prstGeom><a:noFill/><a:ln w="28575"><a:solidFill><a:srgbClr val="FF0000"><a:alpha val="50000"/></a:srgbClr></a:solidFill><a:prstDash val="sysDash"/><a:tailEnd type="triangle"/></a:ln></wps:spPr><wps:style><a:lnRef idx="1"><a:schemeClr val="accent1"/></a:lnRef><a:fillRef idx="1"><a:schemeClr val="accent1"/></a:fillRef></wps:style><wps:bodyPr/></wps:wsp>"#,
        )
        .unwrap();
        assert_eq!(shape.fill, None);
        let line = shape.line.unwrap();
        assert_eq!(line.width, 28_575);
        assert_eq!(line.dash, LineDash::Dash);
        assert_eq!((line.head, line.tail), (LineEnd::None, LineEnd::Triangle));
        assert_eq!(line.color.base, ColorBase::Rgb([255, 0, 0]));
        assert_eq!(line.color.mods, vec![ColorMod::Alpha(50_000)]);
        // Без линии.
        let shape = parse(
            r#"<wps:wsp><wps:spPr><a:prstGeom prst="ellipse"/><a:solidFill><a:prstClr val="yellow"/></a:solidFill><a:ln><a:noFill/></a:ln></wps:spPr><wps:bodyPr/></wps:wsp>"#,
        )
        .unwrap();
        assert_eq!(shape.fill, Some(ShapeColor::rgb([255, 255, 0])));
        assert_eq!(shape.line, None);
    }

    #[test]
    fn text_box_and_custom_path() {
        let shape = parse(
            r#"<wps:wsp><wps:spPr><a:custGeom><a:pathLst><a:path w="10" h="20" fill="none"><a:moveTo><a:pt x="0" y="0"/></a:moveTo><a:lnTo><a:pt x="10" y="0"/></a:lnTo><a:cubicBezTo><a:pt x="1" y="2"/><a:pt x="3" y="4"/><a:pt x="5" y="6"/></a:cubicBezTo><a:arcTo wR="5" hR="5" stAng="0" swAng="5400000"/><a:lnTo><a:pt x="g1" y="0"/></a:lnTo><a:close/></a:path></a:pathLst></a:custGeom></wps:spPr><wps:txbx><w:txbxContent><w:p><w:r><w:t>Привет</w:t></w:r></w:p></w:txbxContent></wps:txbx><wps:bodyPr/></wps:wsp>"#,
        )
        .unwrap();
        let Geometry::Custom(paths) = &shape.geometry else {
            panic!("{:?}", shape.geometry);
        };
        assert_eq!((paths[0].width, paths[0].height), (10, 20));
        assert!(!paths[0].fill && paths[0].stroke);
        assert_eq!(paths[0].commands.len(), 5);
        assert!(matches!(
            paths[0].commands[3],
            PathCommand::ArcTo {
                swing: 5_400_000,
                ..
            }
        ));
        assert_eq!(shape.text.len(), 1);
        assert_eq!(shape.text[0].plain_text(), "Привет");
        assert_eq!(shape.fill, None);
        assert_eq!(shape.line, None);
    }

    #[test]
    fn group_with_nested_group() {
        let shape = parse(
            r#"<wpg:wgp><wpg:cNvGrpSpPr/><wpg:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="200" cy="100"/><a:chOff x="10" y="10"/><a:chExt cx="400" cy="200"/></a:xfrm></wpg:grpSpPr><wps:wsp><wps:spPr><a:xfrm><a:off x="10" y="10"/><a:ext cx="100" cy="100"/></a:xfrm><a:prstGeom prst="ellipse"/></wps:spPr><wps:bodyPr/></wps:wsp><wpg:grpSp><wpg:grpSpPr><a:xfrm><a:off x="200" y="10"/><a:ext cx="200" cy="200"/><a:chOff x="0" y="0"/><a:chExt cx="1" cy="1"/></a:xfrm></wpg:grpSpPr></wpg:grpSp><wps:wsp><wps:spPr/></wps:wsp></wpg:wgp>"#,
        )
        .unwrap();
        let Geometry::Group(group) = &shape.geometry else {
            panic!("{:?}", shape.geometry);
        };
        assert_eq!(
            group.frame,
            Frame {
                x: 10,
                y: 10,
                width: 400,
                height: 200
            }
        );
        // Фигура без места пропускается.
        assert_eq!(group.children.len(), 2);
        assert_eq!(
            group.children[0].shape.geometry,
            Geometry::preset("ellipse")
        );
        assert_eq!(group.children[1].frame.x, 200);
        assert!(matches!(
            group.children[1].shape.geometry,
            Geometry::Group(_)
        ));
        assert!(parse("<a:other/>").is_none());
    }
}
