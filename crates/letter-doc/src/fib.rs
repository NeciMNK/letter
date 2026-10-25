//! Заголовок файла (FIB, File Information Block) в начале потока
//! `WordDocument`: версия, флаги, длины частей текста и места таблиц в
//! потоке `0Table`/`1Table`.

use crate::ReadError;

/// Подпись файла Word (`wIdent`).
const W_IDENT: u16 = 0xA5EC;

/// Самая старая понятная версия (`nFib`): Word 97.
const MIN_NFIB: u16 = 0x00C1;

/// Номера пар «смещение, длина» в `FibRgFcLcb97`.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub(crate) enum Table {
    /// Стили (`STSH`).
    Stshf = 1,
    /// Места сносок в тексте.
    PlcffndRef = 2,
    /// Текст сносок.
    PlcffndTxt = 3,
    /// Разделы (`PlcfSed`).
    PlcfSed = 6,
    /// Колонтитулы (`PlcfHdd`).
    PlcfHdd = 11,
    /// Оформление знаков (`PlcfBteChpx`).
    PlcfBteChpx = 12,
    /// Оформление абзацев (`PlcfBtePapx`).
    PlcfBtePapx = 13,
    /// Шрифты (`SttbfFfn`).
    SttbfFfn = 15,
    /// Поля основного текста (`PlcfFldMom`).
    PlcfFldMom = 16,
    /// Параметры документа (`Dop`).
    Dop = 31,
    /// Куски текста (`Clx`).
    Clx = 33,
    /// Списки (`PlfLst`).
    PlfLst = 73,
    /// Переопределения списков (`PlfLfo`).
    PlfLfo = 74,
}

/// Нужное из заголовка.
#[derive(Debug, Clone)]
pub(crate) struct Fib {
    /// Таблицы лежат в потоке `1Table` (иначе `0Table`).
    pub which_table: bool,
    /// Длина основного текста, знаков. За ним в том же потоке идут
    /// сноски, колонтитулы, примечания и т. д.
    pub ccp_text: u32,
    /// Пары «смещение, длина» таблиц в потоке таблиц.
    fc_lcb: Vec<(u32, u32)>,
}

pub(crate) fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

pub(crate) fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

impl Fib {
    /// Читает заголовок из начала потока `WordDocument`.
    pub fn read(data: &[u8]) -> Result<Fib, ReadError> {
        let bad = || ReadError::Corrupt("заголовок");
        if u16_at(data, 0) != Some(W_IDENT) {
            return Err(ReadError::NotDoc);
        }
        let n_fib = u16_at(data, 2).ok_or_else(bad)?;
        if n_fib < MIN_NFIB {
            return Err(ReadError::TooOld);
        }
        let flags = u16_at(data, 0x0A).ok_or_else(bad)?;
        if flags & 0x0100 != 0 {
            return Err(ReadError::Encrypted);
        }
        let which_table = flags & 0x0200 != 0;
        let mut at = 32;
        let csw = usize::from(u16_at(data, at).ok_or_else(bad)?);
        at += 2 + csw * 2;
        let cslw = usize::from(u16_at(data, at).ok_or_else(bad)?);
        let lw = at + 2;
        let long = |i: usize| -> u32 {
            if i < cslw {
                u32_at(data, lw + 4 * i).unwrap_or(0)
            } else {
                0
            }
        };
        at = lw + cslw * 4;
        let count = usize::from(u16_at(data, at).ok_or_else(bad)?);
        let pairs = at + 2;
        let fc_lcb = (0..count)
            .map_while(|i| {
                let at = pairs + 8 * i;
                Some((u32_at(data, at)?, u32_at(data, at + 4)?))
            })
            .collect();
        Ok(Fib {
            which_table,
            ccp_text: long(3),
            fc_lcb,
        })
    }

    /// Кусок потока таблиц `table` (пусто, если его нет).
    pub fn table<'a>(&self, stream: &'a [u8], table: Table) -> &'a [u8] {
        let Some(&(fc, lcb)) = self.fc_lcb.get(table as usize) else {
            return &[];
        };
        let (fc, lcb) = (fc as usize, lcb as usize);
        stream.get(fc..fc.saturating_add(lcb)).unwrap_or(&[])
    }
}
