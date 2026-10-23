//! Рисунки: `w:drawing` с `wp:inline` (в строке) или `wp:anchor`
//! (плавающий) внутри.
//!
//! Из рисунка программа понимает размер, ссылку на файл картинки
//! (`a:blip r:embed`), а у плавающего — положение и обтекание. Весь
//! элемент хранится как есть ([`Picture::raw`]), а при записи в нём
//! меняется только то, что изменилось в модели.

use letter_model::{
    Anchor, AnchorAlign, AnchorPosition, Distance, HorizontalFrom, Picture, Placement, RawXml,
    VerticalFrom, Wrap,
};

use crate::serialize::escape_attr;
use crate::xml::{self, Element, Node};

const WP_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const PIC_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// Самый большой размер рисунка, EMU (около 5,6 м — больше Word не
/// допускает). Защищает вёрстку от бессмысленных чисел.
const MAX_EMU: i64 = 20_116_800 * 10;

/// Читает `w:drawing`: картинку или фигуру ([`Picture::shape`]).
/// `None`, если это что-то другое (диаграмма, SmartArt) или разметка
/// непонятна: тогда элемент сохраняется без разбора.
pub(crate) fn read_picture(drawing: &Element) -> Option<Picture> {
    let mut children = drawing.elements();
    let frame = children.next()?;
    if children.next().is_some() {
        return None;
    }
    let anchor = match frame.local.as_str() {
        "inline" => None,
        "anchor" => Some(Box::new(read_anchor(frame)?)),
        _ => return None,
    };
    let inline = frame;
    let extent = inline.elements().find(|e| e.local == "extent")?;
    let size = |name: &str| {
        let value: i64 = attr(extent, name)?.parse().ok()?;
        (0..=MAX_EMU).contains(&value).then_some(value)
    };
    let (width, height) = (size("cx")?, size("cy")?);
    let graphic = inline.elements().find(|e| e.local == "graphic")?;
    let data = graphic.elements().find(|e| e.local == "graphicData")?;
    if let Some(shape) = crate::shape::read_graphic_shape(data) {
        return Some(Picture {
            width,
            height,
            image: None,
            relationship: None,
            raw: Some(drawing.raw()),
            anchor,
            shape: Some(std::sync::Arc::new(shape)),
        });
    }
    let pic = data.elements().find(|e| e.local == "pic")?;
    let fill = pic.elements().find(|e| e.local == "blipFill")?;
    let blip = fill.elements().find(|e| e.local == "blip")?;
    let relationship = attr(blip, "embed")?.to_string();
    Some(Picture {
        width,
        height,
        image: None,
        relationship: Some(relationship),
        raw: Some(drawing.raw()),
        anchor,
        shape: None,
    })
}

/// Положение и обтекание из `wp:anchor`.
fn read_anchor(anchor: &Element) -> Option<Anchor> {
    let distance = |name: &str| -> Option<i64> {
        match attr(anchor, name) {
            Some(v) => v.parse().ok().filter(|d: &i64| (0..=MAX_EMU).contains(d)),
            None => Some(0),
        }
    };
    let distance = Distance {
        top: distance("distT")?,
        bottom: distance("distB")?,
        left: distance("distL")?,
        right: distance("distR")?,
    };
    let behind_text = matches!(attr(anchor, "behindDoc"), Some("1" | "true"));
    let (horizontal, vertical) = if matches!(attr(anchor, "simplePos"), Some("1" | "true")) {
        // Простое положение — от угла листа.
        let simple = anchor.elements().find(|e| e.local == "simplePos")?;
        let x: i64 = attr(simple, "x")?.parse().ok()?;
        let y: i64 = attr(simple, "y")?.parse().ok()?;
        (
            AnchorPosition {
                relative: HorizontalFrom::Page,
                placement: Placement::Offset(x),
            },
            AnchorPosition {
                relative: VerticalFrom::Page,
                placement: Placement::Offset(y),
            },
        )
    } else {
        let h = anchor.elements().find(|e| e.local == "positionH")?;
        let v = anchor.elements().find(|e| e.local == "positionV")?;
        (
            AnchorPosition {
                relative: lookup(HORIZONTAL_FROM, attr(h, "relativeFrom")?)?,
                placement: read_placement(h)?,
            },
            AnchorPosition {
                relative: lookup(VERTICAL_FROM, attr(v, "relativeFrom")?)?,
                placement: read_placement(v)?,
            },
        )
    };
    let wrap = anchor
        .elements()
        .find_map(|e| lookup(WRAPS, &e.local))
        .unwrap_or(Wrap::None);
    Some(Anchor {
        horizontal,
        vertical,
        wrap,
        behind_text,
        distance,
    })
}

/// `wp:posOffset` или `wp:align` внутри `wp:positionH` и `wp:positionV`.
fn read_placement(position: &Element) -> Option<Placement> {
    let child = position.elements().next()?;
    let text = child.text();
    let text = text.trim();
    match child.local.as_str() {
        "posOffset" => {
            let offset: i64 = text.parse().ok()?;
            (offset.abs() <= MAX_EMU).then_some(Placement::Offset(offset))
        }
        "align" => Some(Placement::Align(lookup(ALIGNS, text)?)),
        _ => None,
    }
}

fn lookup<T: Copy>(table: &[(&str, T)], name: &str) -> Option<T> {
    table.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

fn name_of<T: Copy + PartialEq>(table: &[(&'static str, T)], value: T) -> &'static str {
    table
        .iter()
        .find(|(_, v)| *v == value)
        .map_or("", |(n, _)| n)
}

const HORIZONTAL_FROM: &[(&str, HorizontalFrom)] = &[
    ("margin", HorizontalFrom::Margin),
    ("page", HorizontalFrom::Page),
    ("column", HorizontalFrom::Column),
    ("character", HorizontalFrom::Character),
    ("leftMargin", HorizontalFrom::LeftMargin),
    ("rightMargin", HorizontalFrom::RightMargin),
    ("insideMargin", HorizontalFrom::InsideMargin),
    ("outsideMargin", HorizontalFrom::OutsideMargin),
];

const VERTICAL_FROM: &[(&str, VerticalFrom)] = &[
    ("margin", VerticalFrom::Margin),
    ("page", VerticalFrom::Page),
    ("paragraph", VerticalFrom::Paragraph),
    ("line", VerticalFrom::Line),
    ("topMargin", VerticalFrom::TopMargin),
    ("bottomMargin", VerticalFrom::BottomMargin),
    ("insideMargin", VerticalFrom::InsideMargin),
    ("outsideMargin", VerticalFrom::OutsideMargin),
];

/// Выравнивание; по горизонтали «left»/«right», по вертикали «top»/«bottom».
const ALIGNS: &[(&str, AnchorAlign)] = &[
    ("left", AnchorAlign::Start),
    ("top", AnchorAlign::Start),
    ("center", AnchorAlign::Center),
    ("right", AnchorAlign::End),
    ("bottom", AnchorAlign::End),
    ("inside", AnchorAlign::Inside),
    ("outside", AnchorAlign::Outside),
];

const WRAPS: &[(&str, Wrap)] = &[
    ("wrapNone", Wrap::None),
    ("wrapSquare", Wrap::Square),
    ("wrapTight", Wrap::Tight),
    ("wrapThrough", Wrap::Through),
    ("wrapTopAndBottom", Wrap::TopAndBottom),
];

/// Атрибут по имени без префикса.
fn attr<'a>(element: &'a Element, local: &str) -> Option<&'a str> {
    element
        .attrs
        .iter()
        .find(|a| a.local == local)
        .map(|a| a.value.as_str())
}

/// `w:drawing` рисунка. `id` — номер рисунка для `wp:docPr` (нужен
/// только новому рисунку). Если у рисунка нет связи, он не записывается.
pub(crate) fn picture_xml(picture: &Picture, id: usize) -> String {
    if let Some(raw) = &picture.raw {
        return patched(raw, picture);
    }
    let Some(relationship) = &picture.relationship else {
        return String::new();
    };
    let name = picture
        .image
        .as_ref()
        .and_then(|i| i.name.rsplit('/').next())
        .unwrap_or("image");
    let (cx, cy) = (picture.width, picture.height);
    let name = escape_attr(name);
    let relationship = escape_attr(relationship);
    // Плавающий рисунок пишется как рисунок в строке, потом становится
    // `wp:anchor` (для разбора нужно объявление `w:`).
    let w = if picture.anchor.is_some() {
        format!(" xmlns:w=\"{}\"", crate::document::W_NS)
    } else {
        String::new()
    };
    let inline = format!(
        "<w:drawing{w}><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" \
         xmlns:wp=\"{WP_NS}\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/>\
         <wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/>\
         <wp:docPr id=\"{id}\" name=\"{name}\"/>\
         <wp:cNvGraphicFramePr><a:graphicFrameLocks xmlns:a=\"{A_NS}\" noChangeAspect=\"1\"/>\
         </wp:cNvGraphicFramePr><a:graphic xmlns:a=\"{A_NS}\">\
         <a:graphicData uri=\"{PIC_NS}\"><pic:pic xmlns:pic=\"{PIC_NS}\"><pic:nvPicPr>\
         <pic:cNvPr id=\"0\" name=\"{name}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill>\
         <a:blip r:embed=\"{relationship}\" xmlns:r=\"{R_NS}\"/><a:stretch><a:fillRect/></a:stretch>\
         </pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/>\
         </a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>\
         </a:graphicData></a:graphic></wp:inline></w:drawing>"
    );
    if picture.anchor.is_some() {
        patched(&RawXml::new(inline), picture)
    } else {
        inline
    }
}

/// Исходный `w:drawing` с размером, положением и обтеканием из модели
/// (`wp:extent`, `a:ext`, `wp:positionH`/`V`, `wp:wrap…`). Если ничего
/// не менялось (или XML не разбирается), — как есть, байт в байт.
fn patched(raw: &RawXml, picture: &Picture) -> String {
    let Ok(mut root) = xml::parse(raw.as_str().as_bytes()) else {
        return raw.as_str().to_string();
    };
    if root.local == "AlternateContent" {
        // Фигура Word 2010 и новее с запасным VML для старых программ:
        // меняется `w:drawing` из `mc:Choice`, а запасной вид не
        // записывается — он показывал бы старый размер и место.
        return match alternate_drawing(&mut root) {
            Some(drawing) => {
                if patch_drawing(drawing, picture) {
                    drawing.to_xml_fragment()
                } else {
                    raw.as_str().to_string()
                }
            }
            None => raw.as_str().to_string(),
        };
    }
    if patch_drawing(&mut root, picture) {
        root.to_xml_fragment()
    } else {
        raw.as_str().to_string()
    }
}

/// `w:drawing` из `mc:Choice` в `mc:AlternateContent`.
fn alternate_drawing(root: &mut Element) -> Option<&mut Element> {
    root.children.iter_mut().find_map(|n| match n {
        Node::Element(choice) if choice.local == "Choice" => {
            choice.children.iter_mut().find_map(|n| match n {
                Node::Element(d) if d.local == "drawing" => Some(d),
                _ => None,
            })
        }
        _ => None,
    })
}

/// Фигура Word 2010 и новее: `mc:AlternateContent`, в `mc:Choice`
/// которого `w:drawing` с фигурой. `None`, если там не фигура.
pub(crate) fn read_alternate(element: &Element) -> Option<Picture> {
    let drawing = element
        .elements()
        .filter(|c| c.local == "Choice")
        .flat_map(Element::elements)
        .find(|d| d.local == "drawing" && crate::document::is_w_ns(d.ns.as_deref()))?;
    let mut picture = read_picture(drawing)?;
    picture.shape.as_ref()?;
    picture.raw = Some(element.raw());
    Some(picture)
}

/// Записывает в `w:drawing` размер, положение и обтекание из модели.
/// `true`, если что-то поменялось.
fn patch_drawing(root: &mut Element, picture: &Picture) -> bool {
    let Some(Node::Element(inline)) = root
        .children
        .iter_mut()
        .find(|n| matches!(n, Node::Element(e) if e.local == "inline" || e.local == "anchor"))
    else {
        return false;
    };
    let mut changed = false;
    match (&picture.anchor, inline.local == "anchor") {
        (Some(anchor), true) => {
            if read_anchor(inline).as_ref() != Some(&**anchor) {
                set_anchor(inline, anchor);
                changed = true;
            }
        }
        (Some(anchor), false) => {
            to_anchor(inline);
            set_anchor(inline, anchor);
            changed = true;
        }
        (None, true) => {
            to_inline(inline);
            changed = true;
        }
        (None, false) => {}
    }
    let (cx, cy) = (picture.width.to_string(), picture.height.to_string());
    for node in &mut inline.children {
        let Node::Element(e) = node else { continue };
        if e.local == "extent" {
            changed |= set_size(e, &cx, &cy);
        }
    }
    // a:graphic/a:graphicData/pic:pic/pic:spPr/a:xfrm/a:ext у картинки,
    // wps:wsp/wps:spPr у фигуры и wpg:wgp/wpg:grpSpPr у группы.
    for path in [["pic", "spPr"], ["wsp", "spPr"], ["wgp", "grpSpPr"]] {
        let path = ["graphic", "graphicData", path[0], path[1], "xfrm"];
        if let Some(xfrm) = descend(inline, &path) {
            for node in &mut xfrm.children {
                let Node::Element(e) = node else { continue };
                if e.local == "ext" {
                    changed |= set_size(e, &cx, &cy);
                }
            }
        }
    }
    changed
}

/// Переименовывает элемент `wp:inline` или `wp:anchor` в `local`.
fn rename(element: &mut Element, local: &str) {
    element.qname = match element.qname.split_once(':') {
        Some((prefix, _)) => format!("{prefix}:{local}"),
        None => local.to_string(),
    };
    element.local = local.to_string();
}

/// Делает из `wp:inline` плавающий `wp:anchor` (положение и обтекание
/// потом ставит [`set_anchor`]): обязательные атрибуты и `wp:simplePos`.
fn to_anchor(element: &mut Element) {
    rename(element, "anchor");
    for (name, value) in [
        ("relativeHeight", "0"),
        ("locked", "0"),
        ("layoutInCell", "1"),
        ("allowOverlap", "1"),
    ] {
        if !element.attrs.iter().any(|a| a.local == name) {
            element.attrs.push(xml::Attr {
                qname: name.to_string(),
                ns: None,
                local: name.to_string(),
                value: value.to_string(),
            });
        }
    }
    let prefix = element
        .qname
        .split_once(':')
        .map_or(String::new(), |(p, _)| format!("{p}:"));
    let simple = element_in(element, &format!("<{prefix}simplePos x=\"0\" y=\"0\"/>"));
    replace_child(element, |e| e.local == "simplePos", simple, &[]);
}

/// Делает из `wp:anchor` рисунок в строке `wp:inline`: остаются только
/// расстояния до текста, размер, описание и сама картинка.
fn to_inline(element: &mut Element) {
    rename(element, "inline");
    element
        .attrs
        .retain(|a| matches!(a.local.as_str(), "distT" | "distB" | "distL" | "distR"));
    element.children.retain(|node| match node {
        Node::Element(e) => matches!(
            e.local.as_str(),
            "extent" | "effectExtent" | "docPr" | "cNvGraphicFramePr" | "graphic"
        ),
        _ => true,
    });
}

/// Записывает в `wp:anchor` положение и обтекание: атрибуты расстояний
/// и «за текстом», `wp:positionH`/`V` и элемент обтекания на их местах.
fn set_anchor(anchor: &mut Element, model: &Anchor) {
    let prefix = anchor
        .qname
        .split_once(':')
        .map_or(String::new(), |(p, _)| format!("{p}:"));
    let d = model.distance;
    for (name, value) in [
        ("distT", d.top.to_string()),
        ("distB", d.bottom.to_string()),
        ("distL", d.left.to_string()),
        ("distR", d.right.to_string()),
        ("behindDoc", u8::from(model.behind_text).to_string()),
        ("simplePos", "0".to_string()),
    ] {
        match anchor.attrs.iter_mut().find(|a| a.local == name) {
            Some(a) => a.value = value,
            None => anchor.attrs.push(xml::Attr {
                qname: name.to_string(),
                ns: None,
                local: name.to_string(),
                value,
            }),
        }
    }
    let placement = |placement: Placement, vertical: bool| match placement {
        Placement::Offset(offset) => format!("<{prefix}posOffset>{offset}</{prefix}posOffset>"),
        Placement::Align(align) => {
            let name = match (align, vertical) {
                (AnchorAlign::Start, false) => "left",
                (AnchorAlign::Start, true) => "top",
                (AnchorAlign::End, false) => "right",
                (AnchorAlign::End, true) => "bottom",
                (other, _) => name_of(ALIGNS, other),
            };
            format!("<{prefix}align>{name}</{prefix}align>")
        }
    };
    let h = format!(
        "<{prefix}positionH relativeFrom=\"{}\">{}</{prefix}positionH>",
        name_of(HORIZONTAL_FROM, model.horizontal.relative),
        placement(model.horizontal.placement, false)
    );
    let v = format!(
        "<{prefix}positionV relativeFrom=\"{}\">{}</{prefix}positionV>",
        name_of(VERTICAL_FROM, model.vertical.relative),
        placement(model.vertical.placement, true)
    );
    let wrap = match model.wrap {
        Wrap::None => format!("<{prefix}wrapNone/>"),
        Wrap::TopAndBottom => format!("<{prefix}wrapTopAndBottom/>"),
        Wrap::Square => format!("<{prefix}wrapSquare wrapText=\"bothSides\"/>"),
        // Контур — прямоугольник рисунка.
        Wrap::Tight | Wrap::Through => {
            let name = name_of(WRAPS, model.wrap);
            format!(
                "<{prefix}{name} wrapText=\"bothSides\"><{prefix}wrapPolygon edited=\"0\">\
                 <{prefix}start x=\"0\" y=\"0\"/><{prefix}lineTo x=\"0\" y=\"21600\"/>\
                 <{prefix}lineTo x=\"21600\" y=\"21600\"/><{prefix}lineTo x=\"21600\" y=\"0\"/>\
                 <{prefix}lineTo x=\"0\" y=\"0\"/></{prefix}wrapPolygon></{prefix}{name}>"
            )
        }
    };
    let make = |text: String| element_in(anchor, &text);
    let (h, v, wrap) = (make(h), make(v), make(wrap));
    replace_child(anchor, |e| e.local == "positionH", h, &["simplePos"]);
    replace_child(anchor, |e| e.local == "positionV", v, &["positionH"]);
    // Старое обтекание — с тем же условием, что и при чтении: первый
    // элемент из списка WRAPS (после положения и размеров).
    let old_wrap = anchor
        .elements()
        .find(|e| lookup(WRAPS, &e.local).is_some())
        .map(|e| e.local.clone());
    match old_wrap {
        Some(old) => replace_child(anchor, |e| e.local == old, wrap, &[]),
        None => replace_child(anchor, |_| false, wrap, &["effectExtent", "extent"]),
    }
}

/// Элемент из текста XML в пространствах имён элемента `parent`.
fn element_in(parent: &Element, text: &str) -> Option<Element> {
    let decls: String = parent
        .scope
        .iter()
        .map(|(prefix, uri)| {
            if prefix.is_empty() {
                format!(" xmlns=\"{}\"", escape_attr(uri))
            } else {
                format!(" xmlns:{prefix}=\"{}\"", escape_attr(uri))
            }
        })
        .collect();
    let wrapped = format!("<x{decls}>{text}</x>");
    let root = xml::parse(wrapped.as_bytes()).ok()?;
    let mut element = root.elements().next()?.clone();
    clear_decls(&mut element, &parent.scope);
    Some(element)
}

/// Убирает объявления пространств имён, видимые и так, и ставит
/// элементу область видимости родителя.
fn clear_decls(element: &mut Element, scope: &xml::Scope) {
    element.decls.clear();
    element.scope = scope.clone();
    for node in &mut element.children {
        if let Node::Element(child) = node {
            clear_decls(child, scope);
        }
    }
}

/// Заменяет первый дочерний элемент, подходящий под `matches`, на
/// `new`; если такого нет — вставляет `new` после первого найденного
/// из `after` (или в начало).
fn replace_child(
    parent: &mut Element,
    matches: impl Fn(&Element) -> bool,
    new: Option<Element>,
    after: &[&str],
) {
    let Some(new) = new else {
        return;
    };
    if let Some(node) = parent
        .children
        .iter_mut()
        .find(|n| matches!(n, Node::Element(e) if matches(e)))
    {
        *node = Node::Element(new);
        return;
    }
    let at = after
        .iter()
        .find_map(|name| {
            parent
                .children
                .iter()
                .position(|n| matches!(n, Node::Element(e) if e.local == *name))
        })
        .map_or(0, |i| i + 1);
    parent.children.insert(at, Node::Element(new));
}

/// Исходный `w:drawing` со ссылкой `a:blip r:embed` на связь `id`.
/// Дополнения картинки (`a:extLst`, например SVG рядом с PNG) убираются:
/// их связи остались в другом документе.
pub(crate) fn with_relationship(raw: &RawXml, id: &str) -> RawXml {
    let Ok(mut root) = xml::parse(raw.as_str().as_bytes()) else {
        return raw.clone();
    };
    let path = ["graphic", "graphicData", "pic", "blipFill", "blip"];
    let frame = root.children.iter_mut().find_map(|n| match n {
        Node::Element(e) if e.local == "inline" || e.local == "anchor" => Some(e),
        _ => None,
    });
    let Some(blip) = frame.and_then(|f| descend(f, &path)) else {
        return raw.clone();
    };
    for a in &mut blip.attrs {
        if a.local == "embed" {
            a.value = id.to_string();
        }
    }
    blip.children
        .retain(|n| !matches!(n, Node::Element(e) if e.local == "extLst"));
    RawXml::new(root.to_xml_fragment())
}

/// Ставит атрибуты `cx` и `cy`; `true`, если что-то изменилось.
fn set_size(element: &mut Element, cx: &str, cy: &str) -> bool {
    let mut changed = false;
    for a in &mut element.attrs {
        let value = match a.local.as_str() {
            "cx" => cx,
            "cy" => cy,
            _ => continue,
        };
        if a.value != value {
            a.value = value.to_string();
            changed = true;
        }
    }
    changed
}

/// Спускается по цепочке дочерних элементов с такими именами.
fn descend<'a>(element: &'a mut Element, path: &[&str]) -> Option<&'a mut Element> {
    let Some((first, rest)) = path.split_first() else {
        return Some(element);
    };
    let child = element.children.iter_mut().find_map(|n| match n {
        Node::Element(e) if e.local == *first => Some(e),
        _ => None,
    })?;
    descend(child, rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DRAWING: &str = r#"<w:drawing xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><wp:inline><wp:extent cx="100" cy="200"/><wp:docPr id="1" name="P" descr="Кот"/><a:graphic><a:graphicData uri="x"><pic:pic><pic:blipFill><a:blip r:embed="rId9"/></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="100" cy="200"/></a:xfrm></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing>"#;

    fn parse(text: &str) -> Element {
        xml::parse(text.as_bytes()).unwrap()
    }

    #[test]
    fn inline_picture_is_read() {
        let picture = read_picture(&parse(DRAWING)).unwrap();
        assert_eq!((picture.width, picture.height), (100, 200));
        assert_eq!(picture.relationship.as_deref(), Some("rId9"));
        assert!(picture.raw.is_some());
    }

    #[test]
    fn broken_drawings_stay_raw() {
        let negative = DRAWING.replace(
            r#"cx="100" cy="200"/><wp:docPr"#,
            r#"cx="-5" cy="200"/><wp:docPr"#,
        );
        assert!(read_picture(&parse(&negative)).is_none());
        let no_blip = DRAWING.replace("a:blip", "a:other");
        assert!(read_picture(&parse(&no_blip)).is_none());
    }

    #[test]
    fn resize_changes_only_the_size() {
        let mut picture = read_picture(&parse(DRAWING)).unwrap();
        // Размер не менялся — байт в байт.
        assert_eq!(picture_xml(&picture, 1), DRAWING);
        picture.width = 300;
        picture.height = 600;
        let xml = picture_xml(&picture, 1);
        let again = read_picture(&parse(&xml)).unwrap();
        assert_eq!((again.width, again.height), (300, 600));
        assert!(xml.contains(r#"<a:ext cx="300" cy="600"/>"#), "{xml}");
        assert!(xml.contains(r#"descr="Кот""#), "{xml}");
    }

    const ANCHOR: &str = r#"<w:drawing xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><wp:anchor distT="0" distB="0" distL="114300" distR="114300" simplePos="0" relativeHeight="251659264" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1"><wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="column"><wp:posOffset>3200400</wp:posOffset></wp:positionH><wp:positionV relativeFrom="paragraph"><wp:align>top</wp:align></wp:positionV><wp:extent cx="100" cy="200"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:wrapSquare wrapText="bothSides"/><wp:docPr id="2" name="P"/><a:graphic><a:graphicData uri="x"><pic:pic><pic:blipFill><a:blip r:embed="rId4"/></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="100" cy="200"/></a:xfrm></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:anchor></w:drawing>"#;

    #[test]
    fn floating_picture_is_read() {
        let picture = read_picture(&parse(ANCHOR)).unwrap();
        assert_eq!((picture.width, picture.height), (100, 200));
        assert_eq!(picture.relationship.as_deref(), Some("rId4"));
        let anchor = picture.anchor.unwrap();
        assert_eq!(
            anchor.horizontal,
            AnchorPosition {
                relative: HorizontalFrom::Column,
                placement: Placement::Offset(3_200_400)
            }
        );
        assert_eq!(
            anchor.vertical,
            AnchorPosition {
                relative: VerticalFrom::Paragraph,
                placement: Placement::Align(AnchorAlign::Start)
            }
        );
        assert_eq!(anchor.wrap, Wrap::Square);
        assert!(!anchor.behind_text);
        assert_eq!(anchor.distance.left, 114_300);
        // Простое положение — от угла листа.
        let simple = ANCHOR
            .replace(r#"simplePos="0""#, r#"simplePos="1""#)
            .replace(
                r#"<wp:simplePos x="0" y="0"/>"#,
                r#"<wp:simplePos x="5" y="7"/>"#,
            );
        let anchor = read_picture(&parse(&simple)).unwrap().anchor.unwrap();
        assert_eq!(anchor.horizontal.placement, Placement::Offset(5));
        assert_eq!(anchor.vertical.relative, VerticalFrom::Page);
        // Непонятное положение — элемент остаётся без разбора.
        let odd = ANCHOR.replace(r#"relativeFrom="column""#, r#"relativeFrom="moon""#);
        assert!(read_picture(&parse(&odd)).is_none());
    }

    #[test]
    fn floating_picture_is_written_back() {
        let mut picture = read_picture(&parse(ANCHOR)).unwrap();
        // Без изменений — байт в байт.
        assert_eq!(picture_xml(&picture, 1), ANCHOR);
        let anchor = picture.anchor.as_mut().unwrap();
        anchor.horizontal = AnchorPosition {
            relative: HorizontalFrom::Page,
            placement: Placement::Align(AnchorAlign::Center),
        };
        anchor.vertical.placement = Placement::Offset(-12_700);
        anchor.wrap = Wrap::Tight;
        anchor.behind_text = true;
        let expected = picture.anchor.clone();
        let xml = picture_xml(&picture, 1);
        let again = read_picture(&parse(&xml)).unwrap();
        assert_eq!(again.anchor, expected, "{xml}");
        assert!(xml.contains("<wp:align>center</wp:align>"), "{xml}");
        assert!(xml.contains("<wp:wrapTight"), "{xml}");
        assert!(!xml.contains("wrapSquare"), "{xml}");
        assert!(xml.contains(r#"relativeHeight="251659264""#), "{xml}");
        // Порядок: положение — до размера, обтекание — после.
        let order = [
            "wp:positionH",
            "wp:positionV",
            "wp:extent",
            "wp:wrapTight",
            "wp:docPr",
        ];
        let places: Vec<usize> = order.iter().map(|n| xml.find(n).unwrap()).collect();
        assert!(places.windows(2).all(|w| w[0] < w[1]), "{xml}");
        // Ссылка на картинку меняется и у плавающего рисунка.
        let raw = with_relationship(picture.raw.as_ref().unwrap(), "rId1");
        assert!(raw.as_str().contains(r#"r:embed="rId1""#));
    }

    #[test]
    fn relationship_is_replaced() {
        let with_svg = DRAWING.replace(
            r#"<a:blip r:embed="rId9"/>"#,
            r#"<a:blip r:embed="rId9"><a:extLst><a:ext uri="svg"/></a:extLst></a:blip>"#,
        );
        let raw = with_relationship(&RawXml::new(with_svg), "rId2");
        let picture = read_picture(&parse(raw.as_str())).unwrap();
        assert_eq!(picture.relationship.as_deref(), Some("rId2"));
        assert!(!raw.as_str().contains("extLst"), "{raw}");
        assert!(raw.as_str().contains(r#"descr="Кот""#), "{raw}");
    }

    #[test]
    fn new_picture_gets_full_markup() {
        let picture = Picture {
            width: 12_700,
            height: 25_400,
            image: None,
            relationship: Some("rId3".into()),
            raw: None,
            anchor: None,
            shape: None,
        };
        let xml = picture_xml(&picture, 7);
        let wrapped = format!(
            r#"<w:r xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">{xml}</w:r>"#
        );
        let root = parse(&wrapped);
        let drawing = root.elements().next().unwrap();
        let again = read_picture(drawing).unwrap();
        assert_eq!((again.width, again.height), (12_700, 25_400));
        assert_eq!(again.relationship.as_deref(), Some("rId3"));
        assert!(xml.contains(r#"<wp:docPr id="7""#));
        // Без связи записывать нечего.
        let lost = Picture {
            relationship: None,
            ..picture
        };
        assert_eq!(picture_xml(&lost, 1), "");
    }

    fn square_anchor() -> Anchor {
        Anchor {
            horizontal: AnchorPosition {
                relative: HorizontalFrom::Column,
                placement: Placement::Offset(127_000),
            },
            vertical: AnchorPosition {
                relative: VerticalFrom::Paragraph,
                placement: Placement::Offset(25_400),
            },
            wrap: Wrap::Square,
            behind_text: false,
            distance: Distance {
                left: 114_300,
                right: 114_300,
                ..Distance::default()
            },
        }
    }

    #[test]
    fn inline_picture_becomes_floating_and_back() {
        let mut picture = read_picture(&parse(DRAWING)).unwrap();
        picture.anchor = Some(Box::new(square_anchor()));
        let xml = picture_xml(&picture, 1);
        let floating = read_picture(&parse(&xml)).unwrap();
        assert_eq!(floating.anchor, picture.anchor, "{xml}");
        assert!(
            xml.contains("<wp:anchor") && !xml.contains("wp:inline"),
            "{xml}"
        );
        assert!(xml.contains(r#"layoutInCell="1""#), "{xml}");
        assert!(xml.contains(r#"descr="Кот""#), "{xml}");
        let order = [
            "wp:simplePos",
            "wp:positionH",
            "wp:positionV",
            "wp:extent",
            "wp:wrapSquare",
            "wp:docPr",
        ];
        let places: Vec<usize> = order.iter().map(|n| xml.find(n).unwrap()).collect();
        assert!(places.windows(2).all(|w| w[0] < w[1]), "{xml}");

        // Обратно в строку: от плавающего ничего не остаётся.
        let mut back = Picture {
            raw: Some(RawXml::new(xml)),
            ..floating
        };
        back.anchor = None;
        let xml = picture_xml(&back, 1);
        let inline = read_picture(&parse(&xml)).unwrap();
        assert_eq!(inline.anchor, None);
        assert!(xml.contains("<wp:inline"), "{xml}");
        for gone in ["positionH", "wrapSquare", "simplePos", "layoutInCell"] {
            assert!(!xml.contains(gone), "{gone}: {xml}");
        }
        assert_eq!((inline.width, inline.height), (100, 200));
    }

    #[test]
    fn new_floating_picture_is_written() {
        let picture = Picture {
            width: 12_700,
            height: 25_400,
            image: None,
            relationship: Some("rId3".into()),
            raw: None,
            anchor: Some(Box::new(square_anchor())),
            shape: None,
        };
        let xml = picture_xml(&picture, 7);
        let wrapped = format!(
            r#"<w:r xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">{xml}</w:r>"#
        );
        let root = parse(&wrapped);
        let again = read_picture(root.elements().next().unwrap()).unwrap();
        assert_eq!(again.anchor, picture.anchor, "{xml}");
        assert_eq!(again.relationship.as_deref(), Some("rId3"));
    }
}
