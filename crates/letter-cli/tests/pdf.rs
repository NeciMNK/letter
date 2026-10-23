//! Сохранение в PDF: текст, оглавление и ссылки читаются обратно.

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use letter_cli::{compare_files, export_pdf};
use letter_layout::FontLibrary;
use lopdf::{Document, Object};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/docx")
        .join(name)
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("letter-pdf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// PDF образца `name` (встроенные шрифты — одинаково на любой машине).
fn exported(name: &str) -> (PathBuf, Vec<u8>) {
    let doc = letter_docx::read_file(fixture(name)).unwrap();
    let bytes = export_pdf(&doc, &FontLibrary::embedded_only());
    let path = temp(&name.replace(".docx", ".pdf"));
    std::fs::write(&path, &bytes).unwrap();
    (path, bytes)
}

/// Текст в PDF стоит там же, где у нас на экране: разбор PDF (по
/// таблицам «глиф → буква» шрифтов) находит те же строки на тех же
/// местах.
#[test]
fn text_reads_back_in_place() {
    for name in [
        "ru-en.docx",
        "headings.docx",
        "bold-italic.docx",
        "table.docx",
        "lists.docx",
        "pages.docx",
        "borders.docx",
    ] {
        let (path, _) = exported(name);
        let result =
            compare_files(&fixture(name), &path, &FontLibrary::embedded_only(), 0.5).unwrap();
        assert!(result.our_lines > 0, "{name}");
        assert_eq!(result.our_pages, result.reference_pages, "{name}");
        assert_eq!(
            result.matched,
            result.our_lines,
            "{name}:\n{}",
            result.report()
        );
    }
}

#[test]
fn outline_and_info_from_headings() {
    let (_, bytes) = exported("headings.docx");
    let pdf = Document::load_mem(&bytes).unwrap();
    let catalog = pdf.catalog().unwrap();
    let outlines = pdf
        .get_dictionary(catalog.get(b"Outlines").unwrap().as_reference().unwrap())
        .unwrap();
    let first = pdf
        .get_dictionary(outlines.get(b"First").unwrap().as_reference().unwrap())
        .unwrap();
    assert_eq!(text(first.get(b"Title").unwrap()), "Heading 1");
    // «Heading 2» — вложенный пункт.
    let child = pdf
        .get_dictionary(first.get(b"First").unwrap().as_reference().unwrap())
        .unwrap();
    assert_eq!(text(child.get(b"Title").unwrap()), "Heading 2");

    let info = pdf
        .get_dictionary(pdf.trailer.get(b"Info").unwrap().as_reference().unwrap())
        .unwrap();
    assert_eq!(text(info.get(b"Title").unwrap()), "Заголовки");
}

#[test]
fn links_become_annotations() {
    let (_, bytes) = exported("links.docx");
    let pdf = Document::load_mem(&bytes).unwrap();
    let (_, page) = pdf.get_pages().into_iter().next().unwrap();
    let page = pdf.get_dictionary(page).unwrap();
    let annotations = page.get(b"Annots").unwrap().as_array().unwrap();
    let mut uri = None;
    let mut internal = false;
    for annotation in annotations {
        let annotation = pdf
            .get_dictionary(annotation.as_reference().unwrap())
            .unwrap();
        if let Ok(action) = annotation.get(b"A") {
            let action = action.as_dict().unwrap();
            uri = Some(text(action.get(b"URI").unwrap()));
        }
        internal |= annotation.get(b"Dest").is_ok();
    }
    assert_eq!(uri.as_deref(), Some("https://example.com/page?a=1&b=2"));
    assert!(internal, "нет перехода к закладке");
}

#[test]
fn pictures_and_rotated_text_are_written() {
    let (_, image) = exported("image.docx");
    let pdf = Document::load_mem(&image).unwrap();
    let images = pdf
        .objects
        .values()
        .filter(|o| {
            o.as_stream().is_ok_and(|s| {
                s.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image")
            })
        })
        .count();
    assert!(images >= 1);

    // Подложка повёрнута, но читается.
    let (path, _) = exported("watermark.docx");
    let out = letter_cli::pdf_lines(&path).unwrap();
    assert!(out.contains("Абзац 1"), "{out}");
}

/// Текст строки PDF (латиница или UTF-16 с меткой порядка байт).
fn text(object: &Object) -> String {
    let bytes = object.as_str().unwrap();
    if let Some(utf16) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = utf16
            .chunks(2)
            .map(|c| u16::from_be_bytes([c[0], c.get(1).copied().unwrap_or(0)]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}
