//! Вёрстка настоящих документов из `tests/fixtures/docx`.

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use letter_layout::{FontLibrary, LineItem, Shaper, layout_document};
use letter_model::{Block, Document};

fn fixtures() -> Vec<(String, Document)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/docx");
    let mut docs: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "docx"))
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, letter_docx::read_file(&p).unwrap())
        })
        .collect();
    docs.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(docs.len() >= 9);
    docs
}

/// Абзацы документа по порядку, с абзацами таблиц.
fn paragraph_texts(blocks: &[Block], out: &mut Vec<String>) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => out.push(p.plain_text()),
            Block::Table(t) => {
                for cell in t.rows.iter().flat_map(|r| &r.cells) {
                    paragraph_texts(&cell.blocks, out);
                }
            }
            Block::Unknown(_) => {}
        }
    }
}

#[test]
fn fixtures_are_laid_out_on_pages() {
    let library = FontLibrary::embedded_only();
    let mut shaper = Shaper::new();
    for (name, doc) in fixtures() {
        let layout = layout_document(&doc, &mut shaper, &library);
        assert!(!layout.pages.is_empty(), "{name}");
        let mut texts = Vec::new();
        for section in &doc.sections {
            paragraph_texts(&section.blocks, &mut texts);
        }
        let mut seen = vec![String::new(); texts.len()];
        for page in &layout.pages {
            assert!(page.width > 100.0 && page.height > 100.0, "{name}");
            for line in &page.lines {
                // Строка на листе и не налезает на поля сверху.
                assert!(line.top >= page.margins.top - 0.01, "{name}");
                assert!(line.top + line.height <= page.height, "{name}");
                assert!(line.baseline > line.top, "{name}");
                for item in &line.line.items {
                    assert!(item.x() >= -0.01, "{name}: {item:?}");
                    if let LineItem::Text(t) = item {
                        assert!(t.span < line.spans.len(), "{name}");
                        assert!(t.run.glyphs.iter().all(|g| g.glyph_id != 0), "{name}");
                    }
                }
                seen[line.paragraph].push_str(&line_text(line));
            }
        }
        // Весь текст абзацев попал на страницы.
        for (text, laid) in texts.iter().zip(&seen) {
            let expected: String = text.chars().filter(|c| !c.is_whitespace()).collect();
            let got: String = laid.chars().filter(|c| !c.is_whitespace()).collect();
            assert_eq!(got, expected, "{name}");
        }
    }
}

/// Текст строки.
fn line_text(line: &letter_layout::PageLine) -> String {
    line.text
        .get(line.line.range.clone())
        .unwrap_or_default()
        .to_string()
}
