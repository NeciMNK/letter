//! Рассылки (слияние): источник данных — таблица с заголовками (файл CSV
//! или первая таблица документа Word), в документе — поля
//! `MERGEFIELD имя` (значение столбца) и `MERGEREC` (номер записи).
//! «Просмотр результатов» ставит в поля значения записи, «Найти и
//! объединить» собирает документ из копий — по одной на запись.

use letter_model::{
    Block, Document, FieldChar, FieldCharKind, Inline, Paragraph, RunContent, SectionStart, fields,
    set_field_result,
};

use crate::edit::{
    clamp_offset, insert_items, paragraph_count, paragraph_mut, paragraph_ref, tidy,
};
use crate::position::Position;

/// Источник данных: заголовки столбцов и записи.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MergeData {
    /// Имена столбцов (имена полей).
    pub headers: Vec<String>,
    /// Записи: значения по столбцам.
    pub rows: Vec<Vec<String>>,
}

impl MergeData {
    /// Таблица из текста CSV: первая строка — заголовки. Разделитель —
    /// `;`, `,` или табуляция (какой чаще в первой строке); значения в
    /// кавычках могут содержать разделитель, перевод строки и `""`.
    pub fn from_csv(text: &str) -> MergeData {
        let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
        let first = text.lines().next().unwrap_or_default();
        let delimiter = [';', ',', '\t']
            .into_iter()
            .max_by_key(|d| count_outside_quotes(first, *d))
            .unwrap_or(',');
        let mut records = parse_csv(text, delimiter).into_iter();
        let headers: Vec<String> = records
            .next()
            .unwrap_or_default()
            .into_iter()
            .map(|h| h.trim().to_string())
            .collect();
        let rows = records
            .filter(|r| r.iter().any(|v| !v.trim().is_empty()))
            .collect();
        MergeData { headers, rows }
    }

    /// Как [`MergeData::from_csv`], но из байтов файла: UTF-8 (с меткой
    /// или без), а если это не UTF-8 — Windows-1251 (так сохраняет CSV
    /// русский Excel).
    pub fn from_csv_bytes(bytes: &[u8]) -> MergeData {
        match std::str::from_utf8(bytes) {
            Ok(text) => MergeData::from_csv(text),
            Err(_) => {
                let (text, _, _) = encoding_rs::WINDOWS_1251.decode(bytes);
                MergeData::from_csv(&text)
            }
        }
    }

    /// Таблица из первой таблицы документа: первая строка — заголовки.
    /// `None`, если таблиц нет.
    pub fn from_document(doc: &Document) -> Option<MergeData> {
        let table = doc.blocks().find_map(|b| match b {
            Block::Table(t) => Some(t),
            _ => None,
        })?;
        let mut rows = table.rows.iter().map(|row| {
            row.cells
                .iter()
                .map(|cell| {
                    let texts: Vec<String> = cell.blocks.iter().map(Block::plain_text).collect();
                    texts.join("\n").trim().to_string()
                })
                .collect::<Vec<String>>()
        });
        let headers = rows.next()?;
        Some(MergeData {
            headers,
            rows: rows.collect(),
        })
    }

    /// Значение поля `name` (без учёта регистра) в записи `row`.
    pub fn value(&self, row: usize, name: &str) -> Option<&str> {
        let column = self
            .headers
            .iter()
            .position(|h| h.to_lowercase() == name.to_lowercase())?;
        let record = self.rows.get(row)?;
        Some(record.get(column).map_or("", String::as_str))
    }
}

/// Сколько раз знак `c` встречается в строке вне кавычек.
fn count_outside_quotes(line: &str, c: char) -> usize {
    let mut quoted = false;
    let mut count = 0;
    for ch in line.chars() {
        if ch == '"' {
            quoted = !quoted;
        } else if ch == c && !quoted {
            count += 1;
        }
    }
    count
}

/// Записи CSV с разделителем `delimiter`.
fn parse_csv(text: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut value = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                value.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == delimiter && !quoted => record.push(std::mem::take(&mut value)),
            '\r' if !quoted => {}
            '\n' if !quoted => {
                record.push(std::mem::take(&mut value));
                records.push(std::mem::take(&mut record));
            }
            c => value.push(c),
        }
    }
    if !value.is_empty() || !record.is_empty() {
        record.push(value);
        records.push(record);
    }
    records
}

/// Код поля слияния для столбца `name` (в кавычках, если в имени есть
/// пробелы).
fn merge_field_code(name: &str) -> String {
    if name.contains(char::is_whitespace) {
        format!(" MERGEFIELD \"{name}\" ")
    } else {
        format!(" MERGEFIELD {name} ")
    }
}

/// Как поле слияния выглядит без данных: `«имя»`.
fn shown_name(name: &str) -> String {
    format!("«{name}»")
}

/// Вставляет поле слияния столбца `name` у места `at` (с оформлением
/// текста перед ним). Возвращает место после поля.
pub fn insert_merge_field(doc: &mut Document, at: Position, name: &str) -> Position {
    let Some(paragraph) = paragraph_mut(doc, at.paragraph) else {
        return at;
    };
    let offset = clamp_offset(paragraph, at.offset);
    let shown = shown_name(name);
    let mark = |kind| RunContent::FieldChar(FieldChar::new(kind));
    let items = vec![
        mark(FieldCharKind::Begin),
        RunContent::FieldCode(merge_field_code(name)),
        mark(FieldCharKind::Separate),
        RunContent::Text(shown.clone()),
        mark(FieldCharKind::End),
    ];
    insert_items(paragraph, offset, items);
    Position::new(at.paragraph, offset + shown.len())
}

/// Имя столбца поля `MERGEFIELD`; `None` у других полей.
fn merge_field_name(field: &letter_model::Field) -> Option<String> {
    (field.name() == "MERGEFIELD")
        .then(|| field.arguments().into_iter().next())
        .flatten()
}

/// Ставит в поля слияния значения записи `record` (`Some((данные,
/// номер))`) или их имена `«имя»` (`None`). Возвращает, сколько полей
/// поменялось.
pub fn show_record(doc: &mut Document, record: Option<(&MergeData, usize)>) -> usize {
    let mut changed = 0;
    for index in 0..paragraph_count(doc) {
        let Some(paragraph) = paragraph_mut(doc, index) else {
            continue;
        };
        changed += show_in_paragraph(paragraph, record);
    }
    changed
}

/// [`show_record`] для одного абзаца.
fn show_in_paragraph(paragraph: &mut Paragraph, record: Option<(&MergeData, usize)>) -> usize {
    let values: Vec<(usize, String)> = fields(paragraph)
        .iter()
        .enumerate()
        .filter_map(|(k, field)| {
            let value = if field.name() == "MERGEREC" {
                record.map_or_else(|| "«MERGEREC»".into(), |(_, row)| (row + 1).to_string())
            } else {
                let name = merge_field_name(field)?;
                match record {
                    Some((data, row)) => field.format_text(data.value(row, &name).unwrap_or("")),
                    None => shown_name(&name),
                }
            };
            Some((k, value))
        })
        .collect();
    let mut changed = 0;
    for (k, value) in values {
        let old = fields(paragraph)
            .get(k)
            .map(|f| paragraph.plain_text()[f.result.clone()].to_string());
        if old.as_deref() != Some(value.as_str()) && set_field_result(paragraph, k, &value) {
            changed += 1;
        }
    }
    changed
}

/// Есть ли в документе поля слияния.
pub fn has_merge_fields(doc: &Document) -> bool {
    (0..paragraph_count(doc)).any(|index| {
        paragraph_ref(doc, index).is_some_and(|p| {
            fields(p)
                .iter()
                .any(|f| matches!(f.name().as_str(), "MERGEFIELD" | "MERGEREC"))
        })
    })
}

/// «Найти и объединить»: документ из копий `doc` — по одной на запись,
/// каждая с новой страницы. Поля слияния в копиях становятся обычным
/// текстом. Без записей — копия `doc` как есть.
pub fn merge_all(doc: &Document, data: &MergeData) -> Document {
    if data.rows.is_empty() {
        return doc.clone();
    }
    let mut out = doc.clone();
    out.sections.clear();
    for row in 0..data.rows.len() {
        let mut one = doc.clone();
        show_record(&mut one, Some((data, row)));
        for index in 0..paragraph_count(&one) {
            if let Some(paragraph) = paragraph_mut(&mut one, index) {
                unlink_merge_fields(paragraph);
            }
        }
        if row > 0
            && let Some(first) = one.sections.first_mut()
        {
            first.props.start = Some(SectionStart::NextPage);
        }
        out.sections.extend(one.sections);
    }
    out
}

/// Заменяет поля слияния абзаца их результатом (обычным текстом).
fn unlink_merge_fields(paragraph: &mut Paragraph) {
    let targets: Vec<bool> = fields(paragraph)
        .iter()
        .map(|f| matches!(f.name().as_str(), "MERGEFIELD" | "MERGEREC"))
        .collect();
    if !targets.contains(&true) {
        return;
    }
    let mut begins = 0;
    // Открытые поля: убирается ли поле и прошёл ли разделитель.
    let mut stack: Vec<(bool, bool)> = Vec::new();
    for inline in &mut paragraph.content {
        let Inline::Run(run) = inline else {
            continue;
        };
        run.content.retain(|item| {
            // Внутри кода убираемого поля убирается всё (и вложенные поля).
            let in_code = stack
                .iter()
                .any(|(target, separated)| *target && !separated);
            match item {
                RunContent::FieldChar(c) => match c.kind {
                    FieldCharKind::Begin => {
                        let target = in_code || targets.get(begins).copied().unwrap_or(false);
                        begins += 1;
                        stack.push((target, false));
                        !target
                    }
                    FieldCharKind::Separate => match stack.last_mut() {
                        Some(top) => {
                            top.1 = true;
                            !top.0
                        }
                        None => true,
                    },
                    FieldCharKind::End => stack.pop().is_none_or(|(target, _)| !target),
                },
                _ => !in_code,
            }
        });
    }
    tidy(paragraph);
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::Run;

    fn data() -> MergeData {
        MergeData::from_csv(
            "\u{FEFF}Имя;Город;Сумма\r\nАнна;Москва;100\r\n\"Иванов; Пётр\";\"Санкт-\nПетербург\";\"2 \"\"000\"\"\"\r\n;;\r\n",
        )
    }

    #[test]
    fn csv_is_read() {
        let d = data();
        assert_eq!(d.headers, ["Имя", "Город", "Сумма"]);
        assert_eq!(d.rows.len(), 2);
        assert_eq!(
            d.rows[1],
            ["Иванов; Пётр", "Санкт-\nПетербург", "2 \"000\""]
        );
        assert_eq!(d.value(0, "город"), Some("Москва"));
        assert_eq!(d.value(0, "Нет"), None);
        let commas = MergeData::from_csv("a,b\n1,2");
        assert_eq!(commas.rows, [["1", "2"]]);
        let (cp1251, _, _) = encoding_rs::WINDOWS_1251.encode("Имя;Город\nОля;Тверь");
        let d = MergeData::from_csv_bytes(&cp1251);
        assert_eq!(d.value(0, "Город"), Some("Тверь"));
        let tabs = MergeData::from_csv("a\tb,c\n1\t2,3\n");
        assert_eq!(tabs.headers, ["a", "b,c"]);
    }

    fn letter() -> Document {
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![Paragraph::from_runs([Run::text("Привет, !")]).into()];
        let at = insert_merge_field(&mut doc, Position::new(0, "Привет, ".len()), "Имя");
        assert_eq!(at.offset, "Привет, «Имя»".len());
        insert_merge_field(&mut doc, Position::new(0, 0), "Город");
        doc
    }

    #[test]
    fn fields_show_records() {
        let mut doc = letter();
        assert_eq!(doc.plain_text(), "«Город»Привет, «Имя»!");
        assert!(has_merge_fields(&doc));
        let d = data();
        assert_eq!(show_record(&mut doc, Some((&d, 1))), 2);
        assert_eq!(doc.plain_text(), "Санкт-\nПетербургПривет, Иванов; Пётр!");
        show_record(&mut doc, None);
        assert_eq!(doc.plain_text(), "«Город»Привет, «Имя»!");
        // Поле переживает сохранение в .docx.
        let p = paragraph_ref(&doc, 0).unwrap();
        assert_eq!(merge_field_name(&fields(p)[1]).as_deref(), Some("Имя"));
    }

    #[test]
    fn merge_makes_a_copy_per_record() {
        let doc = letter();
        let merged = merge_all(&doc, &data());
        assert_eq!(merged.sections.len(), 2);
        assert_eq!(merged.sections[1].props.start, Some(SectionStart::NextPage));
        let texts: Vec<String> = merged.blocks().map(Block::plain_text).collect();
        assert_eq!(texts[0], "МоскваПривет, Анна!");
        assert!(!has_merge_fields(&merged));
        let first = paragraph_ref(&merged, 0).unwrap();
        assert!(fields(first).is_empty());
        // Без записей — тот же документ.
        assert_eq!(merge_all(&doc, &MergeData::default()), doc);
    }

    #[test]
    fn word_table_is_a_source() {
        let mut doc = Document::new();
        crate::table::insert_table(&mut doc, Position::new(0, 0), 2, 2);
        let d = MergeData::from_document(&doc).unwrap();
        assert_eq!(d.headers.len(), 2);
        assert_eq!(d.rows.len(), 1);
        assert!(MergeData::from_document(&Document::new()).is_none());
    }
}
