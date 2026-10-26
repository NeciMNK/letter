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
//! - [`table`] — таблицы: вставка, ячейки, строки и столбцы;
//! - [`protection`] — пароли защиты документа;
//! - [`revisions`] — исправления: запись, принять и отклонить;
//! - [`compare`] — сравнение двух документов;
//! - [`spelling`] — проверка орфографии.
//! - [`controls`] — элементы управления содержимым: флажки, списки, даты.

pub mod caret;
pub mod clipboard;
pub mod comments;
pub mod compare;
pub mod controls;
pub mod cover;
pub mod edit;
pub mod equation;
pub mod fields;
pub mod format;
pub mod history;
pub mod levels;
pub mod links;
pub mod lists;
pub mod merge;
pub mod navigation;
pub mod notes;
pub mod outline;
pub mod picture;
pub mod position;
pub mod protection;
pub mod revisions;
pub mod search;
pub mod section;
pub mod selection;
pub mod shape;
pub mod spelling;
pub mod table;
pub mod toc;
pub mod watermark;

pub use caret::{Caret, CaretStop, caret, hit_test, line_stops, position_in_line};
pub use clipboard::{copy_range, insert_fragment, insert_plain_text};
pub use comments::{
    CommentPlace, add_comment, comment_blocks, comment_places, delete_all_comments, delete_comment,
    reply_to_comment, set_comment_done, set_comment_text, thread_root,
};
pub use compare::compare_documents;
pub use controls::{
    ControlSetup, NewControl, PlaceholderTexts, choose_list_item, control_at, control_range,
    controls_in, editable, insert_control, placeholder_at, removes_locked_control, set_control,
    set_control_date, set_control_text, toggle_checkbox,
};
pub use cover::{CoverLayout, CoverPage, insert_cover_page};
pub use edit::{
    delete_backward, delete_forward, delete_range, insert_line_break, insert_text, paragraph_count,
    paragraph_mut, paragraph_ref, split_paragraph,
};
pub use equation::{insert_math, math_ref, new_math, set_math};
pub use fields::{DateTime, FieldContext, format_date, update_fields};
pub use format::{
    BorderEdge, ParagraphChanges, ParagraphFormat, RunChanges, RunFormat, align_paragraphs,
    apply_style, format_paragraphs, format_runs, formatting_at, language_at,
};
pub use history::{Command, History};
pub use levels::{OutlineChange, set_outline_level};
pub use links::{
    add_bookmark, bookmark_position, bookmark_range, bookmarks, delete_bookmark, link_at, set_link,
};
pub use lists::{
    ListKind, can_continue_numbering, change_list_level, continue_numbering, list_item, list_kind,
    multilevel_list, remove_from_list, restart_numbering, tab_changes_level, toggle_list,
};
pub use merge::{MergeData, has_merge_fields, insert_merge_field, merge_all, show_record};
pub use navigation::{Motion, Moved, move_cursor};
pub use notes::{insert_note, note_places, note_styles, notes, notes_mut};
pub use outline::{OutlineEntry, bookmark_places, document_outline};
pub use picture::{
    MIN_PICTURE_SIZE, PictureBox, PictureWrap, anchor_with_wrap, insert_picture, is_picture,
    moved_anchor, picture_at_point, picture_ref, pictures_on_page, resize_picture,
    selected_picture, set_picture_anchor,
};
pub use position::{Affinity, Position};
pub use search::{Found, SearchOptions, find_all, find_next, found_context, replace_all};
pub use section::{
    ColumnPreset, PageBorderPreset, PageSetup, header_footer_source, insert_break,
    insert_section_break, page_number_runs, section_props, set_header_footer, set_page_setup,
    set_title_page,
};
pub use selection::{Selection, SelectionRect, paragraph_at, select_all, selection_rects, word_at};
pub use shape::{
    SHAPE_GROUPS, TEXT_BOX, is_line, new_shape, set_picture_shape, shape_ref, with_fill,
    with_geometry, with_line_color, with_line_width,
};
pub use table::{
    BorderPreset, can_merge, delete_columns, delete_rows, delete_table, in_table, insert_columns,
    insert_rows, insert_table, merge_cells, next_cell, set_cell_borders, set_cell_shading,
};
pub use toc::{TOC_CODE, delete_toc, insert_toc, outline_level, update_tocs};
pub use watermark::set_watermark;
