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

    /// Ключи вида `\* формат` по порядку, кроме `MERGEFORMAT` и
    /// `CHARFORMAT` (они про оформление результата).
    fn formats(&self) -> Vec<String> {
        let arguments = self.arguments();
        arguments
            .windows(2)
            .filter(|w| w[0] == "\\*")
            .map(|w| w[1].clone())
            .filter(|f| {
                !f.eq_ignore_ascii_case("MERGEFORMAT") && !f.eq_ignore_ascii_case("CHARFORMAT")
            })
            .collect()
    }

    /// Число `n` в виде, который задаёт ключ `\*`: `roman`, `ROMAN`,
    /// `alphabetic`, `ALPHABETIC`, `ArabicDash`; иначе арабскими цифрами.
    pub fn format_number(&self, n: usize) -> String {
        let mut out = n.to_string();
        for format in self.formats() {
            out = match format.as_str() {
                "roman" => roman(n).to_lowercase(),
                "ROMAN" | "Roman" => roman(n),
                "alphabetic" => alphabetic(n).to_lowercase(),
                "ALPHABETIC" | "Alphabetic" => alphabetic(n),
                "ArabicDash" => format!("- {n} -"),
                _ => out,
            };
        }
        self.format_text(&out)
    }

    /// Текст в виде, который задаёт ключ `\*`: `Upper` (прописные),
    /// `Lower` (строчные), `Caps` (каждое слово с прописной), `FirstCap`
    /// (первая буква прописная).
    pub fn format_text(&self, text: &str) -> String {
        let mut out = text.to_string();
        for format in self.formats() {
            out = match format.to_ascii_lowercase().as_str() {
                "upper" => out.to_uppercase(),
                "lower" => out.to_lowercase(),
                "caps" => out
                    .split(' ')
                    .map(capitalized)
                    .collect::<Vec<_>>()
                    .join(" "),
                "firstcap" => capitalized(&out),
                _ => out,
            };
        }
        out
    }
}

/// Слово с первой буквой прописной.
fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Римская запись числа (0 — пусто).
pub fn roman(mut n: usize) -> String {
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
pub fn alphabetic(n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    let letter = char::from(b'A' + ((n - 1) % 26) as u8);
    letter.to_string().repeat((n - 1) / 26 + 1)
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

    fn field(code: &str) -> Field {
        Field {
            code: code.into(),
            result: 0..0,
            depth: 0,
            dirty: false,
            locked: false,
        }
    }

    #[test]
    fn formats() {
        assert_eq!(field("PAGE \\* roman").format_number(14), "xiv");
        assert_eq!(
            field("PAGE \\* ROMAN \\* MERGEFORMAT").format_number(4),
            "IV"
        );
        assert_eq!(field("PAGE \\* ALPHABETIC").format_number(28), "BB");
        assert_eq!(field("PAGE \\* ArabicDash").format_number(2), "- 2 -");
        assert_eq!(field("PAGE").format_number(5), "5");
        assert_eq!(field("TITLE \\* Upper").format_text("Отчёт"), "ОТЧЁТ");
        assert_eq!(
            field("TITLE \\* Caps").format_text("годовой отчёт"),
            "Годовой Отчёт"
        );
        assert_eq!(
            field("TITLE \\* FirstCap").format_text("годовой отчёт"),
            "Годовой отчёт"
        );
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
