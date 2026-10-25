//! Оформление знаков и абзацев: страницы FKP по 512 байт в потоке
//! `WordDocument`. Каждая страница делит кусок потока (по местам FC) на
//! отрезки и даёт каждому список свойств. Где лежат страницы — в
//! таблицах `PlcfBteChpx` и `PlcfBtePapx`.

use crate::fib::{u16_at, u32_at};

/// Размер страницы FKP.
const PAGE: usize = 512;

/// Отрезок потока с его списком свойств.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Span<'a> {
    /// Начало (FC, байты потока).
    pub fc_start: u32,
    /// Конец.
    pub fc_end: u32,
    /// Номер стиля абзаца (только у абзацев).
    pub istd: u16,
    /// Список свойств (sprm).
    pub grpprl: &'a [u8],
}

/// Страницы FKP из таблицы `PlcfBte…`: (n + 1) FC и n номеров страниц.
fn pages(plc: &[u8]) -> Vec<usize> {
    if plc.len() < 8 {
        return Vec::new();
    }
    let count = (plc.len() - 4) / 8;
    (0..count)
        .filter_map(|i| u32_at(plc, 4 * (count + 1) + 4 * i))
        .map(|pn| (pn & 0x003F_FFFF) as usize)
        .collect()
}

/// Отрезки оформления знаков.
pub(crate) fn chpx<'a>(plc: &[u8], word: &'a [u8]) -> Vec<Span<'a>> {
    let mut out = Vec::new();
    for pn in pages(plc) {
        let Some(page) = word.get(pn * PAGE..(pn + 1) * PAGE) else {
            continue;
        };
        let count = usize::from(page[PAGE - 1]);
        for i in 0..count {
            let (Some(fc_start), Some(fc_end)) = (u32_at(page, 4 * i), u32_at(page, 4 * (i + 1)))
            else {
                break;
            };
            let offset = usize::from(page.get(4 * (count + 1) + i).copied().unwrap_or(0)) * 2;
            // Смещение 0 — без свойств.
            let grpprl = if offset == 0 {
                &[][..]
            } else {
                let size = usize::from(page.get(offset).copied().unwrap_or(0));
                page.get(offset + 1..offset + 1 + size).unwrap_or(&[])
            };
            out.push(Span {
                fc_start,
                fc_end,
                istd: 0,
                grpprl,
            });
        }
    }
    out.sort_by_key(|s| s.fc_start);
    out
}

/// Отрезки оформления абзацев (каждый кончается знаком конца абзаца).
pub(crate) fn papx<'a>(plc: &[u8], word: &'a [u8]) -> Vec<Span<'a>> {
    let mut out = Vec::new();
    for pn in pages(plc) {
        let Some(page) = word.get(pn * PAGE..(pn + 1) * PAGE) else {
            continue;
        };
        let count = usize::from(page[PAGE - 1]);
        for i in 0..count {
            let (Some(fc_start), Some(fc_end)) = (u32_at(page, 4 * i), u32_at(page, 4 * (i + 1)))
            else {
                break;
            };
            // BxPap: байт смещения и 12 запасных.
            let offset = usize::from(page.get(4 * (count + 1) + 13 * i).copied().unwrap_or(0)) * 2;
            let mut istd = 0;
            let mut grpprl = &[][..];
            if offset != 0 {
                let cb = usize::from(page.get(offset).copied().unwrap_or(0));
                let (start, size) = if cb == 0 {
                    let cb = usize::from(page.get(offset + 1).copied().unwrap_or(0));
                    (offset + 2, 2 * cb)
                } else {
                    (offset + 1, (2 * cb).saturating_sub(1))
                };
                if let Some(data) = page.get(start..start + size) {
                    istd = u16_at(data, 0).unwrap_or(0);
                    grpprl = data.get(2..).unwrap_or(&[]);
                }
            }
            out.push(Span {
                fc_start,
                fc_end,
                istd,
                grpprl,
            });
        }
    }
    out.sort_by_key(|s| s.fc_start);
    out
}

/// Отрезок, в котором лежит `fc`.
pub(crate) fn find<'s, 'a>(spans: &'s [Span<'a>], fc: u32) -> Option<&'s Span<'a>> {
    let i = spans.partition_point(|s| s.fc_end <= fc);
    spans.get(i).filter(|s| s.fc_start <= fc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chpx_page_is_read() {
        // Страница 1: два отрезка [0x400, 0x410) без свойств и
        // [0x410, 0x420) полужирный.
        let mut word = vec![0u8; 2 * PAGE];
        let page = &mut word[PAGE..];
        for (i, fc) in [0x400u32, 0x410, 0x420].iter().enumerate() {
            page[4 * i..4 * i + 4].copy_from_slice(&fc.to_le_bytes());
        }
        page[12] = 0;
        page[13] = 100; // свойства с байта 200
        page[200] = 3;
        page[201..204].copy_from_slice(&[0x35, 0x08, 0x01]);
        page[PAGE - 1] = 2;
        let mut plc = Vec::new();
        for v in [0x400u32, 0x420, 1] {
            plc.extend(v.to_le_bytes());
        }
        let spans = chpx(&plc, &word);
        assert_eq!(spans.len(), 2);
        assert!(spans[0].grpprl.is_empty());
        assert_eq!(spans[1].grpprl, [0x35, 0x08, 0x01]);
        assert_eq!(find(&spans, 0x415).map(|s| s.fc_start), Some(0x410));
        assert_eq!(find(&spans, 0x420), None);
    }
}
