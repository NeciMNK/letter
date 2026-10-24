//! Чтение .doc: файлы, которые LibreOffice сохранил из наших .docx
//! (`tests/fixtures/doc/`), сверяются с исходными .docx.

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use letter_doc::{ReadError, is_doc, read, read_file};
use letter_model::{Block, Document, Inline, Paragraph, RunContent, StyleKind, Twips};

const FIXTURES: &[&str] = &[
    "plain-text",
    "bold-italic",
    "headings",
    "lists",
    "table",
    "ru-en",
    "pages",
    "header-footer",
];

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn doc(name: &str) -> Document {
    read_file(dir().join(format!("doc/{name}.doc"))).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn docx(name: &str) -> Document {
    letter_docx::read_file(dir().join(format!("docx/{name}.docx"))).unwrap()
}

fn paragraphs(doc: &Document) -> Vec<&Paragraph> {
    doc.sections
        .iter()
        .flat_map(|s| &s.blocks)
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p),
            _ => None,
        })
        .collect()
}

#[test]
fn text_matches_the_original_docx() {
    for name in FIXTURES {
        let text = doc(name).plain_text();
        // В `pages` LibreOffice превратил разрыв страницы в абзаце
        // в «с новой страницы» у этого абзаца.
        let expected = docx(name).plain_text().replace('\u{c}', "");
        assert_eq!(text, expected, "{name}");
    }
}

#[test]
fn page_size_and_margins_match() {
    for name in FIXTURES {
        let (doc, docx) = (doc(name), docx(name));
        let props = &doc.sections[0].props;
        let original = &docx.sections[0].props;
        assert_eq!(props.page_size, original.page_size, "{name}");
        assert_eq!(
            props.margins.map(|m| (m.top, m.left, m.right, m.bottom)),
            original.margins.map(|m| (m.top, m.left, m.right, m.bottom)),
            "{name}"
        );
    }
}

#[test]
fn bold_and_italic() {
    let doc = doc("bold-italic");
    let runs: Vec<_> = paragraphs(&doc)
        .into_iter()
        .flat_map(|p| &p.content)
        .filter_map(|i| match i {
            Inline::Run(r) => Some(r),
            _ => None,
        })
        .collect();
    let find = |text: &str| {
        runs.iter()
            .find(|r| r.content == [RunContent::Text(text.into())])
            .unwrap_or_else(|| panic!("{text}"))
    };
    assert_eq!(find("bold").props.bold, Some(true));
    assert_eq!(find("italic").props.italic, Some(true));
    assert_eq!(find("italic").props.bold, None);
}

#[test]
fn headings_have_their_styles() {
    let doc = doc("headings");
    let styles: Vec<_> = paragraphs(&doc)
        .iter()
        .map(|p| p.props.style.clone())
        .collect();
    assert_eq!(styles[0].as_deref(), Some("Title"));
    assert_eq!(styles[1].as_deref(), Some("Heading1"));
    assert_eq!(styles[2], None);
    assert_eq!(styles[3].as_deref(), Some("Heading2"));
    let heading = doc.styles.get("Heading1").unwrap();
    assert_eq!(heading.kind, StyleKind::Paragraph);
    assert_eq!(heading.name.as_deref(), Some("heading 1"));
    assert_eq!(heading.based_on.as_deref(), Some("Normal"));
    assert!(heading.run.size.is_some() || heading.run.bold.is_some());
    assert!(doc.styles.get("Normal").unwrap().is_default);
}

#[test]
fn table_with_merged_cell() {
    let doc = doc("table");
    let table = doc.sections[0]
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::Table(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(table.grid, [Twips(2880); 3]);
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[0].cells.len(), 3);
    let last = &table.rows[2].cells;
    assert_eq!(last.len(), 2);
    assert_eq!(last[0].props.grid_span, Some(2));
    assert_eq!(last[0].plain_text(), "Merged R3C1–C2");
    assert_eq!(
        paragraphs(&doc).last().map(|p| p.plain_text()).as_deref(),
        Some("Text after the table.")
    );
}

#[test]
fn chapters_start_on_new_pages() {
    let doc = doc("pages");
    let all = paragraphs(&doc);
    let i = all
        .iter()
        .position(|p| p.plain_text() == "Chapter 2")
        .unwrap();
    // Разрыв — у самой главы или у пустого абзаца перед ней.
    let before = |p: &Paragraph| p.props.page_break_before == Some(true);
    assert!(before(all[i]) || (all[i - 1].plain_text().is_empty() && before(all[i - 1])));
}

#[test]
fn russian_text_and_fonts() {
    let doc = doc("ru-en");
    assert!(doc.plain_text().contains("Привет") || doc.plain_text().contains('ё'));
    assert!(doc.styles.defaults.run.fonts.ascii.is_some());
}

#[test]
fn broken_files_are_errors_not_crashes() {
    assert!(matches!(read(b"not a doc"), Err(ReadError::NotADoc)));
    assert!(!is_doc(b"PK\x03\x04"));
    let bytes = std::fs::read(dir().join("doc/table.doc")).unwrap();
    assert!(is_doc(&bytes));
    // Обрезанные и испорченные копии: только ошибки, без падений.
    for len in (0..bytes.len()).step_by(997) {
        let _ = read(&bytes[..len]);
    }
    for at in (0..bytes.len()).step_by(61) {
        let mut broken = bytes.clone();
        broken[at] ^= 0xA5;
        let _ = read(&broken);
    }
}
