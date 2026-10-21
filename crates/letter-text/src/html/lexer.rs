//! Разбор HTML на теги и текст. Терпит любые ошибки разметки: всё
//! непонятное становится текстом или пропускается.

/// Элемент потока HTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// Открывающий тег: имя строчными буквами и атрибуты (имена тоже
    /// строчными, значения с раскрытыми ссылками `&...;`).
    Start {
        /// Имя тега.
        name: String,
        /// Атрибуты по порядку.
        attrs: Vec<(String, String)>,
    },
    /// Закрывающий тег.
    End(String),
    /// Текст с раскрытыми ссылками `&...;`.
    Text(String),
}

/// Элементы, чьё содержимое — простой текст до закрывающего тега.
const RAW_TEXT: [&str; 6] = ["script", "style", "title", "textarea", "xmp", "noscript"];

/// Разбирает весь документ.
pub fn tokens(html: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut rest = html;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            push_text(&mut out, rest);
            break;
        };
        push_text(&mut out, &rest[..lt]);
        rest = &rest[lt..];
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.find("-->").map_or("", |end| &after[end + 3..]);
        } else if rest.starts_with("<!") || rest.starts_with("<?") {
            // DOCTYPE, CDATA, инструкции обработки.
            rest = rest.find('>').map_or("", |end| &rest[end + 1..]);
        } else if let Some(after) = rest.strip_prefix("</") {
            let name_len = name_length(after);
            if name_len == 0 {
                rest = after.find('>').map_or("", |end| &after[end + 1..]);
                continue;
            }
            out.push(Token::End(after[..name_len].to_ascii_lowercase()));
            rest = after.find('>').map_or("", |end| &after[end + 1..]);
        } else {
            let after = &rest[1..];
            let name_len = name_length(after);
            if name_len == 0 {
                // `<` без имени — просто знак.
                push_text(&mut out, "<");
                rest = after;
                continue;
            }
            let name = after[..name_len].to_ascii_lowercase();
            let (attrs, consumed) = attributes(&after[name_len..]);
            rest = &after[name_len + consumed..];
            let raw = RAW_TEXT.contains(&name.as_str());
            out.push(Token::Start {
                name: name.clone(),
                attrs,
            });
            if raw {
                let end = find_end_tag(rest, &name);
                let text = &rest[..end];
                if name == "title" || name == "textarea" {
                    push_text(&mut out, text);
                } else {
                    out.push(Token::Text(text.to_owned()));
                }
                rest = &rest[end..];
            }
        }
    }
    out
}

fn push_text(out: &mut Vec<Token>, text: &str) {
    if text.is_empty() {
        return;
    }
    let text = decode_entities(text);
    match out.last_mut() {
        Some(Token::Text(previous)) => previous.push_str(&text),
        _ => out.push(Token::Text(text)),
    }
}

fn name_length(s: &str) -> usize {
    if !s.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return 0;
    }
    s.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == ':' || c == '_'))
        .unwrap_or(s.len())
}

/// Место закрывающего тега `</name` (без учёта регистра) или конец текста.
fn find_end_tag(s: &str, name: &str) -> usize {
    let lower = s.to_ascii_lowercase();
    let pattern = format!("</{name}");
    lower.find(&pattern).unwrap_or(s.len())
}

/// Атрибуты до `>`; второе значение — сколько байт прочитано (с `>`).
fn attributes(s: &str) -> (Vec<(String, String)>, usize) {
    let bytes = s.as_bytes();
    let mut attrs = Vec::new();
    let mut i = 0;
    loop {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/') {
            i += 1;
        }
        if i >= bytes.len() {
            return (attrs, i);
        }
        if bytes[i] == b'>' {
            return (attrs, i + 1);
        }
        let start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && !matches!(bytes[i], b'=' | b'>' | b'/')
        {
            i += 1;
        }
        let name = s[start..i].to_ascii_lowercase();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if bytes.get(i) == Some(&b'=') {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            match bytes.get(i) {
                Some(&quote @ (b'"' | b'\'')) => {
                    let begin = i + 1;
                    let end = s[begin..]
                        .find(char::from(quote))
                        .map_or(s.len(), |e| begin + e);
                    value = decode_entities(&s[begin..end]);
                    i = (end + 1).min(s.len());
                }
                _ => {
                    let begin = i;
                    while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                        i += 1;
                    }
                    value = decode_entities(&s[begin..i]);
                }
            }
        }
        if !name.is_empty() {
            attrs.push((name, value));
        } else if i == start {
            // Ничего не продвинулось (одинокий `=`): пропускаем знак.
            i += 1;
        }
    }
}

/// Раскрывает ссылки на знаки: `&amp;`, `&#1046;`, `&#x416;`.
pub fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let end = rest[1..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '#'))
            .map_or(rest.len(), |e| e + 1);
        let name = &rest[1..end];
        let decoded = if let Some(number) = name.strip_prefix('#') {
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => number.parse::<u32>().ok(),
            };
            code.map(|c| match c {
                // Байты Windows-1252, которые часто пишут как номера.
                0x80..=0x9F => windows_1252(c),
                _ => char::from_u32(c).unwrap_or(char::REPLACEMENT_CHARACTER),
            })
        } else {
            named(name)
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end..];
                if rest.starts_with(';') {
                    rest = &rest[1..];
                }
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn windows_1252(code: u32) -> char {
    let byte = [u8::try_from(code).unwrap_or(b'?')];
    let (text, _) = encoding_rs::WINDOWS_1252.decode_without_bom_handling(&byte);
    text.chars().next().unwrap_or(char::REPLACEMENT_CHARACTER)
}

fn named(name: &str) -> Option<char> {
    let c = match name {
        "amp" | "AMP" => '&',
        "lt" | "LT" => '<',
        "gt" | "GT" => '>',
        "quot" | "QUOT" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "shy" => '\u{ad}',
        "ensp" => '\u{2002}',
        "emsp" => '\u{2003}',
        "thinsp" => '\u{2009}',
        "zwnj" => '\u{200c}',
        "zwj" => '\u{200d}',
        "ndash" => '–',
        "mdash" => '—',
        "lsquo" => '‘',
        "rsquo" => '’',
        "sbquo" => '‚',
        "ldquo" => '“',
        "rdquo" => '”',
        "bdquo" => '„',
        "laquo" => '«',
        "raquo" => '»',
        "hellip" => '…',
        "bull" => '•',
        "middot" => '·',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "deg" => '°',
        "plusmn" => '±',
        "times" => '×',
        "divide" => '÷',
        "minus" => '−',
        "micro" => 'µ',
        "para" => '¶',
        "sect" => '§',
        "euro" => '€',
        "pound" => '£',
        "yen" => '¥',
        "cent" => '¢',
        "iexcl" => '¡',
        "iquest" => '¿',
        "frac12" => '½',
        "frac14" => '¼',
        "frac34" => '¾',
        "sup2" => '²',
        "sup3" => '³',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "ne" => '≠',
        "le" => '≤',
        "ge" => '≥',
        "infin" => '∞',
        "agrave" => 'à',
        "aacute" => 'á',
        "acirc" => 'â',
        "auml" => 'ä',
        "ccedil" => 'ç',
        "egrave" => 'è',
        "eacute" => 'é',
        "ecirc" => 'ê',
        "euml" => 'ë',
        "iacute" => 'í',
        "ntilde" => 'ñ',
        "oacute" => 'ó',
        "ouml" => 'ö',
        "uacute" => 'ú',
        "uuml" => 'ü',
        "szlig" => 'ß',
        "Auml" => 'Ä',
        "Ouml" => 'Ö',
        "Uuml" => 'Ü',
        "Eacute" => 'É',
        _ => return None,
    };
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(name: &str, attrs: &[(&str, &str)]) -> Token {
        Token::Start {
            name: name.into(),
            attrs: attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    #[test]
    fn tags_attributes_and_text() {
        assert_eq!(
            tokens("<!DOCTYPE html><P Class=x style='a:b' hidden>Hi &amp; bye<br/></p><!-- c -->!"),
            vec![
                start("p", &[("class", "x"), ("style", "a:b"), ("hidden", "")]),
                Token::Text("Hi & bye".into()),
                start("br", &[]),
                Token::End("p".into()),
                Token::Text("!".into()),
            ]
        );
    }

    #[test]
    fn raw_text_elements() {
        assert_eq!(
            tokens("<script>if (a<b) x();</script><title>A &amp; B</title>"),
            vec![
                start("script", &[]),
                Token::Text("if (a<b) x();".into()),
                Token::End("script".into()),
                start("title", &[]),
                Token::Text("A & B".into()),
                Token::End("title".into()),
            ]
        );
    }

    #[test]
    fn broken_markup() {
        assert_eq!(tokens("a < b"), vec![Token::Text("a < b".into())]);
        assert_eq!(tokens("<p"), vec![start("p", &[])]);
        assert_eq!(tokens("<!-- open"), vec![]);
        assert_eq!(tokens("x</>y"), vec![Token::Text("xy".into())]);
        assert_eq!(
            tokens("<a href=\"x>y"),
            vec![start("a", &[("href", "x>y")])]
        );
    }

    #[test]
    fn entities() {
        assert_eq!(
            decode_entities("&#1046;&#x416;&#150;&laquo;&nbsp"),
            "ЖЖ–«\u{a0}"
        );
        assert_eq!(
            decode_entities("a & b &unknown; &#xZZ;"),
            "a & b &unknown; &#xZZ;"
        );
        assert_eq!(decode_entities("&#99999999;"), "\u{fffd}");
    }
}
