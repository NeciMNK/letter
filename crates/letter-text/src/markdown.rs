//! Markdown: чтение (CommonMark и таблицы GFM через `pulldown-cmark`)
//! и запись.

use pulldown_cmark::{Alignment as MdAlignment, Event, Options, Parser, Tag, TagEnd};

use letter_model::{
    Alignment, Block, BreakKind, Document, FirstLineIndent, Paragraph, ParagraphProps, RunContent,
    RunFonts, RunProps, Table, Twips, VerticalAlign,
};

use crate::builder::Builder;
use crate::styles::{
    LIST, MONOSPACE, PREFORMATTED, QUOTE, effective_style, heading_level, heading_style,
    is_monospace,
};

/// Отступ одного уровня списка.
const LIST_INDENT: i32 = 720;

/// Наибольшая вложенность, которую стоит помнить (защита от
/// испорченных файлов).
const MAX_DEPTH: usize = 64;

struct List {
    /// Номер следующего пункта; `None` — маркированный список.
    next: Option<u64>,
}

/// Читает Markdown.
pub fn read(text: &str) -> Document {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_SUBSCRIPT;
    let mut builder = Builder::new();
    let mut runs: Vec<RunProps> = vec![RunProps::default()];
    let mut lists: Vec<List> = Vec::new();
    let mut quotes = 0usize;
    let mut code_block = false;
    // Пункт списка только что начат: его первый абзац уже открыт.
    let mut item_open = false;
    let mut alignments: Vec<MdAlignment> = Vec::new();
    let mut column = 0usize;

    let paragraph_props = |lists: &[List], quotes: usize| -> ParagraphProps {
        if !lists.is_empty() {
            let depth = i32::try_from(lists.len()).unwrap_or(1);
            ParagraphProps {
                style: Some(LIST.to_owned()),
                indentation: letter_model::Indentation {
                    left: Some(Twips(LIST_INDENT * depth)),
                    right: None,
                    first_line: Some(FirstLineIndent::Hanging(Twips(360))),
                },
                ..ParagraphProps::default()
            }
        } else if quotes > 0 {
            ParagraphProps {
                style: Some(QUOTE.to_owned()),
                ..ParagraphProps::default()
            }
        } else {
            ParagraphProps::default()
        }
    };
    let code_props = || ParagraphProps {
        style: Some(PREFORMATTED.to_owned()),
        ..ParagraphProps::default()
    };

    for event in Parser::new_ext(text, options) {
        let current = runs.last().cloned().unwrap_or_default();
        match event {
            Event::Start(tag) => {
                let mut run = current.clone();
                match tag {
                    Tag::Paragraph => {
                        if !std::mem::take(&mut item_open) {
                            builder.start_paragraph(paragraph_props(&lists, quotes));
                        }
                    }
                    Tag::Heading { level, .. } => {
                        builder.start_paragraph(ParagraphProps {
                            style: Some(heading_style(level as u8)),
                            ..ParagraphProps::default()
                        });
                    }
                    Tag::BlockQuote(_) => quotes += 1,
                    Tag::CodeBlock(_) => {
                        code_block = true;
                        builder.start_paragraph(code_props());
                    }
                    Tag::List(start) => {
                        builder.end_paragraph();
                        item_open = false;
                        if lists.len() < MAX_DEPTH {
                            lists.push(List { next: start });
                        }
                    }
                    Tag::Item => {
                        builder.start_paragraph(paragraph_props(&lists, quotes));
                        let marker = match lists.last_mut().and_then(|l| l.next.as_mut()) {
                            Some(next) => {
                                let marker = format!("{next}.");
                                *next = next.saturating_add(1);
                                marker
                            }
                            None => "•".to_owned(),
                        };
                        builder.text(&marker, &current);
                        builder.content(RunContent::Tab, &current);
                        item_open = true;
                    }
                    Tag::Table(columns) => {
                        alignments = columns;
                        builder.start_table();
                    }
                    Tag::TableHead => {
                        column = 0;
                        builder.start_row();
                    }
                    Tag::TableRow => {
                        column = 0;
                        builder.start_row();
                    }
                    Tag::TableCell => {
                        builder.start_cell();
                        let alignment = match alignments.get(column) {
                            Some(MdAlignment::Center) => Some(Alignment::Center),
                            Some(MdAlignment::Right) => Some(Alignment::Right),
                            _ => None,
                        };
                        column += 1;
                        builder.start_paragraph(ParagraphProps {
                            alignment,
                            ..ParagraphProps::default()
                        });
                    }
                    Tag::Emphasis => run.italic = Some(true),
                    Tag::Strong => run.bold = Some(true),
                    Tag::Strikethrough => run.strike = Some(true),
                    Tag::Superscript => run.vertical_align = Some(VerticalAlign::Superscript),
                    Tag::Subscript => run.vertical_align = Some(VerticalAlign::Subscript),
                    _ => {}
                }
                if runs.len() < MAX_DEPTH {
                    runs.push(run);
                } else {
                    runs.push(current);
                }
            }
            Event::End(tag) => {
                runs.pop();
                if runs.is_empty() {
                    runs.push(RunProps::default());
                }
                match tag {
                    TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item => {
                        item_open = false;
                        builder.end_paragraph();
                    }
                    TagEnd::BlockQuote(_) => quotes = quotes.saturating_sub(1),
                    TagEnd::CodeBlock => {
                        code_block = false;
                        builder.end_paragraph();
                    }
                    TagEnd::List(_) => {
                        lists.pop();
                        builder.end_paragraph();
                    }
                    TagEnd::TableCell => builder.end_cell(),
                    TagEnd::TableHead => builder.end_row(),
                    TagEnd::TableRow => builder.end_row(),
                    TagEnd::Table => builder.end_table(),
                    _ => {}
                }
            }
            Event::Text(text) if code_block => {
                for (i, line) in text.split('\n').enumerate() {
                    if i > 0 {
                        builder.empty_paragraph();
                        builder.start_paragraph(code_props());
                    }
                    for (j, piece) in line.split('\t').enumerate() {
                        if j > 0 {
                            builder.content(RunContent::Tab, &current);
                        }
                        builder.text(piece, &current);
                    }
                }
            }
            Event::Text(text) | Event::InlineMath(text) | Event::DisplayMath(text) => {
                builder.text(&text, &current);
            }
            Event::Code(code) => {
                let props = RunProps {
                    fonts: RunFonts::new(MONOSPACE),
                    ..current
                };
                builder.text(&code, &props);
            }
            Event::SoftBreak => builder.text(" ", &current),
            Event::HardBreak => builder.line_break(&current),
            Event::InlineHtml(html) | Event::Html(html) => {
                let tag = html.trim().to_ascii_lowercase().replace(' ', "");
                if matches!(tag.as_str(), "<br>" | "<br/>") {
                    builder.line_break(&current);
                }
            }
            Event::TaskListMarker(checked) => {
                builder.text(if checked { "☒ " } else { "☐ " }, &current);
            }
            Event::Rule => builder.end_paragraph(),
            Event::FootnoteReference(_) => {}
        }
    }
    builder.finish()
}

/// Записывает документ в Markdown.
pub fn write(doc: &Document) -> String {
    let mut out = String::new();
    let blocks: Vec<&Block> = doc.blocks().collect();
    let mut i = 0;
    // Предыдущий блок — пункт списка (пункты идут без пустой строки).
    let mut previous_item = false;
    while i < blocks.len() {
        match blocks[i] {
            Block::Paragraph(p) if effective_style(doc, p).as_deref() == Some(PREFORMATTED) => {
                let mut lines = Vec::new();
                while let Some(Block::Paragraph(p)) = blocks.get(i)
                    && effective_style(doc, p).as_deref() == Some(PREFORMATTED)
                {
                    lines.push(plain(p));
                    i += 1;
                }
                let fence = fence_for(&lines);
                separate(&mut out);
                out.push_str(&fence);
                out.push('\n');
                for line in lines {
                    out.push_str(&line);
                    out.push('\n');
                }
                out.push_str(&fence);
                out.push('\n');
                previous_item = false;
                continue;
            }
            Block::Paragraph(p) => {
                if let Some((prefix, skip)) = list_item(doc, p) {
                    if !previous_item {
                        separate(&mut out);
                    }
                    out.push_str(&prefix);
                    out.push_str(&inline(doc, p, skip, false));
                    out.push('\n');
                    previous_item = true;
                } else {
                    let text = inline(doc, p, 0, false);
                    previous_item = false;
                    if text.trim().is_empty() {
                        i += 1;
                        continue;
                    }
                    separate(&mut out);
                    if let Some(level) = heading_level(doc, p) {
                        out.push_str(&"#".repeat(usize::from(level)));
                        out.push(' ');
                    } else if effective_style(doc, p).as_deref() == Some(QUOTE) {
                        out.push_str("> ");
                    }
                    out.push_str(&text);
                    out.push('\n');
                }
            }
            Block::Table(table) => {
                separate(&mut out);
                write_table(&mut out, doc, table);
                previous_item = false;
            }
            Block::Unknown(_) => {}
        }
        i += 1;
    }
    out
}

/// Пустая строка между блоками.
fn separate(out: &mut String) {
    if !out.is_empty() {
        out.push('\n');
    }
}

/// Пункт списка: префикс Markdown и сколько знаков маркера пропустить.
fn list_item(doc: &Document, p: &Paragraph) -> Option<(String, usize)> {
    if effective_style(doc, p).as_deref() != Some(LIST) {
        return None;
    }
    let text = p.plain_text();
    let (marker, rest) = text.split_once('\t')?;
    let skip = marker.chars().count() + 1;
    let resolved = doc.resolve_paragraph(&p.props);
    let depth = usize::try_from(resolved.indent_left.0 / LIST_INDENT)
        .unwrap_or(1)
        .max(1);
    let indent = "    ".repeat(depth - 1);
    let _ = rest;
    if marker == "•" || marker == "-" || marker == "·" {
        return Some((format!("{indent}- "), skip));
    }
    let number = marker.strip_suffix(['.', ')'])?;
    if !number.is_empty() && number.len() <= 9 && number.bytes().all(|b| b.is_ascii_digit()) {
        return Some((format!("{indent}{number}. "), skip));
    }
    None
}

fn plain(p: &Paragraph) -> String {
    p.plain_text().replace(['\n', '\u{c}'], " ")
}

/// Ограда блока кода длиннее любой цепочки `` ` `` внутри.
fn fence_for(lines: &[String]) -> String {
    let longest = lines.iter().map(|l| longest_run(l, '`')).max().unwrap_or(0);
    "`".repeat(longest.max(2) + 1)
}

fn longest_run(text: &str, c: char) -> usize {
    let mut best = 0;
    let mut current = 0;
    for ch in text.chars() {
        if ch == c {
            current += 1;
            best = best.max(current);
        } else {
            current = 0;
        }
    }
    best
}

fn write_table(out: &mut String, doc: &Document, table: &Table) {
    let columns = table.rows.iter().map(|r| r.cells.len()).max().unwrap_or(0);
    if columns == 0 {
        return;
    }
    for (i, row) in table.rows.iter().enumerate() {
        out.push('|');
        for c in 0..columns {
            let text = row.cells.get(c).map_or(String::new(), |cell| {
                cell.blocks
                    .iter()
                    .filter_map(|b| match b {
                        Block::Paragraph(p) => Some(inline(doc, p, 0, true)),
                        Block::Table(t) => Some(escape(&t.plain_text().replace('\n', " "), false)),
                        Block::Unknown(_) => None,
                    })
                    .filter(|t| !t.is_empty())
                    .collect::<Vec<_>>()
                    .join("<br>")
            });
            out.push(' ');
            out.push_str(&text);
            out.push_str(" |");
        }
        out.push('\n');
        if i == 0 {
            out.push('|');
            out.push_str(&" --- |".repeat(columns));
            out.push('\n');
        }
    }
}

/// Оформление, которое видно в Markdown.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
struct Marks {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
}

/// Текст абзаца с разметкой. `skip` — сколько знаков пропустить в начале
/// (маркер списка), `cell` — внутри ячейки таблицы.
fn inline(doc: &Document, p: &Paragraph, skip: usize, cell: bool) -> String {
    // Оформление стиля абзаца (курсив цитаты, полужирный заголовка)
    // Markdown показывает сам.
    let base = doc.resolve_run(&p.props, &RunProps::default());
    let mut segments: Vec<(Marks, String)> = Vec::new();
    let mut skip = skip;
    for run in p.runs() {
        let r = doc.resolve_run(&p.props, &run.props);
        let marks = Marks {
            bold: r.bold && !base.bold,
            italic: r.italic && !base.italic,
            strike: r.strike || r.double_strike,
            code: is_monospace(&r.fonts.ascii),
        };
        for item in &run.content {
            let piece = match item {
                RunContent::Text(text) => text.clone(),
                RunContent::Tab => "\t".to_owned(),
                RunContent::Break(BreakKind::Line) => "\n".to_owned(),
                RunContent::Break(_)
                | RunContent::Picture(_)
                | RunContent::FieldChar(_)
                | RunContent::FieldCode(_)
                | RunContent::Unknown(_) => continue,
            };
            let piece: String = if skip > 0 {
                let n = piece.chars().count();
                let taken = skip.min(n);
                skip -= taken;
                piece.chars().skip(taken).collect()
            } else {
                piece
            };
            match segments.last_mut() {
                Some((m, text)) if *m == marks => text.push_str(&piece),
                _ => segments.push((marks, piece)),
            }
        }
    }

    let mut out = String::new();
    let mut first = true;
    for (marks, text) in segments {
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                out.push_str(if cell { "<br>" } else { "\\\n" });
                first = true;
            }
            if line.is_empty() {
                continue;
            }
            out.push_str(&marked(line, marks, first && !cell));
            first = false;
        }
    }
    out
}

/// Кусок текста в знаках выделения. Пробелы по краям выносятся наружу:
/// `** a**` в Markdown не работает.
fn marked(text: &str, marks: Marks, line_start: bool) -> String {
    let body = text.trim_matches(' ');
    if body.is_empty() {
        return text.to_owned();
    }
    let lead = &text[..text.len() - text.trim_start_matches(' ').len()];
    let trail = &text[text.trim_end_matches(' ').len()..];
    let mut inner = if marks.code {
        let ticks = "`".repeat(longest_run(body, '`') + 1);
        let pad = if body.starts_with('`') || body.ends_with('`') {
            " "
        } else {
            ""
        };
        format!("{ticks}{pad}{body}{pad}{ticks}")
    } else {
        escape(body, line_start && lead.is_empty())
    };
    if marks.italic {
        inner = format!("*{inner}*");
    }
    if marks.bold {
        inner = format!("**{inner}**");
    }
    if marks.strike {
        inner = format!("~~{inner}~~");
    }
    format!("{lead}{inner}{trail}")
}

/// Экранирует знаки разметки. `line_start` — текст в начале строки, где
/// особое значение есть ещё у `#`, `-`, `+`, `=` и `1.`.
fn escape(text: &str, line_start: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(
            c,
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '~' | '|' | '&'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    if line_start {
        if out.starts_with(['#', '-', '+', '=']) {
            out.insert(0, '\\');
        } else {
            let digits = out.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 && matches!(out.as_bytes().get(digits), Some(b'.' | b')')) {
                out.insert(digits, '\\');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(doc: &Document) -> Vec<String> {
        doc.blocks().map(Block::plain_text).collect()
    }

    #[test]
    fn blocks_are_read() {
        let doc = read(
            "# Title\n\nSome *em* and **strong** ~~gone~~ `code`.\nNext line\\\nbreak\n\n\
             > quoted\n\n- one\n- two\n  1. inner\n\n```\nfn x() {\n\n}\n```\n\n\
             | A | B |\n|:-:|--:|\n| 1 | 2 |\n",
        );
        assert_eq!(
            texts(&doc),
            [
                "Title",
                "Some em and strong gone code. Next line\nbreak",
                "quoted",
                "•\tone",
                "•\ttwo",
                "1.\tinner",
                "fn x() {",
                "",
                "}",
                "A\tB\n1\t2",
                "",
            ]
        );
        let Block::Paragraph(heading) = &doc.sections[0].blocks[0] else {
            panic!()
        };
        assert_eq!(heading.props.style.as_deref(), Some("Heading1"));
        let Block::Paragraph(p) = &doc.sections[0].blocks[1] else {
            panic!()
        };
        let runs: Vec<_> = p.runs().collect();
        assert_eq!(runs[1].props.italic, Some(true));
        assert_eq!(runs[3].props.bold, Some(true));
        assert_eq!(runs[5].props.strike, Some(true));
        assert_eq!(runs[7].props.fonts.ascii.as_deref(), Some(MONOSPACE));
        let Block::Paragraph(inner) = &doc.sections[0].blocks[5] else {
            panic!()
        };
        assert_eq!(inner.props.indentation.left, Some(Twips(1440)));
    }

    #[test]
    fn escaping() {
        assert_eq!(
            escape("a*b_c [x] <y> 1|2", false),
            r"a\*b\_c \[x\] \<y\> 1\|2"
        );
        assert_eq!(escape("# no", true), r"\# no");
        assert_eq!(escape("12. no", true), r"12\. no");
        assert_eq!(escape("12. ok", false), "12. ok");
        let bold = Marks {
            bold: true,
            ..Marks::default()
        };
        assert_eq!(marked(" x ", bold, false), " **x** ");
        let code = Marks {
            code: true,
            ..Marks::default()
        };
        assert_eq!(marked("a`b", code, false), "``a`b``");
    }

    #[test]
    fn write_and_read_back() {
        let source = "# Title\n\nSome *em* and **strong** ~~gone~~ `code`\\\nbreak\n\n> quoted\n\n\
                      - one\n- two\n    1. inner\n\n```\nfn x() {\n\n}\n```\n\n\
                      | A | B |\n| --- | --- |\n| 1 | 2 |\n";
        let doc = read(source);
        let written = write(&doc);
        assert_eq!(written, source);
        assert_eq!(texts(&read(&written)), texts(&doc));
    }
}
