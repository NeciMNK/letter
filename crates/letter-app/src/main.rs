//! Приложение `letter`: главное окно редактора. Открывает, правит
//! и сохраняет документы .docx, .rtf, .odt, .txt, .html и .md.
//!
//! Запуск: `letter [файл]`; без файла открывается пустой документ.

// В релизной сборке под Windows не показываем окно консоли рядом с программой.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backstage;
mod clipboard;
mod files;
mod find;
mod font_dialog;
mod home;
mod icons;
mod language;
mod paragraph_dialog;
mod ribbon;
mod ruler;
mod settings;
mod styles;
mod units;
mod view;
mod viewer;

use std::path::PathBuf;

use eframe::egui;
use letter_layout::FontLibrary;
use viewer::{APP_NAME, Viewer};

fn main() -> eframe::Result {
    let file = std::env::args_os().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_inner_size([1024.0, 768.0])
            .with_min_inner_size([400.0, 300.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| {
            // Ctrl+плюс/минус меняют масштаб документа, а не интерфейса.
            cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
            // Светлое оформление, как у Word по умолчанию.
            cc.egui_ctx.set_theme(egui::Theme::Light);
            icons::install(&cc.egui_ctx);
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
