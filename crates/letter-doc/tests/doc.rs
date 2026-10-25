//! Чтение .doc: файлы из `tests/fixtures/doc` (сохранены LibreOffice из
//! одноимённых .docx) сверяются с исходными .docx.

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use letter_model::{Block, Document, Paragraph};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

/// Абзацы документа по порядку, с абзацами ячеек таблиц.
fn paragraphs(doc: &Document) -> Vec<&Paragraph> {
    fn walk<'a>(blocks: &'a [Block], out: &mut Vec<&'a Paragraph>) {
        for block in blocks {
            match block {
                Block::Paragraph(p) => out.push(p),
                Block::Table(t) => {
                    for cell in t.rows.iter().flat_map(|r| &r.cells) {
                        walk(&cell.blocks, out);
                    }
                }
                Block::Unknown(_) => {}
            }
        }
    }
    let mut out = Vec::new();
    for section in &doc.sections {
        walk(&section.blocks, &mut out);
    }
    out
}

/// Тексты абзацев. Разрыв страницы не сравнивается: LibreOffice пишет
/// его в .doc свойством абзаца «с новой страницы», а не знаком.
fn texts(doc: &Document) -> Vec<String> {
    paragraphs(doc)
        .iter()
        .map(|p| p.plain_text().replace('\u{c}', ""))
        .collect()
}

#[test]
fn text_matches_the_original_docx() {
    let dir = fixtures().join("doc");
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    names.sort();
    assert!(names.len() >= 8);
    for path in names {
        let doc =
            letter_doc::read_file(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let name = path.file_stem().unwrap().to_str().unwrap();
        let original =
            letter_docx::read_file(fixtures().join("docx").join(format!("{name}.docx"))).unwrap();
        assert_eq!(texts(&doc), texts(&original), "{name}");
    }
}

#[test]
fn not_a_doc_file_is_an_error() {
    assert!(matches!(
        letter_doc::read(b"hello"),
        Err(letter_doc::ReadError::NotDoc)
    ));
    let docx = std::fs::read(fixtures().join("docx/plain-text.docx")).unwrap();
    assert!(letter_doc::read(&docx).is_err());
}
