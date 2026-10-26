//! Сборка пакета .docx при сохранении.
//!
//! Главная часть, стили и списки строятся заново из модели. Все остальные части
//! исходного файла ([`SourcePackage`]) записываются байт в байт, вместе
//! со своими связями, поэтому картинки, колонтитулы, сноски и прочее,
//! что программа пока не понимает, не теряются.

use std::borrow::Cow;
use std::io::{Seek, Write};

use std::sync::Arc;

use letter_model::{
    AppProperties, Block, Comment, CoreProperties, Document, HeaderFooterKind, ImageData, Inline,
    NoteKind, Picture, RunContent, SourcePackage, Styles,
};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::WriteError;
use crate::kind::{FileKind, MACRO_CONTENT_TYPES, MACRO_RELATIONSHIPS};
use crate::package::{CONTENT_TYPES_PART, attr, normalize_part_name, rels_part_for};
use crate::properties;
use crate::serialize::{self, escape_attr};
use crate::xml;

const CT_MAIN: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const CT_SETTINGS: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml";
const REL_SETTINGS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings";
const CT_STYLES: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
const CT_NUMBERING: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
const REL_NUMBERING: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering";
const CT_RELS: &str = "application/vnd.openxmlformats-package.relationships+xml";
const CT_XML: &str = "application/xml";
const REL_OFFICE_DOCUMENT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const REL_STYLES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
const REL_CORE: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const REL_APP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
const REL_HEADER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header";
const REL_FOOTER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer";
const REL_MAIL_MERGE_SOURCE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/mailMergeSource";
const REL_RECIPIENT_DATA: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/recipientData";
const CT_RECIPIENT_DATA: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.mailMergeRecipientData+xml";
const REL_HYPERLINK: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
const REL_FOOTNOTES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes";
const REL_ENDNOTES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes";
const CT_FOOTNOTES: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml";
const CT_ENDNOTES: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml";
const REL_COMMENTS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments";
const REL_COMMENTS_EXTENDED: &str =
    "http://schemas.microsoft.com/office/2011/relationships/commentsExtended";
const CT_COMMENTS: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml";
const CT_COMMENTS_EXTENDED: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.commentsExtended+xml";
const CT_HEADER: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
const CT_FOOTER: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml";
const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const CT_CORE: &str = "application/vnd.openxmlformats-package.core-properties+xml";
const CT_APP: &str = "application/vnd.openxmlformats-officedocument.extended-properties+xml";
const NS_RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const NS_CONTENT_TYPES: &str = "http://schemas.openxmlformats.org/package/2006/content-types";

/// Записывает документ в формате .docx. Если задана разновидность
/// файла `kind`, главная часть получает её тип содержимого, а в файл без
/// макросов не попадают части макросов; иначе разновидность остаётся
/// той, из которой документ был прочитан.
pub(crate) fn write<W: Write + Seek>(
    doc: &Document,
    kind: Option<FileKind>,
    writer: W,
) -> Result<(), WriteError> {
    let empty = SourcePackage::default();
    let original = doc.source.as_ref().unwrap_or(&empty);
    let main = if original.main_part.is_empty() {
        "word/document.xml".to_string()
    } else {
        original.main_part.clone()
    };
    let retargeted = kind.map(|kind| retarget(original, &main, kind));
    let source = retargeted.as_ref().unwrap_or(original);

    // Части в порядке записи: имя и содержимое.
    let mut parts: Vec<(String, Cow<'_, [u8]>)> = Vec::new();
    let mut overrides = source.content_types.clone();
    let mut defaults = source.default_content_types.clone();
    for (ext, content_type) in [("rels", CT_RELS), ("xml", CT_XML)] {
        if !defaults.iter().any(|(e, _)| e.eq_ignore_ascii_case(ext)) {
            defaults.push((ext.to_string(), content_type.to_string()));
        }
    }

    let mut links = Links::new(source);
    links.find_or_add("", REL_OFFICE_DOCUMENT, &["/officeDocument"], &main, true);
    let mut replaced = vec![main.clone()];

    // Картинки, у которых нет своей связи (новые рисунки, рисунки,
    // вставленные из другого документа), записываются новыми частями.
    let mut media = Vec::new();
    let doc = link_pictures(doc, &main, &mut links, &mut media);
    let (doc, mut stories) = link_headers_footers(doc, &main, &mut links);
    let doc = link_hyperlinks_in(doc, &main, &mut links);
    let doc = doc.as_ref();
    for (name, _, blocks) in &mut stories {
        link_hyperlinks(blocks, name, &mut links);
    }
    for (name, header, blocks) in stories {
        let root = source
            .header_footer_parts
            .iter()
            .find(|(part, _)| part.eq_ignore_ascii_case(&name))
            .map(|(_, attributes)| attributes.as_slice());
        let xml = serialize::header_footer_xml(&blocks, header, root);
        let content_type = if header { CT_HEADER } else { CT_FOOTER };
        ensure_override(&mut overrides, &name, content_type);
        replaced.push(name.clone());
        parts.push((name, Cow::Owned(xml.into_bytes())));
    }
    // Сноски: часть строится заново из модели (новая — если сносок
    // раньше не было).
    let base = main.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut referenced = Vec::new();
    for section in &doc.sections {
        note_references(&section.blocks, &mut referenced);
    }
    for (kind, notes, source_part) in [
        (NoteKind::Footnote, &doc.footnotes, &source.footnotes_part),
        (NoteKind::Endnote, &doc.endnotes, &source.endnotes_part),
    ] {
        let (rel, suffix, file, content_type) = match kind {
            NoteKind::Footnote => (REL_FOOTNOTES, "/footnotes", "footnotes.xml", CT_FOOTNOTES),
            NoteKind::Endnote => (REL_ENDNOTES, "/endnotes", "endnotes.xml", CT_ENDNOTES),
        };
        let (name, root) = match source_part {
            Some((name, root)) => (name.clone(), Some(root.as_slice())),
            None if !notes.is_empty() => {
                let name = links.find_or_add(&main, rel, &[suffix], &join(base, file), false);
                (name, None)
            }
            None => continue,
        };
        // Сноски, ссылки на которые удалили, в файл не идут.
        let mut notes = notes.clone();
        notes.retain(|n| !n.is_regular() || referenced.contains(&(kind, n.id.as_str())));
        for note in &mut notes {
            link_hyperlinks(&mut note.blocks, &name, &mut links);
        }
        let xml = serialize::notes_xml(&notes, kind, root);
        ensure_override(&mut overrides, &name, content_type);
        replaced.push(name.clone());
        parts.push((name, Cow::Owned(xml.into_bytes())));
    }
    // Примечания: часть строится заново из модели; без ссылок из текста
    // примечание (и ответы на него) в файл не идёт.
    let mut marked = Vec::new();
    for section in &doc.sections {
        comment_marks(&section.blocks, &mut marked);
    }
    let mut comments: Vec<Comment> = Vec::new();
    for comment in &doc.comments {
        let parent_kept = comment
            .parent
            .as_ref()
            .is_some_and(|p| comments.iter().any(|c| c.id == *p));
        if parent_kept || marked.contains(&comment.id.as_str()) {
            comments.push(comment.clone());
        }
    }
    let comments_part = match &source.comments_part {
        Some((name, root)) => Some((name.clone(), Some(root.as_slice()))),
        None if !comments.is_empty() => Some((
            links.find_or_add(
                &main,
                REL_COMMENTS,
                &["/comments"],
                &join(base, "comments.xml"),
                false,
            ),
            None,
        )),
        None => None,
    };
    if let Some((name, root)) = comments_part {
        crate::comments::assign_para_ids(&mut comments);
        for comment in &mut comments {
            link_hyperlinks(&mut comment.blocks, &name, &mut links);
        }
        let xml = crate::comments::comments_xml(&comments, root);
        ensure_override(&mut overrides, &name, CT_COMMENTS);
        replaced.push(name.clone());
        parts.push((name, Cow::Owned(xml.into_bytes())));
        let extended = match &source.comments_extended_part {
            Some((name, root)) => Some((name.clone(), Some(root.as_slice()))),
            None if comments.iter().any(|c| c.done || c.parent.is_some()) => Some((
                links.find_or_add(
                    &main,
                    REL_COMMENTS_EXTENDED,
                    &["/commentsExtended"],
                    &join(base, "commentsExtended.xml"),
                    false,
                ),
                None,
            )),
            None => None,
        };
        if let Some((name, root)) = extended {
            let xml = crate::comments::comments_extended_xml(&comments, root);
            ensure_override(&mut overrides, &name, CT_COMMENTS_EXTENDED);
            replaced.push(name.clone());
            parts.push((name, Cow::Owned(xml.into_bytes())));
        }
    }
    for (name, data) in media {
        if let Some((_, ext)) = name.rsplit_once('.')
            && !defaults.iter().any(|(e, _)| e.eq_ignore_ascii_case(ext))
        {
            defaults.push((
                ext.to_ascii_lowercase(),
                image_content_type(ext).to_string(),
            ));
        }
        replaced.push(name.clone());
        parts.push((name, Cow::Owned(data)));
    }

    parts.insert(
        0,
        (
            main.clone(),
            serialize::document_xml(doc).into_bytes().into(),
        ),
    );
    ensure_override(&mut overrides, &main, CT_MAIN);

    let base_dir = main.rsplit_once('/').map_or("", |(dir, _)| dir);
    let styles_part = match &source.styles_part {
        Some(name) => Some(name.clone()),
        None if doc.styles != Styles::default() => Some(links.find_or_add(
            &main,
            REL_STYLES,
            &["/styles"],
            &join(base_dir, "styles.xml"),
            false,
        )),
        None => None,
    };
    if let Some(name) = &styles_part {
        let root = (!source.styles_root_attributes.is_empty())
            .then_some(source.styles_root_attributes.as_slice());
        let xml = serialize::styles_xml(&doc.styles, root);
        parts.push((name.clone(), xml.into_bytes().into()));
        ensure_override(&mut overrides, name, CT_STYLES);
        replaced.push(name.clone());
    }

    let numbering_part = match &source.numbering_part {
        Some(name) => Some(name.clone()),
        None if !doc.numbering.is_empty() => Some(links.find_or_add(
            &main,
            REL_NUMBERING,
            &["/numbering"],
            &join(base_dir, "numbering.xml"),
            false,
        )),
        None => None,
    };
    if let Some(name) = &numbering_part {
        let root = (!source.numbering_root_attributes.is_empty())
            .then_some(source.numbering_root_attributes.as_slice());
        let xml = crate::numbering::numbering_xml(&doc.numbering, root);
        parts.push((name.clone(), xml.into_bytes().into()));
        ensure_override(&mut overrides, name, CT_NUMBERING);
        replaced.push(name.clone());
    }

    // Настройки, которые мы меняем: расстановка переносов и флаг
    // `w:displayBackgroundShape`, без которого Word не показывает цвет
    // страницы. Остальное в части не трогается.
    let existing = links.existing(&main, &["/settings"]);
    let original = existing
        .as_deref()
        .and_then(|name| source.part(name))
        .map(|p| p.data.as_slice());
    let externals = existing
        .as_deref()
        .map(|name| links.externals(name))
        .unwrap_or_default();
    // Источник данных слияния — внешняя связь части настроек.
    let merge_source = doc
        .settings
        .mail_merge
        .as_ref()
        .and_then(|m| m.data_source.as_deref());
    let source_id = merge_source.map(|address| {
        let name = links.find_or_add(
            &main,
            REL_SETTINGS,
            &["/settings"],
            &join(base_dir, "settings.xml"),
            false,
        );
        links.external_of_kind(&name, REL_MAIL_MERGE_SOURCE, None, address)
    });
    // Получатели без отметок — своя часть, связанная с настройками.
    let recipients = doc
        .settings
        .mail_merge
        .as_ref()
        .filter(|m| !m.excluded.is_empty());
    let recipients_id = recipients.map(|merge| {
        let settings_name = links.find_or_add(
            &main,
            REL_SETTINGS,
            &["/settings"],
            &join(base_dir, "settings.xml"),
            false,
        );
        let (id, name) = links.internal_of_kind(
            &settings_name,
            REL_RECIPIENT_DATA,
            &join(base_dir, "recipientData.xml"),
        );
        let xml = crate::settings::recipients_xml(&merge.excluded);
        parts.push((name.clone(), xml.into_bytes().into()));
        ensure_override(&mut overrides, &name, CT_RECIPIENT_DATA);
        replaced.push(name);
        id
    });
    if let Some(xml) = crate::settings::updated_settings_with(
        original,
        &doc.settings,
        doc.background.is_some(),
        &externals,
        source_id.as_deref(),
        recipients_id.as_deref(),
    ) {
        let name = links.find_or_add(
            &main,
            REL_SETTINGS,
            &["/settings"],
            &join(base_dir, "settings.xml"),
            false,
        );
        parts.push((name.clone(), xml.into_bytes().into()));
        ensure_override(&mut overrides, &name, CT_SETTINGS);
        replaced.push(name);
    }

    let core = &doc.properties.core;
    let core_part = match &source.core_part {
        Some(name) => Some(name.clone()),
        None if *core != CoreProperties::default() => Some(links.find_or_add(
            "",
            REL_CORE,
            &["/core-properties"],
            "docProps/core.xml",
            false,
        )),
        None => None,
    };
    if let Some(name) = &core_part {
        parts.push((name.clone(), properties::core_xml(core).into_bytes().into()));
        ensure_override(&mut overrides, name, CT_CORE);
        replaced.push(name.clone());
    }

    let app = &doc.properties.app;
    let app_part = match &source.app_part {
        Some(name) => Some(name.clone()),
        None if *app != AppProperties::default() => Some(links.find_or_add(
            "",
            REL_APP,
            &["/extended-properties", "/extendedProperties"],
            "docProps/app.xml",
            false,
        )),
        None => None,
    };
    if let Some(name) = &app_part {
        parts.push((name.clone(), properties::app_xml(app).into_bytes().into()));
        ensure_override(&mut overrides, name, CT_APP);
        replaced.push(name.clone());
    }

    for (name, xml) in links.changed() {
        parts.push((name.clone(), xml.into_bytes().into()));
        replaced.push(name);
    }
    for part in &source.parts {
        if !replaced.iter().any(|r| r.eq_ignore_ascii_case(&part.name)) {
            parts.push((part.name.clone(), Cow::Borrowed(part.data.as_slice())));
        }
    }

    let mut zip = ZipWriter::new(writer);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file(CONTENT_TYPES_PART, options)?;
    zip.write_all(content_types_xml(&defaults, &overrides).as_bytes())?;
    for (name, data) in &parts {
        zip.start_file(name.as_str(), options)?;
        zip.write_all(data)?;
    }
    zip.finish()?;
    Ok(())
}

/// Копия исходного пакета для записи файлом разновидности `kind`: тип
/// содержимого главной части `main` — от `kind`, а если в `kind` нельзя
/// хранить макросы, из пакета убраны части макросов, их файлы связей
/// и связи, которые на них ведут.
fn retarget(source: &SourcePackage, main: &str, kind: FileKind) -> SourcePackage {
    let mut result = source.clone();
    // Тип меняется на месте, чтобы порядок записей не сдвигался.
    match result
        .content_types
        .iter_mut()
        .find(|(part, _)| part.eq_ignore_ascii_case(main))
    {
        Some((_, content_type)) => *content_type = kind.content_type().to_string(),
        None => result
            .content_types
            .push((main.to_string(), kind.content_type().to_string())),
    }
    if kind.has_macros() {
        return result;
    }

    // Части макросов: по типу содержимого и по типу связи на них.
    let mut removed: Vec<String> = source
        .parts
        .iter()
        .filter(|p| {
            content_type_of_part(source, &p.name).is_some_and(|t| MACRO_CONTENT_TYPES.contains(&t))
        })
        .map(|p| p.name.clone())
        .collect();
    let links = Links::new(source);
    let mut rels_files = Vec::new();
    for part in &source.parts {
        if let Some(from) = source_of_rels(&part.name) {
            let rels = links.load(&from, &part.name);
            for rel in &rels {
                if !rel.external && MACRO_RELATIONSHIPS.iter().any(|s| rel.kind.ends_with(s)) {
                    removed.push(rel.target.clone());
                }
            }
            rels_files.push((part.name.clone(), rels));
        }
    }
    // Файлы связей самих частей макросов тоже не нужны.
    let removed_rels: Vec<String> = removed.iter().map(|r| rels_part_for(r)).collect();
    let is_removed = |name: &str| {
        removed
            .iter()
            .chain(&removed_rels)
            .any(|r| r.eq_ignore_ascii_case(name))
    };
    result.parts.retain(|p| !is_removed(&p.name));
    result.content_types.retain(|(part, _)| !is_removed(part));
    for (name, rels) in rels_files {
        if is_removed(&name) {
            continue;
        }
        let before = rels.len();
        let kept: Vec<Relationship> = rels
            .into_iter()
            .filter(|r| r.external || !is_removed(&r.target))
            .collect();
        if let Some(part) = result
            .parts
            .iter_mut()
            .find(|p| p.name.eq_ignore_ascii_case(&name))
            && kept.len() != before
        {
            part.data = relationships_xml(&kept).into_bytes();
        }
    }
    result
}

/// Часть, связи которой лежат в файле связей `rels_name`
/// (`word/_rels/document.xml.rels` → `word/document.xml`, `_rels/.rels` →
/// пустая строка — сам пакет). Для других имён — `None`.
fn source_of_rels(rels_name: &str) -> Option<String> {
    let (dir, file) = rels_name.rsplit_once('/').unwrap_or(("", rels_name));
    let name = file.strip_suffix(".rels")?;
    let dir = if dir.eq_ignore_ascii_case("_rels") {
        ""
    } else {
        dir.strip_suffix("/_rels")?
    };
    Some(join(dir, name))
}

/// Тип содержимого части исходного пакета: свой (`Override`) или по
/// расширению имени (`Default`).
pub(crate) fn content_type_of_part<'s>(source: &'s SourcePackage, name: &str) -> Option<&'s str> {
    let by_override = source
        .content_types
        .iter()
        .find(|(part, _)| part.eq_ignore_ascii_case(name));
    let by_extension = || {
        let (_, extension) = name.rsplit_once('.')?;
        source
            .default_content_types
            .iter()
            .find(|(ext, _)| ext.eq_ignore_ascii_case(extension))
    };
    by_override
        .or_else(by_extension)
        .map(|(_, content_type)| content_type.as_str())
}

/// Проверяет, что связь каждого рисунка ведёт к его картинке. Если нет
/// (новый рисунок, рисунок из другого документа), картинка добавляется
/// в `media` новой частью со своей связью, а в копии документа рисунок
/// получает эту связь. Если всё верно, документ не копируется.
fn link_pictures<'d>(
    doc: &'d Document,
    main: &str,
    links: &mut Links<'_>,
    media: &mut Vec<(String, Vec<u8>)>,
) -> Cow<'d, Document> {
    let mut broken = false;
    let mut probe = |picture: &Picture| {
        if let Some(image) = &picture.image
            && image.chart.is_none()
            && !links.points_to(main, picture.relationship.as_deref(), image)
        {
            broken = true;
        }
    };
    visit_pictures(doc, &mut probe);
    if !broken {
        return Cow::Borrowed(doc);
    }
    let mut fixed = doc.clone();
    // Одна и та же картинка (общие байты) — одна часть.
    let mut added: Vec<(Arc<ImageData>, String)> = Vec::new();
    fixed.for_each_picture_mut(|picture| {
        let Some(image) = picture.image.clone() else {
            return;
        };
        // Диаграмма остаётся при своей части и связи.
        if image.chart.is_some() {
            return;
        }
        if links.points_to(main, picture.relationship.as_deref(), &image) {
            return;
        }
        let id = match added.iter().find(|(a, _)| Arc::ptr_eq(a, &image)) {
            Some((_, id)) => id.clone(),
            None => {
                let base = main.rsplit_once('/').map_or("", |(dir, _)| dir);
                let file = image.name.rsplit('/').next().unwrap_or_default();
                let file = if file.contains('.') {
                    file
                } else {
                    "image.png"
                };
                let wanted = join(base, &format!("media/{file}"));
                let (id, name) = links.add(main, REL_IMAGE, &wanted);
                media.push((name, image.bytes.clone()));
                added.push((image, id.clone()));
                id
            }
        };
        if let Some(raw) = &picture.raw {
            picture.raw = Some(crate::picture::with_relationship(raw, &id));
        }
        picture.relationship = Some(id);
    });
    Cow::Owned(fixed)
}

/// Колонтитулы, которые нужно записать: имя части, верхний ли
/// колонтитул и содержимое. Новые колонтитулы (без связи) получают
/// новую часть и связь — в копии документа. Колонтитул, часть которого
/// не была прочитана в модель, не переписывается: его часть сохранится
/// как есть.
type Stories = Vec<(String, bool, Vec<Block>)>;

fn link_headers_footers<'d>(
    doc: Cow<'d, Document>,
    main: &str,
    links: &mut Links<'_>,
) -> (Cow<'d, Document>, Stories) {
    let unlinked = doc.sections.iter().any(|s| {
        [&s.props.headers, &s.props.footers]
            .iter()
            .any(|set| set.iter().any(|(_, story)| story.relationship.is_none()))
    });
    let base = main.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut created: Vec<String> = Vec::new();
    let doc = if unlinked {
        let mut doc = doc.into_owned();
        for section in &mut doc.sections {
            let props = &mut section.props;
            for (set, header) in [(&mut props.headers, true), (&mut props.footers, false)] {
                for kind in HeaderFooterKind::ALL {
                    let Some(story) = set.get_mut(kind) else {
                        continue;
                    };
                    if story.relationship.is_some() {
                        continue;
                    }
                    let (kind, file) = if header {
                        (REL_HEADER, "header.xml")
                    } else {
                        (REL_FOOTER, "footer.xml")
                    };
                    let (id, name) = links.add(main, kind, &join(base, file));
                    story.relationship = Some(id);
                    created.push(name);
                }
            }
        }
        Cow::Owned(doc)
    } else {
        doc
    };
    let mut stories: Stories = Vec::new();
    for section in &doc.sections {
        for (set, header) in [
            (&section.props.headers, true),
            (&section.props.footers, false),
        ] {
            for (_, story) in set.iter() {
                let Some(name) = story
                    .relationship
                    .as_deref()
                    .and_then(|id| links.target(main, id))
                else {
                    continue;
                };
                let known = created.iter().any(|c| c.eq_ignore_ascii_case(&name))
                    || links
                        .source
                        .header_footer_parts
                        .iter()
                        .any(|(part, _)| part.eq_ignore_ascii_case(&name));
                if known
                    && !stories
                        .iter()
                        .any(|(n, _, _)| n.eq_ignore_ascii_case(&name))
                {
                    stories.push((name, header, story.blocks.clone()));
                }
            }
        }
    }
    (doc, stories)
}

/// Гиперссылки документа получают связи с адресами (см.
/// [`link_hyperlinks`]); без ссылок документ не копируется.
fn link_hyperlinks_in<'d>(
    doc: Cow<'d, Document>,
    main: &str,
    links: &mut Links<'_>,
) -> Cow<'d, Document> {
    fn has_links(blocks: &[Block]) -> bool {
        blocks.iter().any(|block| match block {
            Block::Paragraph(p) => p.runs().any(|r| r.props.link.is_some()),
            Block::Table(t) => t
                .rows
                .iter()
                .flat_map(|r| &r.cells)
                .any(|c| has_links(&c.blocks)),
            Block::Unknown(_) => false,
        })
    }
    if !doc.sections.iter().any(|s| has_links(&s.blocks)) {
        return doc;
    }
    let mut doc = doc.into_owned();
    for section in &mut doc.sections {
        link_hyperlinks(&mut section.blocks, main, links);
    }
    Cow::Owned(doc)
}

/// Ссылки с адресом получают связь части `from` с этим адресом: свою,
/// если она ведёт туда же, иначе такую же или новую. У ссылки без адреса
/// (только на закладку) связи нет.
fn link_hyperlinks(blocks: &mut [Block], from: &str, links: &mut Links<'_>) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                for inline in &mut p.content {
                    let Inline::Run(run) = inline else {
                        continue;
                    };
                    let Some(link) = &mut run.props.link else {
                        continue;
                    };
                    match &link.target {
                        Some(address) => {
                            let id = links.external(from, link.relationship.as_deref(), address);
                            link.relationship = Some(id);
                        }
                        None => link.relationship = None,
                    }
                }
            }
            Block::Table(t) => {
                for cell in t.rows.iter_mut().flat_map(|r| &mut r.cells) {
                    link_hyperlinks(&mut cell.blocks, from, links);
                }
            }
            Block::Unknown(_) => {}
        }
    }
}

/// Вызывает `f` для каждого рисунка документа (без изменения).
fn visit_pictures(doc: &Document, f: &mut impl FnMut(&Picture)) {
    fn blocks(list: &[Block], f: &mut impl FnMut(&Picture)) {
        for block in list {
            match block {
                Block::Paragraph(p) => {
                    for run in p.runs() {
                        for item in &run.content {
                            if let RunContent::Picture(picture) = item {
                                f(picture);
                            }
                        }
                    }
                }
                Block::Table(table) => {
                    for cell in table.rows.iter().flat_map(|r| &r.cells) {
                        blocks(&cell.blocks, f);
                    }
                }
                Block::Unknown(_) => {}
            }
        }
    }
    for section in &doc.sections {
        blocks(&section.blocks, f);
    }
}

/// Тип содержимого картинки по расширению имени.
fn image_content_type(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" | "jpe" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "emf" => "image/x-emf",
        "wmf" => "image/x-wmf",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// Добавляет `Override`, если для части ещё нет своего типа содержимого.
fn ensure_override(overrides: &mut Vec<(String, String)>, part: &str, content_type: &str) {
    if !overrides.iter().any(|(p, _)| p.eq_ignore_ascii_case(part)) {
        overrides.push((part.to_string(), content_type.to_string()));
    }
}

/// Связь из файла `.rels`.
struct Relationship {
    id: String,
    kind: String,
    /// Для внутренних связей — имя части-цели внутри архива, для внешних —
    /// адрес как есть.
    target: String,
    external: bool,
}

/// Файлы связей, которые нужно изменить при записи.
struct Links<'a> {
    source: &'a SourcePackage,
    /// Изменённые файлы связей: имя файла связей и связи в нём.
    changed: Vec<(String, Vec<Relationship>)>,
}

impl<'a> Links<'a> {
    fn new(source: &'a SourcePackage) -> Links<'a> {
        Links {
            source,
            changed: Vec::new(),
        }
    }

    /// Ищет у части `from` (пустая строка — сам пакет) связь, тип которой
    /// оканчивается на один из `suffixes`, и возвращает имя её цели. Если
    /// связи нет, добавляет связь типа `kind` на свободное имя рядом
    /// с `wanted` (или ровно на `wanted`, если `exact`). Если связь есть,
    /// а часть-цель испорчена, новая часть запишется на её место.
    fn find_or_add(
        &mut self,
        from: &str,
        kind: &str,
        suffixes: &[&str],
        wanted: &str,
        exact: bool,
    ) -> String {
        let rels_name = rels_part_for(from);
        let index = match self.changed.iter().position(|(n, _)| *n == rels_name) {
            Some(i) => i,
            None => {
                let rels = self.load(from, &rels_name);
                if let Some(target) = find(&rels, suffixes) {
                    return target;
                }
                self.changed.push((rels_name, rels));
                self.changed.len() - 1
            }
        };
        if let Some(target) = find(&self.changed[index].1, suffixes) {
            return target;
        }
        let name = if exact {
            wanted.to_string()
        } else {
            self.free_name(wanted)
        };
        push_relationship(&mut self.changed[index].1, kind, &name);
        name
    }

    /// Имя цели связи части `from` с типом, который оканчивается на один
    /// из `suffixes`, если такая связь уже есть; ничего не добавляет.
    fn existing(&self, from: &str, suffixes: &[&str]) -> Option<String> {
        let rels_name = rels_part_for(from);
        match self.changed.iter().find(|(n, _)| *n == rels_name) {
            Some((_, rels)) => find(rels, suffixes),
            None => find(&self.load(from, &rels_name), suffixes),
        }
    }

    /// Ведёт ли связь `id` части `from` к картинке `image`: связь
    /// с картинкой, имя части и байты совпадают.
    fn points_to(&self, from: &str, id: Option<&str>, image: &ImageData) -> bool {
        let Some(id) = id else {
            return false;
        };
        let rels_name = rels_part_for(from);
        let owned;
        let rels = match self.changed.iter().find(|(n, _)| *n == rels_name) {
            Some((_, rels)) => rels,
            None => {
                owned = self.load(from, &rels_name);
                &owned
            }
        };
        rels.iter().any(|r| {
            r.id == id
                && !r.external
                && r.kind.ends_with("/image")
                && r.target.eq_ignore_ascii_case(&image.name)
                && self
                    .source
                    .part(&r.target)
                    .is_some_and(|p| p.data == image.bytes)
        })
    }

    /// Добавляет части `from` связь типа `kind` с новой частью на
    /// свободном имени рядом с `wanted`; возвращает имя связи и части.
    fn add(&mut self, from: &str, kind: &str, wanted: &str) -> (String, String) {
        let rels_name = rels_part_for(from);
        let index = match self.changed.iter().position(|(n, _)| *n == rels_name) {
            Some(i) => i,
            None => {
                let rels = self.load(from, &rels_name);
                self.changed.push((rels_name, rels));
                self.changed.len() - 1
            }
        };
        let name = self.free_name(wanted);
        let id = push_relationship(&mut self.changed[index].1, kind, &name);
        (id, name)
    }

    /// Внутренняя связь типа `kind` части `from`: имя связи и части —
    /// прежние, если такая связь есть, иначе новая связь с частью рядом
    /// с `wanted`.
    fn internal_of_kind(&mut self, from: &str, kind: &str, wanted: &str) -> (String, String) {
        let suffix = kind.rsplit_once('/').map_or(kind, |(_, s)| s);
        let rels_name = rels_part_for(from);
        let found = |rels: &[Relationship]| {
            rels.iter()
                .find(|r| !r.external && r.kind.ends_with(&format!("/{suffix}")))
                .map(|r| (r.id.clone(), r.target.clone()))
        };
        let existing = match self.changed.iter().find(|(n, _)| *n == rels_name) {
            Some((_, rels)) => found(rels),
            None => found(&self.load(from, &rels_name)),
        };
        existing.unwrap_or_else(|| self.add(from, kind, wanted))
    }

    /// Внешняя связь части `from` с адресом гиперссылки `address`:
    /// `id`, если она ведёт туда же, иначе такая же связь или новая.
    fn external(&mut self, from: &str, id: Option<&str>, address: &str) -> String {
        self.external_of_kind(from, REL_HYPERLINK, id, address)
    }

    /// Внешние связи части `from`: имя связи и адрес.
    fn externals(&self, from: &str) -> Vec<(String, String)> {
        let rels_name = rels_part_for(from);
        let pairs = |rels: &[Relationship]| {
            rels.iter()
                .filter(|r| r.external)
                .map(|r| (r.id.clone(), r.target.clone()))
                .collect()
        };
        match self.changed.iter().find(|(n, _)| *n == rels_name) {
            Some((_, rels)) => pairs(rels),
            None => pairs(&self.load(from, &rels_name)),
        }
    }

    /// Внешняя связь типа `kind` части `from` с адресом `address`: `id`,
    /// если она ведёт туда же, иначе такая же связь или новая.
    fn external_of_kind(
        &mut self,
        from: &str,
        kind: &str,
        id: Option<&str>,
        address: &str,
    ) -> String {
        let suffix = kind.rsplit_once('/').map_or(kind, |(_, s)| s);
        let matches = |r: &Relationship| {
            r.external && r.kind.ends_with(&format!("/{suffix}")) && r.target == address
        };
        let found = |rels: &[Relationship]| {
            id.filter(|id| rels.iter().any(|r| r.id == *id && matches(r)))
                .map(str::to_string)
                .or_else(|| rels.iter().find(|r| matches(r)).map(|r| r.id.clone()))
        };
        let rels_name = rels_part_for(from);
        let index = match self.changed.iter().position(|(n, _)| *n == rels_name) {
            Some(i) => i,
            None => {
                let rels = self.load(from, &rels_name);
                if let Some(id) = found(&rels) {
                    return id;
                }
                self.changed.push((rels_name, rels));
                self.changed.len() - 1
            }
        };
        let rels = &mut self.changed[index].1;
        if let Some(id) = found(rels) {
            return id;
        }
        let id = push_relationship(rels, kind, address);
        if let Some(last) = rels.last_mut() {
            last.external = true;
        }
        id
    }

    /// Имя части, к которой ведёт внутренняя связь `id` части `from`.
    fn target(&self, from: &str, id: &str) -> Option<String> {
        let rels_name = rels_part_for(from);
        let find = |rels: &[Relationship]| {
            rels.iter()
                .find(|r| r.id == id && !r.external)
                .map(|r| r.target.clone())
        };
        match self.changed.iter().find(|(n, _)| *n == rels_name) {
            Some((_, rels)) => find(rels),
            None => find(&self.load(from, &rels_name)),
        }
    }

    /// Связи из исходного файла (битый или отсутствующий файл — нет связей).
    fn load(&self, from: &str, rels_name: &str) -> Vec<Relationship> {
        let base_dir = from.rsplit_once('/').map_or("", |(dir, _)| dir);
        let Some(root) = self
            .source
            .part(rels_name)
            .and_then(|p| xml::parse(&p.data).ok())
        else {
            return Vec::new();
        };
        root.elements()
            .filter(|e| e.local == "Relationship")
            .filter_map(|e| {
                let external = attr(e, "TargetMode") == Some("External");
                let target = attr(e, "Target")?;
                Some(Relationship {
                    id: attr(e, "Id")?.to_string(),
                    kind: attr(e, "Type")?.to_string(),
                    target: if external {
                        target.to_string()
                    } else {
                        resolve_target(base_dir, target)
                    },
                    external,
                })
            })
            .collect()
    }

    /// `wanted`, если такое имя свободно, иначе `имя2.xml`, `имя3.xml`…
    fn free_name(&self, wanted: &str) -> String {
        let taken = |name: &str| {
            self.source.part(name).is_some()
                || name.eq_ignore_ascii_case(&self.source.main_part)
                || self
                    .changed
                    .iter()
                    .flat_map(|(_, rels)| rels)
                    .any(|r| !r.external && r.target.eq_ignore_ascii_case(name))
        };
        let (stem, ext) = wanted.rsplit_once('.').unwrap_or((wanted, ""));
        let mut name = wanted.to_string();
        let mut n = 1;
        while taken(&name) {
            n += 1;
            name = format!("{stem}{n}.{ext}");
        }
        name
    }

    /// Изменённые файлы связей: имя и содержимое.
    fn changed(self) -> Vec<(String, String)> {
        self.changed
            .into_iter()
            .map(|(name, rels)| (name, relationships_xml(&rels)))
            .collect()
    }
}

/// Добавляет внутреннюю связь со свободным именем `rId<N>`; возвращает имя.
fn push_relationship(rels: &mut Vec<Relationship>, kind: &str, target: &str) -> String {
    let mut number = rels.len() + 1;
    let id = loop {
        let id = format!("rId{number}");
        if !rels.iter().any(|r| r.id == id) {
            break id;
        }
        number += 1;
    };
    rels.push(Relationship {
        id: id.clone(),
        kind: kind.into(),
        target: target.into(),
        external: false,
    });
    id
}

fn find(rels: &[Relationship], suffixes: &[&str]) -> Option<String> {
    rels.iter()
        .find(|r| !r.external && suffixes.iter().any(|s| r.kind.ends_with(s)))
        .map(|r| r.target.clone())
}

fn resolve_target(base_dir: &str, target: &str) -> String {
    if target.starts_with('/') {
        normalize_part_name(target)
    } else {
        normalize_part_name(&join(base_dir, target))
    }
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// Файл связей. Цели внутренних связей пишутся абсолютными (`/word/...`),
/// так их понимают все программы и не нужно считать относительный путь.
fn relationships_xml(rels: &[Relationship]) -> String {
    let mut out = format!("{XML_DECLARATION}<Relationships xmlns=\"{NS_RELS}\">");
    for rel in rels {
        let target = if rel.external {
            rel.target.clone()
        } else {
            format!("/{}", encode_part_name(&rel.target))
        };
        out.push_str(&format!(
            "<Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"{}/>",
            escape_attr(&rel.id),
            escape_attr(&rel.kind),
            escape_attr(&target),
            if rel.external {
                " TargetMode=\"External\""
            } else {
                ""
            }
        ));
    }
    out.push_str("</Relationships>");
    out
}

fn content_types_xml(defaults: &[(String, String)], overrides: &[(String, String)]) -> String {
    let mut out = format!("{XML_DECLARATION}<Types xmlns=\"{NS_CONTENT_TYPES}\">");
    for (ext, content_type) in defaults {
        out.push_str(&format!(
            "<Default Extension=\"{}\" ContentType=\"{}\"/>",
            escape_attr(ext),
            escape_attr(content_type)
        ));
    }
    for (part, content_type) in overrides {
        out.push_str(&format!(
            "<Override PartName=\"/{}\" ContentType=\"{}\"/>",
            escape_attr(&encode_part_name(part)),
            escape_attr(content_type)
        ));
    }
    out.push_str("</Types>");
    out
}

const XML_DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";

/// Кодирует имя части для адреса: пробелы, русские буквы и прочие
/// символы вне разрешённого набора — как `%XX`.
fn encode_part_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for byte in name.bytes() {
        let allowed = byte.is_ascii_alphanumeric() || b"-._~/!$&'()*+,;=:@".contains(&byte);
        if allowed {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Сноски, на которые ссылается текст `blocks`: вид и номер.
/// Номера примечаний, отмеченных в тексте (началом, концом или ссылкой).
fn comment_marks<'d>(blocks: &'d [Block], out: &mut Vec<&'d str>) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                for inline in &p.content {
                    match inline {
                        Inline::CommentStart(id) | Inline::CommentEnd(id) => out.push(id),
                        Inline::Run(run) => {
                            for item in &run.content {
                                if let RunContent::CommentReference(id) = item {
                                    out.push(id);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Block::Table(t) => {
                for cell in t.rows.iter().flat_map(|r| &r.cells) {
                    comment_marks(&cell.blocks, out);
                }
            }
            Block::Unknown(_) => {}
        }
    }
}

fn note_references<'d>(blocks: &'d [Block], out: &mut Vec<(NoteKind, &'d str)>) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                for item in p.runs().flat_map(|r| &r.content) {
                    if let RunContent::NoteReference(r) = item {
                        out.push((r.kind, r.id.as_str()));
                    }
                }
            }
            Block::Table(t) => {
                for cell in t.rows.iter().flat_map(|r| &r.cells) {
                    note_references(&cell.blocks, out);
                }
            }
            Block::Unknown(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_names_are_percent_encoded() {
        assert_eq!(encode_part_name("word/document.xml"), "word/document.xml");
        assert_eq!(encode_part_name("word/my doc.xml"), "word/my%20doc.xml");
        assert_eq!(encode_part_name("word/Ж.xml"), "word/%D0%96.xml");
        assert_eq!(
            normalize_part_name(&encode_part_name("a/Ж b%.xml")),
            "a/Ж b%.xml"
        );
    }

    #[test]
    fn styles_relationship_gets_a_free_id_and_name() {
        let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId2" Type="http://x/image" Target="media/a.png"/>
<Relationship Id="rId1" Type="http://x/hyperlink" Target="https://example.com/a?b&amp;c" TargetMode="External"/>
</Relationships>"#;
        let source = SourcePackage {
            parts: vec![
                letter_model::SourcePart {
                    name: "word/_rels/document.xml.rels".into(),
                    data: rels.as_bytes().to_vec(),
                },
                letter_model::SourcePart {
                    name: "word/styles.xml".into(),
                    data: Vec::new(),
                },
            ],
            ..SourcePackage::default()
        };
        let mut links = Links::new(&source);
        let name = links.find_or_add(
            "word/document.xml",
            REL_STYLES,
            &["/styles"],
            "word/styles.xml",
            false,
        );
        let changed = links.changed();
        assert_eq!(changed.len(), 1);
        let (rels_name, xml) = &changed[0];
        assert_eq!(rels_name, "word/_rels/document.xml.rels");
        assert_eq!(name, "word/styles2.xml");
        assert!(xml.contains(r#"Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="/word/styles2.xml""#));
        assert!(xml.contains(r#"Target="/word/media/a.png""#));
        assert!(xml.contains(r#"Target="https://example.com/a?b&amp;c" TargetMode="External""#));
    }
}
