//! Запись .doc: «открыть → сохранить в .doc → открыть». Исходники —
//! тестовые .docx и .doc; после записи документ читается нашим же
//! чтением .doc и сравнивается с исходным. Те же файлы проверены в
//! LibreOffice (текст, оформление, колонтитулы, сноски, списки,
//! картинки совпадают), но в тестах LibreOffice не нужен.

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use letter_model::{
    Block, CellProps, Document, Inline, Paragraph, Run, RunContent, Table, TableCell, TableRow,
    VerticalMerge,
};

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(path)
}

fn docx(name: &str) -> Document {
    letter_docx::read_file(fixture(&format!("docx/{name}.docx"))).unwrap()
}

/// Записывает в .doc и читает обратно.
fn round_trip(doc: &Document) -> Document {
    let bytes = letter_doc::write(doc).unwrap();
    assert!(letter_doc::is_doc(&bytes));
    letter_doc::read(&bytes).unwrap()
}

/// Непустые строки текста. Знак объекта (U+FFFC) не считается: фигуры,
/// диаграммы и формулы в .doc пока не пишутся.
fn lines(doc: &Document) -> Vec<String> {
    doc.plain_text()
        .replace('\u{fffc}', "")
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

fn paragraphs<'a>(blocks: &'a [Block], out: &mut Vec<&'a Paragraph>) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => out.push(p),
            Block::Table(t) => {
                for cell in t.rows.iter().flat_map(|r| &r.cells) {
                    paragraphs(&cell.blocks, out);
                }
            }
            _ => {}
        }
    }
}

fn all_paragraphs(doc: &Document) -> Vec<&Paragraph> {
    let mut all = Vec::new();
    for section in &doc.sections {
        paragraphs(&section.blocks, &mut all);
    }
    all
}

/// Итоговое оформление (с учётом стилей) каждого абзаца и каждого
/// куска текста с одинаковым оформлением, разделы. Шрифты для
/// восточноазиатского и сложного письма не сравниваются: в .doc у
/// документа их нет по отдельности.
fn profile(doc: &Document) -> Vec<String> {
    let mut out = Vec::new();
    for p in all_paragraphs(doc) {
        let text = p.plain_text();
        if text.trim().is_empty() {
            continue;
        }
        let mut props = doc.resolve_paragraph(&p.props);
        props.style = None;
        props.numbering = None;
        // В .doc у линий границ нет ссылок на цвета темы, только сам цвет.
        let b = &mut props.borders;
        for side in [
            &mut b.top,
            &mut b.left,
            &mut b.bottom,
            &mut b.right,
            &mut b.between,
            &mut b.bar,
        ]
        .into_iter()
        .flatten()
        {
            side.theme_color = None;
        }
        out.push(format!("{text:?} {props:?}"));
        let mut pieces: Vec<(String, String)> = Vec::new();
        for inline in &p.content {
            let Inline::Run(run) = inline else { continue };
            let text = run.plain_text().replace('\u{fffc}', "");
            if text.is_empty() {
                continue;
            }
            let mut props = doc.resolve_run(&p.props, &run.props);
            props.fonts.east_asia.clear();
            props.fonts.complex_script.clear();
            if let Some(link) = &mut props.link {
                link.relationship = None;
            }
            let props = format!("{props:?}");
            match pieces.last_mut() {
                Some((last, last_props)) if *last_props == props => last.push_str(&text),
                _ => pieces.push((text, props)),
            }
        }
        out.extend(pieces.into_iter().map(|(t, p)| format!("  {t:?} {p}")));
    }
    for section in &doc.sections {
        let p = &section.props;
        out.push(format!(
            "{:?} {:?} {} {:?} {}",
            p.page_size,
            p.margins,
            p.columns.as_ref().map_or(1, |c| c.count),
            p.start.unwrap_or_default(),
            p.title_page
        ));
    }
    out
}

/// Сноски, примечания и колонтитулы текстом.
fn stories(doc: &Document) -> Vec<String> {
    let text = |blocks: &[Block]| -> String {
        blocks
            .iter()
            .map(Block::plain_text)
            .collect::<Vec<_>>()
            .join("|")
    };
    let mut out = Vec::new();
    for note in doc.footnotes.iter().filter(|n| n.is_regular()) {
        out.push(format!("сноска {}", text(&note.blocks)));
    }
    for note in doc.endnotes.iter().filter(|n| n.is_regular()) {
        out.push(format!("концевая {}", text(&note.blocks)));
    }
    for c in &doc.comments {
        out.push(format!(
            "примечание {} {:?} {}",
            c.author,
            c.initials,
            text(&c.blocks)
        ));
    }
    for (i, section) in doc.sections.iter().enumerate() {
        let (h, f) = (&section.props.headers, &section.props.footers);
        for (name, part) in [
            ("верхний", &h.default),
            ("верхний первой", &h.first),
            ("верхний чётных", &h.even),
            ("нижний", &f.default),
            ("нижний первой", &f.first),
            ("нижний чётных", &f.even),
        ] {
            if let Some(part) = part {
                out.push(format!("{i} {name} {}", text(&part.blocks)));
            }
        }
    }
    out.push(format!("чётные {}", doc.settings.even_and_odd_headers));
    out
}

const ALL: &[&str] = &[
    "bold-italic",
    "borders",
    "cells",
    "charts",
    "comments",
    "empty",
    "floating",
    "header-footer",
    "headers",
    "headings",
    "image",
    "links",
    "lists",
    "math",
    "notes",
    "pages",
    "plain-text",
    "ru-en",
    "sections",
    "shapes",
    "smartart",
    "table",
    "watermark",
];

#[test]
fn text_survives_for_all_test_documents() {
    for name in ALL {
        let doc = docx(name);
        assert_eq!(lines(&round_trip(&doc)), lines(&doc), "{name}");
    }
}

#[test]
fn old_documents_survive_saving_again() {
    let dir = fixture("doc");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if path.extension().is_none_or(|e| e != "doc") || name.starts_with("encrypted") {
            continue;
        }
        let doc = letter_doc::read_file(&path).unwrap();
        let back = round_trip(&doc);
        assert_eq!(lines(&back), lines(&doc), "{name}");
        assert_eq!(stories(&back), stories(&doc), "{name}");
    }
}

#[test]
fn formatting_styles_and_sections() {
    for name in [
        "bold-italic",
        "borders",
        "cells",
        "header-footer",
        "headers",
        "headings",
        "links",
        "lists",
        "notes",
        "pages",
        "plain-text",
        "ru-en",
        "sections",
        "table",
    ] {
        let doc = docx(name);
        assert_eq!(profile(&round_trip(&doc)), profile(&doc), "{name}");
    }
}

#[test]
fn headers_notes_and_comments() {
    for name in ["comments", "header-footer", "headers", "notes", "sections"] {
        let doc = docx(name);
        assert_eq!(stories(&round_trip(&doc)), stories(&doc), "{name}");
    }
    // Кусок текста, к которому относится примечание, тоже отмечен.
    let back = round_trip(&docx("comments"));
    let marked = all_paragraphs(&back).iter().any(|p| {
        p.content
            .iter()
            .any(|i| matches!(i, Inline::CommentStart(_)))
    });
    assert!(marked);
}

#[test]
fn lists() {
    let numbers = |doc: &Document| -> Vec<(Option<String>, Option<String>)> {
        all_paragraphs(doc)
            .iter()
            .filter_map(|p| doc.resolve_paragraph(&p.props).numbering)
            .filter_map(|(id, level)| doc.numbering.level(id, level))
            .map(|l| (l.text, l.format.map(|f| format!("{f:?}"))))
            .collect()
    };
    let doc = docx("lists");
    let expected = numbers(&doc);
    assert!(expected.len() >= 7);
    assert_eq!(numbers(&round_trip(&doc)), expected);
}

#[test]
fn pictures_keep_bytes_and_size() {
    let pictures = |doc: &Document| -> Vec<(i64, i64, Vec<u8>)> {
        all_paragraphs(doc)
            .iter()
            .flat_map(|p| &p.content)
            .filter_map(|i| match i {
                Inline::Run(r) => Some(&r.content),
                _ => None,
            })
            .flatten()
            .filter_map(|c| match c {
                RunContent::Picture(p) => Some((
                    p.width / 635,
                    p.height / 635,
                    p.image.as_ref()?.bytes.clone(),
                )),
                _ => None,
            })
            .collect()
    };
    for name in ["image", "floating"] {
        let doc = docx(name);
        let expected = pictures(&doc);
        assert!(!expected.is_empty(), "{name}");
        // Плавающие картинки пока записываются в строку.
        assert_eq!(pictures(&round_trip(&doc)), expected, "{name}");
    }
}

#[test]
fn document_properties() {
    let doc = docx("plain-text");
    let back = round_trip(&doc);
    assert!(doc.properties.core.title.is_some());
    assert_eq!(back.properties.core.title, doc.properties.core.title);
    assert_eq!(back.properties.core.creator, doc.properties.core.creator);
    assert_eq!(back.properties.core.created, doc.properties.core.created);
}

#[test]
fn vertically_merged_cells() {
    let cell = |text: &str, v_merge| TableCell {
        props: CellProps {
            v_merge,
            ..CellProps::default()
        },
        raw_props: Vec::new(),
        blocks: vec![Block::Paragraph(Paragraph {
            content: vec![Inline::Run(Run::text(text))],
            ..Paragraph::default()
        })],
    };
    let row = |cells| TableRow {
        cells,
        ..TableRow::default()
    };
    let mut doc = Document::new();
    doc.sections[0].blocks = vec![Block::Table(Table {
        rows: vec![
            row(vec![
                cell("A", Some(VerticalMerge::Restart)),
                cell("B", None),
            ]),
            row(vec![
                cell("", Some(VerticalMerge::Continue)),
                cell("C", None),
            ]),
        ],
        ..Table::default()
    })];
    let back = round_trip(&doc);
    let Some(Block::Table(table)) = back.sections[0].blocks.first() else {
        panic!("нет таблицы");
    };
    let merges: Vec<_> = table
        .rows
        .iter()
        .map(|r| r.cells[0].props.v_merge)
        .collect();
    assert_eq!(
        merges,
        [Some(VerticalMerge::Restart), Some(VerticalMerge::Continue)]
    );
}

#[test]
fn template_and_files() {
    let doc = docx("plain-text");
    let dot = letter_doc::write_template(&doc).unwrap();
    assert_eq!(lines(&letter_doc::read(&dot).unwrap()), lines(&doc));
    let path = std::env::temp_dir().join(format!("letter-doc-write-{}.doc", std::process::id()));
    letter_doc::write_file(&doc, &path).unwrap();
    assert_eq!(lines(&letter_doc::read_file(&path).unwrap()), lines(&doc));
    let _ = std::fs::remove_file(&path);
    // Пустой документ тоже записывается.
    assert!(lines(&round_trip(&Document::new())).is_empty());
}
