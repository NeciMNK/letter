//! Как видны исправления («Все исправления» в Word): вставленный текст
//! подчёркнут, удалённый — зачёркнут, и то и другое — цветом автора.

use letter_model::{Color, ResolvedRunProps, Revision, RevisionKind, Underline};

/// Цвета авторов исправлений по порядку.
const AUTHOR_COLORS: [Color; 6] = [
    Color::Rgb(0xB7, 0x1C, 0x1C),
    Color::Rgb(0x1F, 0x4E, 0x9A),
    Color::Rgb(0x1E, 0x7D, 0x32),
    Color::Rgb(0x7B, 0x1F, 0xA2),
    Color::Rgb(0xC2, 0x5B, 0x00),
    Color::Rgb(0x00, 0x83, 0x8F),
];

/// Цвет исправлений автора `author`: у одного автора всегда один цвет.
pub fn revision_color(author: &str) -> Color {
    let sum = author
        .chars()
        .fold(0u32, |sum, c| sum.wrapping_mul(31).wrapping_add(c as u32));
    AUTHOR_COLORS[sum as usize % AUTHOR_COLORS.len()]
}

/// Оформление текста с исправлением `revision`.
pub fn mark_revision(props: &mut ResolvedRunProps, revision: &Revision) {
    let color = revision_color(&revision.author);
    props.color = color;
    match revision.kind {
        RevisionKind::Insert => {
            props.underline = Underline::Single;
            props.underline_color = Some(color);
        }
        RevisionKind::Delete => props.strike = true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insertions_are_underlined_and_deletions_struck() {
        let revision = |kind| Revision {
            kind,
            id: "1".into(),
            author: "Анна".into(),
            date: None,
        };
        let mut props =
            letter_model::Document::new().resolve_run(&Default::default(), &Default::default());
        mark_revision(&mut props, &revision(RevisionKind::Insert));
        assert_eq!(props.underline, Underline::Single);
        assert_eq!(props.color, revision_color("Анна"));
        let mut props =
            letter_model::Document::new().resolve_run(&Default::default(), &Default::default());
        mark_revision(&mut props, &revision(RevisionKind::Delete));
        assert!(props.strike);
        assert_eq!(revision_color("Анна"), revision_color("Анна"));
    }
}
