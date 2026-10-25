//! Исправления (`w:ins`, `w:del`): чтение и «открыть → сохранить → открыть».

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Cursor, Read};
use std::path::PathBuf;

use letter_docx::{read_bytes, write_bytes};
use letter_model::{Block, Document, Hyperlink, Paragraph, Revision, RevisionKind, Run};

fn fixture() -> Document {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/docx/revisions.docx");
    letter_docx::read_file(path).unwrap()
}

fn main_part(bytes: &[u8]) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut file = archive.by_name("word/document.xml").unwrap();
    let mut text = String::new();
    file.read_to_string(&mut text).unwrap();
    text
}

fn first_paragraph(doc: &Document) -> &Paragraph {
    match doc.blocks().next() {
        Some(Block::Paragraph(p)) => p,
        _ => panic!("нет абзаца"),
    }
}

#[test]
fn insertions_and_deletions_are_read() {
    let doc = fixture();
    let p = first_paragraph(&doc);
    // Удалённый текст тоже в абзаце: он виден зачёркнутым.
    assert_eq!(p.plain_text(), "The cat sat on jumped over the mat.");
    let runs: Vec<&Run> = p.runs().collect();
    let deleted = runs[1].props.revision.as_ref().unwrap();
    assert_eq!(deleted.kind, RevisionKind::Delete);
    assert_eq!(deleted.author, "Анна");
    assert_eq!(deleted.date.as_deref(), Some("2026-10-25T09:00:00Z"));
    let inserted = runs[2].props.revision.as_ref().unwrap();
    assert_eq!(inserted.kind, RevisionKind::Insert);
    assert_eq!(inserted.id, "2");
    assert_eq!(runs[2].props.bold, Some(true));
    assert!(runs[0].props.revision.is_none() && runs[3].props.revision.is_none());
}

#[test]
fn revisions_survive_round_trip() {
    let doc = fixture();
    let bytes = write_bytes(&doc).unwrap();
    let xml = main_part(&bytes);
    assert!(xml.contains(r#"<w:del w:id="1" w:author="Анна""#), "{xml}");
    assert!(xml.contains(r#"<w:delText xml:space="preserve">sat on </w:delText>"#));
    assert!(xml.contains(r#"<w:ins w:id="2" w:author="Boris""#));
    assert_eq!(read_bytes(&bytes).unwrap().sections, doc.sections);
}

#[test]
fn neighbouring_runs_share_one_revision_and_link() {
    let revision = Revision {
        kind: RevisionKind::Insert,
        id: "7".into(),
        author: "Никита".into(),
        date: None,
    };
    let mut a = Run::text("a ");
    let mut b = Run::text("b");
    b.props.bold = Some(true);
    let mut c = Run::text("c");
    c.props.link = Some(Hyperlink::to("https://example.com"));
    for run in [&mut a, &mut b, &mut c] {
        run.props.revision = Some(revision.clone());
    }
    let mut doc = Document::new();
    doc.sections[0].blocks = vec![Paragraph::from_runs([a, b, c, Run::text(" d")]).into()];
    let bytes = write_bytes(&doc).unwrap();
    let xml = main_part(&bytes);
    assert_eq!(xml.matches("<w:ins ").count(), 1, "{xml}");
    assert!(xml.contains("<w:ins w:id=\"7\" w:author=\"Никита\"><w:r>"));
    assert!(xml.contains("</w:hyperlink></w:ins><w:r>"), "{xml}");
    let again = read_bytes(&bytes).unwrap();
    let revisions = |d: &Document| -> Vec<Option<Revision>> {
        first_paragraph(d)
            .runs()
            .map(|r| r.props.revision.clone())
            .collect()
    };
    assert_eq!(revisions(&again), revisions(&doc));
    assert_eq!(first_paragraph(&again).plain_text(), "a bc d");
}
