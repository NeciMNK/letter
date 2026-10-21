//! Свойства документа: `docProps/core.xml` (название, автор, даты)
//! и `docProps/app.xml` (программа, статистика).

use letter_model::{AppProperties, CoreProperties, RawXml};

use crate::serialize::{escape_text, xml_part};
use crate::xml::Element;

const NS_CP: &str = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
const NS_DC: &str = "http://purl.org/dc/elements/1.1/";
const NS_DCTERMS: &str = "http://purl.org/dc/terms/";
const NS_XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
const NS_EP: &str = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";
const NS_EP_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/extendedProperties";
const NS_VT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes";

/// Поля основных свойств: пространство имён, имя элемента, имя для записи.
type CoreField = (&'static str, &'static str, &'static str);

/// Порядок записи основных свойств (схема разрешает любой).
const CORE_FIELDS: [CoreField; 13] = [
    (NS_DC, "title", "dc:title"),
    (NS_DC, "subject", "dc:subject"),
    (NS_DC, "creator", "dc:creator"),
    (NS_CP, "keywords", "cp:keywords"),
    (NS_DC, "description", "dc:description"),
    (NS_CP, "lastModifiedBy", "cp:lastModifiedBy"),
    (NS_CP, "revision", "cp:revision"),
    (NS_CP, "lastPrinted", "cp:lastPrinted"),
    (NS_DCTERMS, "created", "dcterms:created"),
    (NS_DCTERMS, "modified", "dcterms:modified"),
    (NS_CP, "category", "cp:category"),
    (NS_CP, "contentStatus", "cp:contentStatus"),
    (NS_DC, "language", "dc:language"),
];

fn core_slot<'a>(props: &'a mut CoreProperties, local: &str) -> Option<&'a mut Option<String>> {
    Some(match local {
        "title" => &mut props.title,
        "subject" => &mut props.subject,
        "creator" => &mut props.creator,
        "keywords" => &mut props.keywords,
        "description" => &mut props.description,
        "lastModifiedBy" => &mut props.last_modified_by,
        "revision" => &mut props.revision,
        "lastPrinted" => &mut props.last_printed,
        "created" => &mut props.created,
        "modified" => &mut props.modified,
        "category" => &mut props.category,
        "contentStatus" => &mut props.content_status,
        "language" => &mut props.language,
        _ => return None,
    })
}

/// Даты `created`/`modified` пишутся с `xsi:type="dcterms:W3CDTF"`.
fn is_dated(local: &str) -> bool {
    matches!(local, "created" | "modified")
}

/// Разбирает `cp:coreProperties`. `None`, если корень не тот.
pub(crate) fn read_core(root: &Element) -> Option<CoreProperties> {
    if root.local != "coreProperties" || root.ns.as_deref() != Some(NS_CP) {
        return None;
    }
    let mut props = CoreProperties::default();
    for child in root.elements() {
        let known = CORE_FIELDS
            .iter()
            .any(|(ns, local, _)| child.ns.as_deref() == Some(*ns) && child.local == *local);
        let attrs_ok = child.attrs.iter().all(|a| {
            is_dated(&child.local)
                && a.ns.as_deref() == Some(NS_XSI)
                && a.local == "type"
                && a.value.rsplit(':').next() == Some("W3CDTF")
        });
        let understood = known
            && attrs_ok
            && child.elements().next().is_none()
            && match core_slot(&mut props, &child.local) {
                Some(slot) if slot.is_none() => {
                    *slot = Some(child.text());
                    true
                }
                _ => false,
            };
        if !understood {
            props.unknown.push(RawXml::new(child.to_xml_fragment()));
        }
    }
    Some(props)
}

/// Записывает `docProps/core.xml`.
pub(crate) fn core_xml(props: &CoreProperties) -> String {
    let mut out = format!(
        "<cp:coreProperties xmlns:cp=\"{NS_CP}\" xmlns:dc=\"{NS_DC}\" \
         xmlns:dcterms=\"{NS_DCTERMS}\" xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" \
         xmlns:xsi=\"{NS_XSI}\">"
    );
    let mut props = props.clone();
    for (_, local, name) in CORE_FIELDS {
        let Some(Some(value)) = core_slot(&mut props, local).map(Option::take) else {
            continue;
        };
        let attrs = if is_dated(local) {
            " xsi:type=\"dcterms:W3CDTF\""
        } else {
            ""
        };
        out.push_str(&element(name, attrs, &value));
    }
    for raw in &props.unknown {
        out.push_str(raw.as_str());
    }
    out.push_str("</cp:coreProperties>");
    xml_part(&out)
}

/// Порядок записи сведений о программе (схема разрешает любой).
const APP_TEXT_FIELDS: [&str; 5] = [
    "Template",
    "Application",
    "AppVersion",
    "Company",
    "Manager",
];
const APP_NUMBER_FIELDS: [&str; 7] = [
    "TotalTime",
    "Pages",
    "Words",
    "Characters",
    "CharactersWithSpaces",
    "Lines",
    "Paragraphs",
];

fn app_text_slot<'a>(props: &'a mut AppProperties, local: &str) -> Option<&'a mut Option<String>> {
    Some(match local {
        "Template" => &mut props.template,
        "Application" => &mut props.application,
        "AppVersion" => &mut props.app_version,
        "Company" => &mut props.company,
        "Manager" => &mut props.manager,
        _ => return None,
    })
}

fn app_number_slot<'a>(props: &'a mut AppProperties, local: &str) -> Option<&'a mut Option<u32>> {
    Some(match local {
        "TotalTime" => &mut props.total_time,
        "Pages" => &mut props.pages,
        "Words" => &mut props.words,
        "Characters" => &mut props.characters,
        "CharactersWithSpaces" => &mut props.characters_with_spaces,
        "Lines" => &mut props.lines,
        "Paragraphs" => &mut props.paragraphs,
        _ => return None,
    })
}

/// Разбирает `Properties` из `docProps/app.xml`. `None`, если корень не тот.
pub(crate) fn read_app(root: &Element) -> Option<AppProperties> {
    let is_ep = |e: &Element| matches!(e.ns.as_deref(), Some(NS_EP | NS_EP_STRICT));
    if root.local != "Properties" || !is_ep(root) {
        return None;
    }
    let mut props = AppProperties::default();
    for child in root.elements() {
        let simple = is_ep(child) && child.attrs.is_empty() && child.elements().next().is_none();
        let text = child.text();
        let understood = simple
            && if let Some(slot) = app_text_slot(&mut props, &child.local) {
                slot.is_none() && slot.replace(text).is_none()
            } else if let Some(slot) = app_number_slot(&mut props, &child.local) {
                match text.trim().parse::<u32>() {
                    Ok(n) if slot.is_none() => slot.replace(n).is_none(),
                    _ => false,
                }
            } else {
                false
            };
        if !understood {
            props.unknown.push(RawXml::new(child.to_xml_fragment()));
        }
    }
    Some(props)
}

/// Записывает `docProps/app.xml`.
pub(crate) fn app_xml(props: &AppProperties) -> String {
    let mut out = format!("<Properties xmlns=\"{NS_EP}\" xmlns:vt=\"{NS_VT}\">");
    let mut props = props.clone();
    for name in APP_TEXT_FIELDS {
        if let Some(Some(value)) = app_text_slot(&mut props, name).map(Option::take) {
            out.push_str(&element(name, "", &value));
        }
    }
    for name in APP_NUMBER_FIELDS {
        if let Some(Some(value)) = app_number_slot(&mut props, name).map(Option::take) {
            out.push_str(&element(name, "", &value.to_string()));
        }
    }
    for raw in &props.unknown {
        out.push_str(raw.as_str());
    }
    out.push_str("</Properties>");
    xml_part(&out)
}

fn element(name: &str, attrs: &str, text: &str) -> String {
    if text.is_empty() {
        format!("<{name}{attrs}/>")
    } else {
        format!("<{name}{attrs}>{}</{name}>", escape_text(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml;

    #[test]
    fn core_round_trip() {
        let source = r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
<dc:title>Отчёт &amp; план</dc:title><dc:subject/><dc:creator>Никита</dc:creator>
<cp:revision>3</cp:revision>
<dcterms:created xsi:type="dcterms:W3CDTF">2024-05-01T10:00:00Z</dcterms:created>
<dcterms:modified>2024-05-02T10:00:00Z</dcterms:modified>
<dc:title>второй заголовок</dc:title>
<dc:identifier>x</dc:identifier>
</cp:coreProperties>"#;
        let props = read_core(&xml::parse(source.as_bytes()).unwrap()).unwrap();
        assert_eq!(props.title.as_deref(), Some("Отчёт & план"));
        assert_eq!(props.subject.as_deref(), Some(""));
        assert_eq!(props.creator.as_deref(), Some("Никита"));
        assert_eq!(props.revision.as_deref(), Some("3"));
        assert_eq!(props.created.as_deref(), Some("2024-05-01T10:00:00Z"));
        assert_eq!(props.modified.as_deref(), Some("2024-05-02T10:00:00Z"));
        // Повтор и неизвестный элемент сохранены как есть.
        let unknown: Vec<_> = props.unknown.iter().map(RawXml::local_name).collect();
        assert_eq!(unknown, ["title", "identifier"]);

        let written = core_xml(&props);
        assert!(written.contains(r#"<dcterms:modified xsi:type="dcterms:W3CDTF">"#));
        let again = read_core(&xml::parse(written.as_bytes()).unwrap()).unwrap();
        assert_eq!(again, props);
    }

    #[test]
    fn app_round_trip() {
        let source = r#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes">
<Template>Normal.dotm</Template><TotalTime>5</TotalTime><Words>12</Words><Pages>x</Pages>
<Application>Microsoft Office Word</Application><Company/>
<HeadingPairs><vt:vector size="1" baseType="variant"><vt:variant><vt:lpstr>Title</vt:lpstr></vt:variant></vt:vector></HeadingPairs>
</Properties>"#;
        let props = read_app(&xml::parse(source.as_bytes()).unwrap()).unwrap();
        assert_eq!(props.template.as_deref(), Some("Normal.dotm"));
        assert_eq!(props.total_time, Some(5));
        assert_eq!(props.words, Some(12));
        assert_eq!(props.pages, None);
        assert_eq!(props.company.as_deref(), Some(""));
        let unknown: Vec<_> = props.unknown.iter().map(RawXml::local_name).collect();
        assert_eq!(unknown, ["Pages", "HeadingPairs"]);

        let written = app_xml(&props);
        let again = read_app(&xml::parse(written.as_bytes()).unwrap()).unwrap();
        assert_eq!(again, props);
    }

    #[test]
    fn wrong_roots() {
        assert!(read_core(&xml::parse(b"<x/>").unwrap()).is_none());
        assert!(read_app(&xml::parse(b"<Properties/>").unwrap()).is_none());
    }
}
