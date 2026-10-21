//! Разбор темы (`word/theme/theme1.xml`): берём только шрифты.

use letter_model::{Theme, ThemeFontSet};

use crate::xml::Element;

/// Читает шрифты из `a:theme/a:themeElements/a:fontScheme`.
pub(crate) fn read_theme(root: &Element) -> Theme {
    let scheme = root
        .elements()
        .filter(|e| e.local == "themeElements")
        .flat_map(Element::elements)
        .find(|e| e.local == "fontScheme");
    let set = |local: &str| {
        scheme
            .and_then(|s| s.elements().find(|e| e.local == local))
            .map(read_font_set)
            .unwrap_or_default()
    };
    Theme {
        major: set("majorFont"),
        minor: set("minorFont"),
    }
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
            br#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Office"><a:themeElements><a:clrScheme name="x"/><a:fontScheme name="Office">
<a:majorFont><a:latin typeface="Cambria"/><a:ea typeface=""/><a:cs typeface=""/><a:font script="Cyrl" typeface="Times"/></a:majorFont>
<a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface="Arial"/></a:minorFont>
</a:fontScheme></a:themeElements></a:theme>"#,
        )
        .unwrap();
        let theme = read_theme(&root);
        assert_eq!(theme.major.latin.as_deref(), Some("Cambria"));
        assert_eq!(theme.major.east_asia, None);
        assert_eq!(theme.minor.complex_script.as_deref(), Some("Arial"));
        assert_eq!(
            read_theme(&xml::parse(b"<a:theme xmlns:a='x'/>").unwrap()),
            Theme::default()
        );
    }
}
