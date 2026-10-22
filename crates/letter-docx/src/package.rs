//! Пакет OPC: ZIP-архив с частями, типами содержимого и связями.

use std::io::{Read, Seek};

use zip::ZipArchive;

use crate::ReadError;
use crate::xml;

/// Самая большая часть, которую мы согласны распаковать (512 МБ).
/// Защищает от «ZIP-бомб», которые при распаковке занимают всю память.
const MAX_PART_SIZE: u64 = 512 * 1024 * 1024;

/// Часть с типами содержимого всех остальных частей.
pub(crate) const CONTENT_TYPES_PART: &str = "[Content_Types].xml";

/// Тип связи «главный документ» в `_rels/.rels`.
const REL_OFFICE_DOCUMENT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
/// То же для строгого (Strict) варианта OOXML.
const REL_OFFICE_DOCUMENT_STRICT: &str =
    "http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument";

/// Типы содержимого главной части, которые мы считаем документом Word:
/// документ, документ с макросами, шаблон, шаблон с макросами.
const WORD_MAIN_CONTENT_TYPES: &[&str] = &[
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
    "application/vnd.ms-word.document.macroEnabled.main+xml",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml",
    "application/vnd.ms-word.template.macroEnabledTemplate.main+xml",
];

/// Открытый пакет .docx.
pub(crate) struct Package<R> {
    archive: ZipArchive<R>,
}

impl<R: Read + Seek> Package<R> {
    /// Открывает ZIP-архив.
    pub fn open(reader: R) -> Result<Package<R>, ReadError> {
        Ok(Package {
            archive: ZipArchive::new(reader)?,
        })
    }

    /// Читает часть по имени (`word/document.xml`, без ведущего `/`).
    /// Имена частей в OPC не зависят от регистра букв.
    pub fn read_part(&mut self, name: &str) -> Result<Option<Vec<u8>>, ReadError> {
        let index = match self.archive.index_for_name(name) {
            Some(i) => i,
            None => match (0..self.archive.len()).find(|&i| {
                self.archive
                    .name_for_index(i)
                    .is_some_and(|n| n.is_ok_and(|n| n.eq_ignore_ascii_case(name)))
            }) {
                Some(i) => i,
                None => return Ok(None),
            },
        };
        let file = self.archive.by_index(index)?;
        if file.size() > MAX_PART_SIZE {
            return Err(ReadError::PartTooLarge(name.to_string()));
        }
        let mut bytes = Vec::new();
        // `take` — на случай, если размер в заголовке архива подделан.
        file.take(MAX_PART_SIZE + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_PART_SIZE {
            return Err(ReadError::PartTooLarge(name.to_string()));
        }
        Ok(Some(bytes))
    }

    /// Имена всех частей архива в порядке их следования (без папок).
    pub fn part_names(&self) -> Vec<String> {
        self.archive
            .file_names()
            .filter_map(Result::ok)
            .filter(|n| !n.ends_with('/'))
            .map(|n| n.into_owned())
            .collect()
    }

    /// Находит главную часть документа через `_rels/.rels` и проверяет
    /// по `[Content_Types].xml`, что это именно документ Word.
    pub fn main_document_part(&mut self) -> Result<String, ReadError> {
        let main = match self.read_part("_rels/.rels")? {
            Some(bytes) => find_office_document(&xml::parse(&bytes)?),
            None => None,
        }
        // Если связей нет или они битые, пробуем стандартное место.
        .unwrap_or_else(|| "word/document.xml".to_string());

        if let Some(bytes) = self.read_part(CONTENT_TYPES_PART)? {
            let types = xml::parse(&bytes)?;
            if let Some(content_type) = content_type_of(&types, &main)
                && !WORD_MAIN_CONTENT_TYPES.contains(&content_type.as_str())
            {
                return Err(ReadError::NotAWordDocument(content_type));
            }
        }
        Ok(main)
    }

    /// Связи части `part` (из `<папка>/_rels/<имя>.rels`). Внешние связи
    /// (ссылки на сайты) пропускаются. Битый или отсутствующий файл связей
    /// означает «связей нет».
    pub fn relationships(&mut self, part: &str) -> Vec<Relationship> {
        let dir = part.rsplit_once('/').map_or("", |(dir, _)| dir);
        match self.read_part(&rels_part_for(part)) {
            Ok(Some(bytes)) => match xml::parse(&bytes) {
                Ok(root) => parse_relationships(&root, dir),
                Err(_) => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    /// Первая связь части `part` с типом, оканчивающимся на `/<kind>`
    /// (например, `styles`), — имя части-цели.
    pub fn related_part(&mut self, part: &str, kind: &str) -> Option<String> {
        let suffix = format!("/{kind}");
        self.relationships(part)
            .into_iter()
            .find(|r| r.kind.ends_with(&suffix))
            .map(|r| r.target)
    }
}

/// Имя файла связей части: `word/document.xml` → `word/_rels/document.xml.rels`.
pub(crate) fn rels_part_for(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, name)) => format!("{dir}/_rels/{name}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// Связь между частями пакета.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Relationship {
    /// Имя связи (`rId5`): по нему на связь ссылается часть.
    pub id: String,
    /// Тип связи (URI).
    pub kind: String,
    /// Имя части-цели внутри архива.
    pub target: String,
}

/// Разбирает файл связей. Цели считаются относительно папки `base_dir`.
fn parse_relationships(rels: &xml::Element, base_dir: &str) -> Vec<Relationship> {
    rels.elements()
        .filter(|e| e.local == "Relationship")
        .filter(|e| attr(e, "TargetMode") != Some("External"))
        .filter_map(|e| {
            let id = attr(e, "Id").unwrap_or_default().to_string();
            let kind = attr(e, "Type")?.to_string();
            let target = attr(e, "Target")?;
            let target = if target.starts_with('/') || base_dir.is_empty() {
                normalize_part_name(target)
            } else {
                normalize_part_name(&format!("{base_dir}/{target}"))
            };
            Some(Relationship { id, kind, target })
        })
        .collect()
}

/// Ищет в `_rels/.rels` связь с главным документом.
fn find_office_document(rels: &xml::Element) -> Option<String> {
    parse_relationships(rels, "")
        .into_iter()
        .find(|r| r.kind == REL_OFFICE_DOCUMENT || r.kind == REL_OFFICE_DOCUMENT_STRICT)
        .map(|r| r.target)
}

/// Содержимое `[Content_Types].xml`: пары «расширение — тип» (`Default`)
/// и «имя части — тип» (`Override`). Имена частей — без ведущего `/`.
pub(crate) fn read_content_types(types: &xml::Element) -> ContentTypes {
    let mut result = ContentTypes::default();
    for element in types.elements() {
        let content_type = attr(element, "ContentType");
        match (element.local.as_str(), content_type) {
            ("Default", Some(content_type)) => {
                if let Some(ext) = attr(element, "Extension") {
                    result
                        .defaults
                        .push((ext.to_string(), content_type.to_string()));
                }
            }
            ("Override", Some(content_type)) => {
                if let Some(part) = attr(element, "PartName") {
                    let part = normalize_part_name(part);
                    result.overrides.push((part, content_type.to_string()));
                }
            }
            _ => {}
        }
    }
    result
}

/// Типы содержимого пакета, см. [`read_content_types`].
#[derive(Debug, Default)]
pub(crate) struct ContentTypes {
    pub defaults: Vec<(String, String)>,
    pub overrides: Vec<(String, String)>,
}

/// Тип содержимого части по `[Content_Types].xml`: сначала точное
/// совпадение (`Override`), потом по расширению (`Default`).
fn content_type_of(types: &xml::Element, part: &str) -> Option<String> {
    let part_name = format!("/{part}");
    let by_override = types
        .elements()
        .filter(|e| e.local == "Override")
        .find(|e| attr(e, "PartName").is_some_and(|p| p.eq_ignore_ascii_case(&part_name)))
        .and_then(|e| attr(e, "ContentType"));
    let extension = part.rsplit_once('.').map(|(_, ext)| ext)?;
    by_override
        .or_else(|| {
            types
                .elements()
                .filter(|e| e.local == "Default")
                .find(|e| attr(e, "Extension").is_some_and(|x| x.eq_ignore_ascii_case(extension)))
                .and_then(|e| attr(e, "ContentType"))
        })
        .map(str::to_string)
}

pub(crate) fn attr<'a>(element: &'a xml::Element, name: &str) -> Option<&'a str> {
    element
        .attrs
        .iter()
        .find(|a| a.ns.is_none() && a.local == name)
        .map(|a| a.value.as_str())
}

/// Превращает цель связи в имя части архива: убирает ведущий `/`,
/// разбирает `./` и `../`.
pub(crate) fn normalize_part_name(target: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    percent_decode(&parts.join("/"))
}

/// Раскодирует `%XX` в адресе (например, `%20` — пробел). Неправильные
/// последовательности оставляет как есть.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && let (Some(&h), Some(&l)) = (bytes.get(i + 1), bytes.get(i + 2))
            && let (Some(h), Some(l)) = (hex(h), hex(l))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rels_part_names() {
        assert_eq!(
            rels_part_for("word/document.xml"),
            "word/_rels/document.xml.rels"
        );
        assert_eq!(rels_part_for("doc.xml"), "_rels/doc.xml.rels");
    }

    #[test]
    fn normalize_targets() {
        assert_eq!(
            normalize_part_name("word/document.xml"),
            "word/document.xml"
        );
        assert_eq!(
            normalize_part_name("/word/document.xml"),
            "word/document.xml"
        );
        assert_eq!(
            normalize_part_name("./word/../word/doc.xml"),
            "word/doc.xml"
        );
        assert_eq!(normalize_part_name("../../x.xml"), "x.xml");
        assert_eq!(normalize_part_name("word/my%20doc.xml"), "word/my doc.xml");
        assert_eq!(normalize_part_name("word/%D0%96.xml"), "word/Ж.xml");
        assert_eq!(normalize_part_name("word/100%.xml"), "word/100%.xml");
        assert_eq!(normalize_part_name("word/%FF.xml"), "word/%FF.xml");
    }

    #[test]
    fn relationships_are_relative_to_the_source_part() {
        let rels = xml::parse(
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://x/relationships/styles" Target="styles.xml"/>
<Relationship Id="rId2" Type="http://x/relationships/theme" Target="theme/theme1.xml"/>
<Relationship Id="rId3" Type="http://x/relationships/customXml" Target="../customXml/item1.xml"/>
<Relationship Id="rId4" Type="http://x/relationships/hyperlink" Target="https://example.com" TargetMode="External"/>
<Relationship Id="rId5" Type="http://x/relationships/image" Target="/media/a.png"/>
</Relationships>"#,
        )
        .unwrap();
        let targets: Vec<_> = parse_relationships(&rels, "word")
            .into_iter()
            .map(|r| r.target)
            .collect();
        assert_eq!(
            targets,
            [
                "word/styles.xml",
                "word/theme/theme1.xml",
                "customXml/item1.xml",
                "media/a.png"
            ]
        );
    }

    #[test]
    fn content_types() {
        let types = xml::parse(
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.ms-word.document.macroEnabled.main+xml"/>
</Types>"#,
        )
        .unwrap();
        assert_eq!(
            content_type_of(&types, "word/document.xml").as_deref(),
            Some("application/vnd.ms-word.document.macroEnabled.main+xml")
        );
        assert_eq!(
            content_type_of(&types, "word/other.XML").as_deref(),
            Some("application/xml")
        );
        assert_eq!(content_type_of(&types, "word/noext"), None);

        let all = read_content_types(&types);
        assert_eq!(all.defaults, [("xml".into(), "application/xml".into())]);
        assert_eq!(all.overrides[0].0, "word/document.xml");
    }
}
