//! Область примечаний справа от документа, как в Word: карточки
//! примечаний по порядку в тексте, под каждым — ответы. На карточке —
//! «Ответить», «Изменить», «Решить» («Открыть снова») и «Удалить»;
//! щелчок по карточке выделяет текст примечания. Новое примечание
//! пишется в черновике вверху и добавляется кнопкой «Опубликовать».
//!
//! Как и лента, сама ничего не делает — возвращает [`CommentAction`].

use eframe::egui::{self, Color32, RichText, Sense, Stroke, Ui, vec2};
use letter_editor::Selection;

use crate::i18n::tr;
use crate::icons::{self, Icon, icon_text_button};
use crate::keytips::{self, Level, TipAction};
use crate::ribbon::{RibbonCommand, RibbonInfo, group};
use crate::theme::Palette;

/// Подсказки клавиш вкладки «Рецензирование» (для проверки).
#[cfg(test)]
pub const KEYTIPS: [&str; 6] = [
    "keytip-review-new-comment",
    "keytip-review-delete-comment",
    "keytip-review-delete-all",
    "keytip-review-previous",
    "keytip-review-next",
    "keytip-review-show",
];

/// Вкладка «Рецензирование»: группа «Примечания».
pub fn review(ui: &mut Ui, info: &RibbonInfo, out: &mut Vec<RibbonCommand>) {
    let enabled = info.has_document && info.story.is_none();
    group(ui, tr("group-comments"), |ui| {
        ui.horizontal(|ui| comment_buttons(ui, info, enabled, out));
    });
}

/// Кнопки группы «Примечания».
fn comment_buttons(ui: &mut Ui, info: &RibbonInfo, enabled: bool, out: &mut Vec<RibbonCommand>) {
    ui.vertical(|ui| {
        let name = tr("comment-new");
        let response =
            icons::icon_button(ui, Icon::NewComment, name, vec2(44.0, 34.0), enabled, false)
                .on_hover_text(name);
        tip(
            ui,
            &response,
            "keytip-review-new-comment",
            RibbonCommand::NewComment,
            out,
        );
        ui.label(name);
    });
    ui.vertical(|ui| {
        let has = enabled && info.has_comments;
        let response = icon_text_button(
            ui,
            Icon::DeleteComment,
            tr("comment-delete"),
            has && info.in_comment,
        );
        tip(
            ui,
            &response,
            "keytip-review-delete-comment",
            RibbonCommand::DeleteComment,
            out,
        );
        let response = ui.add_enabled(has, egui::Button::new(tr("comment-delete-all")).small());
        tip(
            ui,
            &response,
            "keytip-review-delete-all",
            RibbonCommand::DeleteAllComments,
            out,
        );
    });
    ui.vertical(|ui| {
        let has = enabled && info.has_comments;
        let response = icon_text_button(ui, Icon::ChevronLeft, tr("comment-previous"), has);
        tip(
            ui,
            &response,
            "keytip-review-previous",
            RibbonCommand::NextComment(false),
            out,
        );
        let response = icon_text_button(ui, Icon::ChevronRight, tr("comment-next"), has);
        tip(
            ui,
            &response,
            "keytip-review-next",
            RibbonCommand::NextComment(true),
            out,
        );
    });
    ui.vertical(|ui| {
        let name = tr("comment-show");
        let response = icons::icon_button(
            ui,
            Icon::Comments,
            name,
            vec2(44.0, 34.0),
            info.has_document,
            info.comments,
        )
        .on_hover_text(name);
        tip(
            ui,
            &response,
            "keytip-review-show",
            RibbonCommand::ToggleComments,
            out,
        );
        ui.label(name);
    });
}

/// Подсказка клавиши у кнопки и её команда по щелчку.
fn tip(
    ui: &mut Ui,
    response: &egui::Response,
    keytip: &str,
    command: RibbonCommand,
    out: &mut Vec<RibbonCommand>,
) {
    keytips::tip(
        ui,
        response,
        Level::Commands,
        keytip,
        TipAction::Command(command.clone()),
    );
    if response.clicked() {
        out.push(command);
    }
}

/// Одно примечание на карточке.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CommentEntry {
    /// Номер примечания.
    pub id: String,
    /// Автор.
    pub author: String,
    /// Когда написано, для показа («23.10.2026 12:00»).
    pub date: String,
    /// Текст.
    pub text: String,
}

/// Ветка: примечание и ответы на него.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CommentThread {
    /// Само примечание.
    pub comment: CommentEntry,
    /// Ответы по порядку.
    pub replies: Vec<CommentEntry>,
    /// Отмечено «решено».
    pub done: bool,
}

/// Что показать в области.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CommentsInfo {
    /// Ветки по порядку в тексте.
    pub threads: Vec<CommentThread>,
    /// Ветка, в тексте которой курсор (номер примечания).
    pub current: Option<String>,
}

/// Что выбрано в области.
#[derive(Debug, Clone, PartialEq)]
pub enum CommentAction {
    /// Выделить текст примечания.
    Select(String),
    /// Добавить примечание с этим текстом к выделению `at`.
    Add {
        /// Текст, к которому примечание.
        at: Selection,
        /// Текст примечания.
        text: String,
    },
    /// Ответить на примечание.
    Reply {
        /// На какое.
        parent: String,
        /// Текст ответа.
        text: String,
    },
    /// Новый текст примечания.
    Edit {
        /// Номер примечания.
        id: String,
        /// Текст.
        text: String,
    },
    /// Отметить ветку «решено» или снять отметку.
    Done {
        /// Номер примечания.
        id: String,
        /// Решено.
        done: bool,
    },
    /// Удалить примечание (с ответами).
    Delete(String),
    /// Закрыть область.
    Close,
}

/// Что сейчас пишется в области.
#[derive(Debug, Clone, PartialEq)]
enum Draft {
    /// Новое примечание к выделению.
    New(Selection),
    /// Ответ на примечание.
    Reply(String),
    /// Правка примечания.
    Edit(String),
}

/// Состояние области.
#[derive(Debug, Clone, Default)]
pub struct CommentsPane {
    /// Область показана.
    pub open: bool,
    draft: Option<(Draft, String)>,
    /// Поставить фокус в поле черновика.
    focus: bool,
}

impl CommentsPane {
    /// Начинает новое примечание к выделению `at`: область открывается,
    /// фокус — в поле текста.
    pub fn start_new(&mut self, at: Selection) {
        self.open = true;
        self.draft = Some((Draft::New(at), String::new()));
        self.focus = true;
    }

    /// Рисует область.
    pub fn ui(&mut self, ui: &mut Ui, info: &CommentsInfo) -> Option<CommentAction> {
        let mut action = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("comments-title")).size(16.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let name = tr("comments-close");
                if icons::icon_button(ui, Icon::Close, name, vec2(22.0, 22.0), true, false)
                    .on_hover_text(name)
                    .clicked()
                {
                    action = Some(CommentAction::Close);
                }
            });
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                if let Some((Draft::New(at), _)) = &self.draft {
                    let at = *at;
                    card(ui, true, false, |ui| {
                        if let Some(text) = self.editor(ui, "comments-post") {
                            action = Some(CommentAction::Add { at, text });
                        }
                    });
                }
                if info.threads.is_empty() && self.draft.is_none() {
                    ui.label(RichText::new(tr("comments-none")).color(Color32::GRAY));
                }
                for thread in &info.threads {
                    let current = info.current.as_ref() == Some(&thread.comment.id);
                    card(ui, current, thread.done, |ui| {
                        if let Some(a) = self.thread(ui, thread) {
                            action = Some(a);
                        }
                    });
                }
            });
        action
    }

    /// Ветка: примечание, ответы и кнопки.
    fn thread(&mut self, ui: &mut Ui, thread: &CommentThread) -> Option<CommentAction> {
        let mut action = None;
        if thread.done {
            ui.label(RichText::new(tr("comments-resolved")).small().italics());
        }
        if let Some(a) = self.entry(ui, &thread.comment, &thread.comment.id, true) {
            action = Some(a);
        }
        for reply in &thread.replies {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    if let Some(a) = self.entry(ui, reply, &thread.comment.id, false) {
                        action = Some(a);
                    }
                });
            });
        }
        let id = &thread.comment.id;
        let replying = matches!(&self.draft, Some((Draft::Reply(r), _)) if r == id);
        if replying {
            if let Some(text) = self.editor(ui, "comments-post") {
                action = Some(CommentAction::Reply {
                    parent: id.clone(),
                    text,
                });
            }
        } else {
            ui.horizontal_wrapped(|ui| {
                if !thread.done && ui.small_button(tr("comments-reply")).clicked() {
                    self.draft = Some((Draft::Reply(id.clone()), String::new()));
                    self.focus = true;
                }
                let (name, done) = if thread.done {
                    (tr("comments-reopen"), false)
                } else {
                    (tr("comments-resolve"), true)
                };
                if ui.small_button(name).clicked() {
                    action = Some(CommentAction::Done {
                        id: id.clone(),
                        done,
                    });
                }
                if ui.small_button(tr("comments-delete-thread")).clicked() {
                    action = Some(CommentAction::Delete(id.clone()));
                }
            });
        }
        action
    }

    /// Одно примечание ветки `thread`: автор, дата, текст (щелчок по
    /// ним выделяет текст ветки); у ответа — свои «Изменить» и «Удалить»,
    /// у первого — только «Изменить» (удаляется вся ветка).
    fn entry(
        &mut self,
        ui: &mut Ui,
        entry: &CommentEntry,
        thread: &str,
        first: bool,
    ) -> Option<CommentAction> {
        let mut action = None;
        let select = Some(CommentAction::Select(thread.to_string()));
        ui.horizontal_wrapped(|ui| {
            let author = egui::Label::new(RichText::new(&entry.author).strong())
                .sense(Sense::click())
                .selectable(false);
            if ui.add(author).clicked() {
                action = select.clone();
            }
            ui.label(RichText::new(&entry.date).small().color(Color32::GRAY));
        });
        let editing = matches!(&self.draft, Some((Draft::Edit(e), _)) if *e == entry.id);
        if editing {
            if let Some(text) = self.editor(ui, "comments-save") {
                action = Some(CommentAction::Edit {
                    id: entry.id.clone(),
                    text,
                });
            }
            return action;
        }
        let text = RichText::new(&entry.text).color(ui.visuals().strong_text_color());
        let text = egui::Label::new(text)
            .sense(Sense::click())
            .selectable(false);
        if ui.add(text).clicked() {
            action = select;
        }
        ui.horizontal(|ui| {
            if ui.small_button(tr("comments-edit")).clicked() {
                self.draft = Some((Draft::Edit(entry.id.clone()), entry.text.clone()));
                self.focus = true;
            }
            if !first && ui.small_button(tr("comments-delete")).clicked() {
                action = Some(CommentAction::Delete(entry.id.clone()));
            }
        });
        action
    }

    /// Поле черновика с кнопками `post` и «Отмена». Возвращает текст,
    /// когда нажата `post` (черновик закрывается).
    fn editor(&mut self, ui: &mut Ui, post: &str) -> Option<String> {
        let (_, text) = self.draft.as_mut()?;
        let edit = egui::TextEdit::multiline(text)
            .hint_text(tr("comments-hint"))
            .desired_rows(2)
            .desired_width(f32::INFINITY);
        let response = ui.add(edit);
        if std::mem::take(&mut self.focus) {
            response.request_focus();
        }
        let empty = text.trim().is_empty();
        let mut posted = None;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!empty, egui::Button::new(tr(post)))
                .clicked()
            {
                posted = self.draft.take().map(|(_, text)| text);
            }
            if ui.button(tr("comments-cancel")).clicked() {
                self.draft = None;
            }
        });
        posted
    }
}

/// Карточка: рамка (у текущей — цветная), у решённой — бледный текст.
fn card(ui: &mut Ui, current: bool, done: bool, add: impl FnOnce(&mut Ui)) {
    let colors = Palette::of(ui.ctx());
    let stroke = if current {
        Stroke::new(1.5, ui.visuals().selection.bg_fill)
    } else {
        Stroke::new(1.0, colors.border)
    };
    egui::Frame::new()
        .fill(colors.card)
        .stroke(stroke)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if done {
                ui.style_mut().visuals.override_text_color = Some(Color32::GRAY);
            }
            add(ui);
        });
    ui.add_space(6.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use letter_editor::Position;

    struct State {
        pane: CommentsPane,
        info: CommentsInfo,
        out: Vec<CommentAction>,
    }

    fn harness() -> Harness<'static, State> {
        let entry = |id: &str, author: &str, text: &str| CommentEntry {
            id: id.into(),
            author: author.into(),
            date: "23.10.2026 10:00".into(),
            text: text.into(),
        };
        let info = CommentsInfo {
            threads: vec![CommentThread {
                comment: entry("0", "Anna", "Which fox?"),
                replies: vec![entry("1", "Boris", "The usual one.")],
                done: false,
            }],
            current: Some("0".into()),
        };
        Harness::builder()
            .with_size(vec2(300.0, 600.0))
            .build_ui_state(
                |ui, state: &mut State| {
                    if let Some(action) = state.pane.ui(ui, &state.info) {
                        state.out.push(action);
                    }
                },
                State {
                    pane: CommentsPane {
                        open: true,
                        ..CommentsPane::default()
                    },
                    info,
                    out: Vec::new(),
                },
            )
    }

    #[test]
    fn new_comment_is_posted() {
        let mut h = harness();
        let at = Selection::new(Position::new(0, 1), Position::new(0, 4));
        h.state_mut().pane.start_new(at);
        h.run();
        h.get_by_role(egui::accesskit::Role::MultilineTextInput)
            .type_text("Hello");
        h.run();
        h.get_by_label("Опубликовать").click();
        h.run();
        assert_eq!(
            h.state().out,
            vec![CommentAction::Add {
                at,
                text: "Hello".into()
            }]
        );
        // Черновик закрылся.
        assert!(h.query_by_label("Опубликовать").is_none());
    }

    #[test]
    fn thread_buttons() {
        let mut h = harness();
        h.run();
        h.get_by_label("Which fox?").click();
        h.run();
        assert_eq!(h.state().out, vec![CommentAction::Select("0".into())]);
        h.get_by_label("Решить").click();
        h.run();
        assert_eq!(
            h.state().out.last(),
            Some(&CommentAction::Done {
                id: "0".into(),
                done: true
            })
        );
        h.get_by_label("Ответить").click();
        h.run();
        h.get_by_role(egui::accesskit::Role::MultilineTextInput)
            .type_text("Ok");
        h.run();
        h.get_by_label("Опубликовать").click();
        h.run();
        assert_eq!(
            h.state().out.last(),
            Some(&CommentAction::Reply {
                parent: "0".into(),
                text: "Ok".into()
            })
        );
        // «Удалить» у ответа удаляет только его.
        h.get_by_label("Удалить").click();
        h.run();
        assert_eq!(
            h.state().out.last(),
            Some(&CommentAction::Delete("1".into()))
        );
        h.get_by_label("Закрыть область примечаний").click();
        h.run();
        assert_eq!(h.state().out.last(), Some(&CommentAction::Close));
    }
}
