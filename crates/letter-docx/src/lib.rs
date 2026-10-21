//! Чтение и запись файлов .docx (Office Open XML).
//!
//! Сейчас умеет читать главную часть документа (`word/document.xml`):
//! абзацы, фрагменты текста, свойства символов и абзацев, таблицы,
//! разделы. Всё непонятое сохраняется в модели как
//! [`RawXml`](letter_model::RawXml).
//!
//! ```no_run
//! let doc = letter_docx::read_file("пример.docx")?;
//! println!("{}", doc.plain_text());
//! # Ok::<(), letter_docx::ReadError>(())
//! ```

mod document;
mod error;
mod names;
mod package;
mod xml;

use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek};
use std::path::Path;

use letter_model::Document;

pub use error::ReadError;

/// Читает документ .docx из любого источника с произвольным доступом
/// (файл, буфер в памяти).
///
/// Битый файл никогда не роняет программу: функция возвращает ошибку.
pub fn read<R: Read + Seek>(reader: R) -> Result<Document, ReadError> {
    let mut package = package::Package::open(reader)?;
    let main = package.main_document_part()?;
    let bytes = package
        .read_part(&main)?
        .ok_or_else(|| ReadError::MissingPart(main.clone()))?;
    document::read_document(&xml::parse(&bytes)?)
}

/// Читает документ .docx из байтов в памяти.
pub fn read_bytes(bytes: &[u8]) -> Result<Document, ReadError> {
    read(Cursor::new(bytes))
}

/// Читает документ .docx с диска.
pub fn read_file(path: impl AsRef<Path>) -> Result<Document, ReadError> {
    read(BufReader::new(File::open(path)?))
}
