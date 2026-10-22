//! Сноски: вставка ссылки на новую сноску и её текста.
//!
//! Текст сноски хранится в документе отдельно от текста
//! ([`Document::footnotes`], [`Document::endnotes`]), а в тексте стоит
//! только ссылка на неё. Удалённая ссылка оставляет сноску в документе
//! без ссылок: она не видна и не записывается в файл, а отмена удаления
//! возвращает её на место.

use letter_model::{
    Block, Document, HalfPoints, LineSpacing, Note, NoteKind, NoteReference, Paragraph,
    ParagraphProps, RawXml, Run, RunContent, RunProps, Spacing, Style, StyleKind, Twips,
    VerticalAlign,
};

use crate::edit::{clamp_offset, insert_items, item_len, paragraph_mut};
use crate::format::apply;
use crate::position::Position;

/// Стили сносок вида `kind`: текста сноски (абзаца) и знака сноски
/// (символов), как их называет Word.
pub fn note_styles(kind: NoteKind) -> (&'static str, &'static str) {
    match kind {
        NoteKind::Footnote => ("FootnoteText", "FootnoteReference"),
        NoteKind::Endnote => ("EndnoteText", "EndnoteReference"),
    }
}

/// Сноски документа вида `kind`.
pub fn notes(doc: &Document, kind: NoteKind) -> &[Note] {
    match kind {
        NoteKind::Footnote => &doc.footnotes,
        NoteKind::Endnote => &doc.endnotes,
    }
}

/// Сноски документа вида `kind` для правки.
pub fn notes_mut(doc: &mut Document, kind: NoteKind) -> &mut Vec<Note> {
    match kind {
        NoteKind::Footnote => &mut doc.footnotes,
        NoteKind::Endnote => &mut doc.endnotes,
    }
}

/// Вставляет в место `at` ссылку на новую сноску вида `kind` и создаёт
/// саму сноску: абзац со стилем текста сноски, в начале — её номер и
/// пробел. Возвращает место после ссылки и номер новой сноски.
pub fn insert_note(doc: &mut Document, at: Position, kind: NoteKind) -> (Position, String) {
    let (text_style, mark_style) = note_styles(kind);
    let text_style = ensure_style(doc, text_style, StyleKind::Paragraph);
    let mark_style = ensure_style(doc, mark_style, StyleKind::Character);
    let list = notes_mut(doc, kind);
    if list.iter().all(|n| n.special.is_none()) {
        list.insert(0, separator("-1", "separator"));
        list.insert(1, separator("0", "continuationSeparator"));
    }
    let id = list
        .iter()
        .filter_map(|n| n.id.parse::<i64>().ok())
        .max()
        .unwrap_or(0)
        .max(0)
        + 1;
    let id = id.to_string();
    let mark = Run {
        props: RunProps {
            style: Some(mark_style.clone()),
            ..RunProps::default()
        },
        content: vec![RunContent::NoteMark(kind)],
    };
    let text = Run {
        props: RunProps::default(),
        content: vec![RunContent::Text(" ".into())],
    };
    let mut paragraph = Paragraph::from_runs([mark, text]);
    paragraph.props.style = Some(text_style);
    list.push(Note {
        id: id.clone(),
        special: None,
        blocks: vec![Block::Paragraph(paragraph)],
    });

    let Some(paragraph) = paragraph_mut(doc, at.paragraph) else {
        return (at, id);
    };
    let offset = clamp_offset(paragraph, at.offset);
    let item = RunContent::NoteReference(NoteReference {
        kind,
        id: id.clone(),
        custom_mark: false,
    });
    let end = offset + item_len(&item);
    insert_items(paragraph, offset, vec![item]);
    apply(paragraph, offset, end, |props| {
        props.style = Some(mark_style.clone());
    });
    (Position::new(at.paragraph, end), id)
}

/// Служебная сноска-черта.
fn separator(id: &str, kind: &str) -> Note {
    let mut paragraph = Paragraph::from_runs([Run {
        props: RunProps::default(),
        content: vec![RunContent::Unknown(RawXml::new(format!("<w:{kind}/>")))],
    }]);
    paragraph.props = ParagraphProps {
        spacing: Spacing {
            after: Some(Twips(0)),
            line: Some(LineSpacing::Multiple(240)),
            ..Spacing::default()
        },
        ..ParagraphProps::default()
    };
    Note {
        id: id.to_string(),
        special: Some(kind.to_string()),
        blocks: vec![Block::Paragraph(paragraph)],
    }
}

/// Идентификатор стиля `id` (ищется и по имени, без учёта регистра);
/// если такого стиля нет, он добавляется с оформлением, как в Word: текст
/// сноски — 10 пт, одинарный интервал без интервала после; знак сноски —
/// надстрочный.
fn ensure_style(doc: &mut Document, id: &str, style_kind: StyleKind) -> String {
    let name = style_name(id);
    if let Some(style) = doc.styles.styles.iter().find(|s| {
        s.kind == style_kind
            && (s.id.eq_ignore_ascii_case(id)
                || s.name
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(&name)))
    }) {
        return style.id.clone();
    }
    let mut style = Style::new(id, style_kind);
    style.name = Some(name);
    match style_kind {
        StyleKind::Paragraph => {
            style.based_on = doc
                .styles
                .default_for(StyleKind::Paragraph)
                .map(|s| s.id.clone());
            style.paragraph.spacing = Spacing {
                after: Some(Twips(0)),
                line: Some(LineSpacing::Multiple(240)),
                ..Spacing::default()
            };
            style.run.size = Some(HalfPoints(20));
        }
        _ => style.run.vertical_align = Some(VerticalAlign::Superscript),
    }
    doc.styles.styles.push(style);
    id.to_string()
}

/// Имя встроенного стиля Word по идентификатору: `FootnoteText` →
/// `footnote text`.
fn style_name(id: &str) -> String {
    let mut name = String::new();
    for (i, c) in id.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            name.push(' ');
        }
        name.extend(c.to_lowercase());
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::plain_text;
    use crate::history::{Command, History};
    use crate::selection::Selection;

    fn doc() -> Document {
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![Paragraph::text("Claim.").into()];
        doc
    }

    #[test]
    fn note_is_inserted_with_its_styles() {
        let mut doc = doc();
        let (at, id) = insert_note(&mut doc, Position::new(0, 5), NoteKind::Footnote);
        assert_eq!(id, "1");
        assert_eq!(
            at,
            Position::new(0, 5 + letter_model::OBJECT_CHAR.len_utf8())
        );
        // Служебные черты и сама сноска.
        let specials: Vec<_> = doc.footnotes.iter().map(|n| n.special.as_deref()).collect();
        assert_eq!(
            specials,
            [Some("separator"), Some("continuationSeparator"), None]
        );
        let Some(Block::Paragraph(text)) = doc.footnotes[2].blocks.first() else {
            panic!("нет текста сноски");
        };
        assert_eq!(text.props.style.as_deref(), Some("FootnoteText"));
        assert!(doc.styles.get("FootnoteText").is_some());
        let mark = doc.styles.get("FootnoteReference");
        assert_eq!(
            mark.and_then(|s| s.run.vertical_align),
            Some(VerticalAlign::Superscript)
        );
        // Ссылка — в тексте, со стилем знака сноски.
        let Some(Block::Paragraph(p)) = doc.sections[0].blocks.first() else {
            panic!("нет абзаца");
        };
        let reference = p.runs().find(|r| {
            r.content
                .iter()
                .any(|c| matches!(c, RunContent::NoteReference(_)))
        });
        assert_eq!(
            reference.and_then(|r| r.props.style.as_deref()),
            Some("FootnoteReference")
        );
        assert_eq!(
            plain_text(p).replace(letter_model::OBJECT_CHAR, "^"),
            "Claim^."
        );
        // Следующая сноска получает следующий номер, черты не дублируются.
        let (_, id) = insert_note(&mut doc, at, NoteKind::Footnote);
        assert_eq!(id, "2");
        assert_eq!(doc.footnotes.len(), 4);
    }

    #[test]
    fn insert_note_command_can_be_undone() {
        let mut doc = doc();
        let mut history = History::new();
        let at = history.run(
            &mut doc,
            Selection::caret(Position::new(0, 6)),
            Command::InsertNote(NoteKind::Endnote),
        );
        assert_eq!(doc.endnotes.len(), 3);
        assert!(at.focus.offset > 6);
        history.undo(&mut doc);
        let Some(Block::Paragraph(p)) = doc.sections[0].blocks.first() else {
            panic!("нет абзаца");
        };
        assert_eq!(plain_text(p), "Claim.");
    }

    #[test]
    fn note_text_change_is_one_undo_step() {
        let mut doc = doc();
        let (_, id) = insert_note(&mut doc, Position::new(0, 0), NoteKind::Footnote);
        let mut history = History::new();
        let caret = Selection::caret(Position::new(0, 0));
        let blocks = vec![Paragraph::text("New text").into()];
        let after = history.run(
            &mut doc,
            caret,
            Command::SetNote {
                kind: NoteKind::Footnote,
                id: id.clone(),
                blocks: blocks.clone(),
            },
        );
        assert_eq!(after, caret);
        let note = |doc: &Document| doc.footnotes.iter().find(|n| n.id == id).cloned();
        assert_eq!(note(&doc).map(|n| n.blocks), Some(blocks.clone()));
        history.undo(&mut doc);
        assert_ne!(note(&doc).map(|n| n.blocks), Some(blocks.clone()));
        history.redo(&mut doc);
        assert_eq!(note(&doc).map(|n| n.blocks), Some(blocks));
    }
}
