//! Пакет OPC: ZIP-архив с частями, типами содержимого и связями.

use std::io::{Read, Seek};

use zip::ZipArchive;

use crate::ReadError;
use crate::xml;

/// Самая большая часть, которую мы согласны распаковать (512 МБ).
/// Защищает от «ZIP-бомб», которые при распаковке занимают всю память.
const MAX_PART_SIZE: u64 = 512 * 1024 * 1024;

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

    /// Находит главную часть документа через `_rels/.rels` и проверяет
    /// по `[Content_Types].xml`, что это именно документ Word.
    pub fn main_document_part(&mut self) -> Result<String, ReadError> {
        let main = match self.read_part("_rels/.rels")? {
            Some(bytes) => find_office_document(&xml::parse(&bytes)?),
            None => None,
        }
        // Если связей нет или они битые, пробуем стандартное место.
        .unwrap_or_else(|| "word/document.xml".to_string());

        if let Some(bytes) = self.read_part("[Content_Types].xml")? {
            let types = xml::parse(&bytes)?;
            if let Some(content_type) = content_type_of(&types, &main)
                && !WORD_MAIN_CONTENT_TYPES.contains(&content_type.as_str())
            {
                return Err(ReadError::NotAWordDocument(content_type));
            }
        }
        Ok(main)
    }
}

/// Ищет в `_rels/.rels` связь с главным документом.
fn find_office_document(rels: &xml::Element) -> Option<String> {
    rels.elements()
        .filter(|e| e.local == "Relationship")
        .find(|e| {
            attr(e, "Type")
                .is_some_and(|t| t == REL_OFFICE_DOCUMENT || t == REL_OFFICE_DOCUMENT_STRICT)
        })
        .and_then(|e| attr(e, "Target"))
        .map(normalize_part_name)
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

fn attr<'a>(element: &'a xml::Element, name: &str) -> Option<&'a str> {
    element
        .attrs
        .iter()
        .find(|a| a.ns.is_none() && a.local == name)
        .map(|a| a.value.as_str())
}

/// Превращает цель связи в имя части архива: убирает ведущий `/`,
/// разбирает `./` и `../`.
fn normalize_part_name(target: &str) -> String {
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
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
