//! Настройки документа (`word/settings.xml`). Пока в основном только
//! читаются: часть при сохранении возвращается в файл без изменений,
//! кроме флага «показывать цвет страницы».

use letter_model::Settings;

use crate::document::{W_NS, is_w_ns, parse_twips, toggle};
use crate::serialize::{element_with, xml_part};
use crate::xml::{self, Element};

/// Элементы `w:settings`, которые по схеме стоят перед
/// `w:displayBackgroundShape`.
const BEFORE_BACKGROUND: &[&str] = &[
    "writeProtection",
    "view",
    "zoom",
    "removePersonalInformation",
    "removeDateAndTime",
    "doNotDisplayPageBoundaries",
];

/// Часть настроек с флагом `w:displayBackgroundShape` (без него Word не
/// показывает цвет страницы). `original` — часть из исходного файла;
/// `None` в ответе — флаг там уже есть, менять нечего.
pub(crate) fn with_background_shape(original: Option<&[u8]>) -> Option<String> {
    let root = original
        .and_then(|bytes| xml::parse(bytes).ok())
        .filter(|root| root.is_w("settings"));
    let Some(root) = root else {
        let attrs = [("xmlns:w".to_string(), W_NS.to_string())];
        let flag = "<w:displayBackgroundShape/>";
        return Some(xml_part(&element_with("w:settings", &attrs, flag)));
    };
    if root.elements().any(|e| e.is_w("displayBackgroundShape")) {
        return None;
    }
    let prefix = root
        .qname
        .split_once(':')
        .map_or(String::new(), |(p, _)| format!("{p}:"));
    let flag = format!("<{prefix}displayBackgroundShape/>");
    let children: Vec<&Element> = root.elements().collect();
    let at = children
        .iter()
        .rposition(|e| is_w_ns(e.ns.as_deref()) && BEFORE_BACKGROUND.contains(&e.local.as_str()))
        .map_or(0, |i| i + 1);
    let mut content = String::new();
    for (i, child) in children.iter().enumerate() {
        if i == at {
            content.push_str(&flag);
        }
        content.push_str(&child.to_xml_fragment());
    }
    if at == children.len() {
        content.push_str(&flag);
    }
    Some(xml_part(&element_with(
        &root.qname,
        &root.root_attributes(),
        &content,
    )))
}

/// Читает нужные вёрстке настройки. Непонятое и испорченное заменяется
/// значениями по умолчанию.
pub(crate) fn read_settings(root: &Element) -> Settings {
    let mut settings = Settings::default();
    for child in root.elements().filter(|e| is_w_ns(e.ns.as_deref())) {
        match child.local.as_str() {
            "defaultTabStop" => {
                if let Some(t) = child
                    .w_attr("val")
                    .and_then(parse_twips)
                    .filter(|t| t.0 > 0)
                {
                    settings.default_tab_stop = t;
                }
            }
            "evenAndOddHeaders" => {
                settings.even_and_odd_headers = toggle(child).unwrap_or(false);
            }
            "compat" => {
                for flag in child
                    .elements()
                    .filter(|e| e.is_w("doNotUseHTMLParagraphAutoSpacing"))
                {
                    settings.add_paragraph_spacing = toggle(flag).unwrap_or(false);
                }
                let mode = child
                    .elements()
                    .filter(|e| {
                        e.is_w("compatSetting") && e.w_attr("name") == Some("compatibilityMode")
                    })
                    .find_map(|e| e.w_attr("val")?.trim().parse::<u32>().ok());
                if let Some(mode) = mode {
                    settings.compatibility_mode = mode;
                }
            }
            _ => {}
        }
    }
    settings
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::Twips;

    #[test]
    fn background_flag_goes_after_zoom() {
        let source = r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:zoom w:percent="100"/><w:defaultTabStop w:val="720"/></w:settings>"#;
        let xml = with_background_shape(Some(source.as_bytes())).unwrap_or_default();
        let zoom = xml.find("zoom").unwrap_or(usize::MAX);
        let flag = xml.find("displayBackgroundShape").unwrap_or(0);
        let tab = xml.find("defaultTabStop").unwrap_or(0);
        assert!(zoom < flag && flag < tab, "{xml}");
        let again = with_background_shape(Some(xml.as_bytes()));
        assert_eq!(again, None);
        // Без исходной части — новая часть только с флагом.
        let new = with_background_shape(None).unwrap_or_default();
        assert!(new.contains("<w:displayBackgroundShape/>"));
        assert_eq!(read(""), Settings::default());
    }

    fn read(body: &str) -> Settings {
        let source = format!(
            r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">{body}</w:settings>"#
        );
        read_settings(&xml::parse(source.as_bytes()).unwrap())
    }

    #[test]
    fn reads_tab_stop_and_spacing_mode() {
        let s = read(
            r#"<w:zoom w:percent="100"/><w:defaultTabStop w:val="720"/>
               <w:compat><w:doNotUseHTMLParagraphAutoSpacing/></w:compat>"#,
        );
        assert_eq!(s.default_tab_stop, Twips(720));
        assert!(s.add_paragraph_spacing);
        let s = read(
            r#"<w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat>"#,
        );
        assert_eq!(s.compatibility_mode, 15);
        assert!(read("<w:evenAndOddHeaders/>").even_and_odd_headers);
        assert_eq!(read(""), Settings::default());
        // Нулевой шаг табуляции и выключенный флаг — по умолчанию.
        let s = read(
            r#"<w:defaultTabStop w:val="0"/>
               <w:compat><w:doNotUseHTMLParagraphAutoSpacing w:val="0"/></w:compat>"#,
        );
        assert_eq!(s, Settings::default());
    }
}
