//! Редактирование: курсор, выделение, команды, отмена и повтор.
//!
//! - [`position`] — позиция курсора в документе;
//! - [`caret`](mod@caret) — где на странице нарисовать курсор и какая позиция под
//!   указателем мыши;
//! - [`navigation`] — перемещение курсора с клавиатуры;
//! - [`edit`] — ввод и удаление текста;
//! - [`selection`] — выделение;
//! - [`history`] — команды правки, отмена и повтор;
//! - [`clipboard`] — копирование и вставка;
//! - [`format`] — оформление выделенного.

pub mod caret;
pub mod clipboard;
pub mod edit;
pub mod format;
pub mod history;
pub mod navigation;
pub mod position;
pub mod selection;

pub use caret::{Caret, CaretStop, caret, hit_test, line_stops, position_in_line};
pub use clipboard::{copy_range, insert_fragment, insert_plain_text};
pub use edit::{
    delete_backward, delete_forward, delete_range, insert_line_break, insert_text, paragraph_mut,
    split_paragraph,
};
pub use format::{RunFormat, align_paragraphs, format_runs, formatting_at};
pub use history::{Command, History};
pub use navigation::{Motion, Moved, move_cursor};
pub use position::{Affinity, Position};
pub use selection::{Selection, SelectionRect, paragraph_at, select_all, selection_rects, word_at};
