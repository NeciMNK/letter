//! Запись модели документа в RTF.
//!
//! Как Word: у каждого абзаца и фрагмента записывается итоговое
//! оформление полностью (с учётом стилей), а стили — в таблицу стилей
//! со ссылками `\sN`. Так файл одинаково выглядит в любой программе,
//! даже если она не понимает стили. Текст вне ASCII записывается через
//! `\uN` с заменой `?` для старых программ.

use std::fmt::Write as _;

use letter_model::{
    Alignment, Block, BreakKind, Color, Document, FirstLineIndent, Highlight, Inline, LineSpacing,
    Orientation, PageMargins, PageSize, Paragraph, ParagraphProps, ResolvedFonts, RunContent,
    RunFonts, RunProps, Section, Style, StyleKind, TabLeader, TabStop, TabStopKind, Table,
    Underline, VerticalAlign,
};

/// Шрифт, если у текста не задан никакой.
const FALLBACK_FONT: &str = "Times New Roman";

/// Записывает документ в RTF.
pub fn write(doc: &Document) -> String {
    let mut writer = Writer::new(doc);
    writer.collect(doc);
    let mut body = String::new();
    writer.sections(&mut body, doc);
    let mut out = String::new();
    writer.header(&mut out, doc);
    out.push_str(&body);
    out.push_str("}\r\n");
    out
}

/// Таблицы шрифтов, цветов и стилей, собранные по документу.
struct Writer {
    fonts: Vec<String>,
    colors: Vec<(u8, u8, u8)>,
    styles: Vec<(String, StyleKind)>,
}

impl Writer {
    fn new(doc: &Document) -> Writer {
        let default_font = resolved_font(
            &doc.resolve_run(&ParagraphProps::default(), &RunProps::default())
                .fonts,
        );
        Writer {
            fonts: vec![default_font],
            colors: Vec::new(),
            styles: Vec::new(),
        }
    }

    /// Номер шрифта в таблице (добавляет, если нет).
    fn font(&mut self, name: &str) -> usize {
        if let Some(i) = self.fonts.iter().position(|f| f == name) {
            return i;
        }
        self.fonts.push(name.to_owned());
        self.fonts.len() - 1
    }

    /// Номер цвета в таблице; 0 — «авто».
    fn color(&mut self, rgb: (u8, u8, u8)) -> usize {
        if let Some(i) = self.colors.iter().position(|c| *c == rgb) {
            return i + 1;
        }
        self.colors.push(rgb);
        self.colors.len()
    }

    /// Номер стиля в таблице стилей.
    fn style(&self, id: &str, kind: StyleKind) -> Option<usize> {
        self.styles.iter().position(|(s, k)| s == id && *k == kind)
    }

    /// Стили: «Обычный» — номер 0, как в Word.
    fn collect(&mut self, doc: &Document) {
        let mut ordered: Vec<&Style> = doc
            .styles
            .styles
            .iter()
            .filter(|s| matches!(s.kind, StyleKind::Paragraph | StyleKind::Character))
            .collect();
        ordered.sort_by_key(|s| !(s.kind == StyleKind::Paragraph && s.is_default));
        if !ordered
            .first()
            .is_some_and(|s| s.kind == StyleKind::Paragraph && s.is_default)
        {
            // Номер 0 занят «Обычным» стилем, даже если его нет.
            self.styles.push((String::new(), StyleKind::Paragraph));
        }
        for style in ordered {
            self.styles.push((style.id.clone(), style.kind));
        }
    }

    fn header(&mut self, out: &mut String, doc: &Document) {
        out.push_str("{\\rtf1\\ansi\\ansicpg1252\\deff0\\uc1");
        let tab = doc.settings.default_tab_stop.0;
        let _ = write!(out, "\\deftab{tab}");
        // Стили записываются раньше таблицы шрифтов, но им нужны её номера.
        let mut stylesheet = String::new();
        self.stylesheet(&mut stylesheet, doc);
        let defaults = self.run_props(&doc.styles.defaults.run);
        let para_defaults = paragraph_props_text(&doc.styles.defaults.paragraph);

        out.push_str("\r\n{\\fonttbl");
        for (i, name) in self.fonts.iter().enumerate() {
            let _ = write!(out, "{{\\f{i}\\fnil\\fcharset0 ");
            escape(out, name);
            out.push_str(";}");
        }
        out.push_str("}\r\n{\\colortbl;");
        for (r, g, b) in &self.colors {
            let _ = write!(out, "\\red{r}\\green{g}\\blue{b};");
        }
        out.push('}');
        let _ = write!(
            out,
            "\r\n{{\\*\\defchp{defaults}}}{{\\*\\defpap{para_defaults}}}"
        );
        out.push_str(&stylesheet);
        self.info(out, doc);
        if !doc.settings.add_paragraph_spacing {
            out.push_str("\\htmautsp");
        }
        if let Some(section) = doc.sections.first() {
            let (size, margins) = page(section);
            let _ = write!(
                out,
                "\\paperw{}\\paperh{}\\margl{}\\margr{}\\margt{}\\margb{}\\gutter{}",
                size.width.0,
                size.height.0,
                margins.left.0,
                margins.right.0,
                margins.top.0,
                margins.bottom.0,
                margins.gutter.0
            );
            if size.orientation == Orientation::Landscape {
                out.push_str("\\landscape");
            }
        }
        out.push_str("\r\n");
    }

    fn stylesheet(&mut self, out: &mut String, doc: &Document) {
        out.push_str("\r\n{\\stylesheet");
        let styles = self.styles.clone();
        for (number, (id, kind)) in styles.iter().enumerate() {
            let style = doc.styles.get(id).filter(|s| s.kind == *kind);
            let name = style
                .map(|s| s.name.clone().unwrap_or_else(|| s.id.clone()))
                .unwrap_or_else(|| "Normal".to_owned());
            out.push_str("\r\n{");
            match kind {
                StyleKind::Character => {
                    let _ = write!(out, "\\*\\cs{number}\\additive");
                }
                _ => {
                    let _ = write!(out, "\\s{number}");
                }
            }
            if let Some(style) = style {
                if let Some(parent) = style.based_on.as_deref().and_then(|p| self.style(p, *kind)) {
                    let _ = write!(out, "\\sbasedon{parent}");
                }
                if let Some(next) = style.next.as_deref().and_then(|p| self.style(p, *kind)) {
                    let _ = write!(out, "\\snext{next}");
                }
                if *kind == StyleKind::Paragraph {
                    out.push_str(&paragraph_props_text(&style.paragraph));
                }
                let run = self.run_props(&style.run);
                out.push_str(&run);
            }
            out.push(' ');
            escape(out, &name);
            out.push_str(";}");
        }
        out.push('}');
    }

    fn info(&self, out: &mut String, doc: &Document) {
        let core = &doc.properties.core;
        let fields = [
            ("title", &core.title),
            ("subject", &core.subject),
            ("author", &core.creator),
            ("keywords", &core.keywords),
            ("doccomm", &core.description),
            ("operator", &core.last_modified_by),
            ("category", &core.category),
        ];
        if fields.iter().all(|(_, v)| v.is_none()) {
            return;
        }
        out.push_str("\r\n{\\info");
        for (name, value) in fields {
            if let Some(value) = value {
                let _ = write!(out, "{{\\{name} ");
                escape(out, value);
                out.push('}');
            }
        }
        out.push('}');
    }

    fn sections(&mut self, out: &mut String, doc: &Document) {
        let count = doc.sections.len();
        for (i, section) in doc.sections.iter().enumerate() {
            let (size, margins) = page(section);
            let _ = write!(
                out,
                "\\sectd\\pgwsxn{}\\pghsxn{}\\marglsxn{}\\margrsxn{}\\margtsxn{}\\margbsxn{}\\guttersxn{}\\headery{}\\footery{}",
                size.width.0,
                size.height.0,
                margins.left.0,
                margins.right.0,
                margins.top.0,
                margins.bottom.0,
                margins.gutter.0,
                margins.header.0,
                margins.footer.0,
            );
            if size.orientation == Orientation::Landscape {
                out.push_str("\\lndscpsxn");
            }
            out.push_str("\r\n");
            let last_section = i + 1 == count;
            let blocks = &section.blocks;
            for (j, block) in blocks.iter().enumerate() {
                let last = last_section && j + 1 == blocks.len();
                self.block(out, doc, block, last);
            }
            if !last_section {
                out.push_str("\\sect\r\n");
            }
        }
    }

    fn block(&mut self, out: &mut String, doc: &Document, block: &Block, last: bool) {
        match block {
            Block::Paragraph(p) => {
                self.paragraph(out, doc, p, false);
                // Последний абзац документа — без `\par`, как у Word.
                if !last {
                    out.push_str("\\par\r\n");
                }
            }
            Block::Table(t) => self.table(out, doc, t),
            Block::Unknown(_) => {}
        }
    }

    fn table(&mut self, out: &mut String, doc: &Document, table: &Table) {
        // Ширины столбцов пока не хранятся: делим ширину текста поровну.
        let width = doc.sections.first().map_or(9000, |s| {
            let (size, margins) = page(s);
            (size.width.0 - margins.left.0 - margins.right.0).max(1440)
        });
        for row in &table.rows {
            let cells = row.cells.len().max(1) as i32;
            out.push_str("\\trowd\\trgaph108");
            for i in 1..=cells {
                let _ = write!(out, "\\cellx{}", width * i / cells);
            }
            out.push_str("\r\n");
            for cell in &row.cells {
                let paragraphs = cell_paragraphs(&cell.blocks);
                if paragraphs.is_empty() {
                    out.push_str("\\pard\\intbl");
                }
                for (k, p) in paragraphs.iter().enumerate() {
                    self.paragraph(out, doc, p, true);
                    if k + 1 < paragraphs.len() {
                        out.push_str("\\par\r\n");
                    }
                }
                out.push_str("\\cell\r\n");
            }
            out.push_str("\\row\r\n");
        }
    }

    /// Абзац без `\par` в конце.
    fn paragraph(&mut self, out: &mut String, doc: &Document, p: &Paragraph, in_table: bool) {
        let resolved = doc.resolve_paragraph(&p.props);
        out.push_str("\\pard\\plain");
        if let Some(n) = resolved
            .style
            .as_deref()
            .and_then(|s| self.style(s, StyleKind::Paragraph))
        {
            let _ = write!(out, "\\s{n}");
        }
        let explicit = ParagraphProps {
            style: None,
            numbering: None,
            alignment: Some(resolved.alignment),
            indentation: letter_model::Indentation {
                left: Some(resolved.indent_left),
                right: Some(resolved.indent_right),
                first_line: resolved.first_line,
            },
            spacing: letter_model::Spacing {
                before: Some(resolved.space_before),
                after: Some(resolved.space_after),
                line: Some(resolved.line_spacing),
            },
            contextual_spacing: Some(resolved.contextual_spacing),
            keep_next: Some(resolved.keep_next),
            keep_lines: Some(resolved.keep_lines),
            page_break_before: Some(resolved.page_break_before),
            widow_control: Some(resolved.widow_control),
            tabs: resolved.tabs.clone(),
            unknown: Vec::new(),
        };
        out.push_str(&paragraph_props_text(&explicit));
        if in_table {
            out.push_str("\\intbl");
        }
        for inline in &p.content {
            let Inline::Run(run) = inline else {
                continue;
            };
            let resolved = doc.resolve_run(&p.props, &run.props);
            let explicit = RunProps {
                style: run.props.style.clone(),
                fonts: RunFonts::new(resolved_font(&resolved.fonts)),
                size: Some(resolved.size),
                bold: Some(resolved.bold),
                italic: Some(resolved.italic),
                underline: Some(resolved.underline),
                strike: Some(resolved.strike),
                double_strike: Some(resolved.double_strike),
                color: Some(resolved.color),
                theme_color: None,
                highlight: Some(resolved.highlight),
                vertical_align: Some(resolved.vertical_align),
                ..RunProps::default()
            };
            out.push_str("{\\plain");
            if let Some(n) = explicit
                .style
                .as_deref()
                .and_then(|s| self.style(s, StyleKind::Character))
            {
                let _ = write!(out, "\\cs{n}");
            }
            let props = self.run_props(&explicit);
            out.push_str(&props);
            out.push(' ');
            for item in &run.content {
                match item {
                    RunContent::Text(t) => escape(out, t),
                    RunContent::Tab => out.push_str("\\tab "),
                    RunContent::Break(BreakKind::Line) => out.push_str("\\line "),
                    RunContent::Break(BreakKind::Page) => out.push_str("\\page "),
                    RunContent::Break(BreakKind::Column) => out.push_str("\\column "),
                    // Рисунки в RTF пока не записываются.
                    RunContent::Picture(_)
                    | RunContent::FieldChar(_)
                    | RunContent::FieldCode(_)
                    | RunContent::NoteReference(_)
                    | RunContent::NoteMark(_)
                    | RunContent::Unknown(_) => {}
                }
            }
            out.push('}');
        }
    }

    /// Оформление символов: только заданные поля.
    fn run_props(&mut self, props: &RunProps) -> String {
        let mut out = String::new();
        if let Some(name) = font_name(&props.fonts) {
            let n = self.font(name);
            let _ = write!(out, "\\f{n}");
        }
        if let Some(size) = props.size {
            let _ = write!(out, "\\fs{}", size.0);
        }
        toggle(&mut out, "b", props.bold);
        toggle(&mut out, "i", props.italic);
        toggle(&mut out, "strike", props.strike);
        toggle(&mut out, "striked", props.double_strike);
        if let Some(u) = props.underline {
            out.push_str(underline(u));
        }
        match props.color {
            Some(Color::Rgb(r, g, b)) => {
                let n = self.color((r, g, b));
                let _ = write!(out, "\\cf{n}");
            }
            Some(Color::Auto) => out.push_str("\\cf0"),
            None => {}
        }
        match props.highlight.map(|h| (h, h.rgb())) {
            Some((_, Some(rgb))) => {
                let n = self.color(rgb);
                let _ = write!(out, "\\highlight{n}");
            }
            Some((Highlight::None, None)) => out.push_str("\\highlight0"),
            _ => {}
        }
        match props.vertical_align {
            Some(VerticalAlign::Superscript) => out.push_str("\\super"),
            Some(VerticalAlign::Subscript) => out.push_str("\\sub"),
            Some(VerticalAlign::Baseline) => out.push_str("\\nosupersub"),
            None => {}
        }
        out
    }
}

/// Абзацы ячейки: вложенные таблицы пока разворачиваются в абзацы.
fn cell_paragraphs(blocks: &[Block]) -> Vec<&Paragraph> {
    let mut out = Vec::new();
    for block in blocks {
        match block {
            Block::Paragraph(p) => out.push(p),
            Block::Table(t) => {
                for cell in t.rows.iter().flat_map(|r| &r.cells) {
                    out.extend(cell_paragraphs(&cell.blocks));
                }
            }
            Block::Unknown(_) => {}
        }
    }
    out
}

/// Оформление абзаца: только заданные поля.
fn paragraph_props_text(props: &ParagraphProps) -> String {
    let mut out = String::new();
    if let Some(a) = props.alignment {
        out.push_str(match a {
            Alignment::Left => "\\ql",
            Alignment::Center => "\\qc",
            Alignment::Right => "\\qr",
            Alignment::Justify => "\\qj",
            Alignment::Distribute => "\\qd",
        });
    }
    let indent = &props.indentation;
    if let Some(left) = indent.left {
        let _ = write!(out, "\\li{}", left.0);
    }
    if let Some(right) = indent.right {
        let _ = write!(out, "\\ri{}", right.0);
    }
    match indent.first_line {
        Some(FirstLineIndent::FirstLine(t)) => {
            let _ = write!(out, "\\fi{}", t.0);
        }
        Some(FirstLineIndent::Hanging(t)) => {
            let _ = write!(out, "\\fi{}", t.0.saturating_neg());
        }
        None => {}
    }
    if let Some(before) = props.spacing.before {
        let _ = write!(out, "\\sb{}", before.0);
    }
    if let Some(after) = props.spacing.after {
        let _ = write!(out, "\\sa{}", after.0);
    }
    match props.spacing.line {
        Some(LineSpacing::Multiple(m)) => {
            let _ = write!(out, "\\sl{m}\\slmult1");
        }
        Some(LineSpacing::AtLeast(t)) => {
            let _ = write!(out, "\\sl{}\\slmult0", t.0);
        }
        Some(LineSpacing::Exact(t)) => {
            let _ = write!(out, "\\sl{}\\slmult0", t.0.saturating_neg());
        }
        None => {}
    }
    toggle(&mut out, "contextualspace", props.contextual_spacing);
    toggle(&mut out, "keepn", props.keep_next);
    toggle(&mut out, "keep", props.keep_lines);
    toggle(&mut out, "pagebb", props.page_break_before);
    match props.widow_control {
        Some(true) => out.push_str("\\widctlpar"),
        Some(false) => out.push_str("\\nowidctlpar"),
        None => {}
    }
    for tab in &props.tabs {
        tab_stop(&mut out, tab);
    }
    out
}

fn tab_stop(out: &mut String, tab: &TabStop) {
    let kind = match tab.kind {
        TabStopKind::Clear => return,
        TabStopKind::Left | TabStopKind::Bar => "",
        TabStopKind::Center => "\\tqc",
        TabStopKind::Right => "\\tqr",
        TabStopKind::Decimal => "\\tqdec",
    };
    let leader = match tab.leader {
        TabLeader::None => "",
        TabLeader::Dot => "\\tldot",
        TabLeader::MiddleDot => "\\tlmdot",
        TabLeader::Hyphen => "\\tlhyph",
        TabLeader::Underscore => "\\tlul",
        TabLeader::Heavy => "\\tlth",
    };
    let word = if tab.kind == TabStopKind::Bar {
        "tb"
    } else {
        "tx"
    };
    let _ = write!(out, "{kind}{leader}\\{word}{}", tab.position.0);
}

/// `\b` или `\b0`.
fn toggle(out: &mut String, word: &str, value: Option<bool>) {
    match value {
        Some(true) => {
            let _ = write!(out, "\\{word}");
        }
        Some(false) => {
            let _ = write!(out, "\\{word}0");
        }
        None => {}
    }
}

fn underline(u: Underline) -> &'static str {
    match u {
        Underline::None => "\\ulnone",
        Underline::Single => "\\ul",
        Underline::Words => "\\ulw",
        Underline::Double => "\\uldb",
        Underline::Thick => "\\ulth",
        Underline::Dotted => "\\uld",
        Underline::DottedHeavy => "\\ulthd",
        Underline::Dash => "\\uldash",
        Underline::DashedHeavy => "\\ulthdash",
        Underline::DashLong => "\\ulldash",
        Underline::DashLongHeavy => "\\ulthldash",
        Underline::DotDash => "\\uldashd",
        Underline::DashDotHeavy => "\\ulthdashd",
        Underline::DotDotDash => "\\uldashdd",
        Underline::DashDotDotHeavy => "\\ulthdashdd",
        Underline::Wave => "\\ulwave",
        Underline::WavyHeavy => "\\ulhwave",
        Underline::WavyDouble => "\\ululdbwave",
    }
}

/// Имя шрифта для `\f`: латиница, иначе прочие буквы.
fn font_name(fonts: &RunFonts) -> Option<&str> {
    fonts.ascii.as_deref().or(fonts.h_ansi.as_deref())
}

fn resolved_font(fonts: &ResolvedFonts) -> String {
    [&fonts.ascii, &fonts.h_ansi]
        .into_iter()
        .find(|f| !f.is_empty())
        .cloned()
        .unwrap_or_else(|| FALLBACK_FONT.to_owned())
}

/// Размер листа и поля раздела (по умолчанию — как в модели).
fn page(section: &Section) -> (PageSize, PageMargins) {
    (
        section.props.page_size.unwrap_or_default(),
        section.props.margins.unwrap_or_default(),
    )
}

/// Текст: `\`, `{`, `}` экранируются, знаки вне ASCII — `\uN?`.
fn escape(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '\\' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            '\t' => out.push_str("\\tab "),
            '\n' => out.push_str("\\line "),
            '\r' => {}
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            c => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    // `\u` — число со знаком: старшие значения отрицательные.
                    let _ = write!(out, "\\u{}?", *unit as i16);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_escaped() {
        let mut out = String::new();
        escape(&mut out, "a{b}\\ё😀");
        assert_eq!(out, "a\\{b\\}\\\\\\u1105?\\u-10179?\\u-8704?");
    }

    #[test]
    fn line_spacing_words() {
        let mut props = ParagraphProps::default();
        props.spacing.line = Some(LineSpacing::Exact(letter_model::Twips(300)));
        props.indentation.first_line = Some(FirstLineIndent::Hanging(letter_model::Twips(360)));
        assert_eq!(paragraph_props_text(&props), "\\fi-360\\sl-300\\slmult0");
    }
}
