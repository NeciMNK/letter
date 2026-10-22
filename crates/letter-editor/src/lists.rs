//! Списки: кнопки «Маркеры» и «Нумерация», уровни (Tab и Shift+Tab),
//! выход из списка.

use letter_model::{
    AbstractNumbering, Alignment, Document, FirstLineIndent, Indentation, LevelSuffix, ListLevel,
    NumberFormat, NumberingInstance, NumberingRef, ParagraphProps, RunFonts, RunProps, Twips,
};

use crate::edit::{paragraph_mut, paragraph_ref};
use crate::position::Position;

/// Вид списка.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    /// Маркированный: «•».
    Bullet,
    /// Нумерованный: «1.», «a.», «i.».
    Numbered,
}

/// Сколько уровней у списка.
const LEVELS: u8 = 9;

/// Список и уровень абзаца, если он пункт списка.
pub fn list_item(doc: &Document, paragraph: usize) -> Option<(u32, u8)> {
    let p = paragraph_ref(doc, paragraph)?;
    let (id, level) = doc.resolve_paragraph(&p.props).numbering?;
    // Ссылка на несуществующий список — не список.
    doc.numbering.level(id, level)?;
    Some((id, level))
}

/// Вид списка, в котором абзац (для кнопок на ленте).
pub fn list_kind(doc: &Document, paragraph: usize) -> Option<ListKind> {
    let (id, level) = list_item(doc, paragraph)?;
    let level = doc.numbering.level(id, level)?;
    Some(match level.format {
        Some(NumberFormat::Bullet) => ListKind::Bullet,
        _ => ListKind::Numbered,
    })
}

fn ordered(a: Position, b: Position) -> (usize, usize) {
    (a.paragraph.min(b.paragraph), a.paragraph.max(b.paragraph))
}

/// Кнопка «Маркеры» или «Нумерация» для абзацев от `start` до `end`.
///
/// Если все они уже в списке этого вида — убирает их из списка. Иначе
/// делает их пунктами: продолжает список того же вида прямо над ними
/// (или тот, в котором уже есть кто-то из них), а если такого нет —
/// заводит новый, как в Word. Уровень пункта сохраняется.
pub fn toggle_list(doc: &mut Document, start: Position, end: Position, kind: ListKind) {
    let (first, last) = ordered(start, end);
    let paragraphs: Vec<usize> = (first..=last)
        .filter(|&i| paragraph_ref(doc, i).is_some())
        .collect();
    if paragraphs.is_empty() {
        return;
    }
    if paragraphs.iter().all(|&i| list_kind(doc, i) == Some(kind)) {
        for &i in &paragraphs {
            remove_from_list(doc, i, false);
        }
        return;
    }
    let same_kind = |doc: &Document, i: usize| {
        (list_kind(doc, i) == Some(kind))
            .then(|| list_item(doc, i).map(|(id, _)| id))
            .flatten()
    };
    let above = first.checked_sub(1).and_then(|i| same_kind(doc, i));
    let inside = paragraphs.iter().find_map(|&i| same_kind(doc, i));
    let id = match above.or(inside) {
        Some(id) => id,
        None => new_list(doc, kind),
    };
    for &i in &paragraphs {
        let level = list_item(doc, i).map_or(0, |(_, level)| level);
        if let Some(p) = paragraph_mut(doc, i) {
            p.props.numbering = Some(NumberingRef {
                id: Some(id),
                level: Some(level),
            });
            // Отступы берутся из уровня списка.
            p.props.indentation.left = None;
            p.props.indentation.first_line = None;
        }
    }
}

/// Убирает абзац из списка. `keep_indent` — текст остаётся на месте
/// (Backspace в начале пункта), иначе отступы — как без списка.
pub fn remove_from_list(doc: &mut Document, paragraph: usize, keep_indent: bool) {
    if list_item(doc, paragraph).is_none() {
        return;
    }
    let Some(p) = paragraph_ref(doc, paragraph) else {
        return;
    };
    let indent_left = doc.resolve_paragraph(&p.props).indent_left;
    // Список может прийти из стиля: тогда его отменяет номер 0.
    let without = ParagraphProps {
        numbering: None,
        ..p.props.clone()
    };
    let from_style = doc.resolve_paragraph(&without).numbering.is_some();
    let Some(p) = paragraph_mut(doc, paragraph) else {
        return;
    };
    p.props.numbering = from_style.then_some(NumberingRef {
        id: Some(0),
        level: None,
    });
    if keep_indent {
        p.props.indentation.left = Some(indent_left);
        p.props.indentation.first_line = Some(FirstLineIndent::FirstLine(Twips::ZERO));
    }
}

/// Меняет уровень пунктов списка от `start` до `end` на `delta`
/// (Tab — глубже, Shift+Tab — выше), от 0 до 8. Абзацы не из списка не
/// меняются.
pub fn change_list_level(doc: &mut Document, start: Position, end: Position, delta: i8) {
    let (first, last) = ordered(start, end);
    for i in first..=last {
        let Some((id, level)) = list_item(doc, i) else {
            continue;
        };
        let level = (i16::from(level) + i16::from(delta)).clamp(0, i16::from(LEVELS - 1)) as u8;
        if let Some(p) = paragraph_mut(doc, i) {
            p.props.numbering = Some(NumberingRef {
                id: Some(id),
                level: Some(level),
            });
            p.props.indentation.left = None;
            p.props.indentation.first_line = None;
        }
    }
}

/// Меняет ли Tab уровень списка, а не вставляет табуляцию: все абзацы
/// выделения — пункты списка, а курсор стоит в начале пункта или
/// выделено несколько абзацев.
pub fn tab_changes_level(doc: &Document, start: Position, end: Position) -> bool {
    let (first, last) = ordered(start, end);
    let at_start = first != last || start.offset.min(end.offset) == 0;
    at_start && (first..=last).all(|i| list_item(doc, i).is_some())
}

/// Новый список вида `kind` с уровнями, как у Word по умолчанию;
/// возвращает его номер.
fn new_list(doc: &mut Document, kind: ListKind) -> u32 {
    let abstract_id = doc.numbering.next_abstract_id();
    let id = doc.numbering.next_instance_id();
    let levels = (0..LEVELS)
        .map(|level| default_level(kind, level))
        .collect();
    doc.numbering.abstracts.push(AbstractNumbering {
        id: abstract_id,
        attributes: Vec::new(),
        levels,
        unknown: Vec::new(),
    });
    doc.numbering.instances.push(NumberingInstance {
        id,
        abstract_id,
        ..NumberingInstance::default()
    });
    id
}

/// Уровень списка по умолчанию: отступ на полдюйма глубже с каждым
/// уровнем, выступ четверть дюйма.
fn default_level(kind: ListKind, level: u8) -> ListLevel {
    let n = i32::from(level);
    let mut hanging = 360;
    let (format, text, alignment, font) = match kind {
        ListKind::Bullet => {
            let (text, font) = match level % 3 {
                0 => ("\u{F0B7}", "Symbol"),
                1 => ("o", "Courier New"),
                _ => ("\u{F0A7}", "Wingdings"),
            };
            (
                NumberFormat::Bullet,
                text.to_string(),
                Alignment::Left,
                Some(font),
            )
        }
        ListKind::Numbered => {
            let placeholder = format!("%{}", level + 1);
            match level % 3 {
                0 => (
                    NumberFormat::Decimal,
                    format!("{placeholder}."),
                    Alignment::Left,
                    None,
                ),
                1 => (
                    NumberFormat::LowerLetter,
                    format!("{placeholder}."),
                    Alignment::Left,
                    None,
                ),
                _ => {
                    hanging = 180;
                    (
                        NumberFormat::LowerRoman,
                        format!("{placeholder}."),
                        Alignment::Right,
                        None,
                    )
                }
            }
        }
    };
    let run = RunProps {
        fonts: RunFonts {
            ascii: font.map(str::to_string),
            h_ansi: font.map(str::to_string),
            hint: font.map(|_| "default".to_string()),
            ..RunFonts::default()
        },
        ..RunProps::default()
    };
    ListLevel {
        level,
        start: Some(1),
        format: Some(format),
        text: Some(text),
        suffix: Some(LevelSuffix::Tab),
        alignment: Some(alignment),
        paragraph: ParagraphProps {
            indentation: Indentation {
                left: Some(Twips(720 * (n + 1))),
                first_line: Some(FirstLineIndent::Hanging(Twips(hanging))),
                ..Indentation::default()
            },
            ..ParagraphProps::default()
        },
        run,
        ..ListLevel::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::{Block, ListCounters, Paragraph, Run, Section};

    fn doc(texts: &[&str]) -> Document {
        let mut doc = Document::new();
        doc.sections = vec![Section {
            blocks: texts
                .iter()
                .map(|t| Block::from(Paragraph::from_runs([Run::text(*t)])))
                .collect(),
            ..Section::default()
        }];
        doc
    }

    fn at(paragraph: usize) -> Position {
        Position::new(paragraph, 0)
    }

    /// Номера абзацев, как их покажет вёрстка.
    fn labels(doc: &Document) -> Vec<Option<String>> {
        let mut counters = ListCounters::default();
        (0..doc.sections[0].blocks.len())
            .map(|i| {
                let (id, level) = list_item(doc, i)?;
                counters.next(&doc.numbering, id, level).map(|l| l.text)
            })
            .collect()
    }

    #[test]
    fn numbering_on_and_off() {
        let mut d = doc(&["a", "b", "c"]);
        toggle_list(&mut d, at(0), at(1), ListKind::Numbered);
        assert_eq!(labels(&d), [Some("1.".into()), Some("2.".into()), None]);
        assert_eq!(list_kind(&d, 0), Some(ListKind::Numbered));
        let resolved = d.resolve_paragraph(&ParagraphProps {
            numbering: Some(NumberingRef {
                id: Some(1),
                level: Some(0),
            }),
            ..ParagraphProps::default()
        });
        assert_eq!(resolved.indent_left, Twips(720));
        // Третий абзац продолжает список над ним.
        toggle_list(&mut d, at(2), at(2), ListKind::Numbered);
        assert_eq!(d.numbering.instances.len(), 1);
        assert_eq!(labels(&d)[2].as_deref(), Some("3."));
        // Все уже нумерованные — нумерация снимается.
        toggle_list(&mut d, at(0), at(2), ListKind::Numbered);
        assert_eq!(labels(&d), [None, None, None]);
    }

    #[test]
    fn bullets_replace_numbers() {
        let mut d = doc(&["a", "b"]);
        toggle_list(&mut d, at(0), at(1), ListKind::Numbered);
        toggle_list(&mut d, at(1), at(1), ListKind::Bullet);
        assert_eq!(list_kind(&d, 0), Some(ListKind::Numbered));
        assert_eq!(list_kind(&d, 1), Some(ListKind::Bullet));
        assert_eq!(labels(&d)[1].as_deref(), Some("\u{F0B7}"));
    }

    #[test]
    fn levels_change_with_tab() {
        let mut d = doc(&["a", "b", "c"]);
        toggle_list(&mut d, at(0), at(2), ListKind::Numbered);
        assert!(tab_changes_level(&d, at(1), at(1)));
        assert!(!tab_changes_level(
            &d,
            Position::new(1, 1),
            Position::new(1, 1)
        ));
        change_list_level(&mut d, at(1), at(1), 1);
        assert_eq!(
            labels(&d),
            [Some("1.".into()), Some("a.".into()), Some("2.".into())]
        );
        change_list_level(&mut d, at(1), at(1), -5);
        assert_eq!(list_item(&d, 1).map(|(_, l)| l), Some(0));
        change_list_level(&mut d, at(0), at(0), 20);
        assert_eq!(list_item(&d, 0).map(|(_, l)| l), Some(8));
        let plain = doc(&["x"]);
        assert!(!tab_changes_level(&plain, at(0), at(0)));
    }

    #[test]
    fn leaving_a_list_keeps_text_in_place_on_backspace() {
        let mut d = doc(&["a"]);
        toggle_list(&mut d, at(0), at(0), ListKind::Bullet);
        remove_from_list(&mut d, 0, true);
        assert_eq!(list_item(&d, 0), None);
        let p = paragraph_ref(&d, 0).map(|p| d.resolve_paragraph(&p.props));
        assert_eq!(p.map(|p| p.indent_left), Some(Twips(720)));
    }

    #[test]
    fn keys_in_a_list() {
        use crate::{Command, History, Selection};
        let mut d = doc(&["a"]);
        let mut history = History::new();
        let end = Position::new(0, 1);
        let s = history.run(
            &mut d,
            Selection::caret(end),
            Command::List(ListKind::Bullet),
        );
        // Enter в конце пункта — новый пункт.
        let s = history.run(&mut d, s, Command::SplitParagraph);
        assert_eq!(s.focus, at(1));
        assert!(list_item(&d, 1).is_some());
        // Tab в начале пункта — уровень глубже, Shift+Tab — обратно.
        let s = history.run(&mut d, s, Command::Tab { shift: false });
        assert_eq!(list_item(&d, 1).map(|(_, l)| l), Some(1));
        let s = history.run(&mut d, s, Command::Tab { shift: true });
        assert_eq!(list_item(&d, 1).map(|(_, l)| l), Some(0));
        // Enter в пустом пункте — конец списка.
        let s = history.run(&mut d, s, Command::SplitParagraph);
        assert_eq!(s.focus, at(1));
        assert_eq!(list_item(&d, 1), None);
        assert_eq!(d.sections[0].blocks.len(), 2);
        // Tab не в списке — табуляция; Shift+Tab — ничего.
        let s = history.run(&mut d, s, Command::Tab { shift: true });
        let s = history.run(&mut d, s, Command::Tab { shift: false });
        assert_eq!(s.focus, Position::new(1, 1));
        // Backspace в начале пункта убирает номер, текст остаётся.
        let s = history.run(&mut d, Selection::caret(at(0)), Command::DeleteBackward);
        assert_eq!(s.focus, at(0));
        assert_eq!(list_item(&d, 0), None);
        assert_eq!(d.sections[0].blocks.len(), 2);
        // Отмена возвращает пункт.
        history.undo(&mut d);
        assert!(list_item(&d, 0).is_some());
    }
}
