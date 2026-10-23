//! Обычный текст: абзац на строку.

use encoding_rs::{Encoding, UTF_16BE, UTF_16LE, WINDOWS_1251, WINDOWS_1252};
use letter_model::{Block, BreakKind, Document, Paragraph, Run, RunContent};

use crate::builder::Builder;

/// Переводит байты файла в текст: метка BOM (UTF-8, UTF-16), затем
/// UTF-8 без метки, UTF-16 без метки (много нулевых байтов), иначе
/// однобайтная кодировка Windows: 1251 для русского текста, 1252 для
/// западноевропейского.
pub fn decode(data: &[u8]) -> String {
    if let Some((encoding, bom)) = Encoding::for_bom(data) {
        let (text, _) = encoding.decode_without_bom_handling(&data[bom..]);
        return text.into_owned();
    }
    if let Some(encoding) = utf16(data) {
        let (text, _) = encoding.decode_without_bom_handling(data);
        return text.into_owned();
    }
    if let Ok(text) = std::str::from_utf8(data) {
        return text.to_owned();
    }
    let (text, _) = guess(data).decode_without_bom_handling(data);
    text.into_owned()
}

/// UTF-16 без метки: каждый второй байт латиницы — ноль.
fn utf16(data: &[u8]) -> Option<&'static Encoding> {
    let zeros = |parity: usize| {
        data.iter()
            .skip(parity)
            .step_by(2)
            .filter(|&&b| b == 0)
            .count()
    };
    let half = data.len() / 2;
    if half < 2 {
        None
    } else if zeros(1) * 3 > half && zeros(0) * 3 <= half {
        Some(UTF_16LE)
    } else if zeros(0) * 3 > half && zeros(1) * 3 <= half {
        Some(UTF_16BE)
    } else {
        None
    }
}

fn guess(data: &[u8]) -> &'static Encoding {
    // Русские слова в Windows-1251 — сплошь байты выше 0xC0, а в
    // западноевропейском тексте такие байты стоят по одному среди латиницы.
    let mut paired = 0usize;
    let mut single = 0usize;
    for (i, &b) in data.iter().enumerate() {
        if b < 0xC0 {
            continue;
        }
        let near = |j: Option<usize>| j.and_then(|j| data.get(j)).is_some_and(|&c| c >= 0xC0);
        if near(i.checked_sub(1)) || near(Some(i + 1)) {
            paired += 1;
        } else {
            single += 1;
        }
    }
    if paired > single {
        WINDOWS_1251
    } else {
        WINDOWS_1252
    }
}

/// Читает текст: каждая строка — абзац. `\t` — табуляция, `\f` — разрыв
/// страницы. Перевод строки в самом конце файла нового абзаца не даёт.
pub fn read(data: &[u8]) -> Document {
    let text = decode(data);
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = text.strip_suffix('\n').unwrap_or(&text);
    let mut builder = Builder::new();
    let props = Default::default();
    for line in text.split('\n') {
        for (i, part) in line.split('\u{c}').enumerate() {
            if i > 0 {
                builder.content(RunContent::Break(BreakKind::Page), &props);
            }
            for (j, piece) in part.split('\t').enumerate() {
                if j > 0 {
                    builder.content(RunContent::Tab, &props);
                }
                builder.text(piece, &props);
            }
        }
        builder.empty_paragraph();
    }
    builder.finish()
}

/// Записывает текст: UTF-8 без метки, после каждого абзаца `\r\n`.
pub fn write(doc: &Document) -> Vec<u8> {
    let mut out = String::new();
    for block in doc.blocks() {
        write_block(&mut out, block);
    }
    if out == "\r\n" {
        // Документ из одного пустого абзаца — пустой файл.
        out.clear();
    }
    out.into_bytes()
}

fn write_block(out: &mut String, block: &Block) {
    match block {
        Block::Paragraph(p) => {
            out.push_str(&paragraph_text(p));
            out.push_str("\r\n");
        }
        Block::Table(table) => {
            for row in &table.rows {
                let cells: Vec<String> = row
                    .cells
                    .iter()
                    .map(|cell| {
                        let mut text = String::new();
                        for block in &cell.blocks {
                            write_block(&mut text, block);
                        }
                        // Ячейка — одна строка: абзацы через пробел.
                        text.split("\r\n")
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                            .join(" ")
                            .replace('\t', " ")
                    })
                    .collect();
                out.push_str(&cells.join("\t"));
                out.push_str("\r\n");
            }
        }
        Block::Unknown(_) => {}
    }
}

/// Текст абзаца: разрыв строки — `\r\n`, разрыв страницы и колонки — `\f`.
fn paragraph_text(p: &Paragraph) -> String {
    let mut out = String::new();
    for content in p.runs().flat_map(|r: &Run| &r.content) {
        match content {
            RunContent::Text(text) => out.push_str(text),
            RunContent::Symbol(s) => out.push(s.unicode()),
            RunContent::Tab => out.push('\t'),
            RunContent::Break(BreakKind::Line) => out.push_str("\r\n"),
            RunContent::Break(_) => out.push('\u{c}'),
            // Как в Word: рисунки в обычный текст не попадают.
            RunContent::Picture(_)
            | RunContent::FieldChar(_)
            | RunContent::FieldCode(_)
            | RunContent::NoteReference(_)
            | RunContent::NoteMark(_)
            | RunContent::CommentReference(_)
            | RunContent::Unknown(_) => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(doc: &Document) -> Vec<String> {
        doc.blocks().map(Block::plain_text).collect()
    }

    #[test]
    fn encodings() {
        assert_eq!(decode("Привет".as_bytes()), "Привет");
        assert_eq!(decode(b"\xef\xbb\xbfhi"), "hi");
        assert_eq!(decode(b"\xff\xfeh\0i\0"), "hi");
        assert_eq!(decode(b"\xfe\xff\0h\0i"), "hi");
        assert_eq!(decode(b"h\0e\0l\0l\0o\0"), "hello");
        assert_eq!(
            decode(b"\xcf\xf0\xe8\xe2\xe5\xf2, \xec\xe8\xf0"),
            "Привет, мир"
        );
        assert_eq!(decode(b"caf\xe9 cr\xe8me"), "café crème");
    }

    #[test]
    fn lines_become_paragraphs() {
        let doc = read(b"one\r\ntwo\tcols\n\nlast\x0cpage\n");
        assert_eq!(texts(&doc), ["one", "two\tcols", "", "last\u{c}page"]);
        assert_eq!(texts(&read(b"")), [""]);
        assert_eq!(texts(&read(b"\n")), [""]);
        assert_eq!(texts(&read(b"a\rb")), ["a", "b"]);
    }

    #[test]
    fn write_and_read_back() {
        let doc = read("Строка\tтаб\n\nещё\x0c".as_bytes());
        let bytes = write(&doc);
        assert_eq!(bytes, "Строка\tтаб\r\n\r\nещё\u{c}\r\n".as_bytes());
        assert_eq!(texts(&read(&bytes)), texts(&doc));
    }
}
