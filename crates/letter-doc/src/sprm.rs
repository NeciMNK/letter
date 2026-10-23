//! Свойства в двоичном виде (`Sprm`, MS-DOC 2.6): каждое — номер
//! и значение. Здесь они разбираются и переводятся в модель.

use letter_model::{
    Alignment, Color, FirstLineIndent, HalfPoints, Highlight, LineSpacing, Orientation,
    ParagraphProps, RunProps, SectionStart, TabLeader, TabStop, TabStopKind, Twips, Underline,
    VerticalAlign,
};

use crate::fib::{i16_at, u16_at};

/// Свойства подряд (`grpprl`): номер и значение без поля длины.
pub(crate) fn sprms(grpprl: &[u8]) -> Vec<(u16, &[u8])> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(sprm) = u16_at(grpprl, at) {
        at += 2;
        let (skip, len) = match sprm >> 13 {
            0 | 1 => (0, 1),
            2 | 4 | 5 => (0, 2),
            3 => (0, 4),
            7 => (0, 3),
            _ => match sprm {
                // sprmTDefTable: длина — два байта и на единицу больше.
                0xD608 | 0xD606 => match u16_at(grpprl, at) {
                    Some(cb) => (2, usize::from(cb).saturating_sub(1)),
                    None => break,
                },
                // sprmPChgTabs: длина 255 значит «посчитай сам».
                0xC615 if grpprl.get(at) == Some(&255) => {
                    let deleted = usize::from(*grpprl.get(at + 1).unwrap_or(&0));
                    let added_at = at + 2 + deleted * 4;
                    let added = usize::from(*grpprl.get(added_at).unwrap_or(&0));
                    (1, 1 + deleted * 4 + 1 + added * 3)
                }
                _ => match grpprl.get(at) {
                    Some(&cb) => (1, usize::from(cb)),
                    None => break,
                },
            },
        };
        let start = at + skip;
        let Some(operand) = grpprl.get(start..start + len) else {
            break;
        };
        out.push((sprm, operand));
        at = start + len;
    }
    out
}

fn u8_of(op: &[u8]) -> u8 {
    op.first().copied().unwrap_or(0)
}

fn u16_of(op: &[u8]) -> u16 {
    u16_at(op, 0).unwrap_or(0)
}

fn i16_of(op: &[u8]) -> i16 {
    i16_at(op, 0).unwrap_or(0)
}

/// Значение «переключателя» (полужирный и т. п.): 0 — выкл., 1 — вкл.,
/// 0x80 — как в стиле, 0x81 — наоборот (у стилей Word почти всегда
/// выключено, поэтому «наоборот» считаем включённым).
fn toggle(op: &[u8]) -> Option<bool> {
    match u8_of(op) {
        0 => Some(false),
        0x80 => None,
        _ => Some(true),
    }
}

/// Цвет по номеру из 16 цветов Word (`Ico`); 0 — «авто».
pub(crate) fn ico_color(ico: u8) -> Option<Color> {
    let (r, g, b) = match ico {
        0 => return Some(Color::Auto),
        1..=16 => ico_highlight(ico).rgb()?,
        _ => return None,
    };
    Some(Color::Rgb(r, g, b))
}

/// Цвет маркера по номеру из 16 цветов Word.
fn ico_highlight(ico: u8) -> Highlight {
    match ico {
        1 => Highlight::Black,
        2 => Highlight::Blue,
        3 => Highlight::Cyan,
        4 => Highlight::Green,
        5 => Highlight::Magenta,
        6 => Highlight::Red,
        7 => Highlight::Yellow,
        8 => Highlight::White,
        9 => Highlight::DarkBlue,
        10 => Highlight::DarkCyan,
        11 => Highlight::DarkGreen,
        12 => Highlight::DarkMagenta,
        13 => Highlight::DarkRed,
        14 => Highlight::DarkYellow,
        15 => Highlight::DarkGray,
        16 => Highlight::LightGray,
        _ => Highlight::None,
    }
}

fn underline(kul: u8) -> Underline {
    match kul {
        0 => Underline::None,
        2 => Underline::Words,
        3 => Underline::Double,
        4 => Underline::Dotted,
        6 => Underline::Thick,
        7 => Underline::Dash,
        9 => Underline::DotDash,
        10 => Underline::DotDotDash,
        11 => Underline::Wave,
        20 => Underline::DottedHeavy,
        23 => Underline::DashedHeavy,
        25 => Underline::DashDotHeavy,
        26 => Underline::DashDotDotHeavy,
        27 => Underline::WavyHeavy,
        39 => Underline::DashLong,
        43 => Underline::WavyDouble,
        55 => Underline::DashLongHeavy,
        _ => Underline::Single,
    }
}

/// Применяет свойство знаков. `fonts` — имена шрифтов по номерам,
/// `style_id` — id стиля знаков по номеру (`istd`).
pub(crate) fn apply_chp(
    props: &mut RunProps,
    sprm: u16,
    op: &[u8],
    fonts: &[String],
    style_id: &dyn Fn(u16) -> Option<String>,
) {
    let font = |op: &[u8]| fonts.get(usize::from(u16_of(op))).cloned();
    match sprm {
        0x0835 => props.bold = toggle(op),
        0x0836 => props.italic = toggle(op),
        0x0837 => props.strike = toggle(op),
        0x083A => props.small_caps = toggle(op),
        0x083B => props.caps = toggle(op),
        0x083C => props.hidden = toggle(op),
        0x2A53 => props.double_strike = toggle(op),
        0x2A3E => props.underline = Some(underline(u8_of(op))),
        0x4A43 => props.size = Some(HalfPoints(u32::from(u16_of(op)))),
        0x2A42 => props.color = ico_color(u8_of(op)),
        0x6870 => {
            props.color = match op {
                [_, _, _, 0xFF, ..] => Some(Color::Auto),
                [r, g, b, ..] => Some(Color::Rgb(*r, *g, *b)),
                _ => None,
            }
        }
        0x2A0C => props.highlight = Some(ico_highlight(u8_of(op))),
        0x2A48 => {
            props.vertical_align = Some(match u8_of(op) {
                1 => VerticalAlign::Superscript,
                2 => VerticalAlign::Subscript,
                _ => VerticalAlign::Baseline,
            })
        }
        0x8840 => props.spacing = Some(Twips(i32::from(i16_of(op)))),
        0x4845 => props.position = Some(i32::from(i16_of(op))),
        0x484B => props.kerning = Some(HalfPoints(u32::from(u16_of(op)))),
        0x4852 => props.scale = Some(u16_of(op)),
        0x4A4F => props.fonts.ascii = font(op),
        0x4A50 => props.fonts.east_asia = font(op),
        0x4A51 => props.fonts.h_ansi = font(op),
        0x4A5E => props.fonts.complex_script = font(op),
        0x4A30 => props.style = style_id(u16_of(op)),
        _ => {}
    }
}

/// Признаки абзаца, которых нет в модели: он в таблице, это конец
/// строки таблицы, ширины ячеек строки.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TableMarks {
    /// Абзац внутри таблицы.
    pub in_table: bool,
    /// Знак абзаца — конец строки таблицы.
    pub row_end: bool,
    /// Границы ячеек строки (`rgdxaCenter`), если заданы.
    pub cell_edges: Vec<i16>,
}

/// Применяет свойство абзаца. Номер стиля (`sprmPIstd`) здесь не
/// разбирается: он идёт в начале оформления отдельно.
pub(crate) fn apply_pap(props: &mut ParagraphProps, marks: &mut TableMarks, sprm: u16, op: &[u8]) {
    let twips = |op: &[u8]| Twips(i32::from(i16_of(op)));
    match sprm {
        0x2403 | 0x2461 => {
            props.alignment = Some(match u8_of(op) {
                1 => Alignment::Center,
                2 => Alignment::Right,
                3 => Alignment::Justify,
                4 => Alignment::Distribute,
                _ => Alignment::Left,
            })
        }
        0x840F | 0x845E => props.indentation.left = Some(twips(op)),
        0x840E | 0x845D => props.indentation.right = Some(twips(op)),
        0x8411 | 0x8460 => {
            let first = i32::from(i16_of(op));
            props.indentation.first_line = Some(if first < 0 {
                FirstLineIndent::Hanging(Twips(-first))
            } else {
                FirstLineIndent::FirstLine(Twips(first))
            });
        }
        0xA413 => props.spacing.before = Some(Twips(i32::from(u16_of(op)))),
        0xA414 => props.spacing.after = Some(Twips(i32::from(u16_of(op)))),
        0x6412 => {
            let line = i32::from(i16_of(op));
            let multiple = i16_at(op, 2).unwrap_or(0) != 0;
            props.spacing.line = Some(match line {
                n if multiple => LineSpacing::Multiple(n.unsigned_abs()),
                n if n < 0 => LineSpacing::Exact(Twips(-n)),
                n => LineSpacing::AtLeast(Twips(n)),
            });
        }
        0x2405 => props.keep_lines = Some(u8_of(op) != 0),
        0x2406 => props.keep_next = Some(u8_of(op) != 0),
        0x2407 => props.page_break_before = Some(u8_of(op) != 0),
        0x2431 => props.widow_control = Some(u8_of(op) != 0),
        0x246D => props.contextual_spacing = Some(u8_of(op) != 0),
        0x242A => props.suppress_auto_hyphens = Some(u8_of(op) != 0),
        0x2416 => marks.in_table = u8_of(op) != 0,
        0x6649 => marks.in_table = u16_of(op) > 0,
        0x2417 => marks.row_end = u8_of(op) != 0,
        0xD608 | 0xD606 => {
            let count = usize::from(u8_of(op));
            marks.cell_edges = (0..=count).filter_map(|i| i16_at(op, 1 + i * 2)).collect();
        }
        0xC60D | 0xC615 => change_tabs(&mut props.tabs, op),
        _ => {}
    }
}

/// Добавляет и убирает позиции табуляции (`PChgTabsPapxOperand`).
fn change_tabs(tabs: &mut Vec<TabStop>, op: &[u8]) {
    let deleted = usize::from(u8_of(op));
    for i in 0..deleted {
        if let Some(position) = i16_at(op, 1 + i * 2) {
            let position = Twips(i32::from(position));
            if tabs.iter().any(|t| t.position == position) {
                tabs.retain(|t| t.position != position);
            } else {
                // Позиция из стиля: в абзаце её надо снять явно.
                tabs.push(TabStop {
                    position,
                    kind: TabStopKind::Clear,
                    leader: TabLeader::None,
                });
            }
        }
    }
    let at = 1 + deleted * 2;
    let added = usize::from(*op.get(at).unwrap_or(&0));
    for i in 0..added {
        let (Some(position), Some(&tbd)) =
            (i16_at(op, at + 1 + i * 2), op.get(at + 1 + added * 2 + i))
        else {
            break;
        };
        let kind = match tbd & 7 {
            1 => TabStopKind::Center,
            2 => TabStopKind::Right,
            3 => TabStopKind::Decimal,
            4 => TabStopKind::Bar,
            _ => TabStopKind::Left,
        };
        let leader = match (tbd >> 3) & 7 {
            1 => TabLeader::Dot,
            2 => TabLeader::Hyphen,
            3 => TabLeader::Underscore,
            4 => TabLeader::Heavy,
            5 => TabLeader::MiddleDot,
            _ => TabLeader::None,
        };
        let position = Twips(i32::from(position));
        tabs.retain(|t| t.position != position);
        tabs.push(TabStop {
            position,
            kind,
            leader,
        });
    }
    tabs.sort_by_key(|t| t.position);
}

/// Свойства раздела: лист, поля, начало, колонки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sep {
    pub width: i32,
    pub height: i32,
    pub landscape: bool,
    pub left: i32,
    pub right: i32,
    pub top: i32,
    pub bottom: i32,
    pub header: i32,
    pub footer: i32,
    pub gutter: i32,
    pub start: SectionStart,
    pub columns: u16,
    pub column_space: i32,
    pub column_line: bool,
    pub title_page: bool,
}

impl Default for Sep {
    /// Значения по умолчанию формата (MS-DOC 2.6.4): лист Letter, поля
    /// 1 и 1,25 дюйма.
    fn default() -> Sep {
        Sep {
            width: 12240,
            height: 15840,
            landscape: false,
            left: 1800,
            right: 1800,
            top: 1440,
            bottom: 1440,
            header: 720,
            footer: 720,
            gutter: 0,
            start: SectionStart::NextPage,
            columns: 1,
            column_space: 720,
            column_line: false,
            title_page: false,
        }
    }
}

/// Применяет свойство раздела.
pub(crate) fn apply_sep(sep: &mut Sep, sprm: u16, op: &[u8]) {
    let value = i32::from(u16_of(op));
    let signed = i32::from(i16_of(op));
    match sprm {
        0xB01F => sep.width = value,
        0xB020 => sep.height = value,
        0x301D => sep.landscape = u8_of(op) == 2,
        0xB021 => sep.left = value,
        0xB022 => sep.right = value,
        0x9023 => sep.top = signed.abs(),
        0x9024 => sep.bottom = signed.abs(),
        0xB017 => sep.header = value,
        0xB018 => sep.footer = value,
        0xB025 => sep.gutter = value,
        0x3009 => {
            sep.start = match u8_of(op) {
                0 => SectionStart::Continuous,
                1 => SectionStart::NextColumn,
                3 => SectionStart::EvenPage,
                4 => SectionStart::OddPage,
                _ => SectionStart::NextPage,
            }
        }
        0x500B => sep.columns = u16_of(op).saturating_add(1),
        0x900C => sep.column_space = value,
        0x3019 => sep.column_line = u8_of(op) != 0,
        0x300A => sep.title_page = u8_of(op) != 0,
        _ => {}
    }
}

impl Sep {
    /// Ориентация листа.
    pub fn orientation(&self) -> Orientation {
        if self.landscape {
            Orientation::Landscape
        } else {
            Orientation::Portrait
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operands_are_split_by_their_size() {
        let mut grpprl = Vec::new();
        grpprl.extend(0x0835u16.to_le_bytes()); // полужирный, 1 байт
        grpprl.push(1);
        grpprl.extend(0x4A43u16.to_le_bytes()); // размер, 2 байта
        grpprl.extend(24u16.to_le_bytes());
        grpprl.extend(0xC60Du16.to_le_bytes()); // табуляция, длина впереди
        grpprl.extend([4, 0, 1, 0x40, 0x0B]);
        grpprl.extend(0xD608u16.to_le_bytes()); // ширины ячеек, длина 2 байта
        grpprl.extend(6u16.to_le_bytes());
        grpprl.extend([2, 0, 0, 0x10, 0x0E]);
        grpprl.extend(0x2416u16.to_le_bytes());
        let parsed = sprms(&grpprl);
        assert_eq!(parsed[0], (0x0835, &[1][..]));
        assert_eq!(parsed[1], (0x4A43, &[24, 0][..]));
        assert_eq!(parsed[2], (0xC60D, &[0, 1, 0x40, 0x0B][..]));
        assert_eq!(parsed[3], (0xD608, &[2, 0, 0, 0x10, 0x0E][..]));
        // Обрезанное в конце свойство пропускается.
        assert_eq!(parsed.len(), 4);
    }

    #[test]
    fn character_and_paragraph_properties() {
        let mut run = RunProps::default();
        let fonts = vec!["Times New Roman".to_string(), "Arial".to_string()];
        let none = |_: u16| None;
        apply_chp(&mut run, 0x0835, &[1], &fonts, &none);
        apply_chp(&mut run, 0x0836, &[0x80], &fonts, &none);
        apply_chp(&mut run, 0x4A51, &[1, 0], &fonts, &none);
        apply_chp(&mut run, 0x6870, &[0x12, 0x34, 0x56, 0], &fonts, &none);
        apply_chp(&mut run, 0x2A3E, &[3], &fonts, &none);
        assert_eq!(run.bold, Some(true));
        assert_eq!(run.italic, None);
        assert_eq!(run.fonts.h_ansi.as_deref(), Some("Arial"));
        assert_eq!(run.color, Some(Color::Rgb(0x12, 0x34, 0x56)));
        assert_eq!(run.underline, Some(Underline::Double));

        let mut para = ParagraphProps::default();
        let mut marks = TableMarks::default();
        apply_pap(&mut para, &mut marks, 0x2461, &[1]);
        apply_pap(&mut para, &mut marks, 0x8460, &(-360i16).to_le_bytes());
        apply_pap(&mut para, &mut marks, 0x6412, &[0xE0, 0x01, 1, 0]);
        apply_pap(&mut para, &mut marks, 0xC60D, &[0, 1, 0x40, 0x0B, 0x0A]);
        apply_pap(&mut para, &mut marks, 0x2416, &[1]);
        assert_eq!(para.alignment, Some(Alignment::Center));
        assert_eq!(
            para.indentation.first_line,
            Some(FirstLineIndent::Hanging(Twips(360)))
        );
        assert_eq!(para.spacing.line, Some(LineSpacing::Multiple(480)));
        assert_eq!(
            para.tabs,
            [TabStop {
                position: Twips(2880),
                kind: TabStopKind::Right,
                leader: TabLeader::Dot
            }]
        );
        assert!(marks.in_table);
    }
}
