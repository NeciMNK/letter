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

use letter_model::{
    Block, Document, HeaderFooterKind, ImageData, Inline, NoteKind, Picture, SourcePackage,
    SourcePart, for_each_picture_in,
};

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

    let images = package.relationships(&main);
    let stories = read_headers_footers(&mut package, &mut doc, &images);
    // Сноски: часть, которую не удалось понять, сохранится как есть.
    let mut note_parts = Vec::new();
    for (kind, name) in [
        (NoteKind::Footnote, "footnotes"),
        (NoteKind::Endnote, "endnotes"),
    ] {
        if let Some((part, root)) = read_related_xml(&mut package, &main, name)
            && let Some(notes) = document::read_notes(&root, kind)
        {
            match kind {
                NoteKind::Footnote => doc.footnotes = notes,
                NoteKind::Endnote => doc.endnotes = notes,
            }
            note_parts.push((kind, part, root.root_attributes()));
        }
    }
    let mut rebuilt = vec![
        Some(main.clone()),
        styles_part.clone(),
        numbering_part.clone(),
        core_part.clone(),
        app_part.clone(),
    ];
    rebuilt.extend(stories.iter().map(|(name, _)| Some(name.clone())));
    rebuilt.extend(note_parts.iter().map(|(_, name, _)| Some(name.clone())));
    let mut source = read_source(&mut package, &rebuilt)?;
    attach_images(|f| doc.for_each_picture_mut(f), &images, &source);
    let addresses = package.external_links(&main);
    for section in &mut doc.sections {
        attach_links(&mut section.blocks, &addresses);
    }
    // Картинки колонтитулов — по связям своих частей.
    for (name, _) in &stories {
        let relationships = package.relationships(name);
        for section in &mut doc.sections {
            let props = &mut section.props;
            for set in [&mut props.headers, &mut props.footers] {
                for kind in HeaderFooterKind::ALL {
                    let Some(story) = set.get_mut(kind) else {
                        continue;
                    };
                    let target = story
                        .relationship
                        .as_deref()
                        .and_then(|id| images.iter().find(|r| r.id == id))
                        .map(|r| r.target.as_str());
                    if target.is_some_and(|t| t.eq_ignore_ascii_case(name)) {
                        attach_links(&mut story.blocks, &package.external_links(name));
                        attach_images(
                            |f| for_each_picture_in(&mut story.blocks, f),
                            &relationships,
                            &source,
                        );
                    }
                }
            }
        }
    }
    // Картинки и ссылки сносок — по связям своих частей.
    for (kind, name, attributes) in note_parts {
        let relationships = package.relationships(&name);
        let addresses = package.external_links(&name);
        let notes = match kind {
            NoteKind::Footnote => &mut doc.footnotes,
            NoteKind::Endnote => &mut doc.endnotes,
        };
        for note in notes.iter_mut() {
            attach_links(&mut note.blocks, &addresses);
            attach_images(
                |f| for_each_picture_in(&mut note.blocks, f),
                &relationships,
                &source,
            );
        }
        let entry = Some((name, attributes));
        match kind {
            NoteKind::Footnote => source.footnotes_part = entry,
            NoteKind::Endnote => source.endnotes_part = entry,
        }
    }
    source.header_footer_parts = stories;
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

/// Читает содержимое колонтитулов, на которые ссылаются разделы, из их
/// частей. Возвращает прочитанные части и атрибуты их корневых
/// элементов. Колонтитул, часть которого не читается, остаётся пустым
/// со своей связью: часть тогда сохраняется как есть.
fn read_headers_footers<R: Read + Seek>(
    package: &mut package::Package<R>,
    doc: &mut Document,
    relationships: &[package::Relationship],
) -> Vec<(String, Vec<(String, String)>)> {
    // Прочитанные части: имя, атрибуты корня, содержимое.
    type Part = (String, Vec<(String, String)>, Vec<Block>);
    let mut parts: Vec<Part> = Vec::new();
    for section in &mut doc.sections {
        let props = &mut section.props;
        for (set, suffix) in [
            (&mut props.headers, "/header"),
            (&mut props.footers, "/footer"),
        ] {
            for kind in HeaderFooterKind::ALL {
                let Some(story) = set.get_mut(kind) else {
                    continue;
                };
                let Some(target) = story
                    .relationship
                    .as_deref()
                    .and_then(|id| relationships.iter().find(|r| r.id == id))
                    .filter(|r| r.kind.ends_with(suffix))
                    .map(|r| r.target.clone())
                else {
                    continue;
                };
                if let Some((_, _, blocks)) = parts
                    .iter()
                    .find(|(name, _, _)| name.eq_ignore_ascii_case(&target))
                {
                    story.blocks = blocks.clone();
                    continue;
                }
                let Some(root) = package
                    .read_part(&target)
                    .ok()
                    .flatten()
                    .and_then(|bytes| xml::parse(&bytes).ok())
                else {
                    continue;
                };
                let Some(blocks) = document::read_header_footer(&root) else {
                    continue;
                };
                story.blocks = blocks.clone();
                parts.push((target, root.root_attributes(), blocks));
            }
        }
    }
    parts
        .into_iter()
        .map(|(name, attributes, _)| (name, attributes))
        .collect()
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
/// Адреса гиперссылок по их связям `r:id` (`addresses` — имя связи
/// и адрес).
fn attach_links(blocks: &mut [Block], addresses: &[(String, String)]) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                for inline in &mut p.content {
                    if let Inline::Run(run) = inline
                        && let Some(link) = &mut run.props.link
                        && let Some(id) = &link.relationship
                        && let Some((_, address)) = addresses.iter().find(|(i, _)| i == id)
                    {
                        link.target = Some(address.clone());
                    }
                }
            }
            Block::Table(t) => {
                for cell in t.rows.iter_mut().flat_map(|r| &mut r.cells) {
                    attach_links(&mut cell.blocks, addresses);
                }
            }
            Block::Unknown(_) => {}
        }
    }
}

fn attach_images(
    visit: impl FnOnce(&mut dyn FnMut(&mut Picture)),
    relationships: &[package::Relationship],
    source: &SourcePackage,
) {
    let mut loaded: Vec<(&str, Option<Arc<ImageData>>)> = Vec::new();
    visit(&mut |picture: &mut Picture| {
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
