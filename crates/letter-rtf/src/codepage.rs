//! Кодировки текста в RTF: байты `\'hh` и знаки выше 127 записаны
//! в кодовой странице шрифта (`\fcharset`) или документа (`\ansicpg`).

use encoding_rs::Encoding;

/// Кодовая страница по набору символов шрифта (`\fcharsetN`).
/// `None` — набор не задаёт страницу (берётся страница документа).
pub fn charset_codepage(charset: i32) -> Option<u16> {
    let codepage = match charset {
        77 => 10000,
        128 => 932,
        129 => 949,
        130 => 1361,
        134 => 936,
        136 => 950,
        161 => 1253,
        162 => 1254,
        163 => 1258,
        177 => 1255,
        178 => 1256,
        186 => 1257,
        204 => 1251,
        222 => 874,
        238 => 1250,
        254 => 437,
        255 => 850,
        _ => return None,
    };
    Some(codepage)
}

/// Набор символов шрифта, у которого шрифт «Symbol»-типа: байты —
/// это номера значков, а не буквы.
pub const SYMBOL_CHARSET: i32 = 2;

/// Переводит байты в текст в кодовой странице `codepage`. Незнакомая
/// страница читается как Windows-1252, как делает Word.
pub fn decode(bytes: &[u8], codepage: u16) -> String {
    if bytes.is_ascii() {
        return bytes.iter().map(|&b| char::from(b)).collect();
    }
    let encoding = encoding(codepage);
    let (text, _, _) = encoding.decode(bytes);
    text.into_owned()
}

/// Значки шрифта Symbol: Word показывает их из области U+F0xx.
pub fn decode_symbol(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if b < 0x20 {
                char::from(b)
            } else {
                char::from_u32(0xF000 + u32::from(b)).unwrap_or(char::REPLACEMENT_CHARACTER)
            }
        })
        .collect()
}

fn encoding(codepage: u16) -> &'static Encoding {
    let label = match codepage {
        874 => "windows-874",
        932 => "shift_jis",
        936 => "gbk",
        949 => "euc-kr",
        950 => "big5",
        866 => "ibm866",
        10000 => "macintosh",
        1250..=1258 => return windows(codepage),
        _ => "windows-1252",
    };
    Encoding::for_label(label.as_bytes()).unwrap_or(encoding_rs::WINDOWS_1252)
}

fn windows(codepage: u16) -> &'static Encoding {
    Encoding::for_label(format!("windows-{codepage}").as_bytes())
        .unwrap_or(encoding_rs::WINDOWS_1252)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cyrillic_and_western_pages() {
        assert_eq!(decode(b"\xcf\xf0\xe8\xe2\xe5\xf2", 1251), "Привет");
        assert_eq!(decode(b"caf\xe9", 1252), "café");
        // Незнакомая страница — как Windows-1252.
        assert_eq!(decode(b"\xe9", 12345), "é");
        assert_eq!(decode(b"\x82\xa0", 932), "あ");
        assert_eq!(decode(b"plain", 1251), "plain");
    }

    #[test]
    fn charsets_map_to_pages() {
        assert_eq!(charset_codepage(204), Some(1251));
        assert_eq!(charset_codepage(0), None);
        assert_eq!(decode_symbol(b"\xb7"), "\u{f0b7}");
    }
}
