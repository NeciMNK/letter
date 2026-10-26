use letter_model::{Block, Color, Highlight};

use super::*;

fn texts(doc: &Document) -> Vec<String> {
    doc.blocks().map(Block::plain_text).collect()
}

fn paragraph(doc: &Document, index: usize) -> &letter_model::Paragraph {
    match doc.blocks().nth(index) {
        Some(Block::Paragraph(p)) => p,
        other => panic!("{other:?}"),
    }
}

#[test]
fn structure_and_whitespace() {
    let doc = read(
        b"<html><head><title> My  page </title><style>p{color:red}</style></head>\
          <body>\n  <h1>Head</h1>\n<p>One\n   two<br>three</p>loose text<div>in <b>div</b><p>nested</p>after</div>\
          <ul><li>a<li>b<ol start=3><li>c</ol></ul><pre>\nx  y\n\tz</pre>\
          <table><tr><td>1<td>2<tr><th>3</table><script>alert(1)</script>",
    );
    assert_eq!(doc.properties.core.title.as_deref(), Some("My page"));
    assert_eq!(
        texts(&doc),
        [
            "Head",
            "One two\nthree",
            "loose text",
            "in div",
            "nested",
            "after",
            "•\ta",
            "•\tb",
            "3.\tc",
            "x  y",
            "\tz",
            "1\t2\n3",
            "",
        ]
    );
    assert_eq!(paragraph(&doc, 0).props.style.as_deref(), Some("Heading1"));
    assert_eq!(paragraph(&doc, 8).props.indentation.left, Some(Twips(1440)));
    assert_eq!(
        paragraph(&doc, 9).props.style.as_deref(),
        Some(PREFORMATTED)
    );
}

#[test]
fn formatting() {
    let doc = read(
        "<p style=\"text-align:center;margin-left:36pt;text-indent:-18pt;margin-top:6pt;\
         line-height:150%\">\
         <span style=\"font-family:'Times New Roman';font-size:16pt;color:#C00000\">big</span>\
         <font color=blue size=1>small</font><mark>hl</mark>\
         <span style=\"background-color:#00FF00;font-weight:700\">g</span>\
         x<sup>2</sup><s>gone</s><u>u</u><i>i</i><code>c</code>\
         <span style=\"display:none\">hidden</span></p>"
            .as_bytes(),
    );
    assert_eq!(texts(&doc), ["bigsmallhlgx2goneuic"]);
    let p = paragraph(&doc, 0);
    assert_eq!(p.props.alignment, Some(Alignment::Center));
    assert_eq!(p.props.indentation.left, Some(Twips(720)));
    assert_eq!(
        p.props.indentation.first_line,
        Some(FirstLineIndent::Hanging(Twips(360)))
    );
    assert_eq!(p.props.spacing.before, Some(Twips(120)));
    assert_eq!(p.props.spacing.line, Some(LineSpacing::ONE_AND_HALF));
    let runs: Vec<_> = p.runs().collect();
    let big = &runs[0].props;
    assert_eq!(big.fonts.ascii.as_deref(), Some("Times New Roman"));
    assert_eq!(big.size, Some(HalfPoints(32)));
    assert_eq!(big.color, Some(Color::Rgb(0xC0, 0, 0)));
    assert_eq!(runs[1].props.size, Some(HalfPoints(16)));
    assert_eq!(runs[2].props.highlight, Some(Highlight::Yellow));
    assert_eq!(runs[3].props.highlight, Some(Highlight::Green));
    assert_eq!(runs[3].props.bold, Some(true));
    assert_eq!(
        runs[5].props.vertical_align,
        Some(VerticalAlign::Superscript)
    );
    assert_eq!(runs[6].props.strike, Some(true));
    assert_eq!(runs[7].props.underline, Some(Underline::Single));
    assert_eq!(runs[8].props.italic, Some(true));
    assert_eq!(runs[9].props.fonts.ascii.as_deref(), Some(MONOSPACE));
}

#[test]
fn encodings_and_breaks() {
    let doc = read(b"<meta charset=windows-1251><p>\xcf\xf0\xe8\xe2\xe5\xf2</p>");
    assert_eq!(texts(&doc), ["Привет"]);
    let doc = read(b"<p>a<br style='page-break-before:always'>b</p><p><br></p><p></p>");
    assert_eq!(texts(&doc), ["a\u{c}b", ""]);
    assert_eq!(texts(&read(b"")), [""]);
    assert_eq!(texts(&read(b"</p></b><<>>&")), ["<<>>&"]);
}

#[test]
fn deep_nesting_does_not_overflow() {
    let html = "<div><table><tr><td>".repeat(500) + "x";
    let doc = read(html.as_bytes());
    assert!(doc.plain_text().contains('x'));
}

#[test]
fn write_and_read_back() {
    let source = read(
        "<title>T</title><h2>Head</h2><p style='text-align:right'>a <b>b</b>\tc<br>d</p><p></p>\
         <p><br></p><blockquote>q</blockquote><pre>l1\n\nl3</pre><ul><li>x</li></ul>\
         <table><tr><td>1</td><td><p>2</p><p>3</p></td></tr></table>"
            .as_bytes(),
    );
    let html = write(&source);
    let back = read(html.as_bytes());
    assert_eq!(texts(&back), texts(&source));
    assert_eq!(back.properties.core.title.as_deref(), Some("T"));
    for (a, b) in source.blocks().zip(back.blocks()) {
        if let (Block::Paragraph(a), Block::Paragraph(b)) = (a, b) {
            assert_eq!(
                source.resolve_paragraph(&a.props),
                back.resolve_paragraph(&b.props),
                "{html}"
            );
        }
    }
    assert!(html.contains("<h2 "));
    assert!(html.contains("<blockquote "));
    assert!(html.contains("<pre "));
    assert!(html.contains("class=\"ListParagraph\""));
}

#[test]
fn links_are_read_and_written() {
    let doc = read(b"<p>See <a href=\"https://example.com/?a=1&amp;b=2\">site</a> and <a href=\"#top\">top</a>.</p>");
    let p = paragraph(&doc, 0);
    let links: Vec<_> = p
        .runs()
        .filter_map(|r| r.props.link.as_ref())
        .map(letter_model::Hyperlink::address)
        .collect();
    assert_eq!(links, ["https://example.com/?a=1&b=2", "#top"]);
    let html = write(&doc);
    assert!(
        html.contains("<a href=\"https://example.com/?a=1&amp;b=2\">"),
        "{html}"
    );
    let again = read(html.as_bytes());
    let p = paragraph(&again, 0);
    assert_eq!(p.plain_text(), "See site and top.");
    assert!(p.runs().any(|r| r.props.link.is_some()));
}

#[test]
fn pictures_are_kept_inside_the_file() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/docx/image.docx");
    let source = letter_docx::read_bytes(&std::fs::read(fixture).expect("файл")).expect("docx");
    let pictures = |doc: &Document| -> Vec<letter_model::Picture> {
        doc.blocks()
            .filter_map(|b| match b {
                Block::Paragraph(p) => Some(p),
                _ => None,
            })
            .flat_map(|p| p.runs())
            .flat_map(|r| r.content.iter())
            .filter_map(|c| match c {
                RunContent::Picture(p) => Some(p.clone()),
                _ => None,
            })
            .collect()
    };
    let before = pictures(&source);
    assert!(!before.is_empty());
    let html = write(&source);
    assert!(html.contains("<img src=\"data:image/"));
    let after = pictures(&read(html.as_bytes()));
    assert_eq!(after.len(), before.len());
    let image = |p: &letter_model::Picture| p.image.as_ref().map(|i| i.bytes.clone());
    assert_eq!(image(&after[0]), image(&before[0]));
    // Размер — с точностью до пикселя.
    assert!((after[0].width - before[0].width).abs() <= 9525);
    // Картинка по ссылке не загружается: остаётся подпись.
    let linked = read(b"<p><img src='http://example.com/a.png' alt='logo'></p>");
    assert_eq!(texts(&linked), ["logo"]);
}
