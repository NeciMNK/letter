//! Календарь на месяц для элемента выбора даты: стрелки листают месяцы,
//! щелчок по числу выбирает дату. Неделя начинается с понедельника.

use eframe::egui::{self, Color32, Id, RichText, Ui, vec2};

use crate::i18n::tr;

/// Дата: год, месяц (1–12), день.
pub type Day = (i32, u32, u32);

/// Сторона клетки дня, точки экрана.
const CELL: f32 = 26.0;

/// Високосный ли год.
fn leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Сколько дней в месяце.
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// День недели: 0 — понедельник, 6 — воскресенье.
pub fn weekday(year: i32, month: u32, day: u32) -> u32 {
    // Число дней от 1970-01-01 (четверг) по алгоритму Хиннанта.
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    (days + 3).rem_euclid(7) as u32
}

/// Соседний месяц: `step` — +1 или −1.
fn shift((year, month): (i32, u32), step: i32) -> (i32, u32) {
    let index = year * 12 + month as i32 - 1 + step;
    (index.div_euclid(12), index.rem_euclid(12) as u32 + 1)
}

/// Рисует календарь. Показанный месяц хранится по `id` (сначала — месяц
/// `selected`, без неё — `today`). Возвращает выбранный день.
pub fn calendar(ui: &mut Ui, id: Id, selected: Option<Day>, today: Day) -> Option<Day> {
    let start = selected.unwrap_or(today);
    let mut shown: (i32, u32) = ui.data(|d| d.get_temp(id)).unwrap_or((start.0, start.1));
    let mut picked = None;
    ui.horizontal(|ui| {
        if ui
            .button("<")
            .on_hover_text(tr("calendar-previous"))
            .clicked()
        {
            shown = shift(shown, -1);
        }
        let months: Vec<&str> = tr("calendar-months").split(' ').collect();
        let name = months
            .get(shown.1 as usize - 1)
            .copied()
            .unwrap_or_default();
        ui.add_sized(
            vec2(CELL * 5.0 - 8.0, 20.0),
            egui::Label::new(RichText::new(format!("{name} {}", shown.0)).strong()),
        );
        if ui.button(">").on_hover_text(tr("calendar-next")).clicked() {
            shown = shift(shown, 1);
        }
    });
    egui::Grid::new(id.with("days"))
        .spacing(vec2(0.0, 0.0))
        .min_col_width(CELL)
        .show(ui, |ui| {
            for name in tr("calendar-weekdays").split(' ') {
                let name = RichText::new(name).small().color(Color32::GRAY);
                ui.add_sized(vec2(CELL, 16.0), egui::Label::new(name));
            }
            ui.end_row();
            let (year, month) = shown;
            for _ in 0..weekday(year, month, 1) {
                ui.label("");
            }
            for day in 1..=days_in_month(year, month) {
                let date = (year, month, day);
                let mut text = RichText::new(day.to_string());
                if date == today {
                    text = text.strong().underline();
                }
                let chosen = selected == Some(date);
                let mut button = egui::Button::new(text)
                    .frame_when_inactive(chosen)
                    .min_size(vec2(CELL, CELL - 4.0));
                if chosen {
                    button = button.fill(ui.visuals().selection.bg_fill);
                }
                if ui.add(button).clicked() {
                    picked = Some(date);
                }
                if weekday(year, month, day) == 6 {
                    ui.end_row();
                }
            }
        });
    ui.data_mut(|d| d.insert_temp(id, shown));
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekdays_and_lengths() {
        // 1 января 1970 — четверг, 26 октября 2026 — понедельник.
        assert_eq!(weekday(1970, 1, 1), 3);
        assert_eq!(weekday(2026, 10, 26), 0);
        assert_eq!(weekday(2000, 2, 29), 1);
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(days_in_month(2026, 11), 30);
        assert_eq!(shift((2026, 12), 1), (2027, 1));
        assert_eq!(shift((2026, 1), -1), (2025, 12));
    }

    #[test]
    fn picks_a_day_and_turns_months() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;
        let mut h = Harness::new_ui_state(
            |ui, picked: &mut Option<Day>| {
                let id = Id::NULL.with("calendar");
                if let Some(day) = calendar(ui, id, Some((2026, 10, 23)), (2026, 10, 26)) {
                    *picked = Some(day);
                }
            },
            None,
        );
        h.run();
        h.get_by_label("15").click();
        h.run();
        assert_eq!(*h.state(), Some((2026, 10, 15)));
        h.get_by_label(">").click();
        h.run();
        h.get_by_label("30").click();
        h.run();
        assert_eq!(*h.state(), Some((2026, 11, 30)));
    }
}
