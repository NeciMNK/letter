//! Окно «Формат номеров страниц» («Номер страницы → Формат номеров
//! страниц»): вид номеров и «продолжить» или «начать с» у раздела.
//! Окно только собирает ответ; документ меняет просмотрщик.

use eframe::egui::{self, Context};
use letter_model::{NumberFormat, PageNumbering};

use crate::i18n::tr;

/// Виды номеров в окне и их подписи.
const FORMATS: [(NumberFormat, &str); 5] = [
    (NumberFormat::Decimal, "1, 2, 3, …"),
    (NumberFormat::LowerLetter, "a, b, c, …"),
    (NumberFormat::UpperLetter, "A, B, C, …"),
    (NumberFormat::LowerRoman, "i, ii, iii, …"),
    (NumberFormat::UpperRoman, "I, II, III, …"),
];

/// Состояние окна.
#[derive(Debug, Clone, Default)]
pub struct PageNumberDialog {
    /// Окно открыто.
    pub open: bool,
    /// Страница, у раздела которой меняется нумерация.
    pub page: usize,
    /// Номер вида в [`FORMATS`].
    pub format: usize,
    /// «Начать с» (иначе «продолжить»).
    pub restart: bool,
    /// С какого номера начать.
    pub start: u32,
}

impl PageNumberDialog {
    /// Открывает окно для раздела страницы `page` с его нумерацией.
    pub fn show(&mut self, page: usize, current: Option<PageNumbering>) {
        let current = current.unwrap_or_default();
        self.open = true;
        self.page = page;
        self.format = current
            .format
            .and_then(|f| FORMATS.iter().position(|(v, _)| *v == f))
            .unwrap_or(0);
        self.restart = current.start.is_some();
        self.start = current.start.unwrap_or(1);
    }

    /// Нумерация, выбранная в окне; `None` — как у предыдущего раздела.
    pub fn numbering(&self) -> Option<PageNumbering> {
        let format = FORMATS
            .get(self.format)
            .map(|(f, _)| f.clone())
            .filter(|f| *f != NumberFormat::Decimal);
        let start = self.restart.then_some(self.start);
        (format.is_some() || start.is_some()).then_some(PageNumbering { start, format })
    }

    /// Рисует окно; по «ОК» возвращает страницу и выбранную нумерацию.
    pub fn ui(&mut self, ctx: &Context) -> Option<(usize, Option<PageNumbering>)> {
        if !self.open {
            return None;
        }
        let mut done = None;
        let mut close = false;
        let mut open = true;
        egui::Window::new(tr("page-number-format-title"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr("page-number-format-kind"));
                    egui::ComboBox::from_id_salt("page-number-format")
                        .selected_text(FORMATS[self.format.min(FORMATS.len() - 1)].1)
                        .show_ui(ui, |ui| {
                            for (index, (_, label)) in FORMATS.iter().enumerate() {
                                ui.selectable_value(&mut self.format, index, *label);
                            }
                        });
                });
                ui.separator();
                ui.label(tr("page-number-format-numbering"));
                ui.radio_value(&mut self.restart, false, tr("page-number-format-continue"));
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.restart, true, tr("page-number-format-start"));
                    ui.add_enabled(
                        self.restart,
                        egui::DragValue::new(&mut self.start).range(0..=32767),
                    );
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(tr("ok")).clicked() {
                        done = Some((self.page, self.numbering()));
                    }
                    if ui.button(tr("cancel")).clicked() {
                        close = true;
                    }
                });
            });
        if done.is_some() || close || !open {
            self.open = false;
        }
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbering_comes_from_choices() {
        let mut dialog = PageNumberDialog::default();
        dialog.show(2, None);
        assert_eq!((dialog.page, dialog.start), (2, 1));
        assert_eq!(dialog.numbering(), None);
        dialog.restart = true;
        dialog.start = 5;
        dialog.format = 3;
        let numbering = PageNumbering {
            start: Some(5),
            format: Some(NumberFormat::LowerRoman),
        };
        assert_eq!(dialog.numbering(), Some(numbering.clone()));
        // Открытое снова — с той же нумерацией.
        dialog.show(0, Some(numbering.clone()));
        assert_eq!(dialog.numbering(), Some(numbering));
    }
}
