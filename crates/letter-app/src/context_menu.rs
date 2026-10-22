//! Меню по правой кнопке мыши над текстом: вырезать, копировать,
//! параметры вставки, шрифт, абзац — как в Word.

use crate::i18n::tr;
use eframe::egui::{Ui, vec2};

use crate::clipboard::PasteMode;
use crate::icons::{Icon, icon_button, icon_text_button};
use crate::ribbon::RibbonCommand;

/// Параметры вставки: значок, название, способ.
const PASTE_OPTIONS: [(Icon, &str, PasteMode); 3] = [
    (Icon::PasteKeep, "paste-keep", PasteMode::KeepSource),
    (Icon::PasteMerge, "paste-merge", PasteMode::Merge),
    (Icon::PasteText, "paste-text", PasteMode::TextOnly),
];

/// Содержимое меню. `has_selection` — что-то выделено (иначе вырезать
/// и копировать нечего). `numbered` — курсор в нумерованном списке
/// (`true` — выше есть список, который можно продолжить). Выбранное
/// добавляется в `out`.
pub fn context_menu(
    ui: &mut Ui,
    has_selection: bool,
    numbered: Option<bool>,
    out: &mut Vec<RibbonCommand>,
) {
    ui.set_min_width(200.0);
    let mut item = |ui: &mut Ui, icon, name: &str, enabled, command| {
        if icon_text_button(ui, icon, name, enabled).clicked() {
            out.push(command);
            ui.close();
        }
    };
    item(ui, Icon::Cut, tr("cut"), has_selection, RibbonCommand::Cut);
    item(
        ui,
        Icon::Copy,
        tr("copy"),
        has_selection,
        RibbonCommand::Copy,
    );
    ui.label(tr("paste-options"));
    ui.horizontal(|ui| {
        for (icon, name, mode) in PASTE_OPTIONS {
            let name = tr(name);
            if icon_button(ui, icon, name, vec2(28.0, 28.0), true, false)
                .on_hover_text(name)
                .clicked()
            {
                out.push(RibbonCommand::PasteSpecial(mode));
                ui.close();
            }
        }
    });
    ui.separator();
    if ui.button(tr("font-dialog-menu")).clicked() {
        out.push(RibbonCommand::FontDialog);
        ui.close();
    }
    if ui.button(tr("paragraph-dialog-menu")).clicked() {
        out.push(RibbonCommand::ParagraphDialog);
        ui.close();
    }
    if let Some(can_continue) = numbered {
        ui.separator();
        if ui.button(tr("restart-numbering")).clicked() {
            out.push(RibbonCommand::RestartNumbering);
            ui.close();
        }
        let continued = ui.add_enabled(
            can_continue,
            eframe::egui::Button::new(tr("continue-numbering")),
        );
        if continued.clicked() {
            out.push(RibbonCommand::ContinueNumbering);
            ui.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn items_give_commands() {
        let mut h = Harness::new_ui_state(
            |ui, state: &mut (bool, Vec<RibbonCommand>)| {
                context_menu(ui, state.0, Some(false), &mut state.1);
            },
            (false, Vec::new()),
        );
        h.run();
        // Без выделения «Вырезать» не работает.
        h.get_by_label("Вырезать").click();
        h.run();
        assert!(h.state().1.is_empty());
        h.state_mut().0 = true;
        h.run();
        h.get_by_label("Копировать").click();
        h.run();
        h.get_by_label("Сохранить только текст").click();
        h.run();
        h.get_by_label("Абзац…").click();
        h.run();
        // Продолжать нечего — пункт не работает.
        h.get_by_label("Продолжить нумерацию").click();
        h.run();
        h.get_by_label("Начать заново с 1").click();
        h.run();
        assert_eq!(
            h.state().1,
            [
                RibbonCommand::Copy,
                RibbonCommand::PasteSpecial(PasteMode::TextOnly),
                RibbonCommand::ParagraphDialog,
                RibbonCommand::RestartNumbering,
            ]
        );
    }
}
