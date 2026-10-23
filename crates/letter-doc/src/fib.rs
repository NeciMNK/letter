//! Заголовок файла (FIB, MS-DOC 2.5): где в потоках лежат части
//! документа и сколько в нём знаков.

use crate::ReadError;

/// Признак документа Word в начале потока `WordDocument`.
const W_IDENT: u16 = 0xA5EC;

/// Самая ранняя версия формата Word 97 (`nFib`). У Word 6/95 меньше.
const NFIB_WORD97: u16 = 0x00C0;

/// Номера пар «смещение — длина» в `FibRgFcLcb97`, которые нам нужны.
pub(crate) mod index {
    /// Таблица стилей (`STSH`).
    pub const STSHF: usize = 1;
    /// Разделы (`PlcfSed`).
    pub const PLCF_SED: usize = 6;
    /// Страницы оформления знаков (`PlcBteChpx`).
    pub const PLCF_BTE_CHPX: usize = 12;
    /// Страницы оформления абзацев (`PlcBtePapx`).
    pub const PLCF_BTE_PAPX: usize = 13;
    /// Шрифты (`SttbfFfn`).
    pub const STTBF_FFN: usize = 15;
    /// Таблица кусков текста (`Clx`).
    pub const CLX: usize = 33;
}

/// Нужное из заголовка.
#[derive(Debug, Clone)]
pub(crate) struct Fib {
    /// Таблицы лежат в потоке `1Table` (иначе `0Table`).
    pub table_1: bool,
    /// Знаков в основном тексте.
    pub ccp_text: u32,
    /// Пары «смещение в табличном потоке — длина».
    pub fc_lcb: Vec<(u32, u32)>,
}

impl Fib {
    /// Разбирает заголовок в начале потока `WordDocument`.
    pub fn read(stream: &[u8]) -> Result<Fib, ReadError> {
        let short = || ReadError::Corrupt("короткий заголовок".into());
        if u16_at(stream, 0).ok_or_else(short)? != W_IDENT {
            return Err(ReadError::NotADoc);
        }
        let n_fib = u16_at(stream, 2).ok_or_else(short)?;
        let flags = u16_at(stream, 10).ok_or_else(short)?;
        if flags & 0x0100 != 0 {
            return Err(ReadError::Encrypted);
        }
        if n_fib < NFIB_WORD97 {
            return Err(ReadError::OldVersion);
        }
        let csw = usize::from(u16_at(stream, 32).ok_or_else(short)?);
        let at = 34 + csw * 2;
        let cslw = usize::from(u16_at(stream, at).ok_or_else(short)?);
        let rg_lw = at + 2;
        let ccp_text = u32_at(stream, rg_lw + 3 * 4).ok_or_else(short)?;
        let at = rg_lw + cslw * 4;
        let count = usize::from(u16_at(stream, at).ok_or_else(short)?);
        let blob = at + 2;
        let fc_lcb = (0..count)
            .map(|i| {
                Some((
                    u32_at(stream, blob + i * 8)?,
                    u32_at(stream, blob + i * 8 + 4)?,
                ))
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(short)?;
        Ok(Fib {
            table_1: flags & 0x0200 != 0,
            ccp_text,
            fc_lcb,
        })
    }

    /// Кусок табличного потока по номеру пары; `None` — части нет или
    /// она выходит за поток.
    pub fn part<'t>(&self, table: &'t [u8], index: usize) -> Option<&'t [u8]> {
        let &(fc, lcb) = self.fc_lcb.get(index)?;
        if lcb == 0 {
            return None;
        }
        table.get(fc as usize..(fc as usize).checked_add(lcb as usize)?)
    }
}

pub(crate) fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

pub(crate) fn i16_at(bytes: &[u8], at: usize) -> Option<i16> {
    Some(i16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

pub(crate) fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// Таблица `PLC`: `n + 1` позиций по 4 байта и `n` записей по `size`
/// байт. Возвращает позиции и записи.
pub(crate) fn plc(bytes: &[u8], size: usize) -> Option<(Vec<u32>, Vec<&[u8]>)> {
    if bytes.len() < 4 {
        return None;
    }
    let n = (bytes.len() - 4) / (4 + size);
    let positions = (0..=n)
        .map(|i| u32_at(bytes, i * 4))
        .collect::<Option<Vec<_>>>()?;
    let data = (n + 1) * 4;
    let entries = (0..n)
        .map(|i| bytes.get(data + i * size..data + (i + 1) * size))
        .collect::<Option<Vec<_>>>()?;
    Some((positions, entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plc_is_split_into_positions_and_entries() {
        let mut bytes = Vec::new();
        for p in [0u32, 10, 20] {
            bytes.extend(p.to_le_bytes());
        }
        bytes.extend([1, 2, 3, 4, 5, 6]);
        let (positions, entries) = plc(&bytes, 3).unwrap();
        assert_eq!(positions, [0, 10, 20]);
        assert_eq!(entries, [&[1, 2, 3][..], &[4, 5, 6][..]]);
        assert!(plc(&[1, 2], 3).is_none());
    }

    #[test]
    fn not_a_word_document() {
        assert!(matches!(Fib::read(&[0; 600]), Err(ReadError::NotADoc)));
        assert!(matches!(Fib::read(&[]), Err(ReadError::Corrupt(_))));
        let mut old = vec![0; 600];
        old[..2].copy_from_slice(&W_IDENT.to_le_bytes());
        old[2..4].copy_from_slice(&0x0065u16.to_le_bytes());
        assert!(matches!(Fib::read(&old), Err(ReadError::OldVersion)));
        old[10] = 0;
        old[11] = 0x01;
        assert!(matches!(Fib::read(&old), Err(ReadError::Encrypted)));
    }
}
