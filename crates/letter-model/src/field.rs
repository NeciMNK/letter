//! Поля Word: номер страницы, дата, оглавление, ссылки и т. п.
//!
//! Поле записано в фрагментах абзаца по порядку: знак начала
//! ([`FieldCharKind::Begin`]), код ([`RunContent::FieldCode`], может быть
//! разбит на куски), разделитель ([`FieldCharKind::Separate`]), результат —
//! обычный текст, который видно в документе, — и знак конца
//! ([`FieldCharKind::End`]). Поля бывают вложенными: внутри кода или
//! результата одного поля может быть другое.

use std::ops::Range;

use crate::{FieldChar, FieldCharKind, Inline, Paragraph, Run, RunContent, RunProps};

/// Поле абзаца.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// Код поля: куски кода этого поля подряд (без кода вложенных полей),
    /// например « DATE \@ "dd.MM.yyyy" ».
    pub code: String,
    /// Где результат в тексте абзаца, байты. У поля без разделителя —
    /// пусто, на месте конца.
    pub result: Range<usize>,
    /// Вложенность: 0 — поле верхнего уровня.
    pub depth: usize,
    /// Знак начала: поле нужно обновить.
    pub dirty: bool,
    /// Знак начала: поле не обновляется.
    pub locked: bool,
}

impl Field {
    /// Имя поля прописными: первое слово кода (`PAGE`, `DATE`, `TOC`).
    pub fn name(&self) -> String {
        self.code
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_uppercase()
    }

    /// Слова кода после имени с учётом кавычек: `DATE \@ "d MMMM yyyy"`
    /// даёт `["\@", "d MMMM yyyy"]`.
    pub fn arguments(&self) -> Vec<String> {
        let mut words = Vec::new();
        let mut current = String::new();
        let mut quoted = false;
        let mut has_word = false;
        for c in self.code.chars() {
            match c {
                '"' => {
                    quoted = !quoted;
                    has_word = true;
                }
                c if c.is_whitespace() && !quoted => {
                    if has_word {
                        words.push(std::mem::take(&mut current));
                        has_word = false;
                    }
                }
                c => {
                    current.push(c);
                    has_word = true;
                }
            }
        }
        if has_word {
            words.push(current);
        }
        words.into_iter().skip(1).collect()
    }

    /// Значение ключа `switch` (`\@`, `\*`, `\#`): следующее за ним слово.
    pub fn switch(&self, switch: &str) -> Option<String> {
        let arguments = self.arguments();
        let at = arguments.iter().position(|a| a == switch)?;
        arguments.get(at + 1).cloned()
    }
}

/// Поля абзаца по порядку их начала (вложенные — после внешних).
pub fn fields(paragraph: &Paragraph) -> Vec<Field> {
    let mut done = Vec::new();
    // Открытые поля: поле и прошёл ли разделитель.
    let mut open: Vec<(Field, bool, usize)> = Vec::new();
    let mut offset = 0;
    let mut started = 0;
    for inline in &paragraph.content {
        let Inline::Run(run) = inline else {
            continue;
        };
        for item in &run.content {
            match item {
                RunContent::FieldChar(c) => match c.kind {
                    FieldCharKind::Begin => {
                        let field = Field {
                            code: String::new(),
                            result: offset..offset,
                            depth: open.len(),
                            dirty: c.dirty,
                            locked: c.locked,
                        };
                        open.push((field, false, started));
                        started += 1;
                    }
                    FieldCharKind::Separate => {
                        if let Some((field, separated, _)) = open.last_mut() {
                            *separated = true;
                            field.result = offset..offset;
                        }
                    }
                    FieldCharKind::End => {
                        if let Some((mut field, separated, order)) = open.pop() {
                            if separated {
                                field.result.end = offset;
                            } else {
                                field.result = offset..offset;
                            }
                            done.push((order, field));
                        }
                    }
                },
                RunContent::FieldCode(code) => {
                    if let Some((field, false, _)) = open.last_mut() {
                        field.code.push_str(code);
                    }
                }
                other => offset += other.text_len(),
            }
        }
    }
    done.sort_by_key(|(order, _)| *order);
    done.into_iter().map(|(_, field)| field).collect()
}

/// Заменяет результат поля номер `index` (по порядку из [`fields`])
/// текстом `text`. Новый текст получает оформление первого куска
/// старого результата (или знака начала поля). Поле без разделителя
/// его получает. `false`, если такого поля нет.
pub fn set_field_result(paragraph: &mut Paragraph, index: usize, text: &str) -> bool {
    // Кусок абзаца: непонятый элемент или элемент фрагмента с его
    // оформлением.
    enum Piece {
        Other(Inline),
        Item(RunProps, RunContent),
    }
    let mut pieces = Vec::new();
    for inline in std::mem::take(&mut paragraph.content) {
        match inline {
            Inline::Run(run) => {
                pieces.extend(
                    run.content
                        .into_iter()
                        .map(|item| Piece::Item(run.props.clone(), item)),
                );
            }
            other => pieces.push(Piece::Other(other)),
        }
    }
    // Где начало, разделитель и конец нужного поля.
    let mut begins = 0;
    let mut stack: Vec<(usize, usize, Option<usize>)> = Vec::new();
    let mut found = None;
    for (k, piece) in pieces.iter().enumerate() {
        let Piece::Item(_, RunContent::FieldChar(c)) = piece else {
            continue;
        };
        match c.kind {
            FieldCharKind::Begin => {
                stack.push((begins, k, None));
                begins += 1;
            }
            FieldCharKind::Separate => {
                if let Some(top) = stack.last_mut()
                    && top.2.is_none()
                {
                    top.2 = Some(k);
                }
            }
            FieldCharKind::End => {
                if let Some((order, begin, separate)) = stack.pop()
                    && order == index
                {
                    found = Some((begin, separate, k));
                }
            }
        }
    }
    let changed = if let Some((begin, separate, end)) = found {
        let props = |k: usize| match &pieces[k] {
            Piece::Item(props, _) => props.clone(),
            Piece::Other(_) => RunProps::default(),
        };
        let style = separate
            .and_then(|s| {
                (s + 1..end).find(|&k| matches!(&pieces[k], Piece::Item(_, RunContent::Text(_))))
            })
            .map_or_else(|| props(begin), props);
        let new = Piece::Item(style.clone(), RunContent::Text(text.to_string()));
        match separate {
            Some(separate) => {
                pieces.splice(separate + 1..end, [new]);
            }
            None => {
                let mark = Piece::Item(
                    style,
                    RunContent::FieldChar(FieldChar::new(FieldCharKind::Separate)),
                );
                pieces.splice(end..end, [mark, new]);
            }
        }
        true
    } else {
        false
    };
    // Собираем фрагменты обратно: соседние куски с одним оформлением —
    // один фрагмент.
    for piece in pieces {
        match piece {
            Piece::Other(inline) => paragraph.content.push(inline),
            Piece::Item(props, item) => match paragraph.content.last_mut() {
                Some(Inline::Run(run)) if run.props == props => run.content.push(item),
                _ => paragraph.content.push(Inline::Run(Run {
                    props,
                    content: vec![item],
                })),
            },
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(kind: FieldCharKind) -> RunContent {
        RunContent::FieldChar(FieldChar::new(kind))
    }

    fn paragraph(items: Vec<RunContent>) -> Paragraph {
        Paragraph::from_runs(items.into_iter().map(|item| Run {
            content: vec![item],
            ..Run::default()
        }))
    }

    fn sample() -> Paragraph {
        use FieldCharKind::*;
        paragraph(vec![
            RunContent::Text("Date: ".into()),
            mark(Begin),
            RunContent::FieldCode(" DATE \\@ ".into()),
            RunContent::FieldCode("\"dd.MM.yyyy\" ".into()),
            mark(Separate),
            RunContent::Text("01.02.2026".into()),
            mark(End),
            RunContent::Text(", page ".into()),
            mark(Begin),
            RunContent::FieldCode("PAGE".into()),
            mark(End),
        ])
    }

    #[test]
    fn fields_are_found() {
        let p = sample();
        let found = fields(&p);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name(), "DATE");
        assert_eq!(found[0].switch("\\@").as_deref(), Some("dd.MM.yyyy"));
        assert_eq!(&p.plain_text()[found[0].result.clone()], "01.02.2026");
        assert_eq!(found[1].name(), "PAGE");
        assert!(found[1].result.is_empty());
    }

    #[test]
    fn results_are_replaced() {
        let mut p = sample();
        assert!(set_field_result(&mut p, 0, "22.10.2026"));
        assert!(set_field_result(&mut p, 1, "3"));
        assert_eq!(p.plain_text(), "Date: 22.10.2026, page 3");
        assert_eq!(fields(&p).len(), 2);
        assert!(!set_field_result(&mut p, 2, "x"));
    }

    #[test]
    fn nested_fields() {
        use FieldCharKind::*;
        let p = paragraph(vec![
            mark(Begin),
            RunContent::FieldCode("IF ".into()),
            mark(Begin),
            RunContent::FieldCode("PAGE".into()),
            mark(Separate),
            RunContent::Text("2".into()),
            mark(End),
            RunContent::FieldCode(" > 1 \"yes\"".into()),
            mark(Separate),
            RunContent::Text("yes".into()),
            mark(End),
        ]);
        let found = fields(&p);
        assert_eq!(found[0].name(), "IF");
        assert_eq!(found[0].depth, 0);
        assert_eq!(found[1].name(), "PAGE");
        assert_eq!(found[1].depth, 1);
        assert_eq!(&p.plain_text()[found[0].result.clone()], "yes");
    }
}
