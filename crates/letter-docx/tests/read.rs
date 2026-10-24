//! Чтение настоящих файлов .docx из `tests/fixtures/docx` и битых файлов.

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Cursor, Write};
use std::path::PathBuf;

use letter_docx::{ReadError, read_bytes, read_file};
use letter_model::{
    Block, Color, Document, HalfPoints, HeaderFooter, Inline, NoteKind, OBJECT_CHAR, PageSize,
    Paragraph, RunContent, Underline, VerticalAlign,
};

fn fixture(name: &str) -> Document {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/docx")
        .join(name);
    read_file(&path).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn paragraphs(doc: &Document) -> Vec<&Paragraph> {
    doc.blocks()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p),
            _ => None,
        })
        .collect()
}

#[test]
fn empty() {
    let doc = fixture("empty.docx");
    assert_eq!(doc.sections.len(), 1);
    assert_eq!(doc.plain_text(), "");
    // python-docx создаёт документ размера Letter.
    assert_eq!(
        doc.sections[0].props.page_size.map(|s| s.width),
        Some(PageSize::LETTER.width)
    );
}

#[test]
fn plain_text() {
    let doc = fixture("plain-text.docx");
    let text = doc.plain_text();
    assert!(text.starts_with("Hello, world!\nThis is a simple paragraph"));
    assert!(text.ends_with("\nТретий абзац."));
}

#[test]
fn bold_italic() {
    let doc = fixture("bold-italic.docx");
    let p = paragraphs(&doc)[0];
    let find = |text: &str| {
        p.runs()
            .find(|r| r.plain_text() == text)
            .unwrap_or_else(|| panic!("no run {text:?}"))
    };
    assert_eq!(find("bold").props.bold, Some(true));
    assert_eq!(find("italic").props.italic, Some(true));
    let both = &find("bold italic").props;
    assert_eq!((both.bold, both.italic), (Some(true), Some(true)));
    assert_eq!(find("underline").props.underline, Some(Underline::Single));
    assert_eq!(find("strike").props.strike, Some(true));
    let red = &find("red 16pt").props;
    assert_eq!(red.color, Some(Color::Rgb(0xC0, 0, 0)));
    assert_eq!(red.size, Some(HalfPoints(32)));
    let indexes: Vec<_> = p.runs().filter_map(|r| r.props.vertical_align).collect();
    assert_eq!(
        indexes,
        [VerticalAlign::Subscript, VerticalAlign::Superscript]
    );
    assert_eq!(
        p.plain_text(),
        "Normal, bold, italic, bold italic, underline, strike, red 16pt, H2O, x2."
    );
}

#[test]
fn headings() {
    let doc = fixture("headings.docx");
    let styles: Vec<_> = paragraphs(&doc)
        .iter()
        .map(|p| p.props.style.as_deref().unwrap_or("-"))
        .collect();
    assert_eq!(
        styles,
        ["Title", "Heading1", "-", "Heading2", "-", "Heading3", "-"]
    );
}

#[test]
fn lists() {
    let doc = fixture("lists.docx");
    let ps = paragraphs(&doc);
    assert_eq!(ps[1].props.style.as_deref(), Some("ListBullet"));
    assert_eq!(ps[1].plain_text(), "Apples");
    assert_eq!(ps[5].props.style.as_deref(), Some("ListNumber"));
}

#[test]
fn table() {
    let doc = fixture("table.docx");
    let table = doc
        .blocks()
        .find_map(|b| match b {
            Block::Table(t) => Some(t),
            _ => None,
        })
        .expect("table");
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[0].cells.len(), 3);
    assert_eq!(table.rows[0].cells[1].plain_text(), "R1C2");
    // Объединённая ячейка: в строке две ячейки, первая на два столбца.
    assert_eq!(table.rows[2].cells.len(), 2);
    assert_eq!(table.rows[2].cells[0].span(), 2);
    assert_eq!(table.grid.len(), 3);
    assert!(table.raw_props.is_empty());
    assert_eq!(table.props.style.as_deref(), Some("TableGrid"));
    assert_eq!(table.props.width, Some(letter_model::TableWidth::Auto));
    assert!(
        table
            .rows
            .iter()
            .all(|r| r.cells.iter().all(|c| c.raw_props.is_empty()))
    );
}

#[test]
fn inline_image_is_read() {
    let doc = fixture("image.docx");
    let picture = paragraphs(&doc)
        .iter()
        .flat_map(|p| p.runs())
        .flat_map(|r| &r.content)
        .find_map(|c| match c {
            RunContent::Picture(picture) => Some(picture),
            _ => None,
        })
        .expect("picture");
    // 1828800 × 1097280 EMU = 144 × 86,4 пт.
    assert_eq!((picture.width, picture.height), (1_828_800, 1_097_280));
    assert_eq!(picture.relationship.as_deref(), Some("rId9"));
    let image = picture.image.as_ref().expect("image bytes");
    assert_eq!(image.name, "word/media/image1.png");
    assert!(image.bytes.starts_with(b"\x89PNG"));
    let raw = picture.raw.as_ref().expect("raw drawing");
    assert!(raw.as_str().contains("r:embed"));
    assert!(raw.as_str().contains("xmlns:r="));
    assert!(doc.plain_text().contains('\u{fffc}'));
}

#[test]
fn charts_are_read_from_their_cache() {
    use letter_model::{ChartKind, LegendPosition};
    let doc = fixture("charts.docx");
    let charts: Vec<_> = paragraphs(&doc)
        .iter()
        .flat_map(|p| p.runs())
        .flat_map(|r| &r.content)
        .filter_map(|c| match c {
            RunContent::Picture(picture) => Some(picture),
            _ => None,
        })
        .collect();
    assert_eq!(charts.len(), 3);
    // 5 × 3 дюйма.
    assert_eq!((charts[0].width, charts[0].height), (4_572_000, 2_743_200));
    let data = charts[0].image.as_ref().expect("chart data");
    assert_eq!(data.name, "word/charts/chart1.xml");
    let chart = data.chart.as_ref().expect("chart");
    assert_eq!(chart.kind, ChartKind::Column);
    assert_eq!(chart.title.as_deref(), Some("Продажи"));
    assert_eq!(chart.categories, ["Январь", "Февраль", "Март", "Апрель"]);
    assert_eq!(chart.series.len(), 2);
    assert_eq!(chart.series[1].name, "Казань");
    assert_eq!(chart.series[1].values[1], Some(4.4));
    assert_eq!(chart.legend, Some(LegendPosition::Bottom));
    assert_eq!((chart.gap_width, chart.overlap), (219, -27));
    // Цвета рядов — из темы документа (accent1 и accent2 темы Office 2013).
    assert_eq!(chart.series[0].color, Some((0x4F, 0x81, 0xBD)));
    assert_eq!(chart.series[1].color, Some((0xC0, 0x50, 0x4D)));
    let kinds: Vec<_> = charts
        .iter()
        .filter_map(|p| p.image.as_ref()?.chart.as_ref().map(|c| c.kind))
        .collect();
    assert_eq!(kinds, [ChartKind::Column, ChartKind::Pie, ChartKind::Line]);
    let line = charts[2].image.as_ref().unwrap().chart.as_ref().unwrap();
    assert_eq!(line.title, None, "заголовок удалён");
    assert_eq!(line.series[0].values[0], Some(-8.0));
}

#[test]
fn floating_pictures_are_read() {
    use letter_model::{AnchorAlign, HorizontalFrom, Placement, VerticalFrom, Wrap};
    let doc = fixture("floating.docx");
    let anchors: Vec<_> = paragraphs(&doc)
        .iter()
        .flat_map(|p| p.runs())
        .flat_map(|r| &r.content)
        .filter_map(|c| match c {
            RunContent::Picture(picture) => picture.anchor.as_deref(),
            _ => None,
        })
        .collect();
    let [square, behind] = anchors[..] else {
        panic!("{anchors:?}");
    };
    assert_eq!(square.wrap, Wrap::Square);
    assert_eq!(square.horizontal.relative, HorizontalFrom::Page);
    assert_eq!(square.horizontal.placement, Placement::Offset(4_572_000));
    assert_eq!(square.vertical.relative, VerticalFrom::Paragraph);
    assert_eq!(square.distance.left, 114_300);
    assert!(!square.behind_text);
    assert_eq!((behind.wrap, behind.behind_text), (Wrap::None, true));
    assert_eq!(
        behind.vertical.placement,
        Placement::Align(AnchorAlign::End)
    );
}

#[test]
fn headers_and_footers_are_read() {
    let doc = fixture("header-footer.docx");
    let props = &doc.sections[0].props;
    let text = |story: Option<&HeaderFooter>| {
        story
            .map(|s| s.blocks.iter().map(Block::plain_text).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    assert_eq!(text(props.headers.default.as_ref()), ["Header text"]);
    assert_eq!(text(props.footers.default.as_ref()), ["Footer text"]);

    let doc = fixture("headers.docx");
    assert!(doc.settings.even_and_odd_headers);
    let first = &doc.sections[0].props;
    assert!(first.title_page);
    assert_eq!(text(first.headers.first.as_ref()), ["First page header"]);
    assert_eq!(text(first.headers.even.as_ref()), ["Even page header"]);
    assert_eq!(
        text(first.footers.default.as_ref()),
        ["Odd page footer, page 1 of 1"]
    );
    // Второй раздел своего верхнего колонтитула не задаёт.
    let second = &doc.sections[1].props;
    assert!(second.headers.is_empty());
    assert_eq!(
        text(second.footers.even.as_ref()),
        ["Second section even footer"]
    );
}

#[test]
fn hyperlinks_and_bookmarks() {
    let doc = fixture("links.docx");
    let p = paragraphs(&doc);
    assert_eq!(p[0].plain_text(), "Site: example site.");
    let link = |p: &Paragraph| {
        p.runs()
            .find_map(|r| r.props.link.clone())
            .expect("нет ссылки")
    };
    let site = link(p[0]);
    assert_eq!(
        site.target.as_deref(),
        Some("https://example.com/page?a=1&b=2")
    );
    assert!(site.history);
    let inner = link(p[1]);
    assert_eq!(inner.target, None);
    assert_eq!(inner.anchor.as_deref(), Some("target"));
    assert_eq!(p[2].plain_text(), "This is the target paragraph.");
    assert!(matches!(&p[2].content[1], Inline::BookmarkStart(b) if b.name == "target"));
    assert!(matches!(&p[2].content[3], Inline::BookmarkEnd(id) if id == "0"));
}

#[test]
fn footnotes_and_endnotes() {
    let doc = fixture("notes.docx");
    let p = paragraphs(&doc);
    let references: Vec<_> = p
        .iter()
        .flat_map(|p| p.runs())
        .flat_map(|r| &r.content)
        .filter_map(|c| match c {
            RunContent::NoteReference(r) => Some((r.kind, r.id.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        references,
        [
            (NoteKind::Footnote, "1"),
            (NoteKind::Footnote, "2"),
            (NoteKind::Endnote, "1")
        ]
    );
    assert_eq!(doc.footnotes.len(), 4);
    assert_eq!(doc.footnotes[0].special.as_deref(), Some("separator"));
    let second = &doc.footnotes[3];
    assert!(second.is_regular());
    assert_eq!(second.id, "2");
    let text = second.blocks[0].plain_text();
    assert_eq!(text, format!("{OBJECT_CHAR} Second footnote."));
    assert_eq!(doc.endnotes.len(), 3);
}

#[test]
fn russian_and_english() {
    let doc = fixture("ru-en.docx");
    let text = doc.plain_text();
    assert!(text.contains("Съешь же ещё этих мягких французских булок"));
    assert!(text.contains("The quick brown fox"));
    assert!(text.contains("«кавычки», № 5, 10 €"));
    let mixed = paragraphs(&doc)[2];
    assert!(matches!(&mixed.content[1], Inline::Run(r) if r.props.bold == Some(true)));
}

/// Собирает ZIP-архив из пар «имя → содержимое».
fn zip(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in parts {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

const MINIMAL_DOCUMENT: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>hi</w:t></w:r></w:p></w:body></w:document>"#;

#[test]
fn minimal_package_without_rels_or_content_types() {
    let doc = read_bytes(&zip(&[("word/document.xml", MINIMAL_DOCUMENT)])).unwrap();
    assert_eq!(doc.plain_text(), "hi");
}

#[test]
fn main_part_found_through_relationships() {
    let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="/custom/main.xml"/></Relationships>"#;
    let doc = read_bytes(&zip(&[
        ("_rels/.rels", rels),
        ("custom/main.xml", MINIMAL_DOCUMENT),
    ]))
    .unwrap();
    assert_eq!(doc.plain_text(), "hi");
}

#[test]
fn broken_files_are_errors_not_crashes() {
    assert!(matches!(read_bytes(b""), Err(ReadError::Zip(_))));
    assert!(matches!(
        read_bytes(b"not a zip at all"),
        Err(ReadError::Zip(_))
    ));
    assert!(matches!(
        read_bytes(&zip(&[("other.txt", b"x")])),
        Err(ReadError::MissingPart(_))
    ));
    assert!(matches!(
        read_bytes(&zip(&[("word/document.xml", b"<w:document><unclosed>")])),
        Err(ReadError::Xml(_))
    ));
    let excel_types = br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/></Types>"#;
    assert!(matches!(
        read_bytes(&zip(&[
            ("[Content_Types].xml", excel_types),
            ("word/document.xml", MINIMAL_DOCUMENT),
        ])),
        Err(ReadError::NotAWordDocument(_))
    ));

    // Обрезанный настоящий файл.
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/docx/table.docx");
    let bytes = std::fs::read(path).unwrap();
    for len in [10, bytes.len() / 2, bytes.len() - 10] {
        assert!(read_bytes(&bytes[..len]).is_err(), "truncated to {len}");
    }
    assert!(matches!(
        read_file("/no/such/file.docx"),
        Err(ReadError::Io(_))
    ));
}

#[test]
fn styles_and_theme_resolve_like_word() {
    let doc = fixture("headings.docx");
    assert!(doc.styles.styles.len() > 50, "styles.xml was not read");
    // В шаблоне python-docx заголовки — Calibri, основной текст — Cambria.
    assert_eq!(doc.theme.major.latin.as_deref(), Some("Calibri"));
    assert_eq!(doc.theme.minor.latin.as_deref(), Some("Cambria"));

    let ps = paragraphs(&doc);
    let resolve = |p: &Paragraph| {
        let run = p.runs().next().expect("run");
        doc.resolve_run(&p.props, &run.props)
    };

    // «Heading 1»: шрифт заголовков темы, 14 пт, жирный, цвет 365F91;
    // интервал перед 24 пт.
    let heading = resolve(ps[1]);
    assert_eq!(heading.fonts.ascii, "Calibri");
    assert_eq!(heading.size, HalfPoints(28));
    assert!(heading.bold);
    assert_eq!(heading.color, Color::Rgb(0x36, 0x5F, 0x91));
    let heading_para = doc.resolve_paragraph(&ps[1].props);
    assert_eq!(heading_para.space_before, letter_model::Twips(480));

    // Обычный текст: шрифт текста темы, 11 пт, интервал после 10 пт,
    // множитель 1,15.
    let body = resolve(ps[2]);
    assert_eq!(body.fonts.ascii, "Cambria");
    assert_eq!(body.size, HalfPoints(22));
    assert!(!body.bold);
    let body_para = doc.resolve_paragraph(&ps[2].props);
    assert_eq!(body_para.style.as_deref(), Some("Normal"));
    assert_eq!(body_para.space_after, letter_model::Twips(200));
    assert_eq!(
        body_para.line_spacing,
        letter_model::LineSpacing::Multiple(276)
    );
}

#[test]
fn list_definitions() {
    let doc = fixture("lists.docx");
    assert!(!doc.numbering.abstracts.is_empty());
    assert!(!doc.numbering.instances.is_empty());
    let items = paragraphs(&doc);
    // «Apples» — маркированный список из стиля ListBullet.
    let apples = doc.resolve_paragraph(&items[1].props);
    let (id, level) = apples.numbering.expect("в списке");
    assert_eq!(level, 0);
    let lvl = doc.numbering.level(id, level).unwrap();
    assert_eq!(lvl.format, Some(letter_model::NumberFormat::Bullet));
    // Отступ — из уровня списка.
    assert!(apples.indent_left.0 > 0);
    // «First» — нумерованный.
    let first = doc.resolve_paragraph(&items[5].props);
    let (id, level) = first.numbering.expect("в списке");
    let lvl = doc.numbering.level(id, level).unwrap();
    assert_eq!(lvl.format, Some(letter_model::NumberFormat::Decimal));
    assert_eq!(doc.resolve_paragraph(&items[0].props).numbering, None);

    // Открыть → сохранить → открыть: списки те же.
    let again = read_bytes(&letter_docx::write_bytes(&doc).unwrap()).unwrap();
    assert_eq!(again.numbering, doc.numbering);
    assert_eq!(
        again.source.as_ref().and_then(|s| s.numbering_part.clone()),
        Some("word/numbering.xml".to_owned())
    );
}

#[test]
fn new_document_with_a_list_gets_a_numbering_part() {
    use letter_model::{
        AbstractNumbering, ListLevel, NumberFormat, NumberingInstance, NumberingRef,
    };
    let mut doc = Document::new();
    doc.numbering.abstracts.push(AbstractNumbering {
        id: 0,
        levels: vec![ListLevel {
            level: 0,
            format: Some(NumberFormat::Decimal),
            text: Some("%1.".into()),
            start: Some(1),
            ..Default::default()
        }],
        ..Default::default()
    });
    doc.numbering.instances.push(NumberingInstance {
        id: 1,
        abstract_id: 0,
        ..Default::default()
    });
    let mut item = Paragraph::text("пункт");
    item.props.numbering = Some(NumberingRef {
        id: Some(1),
        level: Some(0),
    });
    doc.sections[0].blocks = vec![item.into()];
    let again = read_bytes(&letter_docx::write_bytes(&doc).unwrap()).unwrap();
    assert_eq!(again.numbering, doc.numbering);
    assert_eq!(
        again
            .resolve_paragraph(&paragraphs(&again)[0].props)
            .numbering,
        Some((1, 0))
    );
}

#[test]
fn smart_art_is_read_with_its_drawing() {
    let doc = fixture("smartart.docx");
    let picture = paragraphs(&doc)
        .iter()
        .flat_map(|p| p.runs())
        .flat_map(|r| &r.content)
        .find_map(|c| match c {
            RunContent::Picture(picture) => Some(picture),
            _ => None,
        })
        .expect("smart art");
    assert_eq!((picture.width, picture.height), (5_486_400, 1_600_200));
    // Картинки нет: рисуется готовый вид.
    assert!(picture.image.is_none() && picture.relationship.is_none());
    assert!(picture.raw.as_ref().unwrap().as_str().contains("relIds"));
    let drawing = picture.drawing.as_ref().expect("drawing");
    assert_eq!((drawing.width, drawing.height), (5_486_400, 1_600_200));
    assert_eq!(drawing.shapes.len(), 5);
    let first = &drawing.shapes[0];
    // accent1 темы Office — 4F81BD.
    assert_eq!(first.fill, Some([0x4F, 0x81, 0xBD, 255]));
    let line = first.line.expect("line");
    assert_eq!((line.color, line.width), ([255, 255, 255, 255], 12_700));
    let text = first.text.as_ref().expect("text");
    assert_eq!(text.paragraphs[0].runs[0].text, "Идея");
    assert_eq!(text.paragraphs[0].runs[0].size, 2_000);
    // Цвет текста — из ссылки на шрифт стиля (lt1, белый).
    assert_eq!(text.paragraphs[0].runs[0].color, [255, 255, 255, 255]);
    // Область текста — рамка без внутренних полей (76200 EMU).
    assert_eq!((text.x, text.width), (76_200, 1_371_600 - 2 * 76_200));
    // Стрелка светлее блока (tint 60 %), без текста.
    let arrow = &drawing.shapes[1];
    let fill = arrow.fill.unwrap();
    assert!(
        fill[0] > 0x4F && fill[1] > 0x81 && fill[2] > 0xBD,
        "{fill:?}"
    );
    assert!(arrow.text.is_none());
    // Скруглённый прямоугольник — много точек на дугах углов.
    assert!(first.contours[0].points.len() > 8);
}

#[test]
fn comments_with_replies() {
    let doc = fixture("comments.docx");
    let Block::Paragraph(p) = &doc.sections[0].blocks[0] else {
        panic!("first block is not a paragraph");
    };
    assert_eq!(p.plain_text(), "The quick brown fox.");
    assert!(matches!(&p.content[1], Inline::CommentStart(id) if id == "0"));
    assert!(
        p.runs()
            .any(|r| r.content == [RunContent::CommentReference("1".into())])
    );
    assert_eq!(doc.comments.len(), 3);
    let first = &doc.comments[0];
    assert_eq!(first.author, "Anna Reviewer");
    assert_eq!(first.initials.as_deref(), Some("AR"));
    assert_eq!(first.date.as_deref(), Some("2026-10-23T10:00:00Z"));
    assert_eq!(first.plain_text(), "Which fox?");
    assert_eq!(first.parent, None);
    assert!(!first.done);
    assert_eq!(doc.comments[1].parent.as_deref(), Some("0"));
    assert!(doc.comments[2].done);
}
