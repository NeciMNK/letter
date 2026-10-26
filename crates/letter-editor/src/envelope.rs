//! Конверты: «Рассылки → Конверты» добавляет в начало документа раздел
//! размером с конверт (альбомный лист): обратный адрес в левом верхнем
//! углу, адрес получателя — правее и ниже середины. Вместо адреса
//! получателя может стоять блок адреса рассылки (`ADDRESSBLOCK`), тогда
//! «Найти и объединить» печатает конверт для каждого получателя.

use letter_model::{
    Block, Document, LineSpacing, Orientation, PageMargins, PageSize, Paragraph, Section,
    SectionProps, SectionStart, Twips,
};

use crate::merge::{MergeRule, insert_merge_rule};
use crate::position::Position;

/// Размер конверта.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvelopeSize {
    /// Название («DL», «C5», «№ 10»).
    pub name: &'static str,
    /// Длинная сторона.
    pub width: Twips,
    /// Короткая сторона.
    pub height: Twips,
}

/// Сотые доли миллиметра в твипах.
const fn mm(hundredths: i32) -> Twips {
    Twips((hundredths * 1440 + 1270) / 2540)
}

/// Распространённые конверты.
pub const ENVELOPES: [EnvelopeSize; 5] = [
    EnvelopeSize {
        name: "DL",
        width: mm(22000),
        height: mm(11000),
    },
    EnvelopeSize {
        name: "C6",
        width: mm(16200),
        height: mm(11400),
    },
    EnvelopeSize {
        name: "C5",
        width: mm(22900),
        height: mm(16200),
    },
    EnvelopeSize {
        name: "C4",
        width: mm(32400),
        height: mm(22900),
    },
    EnvelopeSize {
        name: "#10",
        width: Twips(13680),
        height: Twips(5940),
    },
];

/// Что напечатать на конверте.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    /// Размер.
    pub size: EnvelopeSize,
    /// Адрес получателя, строки через `\n`.
    pub delivery: String,
    /// Обратный адрес.
    pub return_address: String,
    /// Вместо адреса получателя — блок адреса рассылки.
    pub address_block: bool,
}

/// Высота строки на конверте (ровно 14 пт): места адресов не зависят
/// от шрифта.
const LINE: Twips = Twips(280);
/// Поля конверта: 1 см сверху и справа, 1,5 см слева.
const TOP: Twips = mm(1000);
const LEFT: Twips = mm(1500);

/// Абзац адреса.
fn address_line(text: &str) -> Paragraph {
    let mut paragraph = Paragraph::text(text);
    paragraph.props.spacing.before = Some(Twips(0));
    paragraph.props.spacing.after = Some(Twips(0));
    paragraph.props.spacing.line = Some(LineSpacing::Exact(LINE));
    paragraph
}

/// Строки адреса (без пустых по краям); пустой адрес — одна пустая строка.
fn lines(text: &str) -> Vec<&str> {
    let lines: Vec<&str> = text.trim_matches('\n').lines().collect();
    if lines.is_empty() { vec![""] } else { lines }
}

/// Раздел конверта в начале документа.
fn is_envelope(section: &Section) -> bool {
    section.props.page_size.is_some_and(|page| {
        ENVELOPES
            .iter()
            .any(|e| e.width == page.width && e.height == page.height)
    })
}

/// Есть ли в начале документа конверт.
pub fn has_envelope(doc: &Document) -> bool {
    doc.sections.len() > 1 && doc.sections.first().is_some_and(is_envelope)
}

/// «Добавить в документ»: раздел конверта в начало документа (прежний
/// конверт заменяется). Возвращает место в начале адреса получателя.
pub fn add_envelope(doc: &mut Document, envelope: &Envelope) -> Position {
    let size = envelope.size;
    let mut blocks: Vec<Block> = lines(&envelope.return_address)
        .into_iter()
        .map(|line| address_line(line).into())
        .collect();
    let returns = blocks.len() as i32;
    let delivery = if envelope.address_block {
        vec![""]
    } else {
        lines(&envelope.delivery)
    };
    // Адрес получателя: от левого края 45 % ширины, сверху — середина.
    let indent = Twips((size.width.0 * 45 / 100 - LEFT.0).max(0));
    let before = Twips((size.height.0 / 2 - TOP.0 - returns * LINE.0).max(0));
    for (k, line) in delivery.into_iter().enumerate() {
        let mut paragraph = address_line(line);
        paragraph.props.indentation.left = Some(indent);
        if k == 0 {
            paragraph.props.spacing.before = Some(before);
        }
        blocks.push(paragraph.into());
    }
    let section = Section {
        props: SectionProps {
            page_size: Some(PageSize {
                width: size.width,
                height: size.height,
                orientation: Orientation::Landscape,
            }),
            margins: Some(PageMargins {
                top: TOP,
                right: mm(1000),
                bottom: mm(1000),
                left: LEFT,
                header: Twips(0),
                footer: Twips(0),
                gutter: Twips(0),
            }),
            start: Some(SectionStart::NextPage),
            ..SectionProps::default()
        },
        blocks,
    };
    if has_envelope(doc) {
        doc.sections[0] = section;
    } else {
        doc.sections.insert(0, section);
    }
    let at = Position::new(returns as usize, 0);
    if envelope.address_block {
        insert_merge_rule(doc, at, &MergeRule::AddressBlock);
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merge::{MergeData, merge_all};

    fn envelope(delivery: &str, address_block: bool) -> Envelope {
        Envelope {
            size: ENVELOPES[0],
            delivery: delivery.into(),
            return_address: "Иванов И. И.\nМосква".into(),
            address_block,
        }
    }

    #[test]
    fn envelope_goes_first_and_is_replaced() {
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![Paragraph::text("Письмо").into()];
        let at = add_envelope(&mut doc, &envelope("Петрову П. П.\nОмск", false));
        assert_eq!(at, Position::new(2, 0));
        assert_eq!(doc.sections.len(), 2);
        assert!(has_envelope(&doc));
        assert_eq!(
            doc.sections[0].plain_text(),
            "Иванов И. И.\nМосква\nПетрову П. П.\nОмск"
        );
        let page = doc.sections[0].props.page_size.expect("размер");
        assert_eq!(page.orientation, Orientation::Landscape);
        let Block::Paragraph(first) = &doc.sections[0].blocks[2] else {
            panic!("нет абзаца");
        };
        assert!(first.props.indentation.left.expect("отступ").0 > 0);
        // Повторно — конверт заменяется, а не добавляется.
        add_envelope(&mut doc, &envelope("Сидорову", false));
        assert_eq!(doc.sections.len(), 2);
        assert!(doc.sections[0].plain_text().ends_with("Сидорову"));
        assert_eq!(doc.sections[1].plain_text(), "Письмо");
    }

    #[test]
    fn envelope_with_address_block_merges() {
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![Paragraph::text("Письмо").into()];
        add_envelope(&mut doc, &envelope("", true));
        assert!(doc.sections[0].plain_text().contains("«AddressBlock»"));
        let data = MergeData::from_csv("Имя;Фамилия;Город\nВера;Лес;Омск\nАнна;Сад;Тверь\n");
        let merged = merge_all(&doc, &data);
        // По конверту и письму на получателя.
        assert_eq!(merged.sections.len(), 4);
        assert!(merged.sections[2].plain_text().contains("Анна Сад"));
        assert!(merged.sections[2].plain_text().contains("Тверь"));
    }
}
