//! Стили новых документов и узнавание заголовков.

use letter_model::{
    Alignment, Color, Document, HalfPoints, LineSpacing, Paragraph, ParagraphProps, RunFonts,
    RunProps, Spacing, Style, StyleKind, Twips,
};

/// Шрифт обычного текста. Как в Word 2013–2019: шрифт Aptos из
/// Word 2024 есть не везде, а Calibri заменяется метрически
/// совместимым Carlito.
const BODY_FONT: &str = "Calibri";
const HEADING_FONT: &str = "Calibri Light";

/// Пустой документ (без абзацев) со стилями, как у нового документа Word.
pub fn base_document() -> Document {
    let mut doc = Document::new();
    doc.sections[0].blocks.clear();
    let defaults = &mut doc.styles.defaults;
    defaults.run = RunProps {
        fonts: RunFonts::new(BODY_FONT),
        size: Some(HalfPoints(22)),
        ..RunProps::default()
    };
    defaults.paragraph = ParagraphProps {
        spacing: Spacing {
            before: None,
            after: Some(Twips(160)),
            line: Some(LineSpacing::Multiple(259)),
        },
        ..ParagraphProps::default()
    };

    let mut normal = Style::new("Normal", StyleKind::Paragraph);
    normal.name = Some("Normal".into());
    normal.is_default = true;
    let mut chars = Style::new("DefaultParagraphFont", StyleKind::Character);
    chars.name = Some("Default Paragraph Font".into());
    chars.is_default = true;
    doc.styles.styles = vec![normal, chars];

    let blue = Color::Rgb(0x2F, 0x54, 0x96);
    let dark_blue = Color::Rgb(0x1F, 0x37, 0x63);
    // Размер (в половинах пункта), интервал перед, курсив, цвет.
    let headings = [
        (32, 240, false, blue),
        (26, 40, false, blue),
        (24, 40, false, dark_blue),
        (22, 40, true, blue),
        (22, 40, false, blue),
        (22, 40, false, dark_blue),
    ];
    for (level, (size, before, italic, color)) in (1..).zip(headings) {
        let mut style = paragraph_style(&format!("Heading{level}"), &format!("heading {level}"));
        style.paragraph.spacing.before = Some(Twips(before));
        style.paragraph.spacing.after = Some(Twips(0));
        style.paragraph.keep_next = Some(true);
        style.paragraph.keep_lines = Some(true);
        style.run = RunProps {
            fonts: RunFonts::new(HEADING_FONT),
            size: Some(HalfPoints(size)),
            italic: italic.then_some(true),
            color: Some(color),
            ..RunProps::default()
        };
        doc.styles.styles.push(style);
    }
    let mut title = paragraph_style("Title", "Title");
    title.paragraph.spacing.after = Some(Twips(0));
    title.paragraph.spacing.line = Some(LineSpacing::SINGLE);
    title.paragraph.contextual_spacing = Some(true);
    title.run = RunProps {
        fonts: RunFonts::new(HEADING_FONT),
        size: Some(HalfPoints(56)),
        ..RunProps::default()
    };
    doc.styles.styles.push(title);

    let mut quote = paragraph_style(QUOTE, "Quote");
    quote.paragraph.alignment = Some(Alignment::Center);
    quote.paragraph.indentation.left = Some(Twips(864));
    quote.paragraph.indentation.right = Some(Twips(864));
    quote.paragraph.spacing.before = Some(Twips(200));
    quote.run.italic = Some(true);
    quote.run.color = Some(Color::Rgb(0x40, 0x40, 0x40));
    doc.styles.styles.push(quote);

    let mut list = paragraph_style(LIST, "List Paragraph");
    list.paragraph.indentation.left = Some(Twips(720));
    list.paragraph.contextual_spacing = Some(true);
    doc.styles.styles.push(list);

    let mut code = paragraph_style(PREFORMATTED, "HTML Preformatted");
    code.paragraph.spacing.after = Some(Twips(0));
    code.paragraph.spacing.line = Some(LineSpacing::SINGLE);
    code.run.fonts = RunFonts::new(MONOSPACE);
    code.run.size = Some(HalfPoints(20));
    doc.styles.styles.push(code);
    doc
}

/// Стиль цитаты (`<blockquote>`, `>` в Markdown).
pub const QUOTE: &str = "Quote";
/// Стиль пункта списка.
pub const LIST: &str = "ListParagraph";
/// Стиль готового текста (`<pre>`, блок кода в Markdown).
pub const PREFORMATTED: &str = "HTMLPreformatted";
/// Моноширинный шрифт для кода.
pub const MONOSPACE: &str = "Courier New";

/// Шрифт моноширинный (для `<code>` и `` ` `` в Markdown).
pub fn is_monospace(font: &str) -> bool {
    let font = font.to_ascii_lowercase();
    [
        "courier",
        "consolas",
        "mono",
        "menlo",
        "monaco",
        "lucida console",
    ]
    .iter()
    .any(|m| font.contains(m))
}

/// Идентификатор стиля абзаца после всех замен (стиль по умолчанию,
/// если не задан).
pub fn effective_style(doc: &Document, paragraph: &Paragraph) -> Option<String> {
    doc.resolve_paragraph(&paragraph.props).style
}

fn paragraph_style(id: &str, name: &str) -> Style {
    let mut style = Style::new(id, StyleKind::Paragraph);
    style.name = Some(name.into());
    style.based_on = Some("Normal".into());
    style.next = Some("Normal".into());
    style
}

/// Идентификатор стиля заголовка уровня `level` (1–6).
pub fn heading_style(level: u8) -> String {
    format!("Heading{}", level.clamp(1, 6))
}

/// Уровень заголовка абзаца (1–6) или `None` для обычного абзаца.
/// «Название» считается заголовком первого уровня, уровни глубже
/// шестого — шестым.
pub fn heading_level(doc: &Document, paragraph: &Paragraph) -> Option<u8> {
    let style = doc.resolve_paragraph(&paragraph.props).style?;
    let name = doc
        .styles
        .get(&style)
        .and_then(|s| s.name.clone())
        .unwrap_or_else(|| style.clone())
        .to_ascii_lowercase()
        .replace(' ', "");
    if name == "title" {
        return Some(1);
    }
    let level: u8 = name.strip_prefix("heading")?.parse().ok()?;
    (1..=9).contains(&level).then_some(level.min(6))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_are_recognised() {
        let doc = base_document();
        let heading = |id: &str| {
            Paragraph::text("x").with_props(ParagraphProps {
                style: Some(id.into()),
                ..ParagraphProps::default()
            })
        };
        assert_eq!(heading_level(&doc, &heading("Heading3")), Some(3));
        assert_eq!(heading_level(&doc, &heading("Title")), Some(1));
        assert_eq!(heading_level(&doc, &heading("Normal")), None);
        assert_eq!(heading_level(&doc, &Paragraph::text("x")), None);
        let props = doc.resolve_run(&heading("Heading1").props, &RunProps::default());
        assert_eq!(props.size, HalfPoints(32));
        assert_eq!(props.fonts.ascii, HEADING_FONT);
    }
}
