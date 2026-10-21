//! Сборка пакета .docx при сохранении.
//!
//! Главная часть и стили строятся заново из модели. Все остальные части
//! исходного файла ([`SourcePackage`]) записываются байт в байт, вместе
//! со своими связями, поэтому картинки, колонтитулы, сноски и прочее,
//! что программа пока не понимает, не теряются.

use std::borrow::Cow;
use std::io::{Seek, Write};

use letter_model::{Document, SourcePackage, Styles};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::WriteError;
use crate::package::{CONTENT_TYPES_PART, attr, normalize_part_name, rels_part_for};
use crate::serialize::{self, escape_attr};
use crate::xml;

const CT_MAIN: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const CT_STYLES: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
const CT_RELS: &str = "application/vnd.openxmlformats-package.relationships+xml";
const CT_XML: &str = "application/xml";
const REL_OFFICE_DOCUMENT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const REL_STYLES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
const NS_RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const NS_CONTENT_TYPES: &str = "http://schemas.openxmlformats.org/package/2006/content-types";

/// Записывает документ в формате .docx.
pub(crate) fn write<W: Write + Seek>(doc: &Document, writer: W) -> Result<(), WriteError> {
    let empty = SourcePackage::default();
    let source = doc.source.as_ref().unwrap_or(&empty);
    let main = if source.main_part.is_empty() {
        "word/document.xml".to_string()
    } else {
        source.main_part.clone()
    };

    // Части в порядке записи: имя и содержимое.
    let mut parts: Vec<(String, Cow<'_, [u8]>)> = Vec::new();
    let mut overrides = source.content_types.clone();
    let mut defaults = source.default_content_types.clone();
    for (ext, content_type) in [("rels", CT_RELS), ("xml", CT_XML)] {
        if !defaults.iter().any(|(e, _)| e.eq_ignore_ascii_case(ext)) {
            defaults.push((ext.to_string(), content_type.to_string()));
        }
    }

    parts.push((
        main.clone(),
        serialize::document_xml(doc).into_bytes().into(),
    ));
    ensure_override(&mut overrides, &main, CT_MAIN);

    // Связи главного документа могут поменяться, если добавляются стили.
    let main_rels = rels_part_for(&main);
    let mut replaced = vec![main.clone()];
    let styles_part = match &source.styles_part {
        Some(name) => Some(name.clone()),
        None if doc.styles != Styles::default() => {
            let (rels, name) = add_styles_relationship(source, &main, &main_rels);
            parts.push((main_rels.clone(), rels.into_bytes().into()));
            replaced.push(main_rels.clone());
            Some(name)
        }
        None => None,
    };
    if let Some(name) = &styles_part {
        let root = (!source.styles_root_attributes.is_empty())
            .then_some(source.styles_root_attributes.as_slice());
        let xml = serialize::styles_xml(&doc.styles, root);
        parts.push((name.clone(), xml.into_bytes().into()));
        ensure_override(&mut overrides, name, CT_STYLES);
        replaced.push(name.clone());
    }

    if source.part("_rels/.rels").is_none() {
        let rels = relationships_xml(&[Relationship {
            id: "rId1".into(),
            kind: REL_OFFICE_DOCUMENT.into(),
            target: main.clone(),
            external: false,
        }]);
        parts.push(("_rels/.rels".into(), rels.into_bytes().into()));
    }
    for part in &source.parts {
        if !replaced.iter().any(|r| r.eq_ignore_ascii_case(&part.name)) {
            parts.push((part.name.clone(), Cow::Borrowed(part.data.as_slice())));
        }
    }

    let mut zip = ZipWriter::new(writer);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file(CONTENT_TYPES_PART, options)?;
    zip.write_all(content_types_xml(&defaults, &overrides).as_bytes())?;
    for (name, data) in &parts {
        zip.start_file(name.as_str(), options)?;
        zip.write_all(data)?;
    }
    zip.finish()?;
    Ok(())
}

/// Добавляет `Override`, если для части ещё нет своего типа содержимого.
fn ensure_override(overrides: &mut Vec<(String, String)>, part: &str, content_type: &str) {
    if !overrides.iter().any(|(p, _)| p.eq_ignore_ascii_case(part)) {
        overrides.push((part.to_string(), content_type.to_string()));
    }
}

/// Связь из файла `.rels`.
struct Relationship {
    id: String,
    kind: String,
    /// Для внутренних связей — имя части-цели внутри архива, для внешних —
    /// адрес как есть.
    target: String,
    external: bool,
}

/// Связи главного документа с добавленной связью на стили. Возвращает
/// новый файл связей и имя части стилей.
fn add_styles_relationship(
    source: &SourcePackage,
    main: &str,
    main_rels: &str,
) -> (String, String) {
    let base_dir = main.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut rels: Vec<Relationship> = source
        .part(main_rels)
        .and_then(|p| xml::parse(&p.data).ok())
        .map(|root| {
            root.elements()
                .filter(|e| e.local == "Relationship")
                .filter_map(|e| {
                    let external = attr(e, "TargetMode") == Some("External");
                    let target = attr(e, "Target")?;
                    Some(Relationship {
                        id: attr(e, "Id")?.to_string(),
                        kind: attr(e, "Type")?.to_string(),
                        target: if external {
                            target.to_string()
                        } else {
                            resolve_target(base_dir, target)
                        },
                        external,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    // Связь на стили может уже быть, если сама часть стилей испорчена:
    // тогда пишем стили на её место.
    if let Some(existing) = rels
        .iter()
        .find(|r| !r.external && r.kind.ends_with("/styles"))
    {
        let name = existing.target.clone();
        return (relationships_xml(&rels), name);
    }
    let mut name = join(base_dir, "styles.xml");
    let mut n = 1;
    while source.part(&name).is_some() || name.eq_ignore_ascii_case(main) {
        n += 1;
        name = join(base_dir, &format!("styles{n}.xml"));
    }
    let mut id_number = rels.len() + 1;
    let id = loop {
        let id = format!("rId{id_number}");
        if !rels.iter().any(|r| r.id == id) {
            break id;
        }
        id_number += 1;
    };
    rels.push(Relationship {
        id,
        kind: REL_STYLES.into(),
        target: name.clone(),
        external: false,
    });
    (relationships_xml(&rels), name)
}

fn resolve_target(base_dir: &str, target: &str) -> String {
    if target.starts_with('/') {
        normalize_part_name(target)
    } else {
        normalize_part_name(&join(base_dir, target))
    }
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// Файл связей. Цели внутренних связей пишутся абсолютными (`/word/...`),
/// так их понимают все программы и не нужно считать относительный путь.
fn relationships_xml(rels: &[Relationship]) -> String {
    let mut out = format!("{XML_DECLARATION}<Relationships xmlns=\"{NS_RELS}\">");
    for rel in rels {
        let target = if rel.external {
            rel.target.clone()
        } else {
            format!("/{}", encode_part_name(&rel.target))
        };
        out.push_str(&format!(
            "<Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"{}/>",
            escape_attr(&rel.id),
            escape_attr(&rel.kind),
            escape_attr(&target),
            if rel.external {
                " TargetMode=\"External\""
            } else {
                ""
            }
        ));
    }
    out.push_str("</Relationships>");
    out
}

fn content_types_xml(defaults: &[(String, String)], overrides: &[(String, String)]) -> String {
    let mut out = format!("{XML_DECLARATION}<Types xmlns=\"{NS_CONTENT_TYPES}\">");
    for (ext, content_type) in defaults {
        out.push_str(&format!(
            "<Default Extension=\"{}\" ContentType=\"{}\"/>",
            escape_attr(ext),
            escape_attr(content_type)
        ));
    }
    for (part, content_type) in overrides {
        out.push_str(&format!(
            "<Override PartName=\"/{}\" ContentType=\"{}\"/>",
            escape_attr(&encode_part_name(part)),
            escape_attr(content_type)
        ));
    }
    out.push_str("</Types>");
    out
}

const XML_DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";

/// Кодирует имя части для адреса: пробелы, русские буквы и прочие
/// символы вне разрешённого набора — как `%XX`.
fn encode_part_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for byte in name.bytes() {
        let allowed = byte.is_ascii_alphanumeric() || b"-._~/!$&'()*+,;=:@".contains(&byte);
        if allowed {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_names_are_percent_encoded() {
        assert_eq!(encode_part_name("word/document.xml"), "word/document.xml");
        assert_eq!(encode_part_name("word/my doc.xml"), "word/my%20doc.xml");
        assert_eq!(encode_part_name("word/Ж.xml"), "word/%D0%96.xml");
        assert_eq!(
            normalize_part_name(&encode_part_name("a/Ж b%.xml")),
            "a/Ж b%.xml"
        );
    }

    #[test]
    fn styles_relationship_gets_a_free_id_and_name() {
        let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId2" Type="http://x/image" Target="media/a.png"/>
<Relationship Id="rId1" Type="http://x/hyperlink" Target="https://example.com/a?b&amp;c" TargetMode="External"/>
</Relationships>"#;
        let source = SourcePackage {
            parts: vec![
                letter_model::SourcePart {
                    name: "word/_rels/document.xml.rels".into(),
                    data: rels.as_bytes().to_vec(),
                },
                letter_model::SourcePart {
                    name: "word/styles.xml".into(),
                    data: Vec::new(),
                },
            ],
            ..SourcePackage::default()
        };
        let (xml, name) =
            add_styles_relationship(&source, "word/document.xml", "word/_rels/document.xml.rels");
        assert_eq!(name, "word/styles2.xml");
        assert!(xml.contains(r#"Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="/word/styles2.xml""#));
        assert!(xml.contains(r#"Target="/word/media/a.png""#));
        assert!(xml.contains(r#"Target="https://example.com/a?b&amp;c" TargetMode="External""#));
    }
}
