//! Чтение и запись RTF: файлы из `tests/fixtures`, переход
//! .docx → RTF → модель и небольшие примеры, написанные вручную.

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use letter_model::{
    Alignment, Block, BreakKind, Color, Document, FirstLineIndent, Highlight, Inline, LineSpacing,
    Orientation, Paragraph, RunContent, StyleKind, TabLeader, TabStopKind, Twips, Underline,
    VerticalAlign,
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

fn rtf(text: &str) -> Document {
    letter_rtf::read(text.as_bytes()).unwrap()
}

#[test]
fn saved_rtf_reads_back_the_same() {
    for path in fixtures("docx") {
        let original = letter_docx::read_file(&path).unwrap();
        let first = letter_rtf::read(letter_rtf::write(&original).as_bytes()).unwrap();
        let text = letter_rtf::write(&first);
        let second = letter_rtf::read(text.as_bytes()).unwrap();
        assert_eq!(first, second, "{}", path.display());
        assert!(text.is_ascii());
    }
}

#[test]
fn docx_survives_conversion_to_rtf() {
    for path in fixtures("docx") {
        let docx = letter_docx::read_file(&path).unwrap();
        let back = letter_rtf::read(letter_rtf::write(&docx).as_bytes()).unwrap();
        let name = path.display();
        // В RTF в документе всегда есть хотя бы один абзац. Рисунки
        // в RTF пока не записываются.
        let mut expected: Vec<String> = texts(&docx)
            .iter()
            .map(|t| t.replace(letter_model::OBJECT_CHAR, ""))
            .collect();
        if expected.is_empty() {
            expected.push(String::new());
        }
        assert_eq!(expected, texts(&back), "{name}");
        assert_eq!(docx.sections.len(), back.sections.len(), "{name}");
        for (a, b) in docx.sections.iter().zip(&back.sections) {
            assert_eq!(
                a.props.page_size.unwrap_or_default(),
                b.props.page_size.unwrap_or_default(),
                "{name}"
            );
            assert_eq!(
                a.props.margins.unwrap_or_default(),
                b.props.margins.unwrap_or_default(),
                "{name}"
            );
        }

        // Итоговое оформление абзацев и фрагментов то же.
        for (p, q) in paragraphs(&docx).into_iter().zip(paragraphs(&back)) {
            let (a, b) = (
                docx.resolve_paragraph(&p.props),
                back.resolve_paragraph(&q.props),
            );
            let first = |f: Option<FirstLineIndent>| match f {
                Some(FirstLineIndent::FirstLine(Twips(0))) => None,
                f => f,
            };
            assert_eq!(a.alignment, b.alignment, "{name}");
            assert_eq!(
                (a.indent_left, a.indent_right, first(a.first_line)),
                (b.indent_left, b.indent_right, first(b.first_line)),
                "{name}"
            );
            assert_eq!(
                (a.space_before, a.space_after, a.line_spacing),
                (b.space_before, b.space_after, b.line_spacing),
                "{name}"
            );
            assert_eq!(
                (
                    a.keep_next,
                    a.keep_lines,
                    a.page_break_before,
                    a.widow_control
                ),
                (
                    b.keep_next,
                    b.keep_lines,
                    b.page_break_before,
                    b.widow_control
                ),
                "{name}"
            );
            assert_eq!(a.tabs, b.tabs, "{name}");
            assert_eq!(a.style, b.style, "{name}");

            let runs = |p: &Paragraph| -> Vec<_> {
                p.content
                    .iter()
                    .filter_map(|i| match i {
                        Inline::Run(r) => Some(r.clone()),
                        _ => None,
                    })
                    .collect()
            };
            // Исправления в RTF пока не пишутся: фрагменты, которые
            // отличались только исправлением, сливаются.
            if p.runs().any(|r| r.props.revision.is_some()) {
                continue;
            }
            for (r, s) in runs(p).iter().zip(runs(q).iter()) {
                let (x, y) = (
                    docx.resolve_run(&p.props, &r.props),
                    back.resolve_run(&q.props, &s.props),
                );
                assert_eq!(x.fonts.ascii, y.fonts.ascii, "{name}");
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
fn libreoffice_rtf_has_the_docx_text() {
    let rtf_files = fixtures("rtf");
    assert!(rtf_files.len() >= 5);
    for path in rtf_files {
        let rtf = letter_rtf::read_file(&path).unwrap();
        let docx_path = path
            .parent()
            .unwrap()
            .join("../docx")
            .join(path.with_extension("docx").file_name().unwrap());
        let docx = letter_docx::read_file(docx_path).unwrap();
        // Номера списков LibreOffice пишет текстом (`\listtext`) — их
        // в модели .docx нет, сравниваем без них.
        let strip = |t: &String| match t.split_once('\t') {
            Some((number, rest)) if number.chars().count() <= 3 && !number.is_empty() => {
                rest.to_owned()
            }
            _ => t.clone(),
        };
        // Разрыв страницы LibreOffice переносит в `\pagebb` следующего абзаца.
        let strip = |t: &String| strip(t).replace('\u{c}', "");
        let a: Vec<String> = texts(&docx).iter().map(strip).collect();
        let b: Vec<String> = texts(&rtf).iter().map(strip).collect();
        assert_eq!(a, b, "{}", path.display());
    }
}

#[test]
fn libreoffice_formatting_is_read() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/rtf");
    let doc = letter_rtf::read_file(dir.join("bold-italic.rtf")).unwrap();
    let p = paragraphs(&doc)[0];
    let find = |text: &str| {
        p.runs()
            .find(|r| r.plain_text().contains(text))
            .map(|r| doc.resolve_run(&p.props, &r.props))
            .unwrap()
    };
    assert!(find("bold").bold);
    assert!(find("italic").italic);
    assert_eq!(find("underline").underline, Underline::Single);
    assert!(find("strike").strike);
    assert_eq!(find("red").color, Color::Rgb(0xC0, 0x00, 0x00));
    assert_eq!(find("red").size.0, 32);

    let headings = letter_rtf::read_file(dir.join("headings.rtf")).unwrap();
    assert!(headings.styles.get("Heading1").is_some());
    let first = paragraphs(&headings)[1];
    assert_eq!(first.props.style.as_deref(), Some("Heading1"));
}

#[test]
fn text_in_code_pages_and_unicode() {
    // `#` вместо обратной черты перед `u`: так запись не спутать
    // с экранированием Юникода.
    let text = concat!(
        r"{\rtf1\ansi\ansicpg1252\deff0{\fonttbl{\f0\fswiss Arial;}{\f1\fcharset204 Times;}}",
        r"\pard caf\'e9 {\f1 \'cf\'f0\'e8\'e2\'e5\'f2} \uc1#u1105?#u-10179?#u-8704? \uc2#u1046xx!",
        r"\par x\~y\emdash z\{\}\\}",
    );
    let doc = rtf(&text.replace('#', "\\"));
    assert_eq!(texts(&doc), vec!["café Привет ё😀 Ж!", "x\u{a0}y—z{}\\"]);
}

#[test]
fn destinations_fields_and_hidden_text() {
    let doc = rtf(concat!(
        r"{\rtf1{\info{\title Report}{\author Ann}{\creatim\yr2024}}",
        r"{\*\generator Test;}{\*\unknown skipped}{\header head}",
        r"\pard A{\field{\*\fldinst HYPERLINK x}{\fldrslt link}}B{\v hidden}",
        r"{\pict\pngblip 89504e47}{\footnote note}C\par}"
    ));
    assert_eq!(texts(&doc), vec!["AlinkBC"]);
    assert_eq!(doc.properties.core.title.as_deref(), Some("Report"));
    assert_eq!(doc.properties.core.creator.as_deref(), Some("Ann"));

    let doc = rtf(r"{\rtf1{\info{\title \'cf\'f0\'e8}}\pard x}");
    assert_eq!(doc.properties.core.title.as_deref(), Some("Ïðè"));
}

#[test]
fn paragraph_and_character_formatting() {
    let doc = rtf(concat!(
        r"{\rtf1\deff0{\fonttbl{\f0 Arial;}{\f1 Courier New;}}",
        r"{\colortbl;\red255\green0\blue0;\red255\green255\blue0;}",
        r"\pard\qc\li720\ri360\fi-360\sb120\sa240\sl360\slmult1\keepn\keep\pagebb\widctlpar",
        r"\tqr\tldot\tx5000\tb6000 {\b\i\ul bold}{\f1\fs28\cf1\highlight2\super x}\uldb y\par",
        r"\pard\sl-300\slmult0 exact\line two\tab\page\par}",
    ));
    let ps = paragraphs(&doc);
    let p = &ps[0].props;
    assert_eq!(p.alignment, Some(Alignment::Center));
    assert_eq!(p.indentation.left, Some(Twips(720)));
    assert_eq!(p.indentation.right, Some(Twips(360)));
    assert_eq!(
        p.indentation.first_line,
        Some(FirstLineIndent::Hanging(Twips(360)))
    );
    assert_eq!(p.spacing.before, Some(Twips(120)));
    assert_eq!(p.spacing.after, Some(Twips(240)));
    assert_eq!(p.spacing.line, Some(LineSpacing::ONE_AND_HALF));
    assert_eq!(
        (
            p.keep_next,
            p.keep_lines,
            p.page_break_before,
            p.widow_control
        ),
        (Some(true), Some(true), Some(true), Some(true))
    );
    assert_eq!(p.tabs.len(), 2);
    assert_eq!(p.tabs[0].kind, TabStopKind::Right);
    assert_eq!(p.tabs[0].leader, TabLeader::Dot);
    assert_eq!(p.tabs[1].kind, TabStopKind::Bar);

    let runs: Vec<_> = ps[0].runs().collect();
    assert_eq!(runs.len(), 3);
    let bold = &runs[0].props;
    assert_eq!(
        (bold.bold, bold.italic, bold.underline),
        (Some(true), Some(true), Some(Underline::Single))
    );
    assert_eq!(bold.fonts.ascii.as_deref(), Some("Arial"));
    let x = &runs[1].props;
    assert_eq!(x.fonts.ascii.as_deref(), Some("Courier New"));
    assert_eq!(x.size.map(|s| s.0), Some(28));
    assert_eq!(x.color, Some(Color::Rgb(255, 0, 0)));
    assert_eq!(x.highlight, Some(Highlight::Yellow));
    assert_eq!(x.vertical_align, Some(VerticalAlign::Superscript));
    // Подчёркивание вне группы действует до конца абзаца.
    assert_eq!(runs[2].props.underline, Some(Underline::Double));

    let second = ps[1];
    assert_eq!(
        second.props.spacing.line,
        Some(LineSpacing::Exact(Twips(300)))
    );
    let content = &second.runs().next().unwrap().content;
    assert!(content.contains(&RunContent::Break(BreakKind::Line)));
    assert!(content.contains(&RunContent::Tab));
    assert!(content.contains(&RunContent::Break(BreakKind::Page)));
}

#[test]
fn tables_sections_and_styles() {
    let doc = rtf(concat!(
        r"{\rtf1{\stylesheet{\s0 Normal;}{\s1\sbasedon0\snext0\b\fs32 heading 1;}",
        r"{\*\cs10\additive Default Paragraph Font;}}",
        r"\paperw11906\paperh16838\margl1701",
        r"\pard\s1 Title\par",
        r"\trowd\cellx3000\cellx6000\pard\intbl a\cell b\par c\cell\row",
        r"\pard after\par\sect\sectd\lndscpsxn\pgwsxn16838\pghsxn11906\pard second}",
    ));
    let heading = doc.styles.get("Heading1").unwrap();
    assert_eq!(heading.kind, StyleKind::Paragraph);
    assert_eq!(heading.based_on.as_deref(), Some("Normal"));
    assert_eq!(heading.run.bold, Some(true));
    assert_eq!(heading.run.size.map(|s| s.0), Some(32));
    assert!(doc.styles.get("Normal").unwrap().is_default);
    assert!(doc.styles.get("DefaultParagraphFont").unwrap().is_default);

    assert_eq!(doc.sections.len(), 2);
    let first = &doc.sections[0];
    assert_eq!(first.props.page_size.unwrap().width, Twips(11906));
    assert_eq!(first.props.margins.unwrap().left, Twips(1701));
    let Block::Table(table) = &first.blocks[1] else {
        panic!("{:?}", first.blocks[1]);
    };
    assert_eq!(table.rows.len(), 1);
    assert_eq!(table.rows[0].cells.len(), 2);
    assert_eq!(table.rows[0].cells[1].blocks.len(), 2);
    assert_eq!(texts(&doc), vec!["Title", "a", "b", "c", "after", "second"]);
    let second = &doc.sections[1].props.page_size.unwrap();
    assert_eq!(second.orientation, Orientation::Landscape);
    assert_eq!(second.width, Twips(16838));

    // Записанный документ читается так же.
    let again = rtf(&letter_rtf::write(&doc));
    assert_eq!(texts(&again), texts(&doc));
    assert_eq!(again.sections.len(), 2);
    assert!(matches!(again.sections[0].blocks[1], Block::Table(_)));
}

#[test]
fn broken_files() {
    assert!(matches!(
        letter_rtf::read(b"not rtf"),
        Err(letter_rtf::ReadError::NotRtf)
    ));
    // Незакрытые группы, лишние скобки, обрыв посреди слова.
    assert_eq!(texts(&rtf(r"{\rtf1 a{\b b")), vec!["ab"]);
    assert_eq!(texts(&rtf(r"{\rtf1 a}}}} b")), vec!["a"]);
    assert_eq!(texts(&rtf(r"{\rtf1 a\")), vec!["a"]);
    // Пустой документ — один пустой абзац.
    assert_eq!(texts(&rtf(r"{\rtf1}")), vec![""]);
    assert!(letter_rtf::read_file("нет-такого-файла.rtf").is_err());
}

#[test]
fn files_on_disk() {
    let dir = std::env::temp_dir().join(format!("letter-rtf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("doc.rtf");
    let mut doc = Document::new();
    doc.sections[0].blocks = vec![Paragraph::text("Привет").into()];
    letter_rtf::write_file(&doc, &path).unwrap();
    assert_eq!(
        texts(&letter_rtf::read_file(&path).unwrap()),
        vec!["Привет"]
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
