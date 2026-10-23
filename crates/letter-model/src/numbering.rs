//! Списки: маркированные, нумерованные и многоуровневые
//! (`word/numbering.xml`).
//!
//! Устроено как в Word. Абстрактный список ([`AbstractNumbering`])
//! описывает до девяти уровней: вид номера, текст («%1.»), отступы,
//! шрифт маркера. Список ([`NumberingInstance`], `w:num`) ссылается на
//! абстрактный и может поменять отдельные уровни — например, начать
//! нумерацию заново. Абзац ссылается на список номером и уровнем
//! ([`NumberingRef`]).

use std::collections::{HashMap, HashSet};

use crate::{Alignment, ParagraphProps, RawXml, RunProps};

/// Определения списков документа.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Numbering {
    /// Абстрактные списки (`w:abstractNum`).
    pub abstracts: Vec<AbstractNumbering>,
    /// Списки, на которые ссылаются абзацы (`w:num`).
    pub instances: Vec<NumberingInstance>,
    /// Непонятые элементы (`w:numPicBullet` и т. п.).
    pub unknown: Vec<RawXml>,
}

/// Абстрактный список: уровни и их оформление.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AbstractNumbering {
    /// Номер (`w:abstractNumId`).
    pub id: u32,
    /// Прочие атрибуты элемента, как в файле: имя с префиксом и значение.
    pub attributes: Vec<(String, String)>,
    /// Уровни.
    pub levels: Vec<ListLevel>,
    /// Непонятые элементы (`w:nsid`, `w:multiLevelType`, `w:tmpl`…).
    pub unknown: Vec<RawXml>,
}

/// Уровень списка (`w:lvl`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListLevel {
    /// Номер уровня, 0–8 (`w:ilvl`).
    pub level: u8,
    /// Прочие атрибуты (`w:tplc`, `w15:tentative`).
    pub attributes: Vec<(String, String)>,
    /// С какого числа начинать.
    pub start: Option<u32>,
    /// Вид номера.
    pub format: Option<NumberFormat>,
    /// Текст номера: `%1.` — номер первого уровня с точкой, `•` — маркер.
    pub text: Option<String>,
    /// После какого уровня начинать заново (`w:lvlRestart`): 0 — никогда.
    pub restart: Option<u8>,
    /// Стиль абзаца, связанный с уровнем.
    pub style: Option<String>,
    /// Номера всех уровней арабскими цифрами («1.1.1» вместо «I.a.i»).
    pub legal: Option<bool>,
    /// Что стоит после номера.
    pub suffix: Option<LevelSuffix>,
    /// Выравнивание номера.
    pub alignment: Option<Alignment>,
    /// Оформление абзаца (обычно отступы и табуляция).
    pub paragraph: ParagraphProps,
    /// Оформление номера (шрифт маркера, цвет).
    pub run: RunProps,
    /// Непонятые элементы.
    pub unknown: Vec<RawXml>,
}

/// Вид номера (`w:numFmt`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NumberFormat {
    /// 1, 2, 3.
    Decimal,
    /// 01, 02, 03.
    DecimalZero,
    /// I, II, III.
    UpperRoman,
    /// i, ii, iii.
    LowerRoman,
    /// A, B, C.
    UpperLetter,
    /// a, b, c.
    LowerLetter,
    /// А, Б, В.
    RussianUpper,
    /// а, б, в.
    RussianLower,
    /// 1st, 2nd.
    Ordinal,
    /// Маркер: номер — сам текст уровня.
    Bullet,
    /// Без номера.
    None,
    /// Другой вид — имя как в файле; номер пишется арабскими цифрами.
    Other(String),
}

/// Что стоит после номера (`w:suff`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LevelSuffix {
    /// Табуляция (так по умолчанию).
    Tab,
    /// Пробел.
    Space,
    /// Ничего.
    Nothing,
}

/// Список, на который ссылаются абзацы (`w:num`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NumberingInstance {
    /// Номер (`w:numId`).
    pub id: u32,
    /// Прочие атрибуты (`w16cid:durableId`).
    pub attributes: Vec<(String, String)>,
    /// Абстрактный список.
    pub abstract_id: u32,
    /// Изменения отдельных уровней.
    pub overrides: Vec<LevelOverride>,
    /// Непонятые элементы.
    pub unknown: Vec<RawXml>,
}

/// Изменение уровня в списке (`w:lvlOverride`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LevelOverride {
    /// Уровень.
    pub level: u8,
    /// Начать с этого числа (`w:startOverride`).
    pub start: Option<u32>,
    /// Уровень целиком вместо абстрактного.
    pub definition: Option<ListLevel>,
    /// Непонятые элементы.
    pub unknown: Vec<RawXml>,
}

/// Ссылка абзаца на список (`w:numPr`). Часть может прийти из стиля:
/// например, стиль задаёт список, а абзац — только уровень.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct NumberingRef {
    /// Номер списка (`w:numId`); 0 — «не в списке».
    pub id: Option<u32>,
    /// Уровень (`w:ilvl`), 0–8.
    pub level: Option<u8>,
}

impl Numbering {
    /// Списков нет.
    pub fn is_empty(&self) -> bool {
        *self == Numbering::default()
    }

    /// Список по номеру.
    pub fn instance(&self, id: u32) -> Option<&NumberingInstance> {
        self.instances.iter().find(|n| n.id == id)
    }

    /// Абстрактный список по номеру.
    pub fn abstract_numbering(&self, id: u32) -> Option<&AbstractNumbering> {
        self.abstracts.iter().find(|a| a.id == id)
    }

    /// Итоговый уровень списка `id`: уровень абстрактного списка с
    /// изменениями из `w:lvlOverride`.
    pub fn level(&self, id: u32, level: u8) -> Option<ListLevel> {
        let instance = self.instance(id)?;
        let over = instance.overrides.iter().find(|o| o.level == level);
        let mut found = match over.and_then(|o| o.definition.clone()) {
            Some(definition) => definition,
            None => self
                .abstract_numbering(instance.abstract_id)?
                .levels
                .iter()
                .find(|l| l.level == level)?
                .clone(),
        };
        if let Some(start) = over.and_then(|o| o.start) {
            found.start = Some(start);
        }
        Some(found)
    }

    /// Свободный номер для нового списка.
    pub fn next_instance_id(&self) -> u32 {
        self.instances.iter().map(|n| n.id).max().unwrap_or(0) + 1
    }

    /// Свободный номер для нового абстрактного списка.
    pub fn next_abstract_id(&self) -> u32 {
        self.abstracts.iter().map(|a| a.id + 1).max().unwrap_or(0)
    }
}

/// Номер пункта списка, готовый к показу.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListLabel {
    /// Текст номера: «2.», «1.3», «iv)» или знак маркера.
    pub text: String,
    /// Уровень списка, к которому относится пункт (выравнивание,
    /// отделитель, шрифт номера).
    pub level: ListLevel,
}

/// Счётчики списков: пункты по порядку документа превращаются в номера.
///
/// Как в Word, списки (`w:num`) с одним абстрактным списком считают
/// подряд, а переопределение начала (`w:startOverride`) начинает счёт
/// заново, когда список встречается впервые.
#[derive(Debug, Clone, Default)]
pub struct ListCounters {
    /// Текущие номера уровней по абстрактному списку; `None` — уровень
    /// ещё не начат.
    counters: HashMap<u32, [Option<u32>; 9]>,
    /// Списки, которые уже встречались.
    seen: HashSet<u32>,
}

impl ListCounters {
    /// Следующий пункт списка `id` уровня `level`: увеличивает счётчик и
    /// возвращает номер. `None` — такого списка или уровня нет.
    pub fn next(&mut self, numbering: &Numbering, id: u32, level: u8) -> Option<ListLabel> {
        let instance = numbering.instance(id)?;
        let level = level.min(8);
        let definition = numbering.level(id, level)?;
        let counters = self
            .counters
            .entry(instance.abstract_id)
            .or_insert([None; 9]);
        if self.seen.insert(id) {
            for o in &instance.overrides {
                if (o.start.is_some() || o.definition.is_some())
                    && let Some(c) = counters.get_mut(usize::from(o.level))
                {
                    *c = None;
                }
            }
        }
        let start = |l: u8| numbering.level(id, l).and_then(|d| d.start).unwrap_or(0);
        let i = usize::from(level);
        counters[i] = Some(match counters[i] {
            Some(n) => n.saturating_add(1),
            None => start(level),
        });
        // Более глубокие уровни начинаются заново, если `w:lvlRestart`
        // не говорит иначе (0 — не начинать заново никогда, N — после
        // уровня N и выше).
        for deeper in level + 1..9 {
            let restart = numbering.level(id, deeper).and_then(|d| d.restart);
            let reset = match restart {
                Some(0) => false,
                Some(after) => level < after,
                None => true,
            };
            if reset {
                counters[usize::from(deeper)] = None;
            }
        }
        let current = *counters;
        let legal = definition.legal == Some(true);
        let mut text = String::new();
        let mut chars = definition
            .text
            .as_deref()
            .unwrap_or_default()
            .chars()
            .peekable();
        while let Some(c) = chars.next() {
            let placeholder = chars
                .peek()
                .and_then(|d| d.to_digit(10))
                .filter(|d| c == '%' && (1..=9).contains(d));
            let Some(n) = placeholder else {
                text.push(c);
                continue;
            };
            chars.next();
            let l = (n - 1) as u8;
            let value = current[usize::from(l)].unwrap_or_else(|| start(l));
            let format = if legal {
                NumberFormat::Decimal
            } else {
                numbering
                    .level(id, l)
                    .and_then(|d| d.format)
                    .unwrap_or(NumberFormat::Decimal)
            };
            text.push_str(&format.format(value));
        }
        Some(ListLabel {
            text,
            level: definition,
        })
    }
}

impl NumberFormat {
    /// Номер `n` в этом виде: `3` → «3», «iii», «C», «В».
    pub fn format(&self, n: u32) -> String {
        match self {
            NumberFormat::Decimal | NumberFormat::Other(_) => n.to_string(),
            NumberFormat::DecimalZero => format!("{n:02}"),
            NumberFormat::UpperRoman => roman(n),
            NumberFormat::LowerRoman => roman(n).to_lowercase(),
            NumberFormat::UpperLetter => letters(n, &LATIN),
            NumberFormat::LowerLetter => letters(n, &LATIN).to_lowercase(),
            NumberFormat::RussianUpper => letters(n, &RUSSIAN),
            NumberFormat::RussianLower => letters(n, &RUSSIAN).to_lowercase(),
            NumberFormat::Ordinal => ordinal(n),
            NumberFormat::Bullet | NumberFormat::None => String::new(),
        }
    }
}

const LATIN: [char; 26] = [
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S',
    'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
];

/// Русские буквы для нумерации, как в Word: без Ё, Й, Ъ, Ы, Ь.
const RUSSIAN: [char; 28] = [
    'А', 'Б', 'В', 'Г', 'Д', 'Е', 'Ж', 'З', 'И', 'К', 'Л', 'М', 'Н', 'О', 'П', 'Р', 'С', 'Т', 'У',
    'Ф', 'Х', 'Ц', 'Ч', 'Ш', 'Щ', 'Э', 'Ю', 'Я',
];

/// Буквенный номер как в Word: A…Z, потом AA…ZZ, AAA… (буква повторяется).
fn letters(n: u32, alphabet: &[char]) -> String {
    if n == 0 {
        return String::new();
    }
    let size = alphabet.len() as u32;
    let index = ((n - 1) % size) as usize;
    let count = ((n - 1) / size + 1) as usize;
    std::iter::repeat_n(alphabet[index], count).collect()
}

/// Римские цифры; ноль и числа больше 3999 — арабскими.
fn roman(n: u32) -> String {
    if n == 0 || n > 3999 {
        return n.to_string();
    }
    const DIGITS: [(u32, &str); 13] = [
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
    let mut rest = n;
    let mut out = String::new();
    for (value, digits) in DIGITS {
        while rest >= value {
            out.push_str(digits);
            rest -= value;
        }
    }
    out
}

/// 1st, 2nd, 3rd, 4th, 11th…
fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_formats() {
        let f = |format: NumberFormat, n| format.format(n);
        assert_eq!(f(NumberFormat::Decimal, 12), "12");
        assert_eq!(f(NumberFormat::DecimalZero, 7), "07");
        assert_eq!(f(NumberFormat::UpperRoman, 1994), "MCMXCIV");
        assert_eq!(f(NumberFormat::LowerRoman, 4), "iv");
        assert_eq!(f(NumberFormat::UpperLetter, 1), "A");
        assert_eq!(f(NumberFormat::UpperLetter, 27), "AA");
        assert_eq!(f(NumberFormat::LowerLetter, 28), "bb");
        assert_eq!(f(NumberFormat::RussianLower, 3), "в");
        assert_eq!(f(NumberFormat::RussianUpper, 29), "АА");
        assert_eq!(f(NumberFormat::Ordinal, 2), "2nd");
        assert_eq!(f(NumberFormat::Ordinal, 13), "13th");
        assert_eq!(f(NumberFormat::Bullet, 5), "");
    }

    #[test]
    fn levels_with_overrides() {
        let level = |level, start| ListLevel {
            level,
            start: Some(start),
            format: Some(NumberFormat::Decimal),
            text: Some(format!("%{}.", level + 1)),
            ..ListLevel::default()
        };
        let numbering = Numbering {
            abstracts: vec![AbstractNumbering {
                id: 0,
                levels: vec![level(0, 1), level(1, 1)],
                ..Default::default()
            }],
            instances: vec![
                NumberingInstance {
                    id: 1,
                    abstract_id: 0,
                    ..Default::default()
                },
                NumberingInstance {
                    id: 2,
                    abstract_id: 0,
                    overrides: vec![LevelOverride {
                        level: 0,
                        start: Some(5),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ],
            unknown: Vec::new(),
        };
        assert_eq!(numbering.level(1, 0).and_then(|l| l.start), Some(1));
        assert_eq!(numbering.level(2, 0).and_then(|l| l.start), Some(5));
        assert_eq!(numbering.level(2, 1).and_then(|l| l.start), Some(1));
        assert_eq!(numbering.level(3, 0), None);
        assert_eq!(numbering.level(1, 4), None);
        assert_eq!(
            (numbering.next_instance_id(), numbering.next_abstract_id()),
            (3, 1)
        );
        assert!(!numbering.is_empty());
        assert!(Numbering::default().is_empty());
    }

    #[test]
    fn counting_items() {
        let level = |level: u8, format, text: &str| ListLevel {
            level,
            start: Some(1),
            format: Some(format),
            text: Some(text.to_string()),
            ..ListLevel::default()
        };
        let mut never = level(2, NumberFormat::LowerRoman, "%3)");
        never.restart = Some(0);
        let numbering = Numbering {
            abstracts: vec![AbstractNumbering {
                id: 0,
                levels: vec![
                    level(0, NumberFormat::Decimal, "%1."),
                    level(1, NumberFormat::LowerLetter, "%1.%2"),
                    never,
                ],
                ..Default::default()
            }],
            instances: vec![
                NumberingInstance {
                    id: 1,
                    abstract_id: 0,
                    ..Default::default()
                },
                NumberingInstance {
                    id: 2,
                    abstract_id: 0,
                    ..Default::default()
                },
                NumberingInstance {
                    id: 3,
                    abstract_id: 0,
                    overrides: vec![LevelOverride {
                        level: 0,
                        start: Some(10),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let mut counters = ListCounters::default();
        let mut next = |id, level| {
            counters
                .next(&numbering, id, level)
                .map(|l| l.text)
                .unwrap_or_else(|| "-".into())
        };
        assert_eq!(next(1, 0), "1.");
        assert_eq!(next(1, 1), "1.a");
        assert_eq!(next(1, 1), "1.b");
        assert_eq!(next(1, 2), "i)");
        assert_eq!(next(1, 0), "2.");
        // Второй уровень начат заново, третий — нет (`lvlRestart` 0).
        assert_eq!(next(1, 1), "2.a");
        assert_eq!(next(1, 2), "ii)");
        // Список с тем же абстрактным продолжает счёт.
        assert_eq!(next(2, 0), "3.");
        // Переопределение начала начинает заново.
        assert_eq!(next(3, 0), "10.");
        assert_eq!(next(3, 0), "11.");
        assert_eq!(next(1, 0), "12.");
        // Нет такого списка или уровня.
        assert_eq!(next(9, 0), "-");
        assert_eq!(next(1, 5), "-");
    }
}
