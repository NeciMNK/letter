//! Длины ODF (`2.5cm`, `0.5in`, `12pt`) и их запись.

use letter_model::Twips;

/// Длина ODF в твипах. Проценты и неизвестные единицы — `None`.
pub(crate) fn parse_length(value: &str) -> Option<Twips> {
    let value = value.trim();
    let split = value
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let number: f64 = number.trim().parse().ok()?;
    let points = match unit.trim().to_ascii_lowercase().as_str() {
        "pt" => number,
        "in" | "inch" => number * 72.0,
        "cm" => number * 72.0 / 2.54,
        "mm" => number * 72.0 / 25.4,
        "pc" => number * 12.0,
        "px" => number * 0.75,
        "twip" => number / 20.0,
        // Число без единиц стандарт не допускает: что имелось в виду,
        // не угадать, поэтому пропускаем.
        _ => return None,
    };
    if !points.is_finite() || points.abs() > 1.0e7 {
        return None;
    }
    Some(Twips::from_points(points))
}

/// Процент: `"115%"` → `115.0`.
pub(crate) fn parse_percent(value: &str) -> Option<f64> {
    let number: f64 = value.trim().strip_suffix('%')?.trim().parse().ok()?;
    number.is_finite().then_some(number)
}

/// Длина для записи: в пунктах, без лишних нулей (`12pt`, `0.05pt`).
/// Твип — ровно 0,05 пункта, так что значение не округляется.
pub(crate) fn length(twips: Twips) -> String {
    format!("{}pt", number(f64::from(twips.0) / 20.0))
}

/// Число без лишних нулей: `1.5`, `2`, `0.05`.
pub(crate) fn number(value: f64) -> String {
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
    fn lengths() {
        assert_eq!(parse_length("1in"), Some(Twips(1440)));
        assert_eq!(parse_length("2.54cm"), Some(Twips(1440)));
        assert_eq!(parse_length("25.4mm"), Some(Twips(1440)));
        assert_eq!(parse_length("12pt"), Some(Twips(240)));
        assert_eq!(parse_length("-0.25in"), Some(Twips(-360)));
        assert_eq!(parse_length("0.139in"), Some(Twips(200)));
        assert_eq!(parse_length("50%"), None);
        assert_eq!(parse_length("abc"), None);
        assert_eq!(parse_length(""), None);
    }

    #[test]
    fn written_lengths_read_back() {
        for t in [0, 1, 19, 20, 708, -360, 11906, 16838] {
            assert_eq!(parse_length(&length(Twips(t))), Some(Twips(t)), "{t}");
        }
        assert_eq!(length(Twips(240)), "12pt");
        assert_eq!(length(Twips(1)), "0.05pt");
    }

    #[test]
    fn percents() {
        assert_eq!(parse_percent("115%"), Some(115.0));
        assert_eq!(parse_percent("115"), None);
    }
}
