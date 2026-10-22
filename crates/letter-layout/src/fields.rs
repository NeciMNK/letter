//! Номера страниц в колонтитулах: поля PAGE (номер страницы) и NUMPAGES
//! (число страниц). При вёрстке колонтитула результат таких полей
//! заменяется номером этой страницы. Остальные поля показывают свой
//! сохранённый результат.

use letter_model::{Block, Paragraph, fields, set_field_result};

/// Блоки `blocks` с номером страницы `page` и числом страниц `pages`
/// в полях PAGE и NUMPAGES; `None`, если таких полей нет.
pub(crate) fn with_page_numbers(blocks: &[Block], page: usize, pages: usize) -> Option<Vec<Block>> {
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
            .any(|f| field_value(&f.code, 1, 1).is_some()),
        Block::Table(t) => t
            .rows
            .iter()
            .flat_map(|r| &r.cells)
            .flat_map(|c| &c.blocks)
            .any(has_page_fields),
        Block::Unknown(_) => false,
    }
}

fn substitute(block: &mut Block, page: usize, pages: usize) {
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

fn substitute_paragraph(paragraph: &mut Paragraph, page: usize, pages: usize) {
    for (index, field) in fields(paragraph).iter().enumerate() {
        if let Some(value) = field_value(&field.code, page, pages) {
            set_field_result(paragraph, index, &value);
        }
    }
}

/// Значение поля с кодом `code`: номер страницы или число страниц
/// в нужном виде (`\* roman` и т. п.); `None` — другое поле.
fn field_value(code: &str, page: usize, pages: usize) -> Option<String> {
    let mut words = code.split_whitespace();
    let number = match words.next()?.to_ascii_uppercase().as_str() {
        "PAGE" => page,
        "NUMPAGES" => pages,
        _ => return None,
    };
    let mut format = None;
    while let Some(word) = words.next() {
        if word == "\\*" {
            format = words.next();
        }
    }
    Some(match format {
        Some("roman") => roman(number).to_lowercase(),
        Some("ROMAN") => roman(number),
        Some("alphabetic") => alphabetic(number).to_lowercase(),
        Some("ALPHABETIC") => alphabetic(number),
        _ => number.to_string(),
    })
}

/// Римская запись числа (0 — пусто).
fn roman(mut n: usize) -> String {
    const DIGITS: [(usize, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (value, digit) in DIGITS {
        while n >= value {
            out.push_str(digit);
            n -= value;
        }
    }
    out
}

/// Буквенная запись, как в Word: A…Z, потом AA…ZZ, AAA…
fn alphabetic(n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    let letter = char::from(b'A' + ((n - 1) % 26) as u8);
    letter.to_string().repeat((n - 1) / 26 + 1)
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
        let filled = with_page_numbers(&blocks, 3, 12).unwrap();
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
        assert_eq!(with_page_numbers(&blocks, 2, 2), None);
    }

    #[test]
    fn number_formats() {
        assert_eq!(field_value("PAGE \\* roman", 14, 1).as_deref(), Some("xiv"));
        assert_eq!(field_value("PAGE \\* ROMAN", 4, 1).as_deref(), Some("IV"));
        assert_eq!(
            field_value("PAGE \\* ALPHABETIC", 28, 1).as_deref(),
            Some("BB")
        );
        assert_eq!(field_value("page", 5, 1).as_deref(), Some("5"));
        assert_eq!(field_value("REF x", 5, 1), None);
    }
}
