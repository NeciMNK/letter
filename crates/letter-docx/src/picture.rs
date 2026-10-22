//! Рисунки в строке: `w:drawing` с `wp:inline` внутри.
//!
//! Из рисунка программа понимает только размер и ссылку на файл
//! картинки (`a:blip r:embed`). Весь элемент хранится как есть
//! ([`Picture::raw`]), а при записи в нём меняется только размер.

use letter_model::{Picture, RawXml};

use crate::serialize::escape_attr;
use crate::xml::{self, Element, Node};

const WP_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const PIC_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// Самый большой размер рисунка, EMU (около 5,6 м — больше Word не
/// допускает). Защищает вёрстку от бессмысленных чисел.
const MAX_EMU: i64 = 20_116_800 * 10;

/// Читает `w:drawing`. `None`, если это не рисунок в строке (плавающий
/// объект, надпись, диаграмма): тогда элемент сохраняется без разбора.
pub(crate) fn read_picture(drawing: &Element) -> Option<Picture> {
    let mut children = drawing.elements();
    let inline = children.next()?;
    if inline.local != "inline" || children.next().is_some() {
        return None;
    }
    let extent = inline.elements().find(|e| e.local == "extent")?;
    let size = |name: &str| {
        let value: i64 = attr(extent, name)?.parse().ok()?;
        (0..=MAX_EMU).contains(&value).then_some(value)
    };
    let (width, height) = (size("cx")?, size("cy")?);
    let graphic = inline.elements().find(|e| e.local == "graphic")?;
    let data = graphic.elements().find(|e| e.local == "graphicData")?;
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
    })
}

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
        return resized(raw, picture.width, picture.height);
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
    format!(
        "<w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" \
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
    )
}

/// Исходный `w:drawing` с новым размером в `wp:extent` и `a:ext`.
/// Если размер не менялся (или XML не разбирается), — как есть, байт
/// в байт.
fn resized(raw: &RawXml, width: i64, height: i64) -> String {
    let Ok(mut root) = xml::parse(raw.as_str().as_bytes()) else {
        return raw.as_str().to_string();
    };
    let Some(Node::Element(inline)) = root
        .children
        .iter_mut()
        .find(|n| matches!(n, Node::Element(e) if e.local == "inline"))
    else {
        return raw.as_str().to_string();
    };
    let (cx, cy) = (width.to_string(), height.to_string());
    let mut changed = false;
    for node in &mut inline.children {
        let Node::Element(e) = node else { continue };
        if e.local == "extent" {
            changed |= set_size(e, &cx, &cy);
        }
    }
    // a:graphic/a:graphicData/pic:pic/pic:spPr/a:xfrm/a:ext
    if let Some(xfrm) = descend(inline, &["graphic", "graphicData", "pic", "spPr", "xfrm"]) {
        for node in &mut xfrm.children {
            let Node::Element(e) = node else { continue };
            if e.local == "ext" {
                changed |= set_size(e, &cx, &cy);
            }
        }
    }
    if changed {
        root.to_xml_fragment()
    } else {
        raw.as_str().to_string()
    }
}

/// Исходный `w:drawing` со ссылкой `a:blip r:embed` на связь `id`.
/// Дополнения картинки (`a:extLst`, например SVG рядом с PNG) убираются:
/// их связи остались в другом документе.
pub(crate) fn with_relationship(raw: &RawXml, id: &str) -> RawXml {
    let Ok(mut root) = xml::parse(raw.as_str().as_bytes()) else {
        return raw.clone();
    };
    let path = [
        "inline",
        "graphic",
        "graphicData",
        "pic",
        "blipFill",
        "blip",
    ];
    let Some(blip) = descend(&mut root, &path) else {
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
    fn floating_and_broken_drawings_stay_raw() {
        let anchor = DRAWING.replace("wp:inline", "wp:anchor");
        assert!(read_picture(&parse(&anchor)).is_none());
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
}
