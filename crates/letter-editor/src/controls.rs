//! Элементы управления содержимым внутри абзаца: флажки, списки, даты,
//! текст-подсказка и запрет правки.

use std::sync::Arc;

use letter_model::{
    ContentControl, ContentControlRef, ControlKind, Document, Inline, Paragraph, RunContent,
};

use crate::edit::{item_len, paragraph_mut, paragraph_ref};
use crate::fields::{DateTime, format_date};
use crate::position::Position;

/// Элемент внутри абзаца у места `at` и где он: начало и конец (в одном
/// абзаце). Смотрим знак после места, а в конце элемента — знак перед ним.
pub fn control_at(doc: &Document, at: Position) -> Option<(ContentControlRef, Position, Position)> {
    let paragraph = paragraph_ref(doc, at.paragraph)?;
    let segments = segments(paragraph);
    let index = segments
        .iter()
        .position(|(a, b, c)| *a <= at.offset && at.offset < *b && c.is_some())
        .or_else(|| {
            segments
                .iter()
                .position(|(a, b, c)| *a <= at.offset && at.offset == *b && c.is_some())
        })?;
    let control = segments[index].2.clone()?;
    let same = |s: &(usize, usize, Option<ContentControlRef>)| {
        s.2.as_ref().is_some_and(|c| Arc::ptr_eq(c, &control))
    };
    let mut first = index;
    while first > 0 && same(&segments[first - 1]) {
        first -= 1;
    }
    let mut last = index;
    while segments.get(last + 1).is_some_and(same) {
        last += 1;
    }
    let start = Position::new(at.paragraph, segments[first].0);
    let end = Position::new(at.paragraph, segments[last].1);
    Some((control, start, end))
}

/// Фрагменты абзаца: начало, конец (байты) и элемент управления.
fn segments(paragraph: &Paragraph) -> Vec<(usize, usize, Option<ContentControlRef>)> {
    let mut out = Vec::new();
    let mut offset = 0;
    for run in paragraph.runs() {
        let len: usize = run.content.iter().map(item_len).sum();
        out.push((offset, offset + len, run.props.control.clone()));
        offset += len;
    }
    out
}

/// Показывает ли элемент у места `at` текст-подсказку: тогда набранный
/// текст заменяет её (см. [`set_control_text`]).
pub fn placeholder_at(doc: &Document, at: Position) -> bool {
    control_at(doc, at).is_some_and(|(c, _, _)| c.showing_placeholder())
}

/// Можно ли править текст от `start` до `end`: ни он, ни абзацы вокруг
/// не лежат в элементе с запретом правки содержимого.
pub fn editable(doc: &Document, start: Position, end: Position) -> bool {
    for index in start.paragraph..=end.paragraph {
        let Some(paragraph) = paragraph_ref(doc, index) else {
            continue;
        };
        if paragraph
            .props
            .controls
            .iter()
            .any(|c| !c.content_editable())
        {
            return false;
        }
        let from = if index == start.paragraph {
            start.offset
        } else {
            0
        };
        let to = if index == end.paragraph {
            end.offset
        } else {
            usize::MAX
        };
        let locked = segments(paragraph).into_iter().any(|(a, b, c)| {
            c.is_some_and(|c| !c.content_editable())
                && (a < to && from < b || a <= from && from <= b && from == to)
        });
        if locked {
            return false;
        }
    }
    true
}

/// Заменяет текст элемента у места `at` на `text` (оформление — как у
/// первого фрагмента, без стиля подсказки) и снимает пометку
/// «показывает подсказку». Возвращает место после текста; `None`, если
/// элемента нет или его содержимое менять нельзя.
pub fn set_control_text(doc: &mut Document, at: Position, text: &str) -> Option<Position> {
    replace_control(doc, at, text, ContentControl::without_placeholder)
}

/// Ставит или снимает флажок у места `at`. Возвращает место после флажка;
/// `None`, если там не флажок.
pub fn toggle_checkbox(doc: &mut Document, at: Position) -> Option<Position> {
    let (control, _, _) = control_at(doc, at)?;
    let ControlKind::CheckBox { checked, symbols } = control.kind() else {
        return None;
    };
    let symbol = if checked {
        symbols.unchecked
    } else {
        symbols.checked
    };
    replace_control(doc, at, &symbol.to_string(), |c| {
        c.without_placeholder().with_checked(!checked)
    })
}

/// Выбирает пункт `index` списка у места `at`.
pub fn choose_list_item(doc: &mut Document, at: Position, index: usize) -> Option<Position> {
    let (control, _, _) = control_at(doc, at)?;
    let ControlKind::List { items, .. } = control.kind() else {
        return None;
    };
    let item = items.get(index)?;
    set_control_text(doc, at, &item.text)
}

/// Ставит дату `date` в элемент выбора даты у места `at`: текст по образцу
/// элемента (без образца — `dd.MM.yyyy`) и полную дату в свойствах.
pub fn set_control_date(doc: &mut Document, at: Position, date: DateTime) -> Option<Position> {
    let (control, _, _) = control_at(doc, at)?;
    let ControlKind::Date { format } = control.kind() else {
        return None;
    };
    let russian = control
        .property("lid")
        .is_none_or(|lid| lid.to_ascii_lowercase().starts_with("ru"));
    let text = format_date(format.as_deref().unwrap_or("dd.MM.yyyy"), date, russian);
    let full = format!(
        "{:04}-{:02}-{:02}T00:00:00Z",
        date.year, date.month, date.day
    );
    replace_control(doc, at, &text, |c| {
        c.without_placeholder().with_full_date(&full)
    })
}

/// Заменяет текст элемента у `at` и сам элемент (на `change` от него).
fn replace_control(
    doc: &mut Document,
    at: Position,
    text: &str,
    change: impl FnOnce(&ContentControl) -> ContentControl,
) -> Option<Position> {
    let (control, start, _) = control_at(doc, at)?;
    if !control.content_editable() {
        return None;
    }
    let placeholder_style = control.showing_placeholder();
    let new = Arc::new(change(&control));
    let paragraph = paragraph_mut(doc, at.paragraph)?;
    let mut first = true;
    paragraph.content.retain_mut(|inline| {
        let Inline::Run(run) = inline else {
            return true;
        };
        if !run
            .props
            .control
            .as_ref()
            .is_some_and(|c| Arc::ptr_eq(c, &control))
        {
            return true;
        }
        if !first {
            return false;
        }
        first = false;
        run.props.control = Some(new.clone());
        run.content = if text.is_empty() {
            Vec::new()
        } else {
            vec![RunContent::Text(text.to_string())]
        };
        // Подсказка у Word оформлена стилем «Текст заполнителя» (серым).
        if placeholder_style
            && run
                .props
                .style
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case("PlaceholderText"))
        {
            run.props.style = None;
        }
        true
    });
    Some(Position::new(at.paragraph, start.offset + text.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::{RawXml, Run, RunProps};

    fn control(props: &str) -> ContentControlRef {
        Arc::new(ContentControl {
            props: Some(RawXml::new(format!(
                r#"<w:sdtPr xmlns:w="urn:w" xmlns:w14="urn:w14">{props}</w:sdtPr>"#
            ))),
            end_props: None,
        })
    }

    /// Абзац «до [элемент] после», элемент из двух фрагментов.
    fn doc_with(control: &ContentControlRef, style: Option<&str>) -> Document {
        let inside = RunProps {
            control: Some(control.clone()),
            style: style.map(str::to_string),
            ..RunProps::default()
        };
        let paragraph = Paragraph::from_runs([
            Run::text("до "),
            Run::text("эл").with_props(inside.clone()),
            Run::text("емент").with_props(RunProps {
                bold: Some(true),
                ..inside
            }),
            Run::text(" после"),
        ]);
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![paragraph.into()];
        doc
    }

    fn text(doc: &Document) -> String {
        doc.plain_text()
    }

    #[test]
    fn finds_control_around_the_caret() {
        let c = control("");
        let doc = doc_with(&c, None);
        assert!(control_at(&doc, Position::new(0, 0)).is_none());
        let (found, start, end) = control_at(&doc, Position::new(0, 5)).unwrap();
        assert!(Arc::ptr_eq(&found, &c));
        assert_eq!((start.offset, end.offset), (5, 5 + "элемент".len()));
        // В конце элемента — тоже он.
        assert!(control_at(&doc, end).is_some());
    }

    #[test]
    fn typing_replaces_placeholder() {
        let c = control("<w:showingPlcHdr/>");
        let mut doc = doc_with(&c, Some("PlaceholderText"));
        assert!(placeholder_at(&doc, Position::new(0, 5)));
        let after = set_control_text(&mut doc, Position::new(0, 5), "Иван").unwrap();
        assert_eq!(text(&doc), "до Иван после");
        assert_eq!(after.offset, 5 + "Иван".len());
        assert!(!placeholder_at(&doc, Position::new(0, 5)));
        let (_, start, end) = control_at(&doc, Position::new(0, 5)).unwrap();
        assert_eq!((start.offset, end.offset), (5, after.offset));
        let Some(letter_model::Block::Paragraph(p)) = doc.blocks().next() else {
            panic!("expected paragraph")
        };
        assert!(p.runs().all(|r| r.props.style.is_none()));
    }

    #[test]
    fn checkbox_toggles() {
        let c = control(
            r#"<w14:checkbox><w14:checked w14:val="0"/><w14:checkedState w14:val="2612"/><w14:uncheckedState w14:val="2610"/></w14:checkbox>"#,
        );
        let mut doc = doc_with(&c, None);
        toggle_checkbox(&mut doc, Position::new(0, 5)).unwrap();
        assert_eq!(text(&doc), "до ☒ после");
        let (now, _, _) = control_at(&doc, Position::new(0, 5)).unwrap();
        assert!(matches!(
            now.kind(),
            ControlKind::CheckBox { checked: true, .. }
        ));
        toggle_checkbox(&mut doc, Position::new(0, 5)).unwrap();
        assert_eq!(text(&doc), "до ☐ после");
        // Не флажок — ничего не делаем.
        let mut plain = doc_with(&control(""), None);
        assert!(toggle_checkbox(&mut plain, Position::new(0, 5)).is_none());
    }

    #[test]
    fn list_and_date() {
        let list = control(
            r#"<w:dropDownList><w:listItem w:displayText="Да" w:value="1"/><w:listItem w:displayText="Нет" w:value="0"/></w:dropDownList>"#,
        );
        let mut doc = doc_with(&list, None);
        choose_list_item(&mut doc, Position::new(0, 5), 1).unwrap();
        assert_eq!(text(&doc), "до Нет после");
        assert!(choose_list_item(&mut doc, Position::new(0, 5), 7).is_none());

        let date = control(r#"<w:date><w:dateFormat w:val="d MMMM yyyy"/></w:date>"#);
        let mut doc = doc_with(&date, None);
        let day = DateTime {
            year: 2026,
            month: 10,
            day: 23,
            ..DateTime::default()
        };
        set_control_date(&mut doc, Position::new(0, 5), day).unwrap();
        assert_eq!(text(&doc), "до 23 октября 2026 после");
        let (now, _, _) = control_at(&doc, Position::new(0, 5)).unwrap();
        assert!(
            now.props
                .as_ref()
                .unwrap()
                .as_str()
                .contains("2026-10-23T00:00:00Z")
        );
    }

    #[test]
    fn locked_content_is_not_editable() {
        let c = control(r#"<w:lock w:val="contentLocked"/>"#);
        let mut doc = doc_with(&c, None);
        assert!(editable(&doc, Position::new(0, 0), Position::new(0, 2)));
        assert!(!editable(&doc, Position::new(0, 6), Position::new(0, 6)));
        assert!(!editable(&doc, Position::new(0, 0), Position::new(0, 20)));
        assert!(set_control_text(&mut doc, Position::new(0, 6), "x").is_none());
        // Запрет только удаления самого элемента не мешает правке.
        let doc = doc_with(&control(r#"<w:lock w:val="sdtLocked"/>"#), None);
        assert!(editable(&doc, Position::new(0, 6), Position::new(0, 6)));
    }
}
