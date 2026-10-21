//! Чтение настоящих файлов .docx из `tests/fixtures/docx` и битых файлов.

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Cursor, Write};
use std::path::PathBuf;

use letter_docx::{ReadError, read_bytes, read_file};
use letter_model::{
    Block, Color, Document, HalfPoints, Inline, PageSize, Paragraph, RunContent, Underline,
    VerticalAlign,
};

fn fixture(name: &str) -> Document {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/docx")
        .join(name);
    read_file(&path).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn paragraphs(doc: &Document) -> Vec<&Paragraph> {
    doc.blocks()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p),
            _ => None,
        })
        .collect()
}

#[test]
fn empty() {
    let doc = fixture("empty.docx");
    assert_eq!(doc.sections.len(), 1);
    assert_eq!(doc.plain_text(), "");
    // python-docx создаёт документ размера Letter.
    assert_eq!(
        doc.sections[0].props.page_size.map(|s| s.width),
        Some(PageSize::LETTER.width)
    );
}

#[test]
fn plain_text() {
    let doc = fixture("plain-text.docx");
    let text = doc.plain_text();
    assert!(text.starts_with("Hello, world!\nThis is a simple paragraph"));
    assert!(text.ends_with("\nТретий абзац."));
}

#[test]
fn bold_italic() {
    let doc = fixture("bold-italic.docx");
    let p = paragraphs(&doc)[0];
    let find = |text: &str| {
        p.runs()
            .find(|r| r.plain_text() == text)
            .unwrap_or_else(|| panic!("no run {text:?}"))
    };
    assert_eq!(find("bold").props.bold, Some(true));
    assert_eq!(find("italic").props.italic, Some(true));
    let both = &find("bold italic").props;
    assert_eq!((both.bold, both.italic), (Some(true), Some(true)));
    assert_eq!(find("underline").props.underline, Some(Underline::Single));
    assert_eq!(find("strike").props.strike, Some(true));
    let red = &find("red 16pt").props;
    assert_eq!(red.color, Some(Color::Rgb(0xC0, 0, 0)));
    assert_eq!(red.size, Some(HalfPoints(32)));
    let indexes: Vec<_> = p.runs().filter_map(|r| r.props.vertical_align).collect();
    assert_eq!(
        indexes,
        [VerticalAlign::Subscript, VerticalAlign::Superscript]
    );
    assert_eq!(
        p.plain_text(),
        "Normal, bold, italic, bold italic, underline, strike, red 16pt, H2O, x2."
    );
}

#[test]
fn headings() {
    let doc = fixture("headings.docx");
    let styles: Vec<_> = paragraphs(&doc)
        .iter()
        .map(|p| p.props.style.as_deref().unwrap_or("-"))
        .collect();
    assert_eq!(
        styles,
        ["Title", "Heading1", "-", "Heading2", "-", "Heading3", "-"]
    );
}

#[test]
fn lists() {
    let doc = fixture("lists.docx");
    let ps = paragraphs(&doc);
    assert_eq!(ps[1].props.style.as_deref(), Some("ListBullet"));
    assert_eq!(ps[1].plain_text(), "Apples");
    assert_eq!(ps[5].props.style.as_deref(), Some("ListNumber"));
}

#[test]
fn table() {
    let doc = fixture("table.docx");
    let table = doc
        .blocks()
        .find_map(|b| match b {
            Block::Table(t) => Some(t),
            _ => None,
        })
        .expect("table");
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[0].cells.len(), 3);
    assert_eq!(table.rows[0].cells[1].plain_text(), "R1C2");
    // Объединённая ячейка: в строке две ячейки, у первой gridSpan в свойствах.
    assert_eq!(table.rows[2].cells.len(), 2);
    assert!(
        table.rows[2].cells[0].raw_props[0]
            .as_str()
            .contains("gridSpan")
    );
    assert!(table.raw_props.iter().any(|r| r.local_name() == "tblGrid"));
}

#[test]
fn image_is_kept_as_unknown() {
    let doc = fixture("image.docx");
    let drawing = paragraphs(&doc)
        .iter()
        .flat_map(|p| p.runs())
        .flat_map(|r| &r.content)
        .find_map(|c| match c {
            RunContent::Unknown(raw) if raw.local_name() == "drawing" => Some(raw),
            _ => None,
        })
        .expect("drawing");
    assert!(drawing.as_str().contains("r:embed"));
    assert!(drawing.as_str().contains("xmlns:r="));
}

#[test]
fn header_footer_references_are_kept() {
    let doc = fixture("header-footer.docx");
    let names: Vec<_> = doc.sections[0]
        .props
        .unknown
        .iter()
        .map(|r| r.local_name())
        .collect();
    assert!(names.contains(&"headerReference"), "{names:?}");
    assert!(names.contains(&"footerReference"), "{names:?}");
}

#[test]
fn russian_and_english() {
    let doc = fixture("ru-en.docx");
    let text = doc.plain_text();
    assert!(text.contains("Съешь же ещё этих мягких французских булок"));
    assert!(text.contains("The quick brown fox"));
    assert!(text.contains("«кавычки», № 5, 10 €"));
    let mixed = paragraphs(&doc)[2];
    assert!(matches!(&mixed.content[1], Inline::Run(r) if r.props.bold == Some(true)));
}

/// Собирает ZIP-архив из пар «имя → содержимое».
fn zip(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in parts {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

const MINIMAL_DOCUMENT: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>hi</w:t></w:r></w:p></w:body></w:document>"#;

#[test]
fn minimal_package_without_rels_or_content_types() {
    let doc = read_bytes(&zip(&[("word/document.xml", MINIMAL_DOCUMENT)])).unwrap();
    assert_eq!(doc.plain_text(), "hi");
}

#[test]
fn main_part_found_through_relationships() {
    let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="/custom/main.xml"/></Relationships>"#;
    let doc = read_bytes(&zip(&[
        ("_rels/.rels", rels),
        ("custom/main.xml", MINIMAL_DOCUMENT),
    ]))
    .unwrap();
    assert_eq!(doc.plain_text(), "hi");
}

#[test]
fn broken_files_are_errors_not_crashes() {
    assert!(matches!(read_bytes(b""), Err(ReadError::Zip(_))));
    assert!(matches!(
        read_bytes(b"not a zip at all"),
        Err(ReadError::Zip(_))
    ));
    assert!(matches!(
        read_bytes(&zip(&[("other.txt", b"x")])),
        Err(ReadError::MissingPart(_))
    ));
    assert!(matches!(
        read_bytes(&zip(&[("word/document.xml", b"<w:document><unclosed>")])),
        Err(ReadError::Xml(_))
    ));
    let excel_types = br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/></Types>"#;
    assert!(matches!(
        read_bytes(&zip(&[
            ("[Content_Types].xml", excel_types),
            ("word/document.xml", MINIMAL_DOCUMENT),
        ])),
        Err(ReadError::NotAWordDocument(_))
    ));

    // Обрезанный настоящий файл.
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/docx/table.docx");
    let bytes = std::fs::read(path).unwrap();
    for len in [10, bytes.len() / 2, bytes.len() - 10] {
        assert!(read_bytes(&bytes[..len]).is_err(), "truncated to {len}");
    }
    assert!(matches!(
        read_file("/no/such/file.docx"),
        Err(ReadError::Io(_))
    ));
}
