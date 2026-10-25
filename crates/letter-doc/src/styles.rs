//! Шрифты (`SttbfFfn`) и стили (`STSH`) документа.

use letter_model::{HalfPoints, ParagraphProps, RunProps, Style, StyleKind, Styles};

use crate::fib::u16_at;
use crate::sprm::{Names, apply_paragraph, apply_run, sprms};

/// Имена шрифтов по номерам.
pub(crate) fn fonts(sttb: &[u8]) -> Vec<String> {
    let Some(mut count) = u16_at(sttb, 0) else {
        return Vec::new();
    };
    let mut at = 4;
    if count == 0xFFFF {
        // Расширенная таблица: число строк дальше.
        count = u16_at(sttb, 2).unwrap_or(0);
        at = 6;
    }
    let mut out = Vec::new();
    for _ in 0..count {
        let Some(&size) = sttb.get(at) else {
            break;
        };
        let size = usize::from(size);
        let ffn = sttb.get(at + 1..at + 1 + size).unwrap_or(&[]);
        // Имя — после 39 байт описания, UTF-16 до нуля.
        let units: Vec<u16> = ffn
            .get(39..)
            .unwrap_or(&[])
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .take_while(|&u| u != 0)
            .collect();
        out.push(String::from_utf16_lossy(&units));
        at += 1 + size;
    }
    out
}

/// Имя и номер встроенного стиля (`sti`) в модели: как у Word в .docx.
fn builtin_id(sti: u16) -> Option<&'static str> {
    Some(match sti {
        0 => "Normal",
        1 => "Heading1",
        2 => "Heading2",
        3 => "Heading3",
        4 => "Heading4",
        5 => "Heading5",
        6 => "Heading6",
        7 => "Heading7",
        8 => "Heading8",
        9 => "Heading9",
        29 => "FootnoteText",
        30 => "CommentText",
        31 => "Header",
        32 => "Footer",
        38 => "FootnoteReference",
        39 => "CommentReference",
        62 => "Title",
        65 => "DefaultParagraphFont",
        74 => "Subtitle",
        85 => "Hyperlink",
        105 => "TableNormal",
        107 => "NoList",
        _ => return None,
    })
}

/// Описание стиля из STSH до разбора свойств.
struct RawStyle<'a> {
    sti: u16,
    kind: StyleKind,
    base: u16,
    next: u16,
    name: String,
    /// Свойства абзаца (у стилей абзацев) и знаков.
    papx: &'a [u8],
    chpx: &'a [u8],
}

/// Нет стиля (номер-заглушка).
const NIL: u16 = 0x0FFF;

/// Разбирает один стиль (`STD`).
fn raw_style(std: &[u8], base_size: usize) -> Option<RawStyle<'_>> {
    let sti = u16_at(std, 0)? & 0x0FFF;
    let second = u16_at(std, 2)?;
    let kind = match second & 0x000F {
        1 => StyleKind::Paragraph,
        2 => StyleKind::Character,
        3 => StyleKind::Table,
        4 => StyleKind::Numbering,
        _ => return None,
    };
    let base = second >> 4;
    let third = u16_at(std, 4)?;
    let upx_count = usize::from(third & 0x000F);
    let next = third >> 4;
    let mut at = base_size;
    let length = usize::from(u16_at(std, at)?);
    let units: Vec<u16> = (0..length)
        .filter_map(|i| u16_at(std, at + 2 + 2 * i))
        .collect();
    let name = String::from_utf16_lossy(&units);
    at += 2 + 2 * length + 2;
    let mut upx = Vec::new();
    for _ in 0..upx_count {
        if at % 2 == 1 {
            at += 1;
        }
        let size = usize::from(u16_at(std, at)?);
        upx.push(std.get(at + 2..at + 2 + size).unwrap_or(&[]));
        at += 2 + size;
    }
    let (papx, chpx) = match kind {
        // У стиля абзаца сначала свойства абзаца (с номером стиля), потом
        // знаков.
        StyleKind::Paragraph => (
            upx.first().and_then(|p| p.get(2..)).unwrap_or(&[]),
            upx.get(1).copied().unwrap_or(&[]),
        ),
        StyleKind::Character => (&[][..], upx.first().copied().unwrap_or(&[])),
        // У табличного стиля: свойства таблицы, абзаца, знаков.
        _ => (
            upx.get(1).and_then(|p| p.get(2..)).unwrap_or(&[]),
            upx.get(2).copied().unwrap_or(&[]),
        ),
    };
    Some(RawStyle {
        sti,
        kind,
        base,
        next,
        name,
        papx,
        chpx,
    })
}

/// Стили документа и номера стилей → их id в модели.
pub(crate) struct StyleSheet {
    pub styles: Styles,
    pub ids: Vec<Option<String>>,
}

/// Читает стили. `fonts` — имена шрифтов по номерам.
pub(crate) fn style_sheet(stsh: &[u8], fonts: &[String]) -> StyleSheet {
    let mut styles = Styles::default();
    // Word 97 по умолчанию: 10 пт.
    styles.defaults.run.size = Some(HalfPoints(20));
    let Some(header) = u16_at(stsh, 0).map(usize::from) else {
        return StyleSheet {
            styles,
            ids: Vec::new(),
        };
    };
    let info = stsh.get(2..2 + header).unwrap_or(&[]);
    let count = usize::from(u16_at(info, 0).unwrap_or(0));
    let base_size = usize::from(u16_at(info, 2).unwrap_or(10));
    // Шрифт по умолчанию — первый из rgftcStandardChpStsh.
    if let Some(font) = u16_at(info, 12).and_then(|f| fonts.get(usize::from(f))) {
        styles.defaults.run.fonts.ascii = Some(font.clone());
        styles.defaults.run.fonts.h_ansi = Some(font.clone());
    }
    let mut at = 2 + header;
    let mut raw = Vec::with_capacity(count);
    for _ in 0..count {
        let Some(size) = u16_at(stsh, at).map(usize::from) else {
            break;
        };
        let std = stsh.get(at + 2..at + 2 + size).unwrap_or(&[]);
        raw.push(if size == 0 {
            None
        } else {
            raw_style(std, base_size)
        });
        at += 2 + size;
    }
    // Id: у встроенных — как в Word, у своих — из имени.
    let mut ids: Vec<Option<String>> = Vec::with_capacity(raw.len());
    for style in &raw {
        let id = style.as_ref().map(|s| match builtin_id(s.sti) {
            Some(id) if s.sti != 4094 => id.to_string(),
            _ => custom_id(&s.name),
        });
        // Одинаковые id (например, свой стиль «Heading1») разводятся.
        let id = id.map(|id| {
            let mut unique = id.clone();
            let mut n = 2;
            while ids.iter().flatten().any(|other| *other == unique) {
                unique = format!("{id}{n}");
                n += 1;
            }
            unique
        });
        ids.push(id);
    }
    let names = Names {
        fonts,
        styles: &ids,
    };
    let id_of = |istd: u16| ids.get(usize::from(istd)).cloned().flatten();
    for (istd, style) in raw.iter().enumerate() {
        let (Some(style), Some(id)) = (style, ids[istd].clone()) else {
            continue;
        };
        let mut paragraph = ParagraphProps::default();
        apply_paragraph(&mut paragraph, &sprms(style.papx));
        let mut run = RunProps::default();
        apply_run(&mut run, &sprms(style.chpx), &names);
        styles.styles.push(Style {
            id,
            name: Some(english_name(style.sti).unwrap_or(&style.name).to_string()),
            kind: style.kind,
            is_default: matches!(style.sti, 0 | 65 | 105 | 107),
            based_on: (style.base != NIL).then(|| id_of(style.base)).flatten(),
            next: (style.next != NIL).then(|| id_of(style.next)).flatten(),
            paragraph,
            run,
            custom: builtin_id(style.sti).is_none(),
            ..Style::new("", style.kind)
        });
    }
    StyleSheet { styles, ids }
}

/// Английское имя встроенного стиля (в .doc имена бывают переведены).
fn english_name(sti: u16) -> Option<&'static str> {
    Some(match sti {
        0 => "Normal",
        1 => "heading 1",
        2 => "heading 2",
        3 => "heading 3",
        4 => "heading 4",
        5 => "heading 5",
        6 => "heading 6",
        7 => "heading 7",
        8 => "heading 8",
        9 => "heading 9",
        31 => "header",
        32 => "footer",
        62 => "Title",
        65 => "Default Paragraph Font",
        74 => "Subtitle",
        85 => "Hyperlink",
        105 => "Normal Table",
        107 => "No List",
        _ => return None,
    })
}

/// Id своего стиля из имени: слова слитно, каждое с большой буквы.
fn custom_id(name: &str) -> String {
    let id: String = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect();
    if id.is_empty() { "Style".into() } else { id }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_names_are_read() {
        let mut sttb = vec![2, 0, 0, 0];
        for name in ["Arial", "Times"] {
            let mut ffn = vec![0u8; 39];
            for u in name.encode_utf16().chain([0]) {
                ffn.extend(u.to_le_bytes());
            }
            sttb.push(ffn.len() as u8);
            sttb.extend(ffn);
        }
        assert_eq!(fonts(&sttb), ["Arial", "Times"]);
        assert!(fonts(&[]).is_empty());
    }

    #[test]
    fn custom_ids() {
        assert_eq!(custom_id("my style-1"), "MyStyle1");
        assert_eq!(custom_id("Мой стиль"), "МойСтиль");
        assert_eq!(custom_id("--"), "Style");
    }
}
