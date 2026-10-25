//! Текст документа: таблица кусков (`Clx`, `PlcPcd`). Текст .doc лежит
//! в потоке `WordDocument` кусками; каждый кусок — либо по байту на знак
//! («сжатый», кодовая страница 1252), либо UTF-16. Номер знака в тексте
//! (CP) переводится в место в потоке (FC) через эту таблицу.

use crate::ReadError;
use crate::fib::{u16_at, u32_at};

/// Кусок текста.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Piece {
    /// Первый знак куска (CP).
    pub cp_start: u32,
    /// Знак после последнего.
    pub cp_end: u32,
    /// Место первого знака в потоке `WordDocument`, байты.
    pub fc: u32,
    /// По байту на знак.
    pub compressed: bool,
}

impl Piece {
    /// Место знака `cp` куска в потоке.
    pub fn fc_of(&self, cp: u32) -> u32 {
        let width = if self.compressed { 1 } else { 2 };
        self.fc + (cp - self.cp_start) * width
    }
}

/// Таблица кусков из `Clx` (поток таблиц).
pub(crate) fn pieces(clx: &[u8]) -> Result<Vec<Piece>, ReadError> {
    let bad = || ReadError::Corrupt("таблица кусков текста");
    let mut at = 0;
    // Сначала идут необязательные `Prc` (оформление кусков) — пропускаем.
    while clx.get(at) == Some(&0x01) {
        let size = u16_at(clx, at + 1).ok_or_else(bad)? as i16;
        at += 3 + usize::try_from(size).map_err(|_| bad())?;
    }
    if clx.get(at) != Some(&0x02) {
        return Err(bad());
    }
    let lcb = u32_at(clx, at + 1).ok_or_else(bad)? as usize;
    let plc = clx.get(at + 5..at + 5 + lcb).ok_or_else(bad)?;
    if plc.len() < 4 || (plc.len() - 4) % 12 != 0 {
        return Err(bad());
    }
    let count = (plc.len() - 4) / 12;
    let mut list = Vec::with_capacity(count);
    for i in 0..count {
        let cp_start = u32_at(plc, 4 * i).ok_or_else(bad)?;
        let cp_end = u32_at(plc, 4 * (i + 1)).ok_or_else(bad)?;
        let pcd = 4 * (count + 1) + 8 * i;
        let raw = u32_at(plc, pcd + 2).ok_or_else(bad)?;
        let compressed = raw & 0x4000_0000 != 0;
        let fc = raw & 0x3FFF_FFFF;
        let fc = if compressed { fc / 2 } else { fc };
        if cp_end < cp_start {
            return Err(bad());
        }
        list.push(Piece {
            cp_start,
            cp_end,
            fc,
            compressed,
        });
    }
    Ok(list)
}

/// Знак кодовой страницы 1252 (сжатый текст).
fn cp1252(byte: u8) -> char {
    const HIGH: [u16; 32] = [
        0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160,
        0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013,
        0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
    ];
    match byte {
        0x80..=0x9F => char::from_u32(u32::from(HIGH[usize::from(byte - 0x80)])).unwrap_or('?'),
        _ => char::from(byte),
    }
}

/// Текст от знака `from` до `to` (CP) как UTF-16: по знаку на элемент,
/// с номером знака (CP) и местом в потоке (FC). Чего нет в таблице кусков
/// или потоке — пропускается.
pub(crate) fn text(pieces: &[Piece], stream: &[u8], from: u32, to: u32) -> Vec<(u32, u32, u16)> {
    let mut out = Vec::new();
    for piece in pieces {
        let start = piece.cp_start.max(from);
        let end = piece.cp_end.min(to);
        for cp in start..end {
            let fc = piece.fc_of(cp);
            let at = fc as usize;
            let unit = if piece.compressed {
                stream.get(at).map(|&b| cp1252(b) as u16)
            } else {
                u16_at(stream, at)
            };
            if let Some(unit) = unit {
                out.push((cp, fc, unit));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_map_cps_to_stream() {
        // Два куска: «Hi» сжатый с байта 0x10, «Ж» в UTF-16 с 0x20.
        let mut plc = Vec::new();
        for cp in [0u32, 2, 3] {
            plc.extend(cp.to_le_bytes());
        }
        for (fc, compressed) in [(0x10u32, true), (0x20, false)] {
            plc.extend(0u16.to_le_bytes());
            let raw = if compressed {
                (fc * 2) | 0x4000_0000
            } else {
                fc
            };
            plc.extend(raw.to_le_bytes());
            plc.extend(0u16.to_le_bytes());
        }
        let mut clx = vec![0x01, 2, 0, 0xAA, 0xBB, 0x02];
        clx.extend((plc.len() as u32).to_le_bytes());
        clx.extend(&plc);
        let list = pieces(&clx).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].fc, 0x10);
        let mut stream = vec![0u8; 0x30];
        stream[0x10] = b'H';
        stream[0x11] = 0x93; // “
        stream[0x20..0x22].copy_from_slice(&0x0416u16.to_le_bytes());
        let units: Vec<u16> = text(&list, &stream, 0, 3).iter().map(|u| u.2).collect();
        assert_eq!(String::from_utf16_lossy(&units), "H“Ж");
        assert_eq!(list[1].fc_of(2), 0x20);
        assert!(pieces(&[0x03]).is_err());
    }
}
