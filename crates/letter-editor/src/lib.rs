//! Редактирование: курсор, выделение, команды, отмена и повтор.
//!
//! - [`position`] — позиция курсора в документе;
//! - [`caret`](mod@caret) — где на странице нарисовать курсор и какая позиция под
//!   указателем мыши.

pub mod caret;
pub mod position;

pub use caret::{Caret, CaretStop, caret, hit_test, line_stops};
pub use position::{Affinity, Position};
