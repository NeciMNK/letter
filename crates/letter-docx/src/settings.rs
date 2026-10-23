//! Настройки документа (`word/settings.xml`). Пока в основном только
//! читаются: часть при сохранении возвращается в файл без изменений,
//! кроме флага «показывать цвет страницы» и расстановки переносов.

use letter_model::{DocumentProtection, EditRestriction, PasswordHash, Settings, WriteProtection};

use crate::document::{W_NS, is_w_ns, parse_twips, toggle};
use crate::serialize::{element_with, escape_attr, xml_part};
use crate::xml::{self, Element};

/// Порядок дочерних элементов `w:settings` по схеме (CT_Settings) до
/// последнего элемента, который мы пишем.
const SETTINGS_ORDER: &[&str] = &[
    "writeProtection",
    "view",
    "zoom",
    "removePersonalInformation",
    "removeDateAndTime",
    "doNotDisplayPageBoundaries",
    "displayBackgroundShape",
    "printPostScriptOverText",
    "printFractionalCharacterWidth",
    "printFormsData",
    "embedTrueTypeFonts",
    "embedSystemFonts",
    "saveSubsetFonts",
    "saveFormsData",
    "mirrorMargins",
    "alignBordersAndEdges",
    "bordersDoNotSurroundHeader",
    "bordersDoNotSurroundFooter",
    "gutterAtTop",
    "hideSpellingErrors",
    "hideGrammaticalErrors",
    "activeWritingStyle",
    "proofState",
    "formsDesign",
    "attachedTemplate",
    "linkStyles",
    "stylePaneFormatFilter",
    "stylePaneSortMethod",
    "documentType",
    "mailMerge",
    "revisionView",
    "trackRevisions",
    "doNotTrackMoves",
    "doNotTrackFormatting",
    "documentProtection",
    "autoFormatOverride",
    "styleLockTheme",
    "styleLockQFSet",
    "defaultTabStop",
    "autoHyphenation",
    "consecutiveHyphenLimit",
    "hyphenationZone",
    "doNotHyphenateCaps",
];

/// Новая часть настроек, если в исходной (`original`) нужно что-то
/// поменять: флаг `w:displayBackgroundShape` (если у документа есть цвет
/// страницы, `background`) и настройки расстановки переносов из
/// `settings`. `None` — менять нечего. Остальные элементы части
/// остаются как были.
pub(crate) fn updated_settings(
    original: Option<&[u8]>,
    settings: &Settings,
    background: bool,
) -> Option<String> {
    let root = original
        .and_then(|bytes| xml::parse(bytes).ok())
        .filter(|root| root.is_w("settings"));
    let current = root.as_ref().map(read_settings).unwrap_or_default();
    let has_flag = root
        .as_ref()
        .is_some_and(|r| r.elements().any(|e| e.is_w("displayBackgroundShape")));
    let same = |a: &Settings, b: &Settings| {
        a.auto_hyphenation == b.auto_hyphenation
            && a.hyphenation_zone == b.hyphenation_zone
            && a.consecutive_hyphen_limit == b.consecutive_hyphen_limit
            && a.do_not_hyphenate_caps == b.do_not_hyphenate_caps
            && a.protection == b.protection
            && a.write_protection == b.write_protection
    };
    if same(&current, settings) && (has_flag || !background) {
        return None;
    }
    let prefix = root
        .as_ref()
        .and_then(|r| r.qname.split_once(':'))
        .map_or("w:".to_string(), |(p, _)| format!("{p}:"));
    let w = |name: &str, val: Option<String>| match val {
        Some(v) => format!(r#"<{prefix}{name} {prefix}val="{v}"/>"#),
        None => format!("<{prefix}{name}/>"),
    };
    // Наши элементы: имя и новое содержимое (`None` — убрать).
    let ours: [(&str, Option<String>); 7] = [
        (
            "writeProtection",
            settings
                .write_protection
                .as_ref()
                .map(|p| write_protection_xml(&prefix, p)),
        ),
        (
            "documentProtection",
            settings
                .protection
                .as_ref()
                .map(|p| protection_xml(&prefix, p)),
        ),
        (
            "displayBackgroundShape",
            (background || has_flag).then(|| w("displayBackgroundShape", None)),
        ),
        (
            "autoHyphenation",
            settings
                .auto_hyphenation
                .then(|| w("autoHyphenation", None)),
        ),
        (
            "consecutiveHyphenLimit",
            (settings.consecutive_hyphen_limit > 0).then(|| {
                w(
                    "consecutiveHyphenLimit",
                    Some(settings.consecutive_hyphen_limit.to_string()),
                )
            }),
        ),
        (
            "hyphenationZone",
            (settings.hyphenation_zone != Settings::default().hyphenation_zone).then(|| {
                w(
                    "hyphenationZone",
                    Some(settings.hyphenation_zone.0.to_string()),
                )
            }),
        ),
        (
            "doNotHyphenateCaps",
            settings
                .do_not_hyphenate_caps
                .then(|| w("doNotHyphenateCaps", None)),
        ),
    ];
    let rank = |name: &str| SETTINGS_ORDER.iter().position(|n| *n == name);
    // Чужие элементы — как есть, со своим местом по схеме (непонятые —
    // после наших).
    let mut items: Vec<(usize, String)> = Vec::new();
    if let Some(root) = &root {
        let mut last = 0;
        for child in root.elements() {
            let known = is_w_ns(child.ns.as_deref());
            if known && ours.iter().any(|(n, _)| *n == child.local) {
                continue;
            }
            if known && let Some(r) = rank(&child.local) {
                last = r;
            } else if known {
                last = SETTINGS_ORDER.len();
            }
            items.push((last, child.to_xml_fragment()));
        }
    }
    for (name, xml) in ours {
        if let (Some(xml), Some(r)) = (xml, rank(name)) {
            let at = items
                .iter()
                .position(|(k, _)| *k > r)
                .unwrap_or(items.len());
            items.insert(at, (r, xml));
        }
    }
    let content: String = items.into_iter().map(|(_, x)| x).collect();
    let attrs = match &root {
        Some(root) => root.root_attributes(),
        None => vec![("xmlns:w".to_string(), W_NS.to_string())],
    };
    let qname = root.as_ref().map_or("w:settings", |r| r.qname.as_str());
    Some(xml_part(&element_with(qname, &attrs, &content)))
}

/// Атрибуты хеша пароля и прочие сохранённые атрибуты.
fn password_attrs(
    prefix: &str,
    password: Option<&PasswordHash>,
    other: &[(String, String)],
) -> String {
    let mut out = String::new();
    if let Some(p) = password {
        out.push_str(&format!(
            r#" {prefix}algorithmName="{}" {prefix}hashValue="{}" {prefix}saltValue="{}" {prefix}spinCount="{}""#,
            escape_attr(&p.algorithm),
            escape_attr(&p.hash),
            escape_attr(&p.salt),
            p.spin_count
        ));
    }
    for (name, value) in other {
        out.push_str(&format!(r#" {prefix}{name}="{}""#, escape_attr(value)));
    }
    out
}

fn protection_xml(prefix: &str, p: &DocumentProtection) -> String {
    format!(
        r#"<{prefix}documentProtection {prefix}edit="{}" {prefix}enforcement="{}"{}/>"#,
        p.edit.as_str(),
        if p.enforced { "1" } else { "0" },
        password_attrs(prefix, p.password.as_ref(), &p.other)
    )
}

fn write_protection_xml(prefix: &str, p: &WriteProtection) -> String {
    let recommended = if p.recommended {
        format!(r#" {prefix}recommended="1""#)
    } else {
        String::new()
    };
    format!(
        r#"<{prefix}writeProtection{recommended}{}/>"#,
        password_attrs(prefix, p.password.as_ref(), &p.other)
    )
}

/// Атрибуты, которые разбираются сами (остальные идут в `other`).
const PASSWORD_ATTRS: [&str; 4] = ["algorithmName", "hashValue", "saltValue", "spinCount"];

/// Читает хеш пароля нового вида и прочие атрибуты пространства `w`,
/// кроме `known`.
fn read_password(e: &Element, known: &[&str]) -> (Option<PasswordHash>, Vec<(String, String)>) {
    let password = match (
        e.w_attr("algorithmName"),
        e.w_attr("hashValue"),
        e.w_attr("saltValue"),
    ) {
        (Some(algorithm), Some(hash), Some(salt)) => Some(PasswordHash {
            algorithm: algorithm.trim().to_string(),
            hash: hash.trim().to_string(),
            salt: salt.trim().to_string(),
            spin_count: e
                .w_attr("spinCount")
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0),
        }),
        _ => None,
    };
    let other = e
        .attrs
        .iter()
        .filter(|a| a.ns.is_some() && is_w_ns(a.ns.as_deref()))
        .filter(|a| !known.contains(&a.local.as_str()))
        .filter(|a| password.is_none() || !PASSWORD_ATTRS.contains(&a.local.as_str()))
        .map(|a| (a.local.clone(), a.value.clone()))
        .collect();
    (password, other)
}

fn on_off(value: Option<&str>) -> bool {
    matches!(value.map(str::trim), Some("1" | "true" | "on"))
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
            "documentProtection" => {
                let (password, other) = read_password(child, &["edit", "enforcement"]);
                settings.protection = Some(DocumentProtection {
                    edit: child
                        .w_attr("edit")
                        .and_then(EditRestriction::parse)
                        .unwrap_or_default(),
                    enforced: on_off(child.w_attr("enforcement")),
                    password,
                    other,
                });
            }
            "writeProtection" => {
                let (password, other) = read_password(child, &["recommended"]);
                settings.write_protection = Some(WriteProtection {
                    recommended: on_off(child.w_attr("recommended")),
                    password,
                    other,
                });
            }
            "evenAndOddHeaders" => {
                settings.even_and_odd_headers = toggle(child).unwrap_or(false);
            }
            "autoHyphenation" => settings.auto_hyphenation = toggle(child).unwrap_or(false),
            "doNotHyphenateCaps" => {
                settings.do_not_hyphenate_caps = toggle(child).unwrap_or(false);
            }
            "hyphenationZone" => {
                if let Some(t) = child
                    .w_attr("val")
                    .and_then(parse_twips)
                    .filter(|t| t.0 > 0)
                {
                    settings.hyphenation_zone = t;
                }
            }
            "consecutiveHyphenLimit" => {
                if let Some(n) = child.w_attr("val").and_then(|v| v.trim().parse().ok()) {
                    settings.consecutive_hyphen_limit = n;
                }
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
        let plain = Settings::default();
        let xml = updated_settings(Some(source.as_bytes()), &plain, true).unwrap_or_default();
        let zoom = xml.find("zoom").unwrap_or(usize::MAX);
        let flag = xml.find("displayBackgroundShape").unwrap_or(0);
        let tab = xml.find("defaultTabStop").unwrap_or(0);
        assert!(zoom < flag && flag < tab, "{xml}");
        let again = updated_settings(Some(xml.as_bytes()), &plain, true);
        assert_eq!(again, None);
        // Без исходной части — новая часть только с флагом.
        assert_eq!(
            updated_settings(Some(source.as_bytes()), &plain, false),
            None
        );
        let new = updated_settings(None, &plain, true).unwrap_or_default();
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

    #[test]
    fn hyphenation_is_read_and_written() {
        let s = read(
            r#"<w:autoHyphenation/><w:consecutiveHyphenLimit w:val="2"/>
               <w:hyphenationZone w:val="425"/><w:doNotHyphenateCaps/>"#,
        );
        assert!(s.auto_hyphenation && s.do_not_hyphenate_caps);
        assert_eq!(s.consecutive_hyphen_limit, 2);
        assert_eq!(s.hyphenation_zone, Twips(425));

        let source = r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:zoom w:percent="100"/><w:defaultTabStop w:val="720"/><w:characterSpacingControl w:val="doNotCompress"/><w:compat/></w:settings>"#;
        let on = Settings {
            auto_hyphenation: true,
            hyphenation_zone: Twips(425),
            ..Settings::default()
        };
        let xml = updated_settings(Some(source.as_bytes()), &on, false).unwrap_or_default();
        let at = |name: &str| xml.find(name).unwrap_or(usize::MAX);
        assert!(at("defaultTabStop") < at("autoHyphenation"), "{xml}");
        assert!(at("autoHyphenation") < at("hyphenationZone"), "{xml}");
        assert!(
            at("hyphenationZone") < at("characterSpacingControl"),
            "{xml}"
        );
        assert!(at("characterSpacingControl") < at("compat"), "{xml}");
        let reread = read_settings(&xml::parse(xml.as_bytes()).unwrap());
        assert_eq!(reread.default_tab_stop, Twips(720));
        assert!(reread.auto_hyphenation);
        assert_eq!(reread.hyphenation_zone, Twips(425));
        // Сохранённое ещё раз — без изменений; выключение убирает элементы.
        assert_eq!(updated_settings(Some(xml.as_bytes()), &on, false), None);
        let off =
            updated_settings(Some(xml.as_bytes()), &Settings::default(), false).unwrap_or_default();
        assert!(!off.contains("autoHyphenation") && !off.contains("hyphenationZone"));
        assert!(off.contains("characterSpacingControl"));
    }

    #[test]
    fn protection_is_read_and_written() {
        let s = read(
            r#"<w:writeProtection w:recommended="1"/>
               <w:documentProtection w:edit="readOnly" w:enforcement="1"
                 w:cryptProviderType="rsaAES" w:hash="abc=" w:salt="xyz="/>"#,
        );
        let protection = s.protection.clone().unwrap_or_default();
        assert_eq!(protection.edit, EditRestriction::ReadOnly);
        assert!(protection.enforced && protection.password.is_none());
        assert_eq!(protection.other.len(), 3);
        assert!(s.write_protection.as_ref().is_some_and(|w| w.recommended));

        let with_password = Settings {
            protection: Some(DocumentProtection {
                edit: EditRestriction::Comments,
                enforced: true,
                password: Some(PasswordHash {
                    algorithm: "SHA-512".into(),
                    hash: "aGFzaA==".into(),
                    salt: "c2FsdA==".into(),
                    spin_count: 100000,
                }),
                other: Vec::new(),
            }),
            ..s.clone()
        };
        let source = r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:zoom w:percent="100"/><w:defaultTabStop w:val="720"/></w:settings>"#;
        for settings in [&s, &with_password] {
            let xml =
                updated_settings(Some(source.as_bytes()), settings, false).unwrap_or_default();
            let at = |name: &str| xml.find(name).unwrap_or(usize::MAX);
            assert!(at("writeProtection") < at("zoom"), "{xml}");
            assert!(at("zoom") < at("documentProtection"), "{xml}");
            assert!(at("documentProtection") < at("defaultTabStop"), "{xml}");
            let reread = read_settings(&xml::parse(xml.as_bytes()).unwrap());
            assert_eq!(reread.protection, settings.protection);
            assert_eq!(reread.write_protection, settings.write_protection);
            assert_eq!(
                updated_settings(Some(xml.as_bytes()), settings, false),
                None
            );
        }
        // Снятая защита убирает элемент.
        let xml = updated_settings(Some(source.as_bytes()), &s, false).unwrap_or_default();
        let off =
            updated_settings(Some(xml.as_bytes()), &Settings::default(), false).unwrap_or_default();
        assert!(!off.contains("Protection"), "{off}");
    }
}
