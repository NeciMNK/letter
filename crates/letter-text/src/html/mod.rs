//! HTML: чтение в модель и запись.

mod css;
mod lexer;
mod write;

pub use write::write;

use encoding_rs::Encoding;
use letter_model::{
    Alignment, BreakKind, Document, FirstLineIndent, HalfPoints, LineSpacing, ParagraphProps,
    RunContent, RunFonts, RunProps, StyleKind, Twips, Underline, VerticalAlign,
};

use crate::builder::Builder;
use crate::styles::{self, LIST, MONOSPACE, PREFORMATTED, QUOTE};
use lexer::Token;

/// Наибольшая вложенность элементов: глубже теги не открываются
/// (защита от испорченных файлов).
const MAX_DEPTH: usize = 256;

/// Отступ одного уровня списка.
const LIST_INDENT: i32 = 720;

/// Элементы без содержимого.
const VOID: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Элементы, содержимое которых не показывается.
const SKIP: [&str; 10] = [
    "head", "script", "style", "template", "noscript", "iframe", "object", "svg", "select",
    "button",
];

/// Блочные элементы: каждый начинает новый абзац.
const BLOCK: [&str; 30] = [
    "p",
    "div",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "li",
    "blockquote",
    "pre",
    "ul",
    "ol",
    "dl",
    "dt",
    "dd",
    "section",
    "article",
    "header",
    "footer",
    "nav",
    "main",
    "aside",
    "figure",
    "figcaption",
    "address",
    "center",
    "caption",
    "form",
    "fieldset",
];

/// Читает HTML.
pub fn read(data: &[u8]) -> Document {
    let html = decode(data);
    let mut reader = Reader::new();
    for token in lexer::tokens(&html) {
        reader.token(token);
    }
    reader.finish()
}

/// Текст страницы: метка BOM, затем `charset` из начала файла, затем
/// UTF-8 (или однобайтная кодировка Windows, если это не UTF-8).
fn decode(data: &[u8]) -> String {
    if Encoding::for_bom(data).is_none() {
        let head = &data[..data.len().min(2048)];
        let head = String::from_utf8_lossy(head).to_ascii_lowercase();
        if let Some(at) = head.find("charset=") {
            let label: String = head[at + 8..]
                .trim_start_matches(['"', '\''])
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
                .collect();
            if let Some(encoding) = Encoding::for_label(label.as_bytes())
                && encoding != encoding_rs::UTF_8
                && !(encoding == encoding_rs::UTF_16LE || encoding == encoding_rs::UTF_16BE)
            {
                let (text, _) = encoding.decode_without_bom_handling(data);
                return text.into_owned();
            }
        }
    }
    crate::txt::decode(data)
}

/// Открытый список.
#[derive(Clone)]
struct List {
    ordered: bool,
    next: i32,
}

/// Открытый элемент и то, что наследует его содержимое.
#[derive(Clone)]
struct Frame {
    name: String,
    run: RunProps,
    /// Размер шрифта в половинах пункта (для `em` и `%`).
    size: u32,
    /// Пробелы и переводы строк сохраняются.
    pre: bool,
    /// Содержимое не показывается.
    hidden: bool,
    /// Оформление абзацев внутри.
    para: ParagraphProps,
    list: Option<List>,
}

struct Reader {
    builder: Builder,
    stack: Vec<Frame>,
    /// Внутри `<title>`.
    title: Option<String>,
    /// Сразу после `<pre>`: первый перевод строки не считается.
    after_pre: bool,
}

impl Reader {
    fn new() -> Reader {
        let builder = Builder::new();
        let root = Frame {
            name: String::new(),
            run: RunProps::default(),
            size: 22,
            pre: false,
            hidden: false,
            para: ParagraphProps::default(),
            list: None,
        };
        Reader {
            builder,
            stack: vec![root],
            title: None,
            after_pre: false,
        }
    }

    fn top(&self) -> &Frame {
        // Корень никогда не снимается со стека.
        &self.stack[self.stack.len() - 1]
    }

    fn token(&mut self, token: Token) {
        match token {
            Token::Start { name, attrs } => self.start(name, &attrs),
            Token::End(name) => self.end(&name),
            Token::Text(text) => self.text(&text),
        }
    }

    fn start(&mut self, name: String, attrs: &[(String, String)]) {
        let attr = |key: &str| {
            attrs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        if name == "title" && self.title.is_none() {
            self.title = Some(String::new());
        }
        let block = BLOCK.contains(&name.as_str());
        // Неявное закрытие: новый `<p>` закрывает открытый, `<li>` — пункт,
        // `<td>` — ячейку, `<tr>` — строку.
        match name.as_str() {
            "li" => self.close_open(&["li"], &["ul", "ol", "table"]),
            "dt" | "dd" => self.close_open(&["dt", "dd"], &["dl", "table"]),
            "td" | "th" => self.close_open(&["td", "th"], &["table", "tr"]),
            "tr" => self.close_open(&["tr", "td", "th"], &["table"]),
            "tbody" | "thead" | "tfoot" => self.close_open(&["tr", "td", "th"], &["table"]),
            _ if block || name == "table" => self.close_open(&["p"], &["td", "th", "li", "div"]),
            _ => {}
        }
        if name == "br" {
            self.line_break(attr("style").unwrap_or(""));
            return;
        }
        if name == "img" {
            if let Some(alt) = attr("alt").filter(|a| !a.trim().is_empty()) {
                let alt = alt.to_owned();
                self.text(&alt);
            }
            return;
        }
        if name == "hr" {
            self.builder.end_paragraph();
            return;
        }
        if VOID.contains(&name.as_str()) || self.stack.len() >= MAX_DEPTH {
            return;
        }

        let mut frame = self.top().clone();
        frame.name = name.clone();
        frame.list = None;
        if SKIP.contains(&name.as_str()) {
            frame.hidden = true;
        }
        self.element_defaults(&name, attrs, &mut frame);
        if let Some(style) = attr("style") {
            self.apply_css(
                style,
                &mut frame,
                block || matches!(name.as_str(), "td" | "th"),
            );
        }
        if attr("hidden").is_some() {
            frame.hidden = true;
        }

        match name.as_str() {
            "table" => self.builder.start_table(),
            "tr" => self.builder.start_row(),
            "td" | "th" => self.builder.start_cell(),
            _ => {}
        }
        if name == "pre" {
            self.after_pre = true;
        }
        let starts_paragraph = block || matches!(name.as_str(), "td" | "th");
        let paragraph = frame.para.clone();
        let marker = (name == "li").then(|| self.list_marker(attr("value")));
        self.stack.push(frame);
        if starts_paragraph && !self.top().hidden {
            self.builder.start_paragraph(paragraph);
        }
        if let Some(marker) = marker {
            let props = self.top().run.clone();
            self.builder.text(&marker, &props);
            self.builder.content(RunContent::Tab, &props);
        }
    }

    /// Закрывает открытый элемент из `names`, если он ближе, чем
    /// граница из `stop`.
    fn close_open(&mut self, names: &[&str], stop: &[&str]) {
        let found = self
            .stack
            .iter()
            .enumerate()
            .rev()
            .take_while(|(_, f)| !stop.contains(&f.name.as_str()))
            .find(|(_, f)| names.contains(&f.name.as_str()))
            .map(|(i, _)| i);
        if let Some(index) = found {
            while self.stack.len() > index {
                self.pop();
            }
        }
    }

    /// Номер или маркер пункта списка.
    fn list_marker(&mut self, value: Option<&str>) -> String {
        let list = self.stack.iter_mut().rev().find_map(|f| f.list.as_mut());
        match list {
            Some(list) if list.ordered => {
                if let Some(v) = value.and_then(|v| v.trim().parse().ok()) {
                    list.next = v;
                }
                let number = list.next;
                list.next = list.next.saturating_add(1);
                format!("{number}.")
            }
            _ => "•".to_owned(),
        }
    }

    /// Оформление, которое элемент даёт сам по себе (как в браузере).
    fn element_defaults(&self, name: &str, attrs: &[(String, String)], frame: &mut Frame) {
        let attr = |key: &str| {
            attrs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        let run = &mut frame.run;
        match name {
            "b" | "strong" => run.bold = Some(true),
            "i" | "em" | "cite" | "var" | "dfn" => run.italic = Some(true),
            "u" | "ins" => run.underline = Some(Underline::Single),
            "s" | "strike" | "del" => run.strike = Some(true),
            "sup" => run.vertical_align = Some(VerticalAlign::Superscript),
            "sub" => run.vertical_align = Some(VerticalAlign::Subscript),
            "code" | "kbd" | "samp" | "tt" => run.fonts = RunFonts::new(MONOSPACE),
            "mark" => run.highlight = Some(letter_model::Highlight::Yellow),
            "th" => run.bold = Some(true),
            // Ссылка: как в браузере, синяя и подчёркнутая.
            "a" => {
                if let Some(href) = attr("href").map(str::trim).filter(|h| !h.is_empty()) {
                    run.link = Some(match href.strip_prefix('#') {
                        Some(anchor) => letter_model::Hyperlink::to_bookmark(anchor),
                        None => letter_model::Hyperlink::to(href),
                    });
                    run.color = Some(letter_model::Color::Rgb(0x05, 0x63, 0xC1));
                    run.underline = Some(Underline::Single);
                }
            }
            "font" => {
                if let Some(face) = attr("face").and_then(css::font_family) {
                    run.fonts = RunFonts::new(face);
                }
                if let Some(color) = attr("color").and_then(css::color) {
                    run.color = Some(color);
                }
                if let Some(size) = attr("size").and_then(css::font_tag_size) {
                    run.size = Some(HalfPoints(size));
                    frame.size = size;
                }
            }
            _ => {}
        }

        let para = &mut frame.para;
        // Отступы и интервалы не наследуются: у каждого блока свои.
        if BLOCK.contains(&name) || matches!(name, "td" | "th" | "table") {
            para.spacing = Default::default();
            para.indentation.first_line = None;
            para.page_break_before = None;
        }
        if matches!(name, "td" | "th" | "table") {
            para.indentation = Default::default();
            para.style = None;
            para.alignment = None;
        }
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = name[1..].parse().unwrap_or(1);
                para.style = Some(styles::heading_style(level));
            }
            "p" | "div" => {
                if para.style.as_deref() != Some(QUOTE) {
                    para.style = None;
                }
            }
            "blockquote" => para.style = Some(QUOTE.to_owned()),
            "pre" => {
                para.style = Some(PREFORMATTED.to_owned());
                frame.pre = true;
            }
            "center" => para.alignment = Some(Alignment::Center),
            "ul" | "ol" => {
                let start = attr("start").and_then(|s| s.trim().parse().ok());
                frame.list = Some(List {
                    ordered: name == "ol",
                    next: start.unwrap_or(1),
                });
                let depth = self.stack.iter().filter(|f| f.list.is_some()).count() + 1;
                let depth = i32::try_from(depth).unwrap_or(1);
                para.style = Some(LIST.to_owned());
                para.indentation.left = Some(Twips(LIST_INDENT * depth));
                para.indentation.first_line = Some(FirstLineIndent::Hanging(Twips(360)));
            }
            "li" => {
                if para.style.as_deref() != Some(LIST) {
                    para.style = Some(LIST.to_owned());
                    para.indentation.left = Some(Twips(LIST_INDENT));
                }
                para.indentation.first_line = Some(FirstLineIndent::Hanging(Twips(360)));
            }
            "dd" => {
                let left = para.indentation.left.map_or(0, |t| t.0);
                para.indentation.left = Some(Twips(left + LIST_INDENT));
            }
            _ => {}
        }
        if let Some(align) = attr("align").and_then(alignment) {
            para.alignment = Some(align);
        }
        if let Some(class) = attr("class") {
            // Свой стиль, записанный классом (`class="Quote"`,
            // у Word — `class="MsoQuote"`).
            let found = class.split_whitespace().find_map(|c| {
                let id = c.strip_prefix("Mso").unwrap_or(c);
                let style = self.builder_doc_style(id)?;
                Some(style)
            });
            if let Some(style) = found {
                para.style = Some(style);
            }
        }
    }

    fn builder_doc_style(&self, id: &str) -> Option<String> {
        let style = self
            .builder
            .doc()
            .styles
            .styles
            .iter()
            .find(|s| s.kind == StyleKind::Paragraph && s.id.eq_ignore_ascii_case(id))?;
        Some(style.id.clone())
    }

    /// Применяет атрибут `style`.
    fn apply_css(&self, style: &str, frame: &mut Frame, block: bool) {
        let parent_size = frame.size;
        for (name, value) in css::declarations(style) {
            let lower = value.to_ascii_lowercase();
            let run = &mut frame.run;
            match name.as_str() {
                "font-weight" => {
                    let bold = match lower.as_str() {
                        "bold" | "bolder" => true,
                        "normal" | "lighter" => false,
                        number => number.parse::<u32>().is_ok_and(|w| w >= 600),
                    };
                    run.bold = Some(bold);
                }
                "font-style" => run.italic = Some(lower != "normal"),
                "text-decoration" | "text-decoration-line" => {
                    if lower.contains("underline") {
                        run.underline = Some(Underline::Single);
                    }
                    if lower.contains("line-through") {
                        run.strike = Some(true);
                    }
                    if lower == "none" {
                        run.underline = Some(Underline::None);
                        run.strike = Some(false);
                    }
                }
                "color" => {
                    if let Some(color) = css::color(&value) {
                        run.color = Some(color);
                    }
                }
                "background-color" | "background" | "mso-highlight" => {
                    if let Some(h) = css::highlight(&value) {
                        run.highlight = Some(h);
                    }
                }
                "font-size" => {
                    if let Some(size) = css::font_size(&value, parent_size) {
                        run.size = Some(HalfPoints(size));
                        frame.size = size;
                    }
                }
                "font-family" => {
                    if let Some(font) = css::font_family(&value) {
                        run.fonts = RunFonts::new(font);
                    }
                }
                "vertical-align" => {
                    run.vertical_align = match lower.as_str() {
                        "super" => Some(VerticalAlign::Superscript),
                        "sub" => Some(VerticalAlign::Subscript),
                        "baseline" => Some(VerticalAlign::Baseline),
                        _ => run.vertical_align,
                    }
                }
                "white-space" => {
                    frame.pre = matches!(
                        lower.as_str(),
                        "pre" | "pre-wrap" | "pre-line" | "break-spaces"
                    );
                }
                "display" | "visibility" if matches!(lower.as_str(), "none" | "hidden") => {
                    frame.hidden = true;
                }
                _ if block => self.apply_block_css(&name, &value, frame),
                _ => {}
            }
        }
    }

    fn apply_block_css(&self, name: &str, value: &str, frame: &mut Frame) {
        let size = frame.size;
        let para = &mut frame.para;
        let length = |v: &str| css::length(v, size);
        match name {
            "text-align" => {
                if let Some(align) = alignment(value) {
                    para.alignment = Some(align);
                }
            }
            "margin-left" => {
                if let Some(v) = length(value) {
                    para.indentation.left = Some(Twips(v));
                }
            }
            "margin-right" => {
                if let Some(v) = length(value) {
                    para.indentation.right = Some(Twips(v));
                }
            }
            "margin-top" => para.spacing.before = length(value).map(|v| Twips(v.max(0))),
            "margin-bottom" => para.spacing.after = length(value).map(|v| Twips(v.max(0))),
            "margin" => {
                // `margin: верх [право [низ [лево]]]`.
                let parts: Vec<Option<i32>> = value.split_whitespace().map(length).collect();
                let (top, right, bottom, left) = match parts[..] {
                    [a] => (a, a, a, a),
                    [a, b] => (a, b, a, b),
                    [a, b, c] => (a, b, c, b),
                    [a, b, c, d, ..] => (a, b, c, d),
                    [] => return,
                };
                para.spacing.before = top.map(|v| Twips(v.max(0)));
                para.spacing.after = bottom.map(|v| Twips(v.max(0)));
                para.indentation.left = left.map(Twips).or(para.indentation.left);
                para.indentation.right = right.map(Twips).or(para.indentation.right);
            }
            "text-indent" => {
                para.indentation.first_line = length(value).map(|v| {
                    if v < 0 {
                        FirstLineIndent::Hanging(Twips(-v))
                    } else {
                        FirstLineIndent::FirstLine(Twips(v))
                    }
                });
            }
            "line-height" => {
                let lower = value.trim().to_ascii_lowercase();
                para.spacing.line = if lower == "normal" {
                    Some(LineSpacing::SINGLE)
                } else if let Ok(factor) = lower.parse::<f64>() {
                    Some(LineSpacing::multiple(factor))
                } else if let Some(percent) = lower.strip_suffix('%') {
                    percent
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .map(|p| LineSpacing::multiple(p / 100.0))
                } else {
                    length(&lower).map(|v| LineSpacing::Exact(Twips(v.max(0))))
                };
            }
            "mso-line-height-rule" if value.trim().eq_ignore_ascii_case("at-least") => {
                if let Some(LineSpacing::Exact(v)) = para.spacing.line {
                    para.spacing.line = Some(LineSpacing::AtLeast(v));
                }
            }
            "page-break-before" | "break-before" => {
                let value = value.trim().to_ascii_lowercase();
                para.page_break_before = Some(matches!(value.as_str(), "always" | "page"));
            }
            _ => {}
        }
    }

    fn line_break(&mut self, style: &str) {
        if self.top().hidden {
            return;
        }
        let page = css::declarations(style).iter().any(|(name, value)| {
            matches!(name.as_str(), "page-break-before" | "break-before")
                && matches!(value.to_ascii_lowercase().as_str(), "always" | "page")
        });
        let props = self.top().run.clone();
        if page {
            self.builder
                .content(RunContent::Break(BreakKind::Page), &props);
        } else {
            self.builder.line_break(&props);
        }
    }

    fn end(&mut self, name: &str) {
        let Some(index) = self.stack.iter().rposition(|f| f.name == name) else {
            if name == "p" {
                // `</p>` без `<p>` браузер считает пустым абзацем.
                self.builder.end_paragraph();
            }
            return;
        };
        if index == 0 {
            return;
        }
        while self.stack.len() > index {
            self.pop();
        }
    }

    /// Снимает верхний элемент и завершает его абзац или таблицу.
    fn pop(&mut self) {
        if self.stack.len() <= 1 {
            return;
        }
        let Some(frame) = self.stack.pop() else {
            return;
        };
        if frame.name == "title"
            && let Some(title) = self.title.as_mut()
        {
            let text = std::mem::take(title);
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if !text.is_empty() && self.builder.doc_mut().properties.core.title.is_none() {
                self.builder.doc_mut().properties.core.title = Some(text);
            }
        }
        let block = BLOCK.contains(&frame.name.as_str());
        let parent = self.top().para.clone();
        match frame.name.as_str() {
            "td" | "th" => self.builder.end_cell(),
            "tr" => self.builder.end_row(),
            "table" => {
                self.builder.end_table();
                self.builder.start_paragraph(parent);
            }
            _ if block && !frame.hidden => {
                if self.builder.drop_trailing_line_break() {
                    self.builder.empty_paragraph();
                }
                self.builder.start_paragraph(parent);
            }
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        let frame = self.top();
        if frame.name == "title" {
            if let Some(title) = self.title.as_mut() {
                title.push_str(text);
            }
            return;
        }
        if frame.hidden {
            return;
        }
        let props = frame.run.clone();
        if frame.pre {
            let mut text = text;
            if std::mem::take(&mut self.after_pre) {
                text = text
                    .strip_prefix("\r\n")
                    .or(text.strip_prefix('\n'))
                    .unwrap_or(text);
            }
            let para = self.top().para.clone();
            let text = text.replace("\r\n", "\n");
            for (i, line) in text.split('\n').enumerate() {
                if i > 0 {
                    self.builder.empty_paragraph();
                    self.builder.start_paragraph(para.clone());
                }
                for (j, piece) in line.split('\t').enumerate() {
                    if j > 0 {
                        self.builder.content(RunContent::Tab, &props);
                    }
                    self.builder.text(piece, &props);
                }
            }
            return;
        }
        self.after_pre = false;
        if self.builder.in_table() && !self.builder.has_content() && text.trim().is_empty() {
            return;
        }
        let mut collapsed = String::with_capacity(text.len());
        let mut space = matches!(self.builder.last_char(), None | Some(' ' | '\n'));
        for c in text.chars() {
            if matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}') {
                if !space {
                    collapsed.push(' ');
                    space = true;
                }
            } else {
                collapsed.push(c);
                space = false;
            }
        }
        self.builder.text(&collapsed, &props);
    }

    fn finish(mut self) -> Document {
        while self.stack.len() > 1 {
            self.pop();
        }
        self.builder.finish()
    }
}

fn alignment(value: &str) -> Option<Alignment> {
    match value.trim().to_ascii_lowercase().as_str() {
        "left" | "start" => Some(Alignment::Left),
        "right" | "end" => Some(Alignment::Right),
        "center" | "middle" => Some(Alignment::Center),
        "justify" => Some(Alignment::Justify),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
