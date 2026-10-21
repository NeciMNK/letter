//! Окно документа: меню «Файл», листы с прокруткой, масштаб
//! (Ctrl+колесо, кнопки, ползунок, «по ширине страницы»), строка
//! состояния и мигающий курсор, который ставится щелчком мыши и ходит
//! по тексту клавишами (стрелки, Home/End, PageUp/PageDown, с Ctrl —
//! по словам, абзацам и к краям документа). Текст можно набирать
//! (в том числе через IME), Enter делит абзац, Shift+Enter переносит
//! строку, Backspace и Delete стирают, Tab вставляет табуляцию.
//! Выделение — Shift с клавишами перемещения, мышью (протянуть,
//! Shift+щелчок, двойной щелчок — слово, тройной — абзац) и Ctrl+A.

use std::path::{Path, PathBuf};

use std::time::Duration;

use eframe::egui::{
    self, Align, Align2, Color32, Context, CursorIcon, Key, KeyboardShortcut, Layout, Modifiers,
    Pos2, Rect, Sense, Vec2, ViewportCommand, emath::GuiRounding,
};
use letter_editor::{
    Motion, Position, Selection, caret, delete_backward, delete_forward, delete_range, hit_test,
    insert_line_break, insert_text, move_cursor, paragraph_at, select_all, selection_rects,
    split_paragraph, word_at,
};
use letter_layout::{DocumentLayout, FontLibrary, Shaper, layout_document};
use letter_model::Document;
use letter_render::{DESK_COLOR, PagePainter, SCREEN_PIXELS_PER_POINT};

use crate::view::{
    MAX_ZOOM, MIN_ZOOM, PAGE_GAP, Zoom, anchored_offset, clamp_zoom, current_page, page_width_zoom,
    place_pages, step_zoom,
};

/// Название программы в заголовке окна.
pub const APP_NAME: &str = "letter";

const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const ZOOM_IN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Equals);
const ZOOM_IN_PLUS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus);
const ZOOM_OUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus);
const ZOOM_RESET: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num0);

/// Заливка выделения: полупрозрачная серо-голубая, текст под ней виден.
const SELECTION_COLOR: Color32 = Color32::from_rgba_premultiplied(0, 50, 110, 70);

/// Сколько курсор виден и сколько скрыт при мигании, секунды (как
/// в Windows по умолчанию).
const CARET_BLINK: f64 = 0.53;

/// Виден ли курсор через `elapsed` секунд после того, как его поставили,
/// и через сколько секунд это изменится.
fn caret_phase(elapsed: f64) -> (bool, f64) {
    let elapsed = elapsed.max(0.0);
    let phase = elapsed % (2.0 * CARET_BLINK);
    if phase < CARET_BLINK {
        (true, CARET_BLINK - phase)
    } else {
        (false, 2.0 * CARET_BLINK - phase)
    }
}

/// Открытый документ.
struct OpenDocument {
    /// Откуда открыт.
    path: PathBuf,
    /// Сам документ; правки меняют его, а потом он верстается заново.
    doc: Document,
    /// Свёрстанные страницы.
    layout: DocumentLayout,
    /// Где стоит курсор.
    cursor: Position,
    /// Где начато выделение; выделено всё от него до курсора. Совпадает
    /// с курсором, если ничего не выделено.
    anchor: Position,
    /// Когда курсор поставили (время egui, секунды): от этого момента
    /// он мигает, начиная с «виден».
    cursor_since: f64,
    /// Колонка для ↑ и ↓ (см. [`letter_editor::Moved::goal_x`]).
    goal_x: Option<f32>,
}

impl OpenDocument {
    /// Выделение от якоря до курсора.
    fn selection(&self) -> Selection {
        Selection::new(self.anchor, self.cursor)
    }

    /// Верстает документ заново после правки.
    fn relayout(&mut self, shaper: &mut Shaper, library: &FontLibrary) {
        self.layout = layout_document(&self.doc, shaper, library);
    }
}

/// Что сделать по событию клавиатуры.
#[derive(Debug, Clone, PartialEq)]
enum Action {
    /// Сдвинуть курсор; `true` — с Shift, выделение растёт.
    Move(Motion, bool),
    SelectAll,
    Edit(Edit),
}

/// Правка с клавиатуры.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Edit {
    Text(String),
    Enter,
    LineBreak,
    Tab,
    Backspace,
    Delete,
}

/// Состояние окна просмотра.
pub struct Viewer {
    library: FontLibrary,
    shaper: Shaper,
    painter: PagePainter,
    document: Option<OpenDocument>,
    zoom: Zoom,
    /// Сообщение об ошибке, показывается в отдельном окошке.
    error: Option<String>,
    /// Прокрутка и размер видимой части в прошлом кадре.
    offset: Vec2,
    view_size: Vec2,
    /// Левый верхний угол видимой части на экране.
    view_origin: Pos2,
    /// Прокрутка, которую надо поставить в следующем кадре.
    pending_offset: Option<Vec2>,
    /// Лист в середине окна, с нуля.
    current_page: usize,
    /// Надо обновить заголовок окна.
    title_changed: bool,
    /// Пользователь попросил окно выбора файла.
    open_requested: bool,
    /// Курсор сдвинули с клавиатуры: прокрутить так, чтобы он был виден.
    reveal_cursor: bool,
}

impl Viewer {
    /// Пустой просмотрщик со шрифтами `library`.
    pub fn new(library: FontLibrary) -> Viewer {
        Viewer {
            library,
            shaper: Shaper::new(),
            painter: PagePainter::new(),
            document: None,
            zoom: Zoom::default(),
            error: None,
            offset: Vec2::ZERO,
            view_size: Vec2::ZERO,
            view_origin: Pos2::ZERO,
            pending_offset: None,
            current_page: 0,
            title_changed: false,
            open_requested: false,
            reveal_cursor: false,
        }
    }

    /// Открывает .docx и верстает его. При ошибке прежний документ
    /// остаётся на экране.
    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        let doc = letter_docx::read_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let layout = layout_document(&doc, &mut self.shaper, &self.library);
        self.document = Some(OpenDocument {
            path: path.to_owned(),
            doc,
            layout,
            cursor: Position::default(),
            anchor: Position::default(),
            cursor_since: 0.0,
            goal_x: None,
        });
        self.pending_offset = Some(Vec2::ZERO);
        self.current_page = 0;
        self.title_changed = true;
        Ok(())
    }

    /// Открывает файл, а ошибку показывает пользователю.
    pub fn open_or_report(&mut self, path: &Path) {
        if let Err(e) = self.open(path) {
            self.error = Some(e);
        }
    }

    /// Рисует окно целиком.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.handle_input(&ctx);

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::no_frame()
            .frame(egui::Frame::new().fill(DESK_COLOR))
            .show(ui, |ui| self.pages(ui));
        self.error_window(&ctx);

        if self.title_changed {
            self.title_changed = false;
            ctx.send_viewport_cmd(ViewportCommand::Title(self.title()));
        }
        if std::mem::take(&mut self.open_requested) {
            self.pick_file();
        }
    }

    /// Заголовок окна: «имя.docx — letter».
    pub fn title(&self) -> String {
        match self.document.as_ref().and_then(|d| d.path.file_name()) {
            Some(name) => format!("{} — {APP_NAME}", name.to_string_lossy()),
            None => APP_NAME.to_owned(),
        }
    }

    /// Масштаб, которым сейчас нарисованы листы (1.0 — 100 %).
    pub fn zoom(&self) -> f32 {
        self.effective_zoom(self.view_size.x)
    }

    /// Где стоит курсор. `None`, если документ не открыт.
    #[cfg(test)]
    pub fn cursor(&self) -> Option<Position> {
        self.document.as_ref().map(|d| d.cursor)
    }

    /// Выделение (пустое, если ничего не выделено), когда документ открыт.
    #[cfg(test)]
    pub fn selection(&self) -> Option<Selection> {
        self.document.as_ref().map(OpenDocument::selection)
    }

    /// Сколько страниц в открытом документе.
    pub fn page_count(&self) -> usize {
        self.document.as_ref().map_or(0, |d| d.layout.pages.len())
    }

    /// Масштаб при ширине видимой части `view_width`.
    fn effective_zoom(&self, view_width: f32) -> f32 {
        match self.zoom {
            Zoom::Fixed(z) => clamp_zoom(z),
            Zoom::PageWidth => {
                let widest = self.document.as_ref().map_or(0.0, |d| {
                    d.layout.pages.iter().map(|p| p.width).fold(0.0, f32::max)
                });
                page_width_zoom(widest, view_width)
            }
        }
    }

    /// Меняет масштаб так, чтобы точка `anchor` видимой части (по
    /// умолчанию — середина) осталась на месте.
    fn set_zoom(&mut self, zoom: Zoom, anchor: Option<Vec2>) {
        let old = self.effective_zoom(self.view_size.x);
        self.zoom = match zoom {
            Zoom::Fixed(z) => Zoom::Fixed(clamp_zoom(z)),
            Zoom::PageWidth => Zoom::PageWidth,
        };
        let new = self.effective_zoom(self.view_size.x);
        if old > 0.0 && new != old {
            let anchor = anchor.unwrap_or(self.view_size / 2.0);
            let base = self.pending_offset.unwrap_or(self.offset);
            self.pending_offset = Some(anchored_offset(base, anchor, new / old));
        }
    }

    fn zoom_step(&mut self, up: bool) {
        let zoom = step_zoom(self.zoom(), up);
        self.set_zoom(Zoom::Fixed(zoom), None);
    }

    /// Сочетания клавиш и перетаскивание файла в окно.
    fn handle_input(&mut self, ctx: &Context) {
        let (open, zoom_in, zoom_out, reset, dropped) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&OPEN),
                i.consume_shortcut(&ZOOM_IN) || i.consume_shortcut(&ZOOM_IN_PLUS),
                i.consume_shortcut(&ZOOM_OUT),
                i.consume_shortcut(&ZOOM_RESET),
                i.raw
                    .dropped_files
                    .iter()
                    .map(|f| f.path().to_path_buf())
                    .find(|p| !p.as_os_str().is_empty()),
            )
        });
        if open {
            self.open_requested = true;
        }
        if zoom_in {
            self.zoom_step(true);
        }
        if zoom_out {
            self.zoom_step(false);
        }
        if reset {
            self.set_zoom(Zoom::Fixed(1.0), None);
        }
        if let Some(path) = dropped {
            self.open_or_report(&path);
        }
        if self.error.is_none() {
            self.keyboard(ctx);
        }
    }

    /// Клавиатура: стрелки, Home/End, PageUp/PageDown двигают курсор,
    /// набранный текст и Enter, Backspace, Delete, Tab правят документ.
    /// События разбираются по порядку: «ab←c» даёт «acb».
    fn keyboard(&mut self, ctx: &Context) {
        let points = 1.0 / (self.zoom() * SCREEN_PIXELS_PER_POINT);
        let screen = self.view_size.y * points;
        let Some(document) = &mut self.document else {
            return;
        };
        let actions = ctx.input_mut(|i| {
            let mut actions = Vec::new();
            i.events.retain(|event| match key_action(event, screen) {
                Some(action) => {
                    actions.push(action);
                    false
                }
                None => true,
            });
            actions
        });
        if actions.is_empty() {
            return;
        }
        // Вёрстка устаревает после правки; обновляем её перед следующим
        // движением курсора и в конце.
        let mut stale = false;
        for action in actions {
            match action {
                Action::Move(motion, extend) => {
                    if std::mem::take(&mut stale) {
                        document.relayout(&mut self.shaper, &self.library);
                    }
                    let selection = document.selection();
                    // ← и → при выделении только снимают его, оставляя
                    // курсор у его края, как в Word.
                    let collapse = match motion {
                        Motion::Left if !extend && !selection.is_empty() => Some(selection.start()),
                        Motion::Right if !extend && !selection.is_empty() => Some(selection.end()),
                        _ => None,
                    };
                    if let Some(edge) = collapse {
                        document.cursor = edge;
                        document.goal_x = None;
                    } else {
                        let moved =
                            move_cursor(&document.layout, document.cursor, motion, document.goal_x);
                        document.cursor = moved.position;
                        document.goal_x = moved.goal_x;
                    }
                    if !extend {
                        document.anchor = document.cursor;
                    }
                }
                Action::SelectAll => {
                    if std::mem::take(&mut stale) {
                        document.relayout(&mut self.shaper, &self.library);
                    }
                    let all = select_all(&document.layout);
                    document.anchor = all.anchor;
                    document.cursor = all.focus;
                    document.goal_x = None;
                }
                Action::Edit(edit) => {
                    let selection = document.selection();
                    let doc = &mut document.doc;
                    // Выделенное заменяется набранным или просто стирается.
                    let at = if selection.is_empty() {
                        document.cursor
                    } else {
                        delete_range(doc, selection.start(), selection.end())
                    };
                    let erased = !selection.is_empty();
                    document.cursor = match edit {
                        Edit::Text(text) => insert_text(doc, at, &text),
                        Edit::Enter => split_paragraph(doc, at),
                        Edit::LineBreak => insert_line_break(doc, at),
                        Edit::Tab => insert_text(doc, at, "\t"),
                        Edit::Backspace | Edit::Delete if erased => at,
                        Edit::Backspace => delete_backward(doc, at),
                        Edit::Delete => delete_forward(doc, at),
                    };
                    document.anchor = document.cursor;
                    document.goal_x = None;
                    stale = true;
                }
            }
        }
        if stale {
            document.relayout(&mut self.shaper, &self.library);
        }
        document.cursor_since = ctx.input(|i| i.time);
        self.reveal_cursor = true;
    }

    /// Системное окно выбора файла.
    fn pick_file(&mut self) {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Открыть документ")
            .add_filter("Документ Word (*.docx)", &["docx"])
            .add_filter("Все файлы", &["*"]);
        if let Some(dir) = self
            .document
            .as_ref()
            .and_then(|d| d.path.parent())
            .filter(|d| !d.as_os_str().is_empty())
        {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.open_or_report(&path);
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Файл", |ui| {
                let open =
                    egui::Button::new("Открыть…").shortcut_text(ui.ctx().format_shortcut(&OPEN));
                if ui.add(open).clicked() {
                    self.open_requested = true;
                }
                ui.separator();
                if ui.button("Выход").clicked() {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                }
            });
            ui.menu_button("Вид", |ui| {
                let ctx = ui.ctx().clone();
                let zoom_in =
                    egui::Button::new("Увеличить").shortcut_text(ctx.format_shortcut(&ZOOM_IN));
                if ui.add(zoom_in).clicked() {
                    self.zoom_step(true);
                }
                let zoom_out =
                    egui::Button::new("Уменьшить").shortcut_text(ctx.format_shortcut(&ZOOM_OUT));
                if ui.add(zoom_out).clicked() {
                    self.zoom_step(false);
                }
                ui.separator();
                let actual =
                    egui::Button::new("100 %").shortcut_text(ctx.format_shortcut(&ZOOM_RESET));
                if ui.add(actual).clicked() {
                    self.set_zoom(Zoom::Fixed(1.0), None);
                }
                if ui
                    .add(egui::Button::selectable(
                        self.zoom == Zoom::PageWidth,
                        "По ширине страницы",
                    ))
                    .clicked()
                {
                    self.set_zoom(Zoom::PageWidth, None);
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if self.document.is_some() {
                ui.label(format!(
                    "Страница {} из {}",
                    self.current_page + 1,
                    self.page_count()
                ));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let zoom = self.zoom();
                if ui
                    .button(format!("{:.0} %", zoom * 100.0))
                    .on_hover_text("Масштаб 100 %")
                    .clicked()
                {
                    self.set_zoom(Zoom::Fixed(1.0), None);
                }
                if ui.small_button("+").on_hover_text("Увеличить").clicked() {
                    self.zoom_step(true);
                }
                let mut percent = zoom * 100.0;
                let slider = egui::Slider::new(&mut percent, MIN_ZOOM * 100.0..=MAX_ZOOM * 100.0)
                    .logarithmic(true)
                    .show_value(false);
                if ui.add(slider).changed() {
                    self.set_zoom(Zoom::Fixed(percent / 100.0), None);
                }
                if ui.small_button("−").on_hover_text("Уменьшить").clicked() {
                    self.zoom_step(false);
                }
                if ui
                    .selectable_label(self.zoom == Zoom::PageWidth, "По ширине")
                    .on_hover_text("По ширине страницы")
                    .clicked()
                {
                    self.set_zoom(Zoom::PageWidth, None);
                }
            });
        });
    }

    /// Листы документа в прокручиваемой области.
    fn pages(&mut self, ui: &mut egui::Ui) {
        if self.document.is_none() {
            ui.centered_and_justified(|ui| {
                ui.label("Откройте документ: Файл → Открыть (Ctrl+O) или перетащите файл в окно");
            });
            return;
        }

        let area_rect = ui.available_rect_before_wrap();
        self.view_size = area_rect.size();
        self.view_origin = area_rect.min;
        // Ctrl+колесо (и щипок на тачпаде) меняет масштаб вокруг мыши.
        let zoom_delta = ui.input(|i| i.zoom_delta());
        if zoom_delta != 1.0 && ui.rect_contains_pointer(area_rect) {
            let anchor = ui
                .input(|i| i.pointer.hover_pos())
                .map(|p| p - area_rect.min);
            let zoom = self.zoom() * zoom_delta;
            self.set_zoom(Zoom::Fixed(zoom), anchor);
        }

        let zoom = self.zoom();
        let Viewer {
            library,
            painter,
            document,
            ..
        } = self;
        let Some(document) = document else {
            return;
        };
        let pages = &document.layout.pages;
        let sizes: Vec<(f32, f32)> = pages.iter().map(|p| (p.width, p.height)).collect();
        let placement = place_pages(&sizes, zoom, area_rect.width());

        if std::mem::take(&mut self.reveal_cursor)
            && let Some(c) = caret(&document.layout, document.cursor)
            && let Some(page) = placement.rects.get(c.page)
        {
            let scale = zoom * SCREEN_PIXELS_PER_POINT;
            let bar = Rect::from_min_size(
                page.min + Vec2::new(c.x, c.top) * scale,
                Vec2::new(1.0, c.height * scale),
            );
            let base = self.pending_offset.unwrap_or(self.offset);
            self.pending_offset = Some(reveal(base, area_rect.size(), bar));
        }

        let mut area = egui::ScrollArea::both().auto_shrink(false);
        if let Some(offset) = self.pending_offset.take() {
            area = area.scroll_offset(offset);
        }
        let output = area.show_viewport(ui, |ui, viewport| {
            let origin = ui.max_rect().min;
            let response = ui
                .allocate_rect(
                    Rect::from_min_size(origin, placement.size),
                    Sense::click_and_drag(),
                )
                .on_hover_cursor(CursorIcon::Text);
            let ppp = ui.ctx().pixels_per_point();
            // Лист с целого пикселя экрана — буквы чётче.
            let screen_rect = |i: usize| {
                placement.rects[i]
                    .translate(origin.to_vec2())
                    .round_to_pixels(ppp)
            };
            // Пунктов документа на точку экрана.
            let points = 1.0 / (zoom * SCREEN_PIXELS_PER_POINT);

            // Щелчок ставит курсор, как в Word — сразу при нажатии;
            // с Shift — выделяет до места щелчка. Если тянуть мышь с
            // нажатой кнопкой, выделение идёт за ней.
            let now = ui.input(|i| i.time);
            let (press, dragging, pointer, shift) = ui.input(|i| {
                (
                    i.pointer.primary_pressed(),
                    i.pointer.primary_down(),
                    i.pointer.interact_pos(),
                    i.modifiers.shift,
                )
            });
            let under_pointer = pointer.and_then(|pointer| {
                let page = nearest_page(&placement.rects, pointer - origin.to_vec2())?;
                let local = (pointer - screen_rect(page).min) * points;
                hit_test(&document.layout, page, local.x, local.y)
            });
            if let Some(position) = under_pointer {
                if press && response.hovered() {
                    document.cursor = position;
                    if !shift {
                        document.anchor = position;
                    }
                    document.cursor_since = now;
                    document.goal_x = None;
                } else if dragging && response.is_pointer_button_down_on() {
                    document.cursor = position;
                    document.goal_x = None;
                }
                // Двойной щелчок — слово, тройной — абзац.
                let picked = if response.triple_clicked() {
                    Some(paragraph_at(&document.layout, position.paragraph))
                } else if response.double_clicked() {
                    Some(word_at(&document.layout, position))
                } else {
                    None
                };
                if let Some(picked) = picked {
                    document.anchor = picked.anchor;
                    document.cursor = picked.focus;
                    document.goal_x = None;
                }
            }

            // Тень листа выходит за его края: рисуем и чуть невидимые.
            let visible = viewport.expand(PAGE_GAP);
            for (i, page) in pages.iter().enumerate() {
                if placement.rects[i].intersects(visible) {
                    painter.paint_page(ui.painter(), library, page, screen_rect(i));
                }
            }

            // Выделение — полупрозрачная заливка поверх текста.
            let selection = Selection::new(document.anchor, document.cursor);
            for r in selection_rects(&document.layout, selection) {
                if r.page < placement.rects.len() {
                    let page = screen_rect(r.page);
                    let scale = 1.0 / points;
                    let rect = Rect::from_min_max(
                        page.min + Vec2::new(r.left, r.top) * scale,
                        page.min + Vec2::new(r.right, r.bottom) * scale,
                    );
                    ui.painter().rect_filled(rect, 0.0, SELECTION_COLOR);
                }
            }

            if let Some(c) = caret(&document.layout, document.cursor) {
                let (blink, next) = caret_phase(now - document.cursor_since);
                // При выделении курсор не рисуется, как в Word.
                let shown = blink && selection.is_empty();
                if c.page < placement.rects.len() {
                    let page = screen_rect(c.page);
                    let scale = 1.0 / points;
                    // Курсор — один пиксель экрана шириной.
                    let x = (page.min.x + c.x * scale).round_to_pixels(ppp);
                    let top = page.min.y + c.top * scale;
                    let bar = Rect::from_min_max(
                        Pos2::new(x, top),
                        Pos2::new(x + 1.0 / ppp, top + c.height * scale),
                    );
                    if shown {
                        ui.painter().rect_filled(bar, 0.0, Color32::BLACK);
                    }
                    // Включает IME (китайский, японский ввод и т. п.) и
                    // говорит ему, где открыть окошко подсказок.
                    ui.ctx().output_mut(|o| {
                        o.ime = Some(egui::output::IMEOutput {
                            purpose: egui::IMEPurpose::Normal,
                            rect: page,
                            cursor_rect: bar,
                            should_interrupt_composition: false,
                        });
                    });
                }
                ui.ctx()
                    .request_repaint_after(Duration::from_secs_f64(next.max(0.01)));
            }
        });
        self.offset = output.state.clamped_offset();
        self.view_size = output.inner_rect.size();
        self.current_page = current_page(&placement, self.offset.y, self.view_size.y);
    }

    fn error_window(&mut self, ctx: &Context) {
        let Some(message) = &self.error else {
            return;
        };
        let mut close = false;
        egui::Window::new("Не удалось открыть файл")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(message.as_str());
                ui.add_space(8.0);
                if ui.button("ОК").clicked() {
                    close = true;
                }
            });
        if close || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.error = None;
        }
    }
}

/// Клавиша перемещения курсора и что она делает. `screen` — сколько
/// Что значит событие клавиатуры: движение курсора, правка или ничего
/// (тогда событие достаётся меню и другим частям окна).
fn key_action(event: &egui::Event, screen: f32) -> Option<Action> {
    use egui::{Event, ImeEvent};
    match event {
        // Набранный текст. Tab и Enter приходят ещё и клавишами, поэтому
        // управляющие знаки отсюда выбрасываем.
        Event::Text(text) | Event::Ime(ImeEvent::Commit(text)) => {
            let text: String = text.chars().filter(|c| !c.is_control()).collect();
            (!text.is_empty()).then_some(Action::Edit(Edit::Text(text)))
        }
        Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => {
            let plain = !modifiers.command && !modifiers.alt;
            let edit = match key {
                Key::Enter if plain && modifiers.shift => Some(Edit::LineBreak),
                Key::Enter if plain => Some(Edit::Enter),
                Key::Tab if plain && !modifiers.shift => Some(Edit::Tab),
                Key::Backspace if plain => Some(Edit::Backspace),
                Key::Delete if plain => Some(Edit::Delete),
                _ => None,
            };
            if *key == Key::A && modifiers.command && !modifiers.alt && !modifiers.shift {
                return Some(Action::SelectAll);
            }
            edit.map(Action::Edit).or_else(|| {
                key_motion(*key, *modifiers, screen).map(|m| Action::Move(m, modifiers.shift))
            })
        }
        _ => None,
    }
}

/// пунктов документа видно на экране по высоте (для PageUp/PageDown).
fn key_motion(key: Key, modifiers: Modifiers, screen: f32) -> Option<Motion> {
    // Alt — для меню; Shift выделяет, а куда идти — так же.
    if modifiers.alt {
        return None;
    }
    let ctrl = modifiers.command;
    Some(match key {
        Key::ArrowLeft if ctrl => Motion::WordLeft,
        Key::ArrowLeft => Motion::Left,
        Key::ArrowRight if ctrl => Motion::WordRight,
        Key::ArrowRight => Motion::Right,
        Key::ArrowUp if ctrl => Motion::ParagraphUp,
        Key::ArrowUp => Motion::Up,
        Key::ArrowDown if ctrl => Motion::ParagraphDown,
        Key::ArrowDown => Motion::Down,
        Key::Home if ctrl => Motion::DocumentStart,
        Key::Home => Motion::LineStart,
        Key::End if ctrl => Motion::DocumentEnd,
        Key::End => Motion::LineEnd,
        Key::PageUp if !ctrl => Motion::ScreenUp(screen),
        Key::PageDown if !ctrl => Motion::ScreenDown(screen),
        _ => return None,
    })
}

/// Прокрутка, при которой прямоугольник `target` (координаты содержимого)
/// виден в окне размером `view`; если он уже виден, прокрутка `offset`
/// не меняется. Сверху и снизу оставляется немного места.
fn reveal(offset: Vec2, view: Vec2, target: Rect) -> Vec2 {
    const MARGIN: f32 = 24.0;
    let axis = |offset: f32, view: f32, min: f32, max: f32| {
        if min < offset {
            (min - MARGIN).max(0.0)
        } else if max > offset + view {
            max - view + MARGIN
        } else {
            offset
        }
    };
    Vec2::new(
        axis(offset.x, view.x, target.min.x, target.max.x),
        axis(offset.y, view.y, target.min.y, target.max.y),
    )
}

/// Лист под точкой `point` (координаты содержимого прокрутки) или,
/// если точка между листами, ближайший по вертикали.
fn nearest_page(rects: &[Rect], point: Pos2) -> Option<usize> {
    rects
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let distance = |r: &Rect| {
                if point.y < r.min.y {
                    r.min.y - point.y
                } else {
                    (point.y - r.max.y).max(0.0)
                }
            };
            distance(a).total_cmp(&distance(b))
        })
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, MouseWheelUnit, Pos2, RawInput, Rect, vec2};

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/docx")
            .join(name)
    }

    fn screen() -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 800.0))),
            ..RawInput::default()
        }
    }

    /// Один кадр окна; возвращает число нарисованных фигур.
    fn frame(ctx: &Context, viewer: &mut Viewer, input: RawInput) -> usize {
        let mut output = ctx.run_ui(input, |ui| viewer.ui(ui));
        // Текстуры отдаются видеокарте; в тесте её нет.
        output.textures_delta.clear();
        output.shapes.len()
    }

    #[test]
    fn empty_window_has_a_hint() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        frame(&ctx, &mut viewer, screen());
        assert_eq!(viewer.page_count(), 0);
        assert_eq!(viewer.title(), "letter");
        assert_eq!(viewer.zoom(), 1.0);
    }

    #[test]
    fn opened_document_is_drawn() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        let empty = frame(&ctx, &mut viewer, screen());
        viewer.open(&fixture("headings.docx")).unwrap();
        assert_eq!(viewer.title(), "headings.docx — letter");
        assert!(viewer.page_count() >= 1);
        let drawn = frame(&ctx, &mut viewer, screen());
        // Тень, лист и буквы добавились к меню и строке состояния.
        assert!(drawn >= empty + 3, "{empty} → {drawn}");
        assert_eq!(viewer.current_page, 0);
    }

    #[test]
    fn bad_file_shows_an_error_and_keeps_the_document() {
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("plain-text.docx")).unwrap();
        viewer.open_or_report(&fixture("generate.py"));
        assert!(viewer.error.is_some());
        assert_eq!(viewer.title(), "plain-text.docx — letter");
        viewer.open_or_report(&fixture("no-such-file.docx"));
        let message = viewer.error.clone().unwrap_or_default();
        assert!(message.contains("no-such-file.docx"), "{message}");
    }

    #[test]
    fn ctrl_wheel_zooms_and_keys_step() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("headings.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());

        // Ctrl + колесо вверх над листом.
        let mut input = screen();
        input.events = vec![
            Event::ModifiersChanged(Modifiers::CTRL),
            Event::PointerMoved(Pos2::new(500.0, 400.0)),
            Event::MouseWheel {
                unit: MouseWheelUnit::Line,
                delta: vec2(0.0, 1.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: Modifiers::CTRL,
            },
        ];
        frame(&ctx, &mut viewer, input);
        // Колесо сглаживается за несколько кадров.
        for _ in 0..30 {
            frame(&ctx, &mut viewer, screen());
        }
        assert!(viewer.zoom() > 1.0, "{}", viewer.zoom());

        // Ctrl+0 — 100 %, Ctrl+минус — 90 %.
        for (key, expected) in [(Key::Num0, 1.0), (Key::Minus, 0.9)] {
            let mut input = screen();
            input.events = vec![
                Event::ModifiersChanged(Modifiers::COMMAND),
                Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::COMMAND,
                },
            ];
            frame(&ctx, &mut viewer, input);
            assert!((viewer.zoom() - expected).abs() < 1e-4, "{}", viewer.zoom());
        }
    }

    #[test]
    fn page_width_follows_the_window() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("headings.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        viewer.set_zoom(Zoom::PageWidth, None);
        frame(&ctx, &mut viewer, screen());
        let wide = viewer.zoom();
        let mut narrow = screen();
        narrow.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 800.0)));
        frame(&ctx, &mut viewer, narrow);
        assert!(viewer.zoom() < wide, "{} vs {wide}", viewer.zoom());
        assert_eq!(viewer.zoom, Zoom::PageWidth);
    }

    #[test]
    fn caret_blinks() {
        assert_eq!(caret_phase(0.0), (true, CARET_BLINK));
        let (shown, next) = caret_phase(CARET_BLINK + 0.1);
        assert!(!shown && (next - (CARET_BLINK - 0.1)).abs() < 1e-9);
        assert!(caret_phase(2.0 * CARET_BLINK + 0.01).0);
        assert!(caret_phase(-1.0).0);
    }

    #[test]
    fn nearest_page_between_sheets() {
        let placement = place_pages(&[(612.0, 792.0); 2], 1.0, 1000.0);
        let rects = &placement.rects;
        assert_eq!(nearest_page(rects, Pos2::new(5.0, 5.0)), Some(0));
        assert_eq!(nearest_page(rects, rects[1].center()), Some(1));
        // Чуть ниже первого листа — он ближе второго.
        let gap = Pos2::new(500.0, rects[0].max.y + 2.0);
        assert_eq!(nearest_page(rects, gap), Some(0));
        assert_eq!(nearest_page(&[], gap), None);
    }

    #[test]
    fn click_puts_the_cursor() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        assert_eq!(viewer.cursor(), None);
        viewer.open(&fixture("plain-text.docx")).unwrap();
        assert_eq!(viewer.cursor(), Some(Position::default()));
        let before = frame(&ctx, &mut viewer, screen());

        // Щелчок далеко внизу первого листа — конец последнего абзаца.
        let point = Pos2::new(500.0, 700.0);
        let mut input = screen();
        input.events = vec![
            Event::PointerMoved(point),
            Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ];
        frame(&ctx, &mut viewer, input);
        let cursor = viewer.cursor().unwrap();
        let document = viewer.document.as_ref().unwrap();
        let last = document.layout.pages[0].lines.last().unwrap();
        assert_eq!(cursor.paragraph, last.paragraph);
        assert_eq!(cursor.offset, last.line.range.end);
        // Курсор мигает: через полпериода его нет — на одну фигуру меньше.
        let mut later = screen();
        later.time = Some(CARET_BLINK + 0.1);
        let hidden = frame(&ctx, &mut viewer, later);
        assert_eq!(hidden + 1, before, "{before} → {hidden}");
    }

    fn key(key: Key, modifiers: Modifiers) -> RawInput {
        let mut input = screen();
        input.events = vec![
            Event::ModifiersChanged(modifiers),
            Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            },
        ];
        input
    }

    #[test]
    fn keys_map_to_motions() {
        let ctrl = Modifiers::COMMAND;
        assert_eq!(
            key_motion(Key::ArrowLeft, Modifiers::NONE, 0.0),
            Some(Motion::Left)
        );
        assert_eq!(
            key_motion(Key::ArrowRight, ctrl, 0.0),
            Some(Motion::WordRight)
        );
        assert_eq!(
            key_motion(Key::ArrowDown, ctrl, 0.0),
            Some(Motion::ParagraphDown)
        );
        assert_eq!(
            key_motion(Key::Home, ctrl, 0.0),
            Some(Motion::DocumentStart)
        );
        assert_eq!(
            key_motion(Key::End, Modifiers::NONE, 0.0),
            Some(Motion::LineEnd)
        );
        assert_eq!(
            key_motion(Key::PageDown, Modifiers::NONE, 500.0),
            Some(Motion::ScreenDown(500.0))
        );
        assert_eq!(key_motion(Key::ArrowLeft, Modifiers::ALT, 0.0), None);
        assert_eq!(key_motion(Key::A, Modifiers::NONE, 0.0), None);
    }

    #[test]
    fn reveal_scrolls_only_when_needed() {
        let view = vec2(800.0, 600.0);
        let inside = Rect::from_min_size(Pos2::new(100.0, 300.0), vec2(1.0, 20.0));
        assert_eq!(reveal(vec2(0.0, 100.0), view, inside), vec2(0.0, 100.0));
        let below = Rect::from_min_size(Pos2::new(100.0, 2000.0), vec2(1.0, 20.0));
        let offset = reveal(Vec2::ZERO, view, below);
        assert!(offset.y > 2020.0 - 600.0 && offset.y < 2000.0, "{offset:?}");
        let above = Rect::from_min_size(Pos2::new(100.0, 10.0), vec2(1.0, 20.0));
        assert_eq!(reveal(vec2(0.0, 900.0), view, above).y, 0.0);
    }

    #[test]
    fn keys_move_the_cursor_and_the_view_follows() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("pages.docx")).unwrap();
        assert!(viewer.page_count() >= 2);
        frame(&ctx, &mut viewer, screen());

        frame(&ctx, &mut viewer, key(Key::ArrowRight, Modifiers::NONE));
        assert_eq!(viewer.cursor().map(|c| c.offset), Some(1));
        frame(&ctx, &mut viewer, key(Key::ArrowLeft, Modifiers::NONE));
        assert_eq!(viewer.cursor(), Some(Position::default()));

        // Ctrl+End — в конец документа, лист с курсором прокручен в окно.
        frame(&ctx, &mut viewer, key(Key::End, Modifiers::COMMAND));
        for _ in 0..3 {
            frame(&ctx, &mut viewer, screen());
        }
        let cursor = viewer.cursor().unwrap();
        let document = viewer.document.as_ref().unwrap();
        let last = document.layout.pages.last().unwrap().lines.last().unwrap();
        assert_eq!(cursor.paragraph, last.paragraph);
        assert_eq!(viewer.current_page + 1, viewer.page_count());

        frame(&ctx, &mut viewer, key(Key::Home, Modifiers::COMMAND));
        for _ in 0..3 {
            frame(&ctx, &mut viewer, screen());
        }
        assert_eq!(viewer.cursor(), Some(Position::default()));
        assert_eq!(viewer.current_page, 0);
    }

    fn events(events: Vec<Event>) -> RawInput {
        let mut input = screen();
        input.events = events;
        input
    }

    fn press(key: Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn first_text(viewer: &mut Viewer) -> String {
        let document = viewer.document.as_mut().unwrap();
        letter_editor::paragraph_mut(&mut document.doc, 0)
            .unwrap()
            .plain_text()
    }

    #[test]
    fn keys_map_to_edits() {
        let none = Modifiers::NONE;
        let action = |event: Event| key_action(&event, 0.0);
        assert_eq!(
            action(Event::Text("ы\t".into())),
            Some(Action::Edit(Edit::Text("ы".into())))
        );
        assert_eq!(action(Event::Text("\r".into())), None);
        assert_eq!(
            action(Event::Ime(egui::ImeEvent::Commit("漢字".into()))),
            Some(Action::Edit(Edit::Text("漢字".into())))
        );
        assert_eq!(
            action(press(Key::Enter, none)),
            Some(Action::Edit(Edit::Enter))
        );
        assert_eq!(
            action(press(Key::Enter, Modifiers::SHIFT)),
            Some(Action::Edit(Edit::LineBreak))
        );
        assert_eq!(action(press(Key::Tab, none)), Some(Action::Edit(Edit::Tab)));
        assert_eq!(action(press(Key::Tab, Modifiers::SHIFT)), None);
        assert_eq!(
            action(press(Key::Backspace, none)),
            Some(Action::Edit(Edit::Backspace))
        );
        assert_eq!(
            action(press(Key::Delete, none)),
            Some(Action::Edit(Edit::Delete))
        );
        assert_eq!(action(press(Key::Enter, Modifiers::COMMAND)), None);
        assert_eq!(
            action(press(Key::ArrowLeft, none)),
            Some(Action::Move(Motion::Left, false))
        );
    }

    #[test]
    fn typing_edits_the_document() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("pages.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        let before = first_text(&mut viewer);
        let lines = viewer.document.as_ref().unwrap().layout.pages[0]
            .lines
            .len();

        // События по порядку: «a», «b», ←, «c» — получается «acb».
        frame(
            &ctx,
            &mut viewer,
            events(vec![
                Event::Text("a".into()),
                Event::Text("b".into()),
                press(Key::ArrowLeft, Modifiers::NONE),
                Event::Text("c".into()),
            ]),
        );
        assert_eq!(first_text(&mut viewer), format!("acb{before}"));
        assert_eq!(viewer.cursor(), Some(Position::new(0, 2)));

        // Enter делит абзац: на листе на строку больше.
        frame(&ctx, &mut viewer, key(Key::Enter, Modifiers::NONE));
        assert_eq!(viewer.cursor(), Some(Position::new(1, 0)));
        assert_eq!(first_text(&mut viewer), "ac");
        let document = viewer.document.as_ref().unwrap();
        assert!(document.layout.pages[0].lines.len() > lines);

        // Backspace соединяет обратно, Delete и Backspace стирают буквы.
        frame(&ctx, &mut viewer, key(Key::Backspace, Modifiers::NONE));
        assert_eq!(viewer.cursor(), Some(Position::new(0, 2)));
        frame(&ctx, &mut viewer, key(Key::Delete, Modifiers::NONE));
        frame(&ctx, &mut viewer, key(Key::Backspace, Modifiers::NONE));
        frame(&ctx, &mut viewer, key(Key::Backspace, Modifiers::NONE));
        assert_eq!(first_text(&mut viewer), before);
        assert_eq!(viewer.cursor(), Some(Position::default()));

        // Курсор после правки рисуется, IME знает, где он.
        let output = ctx.run_ui(screen(), |ui| viewer.ui(ui));
        assert!(output.platform_output.ime.is_some());
    }

    #[test]
    fn shift_selects_and_typing_replaces_the_selection() {
        let none = Modifiers::NONE;
        assert_eq!(
            key_action(&press(Key::ArrowRight, Modifiers::SHIFT), 0.0),
            Some(Action::Move(Motion::Right, true))
        );
        assert_eq!(
            key_action(&press(Key::A, Modifiers::COMMAND), 0.0),
            Some(Action::SelectAll)
        );

        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("pages.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        let before = first_text(&mut viewer);

        // Shift+→ три раза: выделены три буквы, курсор не рисуется.
        let right = || press(Key::ArrowRight, Modifiers::SHIFT);
        frame(&ctx, &mut viewer, events(vec![right(), right(), right()]));
        let selection = viewer.selection().unwrap();
        assert_eq!(selection.start(), Position::new(0, 0));
        assert_eq!(selection.end().offset, 3);

        // ← снимает выделение и ставит курсор в его начало.
        frame(&ctx, &mut viewer, events(vec![press(Key::ArrowLeft, none)]));
        assert!(viewer.selection().unwrap().is_empty());
        assert_eq!(viewer.cursor(), Some(Position::new(0, 0)));

        // Набранная буква заменяет выделенное.
        frame(
            &ctx,
            &mut viewer,
            events(vec![right(), right(), Event::Text("Я".into())]),
        );
        assert_eq!(first_text(&mut viewer), format!("Я{}", &before[2..]));
        assert_eq!(viewer.cursor(), Some(Position::new(0, 2)));

        // Ctrl+A и Delete — документ пуст.
        frame(
            &ctx,
            &mut viewer,
            events(vec![
                press(Key::A, Modifiers::COMMAND),
                press(Key::Delete, none),
            ]),
        );
        assert_eq!(first_text(&mut viewer), "");
        let document = viewer.document.as_ref().unwrap();
        assert_eq!(document.layout.pages.len(), 1);
        assert_eq!(document.layout.pages[0].lines.len(), 1);
    }

    #[test]
    fn mouse_drag_and_double_click_select() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("pages.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        frame(&ctx, &mut viewer, screen());

        // Где на экране первая строка: от левого края текста вправо.
        let document = viewer.document.as_ref().unwrap();
        let line = document.layout.pages[0].lines[0].clone();
        let scale = viewer.zoom() * SCREEN_PIXELS_PER_POINT;
        let page = place_pages(
            &[(
                document.layout.pages[0].width,
                document.layout.pages[0].height,
            )],
            viewer.zoom(),
            viewer.view_size.x,
        )
        .rects[0];
        let origin = viewer.view_origin;
        let at = move |x: f32| {
            origin + page.min.to_vec2() + vec2(line.x + x, line.top + line.height / 2.0) * scale
        };
        let button = |pos: Pos2, pressed: bool| Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };

        // Нажали у начала строки, протянули вправо, отпустили.
        frame(
            &ctx,
            &mut viewer,
            events(vec![Event::PointerMoved(at(0.0)), button(at(0.0), true)]),
        );
        frame(
            &ctx,
            &mut viewer,
            events(vec![Event::PointerMoved(at(60.0))]),
        );
        frame(
            &ctx,
            &mut viewer,
            events(vec![Event::PointerMoved(at(60.0)), button(at(60.0), false)]),
        );
        let selection = viewer.selection().unwrap();
        assert_eq!(selection.anchor, Position::new(0, 0));
        assert!(selection.focus.offset > 3, "{selection:?}");

        // Двойной щелчок по первому слову выделяет его.
        let mut input = events(vec![
            button(at(2.0), true),
            button(at(2.0), false),
            button(at(2.0), true),
            button(at(2.0), false),
        ]);
        input.time = Some(10.0);
        frame(&ctx, &mut viewer, input);
        let selection = viewer.selection().unwrap();
        let word = word_at(
            &viewer.document.as_ref().unwrap().layout,
            Position::new(0, 0),
        );
        assert_eq!(
            (selection.start(), selection.end()),
            (word.start(), word.end())
        );
        assert!(!selection.is_empty());
    }
}
