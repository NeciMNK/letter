//! Проверяет, что набор тестовых файлов на месте и каждый файл похож на .docx.

use std::fs;
use std::path::PathBuf;

/// Файлы, которые описаны в `tests/fixtures/README.md`.
const EXPECTED: &[&str] = &[
    "empty.docx",
    "plain-text.docx",
    "bold-italic.docx",
    "headings.docx",
    "lists.docx",
    "table.docx",
    "image.docx",
    "floating.docx",
    "sections.docx",
    "headers.docx",
    "links.docx",
    "notes.docx",
    "borders.docx",
    "watermark.docx",
    "charts.docx",
    "header-footer.docx",
    "ru-en.docx",
    "pages.docx",
];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/docx")
}

#[test]
fn all_fixtures_are_zip_files() {
    for name in EXPECTED {
        let path = fixtures_dir().join(name);
        let bytes = fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        // .docx — это ZIP-архив, а любой ZIP начинается с букв «PK».
        assert!(bytes.starts_with(b"PK\x03\x04"), "{name} is not a ZIP file");
    }
}
