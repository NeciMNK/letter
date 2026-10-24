//! Номера страниц в колонтитулах: поля PAGE (номер страницы) и NUMPAGES
//! (число страниц). При вёрстке колонтитула результат таких полей
//! заменяется номером этой страницы. Остальные поля показывают свой
//! сохранённый результат.

use letter_model::{Block, Field, NumberFormat, Paragraph, fields, set_field_result};

/// Номер страницы и вид номеров её раздела.
pub(crate) type PageNumber<'a> = (usize, Option<&'a NumberFormat>);

/// Блоки `blocks` с номером страницы `page` и числом страниц `pages`
/// в полях PAGE и NUMPAGES; `None`, если таких полей нет.
pub(crate) fn with_page_numbers(
    blocks: &[Block],
    page: PageNumber<'_>,
    pages: usize,
) -> Option<Vec<Block>> {
    if !blocks.iter().any(has_page_fields) {
        return None;
    }
    let mut blocks = blocks.to_vec();
    for block in &mut blocks {
        substitute(block, page, pages);
    }
    Some(blocks)
}

/// Есть ли в блоке поле PAGE или NUMPAGES.
fn has_page_fields(block: &Block) -> bool {
    match block {
        Block::Paragraph(p) => fields(p)
            .iter()
            .any(|f| field_value(f, (1, None), 1).is_some()),
        Block::Table(t) => t
            .rows
            .iter()
            .flat_map(|r| &r.cells)
            .flat_map(|c| &c.blocks)
            .any(has_page_fields),
        Block::Unknown(_) => false,
    }
}

fn substitute(block: &mut Block, page: PageNumber<'_>, pages: usize) {
    match block {
        Block::Paragraph(p) => substitute_paragraph(p, page, pages),
        Block::Table(t) => {
            for cell in t.rows.iter_mut().flat_map(|r| &mut r.cells) {
                for block in &mut cell.blocks {
                    substitute(block, page, pages);
                }
            }
        }
        Block::Unknown(_) => {}
    }
}

fn substitute_paragraph(paragraph: &mut Paragraph, page: PageNumber<'_>, pages: usize) {
    for (index, field) in fields(paragraph).iter().enumerate() {
        if let Some(value) = field_value(field, page, pages) {
            set_field_result(paragraph, index, &value);
        }
    }
}

/// Значение поля: номер страницы или число страниц в нужном виде
/// (`\* roman` и т. п.); `None` — другое поле.
fn field_value(field: &Field, page: PageNumber<'_>, pages: usize) -> Option<String> {
    match field.name().as_str() {
        "PAGE" => Some(field.format_page_number(page.0, page.1)),
        "NUMPAGES" => Some(field.format_number(pages)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::{Paragraph, Run, RunContent};

    use letter_model::{FieldChar, FieldCharKind};

    fn field_char(kind: &str) -> RunContent {
        let kind = match kind {
            "begin" => FieldCharKind::Begin,
            "separate" => FieldCharKind::Separate,
            _ => FieldCharKind::End,
        };
        RunContent::FieldChar(FieldChar::new(kind))
    }

    fn code(text: &str) -> RunContent {
        RunContent::FieldCode(text.into())
    }

    fn paragraph(items: Vec<RunContent>) -> Block {
        Paragraph::from_runs(items.into_iter().map(|item| Run {
            content: vec![item],
            ..Run::default()
        }))
        .into()
    }

    fn text(blocks: &[Block]) -> String {
        blocks.iter().map(Block::plain_text).collect()
    }

    #[test]
    fn page_numbers_replace_field_results() {
        let blocks = vec![paragraph(vec![
            RunContent::Text("Page ".into()),
            field_char("begin"),
            code(" PAGE  \\* MERGEFORMAT "),
            field_char("separate"),
            RunContent::Text("1".into()),
            RunContent::Text("0".into()),
            field_char("end"),
            RunContent::Text(" of ".into()),
            field_char("begin"),
            code("NUMPAGES"),
            field_char("end"),
        ])];
        let filled = with_page_numbers(&blocks, (3, None), 12).unwrap();
        assert_eq!(text(&filled), "Page 3 of 12");
    }

    #[test]
    fn other_fields_keep_their_result() {
        let blocks = vec![paragraph(vec![
            field_char("begin"),
            code("DATE"),
            field_char("separate"),
            RunContent::Text("01.01.2026".into()),
            field_char("end"),
        ])];
        assert_eq!(with_page_numbers(&blocks, (2, None), 2), None);
    }
}
