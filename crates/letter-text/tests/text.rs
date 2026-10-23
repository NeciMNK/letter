//! Переход .docx → TXT / HTML / Markdown → модель на образцах из
//! `tests/fixtures/docx`.

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use letter_model::{Block, Document, Inline, Paragraph, Underline};

fn fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/docx");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "docx"))
        .collect();
    files.sort();
    assert!(files.len() >= 5);
    files
}

/// Абзацы по порядку, с абзацами ячеек таблиц.
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

/// Тексты абзацев без рисунков: в TXT их нет по смыслу, в HTML
/// и Markdown они пока не записываются.
fn texts(doc: &Document) -> Vec<String> {
    paragraphs(doc)
        .iter()
        .map(|p| without_pictures(&p.plain_text()))
        .collect()
}

fn without_pictures(text: &str) -> String {
    text.replace(letter_model::OBJECT_CHAR, "")
}

/// Текст без пустых абзацев (их нет в Markdown).
fn non_empty(doc: &Document) -> Vec<String> {
    texts(doc).into_iter().filter(|t| !t.is_empty()).collect()
}

#[test]
fn html_keeps_text_and_formatting() {
    for path in fixtures() {
        let name = path.display();
        let docx = letter_docx::read_file(&path).unwrap();
        let html = letter_text::write_html(&docx);
        let back = letter_text::read_html(html.as_bytes());
        let mut expected = texts(&docx);
        // Таблица в конце документа: после неё всегда есть абзац.
        if !matches!(docx.blocks().last(), Some(Block::Paragraph(_))) {
            expected.push(String::new());
        }
        assert_eq!(expected, texts(&back), "{name}");

        for (p, q) in paragraphs(&docx).into_iter().zip(paragraphs(&back)) {
            let (a, b) = (
                docx.resolve_paragraph(&p.props),
                back.resolve_paragraph(&q.props),
            );
            assert_eq!(a.alignment, b.alignment, "{name}");
            assert_eq!(
                (a.indent_left, a.indent_right),
                (b.indent_left, b.indent_right),
                "{name}"
            );
            assert_eq!(
                (a.space_before, a.space_after, a.line_spacing),
                (b.space_before, b.space_after, b.line_spacing),
                "{name}"
            );
            assert_eq!(a.page_break_before, b.page_break_before, "{name}");

            let runs = |p: &Paragraph| -> Vec<_> {
                p.content
                    .iter()
                    .filter_map(|i| match i {
                        Inline::Run(r) if !without_pictures(&r.plain_text()).is_empty() => {
                            Some(r.clone())
                        }
                        _ => None,
                    })
                    .collect()
            };
            let (left, right) = (runs(p), runs(q));
            // Соседние фрагменты с одинаковым видом при чтении сливаются:
            // сравниваем оформление каждого знака.
            let per_char = |doc: &Document, para: &Paragraph, runs: &[letter_model::Run]| {
                runs.iter()
                    .flat_map(|r| {
                        let x = doc.resolve_run(&para.props, &r.props);
                        let key = (
                            x.fonts.ascii.clone(),
                            x.size,
                            x.bold,
                            x.italic,
                            x.underline != Underline::None,
                            x.strike || x.double_strike,
                            x.color,
                            x.highlight,
                            x.vertical_align,
                        );
                        std::iter::repeat_n(key, without_pictures(&r.plain_text()).chars().count())
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                per_char(&docx, p, &left),
                per_char(&back, q, &right),
                "{name}"
            );
        }

        // Записанная заново страница не меняется. Со второй записи:
        // у пустых абзацев шрифт берётся из стилей, а стили HTML не хранит.
        let again = letter_text::write_html(&back);
        let third = letter_text::write_html(&letter_text::read_html(again.as_bytes()));
        assert_eq!(third, again, "{name}");
    }
}

#[test]
fn markdown_keeps_text() {
    for path in fixtures() {
        let name = path.display();
        let docx = letter_docx::read_file(&path).unwrap();
        let markdown = letter_text::write_markdown(&docx);
        let back = letter_text::read_markdown(markdown.as_bytes());
        // Разрывы строк и страниц Markdown передаёт не все.
        let clean = |t: Vec<String>| -> Vec<String> {
            t.into_iter()
                .map(|s| s.replace('\u{c}', ""))
                .filter(|s| !s.is_empty())
                .collect()
        };
        assert_eq!(clean(non_empty(&docx)), clean(non_empty(&back)), "{name}");
        assert_eq!(letter_text::write_markdown(&back), markdown, "{name}");
    }
}

#[test]
fn markdown_keeps_headings_and_emphasis() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/docx");
    let docx = letter_docx::read_file(dir.join("headings.docx")).unwrap();
    let markdown = letter_text::write_markdown(&docx);
    assert!(markdown.contains("\n# "), "{markdown}");
    assert!(markdown.contains("\n## "), "{markdown}");
    assert!(markdown.contains("\n### "), "{markdown}");
    let docx = letter_docx::read_file(dir.join("bold-italic.docx")).unwrap();
    let markdown = letter_text::write_markdown(&docx);
    assert!(markdown.contains("**bold**"), "{markdown}");
    assert!(markdown.contains("*italic*"), "{markdown}");
    assert!(markdown.contains("***bold italic***"), "{markdown}");
    assert!(markdown.contains("~~strike~~"), "{markdown}");
}

#[test]
fn text_keeps_lines() {
    for path in fixtures() {
        let name = path.display();
        let docx = letter_docx::read_file(&path).unwrap();
        let txt = letter_text::write_txt(&docx);
        let back = letter_text::read_txt(&txt);
        assert_eq!(letter_text::write_txt(&back), txt, "{name}");
        let lines = String::from_utf8(txt).unwrap();
        for text in non_empty(&docx) {
            for part in text.split('\n') {
                assert!(lines.contains(part.trim()), "{name}: {part}");
            }
        }
    }
}

#[test]
fn files_by_extension() {
    let dir = std::env::temp_dir().join(format!("letter-text-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut doc = Document::new();
    doc.sections[0].blocks = vec![Paragraph::text("Привет, мир").into()];
    for name in ["a.txt", "a.html", "a.md"] {
        let path = dir.join(name);
        letter_text::write_file(&doc, &path).unwrap();
        let back = letter_text::read_file(&path).unwrap();
        assert_eq!(texts(&back), ["Привет, мир"], "{name}");
    }
    assert!(letter_text::write_file(&doc, dir.join("a.xyz")).is_err());
    assert!(letter_text::read_file(dir.join("missing.txt")).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}
