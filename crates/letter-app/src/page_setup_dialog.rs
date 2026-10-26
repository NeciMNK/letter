//! Окно «Параметры страницы», как в Word: вкладки «Поля» (поля,
//! переплёт, ориентация), «Размер бумаги» (готовые размеры и свой) и
//! «Макет» (расстояния до колонтитулов). Окно только собирает ответ;
//! документ меняет просмотрщик.

use eframe::egui::{self, Context, Ui};
use letter_editor::PageSetup;
use letter_model::{LineNumberRestart, LineNumbering, Orientation, PageMargins, PageSize, Twips};

use crate::i18n::tr;
use crate::layout::SIZES;
use crate::units::cm_field;

/// Вкладка окна.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PageSetupTab {
    /// «Поля».
    #[default]
    Margins,
    /// «Размер бумаги».
    Paper,
    /// «Макет».
    Layout,
}

/// Состояние окна.
#[derive(Debug, Clone, Default)]
pub struct PageSetupDialog {
    /// Окно открыто.
    pub open: bool,
    /// Открытая вкладка.
    pub tab: PageSetupTab,
    /// Лист.
    pub size: PageSize,
    /// Поля.
    pub margins: PageMargins,
    /// Строки нумеруются.
    pub lines_on: bool,
    /// Нумерация строк (если включена).
    pub lines: LineNumbering,
    /// Применить ко всему документу (иначе — к разделам выделения).
    pub whole: bool,
    /// В документе несколько разделов: можно выбрать, к чему применить.
    sections: bool,
}

/// Поле в твипах как `i32` для [`cm_field`].
fn twips_field(ui: &mut Ui, label: &str, value: &mut Twips, min_cm: f64, max_cm: f64) {
    let label = ui.label(tr(label));
    let mut twips = value.0;
    cm_field(ui, label.id, &mut twips, min_cm, max_cm);
    *value = Twips(twips);
    ui.end_row();
}

impl PageSetupDialog {
    /// Открывает окно на вкладке `tab` с листом и полями раздела;
    /// `sections` — разделов в документе больше одного.
    pub fn show(
        &mut self,
        tab: PageSetupTab,
        size: Option<PageSize>,
        margins: Option<PageMargins>,
        lines: Option<LineNumbering>,
        sections: bool,
    ) {
        *self = PageSetupDialog {
            open: true,
            tab,
            size: size
                .filter(|s| s.width.0 > 0 && s.height.0 > 0)
                .unwrap_or(PageSize::A4),
            margins: margins.unwrap_or_default(),
            lines_on: lines.is_some(),
            lines: lines.unwrap_or_default(),
            whole: !sections,
            sections,
        };
    }

    /// Правка документа по ответу окна.
    pub fn setup(&self) -> PageSetup {
        PageSetup::Page {
            size: self.size,
            margins: self.margins,
            line_numbering: self.lines_on.then_some(self.lines),
            whole: self.whole,
        }
    }

    /// Поворачивает лист и поля, как кнопка «Ориентация» в Word.
    fn turn(&mut self, to: Orientation) {
        if self.size.orientation == to {
            return;
        }
        self.size = self.size.rotated();
        let m = self.margins;
        let (top, right, bottom, left) = match to {
            Orientation::Landscape => (m.left, m.top, m.right, m.bottom),
            Orientation::Portrait => (m.right, m.bottom, m.left, m.top),
        };
        self.margins = PageMargins {
            top,
            right,
            bottom,
            left,
            ..m
        };
    }

    /// Рисует окно; по «ОК» возвращает правку документа.
    pub fn ui(&mut self, ctx: &Context) -> Option<PageSetup> {
        if !self.open {
            return None;
        }
        let mut done = None;
        let mut close = false;
        let mut open = true;
        egui::Window::new(tr("page-setup-title"))
            .open(&mut open)
            .min_width(320.0)
            .min_height(230.0)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (tab, label) in [
                        (PageSetupTab::Margins, "page-setup-tab-margins"),
                        (PageSetupTab::Paper, "page-setup-tab-paper"),
                        (PageSetupTab::Layout, "page-setup-tab-layout"),
                    ] {
                        ui.selectable_value(&mut self.tab, tab, tr(label));
                    }
                });
                ui.separator();
                match self.tab {
                    PageSetupTab::Margins => self.margins_tab(ui),
                    PageSetupTab::Paper => self.paper_tab(ui),
                    PageSetupTab::Layout => self.layout_tab(ui),
                }
                ui.separator();
                if self.sections {
                    ui.horizontal(|ui| {
                        let label = ui.label(tr("borders-apply-to"));
                        let name = |whole| {
                            tr(if whole {
                                "page-setup-whole"
                            } else {
                                "page-setup-section"
                            })
                        };
                        egui::ComboBox::from_id_salt("letter-page-setup-apply")
                            .selected_text(name(self.whole))
                            .show_ui(ui, |ui| {
                                for whole in [false, true] {
                                    ui.selectable_value(&mut self.whole, whole, name(whole));
                                }
                            })
                            .response
                            .labelled_by(label.id);
                    });
                }
                ui.horizontal(|ui| {
                    if ui.button(tr("ok")).clicked() {
                        done = Some(self.setup());
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

    fn margins_tab(&mut self, ui: &mut Ui) {
        let width = f64::from(self.size.width.0) / crate::units::TWIPS_PER_CM;
        let height = f64::from(self.size.height.0) / crate::units::TWIPS_PER_CM;
        egui::Grid::new("letter-page-setup-margins")
            .num_columns(2)
            .show(ui, |ui| {
                let m = &mut self.margins;
                twips_field(ui, "page-setup-top", &mut m.top, -height, height);
                twips_field(ui, "page-setup-bottom", &mut m.bottom, -height, height);
                twips_field(ui, "page-setup-left", &mut m.left, 0.0, width);
                twips_field(ui, "page-setup-right", &mut m.right, 0.0, width);
                twips_field(ui, "page-setup-gutter", &mut m.gutter, 0.0, width);
            });
        ui.add_space(4.0);
        ui.label(tr("orientation"));
        ui.horizontal(|ui| {
            for (label, orientation) in [
                ("orientation-portrait", Orientation::Portrait),
                ("orientation-landscape", Orientation::Landscape),
            ] {
                let current = self.size.orientation == orientation;
                if ui.radio(current, tr(label)).clicked() {
                    self.turn(orientation);
                }
            }
        });
    }

    fn paper_tab(&mut self, ui: &mut Ui) {
        let (short, long) = (
            self.size.width.0.min(self.size.height.0),
            self.size.width.0.max(self.size.height.0),
        );
        let preset = SIZES.iter().position(|(_, w, h)| (*w, *h) == (short, long));
        ui.horizontal(|ui| {
            let label = ui.label(tr("page-size"));
            let name = preset.map_or_else(|| tr("page-setup-custom-size"), |i| tr(SIZES[i].0));
            egui::ComboBox::from_id_salt("letter-page-setup-size")
                .selected_text(name)
                .show_ui(ui, |ui| {
                    for (i, (label, width, height)) in SIZES.iter().enumerate() {
                        if ui.selectable_label(preset == Some(i), tr(label)).clicked() {
                            let (width, height) = match self.size.orientation {
                                Orientation::Portrait => (*width, *height),
                                Orientation::Landscape => (*height, *width),
                            };
                            self.size.width = Twips(width);
                            self.size.height = Twips(height);
                        }
                    }
                })
                .response
                .labelled_by(label.id);
        });
        egui::Grid::new("letter-page-setup-paper")
            .num_columns(2)
            .show(ui, |ui| {
                twips_field(ui, "page-setup-width", &mut self.size.width, 1.0, 55.87);
                twips_field(ui, "page-setup-height", &mut self.size.height, 1.0, 55.87);
            });
        // Ориентация следует за размерами, как в Word.
        self.size.orientation = if self.size.width.0 > self.size.height.0 {
            Orientation::Landscape
        } else {
            Orientation::Portrait
        };
    }

    fn layout_tab(&mut self, ui: &mut Ui) {
        ui.label(tr("page-setup-distance"));
        egui::Grid::new("letter-page-setup-layout")
            .num_columns(2)
            .show(ui, |ui| {
                let m = &mut self.margins;
                twips_field(ui, "page-setup-header", &mut m.header, 0.0, 55.87);
                twips_field(ui, "page-setup-footer", &mut m.footer, 0.0, 55.87);
            });
        ui.separator();
        ui.checkbox(&mut self.lines_on, tr("line-numbers-on"));
        ui.add_enabled_ui(self.lines_on, |ui| {
            egui::Grid::new("letter-page-setup-lines")
                .num_columns(2)
                .show(ui, |ui| {
                    let lines = &mut self.lines;
                    let label = ui.label(tr("line-numbers-start"));
                    ui.add(egui::DragValue::new(&mut lines.start).range(1..=32767))
                        .labelled_by(label.id);
                    ui.end_row();
                    let label = ui.label(tr("line-numbers-step"));
                    ui.add(egui::DragValue::new(&mut lines.count_by).range(1..=100))
                        .labelled_by(label.id);
                    ui.end_row();
                    let label = ui.label(tr("line-numbers-distance"));
                    let mut auto = lines.distance.is_none();
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut auto, tr("line-numbers-auto")).changed() {
                            lines.distance = (!auto).then_some(Twips(360));
                        }
                        if let Some(distance) = &mut lines.distance {
                            let mut twips = distance.0;
                            cm_field(ui, label.id, &mut twips, 0.0, 55.87);
                            *distance = Twips(twips);
                        }
                    });
                    ui.end_row();
                    let label = ui.label(tr("line-numbers-restart"));
                    let name = |restart| {
                        tr(match restart {
                            LineNumberRestart::NewPage => "line-numbers-each-page",
                            LineNumberRestart::NewSection => "line-numbers-each-section",
                            LineNumberRestart::Continuous => "line-numbers-continuous",
                        })
                    };
                    egui::ComboBox::from_id_salt("letter-page-setup-restart")
                        .selected_text(name(lines.restart))
                        .show_ui(ui, |ui| {
                            for restart in [
                                LineNumberRestart::NewPage,
                                LineNumberRestart::NewSection,
                                LineNumberRestart::Continuous,
                            ] {
                                ui.selectable_value(&mut lines.restart, restart, name(restart));
                            }
                        })
                        .response
                        .labelled_by(label.id);
                    ui.end_row();
                });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientation_turns_paper_and_margins() {
        let mut dialog = PageSetupDialog::default();
        dialog.show(PageSetupTab::Margins, None, None, None, false);
        assert!(dialog.whole);
        dialog.turn(Orientation::Landscape);
        assert_eq!(dialog.size, PageSize::A4.rotated());
        assert_eq!(dialog.margins.top, PageMargins::default().left);
        let PageSetup::Page { size, margins, .. } = dialog.setup() else {
            panic!("не то");
        };
        assert_eq!(size.orientation, Orientation::Landscape);
        assert_eq!(margins.header, PageMargins::default().header);
        // В документе из нескольких разделов — по умолчанию к разделу.
        dialog.show(
            PageSetupTab::Paper,
            Some(PageSize::LETTER),
            None,
            None,
            true,
        );
        dialog.lines_on = true;
        let PageSetup::Page { line_numbering, .. } = dialog.setup() else {
            panic!("не то");
        };
        assert_eq!(line_numbering, Some(LineNumbering::default()));
        assert!(!dialog.whole);
        assert_eq!(dialog.size, PageSize::LETTER);
    }
}
