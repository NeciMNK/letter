//! Доступность (F10.5): документ для экранных дикторов (NVDA, Экранный
//! диктор Windows, VoiceOver, Orca) через AccessKit и переход между
//! частями окна с клавиатуры.
//!
//! Лист документа egui рисует сам, поэтому диктор о тексте не знает.
//! Мы описываем его как многострочное поле ввода: строки абзацев возле
//! курсора — узлы `TextRun` со своим текстом и местом на экране, а курсор
//! и выделение — `TextSelection` в этих строках. Так диктор читает
//! строку под курсором, слово и букву при движении стрелками.
//!
//! F6 и Shift+F6, как в Word, переводят фокус между документом,
//! областями слева и справа, строкой состояния и лентой.

use eframe::egui::accesskit::{self, Role};
use eframe::egui::{
    Context, Id, Key, LayerId, Modifiers, Order, Rect, Response, Stroke, StrokeKind, Ui, UiBuilder,
    Vec2,
};
use letter_editor::{Affinity, Position};
use letter_layout::Page;

/// Сколько абзацев описывать диктору сразу: весь документ на сотни
/// страниц в каждом кадре — слишком долго.
const AROUND: usize = 20;

/// Номер элемента «лист документа» в egui.
pub fn document_id() -> Id {
    Id::NULL.with("letter-document")
}

/// Фокус у поля ввода или кнопки, а не у документа: тогда клавиши
/// достаются им.
pub fn typing_elsewhere(ctx: &Context) -> bool {
    ctx.memory(|m| m.focused().is_some_and(|id| id != document_id()))
}

/// Часть окна, между которыми ходит F6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    /// Документ.
    Document,
    /// Область навигации слева.
    Navigation,
    /// Примечания справа.
    Comments,
    /// Строка состояния.
    Status,
    /// Лента.
    Ribbon,
}

/// Порядок частей для F6, как в Word: документ, области, строка
/// состояния, лента.
const ORDER: [Region; 5] = [
    Region::Document,
    Region::Navigation,
    Region::Comments,
    Region::Status,
    Region::Ribbon,
];

/// Где на экране части окна в этом кадре.
#[derive(Debug, Clone, Default)]
pub struct Regions {
    rects: Vec<(Region, Rect)>,
}

impl Regions {
    /// Запоминает место части окна.
    pub fn set(&mut self, region: Region, rect: Rect) {
        self.rects.retain(|(r, _)| *r != region);
        self.rects.push((region, rect));
    }

    /// Забывает все места (в начале кадра).
    pub fn clear(&mut self) {
        self.rects.clear();
    }

    fn rect(&self, region: Region) -> Option<Rect> {
        self.rects
            .iter()
            .find(|(r, _)| *r == region)
            .map(|(_, rect)| *rect)
    }

    /// В какой части окна сейчас фокус.
    pub fn current(&self, ctx: &Context) -> Region {
        let Some(id) = ctx
            .memory(|m| m.focused())
            .filter(|id| *id != document_id())
        else {
            return Region::Document;
        };
        let Some(rect) = ctx.viewport(|v| v.prev_pass.widgets.get(id).map(|w| w.rect)) else {
            return Region::Document;
        };
        ORDER
            .into_iter()
            .filter(|r| *r != Region::Document)
            .find(|r| {
                self.rect(*r)
                    .is_some_and(|area| area.contains(rect.center()))
            })
            .unwrap_or(Region::Document)
    }

    /// F6 — фокус в следующую часть окна, Shift+F6 — в предыдущую.
    /// Возвращает часть, куда ушёл фокус.
    pub fn handle_keys(&self, ctx: &Context) -> Option<Region> {
        let back = ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::F6));
        let forward = !back && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F6));
        if !back && !forward {
            return None;
        }
        let start = ORDER
            .iter()
            .position(|r| *r == self.current(ctx))
            .unwrap_or(0);
        for step in 1..ORDER.len() {
            let index = if back {
                (start + ORDER.len() - step) % ORDER.len()
            } else {
                (start + step) % ORDER.len()
            };
            let region = ORDER[index];
            let target = match region {
                Region::Document => Some(document_id()),
                _ => self
                    .rect(region)
                    .and_then(|rect| first_focusable(ctx, rect)),
            };
            if let Some(id) = target {
                ctx.memory_mut(|m| m.request_focus(id));
                return Some(region);
            }
        }
        None
    }
}

/// Рамка вокруг элемента с фокусом клавиатуры: у кнопок без рамки
/// (значки, строка состояния) egui фокус не показывает, а без него не
/// видно, где окажется Enter. У поля ввода своя рамка, у документа —
/// курсор.
pub fn paint_focus(ctx: &Context) {
    let Some(id) = ctx
        .memory(|m| m.focused())
        .filter(|id| *id != document_id())
    else {
        return;
    };
    if ctx.text_edit_focused() {
        return;
    }
    let rect = ctx.viewport(|v| {
        (v.this_pass.widgets.get(id))
            .or_else(|| v.prev_pass.widgets.get(id))
            .map(|w| w.rect)
    });
    let Some(rect) = rect else {
        return;
    };
    let color = ctx.global_style().visuals.selection.stroke.color;
    ctx.layer_painter(LayerId::new(
        Order::Foreground,
        Id::NULL.with("letter-focus"),
    ))
    .rect_stroke(
        rect.expand(1.0),
        2.0,
        Stroke::new(2.0, color),
        StrokeKind::Outside,
    );
}

/// Первый (сверху слева) элемент в `rect`, который может получить фокус.
fn first_focusable(ctx: &Context, rect: Rect) -> Option<Id> {
    ctx.viewport(|v| {
        v.prev_pass
            .widgets
            .layers()
            .flat_map(|(_, widgets)| widgets.iter())
            .filter(|w| {
                w.enabled
                    && w.sense.is_focusable()
                    && w.id != document_id()
                    && rect.contains(w.rect.center())
            })
            .min_by(|a, b| {
                (a.rect.top(), a.rect.left())
                    .partial_cmp(&(b.rect.top(), b.rect.left()))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|w| w.id)
    })
}

/// Строка документа для диктора.
struct AccessLine<'a> {
    /// Абзац.
    paragraph: usize,
    /// Начало строки в абзаце, байты.
    start: usize,
    /// Текст строки (с пробелами в конце).
    text: &'a str,
    /// Последняя строка абзаца: после неё — конец абзаца.
    last: bool,
    /// Место на экране.
    rect: Rect,
}

/// Строки абзацев `paragraphs` по порядку. `page_rect` — где на экране
/// лист, `points` — пунктов документа в точке экрана.
fn access_lines<'a>(
    pages: &'a [Page],
    paragraphs: std::ops::RangeInclusive<usize>,
    page_rect: &dyn Fn(usize) -> Rect,
    points: f32,
) -> Vec<AccessLine<'a>> {
    let mut lines: Vec<AccessLine<'a>> = Vec::new();
    for (index, page) in pages.iter().enumerate() {
        for line in page
            .lines
            .iter()
            .filter(|l| paragraphs.contains(&l.paragraph))
        {
            let range = line.line.range.clone();
            let Some(text) = line.text.get(range.clone()) else {
                continue;
            };
            let origin = page_rect(index).min;
            let rect = Rect::from_min_size(
                origin + Vec2::new(line.x + line.line.x, line.top) / points,
                Vec2::new(line.line.width.max(1.0), line.height) / points,
            );
            lines.push(AccessLine {
                paragraph: line.paragraph,
                start: range.start,
                text,
                last: range.end >= line.text.len(),
                rect,
            });
        }
    }
    // Строка переносится на следующую страницу — у последней строки
    // абзаца на прошлой странице `last` уже верно: текст кончается там,
    // где кончается абзац.
    lines
}

/// Где позиция среди строк: номер строки и номер буквы в ней.
fn locate(lines: &[AccessLine], position: Position) -> Option<(usize, usize)> {
    let mut found = None;
    for (index, line) in lines.iter().enumerate() {
        let end = line.start + line.text.len();
        if line.paragraph == position.paragraph
            && line.start <= position.offset
            && position.offset <= end
        {
            let chars = line
                .text
                .get(..position.offset - line.start)?
                .chars()
                .count();
            found = Some((index, chars));
            // В конце строки на месте переноса — начало следующей, если
            // курсор не привязан к концу строки.
            if position.offset < end || line.last || position.affinity == Affinity::Upstream {
                break;
            }
        }
    }
    found
}

/// Описывает лист документа для дикторов: `response` — сам лист, `pages`
/// — показанные страницы, `anchor` и `cursor` — выделение, `name` — имя
/// документа.
#[allow(clippy::too_many_arguments)]
pub fn describe_document(
    ui: &mut Ui,
    response: &Response,
    pages: &[Page],
    anchor: Position,
    cursor: Position,
    name: &str,
    page_rect: &dyn Fn(usize) -> Rect,
    points: f32,
) {
    let ctx = ui.ctx().clone();
    response
        .widget_info(|| eframe::egui::WidgetInfo::labeled(Role::MultilineTextInput, true, name));
    // Без диктора дерево не строится — и считать незачем.
    if ctx.accesskit_node_builder(response.id, |_| ()).is_none() {
        return;
    }
    let first = anchor
        .paragraph
        .min(cursor.paragraph)
        .max(cursor.paragraph.saturating_sub(AROUND));
    let last = anchor
        .paragraph
        .max(cursor.paragraph)
        .min(cursor.paragraph + AROUND);
    let lines = access_lines(pages, first..=last, page_rect, points);
    let ids: Vec<Id> = (0..lines.len())
        .map(|k| {
            let child = ui.new_child(
                UiBuilder::new()
                    .id_salt(("access-line", k))
                    .accessibility_parent(response.id),
            );
            child.unique_id()
        })
        .collect();
    for (line, id) in lines.iter().zip(&ids) {
        ctx.accesskit_node_builder(*id, |node| {
            node.set_role(Role::TextRun);
            node.set_text_direction(accesskit::TextDirection::LeftToRight);
            node.set_bounds(accesskit::Rect {
                x0: line.rect.min.x.into(),
                y0: line.rect.min.y.into(),
                x1: line.rect.max.x.into(),
                y1: line.rect.max.y.into(),
            });
            let mut value = line.text.to_owned();
            if line.last {
                value.push('\n');
            }
            let lengths: Vec<u8> = value.chars().map(|c| c.len_utf8() as u8).collect();
            // Начала слов — номера букв, а их в AccessKit не больше 255.
            let mut starts = Vec::new();
            let mut after_word = false;
            for (k, c) in value.chars().enumerate().take(256) {
                let word = c.is_alphanumeric();
                if word && after_word {
                    starts.push(k as u8);
                }
                after_word = !word;
            }
            node.set_word_starts(starts);
            node.set_character_lengths(lengths);
            node.set_value(value);
        });
    }
    let position = |at: Position| {
        let (line, character_index) = locate(&lines, at)?;
        Some(accesskit::TextPosition {
            node: ids[line].accesskit_id(),
            character_index,
        })
    };
    if let Some(focus) = position(cursor) {
        let anchor = position(anchor).unwrap_or(focus);
        ctx.accesskit_node_builder(response.id, |node| {
            node.set_text_selection(accesskit::TextSelection { anchor, focus });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_layout::{FontLibrary, Shaper, layout_document};
    use letter_model::{Block, Document, Paragraph};

    fn pages(texts: &[&str]) -> Vec<Page> {
        let mut doc = Document::new();
        doc.sections[0].blocks = texts
            .iter()
            .map(|t| Block::Paragraph(Paragraph::text(*t)))
            .collect();
        layout_document(&doc, &mut Shaper::new(), &FontLibrary::embedded_only()).pages
    }

    #[test]
    fn lines_and_positions_for_the_screen_reader() {
        let long = "слово ".repeat(40);
        let pages = pages(&["Первый абзац", long.trim_end(), "Третий"]);
        let rect = |_| Rect::from_min_size(eframe::egui::Pos2::ZERO, Vec2::splat(1000.0));
        let lines = access_lines(&pages, 0..=1, &rect, 1.0);
        assert_eq!(lines[0].text, "Первый абзац");
        assert!(lines[0].last);
        // Длинный абзац — несколько строк, конец абзаца только у последней.
        assert!(lines.len() > 2);
        assert!(
            lines[1..lines.len() - 1]
                .iter()
                .all(|l| !l.last && l.paragraph == 1)
        );
        assert!(lines.last().is_some_and(|l| l.last));
        // Третий абзац не попал.
        assert!(lines.iter().all(|l| l.paragraph < 2));
        // Позиция — номер строки и буквы (не байта).
        assert_eq!(
            locate(&lines, Position::new(0, "Первый".len())),
            Some((0, 6))
        );
        // Место переноса — начало следующей строки.
        let second = &lines[2];
        assert_eq!(locate(&lines, Position::new(1, second.start)), Some((2, 0)));
        assert_eq!(locate(&lines, Position::new(2, 0)), None);
    }
}
