//! Чтение старого двоичного формата .doc (Word 97–2003, MS-DOC).
//!
//! Файл .doc — составной файл OLE с потоками `WordDocument` (заголовок
//! и текст), `0Table`/`1Table` (таблицы: куски текста, оформление,
//! стили) и `Data`. Пока читаются ([`read`], [`read_file`]):
//!
//! - текст основной части документа с абзацами, табуляцией, разрывами
//!   строк, страниц и разделов, неразрывными и мягкими дефисами;
//! - поля (код и показанный результат);
//! - оформление знаков и абзацев (с границами и заливкой), стили, шрифты;
//! - таблицы (и вложенные), разделы, колонтитулы.
//!
//! Пока не читаются (будут в следующих частях задачи F8.1):
//! параметры разделов, колонтитулы, сноски, списки, рисунки. Файлы
//! Word 6/95 и зашифрованные не открываются.

mod border;
mod document;
mod fib;
mod fkp;
mod section;
mod sprm;
mod styles;
mod table;
mod text;

use std::io::{Cursor, Read};
use std::path::Path;

use letter_model::Document;

/// Почему не удалось прочитать файл .doc.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ReadError {
    /// Ошибка чтения с диска.
    #[error("не удалось прочитать файл: {0}")]
    Io(#[from] std::io::Error),
    /// Это не документ Word 97–2003.
    #[error("это не документ Word 97–2003")]
    NotDoc,
    /// Документ Word 6/95 или старше.
    #[error(
        "документ сохранён старой версией Word (6.0/95 или раньше), её формат пока не читается"
    )]
    TooOld,
    /// Документ зашифрован паролем.
    #[error("документ защищён паролем на открытие, такие пока не открываются")]
    Encrypted,
    /// Файл испорчен.
    #[error("файл повреждён: {0}")]
    Corrupt(&'static str),
}

/// Поток составного файла целиком; `None`, если его нет.
fn stream(file: &mut cfb::CompoundFile<Cursor<&[u8]>>, name: &str) -> Option<Vec<u8>> {
    let mut stream = file.open_stream(name).ok()?;
    let mut data = Vec::new();
    stream.read_to_end(&mut data).ok()?;
    Some(data)
}

/// Читает документ .doc из байтов.
pub fn read(data: &[u8]) -> Result<Document, ReadError> {
    let mut file = cfb::CompoundFile::open(Cursor::new(data)).map_err(|_| ReadError::NotDoc)?;
    let word = stream(&mut file, "WordDocument").ok_or(ReadError::NotDoc)?;
    let fib = fib::Fib::read(&word)?;
    let table_name = if fib.which_table { "1Table" } else { "0Table" };
    let table = stream(&mut file, table_name).ok_or(ReadError::Corrupt("нет потока таблиц"))?;
    document::build(&fib, &word, &table)
}

/// Читает документ .doc из файла.
pub fn read_file(path: impl AsRef<Path>) -> Result<Document, ReadError> {
    read(&std::fs::read(path)?)
}
