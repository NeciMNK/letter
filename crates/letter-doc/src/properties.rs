//! Сведения о документе: поток `\u{5}SummaryInformation` (набор свойств
//! OLE, MS-OLEPS) — название, тема, автор, ключевые слова, даты. Читается
//! и записывается.

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

/// Набор свойств `SummaryInformation`.
const FMTID_SUMMARY: [u8; 16] = [
    0xE0, 0x85, 0x9F, 0xF2, 0xF9, 0x4F, 0x68, 0x10, 0xAB, 0x91, 0x08, 0x00, 0x2B, 0x27, 0xB3, 0xD9,
];

/// Поток `SummaryInformation` со сведениями `core`; строки — в UTF-8
/// (кодовая страница 65001). Пустые сведения — `None`.
pub(crate) fn write(core: &CoreProperties) -> Option<Vec<u8>> {
    let mut values: Vec<(u32, Vec<u8>)> = Vec::new();
    for (id, text) in [
        (2, &core.title),
        (3, &core.subject),
        (4, &core.creator),
        (5, &core.keywords),
        (6, &core.description),
        (8, &core.last_modified_by),
        (9, &core.revision),
    ] {
        if let Some(text) = text.as_deref().filter(|t| !t.is_empty()) {
            let mut value = VT_LPSTR.to_le_bytes().to_vec();
            value.extend((text.len() as u32 + 1).to_le_bytes());
            value.extend(text.as_bytes());
            value.push(0);
            values.push((id, value));
        }
    }
    for (id, date) in [
        (11, &core.last_printed),
        (12, &core.created),
        (13, &core.modified),
    ] {
        if let Some(time) = date.as_deref().and_then(parse_filetime) {
            let mut value = VT_FILETIME.to_le_bytes().to_vec();
            value.extend(time.to_le_bytes());
            values.push((id, value));
        }
    }
    if values.is_empty() {
        return None;
    }
    let mut code_page = VT_I2.to_le_bytes().to_vec();
    code_page.extend(65001u16.to_le_bytes());
    values.insert(0, (1, code_page));

    let mut set = Vec::new();
    let mut offset = 8 + values.len() * 8;
    let mut body = Vec::new();
    for (id, value) in &values {
        set.extend(id.to_le_bytes());
        set.extend((offset as u32).to_le_bytes());
        body.extend(value);
        // Каждое значение — с границы 4 байт.
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 8 + values.len() * 8 + body.len();
    }
    let size = 8 + set.len() + body.len();
    let mut out = vec![0xFE, 0xFF, 0, 0];
    // Windows, версия 6.
    out.extend(0x0002_0006u32.to_le_bytes());
    out.extend([0; 16]);
    out.extend(1u32.to_le_bytes());
    out.extend(FMTID_SUMMARY);
    out.extend(48u32.to_le_bytes());
    out.extend((size as u32).to_le_bytes());
    out.extend((values.len() as u32).to_le_bytes());
    out.extend(set);
    out.extend(body);
    Some(out)
}

/// Время как в .docx (`2026-10-23T12:00:00Z`) в `FILETIME`.
fn parse_filetime(text: &str) -> Option<u64> {
    const SECONDS_1601_TO_1970: i64 = 11_644_473_600;
    let number = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = if text.len() >= 19 {
        (number(11..13)?, number(14..16)?, number(17..19)?)
    } else {
        (0, 0, 0)
    };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second;
    u64::try_from(seconds + SECONDS_1601_TO_1970)
        .ok()?
        .checked_mul(10_000_000)
}

/// Число дней с 1970-01-01 по дате (обратное к [`civil`]).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
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
        assert_eq!(parse_filetime("2026-10-23T12:00:00Z"), Some(ft));
        assert_eq!(days_from_civil(2000, 2, 29), 11_016);
        assert_eq!(parse_filetime("вчера"), None);
    }

    #[test]
    fn write_and_read_back() {
        let core = CoreProperties {
            title: Some("Отчёт".into()),
            creator: Some("Иван".into()),
            keywords: Some("doc, word".into()),
            created: Some("2026-10-23T12:00:00Z".into()),
            ..CoreProperties::default()
        };
        let stream = write(&core).unwrap();
        assert_eq!(read(&stream), core);
        assert_eq!(write(&CoreProperties::default()), None);
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
