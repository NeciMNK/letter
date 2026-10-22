//! Темы оформления интерфейса: светлая, тёмная или как в системе.
//! Лист документа остаётся белым в любой теме, как в Word по умолчанию;
//! меняются лента, линейки, представление «Файл» и окна.

use crate::i18n::tr;
use eframe::egui::{Color32, Context, ThemePreference};

/// Выбор темы в «Параметрах».
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    /// Светлая.
    Light,
    /// Тёмная.
    Dark,
    /// Как в системе.
    #[default]
    System,
}

impl ThemeChoice {
    /// Все темы по порядку списка.
    pub const ALL: [ThemeChoice; 3] = [ThemeChoice::Light, ThemeChoice::Dark, ThemeChoice::System];

    /// Название для пользователя.
    pub fn name(self) -> &'static str {
        match self {
            ThemeChoice::Light => tr("theme-light"),
            ThemeChoice::Dark => tr("theme-dark"),
            ThemeChoice::System => tr("theme-system"),
        }
    }

    /// Значение в файле параметров.
    pub fn key(self) -> &'static str {
        match self {
            ThemeChoice::Light => "light",
            ThemeChoice::Dark => "dark",
            ThemeChoice::System => "system",
        }
    }

    /// Тема по значению из файла параметров.
    pub fn from_key(key: &str) -> Option<ThemeChoice> {
        ThemeChoice::ALL.into_iter().find(|t| t.key() == key)
    }

    /// Тема для egui.
    pub fn preference(self) -> ThemePreference {
        match self {
            ThemeChoice::Light => ThemePreference::Light,
            ThemeChoice::Dark => ThemePreference::Dark,
            ThemeChoice::System => ThemePreference::System,
        }
    }
}

/// Цвета интерфейса, которые egui сам не задаёт.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Тёмная тема.
    pub dark: bool,
    /// Фон ленты.
    pub ribbon: Color32,
    /// Рамки панелей.
    pub border: Color32,
    /// Подписи групп ленты и второстепенный текст.
    pub caption: Color32,
    /// Крупные заголовки (представление «Файл»).
    pub heading: Color32,
    /// Фон вокруг листов.
    pub desk: Color32,
    /// Фон представления «Файл» и плиток.
    pub page: Color32,
    /// Плитки и карточки.
    pub card: Color32,
    /// Фон линейки.
    pub ruler: Color32,
    /// Поля на линейке.
    pub ruler_margin: Color32,
    /// Место текста на линейке.
    pub ruler_text: Color32,
    /// Деления и числа линейки.
    pub ruler_marks: Color32,
    /// Заливка меток отступов на линейке.
    pub marker: Color32,
    /// Неактивные значки.
    pub disabled: Color32,
    /// Обычный текст («Авто» в образцах стилей).
    pub text: Color32,
    /// Текст цвета программы (выбранная вкладка ленты).
    pub accent_text: Color32,
}

impl Palette {
    /// Светлые цвета.
    pub const LIGHT: Palette = Palette {
        dark: false,
        ribbon: Color32::from_gray(247),
        border: Color32::from_gray(215),
        caption: Color32::from_gray(100),
        heading: Color32::from_gray(40),
        desk: Color32::from_gray(230),
        page: Color32::WHITE,
        card: Color32::from_gray(250),
        ruler: Color32::from_gray(232),
        ruler_margin: Color32::from_gray(205),
        ruler_text: Color32::WHITE,
        ruler_marks: Color32::from_gray(90),
        marker: Color32::from_gray(245),
        disabled: Color32::from_gray(170),
        text: Color32::BLACK,
        accent_text: Color32::from_rgb(43, 87, 154),
    };

    /// Тёмные цвета.
    pub const DARK: Palette = Palette {
        dark: true,
        ribbon: Color32::from_gray(40),
        border: Color32::from_gray(70),
        caption: Color32::from_gray(160),
        heading: Color32::from_gray(225),
        desk: Color32::from_gray(52),
        page: Color32::from_gray(30),
        card: Color32::from_gray(45),
        ruler: Color32::from_gray(45),
        ruler_margin: Color32::from_gray(75),
        ruler_text: Color32::from_gray(110),
        ruler_marks: Color32::from_gray(215),
        marker: Color32::from_gray(200),
        disabled: Color32::from_gray(95),
        text: Color32::from_gray(230),
        accent_text: Color32::from_rgb(150, 190, 245),
    };

    /// Цвета текущей темы окна.
    pub fn of(ctx: &Context) -> Palette {
        if ctx.theme() == eframe::egui::Theme::Dark {
            Palette::DARK
        } else {
            Palette::LIGHT
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_palettes() {
        for theme in ThemeChoice::ALL {
            assert_eq!(ThemeChoice::from_key(theme.key()), Some(theme));
        }
        assert_eq!(ThemeChoice::from_key("розовая"), None);
        let ctx = Context::default();
        ctx.set_theme(ThemePreference::Dark);
        assert!(Palette::of(&ctx).dark);
        ctx.set_theme(ThemePreference::Light);
        assert_eq!(Palette::of(&ctx), Palette::LIGHT);
    }
}
