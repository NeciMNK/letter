//! Разбор темы (`word/theme/theme1.xml`): шрифты и цвета.

use letter_model::{ColorScheme, Theme, ThemeFontSet};

use crate::xml::Element;

/// Читает шрифты из `a:theme/a:themeElements/a:fontScheme` и цвета
/// из `a:clrScheme`.
pub(crate) fn read_theme(root: &Element) -> Theme {
    let elements = || {
        root.elements()
            .filter(|e| e.local == "themeElements")
            .flat_map(Element::elements)
    };
    let scheme = elements().find(|e| e.local == "fontScheme");
    let colors = elements()
        .find(|e| e.local == "clrScheme")
        .map(read_color_scheme)
        .unwrap_or_default();
    let set = |local: &str| {
        scheme
            .and_then(|s| s.elements().find(|e| e.local == local))
            .map(read_font_set)
            .unwrap_or_default()
    };
    Theme {
        major: set("majorFont"),
        minor: set("minorFont"),
        colors,
    }
}

/// Цвета темы: `a:dk1` и т. д. с `a:srgbClr val` или `a:sysClr lastClr`
/// внутри. Цвета, которых нет в файле, берутся из темы Office.
fn read_color_scheme(scheme: &Element) -> ColorScheme {
    let mut colors = ColorScheme::default();
    for entry in scheme.elements() {
        let Some(color) = entry.elements().find_map(|c| {
            let name = match c.local.as_str() {
                "srgbClr" => "val",
                "sysClr" => "lastClr",
                _ => return None,
            };
            let value = c.attrs.iter().find(|a| a.local == name)?;
            crate::shape::parse_rgb(&value.value)
        }) else {
            continue;
        };
        match colors.colors.iter_mut().find(|(n, _)| *n == entry.local) {
            Some(slot) => slot.1 = color,
            None => colors.colors.push((entry.local.clone(), color)),
        }
    }
    colors
}

fn read_font_set(font: &Element) -> ThemeFontSet {
    let typeface = |local: &str| {
        font.elements()
            .find(|e| e.local == local)
            .and_then(|e| e.attrs.iter().find(|a| a.local == "typeface"))
            .map(|a| a.value.clone())
            .filter(|name| !name.is_empty())
    };
    ThemeFontSet {
        latin: typeface("latin"),
        east_asia: typeface("ea"),
        complex_script: typeface("cs"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml;

    #[test]
    fn reads_font_scheme() {
        let root = xml::parse(
            br#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Office"><a:themeElements><a:clrScheme name="x"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:accent1><a:srgbClr val="156082"/></a:accent1></a:clrScheme><a:fontScheme name="Office">
<a:majorFont><a:latin typeface="Cambria"/><a:ea typeface=""/><a:cs typeface=""/><a:font script="Cyrl" typeface="Times"/></a:majorFont>
<a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface="Arial"/></a:minorFont>
</a:fontScheme></a:themeElements></a:theme>"#,
        )
        .unwrap();
        let theme = read_theme(&root);
        assert_eq!(theme.major.latin.as_deref(), Some("Cambria"));
        assert_eq!(theme.major.east_asia, None);
        assert_eq!(theme.minor.complex_script.as_deref(), Some("Arial"));
        assert_eq!(theme.colors.color("accent1"), [0x15, 0x60, 0x82]);
        assert_eq!(theme.colors.color("tx1"), [0, 0, 0]);
        assert_eq!(theme.colors.color("accent2"), [0xED, 0x7D, 0x31]);
        assert_eq!(
            read_theme(&xml::parse(b"<a:theme xmlns:a='x'/>").unwrap()),
            Theme::default()
        );
    }
}
