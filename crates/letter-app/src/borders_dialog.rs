//! Окно «Границы и заливка», как в Word: вкладка «Граница» — линии
//! абзацев, выделенных ячеек или всей таблицы, «Страница» — рамка
//! страниц, «Заливка» — цвет фона. Вид, цвет и толщина линии выбираются
//! отдельно, стороны — флажками у образца. Окно только собирает ответ;
//! документ меняет просмотрщик.

use eframe::egui::{self, Color32, Context, Sense, Stroke, Ui, pos2, vec2};
use letter_editor::Command;
use letter_model::{Border, Borders, Color, PageBorders, ParagraphBorders};

use crate::i18n::tr;

/// Виды линий: значение `w:val` и имя подписи.
pub const STYLES: [(&str, &str); 7] = [
    ("single", "border-line-single"),
    ("dotted", "border-line-dotted"),
    ("dashed", "border-line-dashed"),
    ("dotDash", "border-line-dot-dash"),
    ("dotDotDash", "border-line-dot-dot-dash"),
    ("double", "border-line-double"),
    ("triple", "border-line-triple"),
];

/// Толщины линий, как в списке Word: восьмые доли пункта и подпись.
pub const WIDTHS: [(u32, &str); 9] = [
    (2, "¼"),
    (4, "½"),
    (6, "¾"),
    (8, "1"),
    (12, "1½"),
    (18, "2¼"),
    (24, "3"),
    (36, "4½"),
    (48, "6"),
];

/// Вкладка окна.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BordersTab {
    /// «Граница».
    #[default]
    Borders,
    /// «Страница».
    Page,
    /// «Заливка».
    Shading,
}

/// К чему применить линии и заливку.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Target {
    /// К выделенным абзацам.
    #[default]
    Paragraph,
    /// К выделенным ячейкам.
    Cells,
    /// Ко всей таблице.
    Table,
}

/// Линия: номер вида в [`STYLES`], толщины в [`WIDTHS`] и цвет
/// (`None` — «Авто»).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line {
    /// Вид.
    pub style: usize,
    /// Толщина.
    pub width: usize,
    /// Цвет.
    pub color: Option<(u8, u8, u8)>,
}

impl Default for Line {
    fn default() -> Line {
        Line {
            style: 0,
            width: 1,
            color: None,
        }
    }
}

impl Line {
    /// Линия границы с отступом `space` пт от текста.
    pub fn border(self, space: u32) -> Border {
        let (style, _) = STYLES[self.style.min(STYLES.len() - 1)];
        let (eighths, _) = WIDTHS[self.width.min(WIDTHS.len() - 1)];
        Border {
            style: style.into(),
            size: Some(eighths),
            space: Some(space),
            color: Some(match self.color {
                Some((r, g, b)) => Color::Rgb(r, g, b),
                None => Color::Auto,
            }),
            theme_color: None,
        }
    }

    /// Линия как у границы `border` (непохожий вид — сплошная, толщина —
    /// ближайшая из списка).
    pub fn of(border: &Border) -> Line {
        let size = border.size.unwrap_or(4);
        let width = (0..WIDTHS.len())
            .min_by_key(|&i| WIDTHS[i].0.abs_diff(size))
            .unwrap_or(1);
        Line {
            style: STYLES
                .iter()
                .position(|(s, _)| *s == border.style)
                .unwrap_or(0),
            width,
            color: match border.color {
                Some(Color::Rgb(r, g, b)) => Some((r, g, b)),
                _ => None,
            },
        }
    }
}

/// Убранная линия (`nil`).
fn nil() -> Border {
    Border {
        style: "nil".into(),
        size: None,
        space: None,
        color: None,
        theme_color: None,
    }
}

/// Стороны на образце: сверху, снизу, слева, справа, внутри по
/// горизонтали (у абзацев — между абзацами), внутри по вертикали.
const SIDES: [&str; 6] = [
    "border-top",
    "border-bottom",
    "border-left",
    "border-right",
    "border-inside-h",
    "border-inside-v",
];

/// Что известно о месте с курсором, когда окно открывается.
#[derive(Debug, Clone, Default)]
pub struct BordersContext {
    /// Курсор в таблице.
    pub in_table: bool,
    /// Итоговые границы абзаца с курсором.
    pub paragraph: ParagraphBorders,
    /// Свои границы ячейки с курсором.
    pub cell: Option<Borders>,
    /// Границы страниц раздела с курсором.
    pub page: Option<PageBorders>,
}

/// Состояние окна.
#[derive(Debug, Clone, Default)]
pub struct BordersDialog {
    /// Окно открыто.
    pub open: bool,
    /// Открытая вкладка.
    pub tab: BordersTab,
    in_table: bool,
    /// К чему применить.
    pub target: Target,
    /// Линия вкладки «Граница».
    pub line: Line,
    /// Стороны с линией (см. [`SIDES`]).
    pub sides: [bool; 6],
    pub(crate) borders_touched: bool,
    /// Линия рамки страниц.
    pub page_line: Line,
    /// Рамка страниц есть.
    pub page_on: bool,
    pub(crate) page_touched: bool,
    /// Цвет заливки; `None` — нет.
    pub fill: Option<(u8, u8, u8)>,
    pub(crate) fill_touched: bool,
}

impl BordersDialog {
    /// Открывает окно на вкладке `tab` для места с курсором.
    pub fn show(&mut self, tab: BordersTab, context: &BordersContext) {
        let in_table = context.in_table;
        *self = BordersDialog {
            open: true,
            tab,
            in_table,
            target: if in_table {
                Target::Cells
            } else {
                Target::Paragraph
            },
            ..BordersDialog::default()
        };
        let visible = |b: &Option<Border>| b.as_ref().filter(|b| !b.is_none()).cloned();
        let sides: [Option<Border>; 6] = match &context.cell {
            Some(c) if in_table => [
                visible(&c.top),
                visible(&c.bottom),
                visible(&c.left),
                visible(&c.right),
                visible(&c.inside_h),
                visible(&c.inside_v),
            ],
            _ if in_table => Default::default(),
            _ => {
                let p = &context.paragraph;
                [
                    visible(&p.top),
                    visible(&p.bottom),
                    visible(&p.left),
                    visible(&p.right),
                    visible(&p.between),
                    None,
                ]
            }
        };
        self.sides = sides.clone().map(|s| s.is_some());
        if let Some(first) = sides.iter().flatten().next() {
            self.line = Line::of(first);
        }
        if let Some(page) = &context.page
            && let Some(line) = [&page.top, &page.left, &page.bottom, &page.right]
                .into_iter()
                .find_map(visible)
        {
            self.page_on = true;
            self.page_line = Line::of(&line);
        }
    }

    /// Правки документа по ответу окна: только то, что меняли.
    pub fn commands(&self) -> Vec<Command> {
        let mut out = Vec::new();
        let on = |i: usize, space: u32| self.sides[i].then(|| self.line.border(space));
        if self.borders_touched {
            match self.target {
                Target::Paragraph => {
                    out.push(Command::ParagraphBorders(Box::new(ParagraphBorders {
                        top: on(0, 1),
                        bottom: on(1, 1),
                        left: on(2, 4),
                        right: on(3, 4),
                        between: on(4, 1),
                        bar: None,
                    })));
                }
                Target::Cells | Target::Table => {
                    let side = |i: usize| Some(on(i, 0).unwrap_or_else(nil));
                    let borders = Borders {
                        top: side(0),
                        bottom: side(1),
                        left: side(2),
                        right: side(3),
                        inside_h: side(4),
                        inside_v: side(5),
                    };
                    out.push(if self.target == Target::Cells {
                        Command::CellBorderLines(Box::new(borders))
                    } else {
                        Command::TableBorders(Box::new(borders))
                    });
                }
            }
        }
        if self.fill_touched {
            let color = self.fill.map(|(r, g, b)| Color::Rgb(r, g, b));
            out.push(match self.target {
                Target::Paragraph => {
                    Command::Paragraph(letter_editor::ParagraphFormat::Shading(self.fill))
                }
                Target::Cells => Command::CellShading(color),
                Target::Table => Command::TableShading(color),
            });
        }
        if self.page_touched {
            let line = self.page_line.border(24);
            let borders = self.page_on.then(|| {
                Box::new(PageBorders {
                    top: Some(line.clone()),
                    left: Some(line.clone()),
                    bottom: Some(line.clone()),
                    right: Some(line.clone()),
                    from_page_edge: true,
                    ..PageBorders::default()
                })
            });
            out.push(Command::PageBorders(borders));
        }
        out
    }

    /// Рисует окно; по «ОК» возвращает правки документа.
    pub fn ui(&mut self, ctx: &Context) -> Option<Vec<Command>> {
        if !self.open {
            return None;
        }
        let mut done = None;
        let mut close = false;
        let mut open = true;
        egui::Window::new(tr("borders-dialog-title"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (tab, label) in [
                        (BordersTab::Borders, "borders-tab-borders"),
                        (BordersTab::Page, "borders-tab-page"),
                        (BordersTab::Shading, "borders-tab-shading"),
                    ] {
                        ui.selectable_value(&mut self.tab, tab, tr(label));
                    }
                });
                ui.separator();
                match self.tab {
                    BordersTab::Borders => self.borders_tab(ui),
                    BordersTab::Page => self.page_tab(ui),
                    BordersTab::Shading => self.shading_tab(ui),
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(tr("ok")).clicked() {
                        done = Some(self.commands());
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

    fn borders_tab(&mut self, ui: &mut Ui) {
        let table = self.target != Target::Paragraph;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(tr("borders-kind"));
                let mut presets = vec![("borders-none", [false; 6]), ("borders-box", BOX)];
                if table {
                    presets.push(("borders-all", [true; 6]));
                }
                for (label, sides) in presets {
                    if ui
                        .selectable_label(self.sides == sides, tr(label))
                        .clicked()
                    {
                        self.sides = sides;
                        self.borders_touched = true;
                    }
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                if line_settings(ui, "borders-line", &mut self.line) {
                    self.borders_touched = true;
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.label(tr("borders-preview"));
                let count = if table { 6 } else { 5 };
                preview(ui, &self.line, &self.sides, table);
                for (i, side) in SIDES.iter().enumerate().take(count) {
                    let label = if i == 4 && !table {
                        tr("border-between")
                    } else {
                        tr(side)
                    };
                    if ui.checkbox(&mut self.sides[i], label).changed() {
                        self.borders_touched = true;
                    }
                }
            });
        });
        self.target_choice(ui);
    }

    fn page_tab(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(tr("borders-kind"));
                for (label, on) in [("borders-none", false), ("borders-box", true)] {
                    if ui.selectable_label(self.page_on == on, tr(label)).clicked() {
                        self.page_on = on;
                        self.page_touched = true;
                    }
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                if line_settings(ui, "borders-page-line", &mut self.page_line) {
                    self.page_on = true;
                    self.page_touched = true;
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.label(tr("borders-preview"));
                let sides = if self.page_on { BOX } else { [false; 6] };
                preview(ui, &self.page_line, &sides, false);
            });
        });
        ui.label(egui::RichText::new(tr("borders-page-note")).small());
    }

    fn shading_tab(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(tr("borders-fill"));
            let (fill, name) = match self.fill {
                Some(rgb) => (
                    crate::home::color32(rgb),
                    format!("#{:02X}{:02X}{:02X}", rgb.0, rgb.1, rgb.2),
                ),
                None => (Color32::TRANSPARENT, tr("shading-none").to_string()),
            };
            crate::home::swatch(ui, fill, &name);
            ui.menu_button(name.clone(), |ui| {
                crate::insert::shading_palette(ui, |picked| {
                    self.fill = picked;
                    self.fill_touched = true;
                });
            });
        });
        self.target_choice(ui);
    }

    /// «Применить к»: абзацу, ячейкам или таблице.
    fn target_choice(&mut self, ui: &mut Ui) {
        let mut targets = vec![(Target::Paragraph, "borders-apply-paragraph")];
        if self.in_table {
            targets.push((Target::Cells, "borders-apply-cells"));
            targets.push((Target::Table, "borders-apply-table"));
        }
        ui.horizontal(|ui| {
            let label = ui.label(tr("borders-apply-to"));
            let current = targets
                .iter()
                .find(|(t, _)| *t == self.target)
                .map_or("", |(_, l)| *l);
            egui::ComboBox::from_id_salt("letter-borders-target")
                .selected_text(tr(current))
                .show_ui(ui, |ui| {
                    for (target, name) in &targets {
                        ui.selectable_value(&mut self.target, *target, tr(name));
                    }
                })
                .response
                .labelled_by(label.id);
        });
    }
}

/// Рамка: четыре стороны без внутренних линий.
const BOX: [bool; 6] = [true, true, true, true, false, false];

/// Вид, цвет и толщина линии; `true`, если что-то поменяли.
fn line_settings(ui: &mut Ui, id: &str, line: &mut Line) -> bool {
    let before = *line;
    ui.label(tr("borders-line-style"));
    egui::ComboBox::from_id_salt((id, "style"))
        .selected_text(tr(STYLES[line.style.min(STYLES.len() - 1)].1))
        .show_ui(ui, |ui| {
            for (i, (_, name)) in STYLES.iter().enumerate() {
                ui.selectable_value(&mut line.style, i, tr(name));
            }
        });
    ui.label(tr("borders-line-color"));
    let (swatch, name) = match line.color {
        Some(rgb) => (
            crate::home::color32(rgb),
            format!("#{:02X}{:02X}{:02X}", rgb.0, rgb.1, rgb.2),
        ),
        None => (Color32::BLACK, tr("color-auto").to_string()),
    };
    ui.horizontal(|ui| {
        crate::home::swatch(ui, swatch, &name);
        ui.menu_button(name.clone(), |ui| {
            if ui.button(tr("color-auto")).clicked() {
                line.color = None;
                ui.close();
            }
            crate::insert::shading_palette(ui, |picked| {
                if picked.is_some() {
                    line.color = picked;
                }
            });
        });
    });
    ui.label(tr("borders-line-width"));
    let points = |(_, label): (u32, &str)| format!("{label} {}", tr("points-short"));
    egui::ComboBox::from_id_salt((id, "width"))
        .selected_text(points(WIDTHS[line.width.min(WIDTHS.len() - 1)]))
        .show_ui(ui, |ui| {
            for (i, width) in WIDTHS.iter().enumerate() {
                ui.selectable_value(&mut line.width, i, points(*width));
            }
        });
    *line != before
}

/// Образец: «текст» серыми полосками и выбранные линии по сторонам.
fn preview(ui: &mut Ui, line: &Line, sides: &[bool; 6], table: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(120.0, 90.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    let inner = rect.shrink(12.0);
    for k in 0..5 {
        let y = inner.top() + 8.0 + k as f32 * 12.0;
        painter.line_segment(
            [pos2(inner.left() + 6.0, y), pos2(inner.right() - 6.0, y)],
            Stroke::new(3.0, Color32::from_gray(200)),
        );
    }
    let color = line.color.map_or(Color32::BLACK, crate::home::color32);
    let width = (WIDTHS[line.width.min(WIDTHS.len() - 1)].0 as f32 / 8.0).clamp(1.0, 4.0);
    let style = STYLES[line.style.min(STYLES.len() - 1)].0;
    let segment = |a: egui::Pos2, b: egui::Pos2| draw_line(&painter, a, b, style, width, color);
    let middle = inner.center();
    let lines: [(egui::Pos2, egui::Pos2); 6] = [
        (inner.left_top(), inner.right_top()),
        (inner.left_bottom(), inner.right_bottom()),
        (inner.left_top(), inner.left_bottom()),
        (inner.right_top(), inner.right_bottom()),
        (pos2(inner.left(), middle.y), pos2(inner.right(), middle.y)),
        (pos2(middle.x, inner.top()), pos2(middle.x, inner.bottom())),
    ];
    for (i, (a, b)) in lines.into_iter().enumerate() {
        if sides[i] && (table || i < 5) {
            segment(a, b);
        }
    }
}

/// Линия образца нужного вида.
fn draw_line(
    painter: &egui::Painter,
    a: egui::Pos2,
    b: egui::Pos2,
    style: &str,
    width: f32,
    color: Color32,
) {
    let stroke = Stroke::new(width, color);
    match style {
        "dotted" => {
            painter.extend(egui::Shape::dotted_line(
                &[a, b],
                color,
                width * 2.0,
                width / 2.0,
            ));
        }
        "dashed" | "dotDash" | "dotDotDash" => {
            painter.extend(egui::Shape::dashed_line(
                &[a, b],
                stroke,
                width * 4.0,
                width * 2.0,
            ));
        }
        "double" | "triple" => {
            let along = (b - a).normalized();
            let across = vec2(-along.y, along.x) * (width + 1.0);
            let count = if style == "double" { 2 } else { 3 };
            for k in 0..count {
                let shift = across * (k as f32 - (count - 1) as f32 / 2.0);
                painter.line_segment([a + shift, b + shift], Stroke::new(1.0, color));
            }
        }
        _ => {
            painter.line_segment([a, b], stroke);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_become_commands() {
        let mut dialog = BordersDialog::default();
        dialog.show(BordersTab::Borders, &BordersContext::default());
        // Ничего не меняли — ничего не правим.
        assert!(dialog.commands().is_empty());
        dialog.line = Line {
            style: 5,
            width: 3,
            color: Some((255, 0, 0)),
        };
        dialog.sides = BOX;
        dialog.borders_touched = true;
        let [Command::ParagraphBorders(borders)] = &dialog.commands()[..] else {
            panic!("нет границ абзаца");
        };
        let top = borders.top.clone().unwrap();
        assert_eq!(top.style, "double");
        assert_eq!(top.size, Some(8));
        assert_eq!(top.color, Some(Color::Rgb(255, 0, 0)));
        assert_eq!(borders.left.as_ref().and_then(|b| b.space), Some(4));
        assert!(borders.between.is_none());

        // В таблице — по умолчанию к ячейкам; убранные стороны — `nil`.
        let context = BordersContext {
            in_table: true,
            cell: Some(Borders::all(Border::single(12))),
            ..BordersContext::default()
        };
        dialog.show(BordersTab::Shading, &context);
        assert_eq!(dialog.target, Target::Cells);
        assert_eq!(dialog.sides, [true; 6]);
        assert_eq!(dialog.line.width, 4);
        dialog.sides[5] = false;
        dialog.borders_touched = true;
        dialog.fill = Some((1, 2, 3));
        dialog.fill_touched = true;
        dialog.target = Target::Table;
        let commands = dialog.commands();
        let [Command::TableBorders(lines), Command::TableShading(fill)] = &commands[..] else {
            panic!("{commands:?}");
        };
        assert!(lines.inside_v.as_ref().is_some_and(Border::is_none));
        assert_eq!(*fill, Some(Color::Rgb(1, 2, 3)));

        // Рамка страниц.
        dialog.show(BordersTab::Page, &context);
        dialog.page_on = true;
        dialog.page_touched = true;
        let [Command::PageBorders(Some(page))] = &dialog.commands()[..] else {
            panic!("нет рамки страниц");
        };
        assert!(page.from_page_edge);
        assert_eq!(page.top.as_ref().and_then(|b| b.space), Some(24));
    }
}
