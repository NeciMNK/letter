//! Шифрованные документы: пароль на открытие (MS-OFFCRYPTO, ECMA-376).
//!
//! Зашифрованный документ Word — не ZIP-архив, а составной файл OLE
//! (тот же контейнер, что у старого .doc). Внутри лежат поток
//! `EncryptionInfo` (как получить ключ из пароля), поток
//! `EncryptedPackage` (зашифрованный ZIP-архив .docx) и служебная папка
//! `\x06DataSpaces`, без которой Word файл не откроет.
//!
//! Читаются оба способа, которыми шифрует Word: «агильный» (Agile,
//! Word 2010 и новее: AES и SHA-512, 100 000 повторов хеша) и
//! «стандартный» (Standard, Word 2007: AES и SHA-1). Записывается только
//! агильный — его Word и предлагает сам.

use std::io::{Cursor, Read, Seek, Write};

use aes::cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use sha1::Digest;

use crate::xml;
use crate::{ReadError, WriteError};

/// Начало любого составного файла OLE.
const OLE_SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Самый большой поток, который мы согласны прочитать (512 МБ), — как
/// и самая большая часть ZIP-архива.
const MAX_STREAM_SIZE: u64 = 512 * 1024 * 1024;

/// Больше повторов хеша Word не ставит; файл с большим числом скорее
/// всего сделан, чтобы программа «зависла» на подборе ключа.
const MAX_SPIN_COUNT: u32 = 10_000_000;

/// Повторы хеша при записи — как у Word.
const SPIN_COUNT: u32 = 100_000;

/// Части ключа (`blockKey`) агильного шифрования: для проверки пароля,
/// для ключа документа и для подписи целостности (HMAC).
const BLOCK_VERIFIER_INPUT: [u8; 8] = [0xfe, 0xa7, 0xd2, 0x76, 0x3b, 0x4b, 0x9e, 0x79];
const BLOCK_VERIFIER_HASH: [u8; 8] = [0xd7, 0xaa, 0x0f, 0x6d, 0x30, 0x61, 0x34, 0x4e];
const BLOCK_KEY: [u8; 8] = [0x14, 0x6e, 0x0b, 0xe7, 0xab, 0xac, 0xd0, 0xd6];
const BLOCK_HMAC_KEY: [u8; 8] = [0x5f, 0xb2, 0xad, 0x01, 0x0c, 0xb9, 0xe1, 0xf6];
const BLOCK_HMAC_VALUE: [u8; 8] = [0xa0, 0x67, 0x7f, 0x02, 0xb2, 0x2c, 0x84, 0x33];

/// Зашифрованный пакет делится на куски по 4096 байт, у каждого свой
/// вектор инициализации.
const SEGMENT: usize = 4096;

/// Составной ли это файл OLE (по первым байтам). Так начинаются
/// зашифрованные документы и старые .doc.
pub(crate) fn is_ole(bytes: &[u8]) -> bool {
    bytes.starts_with(&OLE_SIGNATURE)
}

/// Есть ли в составном файле поток зашифрованного документа.
pub(crate) fn is_encrypted_package(bytes: &[u8]) -> bool {
    is_ole(bytes)
        && cfb::CompoundFile::open(Cursor::new(bytes)).is_ok_and(|file| {
            file.is_stream("/EncryptionInfo") && file.is_stream("/EncryptedPackage")
        })
}

/// Расшифровывает документ паролем; возвращает байты ZIP-архива .docx.
pub(crate) fn decrypt(bytes: &[u8], password: &str) -> Result<Vec<u8>, ReadError> {
    let mut file = cfb::CompoundFile::open(Cursor::new(bytes))
        .map_err(|e| ReadError::Zip(format!("составной файл: {e}")))?;
    let info = read_stream(&mut file, "/EncryptionInfo")?;
    let package = read_stream(&mut file, "/EncryptedPackage")?;
    let (major, minor) = match info.get(..4) {
        Some(v) => (u16_at(v, 0), u16_at(v, 2)),
        None => return Err(unsupported("короткий поток EncryptionInfo")),
    };
    match (major, minor) {
        (4, 4) => decrypt_agile(&info, &package, password),
        (2..=4, 2) => decrypt_standard(&info, &package, password),
        _ => Err(unsupported(&format!("версия {major}.{minor}"))),
    }
}

/// Шифрует архив .docx паролем (агильный способ, AES-256 и SHA-512).
pub(crate) fn encrypt(zip: &[u8], password: &str) -> Result<Vec<u8>, WriteError> {
    let hash = Hash::Sha512;
    let random = |n: usize| -> Result<Vec<u8>, WriteError> {
        let mut bytes = vec![0; n];
        getrandom::fill(&mut bytes)
            .map_err(|e| WriteError::Io(std::io::Error::other(e.to_string())))?;
        Ok(bytes)
    };
    let key_salt = random(16)?;
    let password_salt = random(16)?;
    let secret = random(32)?;
    let verifier = random(16)?;
    let hmac_key = random(hash.size())?;

    let password_hash = password_hash(hash, &password_salt, password, SPIN_COUNT);
    let derive = |block: &[u8]| derive_key(hash, &password_hash, block, 32);
    let encrypted_verifier_input =
        cbc_encrypt(&derive(&BLOCK_VERIFIER_INPUT), &password_salt, &verifier);
    let encrypted_verifier_hash = cbc_encrypt(
        &derive(&BLOCK_VERIFIER_HASH),
        &password_salt,
        &hash.digest(&[&verifier]),
    );
    let encrypted_key = cbc_encrypt(&derive(&BLOCK_KEY), &password_salt, &secret);

    let mut package = (zip.len() as u64).to_le_bytes().to_vec();
    for (index, segment) in zip.chunks(SEGMENT).enumerate() {
        let iv = block_iv(hash, &key_salt, &(index as u32).to_le_bytes(), 16);
        package.extend(cbc_encrypt(&secret, &iv, segment));
    }
    let hmac_value = hmac(hash, &hmac_key, &package);
    let encrypted_hmac_key = cbc_encrypt(
        &secret,
        &block_iv(hash, &key_salt, &BLOCK_HMAC_KEY, 16),
        &hmac_key,
    );
    let encrypted_hmac_value = cbc_encrypt(
        &secret,
        &block_iv(hash, &key_salt, &BLOCK_HMAC_VALUE, 16),
        &hmac_value,
    );

    let algorithm = |salt: &[u8]| {
        format!(
            "saltSize=\"16\" blockSize=\"16\" keyBits=\"256\" hashSize=\"{}\" \
             cipherAlgorithm=\"AES\" cipherChaining=\"ChainingModeCBC\" \
             hashAlgorithm=\"SHA512\" saltValue=\"{}\"",
            hash.size(),
            base64_encode(salt)
        )
    };
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n\
         <encryption xmlns=\"http://schemas.microsoft.com/office/2006/encryption\" \
         xmlns:p=\"http://schemas.microsoft.com/office/2006/keyEncryptor/password\" \
         xmlns:c=\"http://schemas.microsoft.com/office/2006/keyEncryptor/certificate\">\
         <keyData {}/>\
         <dataIntegrity encryptedHmacKey=\"{}\" encryptedHmacValue=\"{}\"/>\
         <keyEncryptors><keyEncryptor uri=\"http://schemas.microsoft.com/office/2006/keyEncryptor/password\">\
         <p:encryptedKey spinCount=\"{SPIN_COUNT}\" {} encryptedVerifierHashInput=\"{}\" \
         encryptedVerifierHashValue=\"{}\" encryptedKeyValue=\"{}\"/>\
         </keyEncryptor></keyEncryptors></encryption>",
        algorithm(&key_salt),
        base64_encode(&encrypted_hmac_key),
        base64_encode(&encrypted_hmac_value),
        algorithm(&password_salt),
        base64_encode(&encrypted_verifier_input),
        base64_encode(&encrypted_verifier_hash),
        base64_encode(&encrypted_key),
    );
    // Версия 4.4, флаги: «агильное шифрование».
    let mut info = vec![4, 0, 4, 0, 0x40, 0, 0, 0];
    info.extend_from_slice(xml.as_bytes());

    let mut file = cfb::CompoundFile::create(Cursor::new(Vec::new()))?;
    write_stream(&mut file, "/EncryptionInfo", &info)?;
    write_stream(&mut file, "/EncryptedPackage", &package)?;
    write_data_spaces(&mut file)?;
    file.flush()?;
    Ok(file.into_inner().into_inner())
}

/// Служебная папка `\x06DataSpaces`: описывает, что поток
/// `EncryptedPackage` надо расшифровать (MS-OFFCRYPTO 2.1.5–2.1.8).
fn write_data_spaces<F: Read + Write + Seek>(
    file: &mut cfb::CompoundFile<F>,
) -> Result<(), WriteError> {
    let versions = |out: &mut Vec<u8>| {
        // Версии читателя, обновителя и писателя — все 1.0.
        for _ in 0..3 {
            out.extend([1, 0, 0, 0]);
        }
    };
    let mut version = Vec::new();
    unicode_lp_p4(&mut version, "Microsoft.Container.DataSpaces");
    versions(&mut version);

    let mut entry = Vec::new();
    entry.extend(1u32.to_le_bytes()); // одна ссылка
    entry.extend(0u32.to_le_bytes()); // на поток
    unicode_lp_p4(&mut entry, "EncryptedPackage");
    unicode_lp_p4(&mut entry, "StrongEncryptionDataSpace");
    let mut map = Vec::new();
    map.extend(8u32.to_le_bytes());
    map.extend(1u32.to_le_bytes());
    map.extend((entry.len() as u32 + 4).to_le_bytes());
    map.extend(entry);

    let mut definition = Vec::new();
    definition.extend(8u32.to_le_bytes());
    definition.extend(1u32.to_le_bytes());
    unicode_lp_p4(&mut definition, "StrongEncryptionTransform");

    let mut id = Vec::new();
    unicode_lp_p4(&mut id, "{FF9A3F03-56EF-4613-BDD5-5A41C1D07246}");
    let mut primary = Vec::new();
    primary.extend((id.len() as u32 + 8).to_le_bytes());
    primary.extend(1u32.to_le_bytes());
    primary.extend(id);
    unicode_lp_p4(&mut primary, "Microsoft.Container.EncryptionTransform");
    versions(&mut primary);
    // Имя шифрования (пустое), размер блока, режим, резерв.
    for value in [0u32, 0, 0, 4] {
        primary.extend(value.to_le_bytes());
    }

    let root = "/\u{6}DataSpaces";
    file.create_storage(root)?;
    file.create_storage(format!("{root}/DataSpaceInfo"))?;
    file.create_storage(format!("{root}/TransformInfo"))?;
    file.create_storage(format!("{root}/TransformInfo/StrongEncryptionTransform"))?;
    write_stream(file, &format!("{root}/Version"), &version)?;
    write_stream(file, &format!("{root}/DataSpaceMap"), &map)?;
    write_stream(
        file,
        &format!("{root}/DataSpaceInfo/StrongEncryptionDataSpace"),
        &definition,
    )?;
    write_stream(
        file,
        &format!("{root}/TransformInfo/StrongEncryptionTransform/\u{6}Primary"),
        &primary,
    )?;
    Ok(())
}

/// Строка UTF-16 с длиной в байтах впереди, дополненная до 4 байт.
fn unicode_lp_p4(out: &mut Vec<u8>, text: &str) {
    let bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    out.extend((bytes.len() as u32).to_le_bytes());
    out.extend(&bytes);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

fn write_stream<F: Read + Write + Seek>(
    file: &mut cfb::CompoundFile<F>,
    path: &str,
    data: &[u8],
) -> Result<(), WriteError> {
    let mut stream = file.create_stream(path)?;
    stream.write_all(data)?;
    Ok(())
}

fn read_stream<F: Read + Seek>(
    file: &mut cfb::CompoundFile<F>,
    path: &str,
) -> Result<Vec<u8>, ReadError> {
    let stream = file
        .open_stream(path)
        .map_err(|_| ReadError::MissingPart(path.trim_start_matches('/').to_string()))?;
    let mut bytes = Vec::new();
    stream.take(MAX_STREAM_SIZE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_STREAM_SIZE {
        return Err(ReadError::PartTooLarge(
            path.trim_start_matches('/').to_string(),
        ));
    }
    Ok(bytes)
}

fn unsupported(what: &str) -> ReadError {
    ReadError::UnsupportedEncryption(what.to_string())
}

/// Агильное шифрование: параметры — в XML после 8 байт заголовка.
fn decrypt_agile(info: &[u8], package: &[u8], password: &str) -> Result<Vec<u8>, ReadError> {
    let root = xml::parse(info.get(8..).unwrap_or_default())?;
    let key_data = root
        .elements()
        .find(|e| e.local == "keyData")
        .ok_or_else(|| unsupported("нет keyData"))?;
    let encrypted_key = root
        .elements()
        .filter(|e| e.local == "keyEncryptors")
        .flat_map(|e| e.elements())
        .flat_map(|e| e.elements())
        .find(|e| e.local == "encryptedKey" && attr(e, "encryptedKeyValue").is_some())
        .ok_or_else(|| unsupported("документ зашифрован сертификатом, а не паролем"))?;

    let params = Params::read(encrypted_key)?;
    let spin_count: u32 = number(encrypted_key, "spinCount")?;
    if spin_count > MAX_SPIN_COUNT {
        return Err(unsupported("слишком много повторов хеша"));
    }
    let password_hash = password_hash(params.hash, &params.salt, password, spin_count);
    let derive = |block: &[u8]| derive_key(params.hash, &password_hash, block, params.key_size);
    let decrypt_value = |block: &[u8], name: &str| -> Result<Vec<u8>, ReadError> {
        cbc_decrypt(
            &derive(block),
            &params.iv(),
            &base64_attr(encrypted_key, name)?,
        )
        .ok_or_else(|| unsupported("неверная длина ключа или данных"))
    };
    let verifier = decrypt_value(&BLOCK_VERIFIER_INPUT, "encryptedVerifierHashInput")?;
    let verifier_hash = decrypt_value(&BLOCK_VERIFIER_HASH, "encryptedVerifierHashValue")?;
    let expected = params
        .hash
        .digest(&[verifier.get(..params.salt.len()).unwrap_or(&verifier)]);
    if verifier_hash.get(..expected.len()) != Some(expected.as_slice()) {
        return Err(ReadError::WrongPassword);
    }
    let secret = decrypt_value(&BLOCK_KEY, "encryptedKeyValue")?;
    let secret = secret
        .get(..params.key_size)
        .ok_or_else(|| unsupported("короткий ключ"))?;

    let data = Params::read(key_data)?;
    let size = package_size(package)?;
    let mut out = Vec::with_capacity(size.min(MAX_STREAM_SIZE as usize));
    for (index, segment) in package[8..].chunks(SEGMENT).enumerate() {
        let iv = block_iv(
            data.hash,
            &data.salt,
            &(index as u32).to_le_bytes(),
            data.block_size,
        );
        let plain = cbc_decrypt(secret, &iv, segment)
            .ok_or_else(|| unsupported("повреждён зашифрованный пакет"))?;
        out.extend(plain);
    }
    if out.len() < size {
        return Err(unsupported("зашифрованный пакет короче, чем указано"));
    }
    out.truncate(size);
    Ok(out)
}

/// Стандартное шифрование Word 2007: заголовок и проверочные данные —
/// двоичные структуры, ключ — SHA-1, пакет — AES в режиме ECB.
fn decrypt_standard(info: &[u8], package: &[u8], password: &str) -> Result<Vec<u8>, ReadError> {
    let short = || unsupported("короткий поток EncryptionInfo");
    let header_size = u32_at(info, 8).ok_or_else(short)? as usize;
    let header = info.get(12..12 + header_size).ok_or_else(short)?;
    let algorithm = u32_at(header, 8).ok_or_else(short)?;
    let hash_algorithm = u32_at(header, 12).ok_or_else(short)?;
    let key_bits = u32_at(header, 16).ok_or_else(short)?;
    // AES-128/192/256 и SHA-1 (0 — «по умолчанию», тоже SHA-1).
    if !matches!(algorithm, 0x660E..=0x6610) || !matches!(hash_algorithm, 0 | 0x8004) {
        return Err(unsupported("алгоритм не AES"));
    }
    let key_size = (key_bits / 8) as usize;
    let verifier = info.get(12 + header_size..).ok_or_else(short)?;
    let salt_size = u32_at(verifier, 0).ok_or_else(short)? as usize;
    let salt = verifier.get(4..4 + salt_size).ok_or_else(short)?;
    let rest = &verifier[4 + salt_size..];
    let encrypted_verifier = rest.get(..16).ok_or_else(short)?;
    let encrypted_hash = rest.get(20..52).ok_or_else(short)?;

    let hash = Hash::Sha1;
    let h = password_hash(hash, salt, password, 50_000);
    let h = hash.digest(&[&h, &0u32.to_le_bytes()]);
    let mut x1 = [0x36u8; 64];
    let mut x2 = [0x5cu8; 64];
    for (i, b) in h.iter().enumerate() {
        x1[i] ^= b;
        x2[i] ^= b;
    }
    let mut key = hash.digest(&[&x1]);
    key.extend(hash.digest(&[&x2]));
    key.truncate(key_size);

    let wrong = || unsupported("неверная длина ключа или данных");
    let verifier = ecb_decrypt(&key, encrypted_verifier).ok_or_else(wrong)?;
    let verifier_hash = ecb_decrypt(&key, encrypted_hash).ok_or_else(wrong)?;
    if verifier_hash[..20] != hash.digest(&[&verifier])[..] {
        return Err(ReadError::WrongPassword);
    }
    let size = package_size(package)?;
    let body = &package[8..];
    let body = &body[..body.len() - body.len() % 16];
    let mut out = ecb_decrypt(&key, body).ok_or_else(wrong)?;
    if out.len() < size {
        return Err(unsupported("зашифрованный пакет короче, чем указано"));
    }
    out.truncate(size);
    Ok(out)
}

/// Размер расшифрованного пакета — первые 8 байт потока.
fn package_size(package: &[u8]) -> Result<usize, ReadError> {
    let size = package
        .get(..8)
        .and_then(|b| b.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or_else(|| unsupported("короткий зашифрованный пакет"))?;
    if size > MAX_STREAM_SIZE {
        return Err(ReadError::PartTooLarge("EncryptedPackage".into()));
    }
    Ok(size as usize)
}

/// Параметры шифрования из `keyData` или `p:encryptedKey`.
struct Params {
    hash: Hash,
    salt: Vec<u8>,
    key_size: usize,
    block_size: usize,
}

impl Params {
    fn read(element: &xml::Element) -> Result<Params, ReadError> {
        let cipher = attr(element, "cipherAlgorithm").unwrap_or_default();
        let chaining = attr(element, "cipherChaining").unwrap_or_default();
        if cipher != "AES" || chaining != "ChainingModeCBC" {
            return Err(unsupported(&format!("{cipher} {chaining}")));
        }
        let hash = match attr(element, "hashAlgorithm").unwrap_or_default() {
            "SHA1" => Hash::Sha1,
            "SHA256" => Hash::Sha256,
            "SHA384" => Hash::Sha384,
            "SHA512" => Hash::Sha512,
            other => return Err(unsupported(&format!("хеш {other}"))),
        };
        let key_bits: usize = number(element, "keyBits")?;
        let block_size: usize = number(element, "blockSize")?;
        if !matches!(key_bits, 128 | 192 | 256) || block_size != 16 {
            return Err(unsupported(&format!("ключ {key_bits} бит")));
        }
        Ok(Params {
            hash,
            salt: base64_attr(element, "saltValue")?,
            key_size: key_bits / 8,
            block_size,
        })
    }

    /// Вектор инициализации для ключей из пароля — соль.
    fn iv(&self) -> Vec<u8> {
        fit(self.salt.clone(), self.block_size)
    }
}

fn attr<'a>(element: &'a xml::Element, name: &str) -> Option<&'a str> {
    element
        .attrs
        .iter()
        .find(|a| a.local == name)
        .map(|a| a.value.as_str())
}

fn number<T: std::str::FromStr>(element: &xml::Element, name: &str) -> Result<T, ReadError> {
    attr(element, name)
        .and_then(|v| v.trim().parse().ok())
        .ok_or_else(|| unsupported(&format!("нет числа {name}")))
}

fn base64_attr(element: &xml::Element, name: &str) -> Result<Vec<u8>, ReadError> {
    attr(element, name)
        .and_then(base64_decode)
        .ok_or_else(|| unsupported(&format!("испорчено поле {name}")))
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// Хеш-функции, которые встречаются в документах Word.
#[derive(Debug, Clone, Copy)]
enum Hash {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl Hash {
    /// Длина хеша в байтах.
    fn size(self) -> usize {
        match self {
            Hash::Sha1 => 20,
            Hash::Sha256 => 32,
            Hash::Sha384 => 48,
            Hash::Sha512 => 64,
        }
    }

    /// Длина блока функции (для HMAC).
    fn block(self) -> usize {
        match self {
            Hash::Sha1 | Hash::Sha256 => 64,
            Hash::Sha384 | Hash::Sha512 => 128,
        }
    }

    /// Хеш нескольких кусков подряд.
    fn digest(self, parts: &[&[u8]]) -> Vec<u8> {
        fn run<D: Digest>(parts: &[&[u8]]) -> Vec<u8> {
            let mut d = D::new();
            for part in parts {
                d.update(part);
            }
            d.finalize().to_vec()
        }
        match self {
            Hash::Sha1 => run::<sha1::Sha1>(parts),
            Hash::Sha256 => run::<sha2::Sha256>(parts),
            Hash::Sha384 => run::<sha2::Sha384>(parts),
            Hash::Sha512 => run::<sha2::Sha512>(parts),
        }
    }
}

/// Хеш пароля: соль и пароль в UTF-16, затем `spin_count` повторов
/// с номером повтора впереди.
fn password_hash(hash: Hash, salt: &[u8], password: &str, spin_count: u32) -> Vec<u8> {
    let password: Vec<u8> = password.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut h = hash.digest(&[salt, &password]);
    for i in 0..spin_count {
        h = hash.digest(&[&i.to_le_bytes(), &h]);
    }
    h
}

/// Ключ для одной из целей (`block`) из хеша пароля.
fn derive_key(hash: Hash, password_hash: &[u8], block: &[u8], size: usize) -> Vec<u8> {
    fit(hash.digest(&[password_hash, block]), size)
}

/// Вектор инициализации: хеш соли и `block`.
fn block_iv(hash: Hash, salt: &[u8], block: &[u8], size: usize) -> Vec<u8> {
    fit(hash.digest(&[salt, block]), size)
}

/// Обрезает до `size` байт или дополняет байтами 0x36.
fn fit(mut bytes: Vec<u8>, size: usize) -> Vec<u8> {
    bytes.resize(size, 0x36);
    bytes
}

fn hmac(hash: Hash, key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut key = if key.len() > hash.block() {
        hash.digest(&[key])
    } else {
        key.to_vec()
    };
    key.resize(hash.block(), 0);
    let inner: Vec<u8> = key.iter().map(|b| b ^ 0x36).collect();
    let outer: Vec<u8> = key.iter().map(|b| b ^ 0x5c).collect();
    let inner = hash.digest(&[&inner, data]);
    hash.digest(&[&outer, &inner])
}

/// AES с ключом 128, 192 или 256 бит.
enum Aes {
    Aes128(aes::Aes128),
    Aes192(aes::Aes192),
    Aes256(aes::Aes256),
}

impl Aes {
    fn new(key: &[u8]) -> Option<Aes> {
        Some(match key.len() {
            16 => Aes::Aes128(aes::Aes128::new_from_slice(key).ok()?),
            24 => Aes::Aes192(aes::Aes192::new_from_slice(key).ok()?),
            32 => Aes::Aes256(aes::Aes256::new_from_slice(key).ok()?),
            _ => return None,
        })
    }

    fn encrypt(&self, block: &mut aes::Block) {
        match self {
            Aes::Aes128(c) => c.encrypt_block(block),
            Aes::Aes192(c) => c.encrypt_block(block),
            Aes::Aes256(c) => c.encrypt_block(block),
        }
    }

    fn decrypt(&self, block: &mut aes::Block) {
        match self {
            Aes::Aes128(c) => c.decrypt_block(block),
            Aes::Aes192(c) => c.decrypt_block(block),
            Aes::Aes256(c) => c.decrypt_block(block),
        }
    }
}

/// Шифрует AES-CBC; данные дополняются нулями до целого числа блоков.
fn cbc_encrypt(key: &[u8], iv: &[u8], data: &[u8]) -> Vec<u8> {
    let Some(cipher) = Aes::new(key) else {
        return Vec::new();
    };
    let mut previous = aes::Block::default();
    previous.copy_from_slice(&iv[..16]);
    let mut out = Vec::with_capacity(data.len() + 16);
    for chunk in data.chunks(16) {
        let mut block = aes::Block::default();
        block[..chunk.len()].copy_from_slice(chunk);
        for (b, p) in block.iter_mut().zip(previous.iter()) {
            *b ^= p;
        }
        cipher.encrypt(&mut block);
        out.extend_from_slice(&block);
        previous = block;
    }
    out
}

/// Расшифровывает AES-CBC. `None` — неверная длина ключа, вектора или
/// данных (не кратна блоку).
fn cbc_decrypt(key: &[u8], iv: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    let cipher = Aes::new(key)?;
    if !data.len().is_multiple_of(16) || iv.len() < 16 {
        return None;
    }
    let mut previous = aes::Block::default();
    previous.copy_from_slice(&iv[..16]);
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let mut block = aes::Block::default();
        block.copy_from_slice(chunk);
        let encrypted = block;
        cipher.decrypt(&mut block);
        for (b, p) in block.iter_mut().zip(previous.iter()) {
            *b ^= p;
        }
        out.extend_from_slice(&block);
        previous = encrypted;
    }
    Some(out)
}

/// Расшифровывает AES-ECB (каждый блок отдельно).
fn ecb_decrypt(key: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    let cipher = Aes::new(key)?;
    if !data.len().is_multiple_of(16) {
        return None;
    }
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let mut block = aes::Block::default();
        block.copy_from_slice(chunk);
        cipher.decrypt(&mut block);
        out.extend_from_slice(&block);
    }
    Some(out)
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | (u32::from(b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(BASE64[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Раскодирует base64; пробелы и переводы строк пропускает. `None` —
/// посторонние знаки.
pub(crate) fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut n = 0u32;
    let mut bits = 0;
    for c in text.bytes() {
        let value = match c {
            b'=' => break,
            c if c.is_ascii_whitespace() => continue,
            c => BASE64.iter().position(|&b| b == c)? as u32,
        };
        n = (n << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((n >> bits) as u8);
            n &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trip() {
        for len in 0..10 {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 200) as u8).collect();
            let text = base64_encode(&bytes);
            assert_eq!(text.len() % 4, 0);
            assert_eq!(base64_decode(&text), Some(bytes));
        }
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_decode("TW\r\nE="), Some(b"Ma".to_vec()));
        assert_eq!(base64_decode("T*"), None);
    }

    #[test]
    fn hmac_matches_rfc_4231() {
        // RFC 4231, пример 2.
        let mac = hmac(Hash::Sha512, b"Jefe", b"what do ya want for nothing?");
        let hex: String = mac.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea250554\
             9758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737"
        );
    }

    #[test]
    fn cbc_round_trip() {
        let key = [7u8; 32];
        let iv = [9u8; 16];
        let data: Vec<u8> = (0..48u8).collect();
        let encrypted = cbc_encrypt(&key, &iv, &data);
        assert_ne!(encrypted, data);
        assert_eq!(cbc_decrypt(&key, &iv, &encrypted), Some(data));
    }

    #[test]
    fn agile_round_trip_and_wrong_password() {
        let zip = b"PK\x03\x04 any bytes".repeat(1000);
        let file = encrypt(&zip, "секрет").unwrap();
        assert!(is_ole(&file) && is_encrypted_package(&file));
        assert_eq!(decrypt(&file, "секрет").unwrap(), zip);
        assert!(matches!(
            decrypt(&file, "Секрет"),
            Err(ReadError::WrongPassword)
        ));
    }

    /// Стандартное шифрование Word 2007 — собранное здесь же по
    /// MS-OFFCRYPTO 2.3.4.5–2.3.4.9.
    #[test]
    fn standard_encryption_is_read() {
        let password = "pass";
        let salt = [3u8; 16];
        let zip = b"PK\x03\x04 standard".repeat(50);
        let hash = Hash::Sha1;
        let h = password_hash(hash, &salt, password, 50_000);
        let h = hash.digest(&[&h, &0u32.to_le_bytes()]);
        let mut x1 = [0x36u8; 64];
        let mut x2 = [0x5cu8; 64];
        for (i, b) in h.iter().enumerate() {
            x1[i] ^= b;
            x2[i] ^= b;
        }
        let mut key = hash.digest(&[&x1]);
        key.extend(hash.digest(&[&x2]));
        key.truncate(16);
        let ecb = |data: &[u8]| {
            let cipher = Aes::new(&key).unwrap();
            let mut out = Vec::new();
            for chunk in data.chunks(16) {
                let mut block = aes::Block::default();
                block[..chunk.len()].copy_from_slice(chunk);
                cipher.encrypt(&mut block);
                out.extend_from_slice(&block);
            }
            out
        };
        let verifier = [5u8; 16];
        let mut info = vec![3, 0, 2, 0, 0x24, 0, 0, 0];
        let mut header = Vec::new();
        for v in [0x24u32, 0, 0x660E, 0x8004, 128, 0x18, 0, 0] {
            header.extend(v.to_le_bytes());
        }
        header.extend([0, 0]);
        info.extend((header.len() as u32).to_le_bytes());
        info.extend(&header);
        info.extend(16u32.to_le_bytes());
        info.extend(salt);
        info.extend(ecb(&verifier));
        info.extend(20u32.to_le_bytes());
        info.extend(ecb(&hash.digest(&[&verifier])));
        let mut package = (zip.len() as u64).to_le_bytes().to_vec();
        package.extend(ecb(&zip));

        let mut file = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        write_stream(&mut file, "/EncryptionInfo", &info).unwrap();
        write_stream(&mut file, "/EncryptedPackage", &package).unwrap();
        file.flush().unwrap();
        let bytes = file.into_inner().into_inner();
        assert_eq!(decrypt(&bytes, password).unwrap(), zip);
        assert!(matches!(
            decrypt(&bytes, "pas"),
            Err(ReadError::WrongPassword)
        ));
    }
}
