//! Меню по правой кнопке мыши над текстом: вырезать, копировать,
//! параметры вставки, шрифт, абзац — как в Word.

use crate::i18n::tr;
use eframe::egui::{Ui, vec2};
use letter_editor::{Position, control_at};
use letter_model::{ControlKind, Document};

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
/// (`true` — выше есть список, который можно продолжить). `link` —
/// курсор в ссылке. Выбранное добавляется в `out`.
pub fn context_menu(
    ui: &mut Ui,
    has_selection: bool,
    numbered: Option<bool>,
    link: bool,
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
    ui.separator();
    if link {
        for (name, command) in [
            ("edit-link", RibbonCommand::LinkDialog),
            ("open-link", RibbonCommand::OpenLink),
            ("remove-link", RibbonCommand::RemoveLink),
        ] {
            if ui.button(tr(name)).clicked() {
                out.push(command);
                ui.close();
            }
        }
    } else if icon_text_button(ui, Icon::Link, tr("link"), true).clicked() {
        out.push(RibbonCommand::LinkDialog);
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

/// Пункты орфографии в начале меню над словом с ошибкой: замены,
/// «Пропустить все», «Добавить в словарь».
pub fn spelling_items(
    ui: &mut Ui,
    word: &str,
    suggestions: &[String],
    start: letter_editor::Position,
    end: letter_editor::Position,
    out: &mut Vec<RibbonCommand>,
) {
    if suggestions.is_empty() {
        ui.label(tr("spelling-no-suggestions"));
    }
    for text in suggestions {
        if ui
            .button(eframe::egui::RichText::new(text).strong())
            .clicked()
        {
            out.push(RibbonCommand::SpellReplace {
                start,
                end,
                text: text.clone(),
            });
            ui.close();
        }
    }
    if ui.button(tr("spelling-ignore")).clicked() {
        out.push(RibbonCommand::SpellIgnore(word.to_string()));
        ui.close();
    }
    if ui.button(tr("spelling-add")).clicked() {
        out.push(RibbonCommand::SpellAdd(word.to_string()));
        ui.close();
    }
    ui.separator();
}

/// Пункты меню элемента управления содержимым у курсора: пункты списка
/// и «Сегодня» у выбора даты.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlMenu {
    /// Пункты списка.
    pub items: Vec<String>,
    /// Элемент выбора даты.
    pub date: bool,
}

/// Меню элемента у места `at`; `None`, если там нет списка или даты.
pub fn control_menu_for(doc: &Document, at: Position) -> Option<ControlMenu> {
    let (control, _, _) = control_at(doc, at)?;
    if !control.content_editable() {
        return None;
    }
    match control.kind() {
        ControlKind::List { items, .. } => Some(ControlMenu {
            items: items.into_iter().map(|i| i.text).collect(),
            date: false,
        }),
        ControlKind::Date { .. } => Some(ControlMenu {
            items: Vec::new(),
            date: true,
        }),
        _ => None,
    }
}

/// Флажок ли у места `at`.
pub fn is_checkbox(doc: &Document, at: Position) -> bool {
    control_at(doc, at).is_some_and(|(c, _, _)| {
        c.content_editable() && matches!(c.kind(), ControlKind::CheckBox { .. })
    })
}

/// Пункты элемента в начале меню; выбранное добавляется в `out`.
pub fn control_menu(ui: &mut Ui, menu: &ControlMenu, out: &mut Vec<RibbonCommand>) {
    ui.set_min_width(200.0);
    for (index, item) in menu.items.iter().enumerate() {
        if ui.button(item).clicked() {
            out.push(RibbonCommand::ChooseListItem(index));
            ui.close();
        }
    }
    if menu.date && ui.button(tr("control-today")).clicked() {
        out.push(RibbonCommand::ControlToday);
        ui.close();
    }
    ui.separator();
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn spelling_items_give_commands() {
        use letter_editor::Position;
        let (a, b) = (Position::new(0, 0), Position::new(0, 4));
        let mut h = Harness::new_ui_state(
            move |ui, out: &mut Vec<RibbonCommand>| {
                spelling_items(ui, "helo", &["hello".to_string()], a, b, out);
            },
            Vec::new(),
        );
        h.run();
        h.get_by_label("hello").click();
        h.run();
        h.get_by_label("Пропустить все").click();
        h.run();
        h.get_by_label("Добавить в словарь").click();
        h.run();
        assert_eq!(
            h.state(),
            &[
                RibbonCommand::SpellReplace {
                    start: a,
                    end: b,
                    text: "hello".into()
                },
                RibbonCommand::SpellIgnore("helo".into()),
                RibbonCommand::SpellAdd("helo".into()),
            ]
        );
    }

    #[test]
    fn items_give_commands() {
        let mut h = Harness::new_ui_state(
            |ui, state: &mut (bool, Vec<RibbonCommand>)| {
                context_menu(ui, state.0, Some(false), false, &mut state.1);
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

    #[test]
    fn control_items_give_commands() {
        let menu = ControlMenu {
            items: vec!["Да".into(), "Нет".into()],
            date: true,
        };
        let mut h = Harness::new_ui_state(
            move |ui, out: &mut Vec<RibbonCommand>| control_menu(ui, &menu, out),
            Vec::new(),
        );
        h.run();
        h.get_by_label("Нет").click();
        h.run();
        h.get_by_label("Сегодня").click();
        h.run();
        assert_eq!(
            *h.state(),
            [
                RibbonCommand::ChooseListItem(1),
                RibbonCommand::ControlToday
            ]
        );
    }

    #[test]
    fn control_menu_only_for_lists_and_dates() {
        use letter_model::{ContentControl, Paragraph, RawXml, Run, RunProps};
        use std::sync::Arc;
        let doc = |props: &str| {
            let control = Arc::new(ContentControl {
                props: Some(RawXml::new(format!(
                    r#"<w:sdtPr xmlns:w="urn:w" xmlns:w14="urn:w14">{props}</w:sdtPr>"#
                ))),
                end_props: None,
            });
            let mut doc = Document::new();
            doc.sections[0].blocks = vec![
                Paragraph::from_runs([Run::text("x").with_props(RunProps {
                    control: Some(control),
                    ..RunProps::default()
                })])
                .into(),
            ];
            doc
        };
        let at = Position::new(0, 0);
        let list = doc(r#"<w:comboBox><w:listItem w:displayText="A" w:value="a"/></w:comboBox>"#);
        assert_eq!(
            control_menu_for(&list, at).map(|m| m.items),
            Some(vec!["A".to_string()])
        );
        assert!(control_menu_for(&doc("<w:date/>"), at).is_some_and(|m| m.date));
        assert!(control_menu_for(&doc("<w:text/>"), at).is_none());
        assert!(is_checkbox(&doc("<w14:checkbox/>"), at));
        assert!(!is_checkbox(&doc("<w:text/>"), at));
    }
}
