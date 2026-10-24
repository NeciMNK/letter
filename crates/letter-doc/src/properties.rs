//! Сведения о документе: поток `\u{5}SummaryInformation` (набор свойств
//! OLE, MS-OLEPS) — название, тема, автор, ключевые слова, даты.

use letter_model::CoreProperties;

use crate::fib::{u16_at, u32_at};

/// Типы значений, которые нам нужны.
const VT_I2: u32 = 0x02;
const VT_LPSTR: u32 = 0x1E;
const VT_LPWSTR: u32 = 0x1F;
const VT_FILETIME: u32 = 0x40;

/// Больше свойств в наборе не читаем (защита от испорченных файлов).
const MAX_PROPERTIES: usize = 1024;

/// Сведения из потока `SummaryInformation`. Испорченные свойства
/// пропускаются.
pub(crate) fn read(stream: &[u8]) -> CoreProperties {
    let mut core = CoreProperties::default();
    if u16_at(stream, 0) != Some(0xFFFE) {
        return core;
    }
    let Some(set) = u32_at(stream, 44).and_then(|at| stream.get(at as usize..)) else {
        return core;
    };
    let count = u32_at(set, 4).map_or(0, |n| (n as usize).min(MAX_PROPERTIES));
    let entries: Vec<(u32, usize)> = (0..count)
        .filter_map(|i| Some((u32_at(set, 8 + i * 8)?, u32_at(set, 12 + i * 8)? as usize)))
        .collect();
    let value = |id: u32| entries.iter().find(|(p, _)| *p == id).map(|&(_, at)| at);
    // Кодовая страница строк (свойство 1).
    let code_page = value(1)
        .filter(|&at| u32_at(set, at) == Some(VT_I2))
        .and_then(|at| u16_at(set, at + 4))
        .unwrap_or(1252);
    let text = |id: u32| value(id).and_then(|at| string(set, at, code_page));
    core.title = text(2);
    core.subject = text(3);
    core.creator = text(4);
    core.keywords = text(5);
    core.description = text(6);
    core.last_modified_by = text(8);
    core.revision = text(9);
    let date = |id: u32| {
        value(id)
            .filter(|&at| u32_at(set, at) == Some(VT_FILETIME))
            .and_then(|at| {
                Some(u64::from(u32_at(set, at + 4)?) | u64::from(u32_at(set, at + 8)?) << 32)
            })
            .and_then(filetime)
    };
    core.last_printed = date(11);
    core.created = date(12);
    core.modified = date(13);
    core
}

/// Строка в позиции `at`: однобайтовая в кодовой странице или UTF-16.
/// Пустая — `None`.
fn string(set: &[u8], at: usize, code_page: u16) -> Option<String> {
    let len = u32_at(set, at + 4)? as usize;
    let text = match u32_at(set, at)? {
        VT_LPSTR => {
            let bytes = set.get(at + 8..at + 8 + len)?;
            let bytes = bytes.split(|&b| b == 0).next().unwrap_or_default();
            decode(bytes, code_page)
        }
        VT_LPWSTR => {
            let units: Vec<u16> = (0..len)
                .map_while(|i| u16_at(set, at + 8 + i * 2))
                .take_while(|&u| u != 0)
                .collect();
            String::from_utf16_lossy(&units)
        }
        _ => return None,
    };
    (!text.is_empty()).then_some(text)
}

/// Байты в кодовой странице Windows.
fn decode(bytes: &[u8], code_page: u16) -> String {
    let label = match code_page {
        65001 => "utf-8".to_string(),
        866 => "ibm866".to_string(),
        932 => "shift_jis".to_string(),
        936 => "gbk".to_string(),
        949 => "euc-kr".to_string(),
        950 => "big5".to_string(),
        10000 => "macintosh".to_string(),
        cp => format!("windows-{cp}"),
    };
    let encoding =
        encoding_rs::Encoding::for_label(label.as_bytes()).unwrap_or(encoding_rs::WINDOWS_1252);
    encoding.decode_without_bom_handling(bytes).0.into_owned()
}

/// Время `FILETIME` (сотни наносекунд с 1601 года) как в .docx:
/// `2026-10-23T12:00:00Z`. Ноль и годы до 1970-го — `None`.
fn filetime(value: u64) -> Option<String> {
    const SECONDS_1601_TO_1970: u64 = 11_644_473_600;
    let seconds = (value / 10_000_000).checked_sub(SECONDS_1601_TO_1970)?;
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    let (year, month, day) = civil(days as i64);
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    ))
}

/// Дата по числу дней с 1970-01-01 (алгоритм Говарда Хиннанта).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(19_723), (2024, 1, 1));
        assert_eq!(civil(11_016), (2000, 2, 29));
        // 2026-10-23 12:00:00 UTC.
        let unix = 1_792_756_800u64;
        let ft = (unix + 11_644_473_600) * 10_000_000;
        assert_eq!(filetime(ft).as_deref(), Some("2026-10-23T12:00:00Z"));
        assert_eq!(filetime(0), None);
    }

    #[test]
    fn summary_information() {
        // Набор из двух свойств: кодовая страница 1251 и автор.
        let mut set = Vec::new();
        set.extend(0u32.to_le_bytes()); // размер (не нужен)
        set.extend(2u32.to_le_bytes());
        set.extend([1, 0, 0, 0, 24, 0, 0, 0]);
        set.extend([4, 0, 0, 0, 32, 0, 0, 0]);
        set.extend(VT_I2.to_le_bytes());
        set.extend(1251u16.to_le_bytes());
        set.extend([0, 0]);
        set.extend(VT_LPSTR.to_le_bytes());
        set.extend(5u32.to_le_bytes());
        set.extend([0xC8, 0xE2, 0xE0, 0xED, 0]); // «Иван» в Windows-1251
        let mut stream = vec![0xFE, 0xFF];
        stream.resize(44, 0);
        stream.extend(48u32.to_le_bytes());
        stream.extend(set);
        let core = read(&stream);
        assert_eq!(core.creator.as_deref(), Some("Иван"));
        assert_eq!(core.title, None);
        // Испорченный поток — пустые сведения.
        assert_eq!(read(&stream[..50]), CoreProperties::default());
        assert_eq!(read(b"junk"), CoreProperties::default());
    }
}
