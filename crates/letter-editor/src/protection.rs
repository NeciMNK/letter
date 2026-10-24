//! Пароли защиты документа: хеш, как его считает Word 2013 и новее
//! (SHA-512, соль, 100 000 повторов), и проверка введённого пароля.
//!
//! Старый вид хеша (Word 2007, `w:cryptProviderType` и `w:hash`) пока
//! не проверяется: такую защиту можно снять только без пароля.

use letter_model::{DocumentProtection, EditRestriction, PasswordHash, WriteProtection};
use sha2::{Digest, Sha256, Sha384, Sha512};

/// Столько повторов хеша ставит Word.
pub const SPIN_COUNT: u32 = 100_000;

/// Хеш пароля `password` с солью `salt` по алгоритму `algorithm`
/// (`SHA-256`, `SHA-384`, `SHA-512`); `None` — алгоритм не знаком.
pub fn password_hash(
    algorithm: &str,
    password: &str,
    salt: &[u8],
    spin_count: u32,
) -> Option<Vec<u8>> {
    fn spin<D: Digest>(password: &str, salt: &[u8], spin_count: u32) -> Vec<u8> {
        let mut first = D::new();
        first.update(salt);
        for unit in password.encode_utf16() {
            first.update(unit.to_le_bytes());
        }
        let mut hash = first.finalize().to_vec();
        for i in 0..spin_count {
            let mut next = D::new();
            next.update(&hash);
            next.update(i.to_le_bytes());
            hash = next.finalize().to_vec();
        }
        hash
    }
    Some(match algorithm.trim().to_ascii_uppercase().as_str() {
        "SHA-256" | "SHA256" => spin::<Sha256>(password, salt, spin_count),
        "SHA-384" | "SHA384" => spin::<Sha384>(password, salt, spin_count),
        "SHA-512" | "SHA512" => spin::<Sha512>(password, salt, spin_count),
        _ => return None,
    })
}

/// Новый хеш пароля со случайной солью, как пишет Word.
pub fn new_password(password: &str) -> PasswordHash {
    let mut salt = [0u8; 16];
    if getrandom::fill(&mut salt).is_err() {
        // Без генератора случайных чисел — хоть что-то неповторяющееся.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        salt = now.to_le_bytes();
    }
    let hash = password_hash("SHA-512", password, &salt, SPIN_COUNT).unwrap_or_default();
    PasswordHash {
        algorithm: "SHA-512".into(),
        hash: base64_encode(&hash),
        salt: base64_encode(&salt),
        spin_count: SPIN_COUNT,
    }
}

/// Подходит ли пароль к хешу. Незнакомый алгоритм или испорченный
/// Base64 — не подходит.
pub fn check_password(hash: &PasswordHash, password: &str) -> bool {
    let (Some(expected), Some(salt)) = (base64_decode(&hash.hash), base64_decode(&hash.salt))
    else {
        return false;
    };
    password_hash(&hash.algorithm, password, &salt, hash.spin_count)
        .is_some_and(|actual| actual == expected)
}

/// Защита «только чтение» и т. п. с паролем (пустой — без пароля).
pub fn protect(edit: EditRestriction, password: &str) -> DocumentProtection {
    DocumentProtection {
        edit,
        enforced: true,
        password: (!password.is_empty()).then(|| new_password(password)),
        other: Vec::new(),
    }
}

/// Можно ли снять защиту этим паролем: без пароля — всегда, со старым
/// видом хеша (`other` не пуст, нового хеша нет) — тоже.
pub fn can_unprotect(protection: &DocumentProtection, password: &str) -> bool {
    match &protection.password {
        Some(hash) => check_password(hash, password),
        None => true,
    }
}

/// Можно ли этим паролем разрешить правку файла с паролем на изменение.
pub fn can_modify(protection: &WriteProtection, password: &str) -> bool {
    match &protection.password {
        Some(hash) => check_password(hash, password),
        None => true,
    }
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Base64 (с `=` в конце).
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Разбирает Base64; пробелы пропускаются, ошибка — `None`.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0;
    for c in text.bytes().filter(|c| !c.is_ascii_whitespace()) {
        if c == b'=' {
            break;
        }
        let v = ALPHABET.iter().position(|&a| a == c)? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits & 0xFF) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trip() {
        for data in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar"] {
            let text = base64_encode(data);
            assert_eq!(base64_decode(&text).as_deref(), Some(data));
        }
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_decode("Zm9v!"), None);
    }

    #[test]
    fn own_hash_checks_own_password() {
        // SHA-512 от соли и «a» в UTF-16LE без повторов — то же, что
        // у любой реализации алгоритма.
        let once = password_hash("SHA-512", "a", b"s", 0).unwrap_or_default();
        let mut plain = Sha512::new();
        plain.update(b"sa\0");
        assert_eq!(once, plain.finalize().to_vec());
        let mine = new_password("секрет");
        assert!(check_password(&mine, "секрет"));
        assert!(!check_password(&mine, "Секрет"));
        assert!(!check_password(&mine, ""));
    }

    #[test]
    fn protection_with_and_without_password() {
        let open = protect(EditRestriction::ReadOnly, "");
        assert!(open.locks_editing() && open.password.is_none());
        assert!(can_unprotect(&open, "что угодно"));
        let closed = protect(EditRestriction::Forms, "1");
        assert!(can_unprotect(&closed, "1"));
        assert!(!can_unprotect(&closed, "2"));
        let unknown = PasswordHash {
            algorithm: "MD2".into(),
            ..new_password("x")
        };
        assert!(!check_password(&unknown, "x"));
    }
}
