//! Чтение и запись ODT: файлы из `tests/fixtures`, переход
//! .docx → ODT → модель и небольшие примеры, написанные вручную.

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::path::{Path, PathBuf};

use letter_model::{
    Alignment, Block, BreakKind, Color, Document, FirstLineIndent, HalfPoints, Highlight, Inline,
    LineSpacing, Orientation, Paragraph, ParagraphProps, Run, RunContent, RunProps, Style,
    StyleKind, TabLeader, TabStop, TabStopKind, Twips, Underline, VerticalAlign,
};

fn fixtures(kind: &str) -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(kind);
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == kind))
        .collect();
    files.sort();
    files
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

fn texts(doc: &Document) -> Vec<String> {
    paragraphs(doc).iter().map(|p| p.plain_text()).collect()
}

/// Текст без разрывов страниц: в ODF разрыв — свойство следующего абзаца.
fn texts_without_page_breaks(doc: &Document) -> Vec<String> {
    texts(doc).iter().map(|t| t.replace('\u{c}', "")).collect()
}

fn round_trip(doc: &Document) -> Document {
    letter_odt::read(&letter_odt::write(doc).unwrap()).unwrap()
}

/// Плоский ODT (.fodt) из кусков: стили, автоматические стили,
/// шаблоны страниц, текст.
fn fodt(styles: &str, auto: &str, masters: &str, body: &str) -> Document {
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
 xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0"
 xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
 xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"
 xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"
 xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"
 xmlns:dc="http://purl.org/dc/elements/1.1/"
 xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0"
 xmlns:config="urn:oasis:names:tc:opendocument:xmlns:config:1.0"
 office:version="1.3" office:mimetype="application/vnd.oasis.opendocument.text">
<office:meta><dc:title>Отчёт</dc:title><meta:initial-creator>Аня</meta:initial-creator>
<meta:keyword>a</meta:keyword><meta:keyword>b</meta:keyword>
<meta:editing-duration>PT1H5M</meta:editing-duration></office:meta>
<office:settings><config:config-item-set config:name="ooo:configuration-settings">
<config:config-item config:name="AddParaTableSpacing" config:type="boolean">false</config:config-item>
</config:config-item-set></office:settings>
<office:font-face-decls><style:font-face style:name="Sans1" svg:font-family="'DejaVu Sans'"/></office:font-face-decls>
<office:styles>{styles}</office:styles>
<office:automatic-styles>{auto}</office:automatic-styles>
<office:master-styles>{masters}</office:master-styles>
<office:body><office:text>{body}</office:text></office:body>
</office:document>"#
    );
    letter_odt::read(xml.as_bytes()).unwrap()
}

#[test]
fn saved_odt_reads_back_the_same() {
    for path in fixtures("docx") {
        let original = letter_docx::read_file(&path).unwrap();
        let first = round_trip(&original);
        let second = round_trip(&first);
        assert_eq!(first, second, "{}", path.display());
    }
}

#[test]
fn docx_survives_conversion_to_odt() {
    for path in fixtures("docx") {
        let docx = letter_docx::read_file(&path).unwrap();
        let back = round_trip(&docx);
        let name = path.display();
        let mut expected = texts_without_page_breaks(&docx);
        if expected.is_empty() {
            expected.push(String::new());
        }
        assert_eq!(expected, texts_without_page_breaks(&back), "{name}");
        assert_eq!(docx.sections.len(), back.sections.len(), "{name}");
        for (a, b) in docx.sections.iter().zip(&back.sections) {
            assert_eq!(
                a.props.page_size.unwrap_or_default(),
                b.props.page_size.unwrap_or_default(),
                "{name}"
            );
            let (m, n) = (
                a.props.margins.unwrap_or_default(),
                b.props.margins.unwrap_or_default(),
            );
            assert_eq!(
                (m.top, m.right, m.bottom, m.left, m.gutter),
                (n.top, n.right, n.bottom, n.left, n.gutter),
                "{name}"
            );
        }
        assert_eq!(
            docx.settings.add_paragraph_spacing, back.settings.add_paragraph_spacing,
            "{name}"
        );

        // Итоговое оформление абзацев и фрагментов то же.
        let docx_paragraphs = paragraphs(&docx);
        for (i, (p, q)) in docx_paragraphs.iter().zip(paragraphs(&back)).enumerate() {
            let (a, b) = (
                docx.resolve_paragraph(&p.props),
                back.resolve_paragraph(&q.props),
            );
            let first = |f: Option<FirstLineIndent>| match f {
                Some(FirstLineIndent::FirstLine(Twips(0))) => None,
                f => f,
            };
            // Абзац после разрыва страницы в конце предыдущего начинается
            // с новой страницы.
            let after_break = i > 0 && docx_paragraphs[i - 1].plain_text().ends_with('\u{c}');
            assert_eq!(a.alignment, b.alignment, "{name}");
            assert_eq!(
                (a.indent_left, a.indent_right, first(a.first_line)),
                (b.indent_left, b.indent_right, first(b.first_line)),
                "{name}"
            );
            // Вверху страницы после разрыва Word не добавляет интервал
            // перед абзацем: в ODT это записано явно.
            let before = if after_break {
                Twips::ZERO
            } else {
                a.space_before
            };
            assert_eq!(
                (before, a.space_after, a.line_spacing),
                (b.space_before, b.space_after, b.line_spacing),
                "{name}"
            );
            assert_eq!(
                (
                    a.keep_next,
                    a.keep_lines,
                    a.page_break_before || after_break,
                    a.widow_control,
                    a.contextual_spacing
                ),
                (
                    b.keep_next,
                    b.keep_lines,
                    b.page_break_before,
                    b.widow_control,
                    b.contextual_spacing
                ),
                "{name}"
            );
            let tabs = |t: &[TabStop]| -> Vec<TabStop> {
                t.iter()
                    .copied()
                    .filter(|t| t.kind != TabStopKind::Bar)
                    .collect()
            };
            assert_eq!(tabs(&a.tabs), tabs(&b.tabs), "{name}");
            assert_eq!(a.style, b.style, "{name}");

            for (r, s) in p.runs().zip(q.runs()) {
                let (x, y) = (
                    docx.resolve_run(&p.props, &r.props),
                    back.resolve_run(&q.props, &s.props),
                );
                assert_eq!(
                    (&x.fonts.ascii, &x.fonts.east_asia, &x.fonts.complex_script),
                    (&y.fonts.ascii, &y.fonts.east_asia, &y.fonts.complex_script),
                    "{name}"
                );
                assert_eq!(
                    (
                        x.size,
                        x.bold,
                        x.italic,
                        x.underline,
                        x.strike,
                        x.double_strike
                    ),
                    (
                        y.size,
                        y.bold,
                        y.italic,
                        y.underline,
                        y.strike,
                        y.double_strike
                    ),
                    "{name}"
                );
                assert_eq!(
                    (x.color, x.highlight, x.vertical_align),
                    (y.color, y.highlight, y.vertical_align),
                    "{name}"
                );
            }
        }
    }
}

#[test]
fn libreoffice_odt_has_the_docx_text() {
    let odt_files = fixtures("odt");
    assert!(odt_files.len() >= 5);
    for path in odt_files {
        let odt = letter_odt::read_file(&path).unwrap();
        let docx_path = path
            .parent()
            .unwrap()
            .join("../docx")
            .join(path.with_extension("docx").file_name().unwrap());
        let docx = letter_docx::read_file(docx_path).unwrap();
        assert_eq!(
            texts_without_page_breaks(&docx),
            texts_without_page_breaks(&odt),
            "{}",
            path.display()
        );
        // И после сохранения в ODT текст тот же.
        assert_eq!(texts(&odt), texts(&round_trip(&odt)), "{}", path.display());
    }
}

#[test]
fn libreoffice_formatting_is_read() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/odt");
    let doc = letter_odt::read_file(dir.join("bold-italic.odt")).unwrap();
    let p = paragraphs(&doc)[0];
    let find = |text: &str| {
        p.runs()
            .find(|r| r.plain_text().contains(text))
            .map(|r| doc.resolve_run(&p.props, &r.props))
            .unwrap()
    };
    assert!(find("bold").bold);
    assert!(!find("Normal").bold);
    assert!(find("italic").italic);
    assert_eq!(find("underline").underline, Underline::Single);
    assert!(find("strike").strike);
    assert_eq!(find("red").color, Color::Rgb(0xC0, 0x00, 0x00));
    assert_eq!(find("red").size, HalfPoints(32));
    assert_eq!(find("Normal").size, HalfPoints(22));
    assert_eq!(find("Normal").fonts.ascii, "Cambria");
    assert_eq!(
        p.plain_text(),
        "Normal, bold, italic, bold italic, underline, strike, red 16pt, H2O, x2."
    );
    // Как в Word: интервалы между абзацами не складываются.
    assert!(!doc.settings.add_paragraph_spacing);
    assert_eq!(doc.settings.default_tab_stop, Twips(720));

    let headings = letter_odt::read_file(dir.join("headings.odt")).unwrap();
    let heading = headings.styles.get("Heading1").unwrap();
    assert_eq!(heading.name.as_deref(), Some("Heading 1"));
    assert_eq!(heading.based_on.as_deref(), Some("Normal"));
    assert!(headings.styles.get("Normal").unwrap().is_default);
    let first = paragraphs(&headings)[1];
    assert_eq!(first.props.style.as_deref(), Some("Heading1"));
    assert!(
        headings
            .resolve_run(&first.props, &RunProps::default())
            .bold
    );

    let pages = letter_odt::read_file(dir.join("pages.odt")).unwrap();
    let size = pages.sections[0].props.page_size.unwrap();
    assert_eq!((size.width, size.height), (Twips(12240), Twips(15840)));
    let chapter = paragraphs(&pages)
        .into_iter()
        .find(|p| p.plain_text() == "Chapter 2")
        .unwrap();
    assert!(pages.resolve_paragraph(&chapter.props).page_break_before);

    let header = letter_odt::read_file(dir.join("header-footer.odt")).unwrap();
    let margins = header.sections[0].props.margins.unwrap();
    assert_eq!((margins.top, margins.header), (Twips(1440), Twips(720)));
    assert_eq!((margins.bottom, margins.footer), (Twips(1440), Twips(720)));

    let table = letter_odt::read_file(dir.join("table.odt")).unwrap();
    let Block::Table(t) = &table.sections[0].blocks[1] else {
        panic!("нет таблицы");
    };
    assert_eq!(t.rows.len(), 3);
    // Объединённая ячейка — одна, как в .docx, и занимает два столбца.
    assert_eq!(t.rows[2].cells.len(), 2);
    let span = |t: &letter_model::Table| t.rows[2].cells[0].span();
    assert_eq!(span(t), 2);
    let again = round_trip(&table);
    let Block::Table(t2) = &again.sections[0].blocks[1] else {
        panic!("нет таблицы");
    };
    assert_eq!(span(t), span(t2));
}

#[test]
fn handwritten_flat_odt() {
    let doc = fodt(
        r##"<style:default-style style:family="paragraph">
             <style:paragraph-properties style:tab-stop-distance="1cm"/>
             <style:text-properties style:font-name="Sans1" fo:font-size="10pt"/>
           </style:default-style>
           <style:style style:name="Standard" style:family="paragraph"/>
           <style:style style:name="Heading_20_1" style:display-name="Heading 1"
               style:family="paragraph" style:parent-style-name="Standard">
             <style:paragraph-properties fo:margin-left="1in">
               <style:tab-stops><style:tab-stop style:position="1in"/></style:tab-stops>
             </style:paragraph-properties>
             <style:text-properties fo:font-size="200%" fo:font-weight="bold"/>
           </style:style>
           <style:style style:name="Strong_20_Emphasis" style:family="text">
             <style:text-properties fo:font-weight="bold"/>
           </style:style>"##,
        r##"<style:page-layout style:name="pm1">
             <style:page-layout-properties fo:page-width="21cm" fo:page-height="29.7cm"
               fo:margin-top="2cm" fo:margin-bottom="2cm" fo:margin-left="3cm" fo:margin-right="1.5cm"/>
           </style:page-layout>
           <style:page-layout style:name="pm2">
             <style:page-layout-properties fo:page-width="29.7cm" fo:page-height="21cm"/>
           </style:page-layout>
           <style:style style:name="P1" style:family="paragraph" style:parent-style-name="Standard">
             <style:paragraph-properties fo:text-align="center" fo:margin-left="0.5in" fo:line-height="150%">
               <style:tab-stops><style:tab-stop style:position="1in" style:type="right" style:leader-text="."/></style:tab-stops>
             </style:paragraph-properties>
             <style:text-properties fo:color="#ff0000"/>
           </style:style>
           <style:style style:name="P2" style:family="paragraph" style:master-page-name="Landscape"/>
           <style:style style:name="T1" style:family="text">
             <style:text-properties fo:font-style="italic" fo:font-size="50%"/>
           </style:style>
           <style:style style:name="T2" style:family="text" style:parent-style-name="Strong_20_Emphasis">
             <style:text-properties style:text-underline-style="solid"/>
           </style:style>"##,
        r##"<style:master-page style:name="Standard" style:page-layout-name="pm1"/>
           <style:master-page style:name="Landscape" style:page-layout-name="pm2"/>"##,
        r##"<text:sequence-decls><text:sequence-decl text:name="x"/></text:sequence-decls>
           <text:h text:style-name="Heading_20_1" text:outline-level="1">Заголовок <text:span text:style-name="Strong_20_Emphasis">важно</text:span></text:h>
           <text:p text:style-name="P1">  a  <text:s text:c="2"/>b<text:tab/>c<text:line-break/>d
             <text:span text:style-name="T1">e<text:span text:style-name="T2">f</text:span></text:span><text:note><text:note-body><text:p>сноска</text:p></text:note-body></text:note><text:a>ссылка</text:a> <text:page-number>3</text:page-number></text:p>
           <text:list><text:list-item><text:p>пункт</text:p><text:list><text:list-item><text:p>вложенный</text:p></text:list-item></text:list></text:list-item></text:list>
           <table:table><table:table-column table:number-columns-repeated="3"/>
             <table:table-header-rows><table:table-row><table:table-cell table:number-columns-spanned="2"><text:p>A</text:p></table:table-cell><table:covered-table-cell/><table:table-cell/></table:table-row></table:table-header-rows>
             <table:table-row table:number-rows-repeated="2"><table:table-cell table:number-columns-repeated="3"><text:p>x</text:p></table:table-cell></table:table-row>
           </table:table>
           <text:section><text:p text:style-name="P2">альбом</text:p></text:section>"##,
    );

    assert_eq!(
        texts(&doc),
        vec![
            "Заголовок важно",
            "a   b\tc\nd efссылка 3",
            "пункт",
            "вложенный",
            "A",
            "",
            "x",
            "x",
            "x",
            "x",
            "x",
            "x",
            "альбом",
        ]
    );
    assert_eq!(doc.properties.core.title.as_deref(), Some("Отчёт"));
    assert_eq!(doc.properties.core.creator.as_deref(), Some("Аня"));
    assert_eq!(doc.properties.core.keywords.as_deref(), Some("a, b"));
    assert_eq!(doc.properties.app.total_time, Some(65));
    assert!(!doc.settings.add_paragraph_spacing);
    assert_eq!(doc.settings.default_tab_stop, Twips(567));

    // Разделы: второй начинается с абзаца с другим шаблоном страницы.
    assert_eq!(doc.sections.len(), 2);
    let margins = doc.sections[0].props.margins.unwrap();
    assert_eq!((margins.left, margins.right), (Twips(1701), Twips(850)));
    let landscape = doc.sections[1].props.page_size.unwrap();
    assert_eq!(landscape.orientation, Orientation::Landscape);

    let ps = paragraphs(&doc);
    // Заголовок: стиль, размер в процентах, табуляция от отступа.
    let heading = ps[0];
    assert_eq!(heading.props.style.as_deref(), Some("Heading1"));
    let style = doc.styles.get("Heading1").unwrap();
    assert_eq!(style.run.size, Some(HalfPoints(40)));
    assert_eq!(style.paragraph.tabs[0].position, Twips(2880));
    // «Важно»: полужирный стиль символов в полужирном заголовке. В ODF
    // текст полужирный, поэтому это записано явно.
    let strong = heading.runs().nth(1).unwrap();
    assert_eq!(strong.props.style.as_deref(), Some("StrongEmphasis"));
    assert!(doc.resolve_run(&heading.props, &strong.props).bold);

    // Абзац с автоматическим стилем.
    let p = ps[1];
    assert_eq!(p.props.style.as_deref(), Some("Normal"));
    assert_eq!(p.props.alignment, Some(Alignment::Center));
    assert_eq!(p.props.spacing.line, Some(LineSpacing::ONE_AND_HALF));
    assert_eq!(
        p.props.tabs,
        vec![TabStop {
            position: Twips(2160),
            kind: TabStopKind::Right,
            leader: TabLeader::Dot,
        }]
    );
    let runs: Vec<&Run> = p.runs().collect();
    // Цвет абзаца переходит во все фрагменты.
    assert!(
        runs.iter()
            .all(|r| r.props.color == Some(Color::Rgb(255, 0, 0)))
    );
    assert!(runs[0].content.contains(&RunContent::Tab));
    assert!(
        runs[0]
            .content
            .contains(&RunContent::Break(BreakKind::Line))
    );
    let e = runs.iter().find(|r| r.plain_text() == "e").unwrap();
    assert_eq!(e.props.italic, Some(true));
    assert_eq!(e.props.size, Some(HalfPoints(10)));
    let f = runs.iter().find(|r| r.plain_text() == "f").unwrap();
    assert_eq!(f.props.style.as_deref(), Some("StrongEmphasis"));
    assert_eq!(
        (f.props.italic, f.props.underline),
        (Some(true), Some(Underline::Single))
    );

    let Block::Table(table) = &doc.sections[0].blocks[4] else {
        panic!("{:?}", doc.sections[0].blocks[4]);
    };
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[0].cells.len(), 2);
    assert_eq!(table.rows[2].cells.len(), 3);
}

#[test]
fn writing_paragraphs_runs_and_breaks() {
    let mut doc = Document::new();
    let mut strong = Style::new("Strong", StyleKind::Character);
    strong.name = Some("Strong".into());
    strong.run.bold = Some(true);
    let mut heading = Style::new("Heading2", StyleKind::Paragraph);
    heading.name = Some("heading 2".into());
    heading.based_on = Some("Normal".into());
    heading.run.bold = Some(true);
    let mut normal = Style::new("Normal", StyleKind::Paragraph);
    normal.is_default = true;
    normal.name = Some("Normal".into());
    doc.styles.styles = vec![normal, heading, strong];

    let styled = |text: &str, props: RunProps| Run::text(text).with_props(props);
    let mut page = Run::text("до");
    page.content.push(RunContent::Break(BreakKind::Page));
    page.content.push(RunContent::Text("после".into()));
    let mut end = Run::text("конец");
    end.content.push(RunContent::Break(BreakKind::Page));
    doc.sections[0].blocks = vec![
        Paragraph::from_runs([
            Run::text("Глава "),
            styled(
                "1",
                RunProps {
                    style: Some("Strong".into()),
                    ..RunProps::default()
                },
            ),
        ])
        .with_props(ParagraphProps {
            style: Some("Heading2".into()),
            ..ParagraphProps::default()
        })
        .into(),
        Paragraph::from_runs([
            Run::text("  два  пробела "),
            styled(
                "x",
                RunProps {
                    highlight: Some(Highlight::Yellow),
                    vertical_align: Some(VerticalAlign::Superscript),
                    underline: Some(Underline::WavyDouble),
                    ..RunProps::default()
                },
            ),
        ])
        .into(),
        Paragraph::from_runs([page]).into(),
        Paragraph::from_runs([end]).into(),
        Paragraph::text("новая страница").into(),
    ];

    let bytes = letter_odt::write(&doc).unwrap();
    assert!(bytes.windows(8).any(|w| w == b"mimetype"));
    let back = letter_odt::read(&bytes).unwrap();
    assert_eq!(
        texts(&back),
        vec![
            "Глава 1",
            "  два  пробела x",
            "до",
            "после",
            "конец",
            "новая страница"
        ]
    );
    let ps = paragraphs(&back);
    assert_eq!(ps[0].props.style.as_deref(), Some("Heading2"));
    // По Word «1» не полужирный (стиль символов отменяет стиль абзаца).
    let one = ps[0].runs().nth(1).unwrap();
    assert!(!back.resolve_run(&ps[0].props, &one.props).bold);
    let x = ps[1].runs().nth(1).unwrap();
    assert_eq!(x.props.highlight, Some(Highlight::Yellow));
    assert_eq!(x.props.underline, Some(Underline::WavyDouble));
    assert!(back.resolve_paragraph(&ps[3].props).page_break_before);
    assert!(!back.resolve_paragraph(&ps[4].props).page_break_before);
    assert!(back.resolve_paragraph(&ps[5].props).page_break_before);
    assert_eq!(
        back.styles.get("Heading2").unwrap().name.as_deref(),
        Some("heading 2")
    );
}

#[test]
fn sections_and_empty_documents() {
    let mut doc = Document::new();
    let mut second = doc.sections[0].clone();
    second.props.page_size = Some(letter_model::PageSize::A4.rotated());
    second.blocks = vec![Paragraph::text("альбом").into()];
    let mut third = doc.sections[0].clone();
    third.blocks.clear();
    doc.sections.push(second);
    doc.sections.push(third);
    let back = round_trip(&doc);
    assert_eq!(back.sections.len(), 3);
    assert_eq!(
        back.sections[1].props.page_size.unwrap().orientation,
        Orientation::Landscape
    );
    assert_eq!(texts(&back), vec!["", "альбом", ""]);

    let mut empty = Document::new();
    empty.sections.clear();
    assert_eq!(texts(&round_trip(&empty)), vec![""]);
}

#[test]
fn nested_tables_and_unknown_content() {
    let inner = letter_model::Table {
        grid: Vec::new(),
        raw_props: Vec::new(),
        props: Default::default(),
        rows: vec![letter_model::TableRow {
            raw_props: Vec::new(),
            props: Default::default(),
            cells: vec![letter_model::TableCell::text("внутри")],
        }],
    };
    let mut doc = Document::new();
    doc.sections[0].blocks = vec![
        letter_model::Table {
            grid: Vec::new(),
            raw_props: Vec::new(),
            props: Default::default(),
            rows: vec![letter_model::TableRow {
                raw_props: Vec::new(),
                props: Default::default(),
                cells: vec![
                    letter_model::TableCell {
                        raw_props: Vec::new(),
                        props: Default::default(),
                        blocks: vec![inner.into()],
                    },
                    letter_model::TableCell::default(),
                ],
            }],
        }
        .into(),
        Block::Unknown(letter_model::RawXml::new("<w:sdt/>")),
    ];
    let mut p = Paragraph::text("a");
    p.content.push(Inline::Unknown(letter_model::RawXml::new(
        "<w:bookmarkStart/>",
    )));
    doc.sections[0].blocks.push(p.into());
    let back = round_trip(&doc);
    assert_eq!(texts(&back), vec!["внутри", "", "a"]);
    let Block::Table(outer) = &back.sections[0].blocks[0] else {
        panic!();
    };
    assert!(matches!(outer.rows[0].cells[0].blocks[0], Block::Table(_)));
}

fn zip(files: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in files {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(data.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn broken_files() {
    use letter_odt::ReadError;
    assert!(matches!(
        letter_odt::read(b"not odt"),
        Err(ReadError::NotOdt(_))
    ));
    assert!(matches!(
        letter_odt::read(b"<html/>"),
        Err(ReadError::NotOdt(_))
    ));
    assert!(matches!(
        letter_odt::read(b"PK\x03\x04broken"),
        Err(ReadError::Zip(_))
    ));
    let sheet = zip(&[("mimetype", "application/vnd.oasis.opendocument.spreadsheet")]);
    assert!(
        matches!(letter_odt::read(&sheet), Err(ReadError::NotOdt(m)) if m.contains("spreadsheet"))
    );
    let no_content = zip(&[("mimetype", "application/vnd.oasis.opendocument.text")]);
    assert!(matches!(
        letter_odt::read(&no_content),
        Err(ReadError::MissingPart(_))
    ));
    let bad_xml = zip(&[("content.xml", "<office:document-content>")]);
    assert!(matches!(letter_odt::read(&bad_xml), Err(ReadError::Xml(_))));
    // Только content.xml без стилей и без текста — пустой документ.
    let bare = zip(&[(
        "content.xml",
        r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"/>"#,
    )]);
    assert_eq!(texts(&letter_odt::read(&bare).unwrap()), vec![""]);
    assert!(letter_odt::read_file("нет-такого-файла.odt").is_err());
}

#[test]
fn files_on_disk() {
    let dir = std::env::temp_dir().join(format!("letter-odt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("doc.odt");
    let mut doc = Document::new();
    doc.sections[0].blocks = vec![Paragraph::text("Привет").into()];
    letter_odt::write_file(&doc, &path).unwrap();
    assert_eq!(
        texts(&letter_odt::read_file(&path).unwrap()),
        vec!["Привет"]
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
