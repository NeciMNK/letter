//! Чтение и запись формата ODT (OpenDocument Text) — основного формата
//! LibreOffice Writer.
//!
//! - [`read`] / [`read_file`] — ODT (и плоский .fodt) в модель документа:
//!   текст, стили абзацев и символов, шрифты, размер, начертание,
//!   подчёркивание, цвет, выделение, индексы, выравнивание, отступы,
//!   интервалы, табуляция, разрывы, таблицы, разделы с размером листа
//!   и полями, сведения о документе;
//! - [`write()`] / [`write_file`] — модель в ODT.
//!
//! Пока не поддерживаются: картинки, колонтитулы, сноски, примечания,
//! списки (пункты становятся обычными абзацами без номеров), поля
//! (берётся показанный текст), ширины столбцов таблиц.

mod names;
mod props;
mod reader;
mod toggles;
mod units;
mod writer;
mod xml;

use std::path::Path;

use letter_model::Document;

/// Почему не удалось прочитать файл ODT.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ReadError {
    /// Ошибка чтения с диска.
    #[error("не удалось прочитать файл: {0}")]
    Io(#[from] std::io::Error),
    /// Файл не является ZIP-архивом или архив повреждён.
    #[error("файл повреждён (ошибка ZIP: {0})")]
    Zip(String),
    /// Это не текстовый документ OpenDocument (например, таблица Calc).
    /// Внутри — тип документа из файла, если он есть.
    #[error("это не текстовый документ OpenDocument ({0})")]
    NotOdt(String),
    /// В архиве нет обязательной части.
    #[error("в файле нет обязательной части {0}")]
    MissingPart(String),
    /// Часть архива слишком большая — скорее всего, файл испорчен.
    #[error("часть {0} слишком большая")]
    PartTooLarge(String),
    /// Повреждённый XML внутри файла.
    #[error("повреждённый XML: {0}")]
    Xml(String),
}

impl ReadError {
    pub(crate) fn xml(message: impl Into<String>) -> ReadError {
        ReadError::Xml(message.into())
    }
}

impl From<zip::result::ZipError> for ReadError {
    fn from(e: zip::result::ZipError) -> ReadError {
        ReadError::Zip(e.to_string())
    }
}

/// Почему не удалось сохранить файл ODT.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WriteError {
    /// Ошибка записи на диск.
    #[error("не удалось записать файл: {0}")]
    Io(#[from] std::io::Error),
    /// Ошибка при упаковке в ZIP-архив.
    #[error("не удалось упаковать файл (ошибка ZIP: {0})")]
    Zip(String),
}

impl From<zip::result::ZipError> for WriteError {
    fn from(e: zip::result::ZipError) -> WriteError {
        WriteError::Zip(e.to_string())
    }
}

/// Читает документ ODT (или плоский .fodt) из байтов.
pub fn read(data: &[u8]) -> Result<Document, ReadError> {
    reader::read(data)
}

/// Читает файл ODT.
pub fn read_file(path: impl AsRef<Path>) -> Result<Document, ReadError> {
    read(&std::fs::read(path)?)
}

/// Записывает документ в ODT (байты архива).
pub fn write(doc: &Document) -> Result<Vec<u8>, WriteError> {
    writer::write(doc)
}

/// Записывает документ в файл ODT.
pub fn write_file(doc: &Document, path: impl AsRef<Path>) -> Result<(), WriteError> {
    std::fs::write(path, write(doc)?)?;
    Ok(())
}
