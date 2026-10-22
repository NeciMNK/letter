//! Настройки документа (`word/settings.xml`). Пока только читаются:
//! сама часть при сохранении возвращается в файл без изменений.

use letter_model::Settings;

use crate::document::{is_w_ns, parse_twips, toggle};
use crate::xml::Element;

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
    use crate::xml;
    use letter_model::Twips;

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
