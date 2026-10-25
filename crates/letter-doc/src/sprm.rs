//! Свойства в .doc записаны списками «sprm» (single property modifier):
//! двухбайтовый код свойства и значение. Длина значения — в трёх старших
//! битах кода (`spra`). Здесь — разбор списка и перевод понятных
//! свойств в свойства модели.

use letter_model::{
    Alignment, Color, FirstLineIndent, HalfPoints, Highlight, LineSpacing, ParagraphProps,
    RunProps, TabLeader, TabStop, TabStopKind, Twips, Underline, VerticalAlign,
};

use crate::fib::u16_at;

/// Свойство: код и значение.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sprm<'a> {
    /// Код.
    pub code: u16,
    /// Значение.
    pub operand: &'a [u8],
}

impl Sprm<'_> {
    pub fn byte(&self) -> u8 {
        self.operand.first().copied().unwrap_or(0)
    }

    pub fn word(&self) -> u16 {
        u16_at(self.operand, 0).unwrap_or(0)
    }

    pub fn signed(&self) -> i16 {
        self.word() as i16
    }
}

/// Коды свойств с особой записью длины.
pub(crate) mod code {
    /// Описание строки таблицы.
    pub const T_DEF_TABLE: u16 = 0xD608;
    /// Изменение табуляций.
    pub const P_CHG_TABS: u16 = 0xC615;
}

/// Длина значения свойства `code`, если она задана кодом (без
/// переменных).
fn fixed_size(code: u16) -> Option<usize> {
    match code >> 13 {
        0 | 1 => Some(1),
        2 | 4 | 5 => Some(2),
        3 => Some(4),
        7 => Some(3),
        _ => None,
    }
}

/// Разбирает список свойств. Обрывок в конце пропускается.
pub(crate) fn sprms(mut data: &[u8]) -> Vec<Sprm<'_>> {
    let mut out = Vec::new();
    while data.len() >= 2 {
        let code = u16::from_le_bytes([data[0], data[1]]);
        let rest = &data[2..];
        let size = match fixed_size(code) {
            Some(size) => size,
            None if code == code::T_DEF_TABLE => {
                // Два байта длины, которая на 1 больше остатка.
                match u16_at(rest, 0) {
                    Some(cb) => 2 + usize::from(cb).saturating_sub(1),
                    None => break,
                }
            }
            None if code == code::P_CHG_TABS => match rest.first() {
                Some(255) => chg_tabs_size(rest),
                Some(&cb) => 1 + usize::from(cb),
                None => break,
            },
            None => match rest.first() {
                Some(&cb) => 1 + usize::from(cb),
                None => break,
            },
        };
        let Some(operand) = rest.get(..size) else {
            break;
        };
        out.push(Sprm { code, operand });
        data = &rest[size..];
    }
    out
}

/// Длина значения `sprmPChgTabs` с особой длиной 255: удаления (с
/// допусками) и добавления.
fn chg_tabs_size(rest: &[u8]) -> usize {
    let deleted = usize::from(rest.get(1).copied().unwrap_or(0));
    let added_at = 2 + deleted * 4;
    let added = usize::from(rest.get(added_at).copied().unwrap_or(0));
    added_at + 1 + added * 3
}

/// Значение «вкл/выкл» знакового свойства: 0 — выкл, 1 — вкл,
/// 0x80 — как в стиле, 0x81 — наоборот (считаем «вкл»).
fn toggle(value: u8) -> Option<bool> {
    match value {
        0 => Some(false),
        1 | 0x81 => Some(true),
        _ => None,
    }
}

/// Цвет по номеру из палитры Word 97 (`ico`).
pub(crate) fn ico_color(ico: u8) -> Option<Color> {
    const PALETTE: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (0, 0, 255),
        (0, 255, 255),
        (0, 255, 0),
        (255, 0, 255),
        (255, 0, 0),
        (255, 255, 0),
        (255, 255, 255),
        (0, 0, 128),
        (0, 128, 128),
        (0, 128, 0),
        (128, 0, 128),
        (128, 0, 0),
        (128, 128, 0),
        (128, 128, 128),
        (192, 192, 192),
    ];
    match ico {
        0 => Some(Color::Auto),
        1..=16 => {
            let (r, g, b) = PALETTE[usize::from(ico - 1)];
            Some(Color::Rgb(r, g, b))
        }
        _ => None,
    }
}

fn ico_highlight(ico: u8) -> Option<Highlight> {
    Some(match ico {
        0 => Highlight::None,
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
        _ => return None,
    })
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
        20 => Underline::DottedHeavy,
        23 => Underline::DashedHeavy,
        39 => Underline::DashLong,
        55 => Underline::DashLongHeavy,
        _ => Underline::Single,
    }
}

/// Шрифты документа по номерам (`SttbfFfn`) и стили по номерам — чтобы
/// перевести номера в свойствах в имена.
pub(crate) struct Names<'a> {
    pub fonts: &'a [String],
    pub styles: &'a [Option<String>],
}

impl Names<'_> {
    fn font(&self, sprm: &Sprm) -> Option<String> {
        self.fonts.get(usize::from(sprm.word())).cloned()
    }

    pub fn style(&self, istd: u16) -> Option<String> {
        self.styles.get(usize::from(istd)).cloned().flatten()
    }
}

/// Применяет к свойствам знаков понятные свойства из списка.
pub(crate) fn apply_run(props: &mut RunProps, list: &[Sprm], names: &Names) {
    for sprm in list {
        match sprm.code {
            0x0835 => props.bold = toggle(sprm.byte()),
            0x0836 => props.italic = toggle(sprm.byte()),
            0x0837 => props.strike = toggle(sprm.byte()),
            0x2A53 => props.double_strike = toggle(sprm.byte()),
            0x083A => props.small_caps = toggle(sprm.byte()),
            0x083B => props.caps = toggle(sprm.byte()),
            0x083C => props.hidden = toggle(sprm.byte()),
            0x2A3E => props.underline = Some(underline(sprm.byte())),
            0x4A43 => props.size = Some(HalfPoints(u32::from(sprm.word()))),
            0x2A42 => {
                if let Some(color) = ico_color(sprm.byte()) {
                    props.color = Some(color);
                }
            }
            0x6870 => {
                let [r, g, b, auto] = sprm.operand else {
                    continue;
                };
                props.color = Some(if *auto == 0xFF {
                    Color::Auto
                } else {
                    Color::Rgb(*r, *g, *b)
                });
            }
            0x2A0C => props.highlight = ico_highlight(sprm.byte()),
            0x2A48 => {
                props.vertical_align = Some(match sprm.byte() {
                    1 => VerticalAlign::Superscript,
                    2 => VerticalAlign::Subscript,
                    _ => VerticalAlign::Baseline,
                });
            }
            0x4A4F => {
                if let Some(font) = names.font(sprm) {
                    props.fonts.ascii = Some(font);
                }
            }
            0x4A50 => props.fonts.east_asia = names.font(sprm),
            0x4A51 => props.fonts.h_ansi = names.font(sprm),
            0x4A5E => props.fonts.complex_script = names.font(sprm),
            0x4A30 => props.style = names.style(sprm.word()).filter(|_| sprm.word() != 10),
            0x8840 => props.spacing = Some(Twips(i32::from(sprm.signed()))),
            0x4852 => props.scale = Some(sprm.word()),
            0x4845 => props.position = Some(i32::from(sprm.signed())),
            _ => {}
        }
    }
    // Шрифт без отдельного «прочего» — тот же для всех знаков.
    if props.fonts.h_ansi.is_none() {
        props.fonts.h_ansi = props.fonts.ascii.clone();
    }
}

/// Применяет к свойствам абзаца понятные свойства из списка.
pub(crate) fn apply_paragraph(props: &mut ParagraphProps, list: &[Sprm]) {
    for sprm in list {
        match sprm.code {
            0x2403 | 0x2461 => {
                props.alignment = Some(match sprm.byte() {
                    1 => Alignment::Center,
                    2 => Alignment::Right,
                    3 => Alignment::Justify,
                    4 => Alignment::Distribute,
                    _ => Alignment::Left,
                });
            }
            0x2405 => props.keep_lines = toggle(sprm.byte()),
            0x2406 => props.keep_next = toggle(sprm.byte()),
            0x2407 => props.page_break_before = toggle(sprm.byte()),
            0x2431 => props.widow_control = toggle(sprm.byte()),
            0x246D => props.contextual_spacing = toggle(sprm.byte()),
            0x840F | 0x845E => props.indentation.left = Some(Twips(i32::from(sprm.signed()))),
            0x840E | 0x845D => props.indentation.right = Some(Twips(i32::from(sprm.signed()))),
            0x8411 | 0x8460 => {
                let value = i32::from(sprm.signed());
                props.indentation.first_line = Some(if value < 0 {
                    FirstLineIndent::Hanging(Twips(-value))
                } else {
                    FirstLineIndent::FirstLine(Twips(value))
                });
            }
            0xA413 => props.spacing.before = Some(Twips(i32::from(sprm.word()))),
            0xA414 => props.spacing.after = Some(Twips(i32::from(sprm.word()))),
            0x6412 => {
                let line = i32::from(sprm.signed());
                let multiple = u16_at(sprm.operand, 2).unwrap_or(0) != 0;
                props.spacing.line = Some(if multiple {
                    LineSpacing::Multiple(line.unsigned_abs())
                } else if line < 0 {
                    LineSpacing::Exact(Twips(-line))
                } else {
                    LineSpacing::AtLeast(Twips(line))
                });
            }
            0xC60D | code::P_CHG_TABS => change_tabs(&mut props.tabs, sprm.operand),
            _ => {}
        }
    }
}

/// Табуляции: удалить и добавить (`PChgTabsPapxOperand`).
fn change_tabs(tabs: &mut Vec<TabStop>, operand: &[u8]) {
    let Some(data) = operand.get(1..) else {
        return;
    };
    let deleted = usize::from(data.first().copied().unwrap_or(0));
    for i in 0..deleted {
        if let Some(at) = u16_at(data, 1 + 2 * i) {
            let at = i32::from(at as i16);
            tabs.retain(|t| t.position.0 != at);
        }
    }
    let added_at = 1 + 2 * deleted;
    let added = usize::from(data.get(added_at).copied().unwrap_or(0));
    for i in 0..added {
        let (Some(at), Some(&tbd)) = (
            u16_at(data, added_at + 1 + 2 * i),
            data.get(added_at + 1 + 2 * added + i),
        ) else {
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
        let position = Twips(i32::from(at as i16));
        tabs.retain(|t| t.position != position);
        tabs.push(TabStop {
            position,
            kind,
            leader,
        });
    }
    tabs.sort_by_key(|t| t.position.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sprm_lists_are_split_by_size() {
        // Полужирный (1 байт), размер (2), цвет RGB (4), переменная длина.
        let data = [
            0x35, 0x08, 0x01, 0x43, 0x4A, 0x18, 0x00, 0x70, 0x68, 1, 2, 3, 0, 0x0D, 0xC6, 2, 9, 9,
        ];
        let list = sprms(&data);
        let codes: Vec<u16> = list.iter().map(|s| s.code).collect();
        assert_eq!(codes, [0x0835, 0x4A43, 0x6870, 0xC60D]);
        assert_eq!(list[3].operand, [2, 9, 9]);
        let names = Names {
            fonts: &[],
            styles: &[],
        };
        let mut run = RunProps::default();
        apply_run(&mut run, &list, &names);
        assert_eq!(run.bold, Some(true));
        assert_eq!(run.size, Some(HalfPoints(24)));
        assert_eq!(run.color, Some(Color::Rgb(1, 2, 3)));
        // Обрывок в конце не мешает.
        assert_eq!(sprms(&[0x35, 0x08]).len(), 0);
    }

    #[test]
    fn tabs_and_line_spacing() {
        let mut props = ParagraphProps::default();
        // Две табуляции: 720 слева и 4320 справа с точками.
        let operand = [8, 0, 2, 0xD0, 0x02, 0xE0, 0x10, 0x00, 0x0A];
        let spacing = [0x68, 0x01, 0x01, 0x00];
        let list = [
            Sprm {
                code: 0xC60D,
                operand: &operand,
            },
            Sprm {
                code: 0x6412,
                operand: &spacing,
            },
        ];
        apply_paragraph(&mut props, &list);
        assert_eq!(props.tabs.len(), 2);
        assert_eq!(props.tabs[1].kind, TabStopKind::Right);
        assert_eq!(props.tabs[1].leader, TabLeader::Dot);
        assert_eq!(props.spacing.line, Some(LineSpacing::Multiple(360)));
    }
}
