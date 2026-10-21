//! Чтение и запись файлов .docx (Office Open XML).
//!
//! Сейчас умеет читать главную часть документа (`word/document.xml`):
//! абзацы, фрагменты текста, свойства символов и абзацев, таблицы,
//! разделы; стили (`word/styles.xml`) и шрифты темы. Всё непонятое
//! сохраняется в модели как [`RawXml`](letter_model::RawXml).
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
mod styles;
mod theme;
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
    let mut doc = document::read_document(&xml::parse(&bytes)?)?;

    // Стили и тема необязательны: если их нет или они испорчены,
    // документ всё равно открывается со значениями по умолчанию.
    if let Some(root) = read_related_xml(&mut package, &main, "styles") {
        doc.styles = styles::read_styles(&root);
    }
    if let Some(root) = read_related_xml(&mut package, &main, "theme") {
        doc.theme = theme::read_theme(&root);
    }
    Ok(doc)
}

/// Читает и разбирает часть, связанную с `part` связью вида `kind`.
/// Любая ошибка означает «части нет».
fn read_related_xml<R: Read + Seek>(
    package: &mut package::Package<R>,
    part: &str,
    kind: &str,
) -> Option<xml::Element> {
    let target = package.related_part(part, kind)?;
    let bytes = package.read_part(&target).ok()??;
    xml::parse(&bytes).ok()
}

/// Читает документ .docx из байтов в памяти.
pub fn read_bytes(bytes: &[u8]) -> Result<Document, ReadError> {
    read(Cursor::new(bytes))
}

/// Читает документ .docx с диска.
pub fn read_file(path: impl AsRef<Path>) -> Result<Document, ReadError> {
    read(BufReader::new(File::open(path)?))
}
