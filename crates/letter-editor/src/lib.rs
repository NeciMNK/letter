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
//! - [`format`] — оформление выделенного;
//! - [`lists`] — списки: маркеры, нумерация, уровни;
//! - [`search`] — поиск и замена;
//! - [`table`] — таблицы: вставка, ячейки, строки и столбцы.

pub mod caret;
pub mod clipboard;
pub mod cover;
pub mod edit;
pub mod fields;
pub mod format;
pub mod history;
pub mod links;
pub mod lists;
pub mod navigation;
pub mod notes;
pub mod picture;
pub mod position;
pub mod search;
pub mod section;
pub mod selection;
pub mod table;
pub mod toc;
pub mod watermark;

pub use caret::{Caret, CaretStop, caret, hit_test, line_stops, position_in_line};
pub use clipboard::{copy_range, insert_fragment, insert_plain_text};
pub use cover::{CoverLayout, CoverPage, insert_cover_page};
pub use edit::{
    delete_backward, delete_forward, delete_range, insert_line_break, insert_text, paragraph_count,
    paragraph_mut, split_paragraph,
};
pub use fields::{DateTime, FieldContext, format_date, update_fields};
pub use format::{
    BorderEdge, ParagraphChanges, ParagraphFormat, RunChanges, RunFormat, align_paragraphs,
    apply_style, format_paragraphs, format_runs, formatting_at, language_at,
};
pub use history::{Command, History};
pub use links::{
    add_bookmark, bookmark_position, bookmark_range, bookmarks, delete_bookmark, link_at, set_link,
};
pub use lists::{
    ListKind, can_continue_numbering, change_list_level, continue_numbering, list_item, list_kind,
    multilevel_list, remove_from_list, restart_numbering, tab_changes_level, toggle_list,
};
pub use navigation::{Motion, Moved, move_cursor};
pub use notes::{insert_note, note_styles, notes, notes_mut};
pub use picture::{
    MIN_PICTURE_SIZE, PictureBox, PictureWrap, anchor_with_wrap, insert_picture, is_picture,
    moved_anchor, picture_at_point, picture_ref, pictures_on_page, resize_picture,
    selected_picture, set_picture_anchor,
};
pub use position::{Affinity, Position};
pub use search::{Found, SearchOptions, find_all, find_next, replace_all};
pub use section::{
    ColumnPreset, PageBorderPreset, PageSetup, header_footer_source, insert_break,
    insert_section_break, page_number_runs, section_props, set_header_footer, set_page_setup,
    set_title_page,
};
pub use selection::{Selection, SelectionRect, paragraph_at, select_all, selection_rects, word_at};
pub use table::{
    BorderPreset, can_merge, delete_columns, delete_rows, delete_table, in_table, insert_columns,
    insert_rows, insert_table, merge_cells, next_cell, set_cell_borders, set_cell_shading,
};
pub use toc::{TOC_CODE, insert_toc, outline_level, update_tocs};
pub use watermark::set_watermark;
