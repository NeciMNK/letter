//! Приложение `letter`: открывает главное окно редактора.

// В релизной сборке под Windows не показываем окно консоли рядом с программой.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;

/// Заголовок главного окна.
const WINDOW_TITLE: &str = "letter";

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(WINDOW_TITLE)
            .with_inner_size([1024.0, 768.0])
            .with_min_inner_size([400.0, 300.0]),
        ..Default::default()
    };
    eframe::run_native(
        WINDOW_TITLE,
        options,
        Box::new(|_cc| Ok(Box::new(LetterApp::default()))),
    )
}

/// Состояние приложения. Пока пустое: окно без содержимого.
#[derive(Default)]
struct LetterApp {}

impl eframe::App for LetterApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |_ui| {});
    }
}
