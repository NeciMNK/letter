//! Шифрованные документы (пароль на открытие): чтение файла, который
//! зашифровала другая программа, и «открыть → сохранить с паролем →
//! открыть».

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use letter_docx::{
    FileKind, ReadError, encrypt_bytes, is_encrypted, read_bytes, read_bytes_with_password,
    write_bytes_as,
};

fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/docx")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn file_encrypted_elsewhere_opens_with_its_password() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/encrypted/ru-en.docx");
    let encrypted = std::fs::read(path).unwrap();
    assert!(is_encrypted(&encrypted));
    assert!(matches!(
        read_bytes(&encrypted),
        Err(ReadError::PasswordRequired)
    ));
    assert!(matches!(
        read_bytes_with_password(&encrypted, "не тот"),
        Err(ReadError::WrongPassword)
    ));
    let doc = read_bytes_with_password(&encrypted, "пароль").unwrap();
    let plain = read_bytes(&fixture_bytes("ru-en.docx")).unwrap();
    assert_eq!(doc.sections, plain.sections);
    assert_eq!(doc.plain_text(), plain.plain_text());
}

#[test]
fn saved_with_password_and_opened_again() {
    let doc = read_bytes(&fixture_bytes("headings.docx")).unwrap();
    let docx = write_bytes_as(&doc, FileKind::Document).unwrap();
    let encrypted = encrypt_bytes(&docx, "Ёлка 2024!").unwrap();
    assert!(is_encrypted(&encrypted));
    assert!(!is_encrypted(&docx));
    let reopened = read_bytes_with_password(&encrypted, "Ёлка 2024!").unwrap();
    assert_eq!(reopened.sections, doc.sections);
    assert_eq!(reopened.styles, doc.styles);
    // Незашифрованный файл открывается и с паролем.
    assert_eq!(
        read_bytes_with_password(&docx, "любой").unwrap().sections,
        doc.sections
    );
}

#[test]
fn old_doc_container_is_not_mistaken_for_encryption() {
    let mut bytes = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    bytes.extend([0; 600]);
    assert!(!is_encrypted(&bytes));
    assert!(matches!(
        read_bytes(&bytes),
        Err(ReadError::NotAWordDocument(_))
    ));
}
