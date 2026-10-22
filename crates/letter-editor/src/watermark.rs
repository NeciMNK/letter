//! «Подложка»: надпись за текстом каждой страницы. Как в Word, она
//! ставится во все верхние колонтитулы документа; у документа без
//! колонтитула появляется пустой верхний колонтитул с подложкой.

use letter_model::{
    Block, Document, HeaderFooter, HeaderFooterKind, Inline, Paragraph, Run, RunContent, Watermark,
    is_watermark,
};

/// Ставит подложку с надписью `text` (прежняя убирается); `None` —
/// «Удалить подложку».
pub fn set_watermark(doc: &mut Document, text: Option<&str>) {
    let text = text.map(str::trim).filter(|t| !t.is_empty());
    if text.is_some()
        && let Some(first) = doc.sections.first_mut()
        && first.props.headers.default.is_none()
    {
        first.props.headers.default = Some(HeaderFooter {
            blocks: vec![Paragraph::default().into()],
            relationship: None,
        });
    }
    let mut number = 1;
    for section in &mut doc.sections {
        for kind in [
            HeaderFooterKind::Default,
            HeaderFooterKind::First,
            HeaderFooterKind::Even,
        ] {
            let Some(story) = section.props.headers.get_mut(kind) else {
                continue;
            };
            remove_watermarks(&mut story.blocks);
            if let Some(text) = text {
                let raw = Watermark::new(text).to_raw(number);
                number += 1;
                insert_run(&mut story.blocks, raw);
            }
        }
    }
}

/// Убирает подложки из абзацев колонтитула.
fn remove_watermarks(blocks: &mut [Block]) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                paragraph.content.retain_mut(|inline| match inline {
                    Inline::Run(run) => {
                        let before = run.content.len();
                        run.content.retain(
                            |item| !matches!(item, RunContent::Unknown(raw) if is_watermark(raw)),
                        );
                        // Фрагмент, в котором была только подложка, уходит.
                        !(run.content.is_empty() && before > 0)
                    }
                    _ => true,
                });
            }
            Block::Table(table) => {
                for cell in table.rows.iter_mut().flat_map(|r| &mut r.cells) {
                    remove_watermarks(&mut cell.blocks);
                }
            }
            Block::Unknown(_) => {}
        }
    }
}

/// Ставит фигуру подложки в начало первого абзаца колонтитула.
fn insert_run(blocks: &mut Vec<Block>, raw: letter_model::RawXml) {
    let run = Inline::Run(Run {
        props: Default::default(),
        content: vec![RunContent::Unknown(raw)],
    });
    match blocks.iter_mut().find_map(|b| match b {
        Block::Paragraph(p) => Some(p),
        _ => None,
    }) {
        Some(paragraph) => paragraph.content.insert(0, run),
        None => {
            let mut paragraph = Paragraph::default();
            paragraph.content.push(run);
            blocks.insert(0, paragraph.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::{Section, find_watermark};

    fn watermarks(doc: &Document) -> Vec<Option<String>> {
        doc.sections
            .iter()
            .flat_map(|s| {
                [&s.props.headers.default, &s.props.headers.first]
                    .map(|h| h.as_ref().and_then(|h| find_watermark(&h.blocks)))
            })
            .map(|m| m.map(|m| m.text))
            .collect()
    }

    #[test]
    fn watermark_goes_to_every_header() {
        let mut doc = Document::new();
        set_watermark(&mut doc, Some("ОБРАЗЕЦ"));
        // У документа без колонтитулов появился верхний.
        assert_eq!(watermarks(&doc), [Some("ОБРАЗЕЦ".to_string()), None]);

        // Второй раздел со своими колонтитулами: обычным и первой страницы.
        let mut second = Section::default();
        second.props.headers.default = Some(HeaderFooter {
            blocks: vec![Paragraph::text("Шапка").into()],
            relationship: None,
        });
        second.props.headers.first = Some(HeaderFooter::default());
        doc.sections.push(second);
        set_watermark(&mut doc, Some("ЧЕРНОВИК"));
        let draft = Some("ЧЕРНОВИК".to_string());
        assert_eq!(
            watermarks(&doc),
            [draft.clone(), None, draft.clone(), draft.clone()]
        );
        // Текст колонтитула остался, подложка одна.
        let header = doc.sections[1]
            .props
            .headers
            .default
            .clone()
            .unwrap_or_default();
        let Some(Block::Paragraph(p)) = header.blocks.first() else {
            panic!("нет абзаца");
        };
        assert_eq!(p.plain_text(), "Шапка");
        assert_eq!(p.content.len(), 2);

        set_watermark(&mut doc, None);
        assert_eq!(watermarks(&doc), [None, None, None, None]);
        let header = doc.sections[1]
            .props
            .headers
            .default
            .clone()
            .unwrap_or_default();
        let Some(Block::Paragraph(p)) = header.blocks.first() else {
            panic!("нет абзаца");
        };
        assert_eq!(p.content.len(), 1);
    }
}
