//! Защита документа (`w:documentProtection` в `settings.xml`): что можно
//! править и хеш пароля для снятия защиты.
//!
//! Пароль хешируется как в Word (ECMA-376, часть 4, «Implementation
//! Notes»): сначала старый 32-битный ключ Word 97 (по первым 15 знакам),
//! его байты в обратном порядке записываются шестнадцатеричной строкой
//! (UTF-16LE), затем хеш `соль + строка` и ещё `spin_count` раз хеш
//! `хеш + номер повтора` (4 байта, младший первым).

use letter_model::{PasswordHash, Protection, ProtectionKind};
use sha2::Digest;

use crate::xml::Element;

/// Сколько раз повторять хеширование, как Word 2010 и новее.
pub const SPIN_COUNT: u32 = 100_000;

/// Больше повторов при проверке не делается: испорченный файл не
/// должен «повесить» программу.
const MAX_SPIN_COUNT: u32 = 10_000_000;

const INITIAL_CODE: [u16; 15] = [
    0xE1F0, 0x1D0F, 0xCC9C, 0x84C0, 0x110C, 0x0E10, 0xF1CE, 0x313E, 0x1872, 0xE139, 0xD40F, 0x84F9,
    0x280C, 0xA96A, 0x4EC3,
];

const ENCRYPTION_MATRIX: [[u16; 7]; 15] = [
    [0xAEFC, 0x4DD9, 0x9BB2, 0x2745, 0x4E8A, 0x9D14, 0x2A09],
    [0x7B61, 0xF6C2, 0xFDA5, 0xEB6B, 0xC6F7, 0x9DCF, 0x2BBF],
    [0x4563, 0x8AC6, 0x05AD, 0x0B5A, 0x16B4, 0x2D68, 0x5AD0],
    [0x0375, 0x06EA, 0x0DD4, 0x1BA8, 0x3750, 0x6EA0, 0xDD40],
    [0xD849, 0xA0B3, 0x5147, 0xA28E, 0x553D, 0xAA7A, 0x44D5],
    [0x6F45, 0xDE8A, 0xAD35, 0x4A4B, 0x9496, 0x390D, 0x721A],
    [0xEB23, 0xC667, 0x9CEF, 0x29FF, 0x53FE, 0xA7FC, 0x5FD9],
    [0x47D3, 0x8FA6, 0x0F6D, 0x1EDA, 0x3DB4, 0x7B68, 0xF6D0],
    [0xB861, 0x60E3, 0xC1C6, 0x93AD, 0x377B, 0x6EF6, 0xDDEC],
    [0x45A0, 0x8B40, 0x06A1, 0x0D42, 0x1A84, 0x3508, 0x6A10],
    [0xAA51, 0x4483, 0x8906, 0x022D, 0x045A, 0x08B4, 0x1168],
    [0x76B4, 0xED68, 0xCAF1, 0x85C3, 0x1BA7, 0x374E, 0x6E9C],
    [0x3730, 0x6E60, 0xDCC0, 0xA9A1, 0x4363, 0x86C6, 0x1DAD],
    [0x3331, 0x6662, 0xCCC4, 0x89A9, 0x0373, 0x06E6, 0x0DCC],
    [0x1021, 0x2042, 0x4084, 0x8108, 0x1231, 0x2462, 0x48C4],
];

/// Старый ключ пароля Word 97 (старшее слово — по матрице, младшее —
/// проверочное). Пустой пароль — 0.
fn legacy_key(password: &str) -> u32 {
    // Каждый знак — один байт: младший байт UTF-16, а если он нулевой —
    // старший.
    let bytes: Vec<u8> = password
        .encode_utf16()
        .take(15)
        .map(|c| match (c & 0xFF) as u8 {
            0 => (c >> 8) as u8,
            low => low,
        })
        .collect();
    if bytes.is_empty() {
        return 0;
    }
    let mut high = INITIAL_CODE[bytes.len() - 1];
    for (i, &byte) in bytes.iter().enumerate() {
        let row = ENCRYPTION_MATRIX[15 - bytes.len() + i];
        for (bit, value) in row.iter().enumerate() {
            if byte & (1 << bit) != 0 {
                high ^= value;
            }
        }
    }
    let rotate = |v: u16| ((v >> 14) & 1) | ((v << 1) & 0x7FFF);
    let mut low: u16 = 0;
    for &byte in bytes.iter().rev() {
        low = rotate(low) ^ u16::from(byte);
    }
    low = rotate(low) ^ bytes.len() as u16 ^ 0xCE4B;
    (u32::from(high) << 16) | u32::from(low)
}

/// Строка, которая хешируется: байты старого ключа в обратном порядке,
/// шестнадцатеричными цифрами.
fn legacy_string(password: &str) -> String {
    let key = legacy_key(password);
    key.to_le_bytes()
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect()
}

/// Хеш `data` алгоритмом `algorithm`; `None` — алгоритм не знаем.
fn digest(algorithm: &str, parts: &[&[u8]]) -> Option<Vec<u8>> {
    fn run<D: Digest>(parts: &[&[u8]]) -> Vec<u8> {
        let mut hasher = D::new();
        for part in parts {
            hasher.update(part);
        }
        hasher.finalize().to_vec()
    }
    Some(match algorithm {
        "4" | "SHA-1" => run::<sha1::Sha1>(parts),
        "12" | "SHA-256" => run::<sha2::Sha256>(parts),
        "13" | "SHA-384" => run::<sha2::Sha384>(parts),
        "14" | "SHA-512" => run::<sha2::Sha512>(parts),
        _ => return None,
    })
}

/// Хеш пароля `password` с солью `salt`.
fn hash_password(password: &str, algorithm: &str, salt: &[u8], spin_count: u32) -> Option<Vec<u8>> {
    let text: Vec<u8> = legacy_string(password)
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut hash = digest(algorithm, &[salt, &text])?;
    for i in 0..spin_count {
        hash = digest(algorithm, &[&hash, &i.to_le_bytes()])?;
    }
    Some(hash)
}

/// Хеш нового пароля (SHA-512, случайная соль), как у Word 2013 и
/// новее. Пустой пароль — `None` (защита без пароля).
pub fn protect_password(password: &str) -> Option<PasswordHash> {
    if password.is_empty() {
        return None;
    }
    let mut salt = [0u8; 16];
    if getrandom::fill(&mut salt).is_err() {
        // Без случайных чисел соль — от времени: хуже, но пароль всё
        // равно не хранится открыто.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        salt = now.to_le_bytes();
    }
    let hash = hash_password(password, "14", &salt, SPIN_COUNT)?;
    Some(PasswordHash {
        algorithm: "14".into(),
        named: false,
        hash: base64_encode(&hash),
        salt: base64_encode(&salt),
        spin_count: SPIN_COUNT,
    })
}

/// Подходит ли пароль. Если алгоритм не знаем или хеш испорчен — нет.
pub fn check_password(stored: &PasswordHash, password: &str) -> bool {
    let (Some(salt), Some(expected)) = (base64_decode(&stored.salt), base64_decode(&stored.hash))
    else {
        return false;
    };
    if stored.spin_count > MAX_SPIN_COUNT {
        return false;
    }
    hash_password(password, &stored.algorithm, &salt, stored.spin_count)
        .is_some_and(|hash| hash == expected)
}

/// Защита из элемента `w:documentProtection`.
pub(crate) fn read_protection(element: &Element) -> Protection {
    let kind = match element.w_attr("edit") {
        Some("readOnly") => ProtectionKind::ReadOnly,
        Some("comments") => ProtectionKind::Comments,
        Some("trackedChanges") => ProtectionKind::TrackedChanges,
        Some("forms") => ProtectionKind::Forms,
        _ => ProtectionKind::None,
    };
    let enforced = matches!(element.w_attr("enforcement"), Some("1" | "true" | "on"));
    let named = element.w_attr("hashValue").is_some();
    let password = if named {
        Some(PasswordHash {
            algorithm: element.w_attr("algorithmName").unwrap_or_default().into(),
            named,
            hash: element.w_attr("hashValue").unwrap_or_default().into(),
            salt: element.w_attr("saltValue").unwrap_or_default().into(),
            spin_count: element
                .w_attr("spinCount")
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0),
        })
    } else {
        element.w_attr("hash").map(|hash| PasswordHash {
            algorithm: element.w_attr("cryptAlgorithmSid").unwrap_or("4").into(),
            named,
            hash: hash.into(),
            salt: element.w_attr("salt").unwrap_or_default().into(),
            spin_count: element
                .w_attr("cryptSpinCount")
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0),
        })
    };
    Protection {
        kind,
        enforced,
        password,
    }
}

/// Элемент `w:documentProtection` (`prefix` — префикс пространства
/// имён, например `w:`).
pub(crate) fn protection_xml(protection: &Protection, prefix: &str) -> String {
    let edit = match protection.kind {
        ProtectionKind::ReadOnly => "readOnly",
        ProtectionKind::Comments => "comments",
        ProtectionKind::TrackedChanges => "trackedChanges",
        ProtectionKind::Forms => "forms",
        ProtectionKind::None => "none",
    };
    let mut attrs = vec![
        ("edit", edit.to_string()),
        ("enforcement", u8::from(protection.enforced).to_string()),
    ];
    if let Some(p) = &protection.password {
        if p.named {
            attrs.extend([
                ("algorithmName", p.algorithm.clone()),
                ("hashValue", p.hash.clone()),
                ("saltValue", p.salt.clone()),
                ("spinCount", p.spin_count.to_string()),
            ]);
        } else {
            let provider = if p.algorithm == "4" {
                "rsaFull"
            } else {
                "rsaAES"
            };
            attrs.extend([
                ("cryptProviderType", provider.to_string()),
                ("cryptAlgorithmClass", "hash".to_string()),
                ("cryptAlgorithmType", "typeAny".to_string()),
                ("cryptAlgorithmSid", p.algorithm.clone()),
                ("cryptSpinCount", p.spin_count.to_string()),
                ("hash", p.hash.clone()),
                ("salt", p.salt.clone()),
            ]);
        }
    }
    let attrs: String = attrs
        .iter()
        .map(|(name, value)| {
            format!(
                r#" {prefix}{name}="{}""#,
                crate::serialize::escape_attr(value)
            )
        })
        .collect();
    format!("<{prefix}documentProtection{attrs}/>")
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | (u32::from(b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(BASE64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0;
    for c in text
        .bytes()
        .filter(|c| !c.is_ascii_whitespace() && *c != b'=')
    {
        let value = BASE64.iter().position(|&b| b == c)? as u32;
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_key_matches_the_specification() {
        // Пример из ECMA-376: «Example» → 0x64CEED7E, наоборот 7EEDCE64.
        assert_eq!(legacy_key("Example"), 0x64CE_ED7E);
        assert_eq!(legacy_string("Example"), "7EEDCE64");
        assert_eq!(legacy_key(""), 0);
    }

    #[test]
    fn base64_round_trips() {
        for bytes in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar"] {
            assert_eq!(base64_decode(&base64_encode(bytes)).as_deref(), Some(bytes));
        }
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_decode("Zm8="), Some(b"fo".to_vec()));
        assert_eq!(base64_decode("Zm8*"), None);
    }

    #[test]
    fn passwords_are_hashed_and_checked() {
        let stored = protect_password("секрет").unwrap();
        assert_eq!(stored.algorithm, "14");
        assert!(check_password(&stored, "секрет"));
        assert!(!check_password(&stored, "Секрет"));
        assert!(protect_password("").is_none());
        // Соль каждый раз новая.
        assert_ne!(protect_password("секрет").unwrap().salt, stored.salt);
        // Хеш из файла Word (SHA-1, 100000 повторов) с паролем «Example»
        // (bug56076.docx из тестов Apache POI).
        let word = PasswordHash {
            algorithm: "4".into(),
            named: false,
            hash: "MUHbcmpC9AnlLsd9v3lW0j30y6E=".into(),
            salt: "2Z+i7o/0EZyUNakVeWzU/w==".into(),
            spin_count: 100_000,
        };
        assert!(check_password(&word, "Example"));
        assert!(!check_password(&word, "example"));
        // Ведущие нули ключа сохраняются.
        assert_eq!(legacy_key("34579"), 0x0005_CB00);
    }
}
