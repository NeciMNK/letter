//! Галерея «Стили» на вкладке «Главная»: какие стили показать, в каком
//! порядке и под какими названиями.

use eframe::egui::Color32;
use letter_model::{Color, Document, ParagraphProps, RunProps, Style, StyleKind};

/// Стиль в галерее.
#[derive(Debug, Clone, PartialEq)]
pub struct GalleryStyle {
    /// Идентификатор стиля.
    pub id: String,
    /// Название для пользователя (встроенные — по-русски).
    pub name: String,
    /// Образец: полужирный.
    pub bold: bool,
    /// Образец: курсив.
    pub italic: bool,
    /// Образец: цвет.
    pub color: Color32,
    /// Образец: размер шрифта, пт.
    pub size: f32,
}

/// Встроенные стили Word, которые показываются в галерее даже без
/// признака `w:qFormat` (например, в документах из TXT и HTML), в порядке
/// галереи Word, и их русские названия.
const BUILT_IN: [(&str, &str); 16] = [
    ("normal", "Обычный"),
    ("no spacing", "Без интервала"),
    ("heading 1", "Заголовок 1"),
    ("heading 2", "Заголовок 2"),
    ("title", "Заголовок"),
    ("subtitle", "Подзаголовок"),
    ("subtle emphasis", "Слабое выделение"),
    ("emphasis", "Выделение"),
    ("intense emphasis", "Сильное выделение"),
    ("strong", "Строгий"),
    ("quote", "Цитата"),
    ("intense quote", "Выделенная цитата"),
    ("subtle reference", "Слабая ссылка"),
    ("intense reference", "Сильная ссылка"),
    ("book title", "Название книги"),
    ("list paragraph", "Абзац списка"),
];

/// Имя стиля из файла (`heading 1`), а если его нет — идентификатор.
fn file_name(style: &Style) -> &str {
    style.name.as_deref().unwrap_or(&style.id)
}

/// Название стиля для пользователя: встроенные стили Word — по-русски
/// («heading 3» → «Заголовок 3»), остальные — как в файле.
pub fn display_name(style: &Style) -> String {
    let name = file_name(style);
    let lower = name.to_lowercase();
    if let Some((_, russian)) = BUILT_IN.iter().find(|(english, _)| *english == lower) {
        return (*russian).to_owned();
    }
    match lower.strip_prefix("heading ") {
        Some(level) if level.parse::<u8>().is_ok() => format!("Заголовок {level}"),
        _ => name.to_owned(),
    }
}

/// Стили для галереи: стили абзацев и символов с признаком «показывать
/// в галерее» или встроенные из списка Word, кроме скрытых. Порядок:
/// стиль по умолчанию, потом по `w:uiPriority`, потом как в Word, потом
/// как в файле.
pub fn gallery(doc: &Document) -> Vec<GalleryStyle> {
    let mut chosen: Vec<(usize, &Style)> = doc
        .styles
        .styles
        .iter()
        .enumerate()
        .filter(|(_, s)| matches!(s.kind, StyleKind::Paragraph | StyleKind::Character))
        .filter(|(_, s)| !s.is_hidden() && (s.is_quick() || built_in_index(s).is_some()))
        .collect();
    chosen.sort_by_key(|&(order, s)| {
        (
            !s.is_default,
            s.ui_priority(),
            built_in_index(s).unwrap_or(usize::MAX),
            order,
        )
    });
    chosen.dedup_by(|a, b| a.1.id == b.1.id);
    chosen
        .into_iter()
        .map(|(_, style)| sample(doc, style))
        .collect()
}

fn built_in_index(style: &Style) -> Option<usize> {
    let lower = file_name(style).to_lowercase();
    BUILT_IN.iter().position(|(english, _)| *english == lower)
}

/// Как выглядит текст в этом стиле.
fn sample(doc: &Document, style: &Style) -> GalleryStyle {
    let (paragraph, run) = match style.kind {
        StyleKind::Character => (
            ParagraphProps::default(),
            RunProps {
                style: Some(style.id.clone()),
                ..RunProps::default()
            },
        ),
        _ => (
            ParagraphProps {
                style: Some(style.id.clone()),
                ..ParagraphProps::default()
            },
            RunProps::default(),
        ),
    };
    let resolved = doc.resolve_run(&paragraph, &run);
    GalleryStyle {
        id: style.id.clone(),
        name: display_name(style),
        bold: resolved.bold,
        italic: resolved.italic,
        color: match resolved.color {
            Color::Auto => Color32::BLACK,
            Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        },
        size: resolved.size.0 as f32 / 2.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::RawXml;

    #[test]
    fn new_document_gallery_is_in_word_order() {
        let doc = crate::files::blank();
        let names: Vec<String> = gallery(&doc).into_iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            [
                "Обычный",
                "Заголовок 1",
                "Заголовок 2",
                "Заголовок",
                "Цитата",
                "Абзац списка"
            ]
        );
        let heading = gallery(&doc)
            .into_iter()
            .find(|s| s.id == "Heading1")
            .unwrap();
        assert!(heading.size > 11.0 && heading.color != Color32::BLACK);
    }

    #[test]
    fn quick_hidden_and_priority() {
        let mut doc = crate::files::blank();
        let mut mine = Style::new("Mine", StyleKind::Paragraph);
        mine.unknown = vec![
            RawXml::new("<w:qFormat/>"),
            RawXml::new(r#"<w:uiPriority w:val="1"/>"#),
        ];
        let mut hidden = Style::new("Hidden", StyleKind::Paragraph);
        hidden.unknown = vec![RawXml::new("<w:qFormat/>"), RawXml::new("<w:hidden/>")];
        let table = Style::new("Grid", StyleKind::Table);
        doc.styles.styles.extend([mine, hidden, table]);
        let ids: Vec<String> = gallery(&doc).into_iter().map(|s| s.id).collect();
        assert_eq!(ids[..2], ["Normal".to_string(), "Mine".to_string()]);
        assert!(!ids.iter().any(|id| id == "Hidden" || id == "Grid"));
        let mut heading = Style::new("H9", StyleKind::Paragraph);
        heading.name = Some("heading 9".into());
        assert_eq!(display_name(&heading), "Заголовок 9");
    }
}
