//! Разновидности файла Word: .docx, .docm, .dotx, .dotm — шаблоны
//! и документы с макросами (макросы сохраняются, но не выполняются).

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;

use letter_docx::{FileKind, has_macros, kind_of, read_bytes, write_bytes, write_bytes_as};

const VBA_PROJECT: &[u8] = b"\xd0\xcf\x11\xe0 not really a VBA project";
const VBA_DATA: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><wne:vbaSuppData xmlns:wne="http://schemas.microsoft.com/office/word/2006/wordml"/>"#;

fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/docx")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Все части архива: имя → байты.
fn zip_parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut parts = BTreeMap::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).unwrap();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        parts.insert(file.name().unwrap().to_string(), data);
    }
    parts
}

fn zip(parts: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, data) in parts {
        writer.start_file(name.as_str(), options).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn text(parts: &BTreeMap<String, Vec<u8>>, name: &str) -> String {
    String::from_utf8(parts[name].clone()).unwrap()
}

/// Документ с макросами, как его сохраняет Word: проект VBA со своими
/// данными, связь на него из главной части и свой тип главной части.
fn macro_document() -> Vec<u8> {
    let mut parts = zip_parts(&fixture_bytes("plain-text.docx"));
    let types = text(&parts, "[Content_Types].xml")
        .replace(
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
            "application/vnd.ms-word.document.macroEnabled.main+xml",
        )
        .replace(
            "</Types>",
            "<Default Extension=\"bin\" ContentType=\"application/vnd.ms-office.vbaProject\"/>\
             <Override PartName=\"/word/vbaData.xml\" ContentType=\"application/vnd.ms-word.vbaData+xml\"/></Types>",
        );
    parts.insert("[Content_Types].xml".into(), types.into_bytes());
    let rels = text(&parts, "word/_rels/document.xml.rels").replace(
        "</Relationships>",
        "<Relationship Id=\"rIdVba\" Type=\"http://schemas.microsoft.com/office/2006/relationships/vbaProject\" Target=\"vbaProject.bin\"/></Relationships>",
    );
    parts.insert("word/_rels/document.xml.rels".into(), rels.into_bytes());
    parts.insert("word/vbaProject.bin".into(), VBA_PROJECT.to_vec());
    parts.insert(
        "word/_rels/vbaProject.bin.rels".into(),
        b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.microsoft.com/office/2006/relationships/wordVbaData\" Target=\"vbaData.xml\"/>\
</Relationships>"
            .to_vec(),
    );
    parts.insert("word/vbaData.xml".into(), VBA_DATA.as_bytes().to_vec());
    zip(&parts)
}

fn main_content_type(parts: &BTreeMap<String, Vec<u8>>) -> String {
    let types = text(parts, "[Content_Types].xml");
    let start = types.find("PartName=\"/word/document.xml\"").unwrap();
    let rest = &types[start..];
    let value = &rest[rest.find("ContentType=\"").unwrap() + 13..];
    value[..value.find('"').unwrap()].to_string()
}

#[test]
fn macros_survive_saving_a_macro_document() {
    let doc = read_bytes(&macro_document()).unwrap();
    assert_eq!(kind_of(&doc), FileKind::MacroDocument);
    assert!(has_macros(&doc));

    for saved in [
        write_bytes(&doc).unwrap(),
        write_bytes_as(&doc, FileKind::MacroDocument).unwrap(),
    ] {
        let parts = zip_parts(&saved);
        assert_eq!(parts["word/vbaProject.bin"], VBA_PROJECT);
        assert_eq!(text(&parts, "word/vbaData.xml"), VBA_DATA);
        assert!(text(&parts, "word/_rels/document.xml.rels").contains("vbaProject.bin"));
        assert_eq!(
            main_content_type(&parts),
            FileKind::MacroDocument.content_type()
        );
        let reopened = read_bytes(&saved).unwrap();
        assert_eq!(
            reopened.sections, doc.sections,
            "открыть → сохранить → открыть"
        );
        assert_eq!(reopened.styles, doc.styles);
        assert_eq!(kind_of(&reopened), FileKind::MacroDocument);
    }
}

#[test]
fn macro_template_keeps_macros() {
    let doc = read_bytes(&macro_document()).unwrap();
    let saved = write_bytes_as(&doc, FileKind::MacroTemplate).unwrap();
    let reopened = read_bytes(&saved).unwrap();
    assert_eq!(kind_of(&reopened), FileKind::MacroTemplate);
    assert!(has_macros(&reopened));
    assert_eq!(reopened.plain_text(), doc.plain_text());
}

#[test]
fn macros_are_dropped_in_a_file_without_macros() {
    let doc = read_bytes(&macro_document()).unwrap();
    for kind in [FileKind::Document, FileKind::Template] {
        let saved = write_bytes_as(&doc, kind).unwrap();
        let parts = zip_parts(&saved);
        for name in [
            "word/vbaProject.bin",
            "word/vbaData.xml",
            "word/_rels/vbaProject.bin.rels",
        ] {
            assert!(!parts.contains_key(name), "{kind:?}: {name}");
        }
        assert!(!text(&parts, "word/_rels/document.xml.rels").contains("vbaProject"));
        let types = text(&parts, "[Content_Types].xml");
        assert!(!types.contains("vbaData"), "{types}");
        assert_eq!(main_content_type(&parts), kind.content_type());

        let reopened = read_bytes(&saved).unwrap();
        assert_eq!(kind_of(&reopened), kind);
        assert!(!has_macros(&reopened));
        assert_eq!(reopened.plain_text(), doc.plain_text());
        assert_eq!(reopened.sections, doc.sections);
    }
}

#[test]
fn template_round_trip_and_back_to_document() {
    let docx = read_bytes(&fixture_bytes("plain-text.docx")).unwrap();
    assert_eq!(kind_of(&docx), FileKind::Document);
    let template = read_bytes(&write_bytes_as(&docx, FileKind::Template).unwrap()).unwrap();
    assert_eq!(kind_of(&template), FileKind::Template);
    assert_eq!(template.sections, docx.sections);
    assert_eq!(template.styles, docx.styles);

    // Обычное сохранение шаблон шаблоном и оставляет.
    let again = read_bytes(&write_bytes(&template).unwrap()).unwrap();
    assert_eq!(kind_of(&again), FileKind::Template);

    let back = write_bytes_as(&template, FileKind::Document).unwrap();
    assert_eq!(
        main_content_type(&zip_parts(&back)),
        FileKind::Document.content_type()
    );
    let back = read_bytes(&back).unwrap();
    assert_eq!(kind_of(&back), FileKind::Document);
    assert_eq!(back.sections, docx.sections);
}

#[test]
fn new_document_can_be_saved_as_any_kind() {
    let doc = read_bytes(&fixture_bytes("empty.docx")).unwrap();
    let mut fresh = doc.clone();
    fresh.source = None;
    for kind in FileKind::ALL {
        let reopened = read_bytes(&write_bytes_as(&fresh, kind).unwrap()).unwrap();
        assert_eq!(kind_of(&reopened), kind);
    }
}
