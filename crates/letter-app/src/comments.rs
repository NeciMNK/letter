//! Область примечаний справа от листов, как в Word: карточка каждого
//! обсуждения (примечание и ответы на него) стоит на высоте своего места
//! в тексте и соединена с ним линией. Щелчок по карточке делает её
//! активной: у активной есть «Ответить», «Решить» и «Удалить», а текст
//! правится двойным щелчком. Новое примечание сразу открыто для набора.
//!
//! Сами примечания правит редактор (команды `InsertComment`,
//! `ReplyComment`, `SetComment`, `DeleteComment`); здесь — только вид
//! и то, что пользователь нажал.

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, RichText, Stroke, Ui, pos2, vec2};

use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::theme::Palette;

/// Ширина области примечаний, точки экрана.
pub const PANE_WIDTH: f32 = 280.0;

/// Отступ карточек от краёв области и друг от друга.
const GAP: f32 = 8.0;

/// Подсветка куска текста с примечанием.
pub const RANGE_COLOR: Color32 = Color32::from_rgba_premultiplied(56, 44, 0, 56);
/// Подсветка куска активного примечания.
pub const ACTIVE_RANGE_COLOR: Color32 = Color32::from_rgba_premultiplied(110, 80, 0, 110);
/// Цвет линий от текста к карточкам.
const LINE_COLOR: Color32 = Color32::from_rgb(196, 140, 40);

/// Одно примечание обсуждения для показа.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Номер.
    pub id: String,
    /// Автор.
    pub author: String,
    /// Дата, как в файле.
    pub date: Option<String>,
    /// Текст.
    pub text: String,
}

/// Обсуждение: примечание и ответы на него.
#[derive(Debug, Clone, PartialEq)]
pub struct Thread {
    /// Примечания по порядку: первое — исходное.
    pub entries: Vec<Entry>,
    /// Обсуждение закрыто.
    pub done: bool,
    /// Где конец куска текста на экране (линия идёт сюда).
    pub anchor: Pos2,
}

impl Thread {
    /// Номер исходного примечания.
    pub fn id(&self) -> &str {
        self.entries.first().map_or("", |e| e.id.as_str())
    }
}

/// Что правится.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Текст примечания с этим номером.
    Comment(String),
    /// Новый ответ на примечание с этим номером.
    Reply(String),
}

/// Набор текста в карточке.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// Что правится.
    pub target: Target,
    /// Текст.
    pub text: String,
    /// Поставить фокус в поле в следующем кадре.
    pub focus: bool,
}

/// Состояние области: активное обсуждение и набор текста.
#[derive(Debug, Clone, Default)]
pub struct CommentPane {
    /// Номер исходного примечания активного обсуждения.
    pub active: Option<String>,
    /// Набор текста.
    pub draft: Option<Draft>,
}

/// Что сделал пользователь.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneAction {
    /// Готов текст примечания или ответа.
    Commit(Target, String),
    /// Отметить обсуждение решённым (`true`) или открыть заново.
    Resolve(String, bool),
    /// Удалить примечание (с ответами).
    Delete(String),
    /// Обсуждение стало активным: показать его место в тексте.
    Select(String),
}

impl CommentPane {
    /// Начинает набор текста `target` с текстом `text`.
    pub fn start(&mut self, target: Target, text: String) {
        self.draft = Some(Draft {
            target,
            text,
            focus: true,
        });
    }

    /// Рисует карточки обсуждений `threads` в области `rect` и линии от
    /// них к тексту; `editable` — можно ли менять примечания.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        threads: &[Thread],
        editable: bool,
    ) -> Vec<PaneAction> {
        let palette = Palette::of(ui.ctx());
        ui.painter().rect_filled(rect, 0.0, palette.desk);
        ui.painter().line_segment(
            [rect.left_top(), rect.left_bottom()],
            Stroke::new(1.0, palette.border),
        );
        let mut actions = Vec::new();
        let mut next_top = rect.top() + GAP;
        let lines = ui.painter().clone().with_clip_rect(ui.clip_rect());
        for thread in threads {
            let top = thread.anchor.y.max(next_top);
            if top > rect.bottom() {
                break;
            }
            let card = Rect::from_min_size(
                pos2(rect.left() + GAP, top),
                vec2(rect.width() - 2.0 * GAP, f32::INFINITY),
            );
            let active = self.active.as_deref() == Some(thread.id());
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(card)
                    .id_salt(("comment", thread.id())),
            );
            child.set_clip_rect(rect.intersect(ui.clip_rect()));
            let response = self.card(&mut child, thread, active, editable, &mut actions);
            let drawn = response.rect;
            next_top = drawn.bottom() + GAP;
            if drawn.bottom() < rect.top() {
                continue;
            }
            // Линия от места в тексте к карточке.
            let stroke = Stroke::new(if active { 1.5 } else { 1.0 }, LINE_COLOR);
            let corner = pos2(rect.left(), drawn.top() + 12.0);
            if thread.anchor.x < rect.left() {
                let points = [
                    thread.anchor,
                    pos2(rect.left() - 4.0, thread.anchor.y),
                    corner,
                ];
                if active {
                    lines.line(points.to_vec(), stroke);
                } else {
                    lines.add(egui::Shape::dashed_line(&points, stroke, 3.0, 3.0));
                }
                lines.line_segment([corner, pos2(drawn.left(), corner.y)], stroke);
            }
            if response.clicked() && !active {
                self.active = Some(thread.id().to_string());
                actions.push(PaneAction::Select(thread.id().to_string()));
            }
        }
        actions
    }

    /// Карточка обсуждения; ответ — её прямоугольник и щелчок.
    fn card(
        &mut self,
        ui: &mut Ui,
        thread: &Thread,
        active: bool,
        editable: bool,
        actions: &mut Vec<PaneAction>,
    ) -> egui::Response {
        let palette = Palette::of(ui.ctx());
        let stroke = if active {
            Stroke::new(1.5, LINE_COLOR)
        } else {
            Stroke::new(1.0, palette.border)
        };
        let frame = egui::Frame::new()
            .fill(palette.card)
            .stroke(stroke)
            .corner_radius(CornerRadius::same(4))
            .inner_margin(egui::Margin::same(8));
        let inner = frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (i, entry) in thread.entries.iter().enumerate() {
                if i > 0 {
                    ui.separator();
                }
                self.entry(ui, entry, thread.done, active && editable, actions);
            }
            if thread.done {
                ui.label(
                    RichText::new(tr("comment-resolved"))
                        .italics()
                        .color(palette.caption),
                );
            }
            let replying = matches!(
                &self.draft,
                Some(Draft { target: Target::Reply(p), .. }) if p == thread.id()
            );
            if replying {
                self.editor(ui, actions);
            }
            if active && editable && self.draft.is_none() {
                ui.horizontal(|ui| {
                    let size = vec2(26.0, 22.0);
                    let id = thread.id().to_string();
                    if icons::icon_button(
                        ui,
                        Icon::Reply,
                        tr("comment-reply"),
                        size,
                        !thread.done,
                        false,
                    )
                    .on_hover_text(tr("comment-reply"))
                    .clicked()
                    {
                        self.start(Target::Reply(id.clone()), String::new());
                    }
                    let (icon, name) = if thread.done {
                        (Icon::Undo, "comment-reopen")
                    } else {
                        (Icon::Check, "comment-resolve")
                    };
                    if icons::icon_button(ui, icon, tr(name), size, true, false)
                        .on_hover_text(tr(name))
                        .clicked()
                    {
                        actions.push(PaneAction::Resolve(id.clone(), !thread.done));
                    }
                    if icons::icon_button(
                        ui,
                        Icon::DeleteComment,
                        tr("comment-delete"),
                        size,
                        true,
                        false,
                    )
                    .on_hover_text(tr("comment-delete"))
                    .clicked()
                    {
                        actions.push(PaneAction::Delete(id));
                    }
                });
            }
        });
        inner.response.interact(egui::Sense::click())
    }

    /// Одно примечание: автор, дата, текст (или поле набора).
    fn entry(
        &mut self,
        ui: &mut Ui,
        entry: &Entry,
        done: bool,
        editable: bool,
        actions: &mut Vec<PaneAction>,
    ) {
        let palette = Palette::of(ui.ctx());
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&entry.author).strong());
            if let Some(date) = &entry.date {
                ui.label(
                    RichText::new(crate::backstage::show_date(date))
                        .small()
                        .color(palette.caption),
                );
            }
        });
        let editing = matches!(
            &self.draft,
            Some(Draft { target: Target::Comment(id), .. }) if *id == entry.id
        );
        if editing {
            self.editor(ui, actions);
            return;
        }
        let color = if done { palette.caption } else { palette.text };
        let text = if entry.text.is_empty() {
            RichText::new(tr("comment-empty"))
                .italics()
                .color(palette.caption)
        } else {
            RichText::new(&entry.text).color(color)
        };
        let response = ui.add(egui::Label::new(text).wrap().sense(egui::Sense::click()));
        if editable && response.double_clicked() {
            self.start(Target::Comment(entry.id.clone()), entry.text.clone());
        }
    }

    /// Поле набора текста. Ctrl+Enter или щелчок вне поля — готово,
    /// Esc — отмена.
    fn editor(&mut self, ui: &mut Ui, actions: &mut Vec<PaneAction>) {
        let Some(draft) = &mut self.draft else {
            return;
        };
        let response = ui.add(
            egui::TextEdit::multiline(&mut draft.text)
                .desired_rows(2)
                .desired_width(f32::INFINITY)
                .hint_text(tr("comment-hint")),
        );
        if std::mem::take(&mut draft.focus) {
            response.request_focus();
        }
        let (escape, ctrl_enter) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                i.modifiers.command && i.key_pressed(egui::Key::Enter),
            )
        });
        if response.has_focus() && escape {
            self.draft = None;
            return;
        }
        if (response.lost_focus() || (response.has_focus() && ctrl_enter))
            && let Some(draft) = self.draft.take()
        {
            let text = draft.text.trim_end().to_string();
            actions.push(PaneAction::Commit(draft.target, text));
        }
    }
}

/// Имя автора новых примечаний: имя пользователя системы.
pub fn author() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .ok()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| tr("comment-author").to_string())
}

/// Инициалы по имени: первые буквы слов, не больше трёх.
pub fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .flat_map(char::to_uppercase)
        .take(3)
        .collect()
}

/// Текущее время для даты примечания, UTC: `2026-10-25T12:00:00Z`.
pub fn now_iso() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let t = letter_editor::DateTime::from_unix(seconds, 0);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_and_dates() {
        assert_eq!(initials("Никита Ошмарин"), "НО");
        assert_eq!(initials("nikita"), "N");
        assert_eq!(initials(""), "");
        let now = now_iso();
        assert_eq!(now.len(), 20);
        assert!(now.ends_with('Z') && now.as_bytes()[10] == b'T');
        assert!(!author().is_empty());
    }
}
