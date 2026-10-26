//! «Вставка → Символы → Символ»: меню с недавними знаками и окно
//! «Символ» со всеми знаками по наборам Юникода и специальными знаками
//! (тире, неразрывные пробел и дефис, мягкий перенос). Окно только
//! собирает ответ; знак вставляет просмотрщик.

use eframe::egui::{
    self, Context, FontData, FontId, Id, Key, RichText, TextEdit, Ui, epaint::text::FontInsert,
    epaint::text::FontPriority, epaint::text::InsertFontFamily, vec2,
};

use crate::i18n::tr;
use crate::icons::{Icon, icon_button};
use crate::insert::menu_button;
use crate::keytips::{self, Level, TipAction};
use crate::ribbon::{RibbonCommand, RibbonInfo, big_label, row_group};

/// Знаки меню, пока недавних нет (как в Word).
const COMMON: [char; 20] = [
    '€', '£', '¥', '©', '®', '™', '±', '≠', '≤', '≥', '÷', '×', '∞', 'µ', 'α', 'β', 'π', 'Ω', '∑',
    '☺',
];

/// Наборы знаков окна: имя в файлах перевода и знаки Юникода.
pub const BLOCKS: [(&str, u32, u32); 13] = [
    ("symbol-block-latin1", 0x00A1, 0x00FF),
    ("symbol-block-latin-ext", 0x0100, 0x017F),
    ("symbol-block-greek", 0x0391, 0x03C9),
    ("symbol-block-cyrillic", 0x0400, 0x045F),
    ("symbol-block-punctuation", 0x2010, 0x205E),
    ("symbol-block-currency", 0x20A0, 0x20C0),
    ("symbol-block-letterlike", 0x2100, 0x214F),
    ("symbol-block-numbers", 0x2150, 0x218B),
    ("symbol-block-arrows", 0x2190, 0x21FF),
    ("symbol-block-math", 0x2200, 0x22FF),
    ("symbol-block-shapes", 0x25A0, 0x25FF),
    ("symbol-block-misc", 0x2600, 0x26FF),
    ("symbol-block-dingbats", 0x2700, 0x27BF),
];

/// Специальные знаки: знак, имя в файлах перевода и сочетание клавиш.
pub const SPECIAL: [(char, &str, &str); 14] = [
    ('—', "special-em-dash", ""),
    ('–', "special-en-dash", ""),
    ('\u{2011}', "special-nb-hyphen", "Ctrl+Shift+-"),
    ('\u{ad}', "special-soft-hyphen", "Ctrl+-"),
    ('\u{2003}', "special-em-space", ""),
    ('\u{2002}', "special-en-space", ""),
    ('\u{2005}', "special-quarter-space", ""),
    ('\u{a0}', "special-nbsp", "Ctrl+Shift+Space"),
    ('©', "special-copyright", ""),
    ('®', "special-registered", ""),
    ('™', "special-trademark", ""),
    ('§', "special-section", ""),
    ('¶', "special-paragraph", ""),
    ('…', "special-ellipsis", ""),
];

/// Сколько знаков в меню и сколько недавних помнится.
const RECENT: usize = 20;
/// Столбцов в сетке окна.
const COLUMNS: usize = 16;
/// Сторона клетки сетки, точки экрана.
const CELL: f32 = 28.0;

/// Где в памяти окна лежат недавние знаки.
fn recent_id() -> Id {
    Id::NULL.with("letter-recent-symbols")
}

/// Недавно вставленные знаки, последние — первыми.
pub fn recent(ctx: &Context) -> Vec<char> {
    ctx.data(|d| d.get_temp(recent_id()).unwrap_or_default())
}

/// Запоминает вставленный знак первым в списке недавних.
pub fn remember(ctx: &Context, c: char) {
    let mut list = recent(ctx);
    list.retain(|&x| x != c);
    list.insert(0, c);
    list.truncate(RECENT);
    ctx.data_mut(|d| d.insert_temp(recent_id(), list));
}

/// Знаки меню: сначала недавние, потом обычные.
fn menu_symbols(ctx: &Context) -> Vec<char> {
    let mut list = recent(ctx);
    for c in COMMON {
        if !list.contains(&c) {
            list.push(c);
        }
    }
    list.truncate(RECENT);
    list
}

/// Подключает к интерфейсу шрифт с греческими буквами, стрелками и
/// знаками математики, которых нет в шрифте egui: он нужен сетке
/// символов. Берётся последним, когда в своих шрифтах знака нет.
pub fn install(ctx: &Context) {
    let Some(data) = letter_layout::fonts::embedded_font("LiberationSans-Regular.ttf") else {
        return;
    };
    ctx.add_font(FontInsert::new(
        "letter-symbols",
        FontData::from_static(data),
        vec![InsertFontFamily {
            family: egui::FontFamily::Proportional,
            priority: FontPriority::Lowest,
        }],
    ));
}

/// Группа «Символы» вкладки «Вставка».
pub fn group_ui(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    row_group(ui, tr("group-symbols"), |ui| {
        ui.vertical(|ui| {
            let response = icon_button(
                ui,
                Icon::Equation,
                tr("equation"),
                vec2(44.0, 34.0),
                info.has_document,
                false,
            )
            .on_hover_text(tr("equation-insert-hint"));
            let command = RibbonCommand::InsertEquation;
            keytips::tip(
                ui,
                &response,
                Level::Commands,
                "keytip-insert-equation",
                TipAction::Command(command.clone()),
            );
            if response.clicked() {
                out.push(command);
            }
            big_label(ui, tr("equation"));
        });
        menu_button(
            ui,
            Icon::Symbol,
            tr("symbol"),
            tr("symbol-hint"),
            "keytip-insert-symbol",
            info.has_document,
            |ui| {
                let symbols = menu_symbols(ui.ctx());
                egui::Grid::new("letter-symbol-menu")
                    .spacing(vec2(2.0, 2.0))
                    .show(ui, |ui| {
                        for (i, c) in symbols.into_iter().enumerate() {
                            if i > 0 && i.is_multiple_of(5) {
                                ui.end_row();
                            }
                            let button = egui::Button::new(RichText::new(c.to_string()).size(16.0))
                                .min_size(vec2(CELL, CELL));
                            if ui.add(button).on_hover_text(code_text(c)).clicked() {
                                remember(ui.ctx(), c);
                                out.push(RibbonCommand::InsertSymbol(c));
                                ui.close();
                            }
                        }
                    });
                ui.separator();
                if ui.button(tr("symbol-more")).clicked() {
                    out.push(RibbonCommand::SymbolDialog);
                    ui.close();
                }
            },
        );
    });
}

/// Номер знака, как его пишет Word: `U+00A9`.
fn code_text(c: char) -> String {
    format!("U+{:04X}", c as u32)
}

/// Состояние окна «Символ».
#[derive(Debug, Clone, Default)]
pub struct SymbolDialog {
    /// Окно открыто.
    pub open: bool,
    /// Открыта страница «Специальные знаки».
    pub special: bool,
    /// Выбранный набор (номер в [`BLOCKS`]).
    pub block: usize,
    /// Выбранный знак.
    pub selected: Option<char>,
    /// Код знака в поле (шестнадцатеричный).
    pub code: String,
}

impl SymbolDialog {
    /// Открывает окно.
    pub fn show(&mut self) {
        self.open = true;
    }

    /// Выбирает знак и показывает его набор.
    pub fn select(&mut self, c: char) {
        self.selected = Some(c);
        self.code = format!("{:04X}", c as u32);
        if let Some(i) = BLOCKS
            .iter()
            .position(|(_, from, to)| (*from..=*to).contains(&(c as u32)))
        {
            self.block = i;
        }
    }

    /// Рисует окно и возвращает знак, который надо вставить.
    pub fn ui(&mut self, ctx: &Context) -> Option<char> {
        if !self.open {
            return None;
        }
        let mut insert = None;
        let mut close = false;
        let mut open = true;
        egui::Window::new(tr("symbol-title"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.special, false, tr("symbol-symbols"));
                    ui.selectable_value(&mut self.special, true, tr("symbol-special"));
                });
                ui.separator();
                if self.special {
                    insert = self.special_page(ui);
                } else {
                    insert = self.symbols_page(ui);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            self.selected.is_some(),
                            egui::Button::new(tr("symbol-insert")),
                        )
                        .clicked()
                    {
                        insert = self.selected;
                    }
                    if ui.button(tr("cancel")).clicked() {
                        close = true;
                    }
                });
            });
        if !open || close || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.open = false;
        }
        insert
    }

    /// Страница «Символы»: набор, сетка знаков и код выбранного.
    fn symbols_page(&mut self, ui: &mut Ui) -> Option<char> {
        let mut insert = None;
        ui.horizontal(|ui| {
            let label = ui.label(tr("symbol-set"));
            let current = BLOCKS.get(self.block).map_or("", |b| tr(b.0));
            egui::ComboBox::from_id_salt("letter-symbol-block")
                .selected_text(current)
                .width(220.0)
                .show_ui(ui, |ui| {
                    for (i, (name, ..)) in BLOCKS.iter().enumerate() {
                        ui.selectable_value(&mut self.block, i, tr(name));
                    }
                })
                .response
                .labelled_by(label.id);
        });
        let (from, to) = BLOCKS
            .get(self.block)
            .map_or((0x20, 0x7E), |&(_, from, to)| (from, to));
        let chars: Vec<char> = (from..=to)
            .filter_map(char::from_u32)
            .filter(|c| !c.is_control())
            .collect();
        egui::ScrollArea::vertical()
            .max_height(CELL * 8.0 + 16.0)
            .min_scrolled_height(CELL * 8.0 + 16.0)
            .show(ui, |ui| {
                egui::Grid::new(("letter-symbol-grid", self.block))
                    .spacing(vec2(1.0, 1.0))
                    .show(ui, |ui| {
                        for (i, c) in chars.into_iter().enumerate() {
                            if i > 0 && i.is_multiple_of(COLUMNS) {
                                ui.end_row();
                            }
                            let chosen = self.selected == Some(c);
                            let button = egui::Button::selectable(
                                chosen,
                                RichText::new(c.to_string()).font(FontId::proportional(16.0)),
                            )
                            .min_size(vec2(CELL, CELL));
                            let response = ui.add(button).on_hover_text(code_text(c));
                            if response.clicked() {
                                self.select(c);
                            }
                            if response.double_clicked() {
                                insert = Some(c);
                            }
                        }
                    });
            });
        ui.horizontal(|ui| {
            let label = ui.label(tr("symbol-code"));
            let field = ui
                .add(
                    TextEdit::singleline(&mut self.code)
                        .desired_width(70.0)
                        .char_limit(6)
                        .id_salt("letter-symbol-code"),
                )
                .labelled_by(label.id);
            if field.changed()
                && let Some(c) = parse_code(&self.code)
            {
                let code = self.code.clone();
                self.select(c);
                self.code = code;
            }
            if let Some(c) = self.selected {
                ui.label(RichText::new(c.to_string()).size(28.0));
            }
        });
        insert
    }

    /// Страница «Специальные знаки»: список знаков с именами и клавишами.
    fn special_page(&mut self, ui: &mut Ui) -> Option<char> {
        let mut insert = None;
        egui::Grid::new("letter-special-chars")
            .striped(true)
            .min_col_width(40.0)
            .show(ui, |ui| {
                for (c, name, keys) in SPECIAL {
                    let shown = match c {
                        '\u{ad}' | '\u{2011}' => '-',
                        c if c.is_whitespace() => '·',
                        c => c,
                    };
                    let chosen = self.selected == Some(c);
                    let row = ui.selectable_label(chosen, shown.to_string());
                    let label = ui.selectable_label(chosen, tr(name));
                    ui.label(keys);
                    ui.end_row();
                    for response in [row, label] {
                        if response.clicked() {
                            self.selected = Some(c);
                        }
                        if response.double_clicked() {
                            insert = Some(c);
                        }
                    }
                }
            });
        insert
    }
}

/// Знак по шестнадцатеричному коду (`00A9`, `U+00A9`).
fn parse_code(text: &str) -> Option<char> {
    let text = text.trim();
    let text = text
        .strip_prefix("U+")
        .or_else(|| text.strip_prefix("u+"))
        .unwrap_or(text);
    let c = char::from_u32(u32::from_str_radix(text, 16).ok()?)?;
    (!c.is_control()).then_some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_parsed() {
        assert_eq!(parse_code("00A9"), Some('©'));
        assert_eq!(parse_code("U+03a9"), Some('Ω'));
        assert_eq!(parse_code("zz"), None);
        assert_eq!(parse_code("0007"), None);
        assert_eq!(code_text('€'), "U+20AC");
    }

    #[test]
    fn recent_symbols_come_first() {
        let ctx = Context::default();
        assert_eq!(menu_symbols(&ctx), COMMON);
        remember(&ctx, '§');
        remember(&ctx, '©');
        let menu = menu_symbols(&ctx);
        assert_eq!(&menu[..3], ['©', '§', '€']);
        assert_eq!(menu.len(), RECENT);
        assert_eq!(menu.iter().filter(|&&c| c == '©').count(), 1);
    }

    #[test]
    fn selecting_a_symbol_shows_its_set() {
        let mut dialog = SymbolDialog::default();
        dialog.select('→');
        assert_eq!(BLOCKS[dialog.block].0, "symbol-block-arrows");
        assert_eq!(dialog.code, "2192");
    }

    #[test]
    fn dialog_inserts_the_chosen_symbol() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;
        let mut dialog = SymbolDialog::default();
        dialog.show();
        let mut h = Harness::new_ui_state(
            |ui, state: &mut (SymbolDialog, Vec<char>)| {
                if let Some(c) = state.0.ui(ui.ctx()) {
                    state.1.push(c);
                }
            },
            (dialog, Vec::new()),
        );
        h.run();
        h.get_by_label("©").click();
        h.run();
        assert_eq!(h.state().0.code, "00A9");
        h.get_by_label("Вставить").click();
        h.run();
        // Специальные знаки: двойной щелчок по имени вставляет знак.
        h.get_by_label("Специальные знаки").click();
        h.run();
        h.get_by_label("Длинное тире").click();
        h.run();
        h.get_by_label("Вставить").click();
        h.run();
        assert_eq!(h.state().1, ['©', '—']);
        assert!(h.state().0.open);
    }

    #[test]
    fn names_are_translated() {
        let names = BLOCKS
            .iter()
            .map(|b| b.0)
            .chain(SPECIAL.iter().map(|s| s.1));
        for name in names {
            assert!(crate::i18n::has(name), "нет перевода {name}");
        }
    }
}
