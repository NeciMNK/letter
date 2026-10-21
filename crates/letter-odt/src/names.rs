//! Имена стилей ODF и идентификаторы стилей модели.
//!
//! LibreOffice записывает в `style:name` имя, в котором недопустимые
//! знаки заменены кодом: пробел — `_20_`, поэтому «Heading 1»
//! хранится как `Heading_20_1`. Модель, как Word, использует
//! идентификаторы без пробелов (`Heading1`).

/// Имя стиля «Обычный» в ODF (в LibreOffice — «Базовый»).
pub(crate) const STANDARD: &str = "Standard";

/// Раскодирует `_20_` и подобные вставки в имени ODF.
pub(crate) fn decode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut rest = name;
    while let Some(start) = rest.find('_') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let decoded = after.find('_').and_then(|end| {
            let hex = &after[..end];
            let valid = (1..=6).contains(&hex.len()) && hex.chars().all(|c| c.is_ascii_hexdigit());
            let c = u32::from_str_radix(hex, 16)
                .ok()
                .filter(|_| valid)
                .and_then(char::from_u32)
                .filter(|c| *c >= ' ')?;
            Some((c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('_');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Кодирует имя для `style:name`: всё, кроме латинских букв, цифр,
/// `-` и `.`, заменяется вставкой `_код_`.
pub(crate) fn encode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for (i, c) in name.chars().enumerate() {
        let allowed =
            c.is_ascii_alphabetic() || (i > 0 && (c.is_ascii_digit() || c == '-' || c == '.'));
        if allowed {
            out.push(c);
        } else {
            out.push_str(&format!("_{:x}_", c as u32));
        }
    }
    if out.is_empty() {
        out.push_str("Style");
    }
    out
}

/// Идентификатор стиля модели по имени ODF: `Heading_20_1` → `Heading1`,
/// `Standard` → `Normal`.
pub(crate) fn style_id(odf_name: &str) -> String {
    let name = decode(odf_name);
    if name == STANDARD {
        return "Normal".into();
    }
    if !name.contains(char::is_whitespace) {
        return name;
    }
    name.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect()
}

/// Имя, которое видит пользователь: `style:display-name` или
/// раскодированное `style:name`.
pub(crate) fn display_name(odf_name: &str, display: Option<&str>) -> String {
    if odf_name == STANDARD {
        return "Normal".into();
    }
    display.map_or_else(|| decode(odf_name), str::to_string)
}

/// Уровень заголовка по имени стиля: «heading 2» → 2.
pub(crate) fn heading_level(name: &str) -> Option<u8> {
    let name = name.to_ascii_lowercase().replace(' ', "");
    let level: u8 = name.strip_prefix("heading")?.parse().ok()?;
    (1..=10).contains(&level).then_some(level)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoding() {
        assert_eq!(decode("Heading_20_1"), "Heading 1");
        assert_eq!(decode("Text_20_body"), "Text body");
        assert_eq!(decode("WW_CharLFO1LVL1"), "WW_CharLFO1LVL1");
        assert_eq!(decode("a_1_b"), "a_1_b");
        assert_eq!(decode("x_"), "x_");
        assert_eq!(decode("_e4_b"), "äb");
    }

    #[test]
    fn encoding_reads_back() {
        for name in ["Heading 1", "Мой стиль", "a_b", "1st", "List-2.x", ""] {
            let encoded = encode(name);
            assert!(encoded.is_ascii(), "{encoded}");
            if !name.is_empty() {
                assert_eq!(decode(&encoded), name);
            }
        }
    }

    #[test]
    fn ids() {
        assert_eq!(style_id("Heading_20_1"), "Heading1");
        assert_eq!(style_id("Standard"), "Normal");
        assert_eq!(style_id("Quotations"), "Quotations");
        assert_eq!(style_id("myStyle"), "myStyle");
        assert_eq!(display_name("Heading_20_1", None), "Heading 1");
        assert_eq!(display_name("Heading1", Some("heading 1")), "heading 1");
        assert_eq!(heading_level("heading 3"), Some(3));
        assert_eq!(heading_level("Heading 10"), Some(10));
        assert_eq!(heading_level("Title"), None);
    }
}
