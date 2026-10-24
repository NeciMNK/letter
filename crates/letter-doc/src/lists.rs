//! Списки (`PlfLst` и `PlfLfo`, MS-DOC 2.9.147, 2.9.183): как в .docx,
//! есть описания списков (абстрактные списки) и списки, на которые
//! ссылаются абзацы (`ilfo`), с изменениями отдельных уровней.

use letter_model::{
    AbstractNumbering, Alignment, LevelOverride, LevelSuffix, ListLevel, NumberFormat, Numbering,
    NumberingInstance, RunProps,
};

use crate::fib::{u16_at, u32_at};
use crate::sprm::{TableMarks, apply_chp, apply_pap, sprms};
use crate::styles::ISTD_NIL;

/// Размер описания списка (`LSTF`).
const LSTF_SIZE: usize = 28;
/// Размер описания уровня (`LVLF`).
const LVLF_SIZE: usize = 28;
/// Размер списка (`LFO`).
const LFO_SIZE: usize = 16;
/// Больше списков не читаем (защита от испорченных файлов).
const MAX_LISTS: usize = 4096;

/// Что нужно для оформления уровней: шрифты и id стилей по номерам.
pub(crate) struct Context<'a> {
    pub fonts: &'a [String],
    pub style_id: &'a dyn Fn(u16) -> Option<String>,
}

/// Списки документа. `lst` — табличный поток с начала `PlfLst` до
/// конца (уровни идут сразу за ним), `lfo` — `PlfLfo` до конца.
pub(crate) fn read(lst: Option<&[u8]>, lfo: Option<&[u8]>, context: &Context) -> Numbering {
    let mut numbering = Numbering::default();
    let lsids = lst
        .map(|lst| read_abstracts(lst, context, &mut numbering))
        .unwrap_or_default();
    if let Some(lfo) = lfo {
        read_instances(lfo, &lsids, context, &mut numbering);
    }
    numbering
}

/// Описания списков; возвращает их номера `lsid` по порядку.
fn read_abstracts(bytes: &[u8], context: &Context, numbering: &mut Numbering) -> Vec<u32> {
    let count = u16_at(bytes, 0).map_or(0, |n| usize::from(n).min(MAX_LISTS));
    let mut lsids = Vec::new();
    let mut at = 2 + count * LSTF_SIZE;
    for i in 0..count {
        let Some(lstf) = bytes.get(2 + i * LSTF_SIZE..2 + (i + 1) * LSTF_SIZE) else {
            break;
        };
        let lsid = u32_at(lstf, 0).unwrap_or(0);
        let simple = lstf[26] & 1 != 0;
        let mut levels = Vec::new();
        for level in 0..if simple { 1 } else { 9 } {
            let Some((mut lvl, size)) = read_level(bytes.get(at..).unwrap_or_default(), context)
            else {
                break;
            };
            at += size;
            lvl.level = level;
            // Стиль, связанный с уровнем (`rgistdPara`).
            let istd = u16_at(lstf, 8 + usize::from(level) * 2).unwrap_or(ISTD_NIL);
            if istd != ISTD_NIL {
                lvl.style = (context.style_id)(istd);
            }
            levels.push(lvl);
        }
        lsids.push(lsid);
        numbering.abstracts.push(AbstractNumbering {
            id: i as u32,
            levels,
            ..AbstractNumbering::default()
        });
    }
    lsids
}

/// Списки, на которые ссылаются абзацы: номер `i + 1`, описание по `lsid`.
fn read_instances(bytes: &[u8], lsids: &[u32], context: &Context, numbering: &mut Numbering) {
    let count = u32_at(bytes, 0).map_or(0, |n| (n as usize).min(MAX_LISTS));
    // За списками — их изменения уровней (`LFOData`) по порядку.
    let mut at = 4 + count * LFO_SIZE;
    for i in 0..count {
        let Some(lfo) = bytes.get(4 + i * LFO_SIZE..4 + (i + 1) * LFO_SIZE) else {
            break;
        };
        let lsid = u32_at(lfo, 0).unwrap_or(0);
        let changes = usize::from(lfo[12]).min(9);
        let mut overrides = Vec::new();
        at += 4; // cp
        for _ in 0..changes {
            let (Some(start), Some(flags)) = (u32_at(bytes, at), u32_at(bytes, at + 4)) else {
                break;
            };
            at += 8;
            let level = (flags & 0x0F).min(8) as u8;
            let mut change = LevelOverride {
                level,
                start: (flags & 0x10 != 0).then_some(start),
                ..LevelOverride::default()
            };
            if flags & 0x20 != 0 {
                let Some((mut lvl, size)) =
                    read_level(bytes.get(at..).unwrap_or_default(), context)
                else {
                    break;
                };
                at += size;
                lvl.level = level;
                change.definition = Some(lvl);
            }
            overrides.push(change);
        }
        let Some(abstract_id) = lsids.iter().position(|&l| l == lsid) else {
            continue;
        };
        numbering.instances.push(NumberingInstance {
            id: i as u32 + 1,
            abstract_id: abstract_id as u32,
            overrides,
            ..NumberingInstance::default()
        });
    }
}

/// Вид номера по `nfc`.
fn number_format(nfc: u8) -> NumberFormat {
    match nfc {
        0 => NumberFormat::Decimal,
        1 => NumberFormat::UpperRoman,
        2 => NumberFormat::LowerRoman,
        3 => NumberFormat::UpperLetter,
        4 => NumberFormat::LowerLetter,
        5 => NumberFormat::Ordinal,
        22 => NumberFormat::DecimalZero,
        23 => NumberFormat::Bullet,
        255 => NumberFormat::None,
        n => NumberFormat::Other(format!("nfc{n}")),
    }
}

/// Уровень (`LVL`): описание, оформление абзаца и номера, текст номера.
/// Возвращает уровень и сколько байт он занял.
fn read_level(bytes: &[u8], context: &Context) -> Option<(ListLevel, usize)> {
    let lvlf = bytes.get(..LVLF_SIZE)?;
    let start = u32_at(lvlf, 0)?;
    let nfc = lvlf[4];
    let flags = lvlf[5];
    // Места номеров уровней в тексте (с 1; 0 — дальше нет).
    let places: Vec<usize> = lvlf[6..15]
        .iter()
        .take_while(|&&p| p != 0)
        .map(|&p| usize::from(p))
        .collect();
    let follow = lvlf[15];
    let chpx_len = usize::from(lvlf[24]);
    let papx_len = usize::from(lvlf[25]);
    let restart_limit = lvlf[26];

    let mut at = LVLF_SIZE;
    let papx = bytes.get(at..at + papx_len)?;
    at += papx_len;
    let chpx = bytes.get(at..at + chpx_len)?;
    at += chpx_len;
    let len = usize::from(u16_at(bytes, at)?);
    at += 2;
    let units: Vec<u16> = (0..len)
        .map(|i| u16_at(bytes, at + i * 2))
        .collect::<Option<_>>()?;
    at += len * 2;

    // Знаки на местах номеров — номера уровней (0–8): «%1», «%2»…
    let mut text = String::new();
    for (i, &unit) in units.iter().enumerate() {
        if places.contains(&(i + 1)) && unit < 9 {
            text.push_str(&format!("%{}", unit + 1));
        } else {
            text.push_str(&String::from_utf16_lossy(&[unit]));
        }
    }

    let mut level = ListLevel {
        start: Some(start),
        format: Some(number_format(nfc)),
        text: Some(text),
        alignment: Some(match flags & 3 {
            1 => Alignment::Center,
            2 => Alignment::Right,
            _ => Alignment::Left,
        }),
        legal: (flags & 4 != 0).then_some(true),
        restart: (flags & 8 != 0).then_some(restart_limit),
        suffix: Some(match follow {
            1 => LevelSuffix::Space,
            2 => LevelSuffix::Nothing,
            _ => LevelSuffix::Tab,
        }),
        ..ListLevel::default()
    };
    let mut marks = TableMarks::default();
    for (sprm, op) in sprms(papx) {
        apply_pap(&mut level.paragraph, &mut marks, sprm, op);
    }
    let mut run = RunProps::default();
    for (sprm, op) in sprms(chpx) {
        apply_chp(&mut run, sprm, op, context.fonts, context.style_id);
    }
    level.run = run;
    Some((level, at))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> (Vec<String>, impl Fn(u16) -> Option<String>) {
        (vec!["Symbol".to_string()], |_| None)
    }

    /// Уровень с текстом `text` (места номеров — `places`).
    fn lvl(nfc: u8, places: &[u8], text: &[u16], chpx: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(1u32.to_le_bytes());
        bytes.push(nfc);
        bytes.push(0);
        let mut rg = [0u8; 9];
        rg[..places.len()].copy_from_slice(places);
        bytes.extend(rg);
        bytes.push(0); // после номера — табуляция
        bytes.extend([0; 8]);
        bytes.push(chpx.len() as u8);
        bytes.push(0); // без оформления абзаца
        bytes.extend([0, 0]);
        bytes.extend(chpx);
        bytes.extend((text.len() as u16).to_le_bytes());
        for unit in text {
            bytes.extend(unit.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn simple_list_with_override() {
        // Один простой список: номер «%1.».
        let mut lst = 1u16.to_le_bytes().to_vec();
        lst.extend(7u32.to_le_bytes()); // lsid
        lst.extend([0; 4]);
        lst.extend([0xFF, 0x0F].repeat(9));
        lst.push(1); // простой
        lst.push(0);
        lst.extend(lvl(0, &[1], &[0, u16::from(b'.')], &[]));

        let mut lfo = 1u32.to_le_bytes().to_vec();
        lfo.extend(7u32.to_le_bytes());
        lfo.extend([0; 8]);
        lfo.extend([1, 0, 0, 0]); // одно изменение
        lfo.extend([0; 4]); // cp
        lfo.extend(5u32.to_le_bytes());
        lfo.extend(0x10u32.to_le_bytes()); // уровень 0, начать с 5

        let (fonts, style_id) = context();
        let context = Context {
            fonts: &fonts,
            style_id: &style_id,
        };
        let numbering = read(Some(&lst), Some(&lfo), &context);
        assert_eq!(numbering.abstracts.len(), 1);
        let level = &numbering.abstracts[0].levels[0];
        assert_eq!(level.text.as_deref(), Some("%1."));
        assert_eq!(level.format, Some(NumberFormat::Decimal));
        assert_eq!(numbering.instances[0].id, 1);
        assert_eq!(numbering.instances[0].overrides[0].start, Some(5));
        assert_eq!(numbering.level(1, 0).and_then(|l| l.start), Some(5));
    }

    #[test]
    fn bullet_level_with_font() {
        let chpx = [0x4F, 0x4A, 0, 0]; // шрифт ASCII номер 0
        let bytes = lvl(23, &[], &[0xF0B7], &chpx);
        let (fonts, style_id) = context();
        let context = Context {
            fonts: &fonts,
            style_id: &style_id,
        };
        let (level, size) = read_level(&bytes, &context).unwrap();
        assert_eq!(size, bytes.len());
        assert_eq!(level.format, Some(NumberFormat::Bullet));
        assert_eq!(level.text.as_deref(), Some("\u{F0B7}"));
        assert_eq!(level.run.fonts.ascii.as_deref(), Some("Symbol"));
        // Испорченный уровень — не падение.
        assert!(read_level(&bytes[..30], &context).is_none());
        assert!(read(Some(&[5, 0]), Some(&[9, 0, 0, 0]), &context).is_empty());
    }
}
