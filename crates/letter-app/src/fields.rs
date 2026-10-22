//! Группа «Текст» вкладки «Вставка»: «Дата и время» и «Поле».

use eframe::egui::{self, Ui};
use letter_editor::{DateTime, format_date};

use crate::i18n::{Language, language, tr};
use crate::icons::Icon;
use crate::insert::menu_button;
use crate::ribbon::{RibbonCommand, RibbonInfo, group};

/// Образцы даты и времени в меню «Дата и время» (ключ `\@` поля).
const DATE_FORMATS_RU: [&str; 9] = [
    "dd.MM.yyyy",
    "d MMMM yyyy 'г.'",
    "dddd, d MMMM yyyy 'г.'",
    "dd.MM.yy",
    "yyyy-MM-dd",
    "MMMM yyyy",
    "dd.MM.yyyy H:mm",
    "H:mm",
    "H:mm:ss",
];
const DATE_FORMATS_EN: [&str; 9] = [
    "M/d/yyyy",
    "dddd, MMMM d, yyyy",
    "MMMM d, yyyy",
    "M/d/yy",
    "yyyy-MM-dd",
    "MMMM yy",
    "M/d/yyyy h:mm am/pm",
    "h:mm am/pm",
    "HH:mm:ss",
];

/// Поля в меню «Поле»: код и имя подписи.
pub const FIELDS: [(&str, &str); 9] = [
    ("PAGE", "field-page"),
    ("NUMPAGES", "field-numpages"),
    ("DATE", "field-date"),
    ("TIME", "field-time"),
    ("AUTHOR", "field-author"),
    ("TITLE", "field-title"),
    ("FILENAME", "field-filename"),
    ("NUMWORDS", "field-numwords"),
    ("SECTION", "field-section"),
];

/// Сейчас (местное время).
pub fn now() -> (DateTime, i64) {
    let now = chrono::Local::now();
    let offset = i64::from(now.offset().local_minus_utc());
    (DateTime::from_unix(now.timestamp(), offset), offset)
}

/// Код поля для образца даты: TIME, если в образце только время.
pub fn date_field_code(picture: &str) -> String {
    let date = picture
        .split('\'')
        .step_by(2)
        .any(|part| part.contains(['d', 'M', 'y']));
    let name = if date { "DATE" } else { "TIME" };
    format!("{name} \\@ \"{picture}\"")
}

/// Группа «Текст».
pub fn text_group(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let enabled = info.has_document;
    group(ui, tr("group-text"), |ui| {
        menu_button(
            ui,
            Icon::DateTime,
            tr("date-time"),
            tr("date-time-hint"),
            "keytip-insert-date-time",
            enabled,
            |ui| date_menu(ui, out),
        );
        menu_button(
            ui,
            Icon::Field,
            tr("field"),
            tr("field-hint"),
            "keytip-insert-field",
            enabled,
            |ui| {
                for (code, label) in FIELDS {
                    let text = format!("{}  ({code})", tr(label));
                    if ui.button(text).clicked() {
                        out.push(RibbonCommand::InsertField(code.to_string()));
                        ui.close();
                    }
                }
            },
        );
    });
}

/// Меню «Дата и время»: образцы с сегодняшней датой и флажок
/// «Обновлять автоматически» (тогда вставляется поле, иначе текст).
fn date_menu(ui: &mut Ui, out: &mut Vec<RibbonCommand>) {
    let id = egui::Id::NULL.with("date-time-auto-update");
    let mut auto = ui.data_mut(|d| *d.get_persisted_mut_or(id, false));
    let russian = language() == Language::Russian;
    let (now, _) = now();
    let formats = if russian {
        DATE_FORMATS_RU
    } else {
        DATE_FORMATS_EN
    };
    for picture in formats {
        let text = format_date(picture, now, russian);
        if ui.button(&text).clicked() {
            out.push(if auto {
                RibbonCommand::InsertField(date_field_code(picture))
            } else {
                RibbonCommand::Edit(letter_editor::Command::Type(text))
            });
            ui.close();
        }
    }
    ui.separator();
    if ui.checkbox(&mut auto, tr("date-time-update")).changed() {
        ui.data_mut(|d| d.insert_persisted(id, auto));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_codes() {
        assert_eq!(date_field_code("dd.MM.yyyy"), "DATE \\@ \"dd.MM.yyyy\"");
        assert_eq!(date_field_code("H:mm"), "TIME \\@ \"H:mm\"");
        assert_eq!(date_field_code("h:mm am/pm"), "TIME \\@ \"h:mm am/pm\"");
        assert_eq!(
            date_field_code("d MMMM yyyy 'г.'"),
            "DATE \\@ \"d MMMM yyyy 'г.'\""
        );
    }

    #[test]
    fn field_labels_exist() {
        for (_, label) in FIELDS {
            assert!(crate::i18n::has(label), "{label}");
        }
    }
}
