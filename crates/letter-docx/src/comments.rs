//! Примечания: часть `word/comments.xml` (текст, автор, дата) и часть
//! `word/commentsExtended.xml` (ответы и отметка «решено»; связь с
//! примечанием — по метке `w14:paraId` его последнего абзаца).

use letter_model::{Block, Comment, Paragraph};

use crate::document::read_block;
use crate::serialize::{block_xml, element_with, escape_attr, root_attributes, xml_part};
use crate::xml::Element;

/// Пространство имён `w14` (метки абзацев).
const W14_NS: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
/// Пространство имён `w15` (ответы и «решено»).
const W15_NS: &str = "http://schemas.microsoft.com/office/word/2012/wordml";
const MC_NS: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

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
        let blocks: Vec<Block> = child.elements().map(read_block).collect();
        let para_id = child
            .elements()
            .filter(|e| e.is_w("p"))
            .last()
            .and_then(|p| p.attrs.iter().find(|a| a.local == "paraId"))
            .map(|a| a.value.clone());
        comments.push(Comment {
            id: child.w_attr("id")?.to_string(),
            author: child.w_attr("author").unwrap_or_default().to_string(),
            initials: child.w_attr("initials").map(str::to_string),
            date: child.w_attr("date").map(str::to_string),
            blocks,
            parent: None,
            done: false,
            para_id,
        });
    }
    Some(comments)
}

/// Ответы и отметки «решено» из части `w15:commentsEx`.
pub(crate) fn read_comments_extended(root: &Element, comments: &mut [Comment]) {
    let attr = |e: &Element, name: &str| {
        e.attrs
            .iter()
            .find(|a| a.local == name)
            .map(|a| a.value.clone())
    };
    let mut links = Vec::new();
    for item in root.elements().filter(|e| e.local == "commentEx") {
        let Some(para) = attr(item, "paraId") else {
            continue;
        };
        let done = matches!(attr(item, "done").as_deref(), Some("1" | "true" | "on"));
        links.push((para, attr(item, "paraIdParent"), done));
    }
    let id_of = |para: &str, comments: &[Comment]| {
        comments
            .iter()
            .find(|c| {
                c.para_id
                    .as_deref()
                    .is_some_and(|p| p.eq_ignore_ascii_case(para))
            })
            .map(|c| c.id.clone())
    };
    for (para, parent, done) in links {
        let parent = parent.and_then(|p| id_of(&p, comments));
        if let Some(comment) = comments.iter_mut().find(|c| {
            c.para_id
                .as_deref()
                .is_some_and(|p| p.eq_ignore_ascii_case(&para))
        }) {
            comment.done = done;
            comment.parent = parent;
        }
    }
}

/// Даёт метку `w14:paraId` каждому примечанию без неё и ставит в конец
/// текста абзац, если текст кончается не абзацем (метка ставится на
/// последний абзац). Метки — восемь шестнадцатеричных цифр меньше
/// 0x80000000, разные у всех примечаний.
pub(crate) fn assign_para_ids(comments: &mut [Comment]) {
    let mut next: u32 = 0x1000_0000;
    for i in 0..comments.len() {
        if !matches!(comments[i].blocks.last(), Some(Block::Paragraph(_))) {
            comments[i].blocks.push(Paragraph::default().into());
        }
        if comments[i].para_id.is_some() {
            continue;
        }
        let id = loop {
            let id = format!("{next:08X}");
            next = next.wrapping_add(1) & 0x7fff_ffff;
            if !comments.iter().any(|c| {
                c.para_id
                    .as_deref()
                    .is_some_and(|p| p.eq_ignore_ascii_case(&id))
            }) {
                break id;
            }
        };
        comments[i].para_id = Some(id);
    }
}

/// Добавляет объявление `xmlns:<prefix>` и (если `ignorable`) префикс
/// в `mc:Ignorable`, если их нет.
fn declare(attrs: &mut Vec<(String, String)>, prefix: &str, uri: &str, ignorable: bool) {
    let name = format!("xmlns:{prefix}");
    match attrs.iter_mut().find(|(n, _)| *n == name) {
        Some((_, value)) => *value = uri.to_string(),
        None => attrs.push((name, uri.to_string())),
    }
    if !ignorable {
        return;
    }
    if !attrs.iter().any(|(n, _)| n == "xmlns:mc") {
        attrs.push(("xmlns:mc".into(), MC_NS.into()));
    }
    match attrs.iter_mut().find(|(n, _)| n == "mc:Ignorable") {
        Some((_, value)) => {
            if !value.split_whitespace().any(|p| p == prefix) {
                value.push(' ');
                value.push_str(prefix);
            }
        }
        None => attrs.push(("mc:Ignorable".into(), prefix.to_string())),
    }
}

/// Часть примечаний. У примечаний уже есть метки (см.
/// [`assign_para_ids`]); `root` — атрибуты корня из исходного файла.
pub(crate) fn comments_xml(comments: &[Comment], root: Option<&[(String, String)]>) -> String {
    let mut attrs = root_attributes(root);
    declare(&mut attrs, "w14", W14_NS, true);
    let mut content = String::new();
    for comment in comments {
        let mut extra = String::new();
        if let Some(date) = &comment.date {
            extra.push_str(&format!(r#" w:date="{}""#, escape_attr(date)));
        }
        if let Some(initials) = &comment.initials {
            extra.push_str(&format!(r#" w:initials="{}""#, escape_attr(initials)));
        }
        let mut inner = String::new();
        let last = comment.blocks.len().saturating_sub(1);
        for (i, block) in comment.blocks.iter().enumerate() {
            let xml = block_xml(block);
            match (&comment.para_id, block) {
                (Some(id), Block::Paragraph(_)) if i == last => {
                    let rest = xml.strip_prefix("<w:p").unwrap_or(&xml);
                    inner.push_str(&format!(r#"<w:p w14:paraId="{}""#, escape_attr(id)));
                    inner.push_str(rest);
                }
                _ => inner.push_str(&xml),
            }
        }
        content.push_str(&format!(
            r#"<w:comment w:id="{}" w:author="{}"{extra}>{inner}</w:comment>"#,
            escape_attr(&comment.id),
            escape_attr(&comment.author),
        ));
    }
    xml_part(&element_with("w:comments", &attrs, &content))
}

/// Часть ответов и отметок «решено» (`w15:commentsEx`).
pub(crate) fn comments_extended_xml(
    comments: &[Comment],
    root: Option<&[(String, String)]>,
) -> String {
    let mut attrs: Vec<(String, String)> = root.map(<[_]>::to_vec).unwrap_or_default();
    declare(&mut attrs, "w15", W15_NS, false);
    let mut content = String::new();
    for comment in comments {
        let Some(para) = &comment.para_id else {
            continue;
        };
        let parent = comment
            .parent
            .as_ref()
            .and_then(|p| comments.iter().find(|c| c.id == *p))
            .and_then(|c| c.para_id.as_ref())
            .map(|p| format!(r#" w15:paraIdParent="{}""#, escape_attr(p)))
            .unwrap_or_default();
        content.push_str(&format!(
            r#"<w15:commentEx w15:paraId="{}"{parent} w15:done="{}"/>"#,
            escape_attr(para),
            u8::from(comment.done)
        ));
    }
    xml_part(&element_with("w15:commentsEx", &attrs, &content))
}
