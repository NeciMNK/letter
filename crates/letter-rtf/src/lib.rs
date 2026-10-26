//! Чтение и запись формата RTF (Rich Text Format).
//!
//! - [`read`] / [`read_file`] — RTF в модель документа: текст, шрифты,
//!   размер, начертание, подчёркивание, цвет, выделение маркером,
//!   надстрочные и подстрочные знаки, выравнивание, отступы, интервалы,
//!   табуляция, разрывы, таблицы (строки и ячейки), разделы с размером
//!   листа и полями, стили, сведения о документе;
//! - [`write()`] / [`write_file`] — модель в RTF.
//!
//! Картинки PNG и JPEG читаются и записываются (`\pict`). Пока не
//! поддерживаются: колонтитулы, сноски, списки (их
//! текст номеров сохраняется как обычный текст), поля (берётся
//! показанный результат), скрытый текст (пропускается), ширины столбцов
//! таблиц.

mod codepage;
mod reader;
mod tokens;
mod writer;

use std::path::Path;

use letter_model::Document;

pub use reader::style_id;

/// Почему не удалось прочитать файл RTF.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ReadError {
    /// Ошибка чтения с диска.
    #[error("не удалось прочитать файл: {0}")]
    Io(#[from] std::io::Error),
    /// Файл не начинается с `{\rtf`.
    #[error("это не файл RTF")]
    NotRtf,
}

/// Читает документ RTF из байтов.
pub fn read(data: &[u8]) -> Result<Document, ReadError> {
    reader::read(data)
}

/// Читает файл RTF.
pub fn read_file(path: impl AsRef<Path>) -> Result<Document, ReadError> {
    read(&std::fs::read(path)?)
}

/// Записывает документ в RTF. Текст файла — только ASCII.
pub fn write(doc: &Document) -> String {
    writer::write(doc)
}

/// Записывает документ в файл RTF.
pub fn write_file(doc: &Document, path: impl AsRef<Path>) -> std::io::Result<()> {
    std::fs::write(path, write(doc))
}
