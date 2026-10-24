//! Гиперссылки. В .doc ссылка — поле `HYPERLINK "адрес" \l "закладка"`;
//! в модели, как в .docx, это ссылка у фрагментов текста результата
//! поля, а самого поля нет.

use letter_model::{FieldCharKind, Hyperlink, Inline, Run, RunContent, RunProps};

/// Кусок содержимого абзаца: знак фрагмента с его оформлением или
/// другой элемент (закладка, примечание).
#[derive(Clone)]
enum Item {
    Content(RunProps, RunContent),
    Other(Inline),
}

/// Превращает поля `HYPERLINK` абзаца в ссылки у фрагментов. Поле,
/// которое не закончилось в этом абзаце, остаётся полем.
pub(crate) fn hyperlinks(content: Vec<Inline>) -> Vec<Inline> {
    let has_link = content.iter().any(|i| {
        matches!(i, Inline::Run(r) if r.content.iter().any(
            |c| matches!(c, RunContent::FieldCode(code) if code.trim_start().starts_with("HYPERLINK"))
        ))
    });
    if !has_link {
        return content;
    }
    let mut items = Vec::new();
    for inline in content {
        match inline {
            Inline::Run(run) => {
                for item in run.content {
                    items.push(Item::Content(run.props.clone(), item));
                }
            }
            other => items.push(Item::Other(other)),
        }
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < items.len() {
        match link_field(&items[i..]) {
            Some((link, result, len)) => {
                for k in result {
                    let mut item = items[i + k].clone();
                    if let Item::Content(props, _) = &mut item {
                        props.link = Some(link.clone());
                    }
                    out.push(item);
                }
                i += len;
            }
            None => {
                out.push(items[i].clone());
                i += 1;
            }
        }
    }
    regroup(out)
}

/// Если `items` начинается с поля `HYPERLINK`, которое здесь же
/// кончается: ссылка, номера элементов результата и длина поля.
fn link_field(items: &[Item]) -> Option<(Hyperlink, std::ops::Range<usize>, usize)> {
    let kind = |item: &Item| match item {
        Item::Content(_, RunContent::FieldChar(c)) => Some(c.kind),
        _ => None,
    };
    if kind(items.first()?) != Some(FieldCharKind::Begin) {
        return None;
    }
    let mut code = String::new();
    let mut separate = None;
    let mut depth = 0;
    for (i, item) in items.iter().enumerate().skip(1) {
        match (kind(item), item) {
            (Some(FieldCharKind::Begin), _) => depth += 1,
            (Some(FieldCharKind::Separate), _) if depth == 0 => separate = Some(i),
            (Some(FieldCharKind::End), _) if depth > 0 => depth -= 1,
            (Some(FieldCharKind::End), _) => {
                let link = parse(&code)?;
                let start = separate.map_or(i, |s| s + 1);
                return Some((link, start..i, i + 1));
            }
            (_, Item::Content(_, RunContent::FieldCode(part)))
                if depth == 0 && separate.is_none() =>
            {
                code.push_str(part)
            }
            _ => {}
        }
    }
    None
}

/// Разбирает код поля `HYPERLINK`: адрес и ключи `\l` (закладка), `\o`
/// (подсказка), `\t` (окно). Не ссылка — `None`.
pub(crate) fn parse(code: &str) -> Option<Hyperlink> {
    let words = words(code);
    let (first, rest) = words.split_first()?;
    if !first.eq_ignore_ascii_case("HYPERLINK") {
        return None;
    }
    let mut link = Hyperlink {
        history: true,
        ..Hyperlink::default()
    };
    let mut rest = rest.iter();
    while let Some(word) = rest.next() {
        match word.as_str() {
            "\\l" => link.anchor = rest.next().cloned(),
            "\\o" => link.tooltip = rest.next().cloned(),
            "\\t" => link.frame = rest.next().cloned(),
            w if w.starts_with('\\') => {}
            w if link.target.is_none() => link.target = Some(w.to_string()),
            _ => {}
        }
    }
    (link.target.is_some() || link.anchor.is_some()).then_some(link)
}

/// Слова кода поля: в кавычках — целиком, без кавычек.
fn words(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = code.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '"' {
            chars.next();
            let mut word = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                word.push(c);
            }
            out.push(word);
        } else {
            let mut word = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() || c == '"' {
                    break;
                }
                word.push(c);
                chars.next();
            }
            out.push(word);
        }
    }
    out
}

/// Собирает соседние знаки с одинаковым оформлением во фрагменты.
fn regroup(items: Vec<Item>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::new();
    for item in items {
        match item {
            Item::Content(props, content) => {
                if let Some(Inline::Run(run)) = out.last_mut()
                    && run.props == props
                {
                    match (run.content.last_mut(), content) {
                        (Some(RunContent::Text(last)), RunContent::Text(more)) => {
                            last.push_str(&more)
                        }
                        (_, content) => run.content.push(content),
                    }
                    continue;
                }
                out.push(Inline::Run(Run {
                    props,
                    content: vec![content],
                }));
            }
            Item::Other(other) => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::FieldChar;

    fn run(content: Vec<RunContent>) -> Inline {
        Inline::Run(Run {
            props: RunProps::default(),
            content,
        })
    }

    fn field(kind: FieldCharKind) -> RunContent {
        RunContent::FieldChar(FieldChar::new(kind))
    }

    #[test]
    fn link_codes() {
        let link = parse(r#" HYPERLINK "https://example.com/" \o "Пример" "#).unwrap();
        assert_eq!(link.target.as_deref(), Some("https://example.com/"));
        assert_eq!(link.tooltip.as_deref(), Some("Пример"));
        let link = parse(r#"HYPERLINK \l "chapter""#).unwrap();
        assert_eq!(link.target, None);
        assert_eq!(link.anchor.as_deref(), Some("chapter"));
        assert!(parse("PAGE").is_none());
        assert!(parse("HYPERLINK").is_none());
    }

    #[test]
    fn field_becomes_linked_text() {
        let content = vec![
            run(vec![RunContent::Text("See ".into())]),
            run(vec![
                field(FieldCharKind::Begin),
                RunContent::FieldCode(r#"HYPERLINK "https://example.com/""#.into()),
                field(FieldCharKind::Separate),
            ]),
            run(vec![RunContent::Text("site".into())]),
            run(vec![
                field(FieldCharKind::End),
                RunContent::Text(".".into()),
            ]),
        ];
        let out = hyperlinks(content);
        assert_eq!(out.len(), 3);
        let Inline::Run(linked) = &out[1] else {
            panic!("нет фрагмента");
        };
        assert_eq!(linked.content, [RunContent::Text("site".into())]);
        assert_eq!(
            linked.props.link.as_ref().and_then(|l| l.target.as_deref()),
            Some("https://example.com/")
        );
        let Inline::Run(after) = &out[2] else {
            panic!("нет фрагмента");
        };
        assert_eq!(after.props.link, None);
        assert_eq!(after.content, [RunContent::Text(".".into())]);
    }

    #[test]
    fn unfinished_field_stays() {
        let content = vec![run(vec![
            field(FieldCharKind::Begin),
            RunContent::FieldCode("HYPERLINK \"x\"".into()),
            field(FieldCharKind::Separate),
            RunContent::Text("x".into()),
        ])];
        let out = hyperlinks(content.clone());
        assert_eq!(out, content);
    }
}
