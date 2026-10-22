//! Параметры программы («Файл → Параметры»). Хранятся в папке программы
//! в `settings.txt`, по строке `имя = значение`; незнакомые строки
//! и ошибки пропускаются.

use std::fs;
use std::path::{Path, PathBuf};

use crate::files::write_atomic;
use crate::theme::ThemeChoice;

/// Параметры программы.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// Автосохранение каждые столько минут (1–120).
    pub autosave_minutes: u32,
    /// Показывать линейку при запуске.
    pub show_ruler: bool,
    /// Тема оформления.
    pub theme: ThemeChoice,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            autosave_minutes: 2,
            show_ruler: true,
            theme: ThemeChoice::default(),
        }
    }
}

/// Пределы интервала автосохранения, минуты.
pub const AUTOSAVE_RANGE: std::ops::RangeInclusive<u32> = 1..=120;

impl Settings {
    /// Разбирает текст файла параметров.
    pub fn parse(text: &str) -> Settings {
        let mut settings = Settings::default();
        for line in text.lines() {
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match name.trim() {
                "autosave_minutes" => {
                    if let Ok(minutes) = value.parse::<u32>() {
                        settings.autosave_minutes =
                            minutes.clamp(*AUTOSAVE_RANGE.start(), *AUTOSAVE_RANGE.end());
                    }
                }
                "show_ruler" => {
                    if let Ok(show) = value.parse() {
                        settings.show_ruler = show;
                    }
                }
                "theme" => {
                    if let Some(theme) = ThemeChoice::from_key(value) {
                        settings.theme = theme;
                    }
                }
                _ => {}
            }
        }
        settings
    }

    /// Текст файла параметров.
    pub fn to_text(self) -> String {
        format!(
            "autosave_minutes = {}\nshow_ruler = {}\ntheme = {}\n",
            self.autosave_minutes,
            self.show_ruler,
            self.theme.key()
        )
    }

    /// Файл параметров в папке программы `dir`.
    fn file(dir: &Path) -> PathBuf {
        dir.join("settings.txt")
    }

    /// Читает параметры из папки программы (`None` — значения по умолчанию).
    pub fn load(dir: Option<&Path>) -> Settings {
        dir.and_then(|d| fs::read_to_string(Settings::file(d)).ok())
            .map_or_else(Settings::default, |text| Settings::parse(&text))
    }

    /// Сохраняет параметры. Ошибку возвращает текстом.
    pub fn save(self, dir: &Path) -> Result<(), String> {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        write_atomic(&Settings::file(dir), self.to_text().as_bytes()).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trip_and_bad_lines() {
        let settings = Settings {
            autosave_minutes: 10,
            show_ruler: false,
            theme: ThemeChoice::Dark,
        };
        assert_eq!(Settings::parse(&settings.to_text()), settings);
        let odd = "мусор\nautosave_minutes = 9999\nshow_ruler = может быть\n";
        assert_eq!(
            Settings::parse(odd),
            Settings {
                autosave_minutes: 120,
                ..Settings::default()
            }
        );
    }

    #[test]
    fn saved_to_the_program_folder() {
        let dir = crate::files::tests::temp_dir("settings");
        assert_eq!(Settings::load(Some(&dir)), Settings::default());
        let settings = Settings {
            autosave_minutes: 5,
            show_ruler: false,
            theme: ThemeChoice::Light,
        };
        settings.save(&dir).unwrap();
        assert_eq!(Settings::load(Some(&dir)), settings);
        assert_eq!(Settings::load(None), Settings::default());
    }
}
