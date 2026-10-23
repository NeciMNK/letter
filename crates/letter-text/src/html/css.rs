//! Значения CSS из атрибута `style`: длины, цвета, шрифты.

use letter_model::{Color, Highlight};

/// Объявления `свойство: значение` из атрибута `style` (имена строчными).
pub fn declarations(style: &str) -> Vec<(String, String)> {
    style
        .split(';')
        .filter_map(|item| {
            let (name, value) = item.split_once(':')?;
            let value = value.trim();
            let value = value
                .strip_suffix("!important")
                .map_or(value, str::trim_end);
            Some((name.trim().to_ascii_lowercase(), value.to_owned()))
        })
        .collect()
}

/// Длина в двадцатых долях пункта. `em` и `%` считаются от `font_size`
/// (в половинах пункта). Число без единиц — пиксели.
pub fn length(value: &str, font_size: u32) -> Option<i32> {
    let value = value.trim().to_ascii_lowercase();
    let split = value
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(value.len());
    let number: f64 = value[..split].parse().ok()?;
    let points = match value[split..].trim() {
        "pt" => number,
        "px" | "" => number * 0.75,
        "in" => number * 72.0,
        "cm" => number * 72.0 / 2.54,
        "mm" => number * 72.0 / 25.4,
        "pc" => number * 12.0,
        "em" | "rem" => number * f64::from(font_size) / 2.0,
        "%" => number / 100.0 * f64::from(font_size) / 2.0,
        _ => return None,
    };
    let twips = (points * 20.0).round();
    twips
        .is_finite()
        .then(|| twips.clamp(-31_680.0, 31_680.0) as i32)
}

/// Размер шрифта в половинах пункта. `parent` — размер снаружи.
pub fn font_size(value: &str, parent: u32) -> Option<u32> {
    let points = match value.trim().to_ascii_lowercase().as_str() {
        "xx-small" => 7.0,
        "x-small" => 7.5,
        "small" => 10.0,
        "medium" => 12.0,
        "large" => 13.5,
        "x-large" => 18.0,
        "xx-large" => 24.0,
        "smaller" => f64::from(parent) / 2.0 / 1.2,
        "larger" => f64::from(parent) / 2.0 * 1.2,
        other => f64::from(length(other, parent)?) / 20.0,
    };
    let half_points = (points * 2.0).round();
    (half_points >= 1.0).then(|| half_points.min(3276.0) as u32)
}

/// Размер шрифта по атрибуту `<font size>` (1–7, можно `+1`, `-2`).
pub fn font_tag_size(value: &str) -> Option<u32> {
    let value = value.trim();
    let number: i32 = value.trim_start_matches('+').parse().ok()?;
    let size = if value.starts_with(['+', '-']) {
        3 + number
    } else {
        number
    };
    let points = [8, 10, 12, 14, 18, 24, 36];
    let index = usize::try_from(size.clamp(1, 7) - 1).unwrap_or(0);
    Some(points[index] * 2)
}

/// Первый шрифт из списка `font-family`. Общие семейства заменяются
/// шрифтами, которые есть везде.
pub fn font_family(value: &str) -> Option<String> {
    let first = value.split(',').next()?.trim();
    let name = first.trim_matches(|c| c == '"' || c == '\'').trim();
    let name = match name.to_ascii_lowercase().as_str() {
        "" | "inherit" | "initial" => return None,
        "serif" => "Times New Roman",
        "sans-serif" | "system-ui" => "Arial",
        "monospace" => "Courier New",
        _ => name,
    };
    Some(name.to_owned())
}

/// Цвет: `#rgb`, `#rrggbb`, `rgb(...)`, имя. `windowtext` и `auto` —
/// «авто».
pub fn color(value: &str) -> Option<Color> {
    let value = value.trim().to_ascii_lowercase();
    if let Some(hex) = value.strip_prefix('#') {
        return hex_color(hex);
    }
    if let Some(args) = value
        .strip_prefix("rgb(")
        .or_else(|| value.strip_prefix("rgba("))
    {
        let parts: Vec<u8> = args
            .trim_end_matches(')')
            .split([',', ' ', '/'])
            .filter(|s| !s.is_empty())
            .take(3)
            .filter_map(|s| {
                let v = match s.strip_suffix('%') {
                    Some(p) => p.parse::<f64>().ok()? * 2.55,
                    None => s.parse::<f64>().ok()?,
                };
                Some(v.round().clamp(0.0, 255.0) as u8)
            })
            .collect();
        return match parts[..] {
            [r, g, b] => Some(Color::Rgb(r, g, b)),
            _ => None,
        };
    }
    let rgb = match value.as_str() {
        "windowtext" | "auto" | "initial" | "inherit" => return Some(Color::Auto),
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "red" => (255, 0, 0),
        "lime" => (0, 255, 0),
        "green" => (0, 128, 0),
        "blue" => (0, 0, 255),
        "yellow" => (255, 255, 0),
        "cyan" | "aqua" => (0, 255, 255),
        "magenta" | "fuchsia" => (255, 0, 255),
        "gray" | "grey" => (128, 128, 128),
        "darkgray" | "darkgrey" => (169, 169, 169),
        "lightgray" | "lightgrey" => (211, 211, 211),
        "silver" => (192, 192, 192),
        "maroon" => (128, 0, 0),
        "darkred" => (139, 0, 0),
        "navy" => (0, 0, 128),
        "darkblue" => (0, 0, 139),
        "olive" => (128, 128, 0),
        "purple" => (128, 0, 128),
        "teal" => (0, 128, 128),
        "orange" => (255, 165, 0),
        "brown" => (165, 42, 42),
        "pink" => (255, 192, 203),
        _ => return None,
    };
    Some(Color::Rgb(rgb.0, rgb.1, rgb.2))
}

fn hex_color(hex: &str) -> Option<Color> {
    let digit = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok();
    let pair = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    match hex.len() {
        3 | 4 => Some(Color::Rgb(digit(0)? * 17, digit(1)? * 17, digit(2)? * 17)),
        6 | 8 => Some(Color::Rgb(pair(0)?, pair(2)?, pair(4)?)),
        _ => None,
    }
}

/// Ближайший из 16 цветов маркера Word. Прозрачный фон — без маркера.
pub fn highlight(value: &str) -> Option<Highlight> {
    let value = value.trim().to_ascii_lowercase();
    if matches!(
        value.as_str(),
        "transparent" | "none" | "initial" | "inherit"
    ) {
        return Some(Highlight::None);
    }
    // `background: #ff0 url(...)`: берём первое, что похоже на цвет.
    let color = value
        .split_whitespace()
        .find_map(color)
        .or_else(|| self::color(&value))?;
    let Color::Rgb(r, g, b) = color else {
        return Some(Highlight::None);
    };
    const ALL: [Highlight; 16] = [
        Highlight::Black,
        Highlight::Blue,
        Highlight::Cyan,
        Highlight::Green,
        Highlight::Magenta,
        Highlight::Red,
        Highlight::Yellow,
        Highlight::White,
        Highlight::DarkBlue,
        Highlight::DarkCyan,
        Highlight::DarkGreen,
        Highlight::DarkMagenta,
        Highlight::DarkRed,
        Highlight::DarkYellow,
        Highlight::DarkGray,
        Highlight::LightGray,
    ];
    ALL.into_iter().min_by_key(|h| {
        let (hr, hg, hb) = h.rgb().unwrap_or((0, 0, 0));
        let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
        d(r, hr) + d(g, hg) + d(b, hb)
    })
}

/// Запись длины в пунктах для CSS: `12pt`, `10.5pt`.
pub fn points(twips: i32) -> String {
    let value = f64::from(twips) / 20.0;
    format!("{}pt", trim_number(value))
}

/// Число без лишних нулей: `1.5`, `2`, `1.0792`.
pub fn trim_number(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".into()
    } else {
        text.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_declarations() {
        assert_eq!(
            declarations("Color: red; font-size:12pt !important;;bad"),
            vec![
                ("color".to_string(), "red".to_string()),
                ("font-size".to_string(), "12pt".to_string())
            ]
        );
    }

    #[test]
    fn lengths_and_sizes() {
        assert_eq!(length("12pt", 24), Some(240));
        assert_eq!(length("16px", 24), Some(240));
        assert_eq!(length("1in", 24), Some(1440));
        assert_eq!(length("2.54cm", 24), Some(1440));
        assert_eq!(length("2em", 24), Some(480));
        assert_eq!(length("-0.25in", 24), Some(-360));
        assert_eq!(length("auto", 24), None);
        assert_eq!(font_size("10.5pt", 24), Some(21));
        assert_eq!(font_size("150%", 24), Some(36));
        assert_eq!(font_size("x-large", 24), Some(36));
        assert_eq!(font_size("0", 24), None);
        assert_eq!(font_tag_size("+1"), Some(28));
        assert_eq!(font_tag_size("7"), Some(72));
        assert_eq!(font_tag_size("99"), Some(72));
    }

    #[test]
    fn colors_and_fonts() {
        assert_eq!(color("#C00000"), Some(Color::Rgb(0xC0, 0, 0)));
        assert_eq!(color("#f0a"), Some(Color::Rgb(255, 0, 170)));
        assert_eq!(color("rgb(1, 2, 3)"), Some(Color::Rgb(1, 2, 3)));
        assert_eq!(color("rgb(100%,0%,0%)"), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(color("WindowText"), Some(Color::Auto));
        assert_eq!(color("#12"), None);
        assert_eq!(color("nonsense"), None);
        assert_eq!(highlight("yellow"), Some(Highlight::Yellow));
        assert_eq!(highlight("#fefe10"), Some(Highlight::Yellow));
        assert_eq!(highlight("transparent"), Some(Highlight::None));
        assert_eq!(
            font_family("'Times New Roman', serif").as_deref(),
            Some("Times New Roman")
        );
        assert_eq!(font_family("monospace").as_deref(), Some("Courier New"));
        assert_eq!(points(210), "10.5pt");
        assert_eq!(trim_number(259.0 / 240.0), "1.0792");
    }
}
