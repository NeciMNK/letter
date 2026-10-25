//! Разделы: где кончаются (`PlcfSed`) и их параметры (`SEPX`): размер и
//! поля страницы, колонки, начало раздела.

use letter_model::{
    Columns, Orientation, PageMargins, PageSize, SectionProps, SectionStart, Twips,
};

use crate::fib::{u16_at, u32_at};
use crate::sprm::sprms;

/// Раздел: CP после его последнего знака и место параметров в потоке
/// `WordDocument` (`None` — параметров нет).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sed {
    pub end: u32,
    pub sepx: Option<usize>,
}

/// Разделы из `PlcfSed`: (n + 1) CP и n описаний по 12 байт.
pub(crate) fn seds(plc: &[u8]) -> Vec<Sed> {
    if plc.len() < 4 + 16 {
        return Vec::new();
    }
    let count = (plc.len() - 4) / 16;
    (0..count)
        .filter_map(|i| {
            let end = u32_at(plc, 4 * (i + 1))?;
            let fc = u32_at(plc, 4 * (count + 1) + 12 * i + 2)?;
            Some(Sed {
                end,
                sepx: (fc != u32::MAX).then_some(fc as usize),
            })
        })
        .collect()
}

/// Параметры раздела из `SEPX` в месте `at` потока `word`. Чего нет в
/// списке — как по умолчанию в Word 97 (Letter, поля 1 и 1,25 дюйма).
pub(crate) fn section_props(word: &[u8], at: Option<usize>) -> SectionProps {
    let grpprl = at
        .and_then(|at| {
            let size = usize::from(u16_at(word, at)?);
            word.get(at + 2..at + 2 + size)
        })
        .unwrap_or(&[]);
    let mut page = PageSize::LETTER;
    let mut margins = PageMargins {
        top: Twips(1440),
        right: Twips(1800),
        bottom: Twips(1440),
        left: Twips(1800),
        header: Twips(720),
        footer: Twips(720),
        gutter: Twips(0),
    };
    let mut columns = Columns {
        space: Twips(720),
        ..Columns::default()
    };
    // Ширины и промежутки неравных колонок по номерам.
    let mut widths: Vec<(Twips, Twips)> = Vec::new();
    let mut evenly = true;
    let mut props = SectionProps::default();
    for sprm in sprms(grpprl) {
        let twips = Twips(i32::from(sprm.signed()));
        let unsigned = Twips(i32::from(sprm.word()));
        match sprm.code {
            0xB01F => page.width = unsigned,
            0xB020 => page.height = unsigned,
            0x301D => {
                page.orientation = if sprm.byte() == 2 {
                    Orientation::Landscape
                } else {
                    Orientation::Portrait
                }
            }
            0xB021 => margins.left = unsigned,
            0xB022 => margins.right = unsigned,
            // Отрицательное — «точно», без раздвигания колонтитулом.
            0x9023 => margins.top = Twips(twips.0.abs()),
            0x9024 => margins.bottom = Twips(twips.0.abs()),
            0xB025 => margins.gutter = unsigned,
            0xB017 => margins.header = unsigned,
            0xB018 => margins.footer = unsigned,
            0x3009 => {
                props.start = match sprm.byte() {
                    0 => Some(SectionStart::Continuous),
                    1 => Some(SectionStart::NextColumn),
                    3 => Some(SectionStart::EvenPage),
                    4 => Some(SectionStart::OddPage),
                    // С новой страницы — как без указания.
                    _ => None,
                }
            }
            0x300A => props.title_page = sprm.byte() != 0,
            0x500B => columns.count = sprm.word().saturating_add(1),
            0x900C => columns.space = unsigned,
            0x3019 => columns.separator = sprm.byte() != 0,
            0x3005 => evenly = sprm.byte() != 0,
            // Ширина и промежуток колонки номер `index`.
            0xF203 | 0xF204 => {
                let index = usize::from(sprm.byte());
                let value = Twips(i32::from(u16_at(sprm.operand, 1).unwrap_or(0)));
                if widths.len() <= index {
                    widths.resize(index + 1, (Twips(0), Twips(0)));
                }
                if sprm.code == 0xF203 {
                    widths[index].0 = value;
                } else {
                    widths[index].1 = value;
                }
            }
            _ => {}
        }
    }
    if !evenly && columns.count > 1 && widths.len() == usize::from(columns.count) {
        columns.widths = widths;
    }
    props.page_size = Some(page);
    props.margins = Some(margins);
    if columns.count > 1 {
        props.columns = Some(columns);
    }
    props
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seds_from_plc() {
        let mut plc = Vec::new();
        for cp in [0u32, 10, 25] {
            plc.extend(cp.to_le_bytes());
        }
        for fc in [0x600u32, u32::MAX] {
            plc.extend([0, 0]);
            plc.extend(fc.to_le_bytes());
            plc.extend([0; 6]);
        }
        let seds = seds(&plc);
        assert_eq!(seds.len(), 2);
        assert_eq!(
            seds[0],
            Sed {
                end: 10,
                sepx: Some(0x600)
            }
        );
        assert_eq!(
            seds[1],
            Sed {
                end: 25,
                sepx: None
            }
        );
        assert!(super::seds(&[0; 8]).is_empty());
    }

    #[test]
    fn landscape_two_columns() {
        let mut word = vec![0u8; 4];
        let grpprl: Vec<u8> = [
            &[0x1F, 0xB0][..],
            &15840u16.to_le_bytes(),
            &[0x20, 0xB0],
            &12240u16.to_le_bytes(),
            &[0x1D, 0x30, 2],
            &[0x0B, 0x50, 1, 0],
            &[0x09, 0x30, 0],
        ]
        .concat();
        word.extend((grpprl.len() as u16).to_le_bytes());
        word.extend(grpprl);
        let props = section_props(&word, Some(4));
        let page = props.page_size.unwrap();
        assert_eq!(page.width, Twips(15840));
        assert_eq!(page.orientation, Orientation::Landscape);
        assert_eq!(props.columns.unwrap().count, 2);
        assert_eq!(props.start, Some(SectionStart::Continuous));
        let plain = section_props(&word, None);
        assert_eq!(plain.margins.unwrap().left, Twips(1800));
        assert!(plain.columns.is_none());
    }
}
