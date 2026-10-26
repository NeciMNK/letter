//! Импорт и экспорт простых форматов: обычный текст (.txt), HTML и
//! Markdown.
//!
//! - [`read_txt`] / [`write_txt`] — абзац на строку, табуляция и разрыв
//!   страницы сохраняются, оформление теряется. При чтении кодировка
//!   определяется сама: UTF-8 и UTF-16 (по метке BOM), иначе
//!   Windows-1251 или Windows-1252.
//! - [`read_html`] / [`write_html`] — абзацы, заголовки `h1`–`h6`,
//!   начертание, шрифт, размер, цвет, выделение, индексы, выравнивание,
//!   отступы, таблицы. Читатель терпит ошибки разметки, как браузер,
//!   но не исполняет скрипты и не загружает картинки по ссылкам; рисунки
//!   документа записываются в сам файл (`data:`-адресом) и читаются
//!   обратно.
//! - [`read_markdown`] / [`write_markdown`] — CommonMark с таблицами
//!   и зачёркиванием (GFM): заголовки, полужирный, курсив, код, цитаты,
//!   таблицы.
//!
//! Новые документы получают набор стилей, как у пустого документа Word
//! («Обычный», «Заголовок 1»–«Заголовок 6», «Название»). Списки пока
//! хранятся текстом: маркер или номер в начале абзаца (настоящие списки —
//! задача F5.1). Ссылки не переносятся: от ссылки остаётся её текст.

mod builder;
mod html;
pub mod images;
mod markdown;
mod styles;
mod txt;

use std::path::Path;

use letter_model::Document;

/// Читает обычный текст (байты файла .txt).
pub fn read_txt(data: &[u8]) -> Document {
    txt::read(data)
}

/// Записывает документ как обычный текст: UTF-8 без метки BOM, абзацы
/// разделены `\r\n` (как пишет Word в Windows), ячейки таблиц — табуляцией.
pub fn write_txt(doc: &Document) -> Vec<u8> {
    txt::write(doc)
}

/// Читает HTML. Кодировка берётся из метки BOM или `<meta charset>`,
/// по умолчанию — UTF-8.
pub fn read_html(data: &[u8]) -> Document {
    html::read(data)
}

/// Записывает документ как страницу HTML (UTF-8).
pub fn write_html(doc: &Document) -> String {
    html::write(doc)
}

/// Читает Markdown (CommonMark и таблицы GFM). Текст — UTF-8.
pub fn read_markdown(data: &[u8]) -> Document {
    markdown::read(&txt::decode(data))
}

/// Записывает документ в Markdown.
pub fn write_markdown(doc: &Document) -> String {
    markdown::write(doc)
}

/// Формат файла, который понимает этот крейт.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// Обычный текст (.txt).
    Text,
    /// HTML (.html, .htm).
    Html,
    /// Markdown (.md, .markdown).
    Markdown,
}

impl Format {
    /// Формат по расширению файла (без учёта регистра).
    pub fn from_path(path: impl AsRef<Path>) -> Option<Format> {
        let extension = path.as_ref().extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "txt" | "text" => Some(Format::Text),
            "html" | "htm" | "xhtml" => Some(Format::Html),
            "md" | "markdown" => Some(Format::Markdown),
            _ => None,
        }
    }
}

/// Читает файл в формате, который определяется по расширению.
pub fn read_file(path: impl AsRef<Path>) -> std::io::Result<Document> {
    let path = path.as_ref();
    let format = Format::from_path(path).ok_or_else(|| unknown_format(path))?;
    let data = std::fs::read(path)?;
    Ok(match format {
        Format::Text => read_txt(&data),
        Format::Html => read_html(&data),
        Format::Markdown => read_markdown(&data),
    })
}

/// Записывает документ в файл в формате, который определяется по
/// расширению.
pub fn write_file(doc: &Document, path: impl AsRef<Path>) -> std::io::Result<()> {
    let path = path.as_ref();
    let format = Format::from_path(path).ok_or_else(|| unknown_format(path))?;
    let data = match format {
        Format::Text => write_txt(doc),
        Format::Html => write_html(doc).into_bytes(),
        Format::Markdown => write_markdown(doc).into_bytes(),
    };
    std::fs::write(path, data)
}

fn unknown_format(path: &Path) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!("неизвестный формат файла: {}", path.display()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_by_extension() {
        assert_eq!(Format::from_path("a.TXT"), Some(Format::Text));
        assert_eq!(Format::from_path("dir/a.htm"), Some(Format::Html));
        assert_eq!(Format::from_path("a.md"), Some(Format::Markdown));
        assert_eq!(Format::from_path("a.docx"), None);
        assert_eq!(Format::from_path("noext"), None);
    }
}
