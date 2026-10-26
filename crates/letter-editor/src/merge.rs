//! Рассылки (слияние): источник данных — таблица с заголовками (файл CSV
//! или первая таблица документа Word), в документе — поля
//! `MERGEFIELD имя` (значение столбца), `MERGEREC` (номер записи),
//! `ADDRESSBLOCK` и `GREETINGLINE` (блок адреса и строка приветствия из
//! столбцов с привычными именами), правила `IF` и `SKIPIF`.
//! «Просмотр результатов» ставит в поля значения записи, «Найти и
//! объединить» собирает документ из копий — по одной на запись.

use letter_model::{
    Block, BreakKind, Document, Field, FieldChar, FieldCharKind, Inline, Paragraph, RunContent,
    SectionStart, fields, set_field_result,
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
    /// Какие записи не войдут в рассылку (снятые отметки), по номерам
    /// записей; нет значения — запись входит.
    pub excluded: Vec<bool>,
}

/// Сравнение в фильтре получателей и в правилах.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Compare {
    /// Равно.
    #[default]
    Equal,
    /// Не равно.
    NotEqual,
    /// Меньше.
    Less,
    /// Больше.
    Greater,
    /// Меньше или равно.
    LessOrEqual,
    /// Больше или равно.
    GreaterOrEqual,
    /// Пусто.
    IsBlank,
    /// Не пусто.
    IsNotBlank,
    /// Содержит (только в фильтре).
    Contains,
    /// Не содержит (только в фильтре).
    NotContains,
}

impl Compare {
    /// Все сравнения правил (без «содержит»).
    pub const RULES: [Compare; 8] = [
        Compare::Equal,
        Compare::NotEqual,
        Compare::Less,
        Compare::Greater,
        Compare::LessOrEqual,
        Compare::GreaterOrEqual,
        Compare::IsBlank,
        Compare::IsNotBlank,
    ];

    /// Все сравнения фильтра.
    pub const FILTERS: [Compare; 10] = [
        Compare::Equal,
        Compare::NotEqual,
        Compare::Less,
        Compare::Greater,
        Compare::LessOrEqual,
        Compare::GreaterOrEqual,
        Compare::IsBlank,
        Compare::IsNotBlank,
        Compare::Contains,
        Compare::NotContains,
    ];

    /// Знак сравнения в коде поля и сравниваемое значение.
    fn operator(self, value: &str) -> (&'static str, String) {
        match self {
            Compare::Equal => ("=", value.into()),
            Compare::NotEqual => ("<>", value.into()),
            Compare::Less => ("<", value.into()),
            Compare::Greater => (">", value.into()),
            Compare::LessOrEqual => ("<=", value.into()),
            Compare::GreaterOrEqual => (">=", value.into()),
            Compare::IsBlank => ("=", String::new()),
            Compare::IsNotBlank => ("<>", String::new()),
            Compare::Contains => ("=", format!("*{value}*")),
            Compare::NotContains => ("<>", format!("*{value}*")),
        }
    }

    /// Выполняется ли `left сравнение right` (числа — как числа, текст —
    /// без учёта регистра, `*` и `?` в `right` у «равно» — любые знаки).
    pub fn holds(self, left: &str, right: &str) -> bool {
        let (operator, right) = self.operator(right);
        compare(left, operator, &right)
    }
}

/// Число из текста («1,5» и «1.5»).
fn number(text: &str) -> Option<f64> {
    text.trim().replace(',', ".").parse().ok()
}

/// Порядок двух значений: числа — как числа, текст — без учёта регистра.
fn order(a: &str, b: &str) -> std::cmp::Ordering {
    match (number(a), number(b)) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
        _ => a.to_lowercase().cmp(&b.to_lowercase()),
    }
}

/// Сравнение по знаку из кода поля (`=`, `<>`, `<`, `>`, `<=`, `>=`).
fn compare(left: &str, operator: &str, right: &str) -> bool {
    use std::cmp::Ordering::{Equal, Greater, Less};
    let order = order(left, right);
    let equal = || {
        if right.contains(['*', '?']) {
            wildcard(&left.to_lowercase(), &right.to_lowercase())
        } else {
            order == Equal
        }
    };
    match operator {
        "=" => equal(),
        "<>" => !equal(),
        "<" => order == Less,
        ">" => order == Greater,
        "<=" => order != Greater,
        ">=" => order != Less,
        _ => false,
    }
}

/// Подходит ли `text` под образец с `*` (любые знаки) и `?` (один знак).
fn wildcard(text: &str, pattern: &str) -> bool {
    let text: Vec<char> = text.chars().collect();
    // row[j]: подходит ли прочитанная часть образца к первым j знакам.
    let mut row = vec![false; text.len() + 1];
    row[0] = true;
    for p in pattern.chars() {
        let mut next = vec![false; text.len() + 1];
        next[0] = p == '*' && row[0];
        for j in 1..=text.len() {
            next[j] = match p {
                '*' => next[j - 1] || row[j],
                '?' => row[j - 1],
                c => row[j - 1] && c == text[j - 1],
            };
        }
        row = next;
    }
    row[text.len()]
}

impl MergeData {
    /// Входит ли запись `row` в рассылку.
    pub fn included(&self, row: usize) -> bool {
        !self.excluded.get(row).copied().unwrap_or(false)
    }

    /// Отмечает, входит ли запись `row` в рассылку.
    pub fn set_included(&mut self, row: usize, on: bool) {
        if self.excluded.len() < self.rows.len() {
            self.excluded.resize(self.rows.len(), false);
        }
        if let Some(excluded) = self.excluded.get_mut(row) {
            *excluded = !on;
        }
    }

    /// Номера записей, которые входят в рассылку, по порядку.
    pub fn records(&self) -> Vec<usize> {
        (0..self.rows.len()).filter(|r| self.included(*r)).collect()
    }

    /// Сортирует записи по столбцу `column` (числа — как числа).
    pub fn sort_by(&mut self, column: usize, descending: bool) {
        let excluded: Vec<bool> = (0..self.rows.len()).map(|r| !self.included(r)).collect();
        let mut pairs: Vec<(Vec<String>, bool)> = std::mem::take(&mut self.rows)
            .into_iter()
            .zip(excluded)
            .collect();
        let key = |row: &[String]| row.get(column).cloned().unwrap_or_default();
        pairs.sort_by(|(a, _), (b, _)| {
            let ordering = order(&key(a), &key(b));
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
        (self.rows, self.excluded) = pairs.into_iter().unzip();
    }

    /// Оставляет в рассылке только записи, у которых значение столбца
    /// `column` подходит под сравнение `how` с `value`.
    pub fn filter(&mut self, column: usize, how: Compare, value: &str) {
        self.excluded = self
            .rows
            .iter()
            .map(|row| !how.holds(row.get(column).map_or("", String::as_str), value))
            .collect();
    }

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
        MergeData {
            headers,
            rows,
            excluded: Vec::new(),
        }
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
            excluded: Vec::new(),
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

/// Знак поля.
fn mark(kind: FieldCharKind) -> RunContent {
    RunContent::FieldChar(FieldChar::new(kind))
}

/// Элементы поля: начало, код, разделитель, результат, конец.
pub(crate) fn field_items(code: String, result: &str) -> Vec<RunContent> {
    vec![
        mark(FieldCharKind::Begin),
        RunContent::FieldCode(code),
        mark(FieldCharKind::Separate),
        RunContent::Text(result.to_string()),
        mark(FieldCharKind::End),
    ]
}

/// Вставляет элементы поля у места `at` (с оформлением текста перед
/// ним). Возвращает место после поля.
fn insert_field_items(doc: &mut Document, at: Position, items: Vec<RunContent>) -> Position {
    let Some(paragraph) = paragraph_mut(doc, at.paragraph) else {
        return at;
    };
    let offset = clamp_offset(paragraph, at.offset);
    let len: usize = items.iter().map(RunContent::text_len).sum();
    insert_items(paragraph, offset, items);
    Position::new(at.paragraph, offset + len)
}

/// Вставляет поле слияния столбца `name` у места `at` (с оформлением
/// текста перед ним). Возвращает место после поля.
pub fn insert_merge_field(doc: &mut Document, at: Position, name: &str) -> Position {
    let items = field_items(merge_field_code(name), &shown_name(name));
    insert_field_items(doc, at, items)
}

/// Особое поле рассылки (кнопки «Блок адреса», «Строка приветствия»,
/// «Правила»).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MergeRule {
    /// Блок адреса (`ADDRESSBLOCK`): имя, организация, адрес из столбцов
    /// с привычными именами («Имя», «Фамилия», «Адрес», «Город»…).
    AddressBlock,
    /// Строка приветствия (`GREETINGLINE`): по-русски «Уважаемый(ая) Имя
    /// Отчество!», по-английски «Dear Title Last,».
    GreetingLine {
        /// По-русски.
        russian: bool,
    },
    /// «Если… то… иначе» (`IF`): текст по значению поля.
    If {
        /// Столбец.
        field: String,
        /// Сравнение.
        compare: Compare,
        /// С чем сравнивать.
        value: String,
        /// Текст, если выполняется.
        then: String,
        /// Текст, если нет.
        otherwise: String,
    },
    /// «Пропустить запись, если…» (`SKIPIF`).
    SkipIf {
        /// Столбец.
        field: String,
        /// Сравнение.
        compare: Compare,
        /// С чем сравнивать.
        value: String,
    },
    /// «Номер записи» (`MERGEREC`).
    RecordNumber,
}

/// Текст в кавычках для кода поля (кавычки внутри заменяются «ёлочками»).
fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "”"))
}

/// Вставляет особое поле рассылки у места `at`. Возвращает место после
/// поля.
pub fn insert_merge_rule(doc: &mut Document, at: Position, rule: &MergeRule) -> Position {
    let condition = |field: &str, compare: Compare, value: &str| {
        let (operator, value) = compare.operator(value);
        let mut items = vec![mark(FieldCharKind::Begin)];
        items.extend(field_items(merge_field_code(field), &shown_name(field)));
        items.push(RunContent::FieldCode(format!(
            " {operator} {} ",
            quoted(&value)
        )));
        items
    };
    let items = match rule {
        MergeRule::AddressBlock => field_items(" ADDRESSBLOCK ".into(), "«AddressBlock»"),
        MergeRule::GreetingLine { russian } => {
            field_items(greeting_code(*russian), "«GreetingLine»")
        }
        MergeRule::RecordNumber => field_items(" MERGEREC ".into(), "«MERGEREC»"),
        MergeRule::If {
            field,
            compare,
            value,
            then,
            otherwise,
        } => {
            let mut items = condition(field, *compare, value);
            // Начало поля IF — перед кодом « IF ».
            items.insert(1, RunContent::FieldCode(" IF ".into()));
            items.push(RunContent::FieldCode(format!(
                "{} {} ",
                quoted(then),
                quoted(otherwise)
            )));
            items.push(mark(FieldCharKind::Separate));
            items.push(RunContent::Text(then.clone()));
            items.push(mark(FieldCharKind::End));
            items
        }
        MergeRule::SkipIf {
            field,
            compare,
            value,
        } => {
            let mut items = condition(field, *compare, value);
            items.insert(1, RunContent::FieldCode(" SKIPIF ".into()));
            items.push(mark(FieldCharKind::End));
            items
        }
    };
    insert_field_items(doc, at, items)
}

/// Код строки приветствия, как пишет Word: образец `\f`, язык `\l`,
/// текст на случай, когда имени нет, `\e`.
fn greeting_code(russian: bool) -> String {
    if russian {
        r#" GREETINGLINE \f "<<_BEFORE_ Уважаемый(ая) >><<_FIRST0_>><< _MIDDLE0_>><<_AFTER_ !>>" \l 1049 \e "Уважаемые господа!" "#.into()
    } else {
        r#" GREETINGLINE \f "<<_BEFORE_ Dear >><<_TITLE0_>><< _LAST0_>><<_AFTER_ ,>>" \l 1033 \e "Dear Sir or Madam," "#.into()
    }
}

/// Части имени и адреса в образцах Word и столбцы, которые им
/// подходят (без учёта регистра).
const ADDRESS_COLUMNS: [(&str, &[&str]); 12] = [
    ("_TITLE0_", &["обращение", "title", "courtesy title"]),
    (
        "_FIRST0_",
        &["имя", "first name", "firstname", "first_name", "name"],
    ),
    ("_MIDDLE0_", &["отчество", "middle name", "middlename"]),
    (
        "_LAST0_",
        &["фамилия", "last name", "lastname", "last_name", "surname"],
    ),
    ("_SUFFIX0_", &["suffix"]),
    (
        "_COMPANY_",
        &[
            "организация",
            "компания",
            "company",
            "company name",
            "organization",
        ],
    ),
    (
        "_STREET1_",
        &[
            "адрес",
            "улица",
            "address",
            "address 1",
            "address line 1",
            "address1",
            "street",
        ],
    ),
    (
        "_STREET2_",
        &["адрес 2", "address 2", "address line 2", "address2"],
    ),
    (
        "_CITY_",
        &["город", "city", "населённый пункт", "населенный пункт"],
    ),
    ("_STATE_", &["регион", "область", "state", "province"]),
    (
        "_POSTAL_",
        &[
            "индекс",
            "почтовый индекс",
            "postal code",
            "zip",
            "zip code",
            "postcode",
        ],
    ),
    ("_COUNTRY_", &["страна", "country"]),
];

/// Образец блока адреса по умолчанию.
const ADDRESS_FORMAT: &str = "<<_TITLE0_ >><<_FIRST0_>><< _MIDDLE0_>><< _LAST0_>><< _SUFFIX0_>>\n<<_COMPANY_\n>><<_STREET1_\n>><<_STREET2_\n>><<_POSTAL_ >><<_CITY_>><<, _STATE_>><<\n_COUNTRY_>>";

/// Значение части имени или адреса `token` в записи `row`.
fn token_value<'a>(data: &'a MergeData, row: usize, token: &str) -> &'a str {
    ADDRESS_COLUMNS
        .iter()
        .find(|(t, _)| *t == token)
        .and_then(|(_, names)| {
            names
                .iter()
                .find_map(|name| data.value(row, name).filter(|v| !v.trim().is_empty()))
        })
        .map_or("", str::trim)
}

/// Текст по образцу Word: куски `<<…>>` с частью `_ИМЯ_` выводятся,
/// только если её значение не пустое; `_BEFORE_` и `_AFTER_` — всегда.
/// Второе значение — была ли хоть одна непустая часть.
fn fill_format(format: &str, data: &MergeData, row: usize) -> (String, bool) {
    let mut out = String::new();
    let mut any = false;
    let mut rest = format;
    while let Some(open) = rest.find("<<") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let Some(close) = after.find(">>") else {
            out.push_str(&rest[open..]);
            return (out, any);
        };
        let piece = &after[..close];
        rest = &after[close + 2..];
        let token = piece.find('_').and_then(|start| {
            let end = piece[start + 1..].find('_')? + start + 2;
            Some(&piece[start..end])
        });
        match token {
            // Пробел сразу после `_BEFORE_` и `_AFTER_` только отделяет их.
            Some(token @ ("_BEFORE_" | "_AFTER_")) => {
                let text = piece.replacen(&format!("{token} "), "", 1);
                out.push_str(&text.replace(token, ""));
            }
            Some(token) => {
                let value = token_value(data, row, token);
                if !value.is_empty() {
                    any = true;
                    out.push_str(&piece.replace(token, value));
                }
            }
            None => out.push_str(piece),
        }
    }
    out.push_str(rest);
    (out, any)
}

/// Блок адреса записи: строки без пустых.
fn address_block(field: &Field, data: &MergeData, row: usize) -> String {
    let format = field
        .switch("\\f")
        .unwrap_or_else(|| ADDRESS_FORMAT.into())
        .replace('\r', "\n");
    let (text, _) = fill_format(&format, data, row);
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Строка приветствия записи; без имени — текст `\e`.
fn greeting_line(field: &Field, data: &MergeData, row: usize) -> String {
    let format = field
        .switch("\\f")
        .unwrap_or_else(|| "<<_BEFORE_ Dear >><<_FIRST0_>><<_AFTER_ ,>>".into());
    match fill_format(&format, data, row) {
        (text, true) => text,
        (_, false) => field.switch("\\e").unwrap_or_default(),
    }
}

/// Коды полей абзаца (по порядку из [`fields`]) вместе с результатами
/// вложенных полей: результат вложенного поля встаёт в код внешнего
/// словом в кавычках (`IF "Анна" = "Анна" …`).
fn codes_with_results(paragraph: &Paragraph) -> Vec<String> {
    let mut codes: Vec<String> = Vec::new();
    // Открытые поля: номер и прошёл ли разделитель.
    let mut stack: Vec<(usize, bool)> = Vec::new();
    let parent_code = |stack: &[(usize, bool)]| match stack {
        [.., (parent, false), _] => Some(*parent),
        _ => None,
    };
    for run in paragraph.runs() {
        for item in &run.content {
            match item {
                RunContent::FieldChar(c) => match c.kind {
                    FieldCharKind::Begin => {
                        if let Some(&(parent, false)) = stack.last() {
                            codes[parent].push_str(" \"");
                        }
                        stack.push((codes.len(), false));
                        codes.push(String::new());
                    }
                    FieldCharKind::Separate => {
                        if let Some(top) = stack.last_mut() {
                            top.1 = true;
                        }
                    }
                    FieldCharKind::End => {
                        stack.pop();
                        if let Some(&(parent, false)) = stack.last() {
                            codes[parent].push_str("\" ");
                        }
                    }
                },
                RunContent::FieldCode(code) => {
                    if let Some(&(own, false)) = stack.last() {
                        codes[own].push_str(code);
                    }
                }
                other => {
                    if let Some(parent) = parent_code(&stack)
                        && stack.last().is_some_and(|(_, separated)| *separated)
                    {
                        let text = match other {
                            RunContent::Text(text) => text.replace('"', "”"),
                            RunContent::Tab => "\t".into(),
                            RunContent::Break(_) => "\n".into(),
                            _ => String::new(),
                        };
                        codes[parent].push_str(&text);
                    }
                }
            }
        }
    }
    codes
}

/// Выполняется ли условие поля `IF` или `SKIPIF`: слова кода после имени
/// — значение, знак сравнения, с чем сравнивать.
fn condition_holds(field: &Field) -> Option<(bool, Vec<String>)> {
    let arguments = field.arguments();
    let [left, operator, right, rest @ ..] = arguments.as_slice() else {
        return None;
    };
    if !matches!(operator.as_str(), "=" | "<>" | "<" | ">" | "<=" | ">=") {
        return None;
    }
    Some((compare(left, operator, right), rest.to_vec()))
}

/// Новый результат поля рассылки; `None` — поле не рассылки или его не
/// надо менять.
fn merge_value(field: &Field, record: Option<(&MergeData, usize)>) -> Option<String> {
    let name = field.name();
    // Записи кончились (наклейки после последней) — поля пустые.
    if let Some((data, row)) = record
        && row >= data.rows.len()
        && MERGE_FIELDS.contains(&name.as_str())
        && name != "SKIPIF"
    {
        return Some(String::new());
    }
    match name.as_str() {
        "NEXT" => Some(match record {
            Some(_) => String::new(),
            None => crate::labels::NEXT_SHOWN.into(),
        }),
        "MERGEREC" => Some(record.map_or_else(
            || "«MERGEREC»".into(),
            |(data, row)| {
                // Номер среди отмеченных записей, как в Word.
                let number = data.records().iter().position(|r| *r == row).unwrap_or(row);
                (number + 1).to_string()
            },
        )),
        "MERGEFIELD" => {
            let name = field.arguments().into_iter().next()?;
            Some(match record {
                Some((data, row)) => field.format_text(data.value(row, &name).unwrap_or("")),
                None => shown_name(&name),
            })
        }
        "ADDRESSBLOCK" => Some(record.map_or_else(
            || "«AddressBlock»".into(),
            |(data, row)| address_block(field, data, row),
        )),
        "GREETINGLINE" => Some(record.map_or_else(
            || "«GreetingLine»".into(),
            |(data, row)| greeting_line(field, data, row),
        )),
        "IF" => {
            let (holds, texts) = condition_holds(field)?;
            let text = if holds { texts.first() } else { texts.get(1) };
            Some(text.cloned().unwrap_or_default())
        }
        _ => None,
    }
}

/// Ставит в поля слияния значения записи `record` (`Some((данные,
/// номер))`) или их имена `«имя»` (`None`). Возвращает, сколько полей
/// поменялось.
///
/// Поле `NEXT` («Следующая запись», наклейки) переводит следующие за
/// ним поля на следующую отмеченную запись.
pub fn show_record(doc: &mut Document, record: Option<(&MergeData, usize)>) -> usize {
    let mut changed = 0;
    let mut record = record;
    for index in 0..paragraph_count(doc) {
        let Some(paragraph) = paragraph_mut(doc, index) else {
            continue;
        };
        let (count, next) = show_in_paragraph(paragraph, record);
        changed += count;
        record = next;
    }
    changed
}

/// Следующая отмеченная запись после `row`; нет — `data.rows.len()`.
fn next_row(data: &MergeData, row: usize) -> usize {
    (row + 1..data.rows.len())
        .find(|r| data.included(*r))
        .unwrap_or(data.rows.len())
}

/// [`show_record`] для одного абзаца: сначала вложенные поля, потом
/// внешние (условие `IF` смотрит на результаты вложенных). Возвращает,
/// сколько полей поменялось, и запись после абзаца (после полей `NEXT`).
fn show_in_paragraph<'a>(
    paragraph: &mut Paragraph,
    record: Option<(&'a MergeData, usize)>,
) -> (usize, Option<(&'a MergeData, usize)>) {
    let Some(deepest) = fields(paragraph).iter().map(|f| f.depth).max() else {
        return (0, record);
    };
    // Запись для каждого поля: после каждого `NEXT` — следующая.
    let mut current = record;
    let records: Vec<Option<(&MergeData, usize)>> = fields(paragraph)
        .iter()
        .map(|field| {
            if field.depth == 0 && field.name() == "NEXT" {
                current = current.map(|(data, row)| (data, next_row(data, row)));
            }
            current
        })
        .collect();
    let mut changed = 0;
    for depth in (0..=deepest).rev() {
        let codes = codes_with_results(paragraph);
        let values: Vec<(usize, String)> = fields(paragraph)
            .into_iter()
            .enumerate()
            .filter(|(_, field)| field.depth == depth)
            .filter_map(|(k, field)| {
                let field = Field {
                    code: codes.get(k).cloned().unwrap_or_default(),
                    ..field
                };
                let record = records.get(k).copied().flatten();
                Some((k, merge_value(&field, record)?))
            })
            .collect();
        for (k, value) in values {
            let old = fields(paragraph)
                .get(k)
                .map(|f| paragraph.plain_text()[f.result.clone()].to_string());
            if old.as_deref() != Some(value.as_str()) && set_field_result(paragraph, k, &value) {
                changed += 1;
            }
        }
    }
    if changed > 0 {
        line_breaks(paragraph);
    }
    (changed, current)
}

/// Переводы строки в тексте абзаца (из значений таблицы и блока адреса)
/// становятся разрывами строки.
fn line_breaks(paragraph: &mut Paragraph) {
    for inline in &mut paragraph.content {
        let Inline::Run(run) = inline else {
            continue;
        };
        if !run
            .content
            .iter()
            .any(|item| matches!(item, RunContent::Text(t) if t.contains('\n')))
        {
            continue;
        }
        let mut content = Vec::new();
        for item in std::mem::take(&mut run.content) {
            match item {
                RunContent::Text(text) if text.contains('\n') => {
                    for (i, line) in text.split('\n').enumerate() {
                        if i > 0 {
                            content.push(RunContent::Break(BreakKind::Line));
                        }
                        if !line.is_empty() {
                            content.push(RunContent::Text(line.to_string()));
                        }
                    }
                }
                other => content.push(other),
            }
        }
        run.content = content;
    }
}

/// Поля рассылки (они становятся текстом в документе слияния).
const MERGE_FIELDS: [&str; 8] = [
    "MERGEFIELD",
    "MERGEREC",
    "ADDRESSBLOCK",
    "GREETINGLINE",
    "IF",
    "SKIPIF",
    "NEXT",
    "NEXTIF",
];

/// Есть ли в документе поля слияния.
pub fn has_merge_fields(doc: &Document) -> bool {
    (0..paragraph_count(doc)).any(|index| {
        paragraph_ref(doc, index).is_some_and(|p| {
            fields(p).iter().any(|f| {
                matches!(
                    f.name().as_str(),
                    "MERGEFIELD" | "MERGEREC" | "ADDRESSBLOCK" | "GREETINGLINE"
                )
            })
        })
    })
}

/// Пропускается ли запись: условие какого-нибудь поля `SKIPIF`
/// выполняется (поля уже показывают значения записи).
fn skipped(doc: &Document) -> bool {
    (0..paragraph_count(doc)).any(|index| {
        let Some(paragraph) = paragraph_ref(doc, index) else {
            return false;
        };
        let codes = codes_with_results(paragraph);
        fields(paragraph).into_iter().enumerate().any(|(k, field)| {
            let field = Field {
                code: codes.get(k).cloned().unwrap_or_default(),
                ..field
            };
            field.name() == "SKIPIF" && condition_holds(&field).is_some_and(|(holds, _)| holds)
        })
    })
}

/// «Найти и объединить»: документ из копий `doc` — по одной на каждую
/// отмеченную запись (кроме пропущенных правилом `SKIPIF`), каждая с
/// новой страницы. Поля рассылки в копиях становятся обычным текстом.
/// Без записей — копия `doc` как есть.
///
/// В листе наклеек каждая копия берёт столько записей, сколько в ней
/// наклеек (полей `NEXT` плюс одна).
pub fn merge_all(doc: &Document, data: &MergeData) -> Document {
    let mut out = doc.clone();
    out.sections.clear();
    let step = 1
        + (0..paragraph_count(doc))
            .filter_map(|index| paragraph_ref(doc, index))
            .map(crate::labels::next_count)
            .sum::<usize>();
    for row in data.records().into_iter().step_by(step) {
        let mut one = doc.clone();
        show_record(&mut one, Some((data, row)));
        if skipped(&one) {
            continue;
        }
        for index in 0..paragraph_count(&one) {
            if let Some(paragraph) = paragraph_mut(&mut one, index) {
                unlink_merge_fields(paragraph);
            }
        }
        if !out.sections.is_empty()
            && let Some(first) = one.sections.first_mut()
        {
            first.props.start = Some(SectionStart::NextPage);
        }
        out.sections.extend(one.sections);
    }
    if out.sections.is_empty() {
        return doc.clone();
    }
    // Документ слияния — обычный документ, без источника данных.
    out.settings.mail_merge = None;
    out
}

/// Заменяет поля рассылки абзаца их результатом (обычным текстом).
fn unlink_merge_fields(paragraph: &mut Paragraph) {
    let targets: Vec<bool> = fields(paragraph)
        .iter()
        .map(|f| MERGE_FIELDS.contains(&f.name().as_str()))
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
        assert_eq!(fields(p)[1].arguments(), ["Имя"]);
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
    fn recipients_are_sorted_and_filtered() {
        let mut d = MergeData::from_csv("Имя;Сумма\nВера;20\nанна;100\nБорис;3\n");
        d.sort_by(1, false);
        let names = |d: &MergeData| d.rows.iter().map(|r| r[0].clone()).collect::<Vec<_>>();
        assert_eq!(names(&d), ["Борис", "Вера", "анна"]);
        d.sort_by(0, true);
        assert_eq!(names(&d), ["Вера", "Борис", "анна"]);
        d.set_included(1, false);
        assert_eq!(d.records(), [0, 2]);
        // Отметка едет вместе с записью при сортировке.
        d.sort_by(0, false);
        assert_eq!(names(&d), ["анна", "Борис", "Вера"]);
        assert_eq!(d.records(), [0, 2]);
        d.filter(1, Compare::GreaterOrEqual, "20");
        assert_eq!(d.records(), [0, 2]);
        d.filter(0, Compare::Contains, "ОР");
        assert_eq!(d.records(), [1]);
        d.filter(0, Compare::IsNotBlank, "");
        assert_eq!(d.records(), [0, 1, 2]);
        assert!(wildcard("иванов", "ив*в"));
        assert!(wildcard("иванов", "?ванов"));
        assert!(!wildcard("иванов", "ив?в"));
        assert!(Compare::Less.holds("9", "10"));
        assert!(Compare::Equal.holds("Да", "да"));
    }

    #[test]
    fn rules_address_and_greeting() {
        let d = MergeData::from_csv(
            "Имя;Отчество;Фамилия;Пол;Адрес;Город;Индекс;Долг\nИван;Петрович;Сидоров;М;ул. Ленина, 1;Тверь;170000;0\nОльга;;Ким;Ж;;Омск;;5\n",
        );
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![Paragraph::default().into(), Paragraph::default().into()];
        let rule = MergeRule::If {
            field: "Пол".into(),
            compare: Compare::Equal,
            value: "Ж".into(),
            then: "Уважаемая".into(),
            otherwise: "Уважаемый".into(),
        };
        let at = insert_merge_rule(&mut doc, Position::new(0, 0), &rule);
        let at = insert_merge_rule(&mut doc, at, &MergeRule::GreetingLine { russian: true });
        insert_merge_rule(&mut doc, at, &MergeRule::RecordNumber);
        insert_merge_rule(&mut doc, Position::new(1, 0), &MergeRule::AddressBlock);
        let skip = MergeRule::SkipIf {
            field: "Долг".into(),
            compare: Compare::Equal,
            value: "0".into(),
        };
        insert_merge_rule(&mut doc, Position::new(1, 0), &skip);
        assert!(has_merge_fields(&doc));
        show_record(&mut doc, Some((&d, 0)));
        let text = |doc: &Document, i: usize| paragraph_ref(doc, i).unwrap().plain_text();
        // «М» — результат вложенного поля в коде IF: на странице его не видно.
        assert_eq!(text(&doc, 0), "МУважаемыйУважаемый(ая) Иван Петрович!1");
        assert_eq!(
            text(&doc, 1),
            // «0» — скрытый результат поля в коде SKIPIF.
            "0Иван Петрович Сидоров\nул. Ленина, 1\n170000 Тверь"
        );
        // Разрывы строки — настоящие.
        let p = paragraph_ref(&doc, 1).unwrap();
        assert!(
            p.runs()
                .any(|r| r.content.contains(&RunContent::Break(BreakKind::Line)))
        );
        show_record(&mut doc, Some((&d, 1)));
        assert_eq!(text(&doc, 0), "ЖУважаемаяУважаемый(ая) Ольга!2");
        assert_eq!(text(&doc, 1), "5Ольга Ким\nОмск");
        // Без данных — имена полей.
        show_record(&mut doc, None);
        assert_eq!(text(&doc, 1), "«Долг»«AddressBlock»");
        // Иван без долга пропущен правилом SKIPIF.
        let merged = merge_all(&doc, &d);
        assert_eq!(merged.sections.len(), 1);
        assert_eq!(text(&merged, 0), "УважаемаяУважаемый(ая) Ольга!2");
        assert!(fields(paragraph_ref(&merged, 0).unwrap()).is_empty());
        assert!(fields(paragraph_ref(&merged, 1).unwrap()).is_empty());
        // Строка приветствия без имени — текст `\e`.
        let nobody = MergeData::from_csv("Город\nТверь\n");
        show_record(&mut doc, Some((&nobody, 0)));
        assert_eq!(text(&doc, 0), "УважаемыйУважаемые господа!1");
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
