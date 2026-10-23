//! Запись .docx: «открыть → сохранить → открыть» и документы, собранные
//! в коде.

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::PathBuf;

use letter_docx::{read_bytes, write_bytes};
use letter_model::{
    Alignment, Block, Border, Borders, BreakKind, CellMargins, CellProps, CellVerticalAlign, Color,
    Columns, DocDefaults, Document, FirstLineIndent, HalfPoints, HeaderFooter, HeightRule,
    Highlight, Hyperlink, Indentation, Inline, LineSpacing, PageMargins, PageSize, Paragraph,
    ParagraphBorders, ParagraphProps, RawXml, RowHeight, RowProps, Run, RunContent, RunFonts,
    RunProps, Section, SectionProps, SectionStart, Shading, Spacing, Style, StyleKind, Styles,
    TabLeader, TabStop, TabStopKind, Table, TableCell, TableLayout, TableProps, TableRow,
    TableWidth, ThemeColor, ThemeFont, Twips, Underline, VerticalAlign, VerticalMerge,
};

const FIXTURES: &[&str] = &[
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
    "shapes.docx",
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
            "word/numbering.xml",
            "docProps/core.xml",
            "docProps/app.xml",
        ];
        for (part, data) in &before {
            let copy = after
                .get(part)
                .unwrap_or_else(|| panic!("{name}: part {part} lost"));
            // Колонтитулы и сноски тоже строятся заново из модели.
            let story = [
                "word/header",
                "word/footer",
                "word/footnotes",
                "word/endnotes",
            ]
            .iter()
            .any(|prefix| part.starts_with(prefix));
            if !rebuilt.contains(&part.as_str()) && !story {
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
                .any(|c| matches!(c, RunContent::Picture(p) if p.image.is_some()))
        }),
        _ => false,
    });
    assert!(has_drawing);
}

#[test]
fn resized_shape_is_saved_without_old_fallback() {
    let mut doc = read_bytes(&fixture_bytes("shapes.docx")).unwrap();
    let mut first = true;
    doc.for_each_picture_mut(|picture| {
        if std::mem::take(&mut first) {
            picture.width *= 2;
        }
    });
    let saved = write_bytes(&doc).unwrap();
    let xml = String::from_utf8(zip_parts(&saved)["word/document.xml"].clone()).unwrap();
    // У изменённой фигуры нет запасного VML, у остальных остался.
    assert_eq!(xml.matches("<mc:AlternateContent").count(), 5);
    assert!(xml.contains(&format!("cx=\"{}\"", 360_000 * 10)));
    let reopened = read_bytes(&saved).unwrap();
    let mut widths = Vec::new();
    reopened.blocks().for_each(|b| {
        if let Block::Paragraph(p) = b {
            for run in p.runs() {
                for item in &run.content {
                    if let RunContent::Picture(picture) = item {
                        assert!(picture.shape.is_some());
                        widths.push(picture.width);
                    }
                }
            }
        }
    });
    assert_eq!(widths.len(), 6);
    assert_eq!(widths[0], 360_000 * 10);
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
        caps: Some(true),
        small_caps: Some(false),
        hidden: Some(true),
        underline_color: Some(Color::Rgb(0xFF, 0, 0)),
        spacing: Some(Twips(-15)),
        scale: Some(150),
        position: Some(-6),
        kerning: Some(HalfPoints(24)),
        link: None,
        // Порядок как в схеме: при записи элементы всё равно встанут так.
        unknown: vec![
            RawXml::new(
                r#"<w:emboss xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"/>"#,
            ),
            RawXml::new(
                r#"<w:lang xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" w:val="ru-RU"/>"#,
            ),
        ],
    };
    let paragraph_props = ParagraphProps {
        style: Some("Heading1".into()),
        numbering: Some(letter_model::NumberingRef {
            id: Some(3),
            level: Some(1),
        }),
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
        keep_next: Some(true),
        keep_lines: Some(false),
        page_break_before: Some(true),
        widow_control: Some(false),
        suppress_auto_hyphens: Some(true),
        tabs: vec![
            TabStop {
                position: Twips(720),
                kind: TabStopKind::Clear,
                leader: TabLeader::None,
            },
            TabStop {
                position: Twips(9350),
                kind: TabStopKind::Right,
                leader: TabLeader::MiddleDot,
            },
        ],
        borders: Some(ParagraphBorders {
            top: Some(Border::single(12)),
            bottom: Some(Border {
                style: "double".into(),
                size: Some(6),
                space: Some(1),
                color: Some(Color::Rgb(0x1F, 0x4E, 0x79)),
            }),
            between: Some(Border::single(4)),
            ..ParagraphBorders::default()
        }),
        shading: Some(Shading::fill(Color::Rgb(0xDD, 0xEE, 0xFF))),
        unknown: vec![RawXml::new(
            r#"<w:framePr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" w:wrap="around"/>"#,
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
            start: Some(SectionStart::Continuous),
            columns: Some(Columns {
                count: 2,
                space: Twips(567),
                separator: true,
                widths: Vec::new(),
            }),
            unknown: vec![RawXml::new(
                r#"<w:docGrid xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" w:linePitch="360"/>"#,
            )],
            ..SectionProps::default()
        },
        blocks: vec![
            Paragraph {
                props: paragraph_props.clone(),
                content: vec![Inline::Run(run)],
            }
            .into(),
            Table {
                props: Box::new(TableProps {
                    style: Some("TableGrid".into()),
                    width: Some(TableWidth::Percent(5000)),
                    alignment: Some(Alignment::Center),
                    indent: Some(TableWidth::Twips(Twips(108))),
                    borders: Some(Borders::all(Border::single(4))),
                    shading: Some(Shading::fill(Color::Rgb(0xEE, 0xEE, 0xEE))),
                    layout: Some(TableLayout::Fixed),
                    cell_margins: Some(CellMargins {
                        left: Some(TableWidth::Twips(Twips(108))),
                        right: Some(TableWidth::Twips(Twips(108))),
                        ..CellMargins::default()
                    }),
                    unknown: Vec::new(),
                }),
                grid: vec![Twips(2000), Twips(3000)],
                raw_props: Vec::new(),
                rows: vec![
                    TableRow {
                        raw_props: Vec::new(),
                        props: RowProps {
                            height: Some(RowHeight {
                                value: Twips(400),
                                rule: HeightRule::Exact,
                            }),
                            header: Some(true),
                            cant_split: Some(true),
                            unknown: Vec::new(),
                        },
                        cells: vec![
                            TableCell {
                                props: CellProps {
                                    width: Some(TableWidth::Twips(Twips(2000))),
                                    v_merge: Some(VerticalMerge::Restart),
                                    v_align: Some(CellVerticalAlign::Bottom),
                                    borders: Some(Borders {
                                        top: Some(Border {
                                            style: "nil".into(),
                                            size: None,
                                            space: None,
                                            color: None,
                                        }),
                                        ..Borders::default()
                                    }),
                                    ..CellProps::default()
                                },
                                ..TableCell::text("a")
                            },
                            TableCell::text("b"),
                        ],
                    },
                    TableRow {
                        cells: vec![
                            TableCell {
                                props: CellProps {
                                    v_merge: Some(VerticalMerge::Continue),
                                    ..CellProps::default()
                                },
                                blocks: vec![Paragraph::default().into()],
                                ..TableCell::default()
                            },
                            TableCell {
                                props: CellProps {
                                    grid_span: Some(1),
                                    margins: Some(CellMargins {
                                        top: Some(TableWidth::Nil),
                                        ..CellMargins::default()
                                    }),
                                    ..CellProps::default()
                                },
                                ..TableCell::text("c")
                            },
                        ],
                        ..TableRow::default()
                    },
                ],
            }
            .into(),
            Paragraph::text("последний абзац первого раздела").into(),
        ],
    };
    let second = Section {
        props: SectionProps {
            page_size: Some(PageSize::LETTER),
            margins: None,
            start: Some(SectionStart::OddPage),
            columns: Some(Columns {
                count: 2,
                space: Twips(0),
                separator: false,
                widths: vec![(Twips(3000), Twips(400)), (Twips(5000), Twips(0))],
            }),
            ..SectionProps::default()
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
    let mut grid = Style::new("Grid", StyleKind::Table);
    grid.table.borders = Some(Borders::all(Border::single(4)));
    grid.table.cell_margins = Some(CellMargins {
        left: Some(TableWidth::Twips(Twips(108))),
        ..CellMargins::default()
    });
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
        styles: vec![normal, heading, mine, grid],
        unknown: Vec::new(),
    };
    doc.settings.auto_hyphenation = true;
    doc.settings.hyphenation_zone = Twips(425);
    doc.settings.consecutive_hyphen_limit = 2;
    doc.settings.do_not_hyphenate_caps = true;
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
fn page_color_and_borders_survive() {
    use letter_model::{PageBorderDisplay, PageBorders};
    let check = |mut doc: Document| {
        doc.background = Some(Color::Rgb(0xFF, 0xF2, 0xCC));
        doc.sections[0].props.page_borders = Some(PageBorders {
            top: Some(Border {
                space: Some(24),
                ..Border::single(4)
            }),
            bottom: Some(Border {
                style: "double".into(),
                size: Some(6),
                space: Some(24),
                color: Some(Color::Rgb(0x1F, 0x4E, 0x79)),
            }),
            from_page_edge: true,
            display: PageBorderDisplay::NotFirstPage,
            behind_text: true,
            ..PageBorders::default()
        });
        let saved = write_bytes(&doc).unwrap();
        let reopened = read_bytes(&saved).unwrap();
        assert_eq!(reopened.background, doc.background);
        assert_eq!(reopened.sections, doc.sections);
        // В настройках — флаг, без которого Word не показывает цвет.
        let parts = zip_parts(&saved);
        let settings = parts
            .iter()
            .find(|(name, _)| name.ends_with("settings.xml"))
            .map(|(_, data)| String::from_utf8_lossy(data).into_owned())
            .unwrap_or_default();
        assert!(settings.contains("displayBackgroundShape"), "{settings}");
        assert_eq!(round_trip(&reopened), reopened);
    };
    // Новый документ (без части настроек) и файл с настройками.
    check(Document::new());
    check(read_bytes(&fixture_bytes("plain-text.docx")).unwrap());
}

#[test]
fn new_watermark_survives() {
    use letter_model::{Watermark, find_watermark};
    let mut doc = Document::new();
    let raw = Watermark::new("ОБРАЗЕЦ").to_raw(1);
    let header = Paragraph::from_runs([Run {
        props: RunProps::default(),
        content: vec![RunContent::Unknown(raw)],
    }]);
    doc.sections[0].props.headers.default = Some(HeaderFooter {
        blocks: vec![header.into()],
        relationship: None,
    });
    let reopened = round_trip(&doc);
    let header = reopened.sections[0].props.headers.default.clone();
    let mark = header.and_then(|h| find_watermark(&h.blocks));
    assert_eq!(mark.map(|m| m.text), Some("ОБРАЗЕЦ".to_string()));
    assert_eq!(round_trip(&reopened), reopened);
}

#[test]
fn special_hyphens_survive() {
    let mut doc = Document::new();
    doc.sections[0].blocks = vec![Paragraph::text("a\u{2011}b\u{ad}c").into()];
    let parts = zip_parts(&write_bytes(&doc).unwrap());
    let xml = String::from_utf8_lossy(&parts["word/document.xml"]).into_owned();
    assert!(xml.contains("<w:t>a</w:t><w:noBreakHyphen/><w:t>b</w:t><w:softHyphen/>"));
    let reopened = round_trip(&doc);
    let Some(Block::Paragraph(p)) = reopened.sections[0].blocks.first() else {
        panic!("нет абзаца");
    };
    let text: String = p
        .runs()
        .flat_map(|r| &r.content)
        .filter_map(|c| match c {
            RunContent::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "a\u{2011}b\u{ad}c");
    assert_eq!(round_trip(&reopened), reopened);
}

#[test]
fn symbols_from_fonts_survive() {
    let symbol = letter_model::Symbol {
        font: "Wingdings".into(),
        code: 0xF0E0,
    };
    let mut doc = Document::new();
    doc.sections[0].blocks = vec![
        Paragraph::from_runs([Run {
            props: RunProps::default(),
            content: vec![RunContent::Symbol(symbol.clone())],
        }])
        .into(),
    ];
    let parts = zip_parts(&write_bytes(&doc).unwrap());
    let xml = String::from_utf8_lossy(&parts["word/document.xml"]).into_owned();
    assert!(xml.contains(r#"<w:sym w:font="Wingdings" w:char="F0E0"/>"#));
    let reopened = round_trip(&doc);
    let Some(Block::Paragraph(p)) = reopened.sections[0].blocks.first() else {
        panic!("нет абзаца");
    };
    let content: Vec<_> = p.runs().flat_map(|r| r.content.clone()).collect();
    assert_eq!(content, [RunContent::Symbol(symbol)]);
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

#[test]
fn document_properties_are_read_and_written() {
    let mut doc = read_bytes(&fixture_bytes("headings.docx")).unwrap();
    let core = &doc.properties.core;
    assert_eq!(core.title.as_deref(), Some("Заголовки"));
    assert_eq!(core.creator.as_deref(), Some("letter"));
    assert_eq!(core.created.as_deref(), Some("2013-12-23T23:15:00Z"));
    assert_eq!(doc.properties.app.template.as_deref(), Some("Normal.dotm"));

    doc.properties.core.title = Some("Новое название".into());
    doc.properties.core.last_modified_by = Some("Никита".into());
    let stats = doc.statistics();
    doc.properties.app.words = Some(stats.words);
    let reopened = round_trip(&doc);
    assert_eq!(reopened.properties, doc.properties);
    assert_eq!(reopened.properties.app.words, Some(stats.words));
    assert!(stats.words > 0);
}

#[test]
fn properties_of_a_new_document_get_their_own_parts() {
    let mut doc = Document::new();
    doc.properties.core.creator = Some("Никита".into());
    doc.properties.app.application = Some("letter".into());
    let bytes = write_bytes(&doc).unwrap();
    let names: Vec<_> = zip_parts(&bytes).into_keys().collect();
    assert_eq!(
        names,
        [
            "[Content_Types].xml",
            "_rels/.rels",
            "docProps/app.xml",
            "docProps/core.xml",
            "word/document.xml"
        ]
    );
    let reopened = read_bytes(&bytes).unwrap();
    assert_eq!(reopened.properties, doc.properties);
    // Повторное сохранение ничего не добавляет.
    assert_eq!(
        write_bytes(&reopened).unwrap(),
        write_bytes(&round_trip(&reopened)).unwrap()
    );
}

#[test]
fn settings_are_read() {
    let doc = read_bytes(&fixture_bytes("headings.docx")).unwrap();
    assert_eq!(doc.settings.default_tab_stop, Twips(720));
    assert!(!doc.settings.add_paragraph_spacing);
    // Часть настроек не пересобирается, поэтому после сохранения те же.
    assert_eq!(round_trip(&doc).settings, doc.settings);
}

#[test]
fn resized_image_keeps_its_new_size() {
    let mut doc = read_bytes(&fixture_bytes("image.docx")).unwrap();
    let mut count = 0;
    doc.for_each_picture_mut(|picture| {
        picture.width /= 2;
        picture.height /= 2;
        count += 1;
    });
    assert_eq!(count, 1);
    let mut reopened = round_trip(&doc);
    let mut sizes = Vec::new();
    reopened.for_each_picture_mut(|picture| {
        sizes.push((picture.width, picture.height, picture.image.is_some()));
    });
    assert_eq!(sizes, [(914_400, 548_640, true)]);
}

#[test]
fn pictures_from_another_document_get_their_own_part() {
    let source = read_bytes(&fixture_bytes("image.docx")).unwrap();
    let mut pictures = Vec::new();
    let mut copy = source.clone();
    copy.for_each_picture_mut(|p| pictures.push(p.clone()));
    let [copied] = &pictures[..] else {
        panic!("{pictures:?}");
    };
    let image = copied.image.clone().unwrap();
    // Новый рисунок без исходной разметки и тот же рисунок, вставленный
    // из другого документа (связь rId9 в новом документе ведёт не туда).
    let fresh = letter_model::Picture {
        width: 127_000,
        height: 254_000,
        image: Some(image.clone()),
        relationship: None,
        raw: None,
        anchor: None,
        shape: None,
    };
    let mut doc = Document::new();
    doc.sections[0].blocks = vec![
        Paragraph::from_runs([letter_model::Run {
            props: Default::default(),
            content: vec![
                RunContent::Picture(copied.clone()),
                RunContent::Text("и".into()),
                RunContent::Picture(fresh),
            ],
        }])
        .into(),
    ];
    let bytes = write_bytes(&doc).unwrap();
    let parts = zip_parts(&bytes);
    // Одна картинка — одна часть, с типом содержимого.
    let media: Vec<_> = parts.keys().filter(|n| n.contains("media/")).collect();
    assert_eq!(media.len(), 1, "{media:?}");
    let types = String::from_utf8(parts["[Content_Types].xml"].clone()).unwrap();
    assert!(
        types.contains(r#"Extension="png" ContentType="image/png""#),
        "{types}"
    );

    let mut reopened = read_bytes(&bytes).unwrap();
    let mut found = Vec::new();
    reopened.for_each_picture_mut(|p| found.push(p.clone()));
    assert_eq!(found.len(), 2);
    for picture in &found {
        let data = picture.image.as_ref().expect("image");
        assert_eq!(data.bytes, image.bytes);
    }
    assert_eq!((found[1].width, found[1].height), (127_000, 254_000));
    // Повторная запись ничего не добавляет.
    let again = write_bytes(&reopened).unwrap();
    let parts = zip_parts(&again);
    assert_eq!(parts.keys().filter(|n| n.contains("media/")).count(), 1);
}

#[test]
fn new_headers_and_footers_get_their_parts() {
    let mut doc = Document::new();
    let story = |text: &str| HeaderFooter {
        blocks: vec![Paragraph::text(text).into()],
        relationship: None,
    };
    let props = &mut doc.sections[0].props;
    props.headers.default = Some(story("Top"));
    props.footers.first = Some(story("Bottom of the first page"));
    props.title_page = true;
    let saved = write_bytes(&doc).unwrap();
    let parts = zip_parts(&saved);
    assert!(parts.contains_key("word/header.xml"), "{:?}", parts.keys());
    assert!(parts.contains_key("word/footer.xml"));
    let types = String::from_utf8(parts["[Content_Types].xml"].clone()).unwrap();
    assert!(types.contains("wordprocessingml.header+xml"));
    let reopened = read_bytes(&saved).unwrap();
    let props = &reopened.sections[0].props;
    assert!(props.title_page);
    let text = |s: Option<&HeaderFooter>| s.map(|s| s.blocks[0].plain_text());
    assert_eq!(text(props.headers.default.as_ref()).as_deref(), Some("Top"));
    assert_eq!(
        text(props.footers.first.as_ref()).as_deref(),
        Some("Bottom of the first page")
    );
    // Сохранённый ещё раз документ не плодит новых частей.
    assert_eq!(
        write_bytes(&reopened).unwrap(),
        write_bytes(&round_trip(&reopened)).unwrap()
    );
}

#[test]
fn edited_header_is_saved() {
    let mut doc = read_bytes(&fixture_bytes("headers.docx")).unwrap();
    let header = doc.sections[0].props.headers.even.as_mut().unwrap();
    header.blocks = vec![Paragraph::text("Changed even header").into()];
    let reopened = round_trip(&doc);
    let header = reopened.sections[0].props.headers.even.as_ref().unwrap();
    assert_eq!(header.blocks[0].plain_text(), "Changed even header");
    assert_eq!(
        reopened.sections[0]
            .props
            .headers
            .first
            .as_ref()
            .unwrap()
            .blocks[0]
            .plain_text(),
        "First page header"
    );
}

#[test]
fn hyperlinks_get_their_relationships() {
    let mut doc = read_bytes(&fixture_bytes("links.docx")).unwrap();
    // Старый адрес меняется, новая ссылка добавляется в новый абзац.
    let Block::Paragraph(p) = &mut doc.sections[0].blocks[0] else {
        panic!("нет абзаца");
    };
    for inline in &mut p.content {
        if let Inline::Run(run) = inline
            && let Some(link) = &mut run.props.link
        {
            link.target = Some("https://example.org/".into());
        }
    }
    let mut run = Run::text("mail");
    run.props.link = Some(Hyperlink::to("mailto:a@b.c"));
    doc.sections[0]
        .blocks
        .push(Paragraph::from_runs([run, Run::text("!")]).into());
    let bytes = write_bytes(&doc).unwrap();
    let parts = zip_parts(&bytes);
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    assert!(rels.contains(r#"Target="https://example.org/" TargetMode="External""#));
    assert!(rels.contains(r#"Target="mailto:a@b.c" TargetMode="External""#));
    let reopened = read_bytes(&bytes).unwrap();
    let links: Vec<_> = reopened
        .sections
        .iter()
        .flat_map(|s| &s.blocks)
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p),
            _ => None,
        })
        .flat_map(|p| p.runs())
        .filter_map(|r| r.props.link.as_ref())
        .map(Hyperlink::address)
        .collect();
    assert_eq!(links, ["https://example.org/", "#target", "mailto:a@b.c"]);
    let last = reopened.sections[0].blocks.last().unwrap();
    assert_eq!(last.plain_text(), "mail!");
    // Второе сохранение не добавляет связей.
    assert_eq!(
        write_bytes(&reopened).unwrap(),
        write_bytes(&round_trip(&reopened)).unwrap()
    );
}

#[test]
fn notes_get_their_part() {
    let source = read_bytes(&fixture_bytes("notes.docx")).unwrap();
    // Новый документ со сносками из другого: части сносок появляются.
    let mut doc = Document::new();
    doc.sections = source.sections.clone();
    doc.styles = source.styles.clone();
    doc.footnotes = source.footnotes.clone();
    doc.endnotes = source.endnotes.clone();
    doc.footnotes[2].blocks = vec![Paragraph::text("Changed").into()];
    let bytes = write_bytes(&doc).unwrap();
    let parts = zip_parts(&bytes);
    assert!(parts.contains_key("word/footnotes.xml"));
    assert!(parts.contains_key("word/endnotes.xml"));
    let types = String::from_utf8(parts["[Content_Types].xml"].clone()).unwrap();
    assert!(types.contains("footnotes+xml"));
    let reopened = read_bytes(&bytes).unwrap();
    assert_eq!(reopened.footnotes, doc.footnotes);
    assert_eq!(reopened.endnotes, doc.endnotes);
    assert_eq!(reopened.sections, doc.sections);
    // Сноска без ссылки в файл не идёт.
    let mut orphan = doc.footnotes[2].clone();
    orphan.id = "99".into();
    doc.footnotes.push(orphan);
    let reopened = read_bytes(&write_bytes(&doc).unwrap()).unwrap();
    assert!(reopened.footnotes.iter().all(|n| n.id != "99"));
    assert_eq!(reopened.footnotes.len(), doc.footnotes.len() - 1);
}
