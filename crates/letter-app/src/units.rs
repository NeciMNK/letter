//! Поля для чисел с единицами, как в окнах Word: «1,25 см», «6 пт».
//! Внутри значения в твипах (1/20 пункта), на экране — с запятой.

use crate::i18n::tr_args;
use eframe::egui::{DragValue, Id, Response, Ui};

/// Размер поля с числом.
pub const FIELD_SIZE: [f32; 2] = [72.0, 20.0];

/// Твипов в сантиметре.
pub const TWIPS_PER_CM: f64 = 1440.0 / 2.54;

/// Число с запятой и без лишних нулей: `1.250` → «1,25».
pub fn format_number(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        &text
    };
    let text = if text == "-0" { "0" } else { text };
    text.replace('.', ",")
}

/// Разбирает число с запятой или точкой и необязательной единицей
/// (`"1,5 см"`, `"6пт"`, `"2"`).
pub fn parse_number(text: &str) -> Option<f64> {
    let digits: String = text
        .trim()
        .trim_end_matches(|c: char| c.is_alphabetic() || c.is_whitespace())
        .replace(',', ".");
    digits.trim().parse().ok().filter(|v: &f64| v.is_finite())
}

/// Поле в сантиметрах для значения в твипах, шаг 0,1 см.
pub fn cm_field(ui: &mut Ui, label: Id, twips: &mut i32, min_cm: f64, max_cm: f64) -> Response {
    let mut cm = f64::from(*twips) / TWIPS_PER_CM;
    let response = ui
        .add_sized(
            FIELD_SIZE,
            DragValue::new(&mut cm)
                .speed(0.05)
                .range(min_cm..=max_cm)
                .custom_formatter(|v, _| {
                    tr_args("unit-cm", &[("value", format_number(v, 2).into())])
                })
                .custom_parser(parse_number),
        )
        .labelled_by(label);
    if response.changed() {
        *twips = (cm * TWIPS_PER_CM).round() as i32;
    }
    response
}

/// Поле в пунктах для значения в твипах.
pub fn pt_field(
    ui: &mut Ui,
    label: Id,
    twips: &mut i32,
    min_pt: f64,
    max_pt: f64,
    speed: f64,
) -> Response {
    let mut pt = f64::from(*twips) / 20.0;
    let response = ui
        .add_sized(
            FIELD_SIZE,
            DragValue::new(&mut pt)
                .speed(speed)
                .range(min_pt..=max_pt)
                .custom_formatter(|v, _| {
                    tr_args("unit-pt", &[("value", format_number(v, 1).into())])
                })
                .custom_parser(parse_number),
        )
        .labelled_by(label);
    if response.changed() {
        *twips = (pt * 20.0).round() as i32;
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_with_commas() {
        assert_eq!(format_number(1.25, 2), "1,25");
        assert_eq!(format_number(1.0, 2), "1");
        assert_eq!(format_number(12.0, 1), "12");
        assert_eq!(format_number(-0.001, 2), "0");
        assert_eq!(format_number(6.5, 1), "6,5");
        assert_eq!(parse_number("1,5 см"), Some(1.5));
        assert_eq!(parse_number(" 6пт "), Some(6.0));
        assert_eq!(parse_number("-0.5"), Some(-0.5));
        assert_eq!(parse_number("см"), None);
        assert_eq!(parse_number("inf"), None);
    }
}
