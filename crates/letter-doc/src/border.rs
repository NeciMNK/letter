//! Границы (`Brc80`, `Brc`) и заливка (`Shd80`, `Shd`) у абзацев, таблиц
//! и ячеек.

use letter_model::{Border, Color, Shading};

use crate::fib::u16_at;
use crate::sprm::ico_color;

/// Цвет `COLORREF`: красный, зелёный, синий и байт «авто» (0xFF).
pub(crate) fn colorref(bytes: &[u8]) -> Option<Color> {
    match bytes {
        [_, _, _, 0xFF, ..] => Some(Color::Auto),
        [r, g, b, _, ..] => Some(Color::Rgb(*r, *g, *b)),
        _ => None,
    }
}

/// Вид линии по номеру `brcType`.
fn style(kind: u8) -> &'static str {
    match kind {
        2 => "thick",
        3 => "double",
        5 => "hairline",
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

/// Линия из толщины (восьмые доли пункта), вида, отступа и цвета.
/// Вид 0 и 0xFF — линии нет (`None`).
fn border(width: u8, kind: u8, space: u8, color: Option<Color>) -> Option<Border> {
    if kind == 0 || kind == 0xFF {
        return None;
    }
    Some(Border {
        style: style(kind).into(),
        size: Some(u32::from(width)),
        space: Some(u32::from(space & 0x1F)),
        color,
    })
}

/// Старая запись линии, 4 байта: толщина, вид, цвет из палитры, отступ.
pub(crate) fn brc80(bytes: &[u8]) -> Option<Border> {
    let [width, kind, ico, space] = *bytes.get(..4)? else {
        return None;
    };
    border(width, kind, space, ico_color(ico))
}

/// Новая запись линии, 8 байт: цвет RGB, толщина, вид, отступ.
pub(crate) fn brc(bytes: &[u8]) -> Option<Border> {
    let bytes = bytes.get(..8)?;
    border(bytes[4], bytes[5], bytes[6], colorref(bytes))
}

/// Имя узора заливки по номеру `ipat`.
fn pattern(ipat: u16) -> &'static str {
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
        _ => "clear",
    }
}

/// Заливка из цвета узора, цвета фона и узора; пустая — `None`.
fn shading(color: Option<Color>, fill: Option<Color>, ipat: u16) -> Option<Shading> {
    let fill = fill.filter(|c| *c != Color::Auto);
    if ipat == 0xFFFF || (ipat == 0 && fill.is_none()) {
        return None;
    }
    Some(Shading {
        pattern: pattern(ipat).into(),
        color,
        fill,
    })
}

/// Старая заливка, 2 байта: цвета из палитры и узор.
pub(crate) fn shd80(bytes: &[u8]) -> Option<Shading> {
    let value = u16_at(bytes, 0)?;
    shading(
        ico_color((value & 0x1F) as u8),
        ico_color(((value >> 5) & 0x1F) as u8),
        value >> 10,
    )
}

/// Новая заливка, 10 байт: цвет узора, цвет фона (RGB) и узор.
pub(crate) fn shd(bytes: &[u8]) -> Option<Shading> {
    shading(
        colorref(bytes.get(..4)?),
        colorref(bytes.get(4..8)?),
        u16_at(bytes, 8)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borders_and_shading() {
        let line = brc80(&[12, 1, 6, 4]).unwrap();
        assert_eq!(line.style, "single");
        assert_eq!(line.size, Some(12));
        assert_eq!(line.color, Some(Color::Rgb(255, 0, 0)));
        assert!(brc80(&[0, 0, 0, 0]).is_none());
        assert!(brc80(&[0xFF; 4]).is_none());
        let line = brc(&[1, 2, 3, 0, 4, 3, 0, 0]).unwrap();
        assert_eq!(line.style, "double");
        assert_eq!(line.color, Some(Color::Rgb(1, 2, 3)));
        // Жёлтый фон без узора.
        let fill = shd(&[0, 0, 0, 0xFF, 255, 255, 0, 0, 0, 0]).unwrap();
        assert_eq!(fill.fill, Some(Color::Rgb(255, 255, 0)));
        assert!(shd(&[0, 0, 0, 0xFF, 0, 0, 0, 0xFF, 0, 0]).is_none());
        // Палитра: фон 7 (жёлтый), узор 1 (сплошной).
        let fill = shd80(&(7u16 << 5 | 1 << 10).to_le_bytes()).unwrap();
        assert_eq!(fill.pattern, "solid");
    }
}
