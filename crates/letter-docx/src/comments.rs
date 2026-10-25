//! Примечания: часть `word/comments.xml` (текст, автор, дата) и часть
//! `word/commentsExtended.xml` (ответы и отметки «решено», Word 2013+).
//!
//! Во второй части примечание указано номером своего последнего абзаца
//! (`w14:paraId`), а не номером примечания, поэтому при записи у этого
//! абзаца ставится `w14:paraId`.

use letter_model::{Block, Comment, Paragraph};

use crate::document::read_block;
use crate::serialize::{block_xml, element_with, escape_attr, root_attributes, xml_part};
use crate::xml::Element;

/// Пространство имён Word 2010 (`w14:paraId`).
pub(crate) const W14_NS: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
/// Пространство имён Word 2012 (`w15:commentEx`).
pub(crate) const W15_NS: &str = "http://schemas.microsoft.com/office/word/2012/wordml";
/// Пространство имён разметки совместимости (`mc:Ignorable`).
const MC_NS: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

/// Значение атрибута `local` из пространства имён `ns`.
fn attr<'a>(element: &'a Element, ns: &str, local: &str) -> Option<&'a str> {
    element
        .attrs
        .iter()
        .find(|a| a.ns.as_deref() == Some(ns) && a.local == local)
        .map(|a| a.value.as_str())
}

/// Примечания из части `w:comments`; `None`, если в части есть что-то
/// непонятное (тогда часть сохраняется как есть).
pub(crate) fn read_comments(root: &Element) -> Option<Vec<Comment>> {
    if !root.is_w("comments") {
        return None;
    }
    let mut comments = Vec::new();
    for child in root.elements() {
        if !child.is_w("comment") {
            return None;
        }
        let para_id = child
            .elements()
            .filter(|e| e.is_w("p"))
            .last()
            .and_then(|p| attr(p, W14_NS, "paraId"))
            .map(str::to_string);
        comments.push(Comment {
            id: child.w_attr("id")?.to_string(),
            author: child.w_attr("author").unwrap_or_default().to_string(),
            initials: child.w_attr("initials").map(str::to_string),
            date: child.w_attr("date").map(str::to_string),
            blocks: child.elements().map(read_block).collect(),
            parent: None,
            done: false,
            para_id,
        });
    }
    Some(comments)
}

/// Ответы и отметки «решено» из части `w15:commentsEx`. `false`, если
/// часть не понята (тогда она сохраняется как есть).
pub(crate) fn read_extended(root: &Element, comments: &mut [Comment]) -> bool {
    if root.ns.as_deref() != Some(W15_NS) || root.local != "commentsEx" {
        return false;
    }
    let mut entries = Vec::new();
    for child in root.elements() {
        if child.ns.as_deref() != Some(W15_NS) || child.local != "commentEx" {
            return false;
        }
        let Some(para) = attr(child, W15_NS, "paraId") else {
            return false;
        };
        let parent = attr(child, W15_NS, "paraIdParent");
        let done = matches!(attr(child, W15_NS, "done"), Some("1" | "true" | "on"));
        entries.push((para.to_string(), parent.map(str::to_string), done));
    }
    let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    for (para, parent, done) in entries {
        let parent_id = parent.and_then(|parent| {
            comments
                .iter()
                .find(|c| c.para_id.as_deref().is_some_and(|p| same(p, &parent)))
                .map(|c| c.id.clone())
        });
        if let Some(comment) = comments
            .iter_mut()
            .find(|c| c.para_id.as_deref().is_some_and(|p| same(p, &para)))
        {
            comment.parent = parent_id;
            comment.done = done;
        }
    }
    true
}

/// Номера последних абзацев примечаний для записи: прежние из файла,
/// новым — свободные. Номер меньше `0x80000000`, как требует Word.
pub(crate) fn para_ids(comments: &[Comment]) -> Vec<String> {
    let taken: Vec<u32> = comments
        .iter()
        .filter_map(|c| c.para_id.as_deref())
        .filter_map(|p| u32::from_str_radix(p, 16).ok())
        .collect();
    let mut next = 0x1000_0000u32;
    comments
        .iter()
        .map(|c| match &c.para_id {
            Some(p) if u32::from_str_radix(p, 16).is_ok_and(|n| n < 0x8000_0000) => {
                p.to_ascii_uppercase()
            }
            _ => {
                while taken.contains(&next) {
                    next += 1;
                }
                let id = format!("{next:08X}");
                next += 1;
                id
            }
        })
        .collect()
}

/// Атрибуты корня с объявлениями нужных пространств имён.
fn with_namespaces(
    root: Option<&[(String, String)]>,
    needed: &[(&str, &str)],
) -> Vec<(String, String)> {
    let mut attrs = root_attributes(root);
    for (prefix, uri) in needed {
        let name = format!("xmlns:{prefix}");
        if !attrs.iter().any(|(n, _)| *n == name) {
            attrs.push((name, uri.to_string()));
        }
    }
    attrs
}

/// Часть примечаний (`w:comments`); `root` — атрибуты корня из исходного
/// файла, `para_ids` — из [`para_ids`].
pub(crate) fn comments_xml(
    comments: &[Comment],
    para_ids: &[String],
    root: Option<&[(String, String)]>,
) -> String {
    let declared = |attrs: &[(String, String)]| attrs.iter().any(|(n, _)| n == "xmlns:w14");
    let had_w14 = root.is_some_and(declared);
    let mut attrs = with_namespaces(root, &[("w14", W14_NS), ("mc", MC_NS)]);
    // Word 2007 не знает `w14`: пусть пропускает. Корень из файла, где
    // `w14` уже объявлен, не трогаем.
    match attrs.iter_mut().find(|(n, _)| n == "mc:Ignorable") {
        _ if had_w14 => {}
        Some((_, value)) if !value.split_whitespace().any(|p| p == "w14") => {
            value.push_str(" w14");
        }
        Some(_) => {}
        None => attrs.push(("mc:Ignorable".into(), "w14".into())),
    }
    let mut content = String::new();
    for (comment, para) in comments.iter().zip(para_ids) {
        let mut attributes = format!(
            r#" w:id="{}" w:author="{}""#,
            escape_attr(&comment.id),
            escape_attr(&comment.author)
        );
        if let Some(date) = &comment.date {
            attributes.push_str(&format!(r#" w:date="{}""#, escape_attr(date)));
        }
        if let Some(initials) = &comment.initials {
            attributes.push_str(&format!(r#" w:initials="{}""#, escape_attr(initials)));
        }
        content.push_str(&format!("<w:comment{attributes}>"));
        // Номер ставится последнему абзацу; если в конце не абзац (или
        // примечание пустое), добавляется пустой абзац.
        let last = comment
            .blocks
            .iter()
            .rposition(|b| matches!(b, Block::Paragraph(_)))
            .filter(|&i| i + 1 == comment.blocks.len());
        for (i, block) in comment.blocks.iter().enumerate() {
            let xml = block_xml(block);
            if Some(i) == last {
                content.push_str(&with_para_id(&xml, para));
            } else {
                content.push_str(&xml);
            }
        }
        if last.is_none() {
            let xml = block_xml(&Paragraph::default().into());
            content.push_str(&with_para_id(&xml, para));
        }
        content.push_str("</w:comment>");
    }
    xml_part(&element_with("w:comments", &attrs, &content))
}

/// Абзац `w:p` с номером `w14:paraId`.
fn with_para_id(paragraph: &str, para: &str) -> String {
    match paragraph.strip_prefix("<w:p") {
        Some(rest) => format!(r#"<w:p w14:paraId="{para}" w14:textId="77777777"{rest}"#),
        None => paragraph.to_string(),
    }
}

/// Нужна ли часть ответов и отметок «решено».
pub(crate) fn needs_extended(comments: &[Comment]) -> bool {
    comments.iter().any(|c| c.parent.is_some() || c.done)
}

/// Часть ответов и отметок «решено» (`w15:commentsEx`).
pub(crate) fn extended_xml(
    comments: &[Comment],
    para_ids: &[String],
    root: Option<&[(String, String)]>,
) -> String {
    let mut attrs: Vec<(String, String)> = match root {
        Some(attrs) => attrs.to_vec(),
        None => vec![
            ("xmlns:mc".into(), MC_NS.into()),
            ("xmlns:w15".into(), W15_NS.into()),
            ("mc:Ignorable".into(), "w15".into()),
        ],
    };
    if !attrs.iter().any(|(n, _)| n == "xmlns:w15") {
        attrs.push(("xmlns:w15".into(), W15_NS.into()));
    }
    let mut content = String::new();
    for (comment, para) in comments.iter().zip(para_ids) {
        let mut entry = format!(r#"<w15:commentEx w15:paraId="{para}""#);
        let parent = comment.parent.as_ref().and_then(|parent| {
            let i = comments.iter().position(|c| &c.id == parent)?;
            para_ids.get(i)
        });
        if let Some(parent) = parent {
            entry.push_str(&format!(r#" w15:paraIdParent="{parent}""#));
        }
        entry.push_str(&format!(r#" w15:done="{}"/>"#, u8::from(comment.done)));
        content.push_str(&entry);
    }
    xml_part(&element_with("w15:commentsEx", &attrs, &content))
}
