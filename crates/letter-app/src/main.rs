//! Приложение `letter`: главное окно редактора. Открывает, правит
//! и сохраняет документы .docx, .rtf, .odt, .txt, .html и .md.
//!
//! Запуск: `letter [файл]`; без файла открывается пустой документ.

// В релизной сборке под Windows не показываем окно консоли рядом с программой.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backstage;
mod bookmark_dialog;
mod calendar;
mod clipboard;
mod comments;
mod context_menu;
mod control_dialog;
mod design;
mod developer;
mod equation;
mod fields;
mod files;
mod find;
mod font_dialog;
mod headers;
mod home;
mod i18n;
mod icons;
mod insert;
mod keytips;
mod language;
mod layout;
mod link_dialog;
mod mailings;
mod merge_dialogs;
mod modes;
mod navigation;
mod outlining;
mod page_number_dialog;
mod paragraph_dialog;
mod password_dialog;
mod picture;
mod print;
mod protect_dialog;
mod references;
mod review;
mod ribbon;
mod ruler;
mod settings;
mod shortcuts;
mod spelling;
mod story;
mod styles;
mod symbols;
mod templates;
mod theme;
mod units;
mod view;
mod viewer;

use std::path::PathBuf;

use eframe::egui;
use letter_layout::FontLibrary;
use viewer::{APP_NAME, Viewer};

/// Значок программы (рисунок — `assets/icon/generate.py`).
const ICON: &[u8] = include_bytes!("../assets/icon/letter-256.png");

fn main() -> eframe::Result {
    let file = std::env::args_os().nth(1).map(PathBuf::from);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(APP_NAME)
        .with_inner_size([1024.0, 768.0])
        .with_min_inner_size([400.0, 300.0])
        .with_drag_and_drop(true);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(ICON) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| {
            // Ctrl+плюс/минус меняют масштаб документа, а не интерфейса.
            cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
            icons::install(&cc.egui_ctx);
            symbols::install(&cc.egui_ctx);
            let mut viewer = Viewer::with_storage(FontLibrary::new(), files::app_dir());
            match file {
                Some(file) => viewer.open_or_report(&file),
                // Как Word: без файла — сразу пустой документ.
                None => viewer.new_document(),
            }
            Ok(Box::new(LetterApp { viewer }))
        }),
    )
}

/// Состояние приложения.
struct LetterApp {
    viewer: Viewer,
}

impl eframe::App for LetterApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.viewer.ui(ui);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_is_a_valid_png() {
        let icon = eframe::icon_data::from_png_bytes(super::ICON).unwrap();
        assert_eq!((icon.width, icon.height), (256, 256));
    }
}
