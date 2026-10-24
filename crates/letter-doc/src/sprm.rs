//! Свойства в двоичном виде (`Sprm`, MS-DOC 2.6): каждое — номер
//! и значение. Здесь они разбираются и переводятся в модель.

use letter_model::{
    Alignment, Border, Borders, Color, FirstLineIndent, HalfPoints, HeightRule, Highlight,
    LineSpacing, NumberingRef, Orientation, ParagraphBorders, ParagraphProps, RowHeight, RowProps,
    RunProps, SectionStart, Shading, TabLeader, TabStop, TabStopKind, Twips, Underline,
    VerticalAlign,
};

use crate::fib::{i16_at, u16_at, u32_at};

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
/// строки таблицы, ширины ячеек и оформление строки.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TableMarks {
    /// Абзац внутри таблицы.
    pub in_table: bool,
    /// Знак абзаца — конец строки таблицы.
    pub row_end: bool,
    /// Границы ячеек строки (`rgdxaCenter`), если заданы.
    pub cell_edges: Vec<i16>,
    /// Свойства строки: высота, строка-заголовок.
    pub row: RowProps,
    /// Границы и заливка ячеек строки по порядку.
    pub cells: Vec<CellLook>,
    /// Границы всей таблицы.
    pub table_borders: Option<Borders>,
    /// Стороны абзаца, заданные новыми свойствами (`Brc`): старые
    /// (`Brc80`, только 16 цветов) их уже не меняют.
    exact_borders: u8,
    /// Заливка абзаца задана новым свойством (`Shd`).
    exact_shading: bool,
}

/// Границы и заливка одной ячейки.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct CellLook {
    pub borders: Borders,
    /// Заливка из нового свойства (`Shd`, любой цвет).
    pub shading: Option<Shading>,
    /// Заливка из старого (`Shd80`, 16 цветов) — если нового нет.
    pub shading80: Option<Shading>,
}

impl TableMarks {
    /// Ячейка номер `i`; недостающие добавляются.
    fn cell(&mut self, i: usize) -> &mut CellLook {
        if self.cells.len() <= i {
            self.cells.resize_with(i + 1, CellLook::default);
        }
        &mut self.cells[i]
    }
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
        0xC60D | 0xC615 => change_tabs(&mut props.tabs, op),
        // Список (`ilfo`, номер с 1; 0 — не в списке) и уровень.
        0x460B => {
            let ilfo = u16_of(op);
            let id = if ilfo < 0x07FF { u32::from(ilfo) } else { 0 };
            let level = props.numbering.and_then(|n| n.level);
            props.numbering = Some(NumberingRef {
                id: Some(id),
                level,
            });
        }
        0x260A => {
            let id = props.numbering.and_then(|n| n.id);
            props.numbering = Some(NumberingRef {
                id,
                level: Some(u8_of(op).min(8)),
            });
        }
        // Границы абзаца: новые (`Brc`) и старые (`Brc80`).
        0xC64E..=0xC653 | 0x6424..=0x6428 | 0x6629 => {
            let (side, border, exact) = match sprm {
                0xC64E..=0xC653 => (usize::from(sprm - 0xC64E), brc(op), true),
                0x6629 => (5, brc80(op), false),
                _ => (usize::from(sprm - 0x6424), brc80(op), false),
            };
            let bit = 1 << side;
            if !exact && marks.exact_borders & bit != 0 {
                return;
            }
            if exact {
                marks.exact_borders |= bit;
            }
            let borders = props.borders.get_or_insert_with(ParagraphBorders::default);
            let slot = match side {
                0 => &mut borders.top,
                1 => &mut borders.left,
                2 => &mut borders.bottom,
                3 => &mut borders.right,
                4 => &mut borders.between,
                _ => &mut borders.bar,
            };
            *slot = border;
            if borders.is_empty() {
                props.borders = None;
            }
        }
        0xC64D => {
            props.shading = shd(op);
            marks.exact_shading = true;
        }
        0x442D if !marks.exact_shading => props.shading = shd80(u16_of(op)),
        _ => apply_tap(marks, sprm, op),
    }
}

/// Свойства строки таблицы (`TAP`): ширины, границы и заливка ячеек,
/// высота строки.
fn apply_tap(marks: &mut TableMarks, sprm: u16, op: &[u8]) {
    match sprm {
        0xD608 | 0xD606 => {
            let count = usize::from(u8_of(op));
            marks.cell_edges = (0..=count).filter_map(|i| i16_at(op, 1 + i * 2)).collect();
            // За границами — ячейки (`TC80`, по 20 байт): флаги, ширина
            // и четыре старые границы.
            let cells = 1 + (count + 1) * 2;
            for i in 0..count {
                let Some(tc) = op.get(cells + i * 20..cells + (i + 1) * 20) else {
                    break;
                };
                let side = |n: usize| tc.get(4 + n * 4..8 + n * 4).and_then(brc80);
                let cell = marks.cell(i);
                cell.borders.top = side(0);
                cell.borders.left = side(1);
                cell.borders.bottom = side(2);
                cell.borders.right = side(3);
            }
        }
        // Границы части ячеек: с какой по какую, какие стороны, линия.
        0xD62F | 0xD620 => {
            let (first, last, sides) = (u8_of(op), op.get(1), op.get(2));
            let (Some(&last), Some(&sides)) = (last, sides) else {
                return;
            };
            let rest = op.get(3..).unwrap_or_default();
            let border = if sprm == 0xD62F {
                brc(rest)
            } else {
                brc80(rest)
            };
            for i in usize::from(first)..usize::from(last).min(64) {
                let cell = marks.cell(i);
                let slots = [
                    (1, &mut cell.borders.top),
                    (2, &mut cell.borders.left),
                    (4, &mut cell.borders.bottom),
                    (8, &mut cell.borders.right),
                ];
                for (bit, slot) in slots {
                    if sides & bit != 0 {
                        slot.clone_from(&border);
                    }
                }
            }
        }
        // Заливка ячеек: новая (`Shd` по 10 байт, три части по 22 ячейки)
        // и старая (`Shd80` по 2 байта).
        0xD612 | 0xD616 | 0xD60C => {
            let first = match sprm {
                0xD612 => 0,
                0xD616 => 22,
                _ => 44,
            };
            for (i, shd_bytes) in op.chunks_exact(10).enumerate() {
                marks.cell(first + i).shading = shd(shd_bytes);
            }
        }
        0xD609 => {
            for (i, shd_bytes) in op.chunks_exact(2).enumerate() {
                marks.cell(i).shading80 = shd80(u16_of(shd_bytes));
            }
        }
        0xD613 | 0xD605 => {
            let size = if sprm == 0xD613 { 8 } else { 4 };
            let side = |n: usize| {
                let bytes = op.get(n * size..(n + 1) * size)?;
                if sprm == 0xD613 {
                    brc(bytes)
                } else {
                    brc80(bytes)
                }
            };
            let borders = Borders {
                top: side(0),
                left: side(1),
                bottom: side(2),
                right: side(3),
                inside_h: side(4),
                inside_v: side(5),
            };
            marks.table_borders = (!borders.is_empty()).then_some(borders);
        }
        0x9407 => {
            let height = i32::from(i16_of(op));
            marks.row.height = (height != 0).then(|| RowHeight {
                value: Twips(height.abs()),
                rule: if height < 0 {
                    HeightRule::Exact
                } else {
                    HeightRule::AtLeast
                },
            });
        }
        0x3404 => marks.row.header = (u8_of(op) != 0).then_some(true),
        0x3403 | 0x3466 => marks.row.cant_split = (u8_of(op) != 0).then_some(true),
        _ => {}
    }
}

/// Вид линии по номеру (`brcType`) — имя, как в .docx.
fn border_style(kind: u8) -> &'static str {
    match kind {
        2 => "thick",
        3 => "double",
        6 => "dotted",
        7 => "dashed",
        8 => "dotDash",
        9 => "dotDotDash",
        10 => "triple",
        11 => "thinThickSmallGap",
        12 => "thickThinSmallGap",
        13 => "thinThickThinSmallGap",
        14 => "thinThickMediumGap",
        15 => "thickThinMediumGap",
        16 => "thinThickThinMediumGap",
        17 => "thinThickLargeGap",
        18 => "thickThinLargeGap",
        19 => "thinThickThinLargeGap",
        20 => "wave",
        21 => "doubleWave",
        22 => "dashSmallGap",
        23 => "dashDotStroked",
        24 => "threeDEmboss",
        25 => "threeDEngrave",
        26 => "outset",
        27 => "inset",
        _ => "single",
    }
}

/// Линия из толщины, вида, отступа и цвета; вид 0 — линии нет (не
/// задана), 0xFF — убрана.
fn border(width: u8, kind: u8, flags: u8, color: Option<Color>) -> Option<Border> {
    match kind {
        0 => None,
        0xFF => Some(Border {
            style: "nil".into(),
            size: None,
            space: None,
            color: None,
        }),
        kind => Some(Border {
            style: border_style(kind).into(),
            size: Some(u32::from(width)),
            space: Some(u32::from(flags & 0x1F)),
            color,
        }),
    }
}

/// Цвет `COLORREF`: красный, зелёный, синий и признак «авто».
fn colorref(op: &[u8]) -> Option<Color> {
    match op {
        [_, _, _, 0xFF, ..] => Some(Color::Auto),
        [r, g, b, _, ..] => Some(Color::Rgb(*r, *g, *b)),
        _ => None,
    }
}

/// Граница `Brc` (8 байт): цвет, толщина, вид, отступ.
pub(crate) fn brc(op: &[u8]) -> Option<Border> {
    let bytes = op.get(..8)?;
    if u32_at(bytes, 0) == Some(0xFFFF_FFFF) {
        return border(0, 0xFF, 0, None);
    }
    border(bytes[4], bytes[5], bytes[6], colorref(bytes))
}

/// Старая граница `Brc80` (4 байта): толщина, вид, цвет из 16, отступ.
pub(crate) fn brc80(op: &[u8]) -> Option<Border> {
    let bytes = op.get(..4)?;
    if u32_at(bytes, 0) == Some(0xFFFF_FFFF) {
        return border(0, 0xFF, 0, None);
    }
    border(bytes[0], bytes[1], bytes[3], ico_color(bytes[2]))
}

/// Узор заливки по номеру (`ipat`) — имя, как в .docx.
fn shading_pattern(ipat: u16) -> &'static str {
    match ipat {
        1 => "solid",
        2 => "pct5",
        3 => "pct10",
        4 => "pct20",
        5 => "pct25",
        6 => "pct30",
        7 => "pct40",
        8 => "pct50",
        9 => "pct60",
        10 => "pct70",
        11 => "pct75",
        12 => "pct80",
        13 => "pct90",
        14 => "horzStripe",
        15 => "vertStripe",
        16 => "reverseDiagStripe",
        17 => "diagStripe",
        18 => "horzCross",
        19 => "diagCross",
        20 => "thinHorzStripe",
        21 => "thinVertStripe",
        22 => "thinReverseDiagStripe",
        23 => "thinDiagStripe",
        24 => "thinHorzCross",
        25 => "thinDiagCross",
        0xFFFF => "nil",
        _ => "clear",
    }
}

/// Заливка из узора и цветов; пустая (без узора и цвета) — `None`.
fn shading(ipat: u16, color: Option<Color>, fill: Option<Color>) -> Option<Shading> {
    let auto = |c: Option<Color>| matches!(c, None | Some(Color::Auto));
    if ipat == 0 && auto(color) && auto(fill) {
        return None;
    }
    Some(Shading {
        pattern: shading_pattern(ipat).into(),
        color,
        fill,
    })
}

/// Заливка `Shd` (10 байт): цвет узора, цвет фона, узор.
pub(crate) fn shd(op: &[u8]) -> Option<Shading> {
    let bytes = op.get(..10)?;
    shading(
        u16_at(bytes, 8)?,
        colorref(&bytes[..4]),
        colorref(&bytes[4..8]),
    )
}

/// Старая заливка `Shd80`: цвета из 16 и узор в двух байтах.
pub(crate) fn shd80(value: u16) -> Option<Shading> {
    if value == 0xFFFF {
        return shading(0xFFFF, None, None);
    }
    let ico = |shift: u16| ico_color(((value >> shift) & 0x1F) as u8);
    shading(value >> 10, ico(0), ico(5))
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

    #[test]
    fn borders_and_shading() {
        // Новая граница: цвет 1F4E79, 1 пт, одинарная, отступ 4 пт.
        let border = brc(&[0x1F, 0x4E, 0x79, 0, 8, 1, 4, 0]).unwrap();
        assert_eq!(border.style, "single");
        assert_eq!(border.size, Some(8));
        assert_eq!(border.space, Some(4));
        assert_eq!(border.color, Some(Color::Rgb(0x1F, 0x4E, 0x79)));
        assert_eq!(brc(&[0; 8]), None);
        assert_eq!(brc80(&[6, 3, 0, 1]).unwrap().style, "double");
        assert!(brc80(&[0xFF; 4]).unwrap().is_none());
        // Заливка: узор «нет», фон FFF2CC.
        let fill = shd(&[0, 0, 0, 0xFF, 0xFF, 0xF2, 0xCC, 0, 0, 0]).unwrap();
        assert_eq!(fill.fill, Some(Color::Rgb(0xFF, 0xF2, 0xCC)));
        assert_eq!(fill.pattern, "clear");
        assert_eq!(shd(&[0, 0, 0, 0xFF, 0, 0, 0, 0xFF, 0, 0]), None);
        // Старая: фон — жёлтый (7) из 16 цветов.
        assert_eq!(shd80(7 << 5).unwrap().fill, ico_color(7));

        // Новая граница абзаца главнее старой, в каком бы порядке ни шли.
        let mut para = ParagraphProps::default();
        let mut marks = TableMarks::default();
        apply_pap(
            &mut para,
            &mut marks,
            0xC64E,
            &[0x12, 0x34, 0x56, 0, 8, 1, 0, 0],
        );
        apply_pap(&mut para, &mut marks, 0x6424, &[8, 1, 2, 0]);
        let top = para.borders.unwrap().top.unwrap();
        assert_eq!(top.color, Some(Color::Rgb(0x12, 0x34, 0x56)));
    }

    #[test]
    fn table_row_properties() {
        let mut para = ParagraphProps::default();
        let mut marks = TableMarks::default();
        apply_pap(&mut para, &mut marks, 0x9407, &(-600i16).to_le_bytes());
        apply_pap(&mut para, &mut marks, 0x3404, &[1]);
        // Правая граница у ячеек 0 и 1.
        apply_pap(
            &mut para,
            &mut marks,
            0xD62F,
            &[0, 2, 8, 0xC0, 0, 0, 0, 8, 1, 0, 0],
        );
        let mut shading = vec![0, 0, 0, 0xFF, 0xD9, 0xE2, 0xF3, 0, 0, 0];
        shading.extend([0, 0, 0, 0xFF, 0, 0, 0, 0xFF, 0, 0]);
        apply_pap(&mut para, &mut marks, 0xD612, &shading);
        assert_eq!(
            marks.row.height,
            Some(RowHeight {
                value: Twips(600),
                rule: HeightRule::Exact
            })
        );
        assert_eq!(marks.row.header, Some(true));
        assert_eq!(marks.cells.len(), 2);
        assert!(marks.cells[1].borders.right.is_some());
        assert!(marks.cells[1].borders.left.is_none());
        assert!(marks.cells[0].shading.is_some());
        assert!(marks.cells[1].shading.is_none());
    }

    #[test]
    fn list_reference() {
        let mut para = ParagraphProps::default();
        let mut marks = TableMarks::default();
        apply_pap(&mut para, &mut marks, 0x260A, &[2]);
        apply_pap(&mut para, &mut marks, 0x460B, &[3, 0]);
        assert_eq!(
            para.numbering,
            Some(NumberingRef {
                id: Some(3),
                level: Some(2)
            })
        );
    }
}
