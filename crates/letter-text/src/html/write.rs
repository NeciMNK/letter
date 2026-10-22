//! Запись документа в HTML.

use std::fmt::Write as _;

use letter_model::{
    Alignment, Block, BreakKind, Color, Document, FirstLineIndent, Highlight, LineSpacing,
    Paragraph, ResolvedParagraphProps, ResolvedRunProps, RunContent, RunProps, StyleKind,
    Underline, VerticalAlign,
};

use super::css::{points, trim_number};
use crate::styles::{PREFORMATTED, QUOTE, effective_style, heading_level};

/// Записывает документ как страницу HTML5 в UTF-8. Оформление — во
/// встроенных стилях (`style="..."`), чтобы страница выглядела так же
/// в любом браузере и почтовой программе.
pub fn write(doc: &Document) -> String {
    let mut out = String::from(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"generator\" content=\"letter\">\n",
    );
    if let Some(title) = &doc.properties.core.title {
        let _ = writeln!(out, "<title>{}</title>", escape(title));
    }
    out.push_str("</head>\n<body>\n");
    let blocks: Vec<&Block> = doc.blocks().collect();
    blocks_html(&mut out, doc, &blocks);
    out.push_str("</body>\n</html>\n");
    out
}

fn blocks_html(out: &mut String, doc: &Document, blocks: &[&Block]) {
    let mut i = 0;
    while i < blocks.len() {
        match blocks[i] {
            Block::Paragraph(p) if is_preformatted(doc, p) => {
                // Подряд идущие абзацы готового текста — один `<pre>`.
                let mut lines = Vec::new();
                while let Some(Block::Paragraph(p)) = blocks.get(i)
                    && is_preformatted(doc, p)
                {
                    lines.push(p);
                    i += 1;
                }
                preformatted(out, doc, &lines);
                continue;
            }
            Block::Paragraph(p) => paragraph(out, doc, p),
            Block::Table(table) => {
                out.push_str("<table style=\"border-collapse:collapse\">\n");
                for row in &table.rows {
                    out.push_str("<tr>\n");
                    for cell in &row.cells {
                        out.push_str(
                            "<td style=\"border:1px solid windowtext;padding:0 5.4pt;\
                             vertical-align:top\">\n",
                        );
                        let inner: Vec<&Block> = cell.blocks.iter().collect();
                        blocks_html(out, doc, &inner);
                        out.push_str("</td>\n");
                    }
                    out.push_str("</tr>\n");
                }
                out.push_str("</table>\n");
            }
            Block::Unknown(_) => {}
        }
        i += 1;
    }
}

fn is_preformatted(doc: &Document, p: &Paragraph) -> bool {
    effective_style(doc, p).as_deref() == Some(PREFORMATTED)
}

fn paragraph(out: &mut String, doc: &Document, p: &Paragraph) {
    let style = effective_style(doc, p);
    let level = heading_level(doc, p);
    let tag = match (level, style.as_deref()) {
        (Some(level), _) => format!("h{level}"),
        (None, Some(QUOTE)) => "blockquote".to_owned(),
        _ => "p".to_owned(),
    };
    let resolved = doc.resolve_paragraph(&p.props);
    let base = common_run(doc, p);
    let _ = write!(out, "<{tag}");
    // Свой стиль абзаца сохраняем классом, чтобы прочитать обратно.
    if let Some(id) = style.as_deref()
        && level.is_none()
        && id != QUOTE
        && doc
            .styles
            .default_for(StyleKind::Paragraph)
            .is_none_or(|s| s.id != id)
    {
        let _ = write!(out, " class=\"{}\"", escape(id));
    }
    let mut css = paragraph_css(&resolved);
    css.push_str(&base_run_css(&base, level.is_some()));
    css.push_str("white-space:pre-wrap");
    let _ = write!(out, " style=\"{}\">", escape(&css));
    content(out, doc, p, &base, true);
    let _ = writeln!(out, "</{tag}>");
}

fn preformatted(out: &mut String, doc: &Document, lines: &[&Paragraph]) {
    let Some(first) = lines.first() else {
        return;
    };
    let resolved = doc.resolve_paragraph(&first.props);
    let base = common_run(doc, first);
    let mut css = paragraph_css(&resolved);
    css.push_str(&base_run_css(&base, false));
    // Первый перевод строки после `<pre>` браузер не показывает.
    let _ = writeln!(out, "<pre style=\"{}\">", escape(css.trim_end_matches(';')));
    for (i, p) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        content(out, doc, p, &base, false);
    }
    out.push_str("</pre>\n");
}

/// Оформление, которое занимает в абзаце больше всего знаков: его
/// пишем у самого абзаца, а у фрагментов — только отличия. Так запись
/// не зависит от того, задано оформление стилем или напрямую.
fn common_run(doc: &Document, p: &Paragraph) -> ResolvedRunProps {
    let mut counts: Vec<(ResolvedRunProps, usize)> = Vec::new();
    for run in p.runs() {
        let n = run.plain_text().chars().count();
        if n == 0 {
            // Фрагменты без текста (картинки) после чтения не остаются.
            continue;
        }
        let props = doc.resolve_run(&p.props, &run.props);
        match counts.iter_mut().find(|(r, _)| *r == props) {
            Some((_, count)) => *count += n,
            None => counts.push((props, n)),
        }
    }
    // При равенстве — первый по порядку.
    let mut best: Option<(ResolvedRunProps, usize)> = None;
    for (props, count) in counts {
        if best.as_ref().is_none_or(|(_, b)| count > *b) {
            best = Some((props, count));
        }
    }
    best.map_or_else(
        || doc.resolve_run(&p.props, &RunProps::default()),
        |(r, _)| r,
    )
}

fn paragraph_css(p: &ResolvedParagraphProps) -> String {
    let mut css = String::new();
    let _ = write!(
        css,
        "margin-top:{};margin-bottom:{};",
        points(p.space_before.0),
        points(p.space_after.0)
    );
    if p.indent_left.0 != 0 {
        let _ = write!(css, "margin-left:{};", points(p.indent_left.0));
    }
    if p.indent_right.0 != 0 {
        let _ = write!(css, "margin-right:{};", points(p.indent_right.0));
    }
    match p.first_line {
        Some(FirstLineIndent::FirstLine(t)) if t.0 != 0 => {
            let _ = write!(css, "text-indent:{};", points(t.0));
        }
        Some(FirstLineIndent::Hanging(t)) if t.0 != 0 => {
            let _ = write!(css, "text-indent:{};", points(-t.0));
        }
        _ => {}
    }
    match p.line_spacing {
        LineSpacing::Multiple(n) => {
            let _ = write!(css, "line-height:{};", trim_number(f64::from(n) / 240.0));
        }
        LineSpacing::Exact(t) => {
            let _ = write!(css, "line-height:{};", points(t.0));
        }
        LineSpacing::AtLeast(t) => {
            let _ = write!(
                css,
                "line-height:{};mso-line-height-rule:at-least;",
                points(t.0)
            );
        }
    }
    let align = match p.alignment {
        Alignment::Left => None,
        Alignment::Center => Some("center"),
        Alignment::Right => Some("right"),
        Alignment::Justify | Alignment::Distribute => Some("justify"),
    };
    if let Some(align) = align {
        let _ = write!(css, "text-align:{align};");
    }
    if p.page_break_before {
        css.push_str("page-break-before:always;");
    }
    css
}

/// Шрифт абзаца целиком: имя, размер, цвет, начертание.
fn base_run_css(r: &ResolvedRunProps, heading: bool) -> String {
    let mut css = String::new();
    let _ = write!(
        css,
        "font-family:{};font-size:{};",
        font_name(&r.fonts.ascii),
        points(i32::try_from(r.size.0).unwrap_or(24) * 10)
    );
    if let Some(color) = color_css(r.color) {
        let _ = write!(css, "color:{color};");
    }
    if r.bold || heading {
        let _ = write!(
            css,
            "font-weight:{};",
            if r.bold { "bold" } else { "normal" }
        );
    }
    if r.italic {
        css.push_str("font-style:italic;");
    }
    css
}

/// Содержимое абзаца. `closing_break` — дописать `<br>` после разрыва
/// строки в конце (не нужно внутри `<pre>`).
fn content(
    out: &mut String,
    doc: &Document,
    p: &Paragraph,
    base: &ResolvedRunProps,
    closing_break: bool,
) {
    let mut last_break = true;
    for run in p.runs() {
        let r = doc.resolve_run(&p.props, &run.props);
        let mut open = Vec::new();
        if let Some(link) = &run.props.link {
            let _ = write!(out, "<a href=\"{}\">", escape(&link.address()));
            open.push("a");
        }
        let mut css = String::new();
        if r.fonts.ascii != base.fonts.ascii {
            let _ = write!(css, "font-family:{};", font_name(&r.fonts.ascii));
        }
        if r.size != base.size {
            let size = i32::try_from(r.size.0).unwrap_or(24) * 10;
            let _ = write!(css, "font-size:{};", points(size));
        }
        // Браузер красит ссылку сам: цвет и подчёркивание ссылки пишем всегда.
        let link = run.props.link.is_some();
        if link && r.underline == Underline::None {
            css.push_str("text-decoration:none;");
        }
        if r.color != base.color || link {
            let color = color_css(r.color).unwrap_or_else(|| "windowtext".into());
            let _ = write!(css, "color:{color};");
        }
        if r.highlight != Highlight::None
            && let Some(color) = color_css(r.highlight.rgb().map_or(Color::Auto, rgb))
        {
            let _ = write!(css, "background-color:{color};");
        }
        if !r.bold && base.bold {
            css.push_str("font-weight:normal;");
        }
        if !r.italic && base.italic {
            css.push_str("font-style:normal;");
        }
        if !css.is_empty() {
            let _ = write!(
                out,
                "<span style=\"{}\">",
                escape(css.trim_end_matches(';'))
            );
            open.push("span");
        }
        let tags = [
            (r.bold && !base.bold, "b"),
            (r.italic && !base.italic, "i"),
            (r.underline != Underline::None, "u"),
            (r.strike || r.double_strike, "s"),
            (r.vertical_align == VerticalAlign::Superscript, "sup"),
            (r.vertical_align == VerticalAlign::Subscript, "sub"),
        ];
        for (on, tag) in tags {
            if on {
                let _ = write!(out, "<{tag}>");
                open.push(tag);
            }
        }
        for item in &run.content {
            if !matches!(item, RunContent::Unknown(_) | RunContent::Picture(_)) {
                last_break = false;
            }
            match item {
                RunContent::Text(text) => out.push_str(&escape(text)),
                RunContent::Tab => out.push('\t'),
                RunContent::Break(BreakKind::Line) => {
                    out.push_str("<br>");
                    last_break = true;
                }
                RunContent::Break(_) => {
                    out.push_str("<br style=\"page-break-before:always\">");
                }
                // Рисунки в HTML пока не записываются.
                RunContent::Picture(_)
                | RunContent::FieldChar(_)
                | RunContent::FieldCode(_)
                | RunContent::Unknown(_) => {}
            }
        }
        for tag in open.iter().rev() {
            let _ = write!(out, "</{tag}>");
        }
    }
    // Пустой абзац или разрыв строки в конце: браузер не покажет строку
    // без ещё одного `<br>` (читатель его уберёт).
    if last_break && closing_break {
        out.push_str("<br>");
    }
}

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::Rgb(r, g, b)
}

fn font_name(name: &str) -> String {
    if name.contains([' ', ',', '"']) {
        format!("'{}'", name.replace(['\'', '"'], ""))
    } else {
        name.to_owned()
    }
}

/// Цвет текста для CSS (`None` — «авто»).
fn color_css(color: Color) -> Option<String> {
    match color {
        Color::Auto => None,
        Color::Rgb(r, g, b) => Some(format!("#{r:02X}{g:02X}{b:02X}")),
    }
}

/// Экранирует текст для HTML.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}
