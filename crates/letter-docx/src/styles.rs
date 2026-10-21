//! Разбор стилей (`word/styles.xml`).

use letter_model::{DocDefaults, Style, StyleKind, Styles};

use crate::document::{is_w_ns, read_paragraph_props, read_run_props};
use crate::xml::Element;

/// Превращает `w:styles` в модель. Всё непонятое сохраняется.
pub(crate) fn read_styles(root: &Element) -> Styles {
    let mut styles = Styles::default();
    if !root.is_w("styles") {
        return styles;
    }
    for child in root.elements() {
        if child.is_w("docDefaults") {
            match read_doc_defaults(child) {
                Some(defaults) => styles.defaults = defaults,
                None => styles.unknown.push(child.raw()),
            }
        } else if child.is_w("style") {
            match read_style(child) {
                Some(style) => styles.styles.push(style),
                None => styles.unknown.push(child.raw()),
            }
        } else {
            styles.unknown.push(child.raw());
        }
    }
    styles
}

/// `w:docDefaults` с `w:rPrDefault/w:rPr` и `w:pPrDefault/w:pPr`.
/// Если внутри есть что-то ещё, элемент сохраняется целиком.
fn read_doc_defaults(element: &Element) -> Option<DocDefaults> {
    if !element.attrs.is_empty() {
        return None;
    }
    let mut defaults = DocDefaults::default();
    for child in element.elements() {
        let inner = |name: &str| {
            let mut items = child.elements();
            match (items.next(), items.next()) {
                (Some(e), None) if e.is_w(name) && child.attrs.is_empty() => Some(e),
                (None, None) => None,
                _ => Some(child),
            }
        };
        if child.is_w("rPrDefault") {
            match inner("rPr") {
                Some(rpr) if rpr.is_w("rPr") => defaults.run = read_run_props(rpr),
                Some(_) => return None,
                None => {}
            }
        } else if child.is_w("pPrDefault") {
            match inner("pPr") {
                Some(ppr) if ppr.is_w("pPr") => {
                    defaults.paragraph = read_paragraph_props(ppr, false).0;
                }
                Some(_) => return None,
                None => {}
            }
        } else {
            return None;
        }
    }
    Some(defaults)
}

fn read_style(element: &Element) -> Option<Style> {
    if !element.only_attrs(&["type", "styleId", "default", "customStyle"]) {
        return None;
    }
    let kind = match element.w_attr("type").unwrap_or("paragraph") {
        "paragraph" => StyleKind::Paragraph,
        "character" => StyleKind::Character,
        "table" => StyleKind::Table,
        "numbering" => StyleKind::Numbering,
        _ => return None,
    };
    let flag = |name: &str| -> Option<bool> {
        match element.w_attr(name) {
            None | Some("false" | "0" | "off") => Some(false),
            Some("true" | "1" | "on") => Some(true),
            Some(_) => None,
        }
    };
    let mut style = Style::new(element.w_attr("styleId")?, kind);
    style.is_default = flag("default")?;
    style.custom = flag("customStyle")?;

    for child in element.elements() {
        let val = || {
            child
                .only_attrs(&["val"])
                .then(|| child.w_attr("val"))
                .flatten()
                .map(str::to_string)
        };
        let understood = is_w_ns(child.ns.as_deref())
            && match child.local.as_str() {
                "name" => val().map(|v| style.name = Some(v)).is_some(),
                "basedOn" => val().map(|v| style.based_on = Some(v)).is_some(),
                "next" => val().map(|v| style.next = Some(v)).is_some(),
                "pPr" => {
                    style.paragraph = read_paragraph_props(child, false).0;
                    true
                }
                "rPr" => {
                    style.run = read_run_props(child);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml;
    use letter_model::{HalfPoints, LineSpacing, ThemeFont, Twips};

    const STYLES: &str = r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:docDefaults>
  <w:rPrDefault><w:rPr><w:rFonts w:asciiTheme="minorHAnsi" w:hAnsiTheme="minorHAnsi"/><w:sz w:val="22"/></w:rPr></w:rPrDefault>
  <w:pPrDefault><w:pPr><w:spacing w:after="200" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault>
</w:docDefaults>
<w:latentStyles w:count="1"><w:lsdException w:name="Normal"/></w:latentStyles>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>
<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/>
  <w:pPr><w:keepNext/><w:spacing w:before="480" w:after="0"/></w:pPr>
  <w:rPr><w:b/><w:sz w:val="28"/></w:rPr></w:style>
<w:style w:type="character" w:customStyle="1" w:styleId="Mine"><w:name w:val="Mine"/></w:style>
<w:style w:type="weird" w:styleId="X"/>
</w:styles>"#;

    #[test]
    fn reads_defaults_and_styles() {
        let styles = read_styles(&xml::parse(STYLES.as_bytes()).unwrap());
        assert_eq!(styles.defaults.run.size, Some(HalfPoints(22)));
        assert_eq!(
            styles.defaults.run.fonts.ascii_theme,
            Some(ThemeFont::MinorHAnsi)
        );
        assert_eq!(styles.defaults.paragraph.spacing.after, Some(Twips(200)));
        assert_eq!(
            styles.defaults.paragraph.spacing.line,
            Some(LineSpacing::Multiple(276))
        );

        let ids: Vec<_> = styles.styles.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["Normal", "Heading1", "Mine"]);
        let normal = styles.get("Normal").unwrap();
        assert!(normal.is_default);
        assert_eq!(normal.unknown.len(), 1); // w:qFormat

        let heading = styles.get("Heading1").unwrap();
        assert_eq!(heading.name.as_deref(), Some("heading 1"));
        assert_eq!(heading.based_on.as_deref(), Some("Normal"));
        assert_eq!(heading.next.as_deref(), Some("Normal"));
        assert_eq!(heading.run.bold, Some(true));
        assert_eq!(heading.paragraph.spacing.before, Some(Twips(480)));
        assert_eq!(heading.paragraph.unknown.len(), 1); // w:keepNext

        let mine = styles.get("Mine").unwrap();
        assert_eq!(mine.kind, StyleKind::Character);
        assert!(mine.custom);

        // latentStyles и стиль неизвестного типа сохранены как есть.
        let unknown: Vec<_> = styles.unknown.iter().map(|r| r.local_name()).collect();
        assert_eq!(unknown, ["latentStyles", "style"]);
    }

    #[test]
    fn wrong_root_gives_empty_styles() {
        let styles = read_styles(&xml::parse(b"<x/>").unwrap());
        assert_eq!(styles, Styles::default());
    }
}
