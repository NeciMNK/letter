//! Вкладка «Уравнение»: видна, когда курсор стоит у формулы или формула
//! выделена. Формула правится в линейном виде (как «Линейный формат»
//! Word: `a/b`, `x^2`, `√x`, `\alpha`) — документ меняется сразу при
//! наборе. Группы «Символы» и «Структуры» вставляют знаки и готовые
//! заготовки в место курсора в поле.

use eframe::egui::{self, Context, Id, RichText, TextEdit, Ui, vec2};
use letter_editor::Position;
use letter_model::Math;

use crate::i18n::tr;
use crate::keytips::{self, Level, TipAction};
use crate::ribbon::{RibbonCommand, RibbonInfo, big_label, group, row_group};

/// Подсказки клавиш вкладки (для проверки).
#[cfg(test)]
pub const KEYTIPS: [&str; 10] = [
    "keytip-equation-text",
    "keytip-equation-display",
    "keytip-equation-fraction",
    "keytip-equation-script",
    "keytip-equation-radical",
    "keytip-equation-integral",
    "keytip-equation-large-operator",
    "keytip-equation-bracket",
    "keytip-equation-function",
    "keytip-equation-matrix",
];

/// Знаки группы «Символы» (все есть в шрифте символов интерфейса).
const SYMBOLS: [char; 24] = [
    '±', '∞', '≠', '≈', '×', '÷', '≤', '≥', '≡', '∂', '∆', '√', '∑', '∏', '∫', '∩', '←', '→', '↔',
    '⇔', 'α', 'β', 'π', 'θ',
];

/// Кнопка группы «Структуры»: знак на кнопке, имя (подпись и подсказка
/// клавиш — `equation-…` и `keytip-equation-…`) и заготовки в меню.
struct Structure {
    glyph: &'static str,
    name: &'static str,
    templates: &'static [&'static str],
}

/// Кнопки группы «Структуры», как в Word.
const STRUCTURES: [Structure; 8] = [
    Structure {
        glyph: "x/y",
        name: "fraction",
        templates: &["a/b", "dy/dx", "a¦b"],
    },
    Structure {
        glyph: "eˣ",
        name: "script",
        templates: &["x^2", "x_i", "x_i^2", "e^(-x)"],
    },
    Structure {
        glyph: "√x",
        name: "radical",
        templates: &["√x", "√(n&x)", "√(3&x)", "(-b±√(b^2-4ac))/2a"],
    },
    Structure {
        glyph: "∫",
        name: "integral",
        templates: &["∫▒(f(x)dx)", "∫_a^b▒(f(x)dx)", "∫_0^∞▒(e^(-x)dx)"],
    },
    Structure {
        glyph: "∑",
        name: "large-operator",
        templates: &["∑_(i=1)^n▒a_i", "∏_(i=1)^n▒a_i", "∑_(k=0)^∞▒x^k"],
    },
    Structure {
        glyph: "{()}",
        name: "bracket",
        templates: &["(x)", "[x]", "{x}", "|x|", "‖x‖"],
    },
    Structure {
        glyph: "sin",
        name: "function",
        templates: &["sin⁡θ", "cos⁡θ", "tan⁡θ", "log⁡x", "ln⁡x", "lim┬(n→∞)⁡a_n"],
    },
    Structure {
        glyph: "[■]",
        name: "matrix",
        templates: &["■(a&b@c&d)", "(■(1&0@0&1))", "█(x+y=1@x-y=0)"],
    },
];

/// Текст поля между кадрами: какую формулу он правит и с чего начат.
#[derive(Debug, Clone, Default)]
struct Buffer {
    /// Место формулы.
    at: Option<Position>,
    /// Линейный вид формулы, с которым поле совпадает по смыслу: пока
    /// формула в документе та же, текст поля не сбрасывается (иначе
    /// пропадали бы пробелы и недописанные `\имена`).
    source: String,
    /// Текст в поле.
    text: String,
}

/// Вкладка «Уравнение».
pub fn equation(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let Some((at, math)) = &info.equation else {
        return;
    };
    let buffer_id = Id::NULL.with("letter-equation-buffer");
    let field = field_id();
    let mut buffer: Buffer = ui.data(|d| d.get_temp(buffer_id)).unwrap_or_default();
    let linear = math.linear_text();
    if buffer.at != Some(*at) || buffer.source != linear {
        buffer = Buffer {
            at: Some(*at),
            source: linear.clone(),
            text: linear,
        };
    }
    let before = buffer.text.clone();
    let mut display = math.display;
    group(ui, tr("group-equation-tools"), |ui| {
        ui.label(tr("equation-linear"));
        let response = ui
            .add(
                TextEdit::singleline(&mut buffer.text)
                    .id(field)
                    .desired_width(230.0)
                    .hint_text(tr("equation-hint")),
            )
            .on_hover_text(tr("equation-linear-hint"));
        keytips::tip(
            ui,
            &response,
            Level::Commands,
            "keytip-equation-text",
            TipAction::Focus(response.id),
        );
        let response = ui
            .checkbox(&mut display, tr("equation-display"))
            .on_hover_text(tr("equation-display-hint"));
        let toggled = Math {
            display: !math.display,
            ..math.clone()
        };
        keytips::tip(
            ui,
            &response,
            Level::Commands,
            "keytip-equation-display",
            TipAction::Command(RibbonCommand::SetEquation(*at, Box::new(toggled))),
        );
    });
    group(ui, tr("group-equation-symbols"), |ui| {
        egui::Grid::new("letter-equation-symbols")
            .spacing(vec2(1.0, 1.0))
            .min_col_width(20.0)
            .show(ui, |ui| {
                for (i, c) in SYMBOLS.into_iter().enumerate() {
                    if i > 0 && i.is_multiple_of(8) {
                        ui.end_row();
                    }
                    let button = egui::Button::new(c.to_string()).min_size(vec2(20.0, 20.0));
                    if ui.add(button).clicked() {
                        insert_at_cursor(ui.ctx(), field, &mut buffer.text, &c.to_string());
                    }
                }
            });
    });
    row_group(ui, tr("group-equation-structures"), |ui| {
        for structure in &STRUCTURES {
            ui.vertical(|ui| {
                let label = tr(&format!("equation-{}", structure.name));
                let button = egui::Button::new(RichText::new(structure.glyph).size(18.0))
                    .min_size(vec2(36.0, 34.0));
                let response = ui.add(button).on_hover_text(label);
                let popup = egui::Popup::default_response_id(&response);
                keytips::tip(
                    ui,
                    &response,
                    Level::Commands,
                    &format!("keytip-equation-{}", structure.name),
                    TipAction::Popup(popup),
                );
                egui::Popup::menu(&response).show(|ui| {
                    for template in structure.templates {
                        // Знак применения функции невидим — в меню пробел.
                        let shown = template.replace('\u{2061}', " ");
                        if ui.button(RichText::new(shown).size(15.0)).clicked() {
                            insert_at_cursor(ui.ctx(), field, &mut buffer.text, template);
                            ui.close();
                        }
                    }
                });
                big_label(ui, label);
            });
        }
    });
    if buffer.text != before {
        autocorrect(ui.ctx(), field, &mut buffer.text);
    }
    if buffer.text != before || display != math.display {
        let mut edited = Math::from_linear(&buffer.text, display);
        edited.align = math.align;
        buffer.source = edited.linear_text();
        out.push(RibbonCommand::SetEquation(*at, Box::new(edited)));
    }
    ui.data_mut(|d| d.insert_temp(buffer_id, buffer));
}

/// Заменяет в поле законченные `\имя` знаками; курсор остаётся на том
/// же месте от конца текста.
fn autocorrect(ctx: &Context, field: Id, text: &mut String) {
    let corrected = letter_model::autocorrect_linear(text);
    if corrected == *text {
        return;
    }
    let old_len = text.chars().count();
    *text = corrected;
    let Some(mut state) = TextEdit::load_state(ctx, field) else {
        return;
    };
    if let Some(range) = state.cursor.char_range() {
        let tail = old_len.saturating_sub(range.primary.index.0);
        let at = text.chars().count().saturating_sub(tail);
        let cursor = egui::text::CCursor::new(at);
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(cursor)));
        TextEdit::store_state(ctx, field, state);
    }
}

/// Поле линейного вида: в него ставится курсор после «Вставка →
/// Уравнение».
pub fn field_id() -> Id {
    Id::NULL.with("letter-equation-text")
}

/// Вставляет `piece` в `text` в место курсора поля `field` (или в
/// конец), ставит курсор за вставленным и возвращает фокус в поле.
fn insert_at_cursor(ctx: &Context, field: Id, text: &mut String, piece: &str) {
    let mut state = TextEdit::load_state(ctx, field).unwrap_or_default();
    let chars = text.chars().count();
    let at = state
        .cursor
        .char_range()
        .map_or(chars, |range| range.primary.index.0.min(chars));
    let byte = text.char_indices().nth(at).map_or(text.len(), |(i, _)| i);
    text.insert_str(byte, piece);
    let after = egui::text::CCursor::new(at + piece.chars().count());
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(after)));
    TextEdit::store_state(ctx, field, state);
    ctx.memory_mut(|m| m.request_focus(field));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_parse_back() {
        for structure in &STRUCTURES {
            for template in structure.templates {
                let math = Math::from_linear(template, false);
                assert!(!math.lines[0].is_empty(), "{template}");
                // Заготовка — не просто текст: в ней есть строение.
                let plain = math.lines[0]
                    .iter()
                    .all(|n| matches!(n, letter_model::MathNode::Text { .. }));
                assert!(!plain, "{template}");
            }
        }
    }

    #[test]
    fn every_label_has_glyphs() {
        let ctx = Context::default();
        crate::symbols::install(&ctx);
        let mut output = ctx.run_ui(Default::default(), |_| {});
        output.textures_delta.clear();
        let font = egui::FontId::proportional(14.0);
        let mut labels: Vec<String> = SYMBOLS.iter().map(char::to_string).collect();
        for structure in &STRUCTURES {
            labels.push(structure.glyph.to_owned());
            labels.extend(
                structure
                    .templates
                    .iter()
                    .map(|t| t.replace('\u{2061}', " ")),
            );
        }
        for label in labels {
            assert!(ctx.fonts_mut(|f| f.has_glyphs(&font, &label)), "{label}");
        }
    }

    #[test]
    fn symbols_go_to_the_cursor() {
        let ctx = Context::default();
        let field = Id::NULL.with("field");
        let mut text = String::from("ab");
        insert_at_cursor(&ctx, field, &mut text, "α");
        assert_eq!(text, "abα");
        let mut state = TextEdit::load_state(&ctx, field).unwrap_or_default();
        let one = egui::text::CCursor::new(1);
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(one)));
        TextEdit::store_state(&ctx, field, state);
        insert_at_cursor(&ctx, field, &mut text, "√x");
        assert_eq!(text, "a√xbα");
    }
}
