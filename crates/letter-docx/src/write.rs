//! Сборка пакета .docx при сохранении.
//!
//! Главная часть и стили строятся заново из модели. Все остальные части
//! исходного файла ([`SourcePackage`]) записываются байт в байт, вместе
//! со своими связями, поэтому картинки, колонтитулы, сноски и прочее,
//! что программа пока не понимает, не теряются.

use std::borrow::Cow;
use std::io::{Seek, Write};

use letter_model::{AppProperties, CoreProperties, Document, SourcePackage, Styles};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::WriteError;
use crate::package::{CONTENT_TYPES_PART, attr, normalize_part_name, rels_part_for};
use crate::properties;
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
const REL_CORE: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const REL_APP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
const CT_CORE: &str = "application/vnd.openxmlformats-package.core-properties+xml";
const CT_APP: &str = "application/vnd.openxmlformats-officedocument.extended-properties+xml";
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

    let mut links = Links::new(source);
    links.find_or_add("", REL_OFFICE_DOCUMENT, &["/officeDocument"], &main, true);
    let mut replaced = vec![main.clone()];

    let base_dir = main.rsplit_once('/').map_or("", |(dir, _)| dir);
    let styles_part = match &source.styles_part {
        Some(name) => Some(name.clone()),
        None if doc.styles != Styles::default() => Some(links.find_or_add(
            &main,
            REL_STYLES,
            &["/styles"],
            &join(base_dir, "styles.xml"),
            false,
        )),
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

    let core = &doc.properties.core;
    let core_part = match &source.core_part {
        Some(name) => Some(name.clone()),
        None if *core != CoreProperties::default() => Some(links.find_or_add(
            "",
            REL_CORE,
            &["/core-properties"],
            "docProps/core.xml",
            false,
        )),
        None => None,
    };
    if let Some(name) = &core_part {
        parts.push((name.clone(), properties::core_xml(core).into_bytes().into()));
        ensure_override(&mut overrides, name, CT_CORE);
        replaced.push(name.clone());
    }

    let app = &doc.properties.app;
    let app_part = match &source.app_part {
        Some(name) => Some(name.clone()),
        None if *app != AppProperties::default() => Some(links.find_or_add(
            "",
            REL_APP,
            &["/extended-properties", "/extendedProperties"],
            "docProps/app.xml",
            false,
        )),
        None => None,
    };
    if let Some(name) = &app_part {
        parts.push((name.clone(), properties::app_xml(app).into_bytes().into()));
        ensure_override(&mut overrides, name, CT_APP);
        replaced.push(name.clone());
    }

    for (name, xml) in links.changed() {
        parts.push((name.clone(), xml.into_bytes().into()));
        replaced.push(name);
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

/// Файлы связей, которые нужно изменить при записи.
struct Links<'a> {
    source: &'a SourcePackage,
    /// Изменённые файлы связей: имя файла связей и связи в нём.
    changed: Vec<(String, Vec<Relationship>)>,
}

impl<'a> Links<'a> {
    fn new(source: &'a SourcePackage) -> Links<'a> {
        Links {
            source,
            changed: Vec::new(),
        }
    }

    /// Ищет у части `from` (пустая строка — сам пакет) связь, тип которой
    /// оканчивается на один из `suffixes`, и возвращает имя её цели. Если
    /// связи нет, добавляет связь типа `kind` на свободное имя рядом
    /// с `wanted` (или ровно на `wanted`, если `exact`). Если связь есть,
    /// а часть-цель испорчена, новая часть запишется на её место.
    fn find_or_add(
        &mut self,
        from: &str,
        kind: &str,
        suffixes: &[&str],
        wanted: &str,
        exact: bool,
    ) -> String {
        let rels_name = rels_part_for(from);
        let index = match self.changed.iter().position(|(n, _)| *n == rels_name) {
            Some(i) => i,
            None => {
                let rels = self.load(from, &rels_name);
                if let Some(target) = find(&rels, suffixes) {
                    return target;
                }
                self.changed.push((rels_name, rels));
                self.changed.len() - 1
            }
        };
        if let Some(target) = find(&self.changed[index].1, suffixes) {
            return target;
        }
        let name = if exact {
            wanted.to_string()
        } else {
            self.free_name(wanted)
        };
        let rels = &mut self.changed[index].1;
        let mut number = rels.len() + 1;
        let id = loop {
            let id = format!("rId{number}");
            if !rels.iter().any(|r| r.id == id) {
                break id;
            }
            number += 1;
        };
        rels.push(Relationship {
            id,
            kind: kind.into(),
            target: name.clone(),
            external: false,
        });
        name
    }

    /// Связи из исходного файла (битый или отсутствующий файл — нет связей).
    fn load(&self, from: &str, rels_name: &str) -> Vec<Relationship> {
        let base_dir = from.rsplit_once('/').map_or("", |(dir, _)| dir);
        let Some(root) = self
            .source
            .part(rels_name)
            .and_then(|p| xml::parse(&p.data).ok())
        else {
            return Vec::new();
        };
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
    }

    /// `wanted`, если такое имя свободно, иначе `имя2.xml`, `имя3.xml`…
    fn free_name(&self, wanted: &str) -> String {
        let taken = |name: &str| {
            self.source.part(name).is_some()
                || name.eq_ignore_ascii_case(&self.source.main_part)
                || self
                    .changed
                    .iter()
                    .flat_map(|(_, rels)| rels)
                    .any(|r| !r.external && r.target.eq_ignore_ascii_case(name))
        };
        let (stem, ext) = wanted.rsplit_once('.').unwrap_or((wanted, ""));
        let mut name = wanted.to_string();
        let mut n = 1;
        while taken(&name) {
            n += 1;
            name = format!("{stem}{n}.{ext}");
        }
        name
    }

    /// Изменённые файлы связей: имя и содержимое.
    fn changed(self) -> Vec<(String, String)> {
        self.changed
            .into_iter()
            .map(|(name, rels)| (name, relationships_xml(&rels)))
            .collect()
    }
}

fn find(rels: &[Relationship], suffixes: &[&str]) -> Option<String> {
    rels.iter()
        .find(|r| !r.external && suffixes.iter().any(|s| r.kind.ends_with(s)))
        .map(|r| r.target.clone())
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
        let mut links = Links::new(&source);
        let name = links.find_or_add(
            "word/document.xml",
            REL_STYLES,
            &["/styles"],
            "word/styles.xml",
            false,
        );
        let changed = links.changed();
        assert_eq!(changed.len(), 1);
        let (rels_name, xml) = &changed[0];
        assert_eq!(rels_name, "word/_rels/document.xml.rels");
        assert_eq!(name, "word/styles2.xml");
        assert!(xml.contains(r#"Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="/word/styles2.xml""#));
        assert!(xml.contains(r#"Target="/word/media/a.png""#));
        assert!(xml.contains(r#"Target="https://example.com/a?b&amp;c" TargetMode="External""#));
    }
}
