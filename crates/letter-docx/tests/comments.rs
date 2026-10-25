//! Примечания: чтение, «открыть → сохранить → открыть», новые примечания.

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Cursor, Read};
use std::path::PathBuf;

use letter_docx::{read_bytes, write_bytes};
use letter_model::{Comment, Document, Inline, Paragraph, Run, RunContent};

fn fixture() -> Document {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/docx/comments.docx");
    letter_docx::read_file(path).unwrap()
}

/// Часть архива как текст.
fn part(bytes: &[u8], name: &str) -> Option<String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut file = archive.by_name(name).ok()?;
    let mut text = String::new();
    file.read_to_string(&mut text).unwrap();
    Some(text)
}

#[test]
fn comments_replies_and_done_are_read() {
    let doc = fixture();
    let comments = &doc.comments;
    assert_eq!(comments.len(), 3);
    assert_eq!(comments[0].author, "Анна");
    assert_eq!(comments[0].initials.as_deref(), Some("А"));
    assert_eq!(comments[0].date.as_deref(), Some("2026-10-25T10:00:00Z"));
    assert!(comments[0].plain_text().ends_with("Where is the source?"));
    assert_eq!(comments[0].parent, None);
    assert!(!comments[0].done);
    assert!(comments[1].done);
    assert_eq!(comments[2].parent.as_deref(), Some("0"));

    // Кусок текста и значок в абзаце.
    let Some(letter_model::Block::Paragraph(p)) = doc.blocks().next() else {
        panic!("нет абзаца");
    };
    assert!(p.content.contains(&Inline::CommentStart("0".into())));
    assert!(p.content.contains(&Inline::CommentEnd("1".into())));
    let references: Vec<_> = p
        .runs()
        .flat_map(|r| &r.content)
        .filter_map(|c| match c {
            RunContent::CommentReference(id) => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(references, ["2", "0", "1"]);
    // Значки не занимают места в тексте.
    assert_eq!(
        p.plain_text(),
        "The first claim needs a source. Second remark."
    );
}

#[test]
fn comments_survive_round_trip() {
    let doc = fixture();
    let bytes = write_bytes(&doc).unwrap();
    let again = read_bytes(&bytes).unwrap();
    assert_eq!(again.comments, doc.comments);
    assert_eq!(again.sections, doc.sections);
    // Номера абзацев остались прежними: ответы связаны по ним.
    let extended = part(&bytes, "word/commentsExtended.xml").unwrap();
    assert!(extended.contains(r#"w15:paraId="1A2B3C03" w15:paraIdParent="1A2B3C01""#));
}

/// Абзац «текст» с примечанием `id` на весь текст.
fn commented(text: &str, id: &str) -> Paragraph {
    let mut p = Paragraph::default();
    p.content.push(Inline::CommentStart(id.into()));
    p.content.push(Run::text(text).into());
    p.content.push(Inline::CommentEnd(id.into()));
    p.content.push(
        Run {
            props: Default::default(),
            content: vec![RunContent::CommentReference(id.into())],
        }
        .into(),
    );
    p
}

#[test]
fn new_comments_are_written() {
    let mut doc = Document::new();
    doc.sections[0].blocks = vec![commented("Текст", "5").into(), commented("Ещё", "6").into()];
    doc.comments = vec![
        Comment {
            id: "5".into(),
            author: "Никита".into(),
            initials: Some("Н".into()),
            date: Some("2026-10-25T12:00:00Z".into()),
            blocks: vec![Paragraph::text("Проверить").into()],
            ..Comment::default()
        },
        Comment {
            id: "6".into(),
            author: "Никита".into(),
            blocks: vec![Paragraph::text("Ответ").into()],
            parent: Some("5".into()),
            done: true,
            ..Comment::default()
        },
        // На это примечание в тексте ссылки нет: в файл не идёт.
        Comment {
            id: "7".into(),
            author: "Никита".into(),
            ..Comment::default()
        },
    ];
    let bytes = write_bytes(&doc).unwrap();
    assert!(part(&bytes, "word/comments.xml").is_some());
    assert!(part(&bytes, "word/commentsExtended.xml").is_some());
    let types = part(&bytes, "[Content_Types].xml").unwrap();
    assert!(types.contains("comments+xml") && types.contains("commentsExtended+xml"));

    let again = read_bytes(&bytes).unwrap();
    assert_eq!(again.comments.len(), 2);
    assert_eq!(again.comments[0].author, "Никита");
    assert_eq!(again.comments[0].plain_text(), "Проверить");
    assert_eq!(again.comments[1].parent.as_deref(), Some("5"));
    assert!(again.comments[1].done);
    assert!(again.comments.iter().all(|c| c.para_id.is_some()));
    assert_eq!(again.sections, doc.sections);
}

#[test]
fn document_without_comments_gets_no_comment_parts() {
    let bytes = write_bytes(&Document::new()).unwrap();
    assert!(part(&bytes, "word/comments.xml").is_none());
    assert!(part(&bytes, "word/commentsExtended.xml").is_none());
}
