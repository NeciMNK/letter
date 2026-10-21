//! Запись .docx: «открыть → сохранить → открыть» и документы, собранные
//! в коде.

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::PathBuf;

use letter_docx::{read_bytes, write_bytes};
use letter_model::{
    Alignment, Block, BreakKind, Color, DocDefaults, Document, FirstLineIndent, HalfPoints,
    Highlight, Indentation, Inline, LineSpacing, PageMargins, PageSize, Paragraph, ParagraphProps,
    RawXml, Run, RunContent, RunFonts, RunProps, Section, SectionProps, Spacing, Style, StyleKind,
    Styles, Table, TableCell, TableRow, ThemeColor, ThemeFont, Twips, Underline, VerticalAlign,
};

const FIXTURES: &[&str] = &[
    "empty.docx",
    "plain-text.docx",
    "bold-italic.docx",
    "headings.docx",
    "lists.docx",
    "table.docx",
    "image.docx",
    "header-footer.docx",
    "ru-en.docx",
];

fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/docx")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Все части архива: имя → байты.
fn zip_parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut parts = BTreeMap::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).unwrap();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        parts.insert(file.name().unwrap().to_string(), data);
    }
    parts
}

/// Сохраняет и снова открывает документ.
fn round_trip(doc: &Document) -> Document {
    read_bytes(&write_bytes(doc).unwrap()).unwrap()
}

#[test]
fn fixtures_survive_round_trip() {
    for name in FIXTURES {
        let original = read_bytes(&fixture_bytes(name)).unwrap();
        let saved = write_bytes(&original).unwrap();
        let reopened = read_bytes(&saved).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(original, reopened, "{name}: model changed after saving");
        // Повторное сохранение даёт тот же файл байт в байт.
        assert_eq!(saved, write_bytes(&reopened).unwrap(), "{name}");
    }
}

#[test]
fn untouched_parts_are_copied_byte_for_byte() {
    for name in FIXTURES {
        let before = zip_parts(&fixture_bytes(name));
        let after = zip_parts(&write_bytes(&read_bytes(&fixture_bytes(name)).unwrap()).unwrap());
        let rebuilt = [
            "[Content_Types].xml",
            "word/document.xml",
            "word/styles.xml",
        ];
        for (part, data) in &before {
            let copy = after
                .get(part)
                .unwrap_or_else(|| panic!("{name}: part {part} lost"));
            if !rebuilt.contains(&part.as_str()) {
                assert_eq!(data, copy, "{name}: part {part} changed");
            }
        }
        assert_eq!(
            before.keys().collect::<Vec<_>>(),
            after.keys().collect::<Vec<_>>(),
            "{name}"
        );
    }
}

#[test]
fn edited_document_keeps_images() {
    let mut doc = read_bytes(&fixture_bytes("image.docx")).unwrap();
    let images_before = doc
        .source
        .as_ref()
        .unwrap()
        .parts
        .iter()
        .filter(|p| p.name.starts_with("word/media/"))
        .count();
    assert!(images_before > 0);
    doc.sections[0]
        .blocks
        .insert(0, Paragraph::text("Новый первый абзац").into());

    let reopened = round_trip(&doc);
    assert!(reopened.plain_text().starts_with("Новый первый абзац\n"));
    let images_after = reopened
        .source
        .as_ref()
        .unwrap()
        .parts
        .iter()
        .filter(|p| p.name.starts_with("word/media/"))
        .count();
    assert_eq!(images_before, images_after);
    // Ссылка на картинку внутри абзаца сохранилась.
    let has_drawing = reopened.blocks().any(|b| match b {
        Block::Paragraph(p) => p.runs().any(|r| {
            r.content
                .iter()
                .any(|c| matches!(c, RunContent::Unknown(raw) if raw.local_name() == "drawing"))
        }),
        _ => false,
    });
    assert!(has_drawing);
}

#[test]
fn new_document_is_a_valid_package() {
    let doc = Document::new();
    let bytes = write_bytes(&doc).unwrap();
    let parts = zip_parts(&bytes);
    let names: Vec<_> = parts.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        ["[Content_Types].xml", "_rels/.rels", "word/document.xml"]
    );
    let reopened = read_bytes(&bytes).unwrap();
    assert_eq!(reopened.sections, doc.sections);
    assert_eq!(reopened.styles, doc.styles);
}

/// Документ, в котором задано всё, что понимает модель.
fn everything() -> Document {
    let run_props = RunProps {
        style: Some("Strong".into()),
        fonts: RunFonts {
            ascii: Some("Arial".into()),
            h_ansi: Some("Arial & Co".into()),
            east_asia: Some("SimSun".into()),
            complex_script: Some("Arial".into()),
            ascii_theme: Some(ThemeFont::MajorAscii),
            h_ansi_theme: Some(ThemeFont::MajorHAnsi),
            east_asia_theme: Some(ThemeFont::MinorEastAsia),
            complex_script_theme: Some(ThemeFont::MinorBidi),
            hint: Some("eastAsia".into()),
        },
        size: Some(HalfPoints(28)),
        bold: Some(true),
        italic: Some(false),
        underline: Some(Underline::DotDash),
        strike: Some(true),
        double_strike: Some(false),
        color: Some(Color::Rgb(0x36, 0x5F, 0x91)),
        theme_color: Some(ThemeColor {
            name: "accent1".into(),
            shade: Some(0xBF),
            tint: Some(0x0A),
        }),
        highlight: Some(Highlight::DarkYellow),
        vertical_align: Some(VerticalAlign::Superscript),
        // Порядок как в схеме: при записи элементы всё равно встанут так.
        unknown: vec![
            RawXml::new(
                r#"<w:caps xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"/>"#,
            ),
            RawXml::new(
                r#"<w:lang xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" w:val="ru-RU"/>"#,
            ),
        ],
    };
    let paragraph_props = ParagraphProps {
        style: Some("Heading1".into()),
        alignment: Some(Alignment::Justify),
        indentation: Indentation {
            left: Some(Twips(720)),
            right: Some(Twips(-360)),
            first_line: Some(FirstLineIndent::Hanging(Twips(360))),
        },
        spacing: Spacing {
            before: Some(Twips(240)),
            after: Some(Twips(0)),
            line: Some(LineSpacing::AtLeast(Twips(300))),
        },
        contextual_spacing: Some(true),
        unknown: vec![RawXml::new(
            r#"<w:keepNext xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"/>"#,
        )],
    };
    let run = Run {
        props: run_props.clone(),
        content: vec![
            RunContent::Text("  два пробела, <теги> & \"кавычки\" ".into()),
            RunContent::Tab,
            RunContent::Break(BreakKind::Line),
            RunContent::Break(BreakKind::Page),
            RunContent::Break(BreakKind::Column),
            RunContent::Text("конец".into()),
        ],
    };
    let first = Section {
        props: SectionProps {
            page_size: Some(PageSize::A4.rotated()),
            margins: Some(PageMargins::default()),
            unknown: vec![RawXml::new(
                r#"<w:cols xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" w:space="708"/>"#,
            )],
        },
        blocks: vec![
            Paragraph {
                props: paragraph_props.clone(),
                content: vec![Inline::Run(run)],
            }
            .into(),
            Table {
                raw_props: vec![RawXml::new(
                    r#"<w:tblPr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:tblW w:w="0" w:type="auto"/></w:tblPr>"#,
                )],
                rows: vec![TableRow {
                    raw_props: Vec::new(),
                    cells: vec![TableCell::text("a"), TableCell::text("b")],
                }],
            }
            .into(),
            Paragraph::text("последний абзац первого раздела").into(),
        ],
    };
    let second = Section {
        props: SectionProps {
            page_size: Some(PageSize::LETTER),
            margins: None,
            unknown: Vec::new(),
        },
        blocks: vec![Paragraph::text("второй раздел").into()],
    };
    let mut heading = Style::new("Heading1", StyleKind::Paragraph);
    heading.name = Some("heading 1".into());
    heading.based_on = Some("Normal".into());
    heading.next = Some("Normal".into());
    heading.paragraph = paragraph_props;
    heading.run = run_props.clone();
    let mut normal = Style::new("Normal", StyleKind::Paragraph);
    normal.is_default = true;
    let mut mine = Style::new("Mine", StyleKind::Character);
    mine.custom = true;
    let mut doc = Document::new();
    doc.sections = vec![first, second];
    doc.styles = Styles {
        defaults: DocDefaults {
            run: RunProps {
                size: Some(HalfPoints(22)),
                ..RunProps::default()
            },
            paragraph: ParagraphProps {
                spacing: Spacing {
                    after: Some(Twips(200)),
                    line: Some(LineSpacing::Multiple(276)),
                    ..Spacing::default()
                },
                ..ParagraphProps::default()
            },
        },
        styles: vec![normal, heading, mine],
        unknown: Vec::new(),
    };
    doc
}

#[test]
fn everything_the_model_knows_survives() {
    let doc = everything();
    let reopened = round_trip(&doc);
    assert_eq!(reopened.sections, doc.sections);
    assert_eq!(reopened.styles, doc.styles);
    // Второе сохранение уже ничего не меняет.
    assert_eq!(round_trip(&reopened), reopened);
}

#[test]
fn styles_are_added_to_a_file_without_them() {
    // empty.docx от python-docx содержит стили; убираем их из пакета,
    // как будто файл пришёл из программы, которая стилей не пишет.
    let mut doc = read_bytes(&fixture_bytes("plain-text.docx")).unwrap();
    let source = doc.source.as_mut().unwrap();
    source.styles_part = None;
    source.content_types.retain(|(p, _)| p != "word/styles.xml");
    let rels = source
        .parts
        .iter_mut()
        .find(|p| p.name == "word/_rels/document.xml.rels")
        .unwrap();
    let text = String::from_utf8(rels.data.clone()).unwrap();
    let without_styles: String = text
        .split("<Relationship ")
        .filter(|r| !r.contains("/styles\""))
        .collect::<Vec<_>>()
        .join("<Relationship ");
    rels.data = without_styles.into_bytes();
    let styles = doc.styles.clone();
    assert_ne!(styles, Styles::default());

    let reopened = round_trip(&doc);
    assert_eq!(reopened.styles, styles);
    assert_eq!(
        reopened.source.unwrap().styles_part.as_deref(),
        Some("word/styles.xml")
    );
}
