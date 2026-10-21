//! Ошибки чтения и записи .docx.

/// Почему не удалось открыть файл .docx.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ReadError {
    /// Ошибка чтения с диска.
    #[error("не удалось прочитать файл: {0}")]
    Io(#[from] std::io::Error),
    /// Файл не является ZIP-архивом или архив повреждён.
    #[error("файл повреждён или это не документ Word (ошибка ZIP: {0})")]
    Zip(String),
    /// В архиве нет обязательной части.
    #[error("в файле нет обязательной части {0}")]
    MissingPart(String),
    /// Часть архива слишком большая — скорее всего, файл испорчен
    /// или специально сделан, чтобы занять всю память.
    #[error("часть {0} слишком большая")]
    PartTooLarge(String),
    /// Это пакет Office, но не документ Word (например, таблица Excel).
    #[error("это не документ Word (тип содержимого {0})")]
    NotAWordDocument(String),
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

/// Почему не удалось сохранить файл .docx.
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
