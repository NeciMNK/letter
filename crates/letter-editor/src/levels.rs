//! Уровни структуры абзацев: «Повысить уровень», «Понизить уровень»,
//! «Обычный текст» в режиме структуры, как в Word. Уровень меняется
//! стилем: заголовок уровня N — стиль «Заголовок N» (если его нет
//! в документе, он добавляется).

use letter_model::{Document, HalfPoints, RawXml, StyleKind};

use crate::edit::{paragraph_mut, paragraph_ref};
use crate::notes::ensure_style;
use crate::position::Position;
use crate::toc::outline_level;

/// Как поменять уровень.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineChange {
    /// На уровень выше (Заголовок 2 → Заголовок 1). Обычный текст
    /// становится заголовком уровня ближайшего заголовка над ним.
    Promote,
    /// На уровень ниже (до 9). Обычный текст не меняется.
    Demote,
    /// Ровно этот уровень, 1–9.
    Level(u8),
    /// Обычный текст.
    Body,
}

/// Стиль заголовка уровня `level` (1–9): имеющийся «heading N» или новый.
fn heading_style(doc: &mut Document, level: u8) -> String {
    ensure_style(
        doc,
        &format!("Heading{level}"),
        &format!("heading {level}"),
        StyleKind::Paragraph,
        |style| {
            style.paragraph.unknown = vec![RawXml::new(format!(
                r#"<w:outlineLvl w:val="{}"/>"#,
                level - 1
            ))];
            style.paragraph.keep_next = Some(true);
            style.run.bold = Some(true);
            style.run.size = Some(HalfPoints(match level {
                1 => 32,
                2 => 26,
                _ => 24,
            }));
        },
    )
}

/// Меняет уровень абзацев от `start` до `end`.
pub fn set_outline_level(
    doc: &mut Document,
    start: Position,
    end: Position,
    change: OutlineChange,
) {
    let (first, last) = if start.paragraph <= end.paragraph {
        (start.paragraph, end.paragraph)
    } else {
        (end.paragraph, start.paragraph)
    };
    for index in first..=last {
        let Some(paragraph) = paragraph_ref(doc, index) else {
            continue;
        };
        let current = outline_level(doc, &paragraph.props);
        let wanted = match (change, current) {
            (OutlineChange::Level(level), _) => Some(level.clamp(1, 9)),
            (OutlineChange::Body, _) => None,
            (OutlineChange::Promote, Some(level)) => Some(level.saturating_sub(1).max(1)),
            (OutlineChange::Promote, None) => Some(
                (0..index)
                    .rev()
                    .filter_map(|i| paragraph_ref(doc, i))
                    .find_map(|p| outline_level(doc, &p.props))
                    .unwrap_or(1),
            ),
            (OutlineChange::Demote, Some(level)) => Some((level + 1).min(9)),
            (OutlineChange::Demote, None) => None,
        };
        if wanted == current {
            continue;
        }
        let style = match wanted {
            Some(level) => Some(heading_style(doc, level)),
            None => doc
                .styles
                .default_for(StyleKind::Paragraph)
                .map(|s| s.id.clone()),
        };
        if let Some(paragraph) = paragraph_mut(doc, index) {
            paragraph.props.style = style;
            // Свой уровень абзаца больше не нужен: уровень — от стиля.
            paragraph
                .props
                .unknown
                .retain(|r| r.local_name() != "outlineLvl");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::{Paragraph, ParagraphProps};

    fn level(doc: &Document, index: usize) -> Option<u8> {
        outline_level(doc, &paragraph_ref(doc, index).unwrap().props)
    }

    #[test]
    fn levels_go_up_and_down() {
        let mut doc = Document::new();
        let heading = |text: &str| {
            Paragraph::text(text).with_props(ParagraphProps {
                style: Some("Heading1".into()),
                ..ParagraphProps::default()
            })
        };
        doc.sections[0].blocks = vec![heading("A").into(), Paragraph::text("text").into()];
        let at = |i| Position::new(i, 0);
        // Стиля «Заголовок 1» нет — он добавляется.
        set_outline_level(&mut doc, at(0), at(0), OutlineChange::Level(1));
        assert_eq!(level(&doc, 0), Some(1));
        set_outline_level(&mut doc, at(0), at(0), OutlineChange::Demote);
        assert_eq!(level(&doc, 0), Some(2));
        assert!(doc.styles.get("Heading2").is_some());
        set_outline_level(&mut doc, at(0), at(0), OutlineChange::Promote);
        set_outline_level(&mut doc, at(0), at(0), OutlineChange::Promote);
        assert_eq!(level(&doc, 0), Some(1));
        // Обычный текст поднимается до уровня заголовка над ним.
        set_outline_level(&mut doc, at(1), at(1), OutlineChange::Promote);
        assert_eq!(level(&doc, 1), Some(1));
        set_outline_level(&mut doc, at(0), at(1), OutlineChange::Body);
        assert_eq!(level(&doc, 0), None);
        assert_eq!(level(&doc, 1), None);
        set_outline_level(&mut doc, at(1), at(1), OutlineChange::Demote);
        assert_eq!(level(&doc, 1), None);
    }
}
