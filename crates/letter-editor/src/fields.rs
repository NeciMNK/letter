//! Обновление полей (F9): новый результат полей даты и времени, свойств
//! документа, имени файла, номеров страниц и статистики. Поля, которые
//! здесь не считаются (оглавление, ссылки и т. п.), и закреплённые поля
//! остаются как были.

use letter_model::{Document, Field, fields, set_field_result};

use crate::edit::{paragraph_count, paragraph_mut, paragraph_ref, plain_text};

/// Дата и время (местные).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DateTime {
    /// Год.
    pub year: i32,
    /// Месяц, 1–12.
    pub month: u32,
    /// День, 1–31.
    pub day: u32,
    /// Часы, 0–23.
    pub hour: u32,
    /// Минуты.
    pub minute: u32,
    /// Секунды.
    pub second: u32,
}

impl DateTime {
    /// Дата и время по числу секунд от 1 января 1970 года (UTC) и сдвигу
    /// часового пояса в секундах.
    pub fn from_unix(seconds: i64, offset: i64) -> DateTime {
        let local = seconds + offset;
        let days = local.div_euclid(86_400);
        let time = local.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        DateTime {
            year,
            month,
            day,
            hour: (time / 3600) as u32,
            minute: (time / 60 % 60) as u32,
            second: (time % 60) as u32,
        }
    }

    /// Дата из свойств документа (`2024-05-01T10:00:00Z`) в местном
    /// времени со сдвигом `offset` секунд; `None`, если не разобрать.
    pub fn parse_w3c(text: &str, offset: i64) -> Option<DateTime> {
        let number = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
        let year = number(0..4)?;
        let month = number(5..7)?;
        let day = number(8..10)?;
        let (hour, minute, second) = if text.len() >= 19 {
            (number(11..13)?, number(14..16)?, number(17..19)?)
        } else {
            (0, 0, 0)
        };
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        let days = days_from_civil(year, month, day);
        let seconds = days * 86_400 + hour * 3600 + minute * 60 + second;
        // Время без «Z» и сдвига считаем уже местным.
        let utc = text.ends_with('Z') || text.len() > 19;
        Some(DateTime::from_unix(seconds, if utc { offset } else { 0 }))
    }

    /// День недели: 0 — понедельник, 6 — воскресенье.
    fn weekday(&self) -> usize {
        let days = days_from_civil(
            i64::from(self.year),
            i64::from(self.month),
            i64::from(self.day),
        );
        // 1 января 1970 года — четверг.
        (days + 3).rem_euclid(7) as usize
    }
}

/// Число дней от 1 января 1970 года по дате (алгоритм Х. Хиннанта).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Дата по числу дней от 1 января 1970 года.
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year as i32, month as u32, day as u32)
}

/// Всё, что нужно для обновления полей, кроме самого документа.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FieldContext {
    /// Сейчас (местное время).
    pub now: DateTime,
    /// Сдвиг местного времени от UTC, секунды (для дат из свойств).
    pub utc_offset: i64,
    /// Полный путь к файлу документа, если он сохранён.
    pub path: Option<String>,
    /// Сколько страниц в документе.
    pub pages: usize,
    /// На какой странице (с 1) каждый абзац, по порядку абзацев.
    pub page_of_paragraph: Vec<usize>,
    /// Русские названия месяцев и дней недели (иначе английские).
    pub russian: bool,
    /// Имя пользователя (поле USERNAME).
    pub user_name: Option<String>,
}

/// Обновляет поля в абзацах с `first` по `last`. Возвращает, сколько
/// полей получили новый результат.
pub fn update_fields(
    doc: &mut Document,
    first: usize,
    last: usize,
    context: &FieldContext,
) -> usize {
    let last = last.min(paragraph_count(doc).saturating_sub(1));
    let statistics = Statistics::of(doc);
    let mut updated = 0;
    for index in first..=last {
        let Some(paragraph) = paragraph_ref(doc, index) else {
            continue;
        };
        let values: Vec<(usize, String)> = fields(paragraph)
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.locked)
            .filter_map(|(k, f)| Some((k, field_value(f, doc, index, context, &statistics)?)))
            .collect();
        let Some(paragraph) = paragraph_mut(doc, index) else {
            continue;
        };
        for (k, value) in values {
            let old = fields(paragraph)
                .get(k)
                .map(|f| plain_text(paragraph)[f.result.clone()].to_string());
            if old.as_deref() != Some(value.as_str()) && set_field_result(paragraph, k, &value) {
                updated += 1;
            }
        }
    }
    updated
}

/// Число слов и знаков документа.
struct Statistics {
    words: usize,
    characters: usize,
}

impl Statistics {
    fn of(doc: &Document) -> Statistics {
        let mut words = 0;
        let mut characters = 0;
        for index in 0..paragraph_count(doc) {
            if let Some(p) = paragraph_ref(doc, index) {
                let text = plain_text(p);
                words += text.split_whitespace().count();
                characters += text.chars().filter(|c| !c.is_whitespace()).count();
            }
        }
        Statistics { words, characters }
    }
}

/// Новый результат поля; `None` — поле так не обновляется.
fn field_value(
    field: &Field,
    doc: &Document,
    paragraph: usize,
    context: &FieldContext,
    statistics: &Statistics,
) -> Option<String> {
    let core = &doc.properties.core;
    let date = |default: &str, value: DateTime| {
        let picture = field.switch("\\@").unwrap_or_else(|| default.to_string());
        Some(field.format_text(&format_date(&picture, value, context.russian)))
    };
    let stored = |text: &Option<String>| {
        text.as_deref()
            .and_then(|t| DateTime::parse_w3c(t, context.utc_offset))
    };
    let text = |value: Option<&str>| Some(field.format_text(value.unwrap_or_default()));
    let date_default = if context.russian {
        "dd.MM.yyyy"
    } else {
        "M/d/yyyy"
    };
    let time_default = if context.russian {
        "H:mm"
    } else {
        "h:mm am/pm"
    };
    match field.name().as_str() {
        "DATE" => date(date_default, context.now),
        "TIME" => date(time_default, context.now),
        "CREATEDATE" => date(date_default, stored(&core.created)?),
        "SAVEDATE" => date(date_default, stored(&core.modified)?),
        "PRINTDATE" => date(date_default, stored(&core.last_printed)?),
        "AUTHOR" => text(core.creator.as_deref()),
        "TITLE" => text(core.title.as_deref()),
        "SUBJECT" => text(core.subject.as_deref()),
        "KEYWORDS" => text(core.keywords.as_deref()),
        "COMMENTS" => text(core.description.as_deref()),
        "LASTSAVEDBY" => text(core.last_modified_by.as_deref()),
        "USERNAME" => text(context.user_name.as_deref()),
        "FILENAME" => {
            let path = context.path.as_deref()?;
            let full = field
                .arguments()
                .iter()
                .any(|a| a.eq_ignore_ascii_case("\\p"));
            let name = if full {
                path
            } else {
                path.rsplit(['/', '\\']).next().unwrap_or(path)
            };
            text(Some(name))
        }
        "PAGE" => Some(field.format_number(*context.page_of_paragraph.get(paragraph)?)),
        "NUMPAGES" if context.pages > 0 => Some(field.format_number(context.pages)),
        "NUMWORDS" => Some(field.format_number(statistics.words)),
        "NUMCHARS" => Some(field.format_number(statistics.characters)),
        // Текст закладки и номер её страницы.
        "REF" => {
            let name = field.arguments().into_iter().next()?;
            let (start, end) = crate::links::bookmark_range(doc, &name)?;
            let piece = crate::clipboard::copy_range(doc, start, end);
            let text = piece
                .blocks()
                .map(letter_model::Block::plain_text)
                .collect::<Vec<_>>()
                .join(" ");
            Some(field.format_text(&text))
        }
        "PAGEREF" => {
            let name = field.arguments().into_iter().next()?;
            let start = crate::links::bookmark_position(doc, &name)?;
            Some(field.format_number(*context.page_of_paragraph.get(start.paragraph)?))
        }
        "SECTION" => {
            let section = crate::edit::top_level(doc, paragraph)?.0 + 1;
            Some(field.format_number(section))
        }
        _ => None,
    }
}

const MONTHS_RU: [&str; 12] = [
    "январь",
    "февраль",
    "март",
    "апрель",
    "май",
    "июнь",
    "июль",
    "август",
    "сентябрь",
    "октябрь",
    "ноябрь",
    "декабрь",
];
const MONTHS_RU_OF: [&str; 12] = [
    "января",
    "февраля",
    "марта",
    "апреля",
    "мая",
    "июня",
    "июля",
    "августа",
    "сентября",
    "октября",
    "ноября",
    "декабря",
];
const MONTHS_EN: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS_RU: [&str; 7] = [
    "понедельник",
    "вторник",
    "среда",
    "четверг",
    "пятница",
    "суббота",
    "воскресенье",
];
const DAYS_EN: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// Дата и время по образцу Word (ключ `\@`): `d`, `dd`, `ddd`, `dddd`,
/// `M`…`MMMM`, `yy`, `yyyy`, `h`/`hh` (12 часов), `H`/`HH`, `m`/`mm`,
/// `s`/`ss`, `am/pm`, `AM/PM`; текст в одинарных кавычках — как есть.
pub fn format_date(picture: &str, value: DateTime, russian: bool) -> String {
    let chars: Vec<char> = picture.chars().collect();
    // Русский месяц после числа — в родительном падеже: «22 октября».
    let with_day = chars.windows(2).any(|w| w[0] == 'd' && w[1] != 'd')
        || chars.last() == Some(&'d') && !picture.contains("ddd");
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let rest: String = chars[i..].iter().collect();
        if c == '\'' {
            let end = chars[i + 1..].iter().position(|&c| c == '\'');
            let end = end.map_or(chars.len(), |e| i + 1 + e);
            out.extend(&chars[i + 1..end]);
            i = end + 1;
            continue;
        }
        if rest.to_ascii_lowercase().starts_with("am/pm") {
            let pm = value.hour >= 12;
            let mark = if pm { "PM" } else { "AM" };
            out.push_str(&if rest.starts_with("am/pm") {
                mark.to_lowercase()
            } else {
                mark.to_string()
            });
            i += 5;
            continue;
        }
        let run = chars[i..].iter().take_while(|&&x| x == c).count();
        let two = |n: u32| format!("{n:02}");
        let hour12 = match value.hour % 12 {
            0 => 12,
            h => h,
        };
        let piece = match (c, run) {
            ('d', 1) => value.day.to_string(),
            ('d', 2) => two(value.day),
            ('d', 3) => {
                let name = if russian { DAYS_RU } else { DAYS_EN }[value.weekday()];
                let short: String = name.chars().take(if russian { 2 } else { 3 }).collect();
                short
            }
            ('d', _) => (if russian { DAYS_RU } else { DAYS_EN })[value.weekday()].to_string(),
            ('M', 1) => value.month.to_string(),
            ('M', 2) => two(value.month),
            ('M', run) => {
                let month = (value.month as usize).clamp(1, 12) - 1;
                let name = match (russian, with_day) {
                    (true, true) => MONTHS_RU_OF[month],
                    (true, false) => MONTHS_RU[month],
                    _ => MONTHS_EN[month],
                };
                if run == 3 {
                    name.chars().take(3).collect()
                } else {
                    name.to_string()
                }
            }
            ('y', 1 | 2) => format!("{:02}", value.year.rem_euclid(100)),
            ('y', _) => value.year.to_string(),
            ('h', 1) => hour12.to_string(),
            ('h', _) => two(hour12),
            ('H', 1) => value.hour.to_string(),
            ('H', _) => two(value.hour),
            ('m', 1) => value.minute.to_string(),
            ('m', _) => two(value.minute),
            ('s', 1) => value.second.to_string(),
            ('s', _) => two(value.second),
            _ => chars[i..i + run].iter().collect(),
        };
        out.push_str(&piece);
        i += run;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::{FieldChar, FieldCharKind, Paragraph, Run, RunContent};

    fn field_paragraph(code: &str, result: &str) -> Paragraph {
        let mark = |kind| Run {
            content: vec![RunContent::FieldChar(FieldChar::new(kind))],
            ..Run::default()
        };
        Paragraph::from_runs([
            Run::text("["),
            mark(FieldCharKind::Begin),
            Run {
                content: vec![RunContent::FieldCode(code.into())],
                ..Run::default()
            },
            mark(FieldCharKind::Separate),
            Run::text(result),
            mark(FieldCharKind::End),
            Run::text("]"),
        ])
    }

    fn context() -> FieldContext {
        FieldContext {
            // 22 октября 2026 года, четверг, 14:05:09.
            now: DateTime::from_unix(1_792_667_109, 3 * 3600),
            utc_offset: 3 * 3600,
            path: Some("/home/nikita/Отчёт.docx".into()),
            pages: 7,
            page_of_paragraph: vec![1, 3],
            russian: true,
            user_name: None,
        }
    }

    fn updated(code: &str) -> String {
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![
            field_paragraph(code, "old").into(),
            field_paragraph(code, "old").into(),
        ];
        doc.properties.core.title = Some("годовой отчёт".into());
        doc.properties.core.created = Some("2024-05-01T22:30:00Z".into());
        update_fields(&mut doc, 0, 1, &context());
        doc.sections[0].blocks[1].plain_text()
    }

    #[test]
    fn dates() {
        let now = context().now;
        assert_eq!((now.year, now.month, now.day), (2026, 10, 22));
        assert_eq!((now.hour, now.minute, now.second), (14, 5, 9));
        assert_eq!(now.weekday(), 3);
        assert_eq!(format_date("d MMMM yyyy", now, true), "22 октября 2026");
        assert_eq!(format_date("MMMM yyyy", now, true), "октябрь 2026");
        assert_eq!(
            format_date("dddd, d MMM yy", now, false),
            "Thursday, 22 Oct 26"
        );
        assert_eq!(format_date("h:mm AM/PM", now, false), "2:05 PM");
        assert_eq!(format_date("HH:mm:ss 'ч'", now, true), "14:05:09 ч");
        let created = DateTime::parse_w3c("2024-05-01T22:30:00Z", 3 * 3600).unwrap();
        assert_eq!((created.month, created.day, created.hour), (5, 2, 1));
    }

    #[test]
    fn fields_are_updated() {
        assert_eq!(updated(" DATE \\@ \"d MMMM yyyy\" "), "[22 октября 2026]");
        assert_eq!(updated("DATE"), "[22.10.2026]");
        assert_eq!(updated("TIME"), "[14:05]");
        assert_eq!(
            updated("CREATEDATE \\@ \"dd.MM.yyyy HH:mm\""),
            "[02.05.2024 01:30]"
        );
        assert_eq!(updated("TITLE \\* Caps"), "[Годовой Отчёт]");
        assert_eq!(updated("FILENAME"), "[Отчёт.docx]");
        assert_eq!(updated("FILENAME \\p"), "[/home/nikita/Отчёт.docx]");
        assert_eq!(updated("PAGE \\* ROMAN"), "[III]");
        assert_eq!(updated("NUMPAGES"), "[7]");
        assert_eq!(updated("SECTION"), "[1]");
        // Неизвестное поле и ссылка на несуществующую закладку сохраняют
        // результат.
        assert_eq!(updated("REF bookmark"), "[old]");
        assert_eq!(updated("HYPERLINK \"https://a.b\""), "[old]");
    }

    #[test]
    fn references_to_bookmarks() {
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![
            Paragraph::text("Chapter one").into(),
            field_paragraph(" REF chapter \\h ", "old").into(),
            field_paragraph("PAGEREF chapter", "old").into(),
            field_paragraph("REF chapter \\* Upper", "old").into(),
        ];
        crate::links::add_bookmark(
            &mut doc,
            "Chapter",
            crate::Position::new(0, 8),
            crate::Position::new(0, 11),
        );
        update_fields(&mut doc, 0, 3, &context());
        let text = |i: usize| doc.sections[0].blocks[i].plain_text();
        assert_eq!(text(1), "[one]");
        assert_eq!(text(2), "[1]");
        assert_eq!(text(3), "[ONE]");
    }

    #[test]
    fn locked_field_is_kept() {
        let mut doc = Document::new();
        let mut p = field_paragraph("DATE", "old");
        if let letter_model::Inline::Run(run) = &mut p.content[1]
            && let RunContent::FieldChar(c) = &mut run.content[0]
        {
            c.locked = true;
        }
        doc.sections[0].blocks = vec![p.into()];
        assert_eq!(update_fields(&mut doc, 0, 0, &context()), 0);
        assert_eq!(doc.sections[0].blocks[0].plain_text(), "[old]");
    }
}
