//! Чтение и запись файлов .docx (Office Open XML).
//!
//! Сейчас умеет читать главную часть документа (`word/document.xml`):
//! абзацы, фрагменты текста, свойства символов и абзацев, таблицы,
//! разделы; стили (`word/styles.xml`), списки (`word/numbering.xml`)
//! и шрифты темы. Всё непонятое
//! сохраняется в модели как [`RawXml`](letter_model::RawXml), а остальные
//! части файла — в [`SourcePackage`], и при
//! записи возвращается на место.
//!
//! ```no_run
//! let doc = letter_docx::read_file("пример.docx")?;
//! println!("{}", doc.plain_text());
//! letter_docx::write_file(&doc, "копия.docx")?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod document;
mod error;
mod names;
mod numbering;
mod package;
mod picture;
mod properties;
mod serialize;
mod settings;
mod styles;
mod table;
mod theme;
mod write;
mod xml;

use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, Write};
use std::path::Path;

use std::sync::Arc;

use letter_model::{Document, ImageData, SourcePackage, SourcePart};

pub use error::{ReadError, WriteError};

use package::CONTENT_TYPES_PART;

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
    let root = xml::parse(&bytes)?;
    let mut doc = document::read_document(&root)?;

    // Стили и тема необязательны: если их нет или они испорчены,
    // документ всё равно открывается со значениями по умолчанию.
    let mut styles_part = None;
    let mut styles_root_attributes = Vec::new();
    if let Some((name, styles_root)) = read_related_xml(&mut package, &main, "styles") {
        doc.styles = styles::read_styles(&styles_root);
        styles_part = Some(name);
        styles_root_attributes = styles_root.root_attributes();
    }
    let mut numbering_part = None;
    let mut numbering_root_attributes = Vec::new();
    if let Some((name, root)) = read_related_xml(&mut package, &main, "numbering") {
        doc.numbering = numbering::read_numbering(&root);
        numbering_part = Some(name);
        numbering_root_attributes = root.root_attributes();
    }
    if let Some((_, root)) = read_related_xml(&mut package, &main, "theme") {
        doc.theme = theme::read_theme(&root);
    }
    if let Some((_, root)) = read_related_xml(&mut package, &main, "settings") {
        doc.settings = settings::read_settings(&root);
    }
    let mut core_part = None;
    if let Some((name, root)) = read_related_xml(&mut package, "", "core-properties")
        && let Some(core) = properties::read_core(&root)
    {
        doc.properties.core = core;
        core_part = Some(name);
    }
    let mut app_part = None;
    let app = read_related_xml(&mut package, "", "extended-properties")
        .or_else(|| read_related_xml(&mut package, "", "extendedProperties"));
    if let Some((name, root)) = app
        && let Some(app) = properties::read_app(&root)
    {
        doc.properties.app = app;
        app_part = Some(name);
    }

    let rebuilt = [
        Some(main.clone()),
        styles_part.clone(),
        numbering_part.clone(),
        core_part.clone(),
        app_part.clone(),
    ];
    let images = package.relationships(&main);
    let mut source = read_source(&mut package, &rebuilt)?;
    attach_images(&mut doc, &images, &source);
    source.main_part = main;
    source.styles_part = styles_part;
    source.numbering_part = numbering_part;
    source.numbering_root_attributes = numbering_root_attributes;
    source.core_part = core_part;
    source.app_part = app_part;
    source.main_root_attributes = root.root_attributes();
    source.styles_root_attributes = styles_root_attributes;
    doc.source = Some(source);
    Ok(doc)
}

/// Собирает части пакета, которые запись не строит заново из модели,
/// чтобы вернуть их в файл без изменений.
fn read_source<R: Read + Seek>(
    package: &mut package::Package<R>,
    rebuilt: &[Option<String>],
) -> Result<SourcePackage, ReadError> {
    let mut source = SourcePackage::default();
    let rebuilt = |name: &str| {
        name.eq_ignore_ascii_case(CONTENT_TYPES_PART)
            || rebuilt
                .iter()
                .flatten()
                .any(|r| name.eq_ignore_ascii_case(r))
    };
    let mut parts = Vec::new();
    for name in package.part_names() {
        if rebuilt(&name) {
            continue;
        }
        if let Some(data) = package.read_part(&name)? {
            parts.push(SourcePart { name, data });
        }
    }
    source.parts = parts;
    if let Some(bytes) = package.read_part(CONTENT_TYPES_PART)?
        && let Ok(root) = xml::parse(&bytes)
    {
        let types = package::read_content_types(&root);
        source.default_content_types = types.defaults;
        source.content_types = types.overrides;
    }
    Ok(source)
}

/// Даёт рисункам байты картинок по их связям (`r:embed`). Части
/// картинок остаются и в [`SourcePackage`]: при сохранении они
/// записываются оттуда.
fn attach_images(
    doc: &mut Document,
    relationships: &[package::Relationship],
    source: &SourcePackage,
) {
    let mut loaded: Vec<(&str, Option<Arc<ImageData>>)> = Vec::new();
    doc.for_each_picture_mut(|picture| {
        let Some(id) = picture.relationship.as_deref() else {
            return;
        };
        if let Some((_, image)) = loaded.iter().find(|(known, _)| *known == id) {
            picture.image = image.clone();
            return;
        }
        let image = relationships
            .iter()
            .find(|r| r.id == id && r.kind.ends_with("/image"))
            .and_then(|r| source.part(&r.target))
            .map(|part| {
                Arc::new(ImageData {
                    name: part.name.clone(),
                    bytes: part.data.clone(),
                })
            });
        picture.image = image.clone();
        if let Some(r) = relationships.iter().find(|r| r.id == id) {
            loaded.push((r.id.as_str(), image));
        }
    });
}

/// Читает и разбирает часть, связанную с `part` связью вида `kind`;
/// возвращает имя части и её корневой элемент. Любая ошибка означает
/// «части нет».
fn read_related_xml<R: Read + Seek>(
    package: &mut package::Package<R>,
    part: &str,
    kind: &str,
) -> Option<(String, xml::Element)> {
    let target = package.related_part(part, kind)?;
    let bytes = package.read_part(&target).ok()??;
    let root = xml::parse(&bytes).ok()?;
    Some((target, root))
}

/// Читает документ .docx из байтов в памяти.
pub fn read_bytes(bytes: &[u8]) -> Result<Document, ReadError> {
    read(Cursor::new(bytes))
}

/// Читает документ .docx с диска.
pub fn read_file(path: impl AsRef<Path>) -> Result<Document, ReadError> {
    read(BufReader::new(File::open(path)?))
}

/// Записывает документ .docx в любой приёмник с произвольным доступом.
///
/// Части исходного файла, которые программа не понимает (картинки,
/// колонтитулы, сноски и т. д.), записываются без изменений.
pub fn write<W: Write + Seek>(doc: &Document, writer: W) -> Result<(), WriteError> {
    write::write(doc, writer)
}

/// Записывает документ .docx в байты в памяти.
pub fn write_bytes(doc: &Document) -> Result<Vec<u8>, WriteError> {
    let mut buffer = Cursor::new(Vec::new());
    write::write(doc, &mut buffer)?;
    Ok(buffer.into_inner())
}

/// Записывает документ .docx на диск. Файл сначала целиком собирается
/// в памяти, поэтому ошибка упаковки не испортит старый файл.
pub fn write_file(doc: &Document, path: impl AsRef<Path>) -> Result<(), WriteError> {
    std::fs::write(path, write_bytes(doc)?)?;
    Ok(())
}
