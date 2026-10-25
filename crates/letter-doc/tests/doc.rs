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

/// Тексты непустых абзацев. Разрыв страницы не сравнивается: LibreOffice
/// пишет его в .doc свойством абзаца «с новой страницы», а не знаком.
/// Пустые абзацы тоже: пустой абзац с концом раздела LibreOffice
/// сливает с предыдущим.
fn texts(doc: &Document) -> Vec<String> {
    paragraphs(doc)
        .iter()
        .map(|p| p.plain_text().replace('\u{c}', ""))
        .filter(|t| !t.is_empty())
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
    let mut compared = 0;
    for path in names {
        let name = path.file_stem().unwrap().to_str().unwrap();
        let source = fixtures().join("docx").join(format!("{name}.docx"));
        // Файлы, сделанные не из .docx (например, из HTML), — в своих тестах.
        if path.extension().is_none_or(|e| e != "doc") || !source.exists() {
            continue;
        }
        let doc =
            letter_doc::read_file(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let original = letter_docx::read_file(source).unwrap();
        assert_eq!(texts(&doc), texts(&original), "{name}");
        compared += 1;
    }
    assert!(compared >= 11);
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

/// Оформление фрагмента с текстом `text`.
fn run_with<'a>(doc: &'a Document, text: &str) -> &'a letter_model::RunProps {
    paragraphs(doc)
        .into_iter()
        .flat_map(|p| &p.content)
        .find_map(|inline| match inline {
            letter_model::Inline::Run(run) if run.plain_text() == text => Some(&run.props),
            _ => None,
        })
        .unwrap_or_else(|| panic!("нет фрагмента {text:?}"))
}

#[test]
fn run_formatting_is_read() {
    let doc = letter_doc::read_file(fixtures().join("doc/bold-italic.doc")).unwrap();
    assert_eq!(run_with(&doc, "bold").bold, Some(true));
    assert_eq!(run_with(&doc, "italic").italic, Some(true));
    let red = run_with(&doc, "red 16pt");
    assert_eq!(red.size, Some(letter_model::HalfPoints(32)));
    assert!(red.color.is_some());
}

#[test]
fn paragraph_styles_are_read() {
    let doc = letter_doc::read_file(fixtures().join("doc/headings.doc")).unwrap();
    let styles: Vec<_> = paragraphs(&doc)
        .iter()
        .map(|p| p.props.style.as_deref())
        .collect();
    assert_eq!(styles[0], Some("Title"));
    assert_eq!(styles[1], Some("Heading1"));
    assert_eq!(styles[3], Some("Heading2"));
    let heading = doc.styles.get("Heading1").expect("стиль Heading1");
    assert_eq!(heading.name.as_deref(), Some("heading 1"));
}

fn read(name: &str) -> Document {
    letter_doc::read_file(fixtures().join("doc").join(name)).unwrap()
}

/// Тексты абзацев блоков (без вложенных таблиц).
fn block_texts(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p.plain_text()),
            _ => None,
        })
        .collect()
}

#[test]
fn tables_are_read() {
    let doc = read("table.doc");
    let Some(Block::Table(table)) = doc.sections[0].blocks.get(1) else {
        panic!("нет таблицы");
    };
    assert_eq!(table.grid.len(), 3);
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[0].cells.len(), 3);
    assert_eq!(table.rows[0].cells[0].plain_text(), "R1C1");
    // Объединённая ячейка последней строки занимает два столбца.
    let last = &table.rows[2];
    assert_eq!(last.cells.len(), 2);
    assert_eq!(last.cells[0].span(), 2);
    assert_eq!(last.cells[0].plain_text(), "Merged R3C1–C2");
    assert!(table.rows[0].cells[0].props.borders.is_some());
    assert_eq!(
        block_texts(&doc.sections[0].blocks[2..]),
        ["Text after the table."]
    );
}

#[test]
fn nested_tables_are_read() {
    // Файл сохранён LibreOffice из nested.html.
    let doc = read("nested.doc");
    let Some(Block::Table(outer)) = doc.sections[0].blocks.get(1) else {
        panic!("нет таблицы");
    };
    assert_eq!(outer.rows.len(), 2);
    let cell = &outer.rows[0].cells[1];
    assert_eq!(cell.blocks.len(), 3);
    let Block::Table(inner) = &cell.blocks[1] else {
        panic!("нет вложенной таблицы");
    };
    assert_eq!(inner.rows.len(), 2);
    assert_eq!(inner.rows[1].cells[1].plain_text(), "y2");
    assert_eq!(outer.rows[1].cells[1].plain_text(), "B2");
    assert_eq!(block_texts(&doc.sections[0].blocks[2..]), ["After"]);
}

#[test]
fn sections_are_read() {
    use letter_model::{Orientation, SectionStart};
    let doc = read("sections.doc");
    assert_eq!(doc.sections.len(), 5);
    let props = |i: usize| &doc.sections[i].props;
    assert_eq!(props(1).columns.as_ref().map(|c| c.count), Some(2));
    assert!(props(0).columns.is_none());
    assert_eq!(
        props(3).page_size.map(|p| p.orientation),
        Some(Orientation::Landscape)
    );
    // «С нечётной страницы» LibreOffice в .doc не пишет.
    assert_eq!(props(1).start, Some(SectionStart::Continuous));
    assert_eq!(props(3).start, None);
    let page = props(0).page_size.unwrap();
    assert_eq!((page.width.0, page.height.0), (12240, 15840));
    assert_eq!(props(0).margins.unwrap().left.0, 1800);
}

#[test]
fn headers_and_footers_are_read() {
    use letter_model::HeaderFooterKind;
    let doc = read("header-footer.doc");
    let props = &doc.sections[0].props;
    let header = props.headers.get(HeaderFooterKind::Default).unwrap();
    assert_eq!(block_texts(&header.blocks), ["Header text"]);
    let footer = props.footers.get(HeaderFooterKind::Default).unwrap();
    assert_eq!(block_texts(&footer.blocks), ["Footer text"]);

    let doc = read("headers.doc");
    assert!(doc.settings.even_and_odd_headers);
    let props = &doc.sections[0].props;
    assert!(props.title_page);
    let text = |h: Option<&letter_model::HeaderFooter>| block_texts(&h.unwrap().blocks);
    assert_eq!(
        text(props.headers.get(HeaderFooterKind::First)),
        ["First page header"]
    );
    assert_eq!(
        text(props.headers.get(HeaderFooterKind::Even)),
        ["Even page header"]
    );
    assert_eq!(
        text(props.footers.get(HeaderFooterKind::Even)),
        ["Even page footer"]
    );
}

#[test]
fn paragraph_borders_and_shading_are_read() {
    let doc = read("borders.doc");
    let all = paragraphs(&doc);
    let framed = all[0].props.borders.as_ref().expect("рамка");
    assert!(framed.top.is_some() && framed.left.is_some());
    let underlined = all[2].props.borders.as_ref().expect("линия снизу");
    assert!(underlined.bottom.is_some() && underlined.top.is_none());
    assert!(all[3].props.shading.is_some());
    assert!(all[4].props.borders.is_none());
}
