//! Окно документа: меню «Файл» (создать, открыть, последние файлы,
//! сохранить, сохранить как; вопрос «сохранить изменения?»,
//! автосохранение и восстановление после сбоя), листы с прокруткой, масштаб
//! (Ctrl+колесо, кнопки, ползунок, «по ширине страницы»), строка
//! состояния и мигающий курсор, который ставится щелчком мыши и ходит
//! по тексту клавишами (стрелки, Home/End, PageUp/PageDown, с Ctrl —
//! по словам, абзацам и к краям документа). Текст можно набирать
//! (в том числе через IME), Enter делит абзац, Shift+Enter переносит
//! строку, Backspace и Delete стирают, Tab вставляет табуляцию.
//! Выделение — Shift с клавишами перемещения, мышью (протянуть,
//! Shift+щелчок, двойной щелчок — слово, тройной — абзац) и Ctrl+A.
//! Ctrl+Z отменяет правку, Ctrl+Y (и Ctrl+Shift+Z) повторяет.
//! Ctrl+C, Ctrl+X, Ctrl+V — копировать, вырезать, вставить.
//! Остальные сочетания Word (Ctrl+B, Ctrl+L, Ctrl+M, Ctrl+Alt+1…) — в
//! таблице `shortcuts.rs`.

use crate::i18n::{tr, tr_args};
use crate::shortcuts::{self, Shortcut};
use crate::story::StoryEdit;
use std::path::{Path, PathBuf};

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use eframe::egui::{
    self, Align, Align2, Color32, Context, CursorIcon, Key, Layout, Modifiers, Pos2, Rect, Sense,
    Vec2, ViewportCommand, emath::GuiRounding, pos2,
};
use letter_editor::{
    Command, FieldContext, History, ListKind, Motion, PictureBox, PictureWrap, Position, RunFormat,
    Selection, anchor_with_wrap, bookmark_position, bookmark_range, bookmarks,
    can_continue_numbering, caret, copy_range, find_all, find_next, formatting_at, hit_test,
    language_at, link_at, list_kind, move_cursor, moved_anchor, paragraph_at, picture_at_point,
    picture_ref, select_all, selected_picture, selection_rects, word_at,
};
use letter_layout::{DocumentLayout, FontLibrary, LayoutCache, Shaper, layout_document_cached};
use letter_model::{
    Alignment, BreakKind, Document, FieldChar, FieldCharKind, NoteKind, Paragraph, Run, RunContent,
    Statistics,
};
use letter_render::{MarkKind, PagePainter, SCREEN_PIXELS_PER_POINT, formatting_marks};

use crate::backstage::{Backstage, BackstageInfo, DocumentInfo, Page};
use crate::bookmark_dialog::{BookmarkAction, BookmarkDialog};
use crate::clipboard::{Clipboard, PasteMode, SystemBuffer};
use crate::control_dialog::ControlDialog;
use crate::files::{self, Autosave, OPEN_FILTERS, Recent, Recovered, SAVE_FILTERS};
use crate::find::{FindAction, FindDialog};
use crate::font_dialog::{FontDialog, FontValues};
use crate::icons::{self, Icon};
use crate::link_dialog::{LinkAction, LinkDialog};
use crate::modes::{
    LayoutView, OutlineView, WEB_MARGIN, outline_hidden, outline_indent, outline_levels,
    view_document,
};
use crate::page_number_dialog::PageNumberDialog;
use crate::paragraph_dialog::{ParagraphDialog, ParagraphValues};
use crate::password_dialog::{PasswordAnswer, PasswordDialog};
use crate::picture::{Handle, PictureDrag};
use crate::protect_dialog::{ProtectAction, ProtectDialog, ProtectMode};
use crate::ribbon::{Ribbon, RibbonCommand, RibbonInfo};
use crate::ruler::{self, RULER_SIZE, RulerParagraph, RulerState};
use crate::settings::Settings;
use crate::symbols::SymbolDialog;
use crate::theme::{Palette, ThemeChoice};
use crate::view::{
    MAX_ZOOM, MIN_ZOOM, PAGE_GAP, PageBox, ViewMode, Zoom, anchored_offset, clamp_zoom,
    current_page, page_width_zoom, place_view, read_zoom, step_zoom,
};
use letter_editor::CoverPage;

/// Образец текста для окна «Шрифт»: одна строка с оформлением `props`
/// по центру `rect`, свёрстанная и нарисованная как в документе (100 %).
fn paint_font_sample(
    painter: &egui::Painter,
    rect: Rect,
    props: &letter_model::RunProps,
    library: &FontLibrary,
    shaper: &mut Shaper,
    page_painter: &mut PagePainter,
) {
    use letter_model::{Block, PageMargins, PageSize, Paragraph, Run, Twips};
    let text = props
        .fonts
        .ascii
        .clone()
        .filter(|f| !f.is_empty())
        .unwrap_or_else(|| tr("preview").to_owned());
    let mut doc = Document::new();
    // Размер листа — как у прямоугольника при масштабе 100 %.
    let twips = |points: f32| Twips((points / SCREEN_PIXELS_PER_POINT * 20.0) as i32);
    let section = &mut doc.sections[0];
    section.props.page_size = Some(PageSize {
        width: twips(rect.width()),
        height: twips(rect.height() * 4.0),
        orientation: Default::default(),
    });
    section.props.margins = Some(PageMargins {
        top: Twips(0),
        right: Twips(0),
        bottom: Twips(0),
        left: Twips(0),
        ..PageMargins::default()
    });
    let mut paragraph = Paragraph::from_runs([Run::text(text).with_props(props.clone())]);
    paragraph.props.alignment = Some(Alignment::Center);
    section.blocks = vec![Block::from(paragraph)];
    let layout = letter_layout::layout_document(&doc, shaper, library);
    let Some(page) = layout.pages.first() else {
        return;
    };
    let Some(line) = page.lines.first() else {
        return;
    };
    let scale = rect.width() / page.width;
    let line_middle = (line.baseline - line.line.ascent / 2.0 + line.line.descent / 2.0) * scale;
    let top = rect.center().y - line_middle;
    let sheet = Rect::from_min_size(
        pos2(rect.left(), top),
        Vec2::new(rect.width(), page.height * scale),
    );
    page_painter.paint_page(painter, library, page, sheet);
}

/// Название программы в заголовке окна.
pub const APP_NAME: &str = "letter";

/// Имя ещё не сохранённого документа.
const UNTITLED: &str = "new-document";

/// Заливка выделения: полупрозрачная серо-голубая, текст под ней виден.
const SELECTION_COLOR: Color32 = Color32::from_rgba_premultiplied(0, 50, 110, 70);
/// Рамка и ярлык элемента управления содержимым (сине-серые, как в Word).
const CONTROL_FRAME_COLOR: Color32 = Color32::from_rgb(0x6F, 0x8F, 0xB8);
/// Подсветка текста примечания.
const COMMENT_COLOR: Color32 = Color32::from_rgba_premultiplied(120, 90, 0, 50);
/// Подсветка текста примечания, в котором курсор.
const COMMENT_CURRENT_COLOR: Color32 = Color32::from_rgba_premultiplied(170, 120, 0, 100);

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

/// Формула, которую правит вкладка «Уравнение»: выделенная целиком или
/// стоящая сразу перед или за курсором. Возвращает её место.
fn equation_at(doc: &Document, selection: Selection) -> Option<(Position, &letter_model::Math)> {
    let (start, end) = (selection.start(), selection.end());
    let len = letter_model::OBJECT_CHAR.len_utf8();
    if start.paragraph != end.paragraph {
        return None;
    }
    let at = |p: Position| letter_editor::math_ref(doc, p).map(|m| (p, m));
    if end.offset == start.offset + len {
        return at(start);
    }
    if start != end {
        return None;
    }
    at(start).or_else(|| {
        let before = Position::new(start.paragraph, start.offset.checked_sub(len)?);
        at(before)
    })
}

/// Переходит по ссылке у места `at`: адрес открывается в браузере;
/// для ссылки на закладку возвращает, где закладка.
fn follow_link(ctx: &Context, doc: &Document, at: Position) -> Option<Position> {
    let (link, _, _) = link_at(doc, at)?;
    if link.target.is_some() {
        ctx.open_url(egui::OpenUrl::new_tab(link.address()));
        return None;
    }
    bookmark_position(doc, link.anchor.as_deref()?)
}

/// Открытый документ.
struct OpenDocument {
    /// Откуда открыт и куда сохранять; `None` — ещё не сохранён.
    path: Option<PathBuf>,
    /// Пароль на открытие: с ним документ шифруется при сохранении.
    password: Option<String>,
    /// [`History::state`] при последнем сохранении: если сейчас другое,
    /// документ изменён. `u64::MAX` — изменён заведомо (восстановлен).
    saved: u64,
    /// [`History::state`] при последнем автосохранении.
    autosaved: u64,
    /// Сам документ; правки меняют его, а потом он верстается заново.
    doc: Document,
    /// Отмена и повтор правок.
    history: History,
    /// Свёрстанные страницы.
    layout: DocumentLayout,
    /// Абзацы прошлой вёрстки: после правки заново верстаются только
    /// изменённые.
    cache: LayoutCache,
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
    /// Статистика для строки состояния, посчитанная для одного состояния
    /// документа и выделения: каждый кадр не пересчитывается.
    counts: Option<Counts>,
    /// Открытый для правки колонтитул: тогда `doc`, `history` и `layout`
    /// — колонтитула, а сам документ отложен здесь.
    story: Option<Box<StoryEdit>>,
    /// Открыт только для чтения: у файла пароль на изменение или
    /// «рекомендовать только чтение», и правку ещё не разрешили.
    read_only: bool,
    /// Как верстается документ: по листам или по ширине окна
    /// («Веб-документ», «Структура»).
    view: LayoutView,
    /// Уровни абзацев для значков режима «Структура» (посчитаны при
    /// вёрстке в этом режиме).
    outline: Vec<Option<u8>>,
    /// Что видно в режиме «Структура» (уровни, свёрнутые заголовки).
    outline_view: OutlineView,
    /// Скрытые в режиме «Структура» абзацы (посчитаны при вёрстке).
    outline_hidden: Vec<bool>,
}

/// Статистика документа и выделения для строки состояния.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Counts {
    /// [`History::state`], для которого посчитано.
    state: u64,
    /// Выделение, для которого посчитано.
    selection: Selection,
    /// Весь документ.
    total: Statistics,
    /// Выделенный текст; `None`, если ничего не выделено.
    selected: Option<Statistics>,
}

/// Имя пользователя системы: им подписываются исправления.
fn user_name() -> String {
    ["USERNAME", "USER", "LOGNAME"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|v| !v.trim().is_empty()))
        .unwrap_or_else(|| "letter".to_string())
}

impl OpenDocument {
    /// Новый открытый документ, курсор в начале.
    fn new(
        path: Option<PathBuf>,
        doc: Document,
        layout: DocumentLayout,
        cache: LayoutCache,
    ) -> OpenDocument {
        OpenDocument {
            cache,
            path,
            password: None,
            saved: 0,
            autosaved: 0,
            history: {
                let mut history = History::new();
                history.set_author(user_name());
                history
            },
            layout,
            cursor: Position::default(),
            anchor: Position::default(),
            cursor_since: 0.0,
            goal_x: None,
            counts: None,
            story: None,
            view: LayoutView::Pages,
            outline: Vec::new(),
            outline_view: OutlineView::default(),
            outline_hidden: Vec::new(),
            read_only: doc
                .settings
                .write_protection
                .as_ref()
                .is_some_and(|w| w.recommended || w.password.is_some()),
            doc,
        }
    }

    /// Статистика документа и выделения (пересчитывается после правки
    /// или нового выделения).
    fn counts(&mut self) -> Counts {
        let state = self.history.state();
        let selection = self.selection();
        match self.counts {
            Some(counts) if counts.state == state && counts.selection == selection => counts,
            _ => {
                let selected = (!selection.is_empty()).then(|| {
                    copy_range(&self.doc, selection.start(), selection.end()).statistics()
                });
                let counts = Counts {
                    state,
                    selection,
                    total: self.doc.statistics(),
                    selected,
                };
                self.counts = Some(counts);
                counts
            }
        }
    }

    /// Изменён ли документ после сохранения.
    fn modified(&self) -> bool {
        match &self.story {
            Some(story) => story.history.state() != self.saved || self.history.can_undo(),
            None => self.history.state() != self.saved,
        }
    }

    /// Имя для заголовка окна и вопросов.
    fn name(&self) -> String {
        self.path.as_ref().and_then(|p| p.file_name()).map_or_else(
            || tr(UNTITLED).to_owned(),
            |n| n.to_string_lossy().into_owned(),
        )
    }

    /// Выделение от якоря до курсора.
    fn selection(&self) -> Selection {
        Selection::new(self.anchor, self.cursor)
    }

    /// Ставит выделение (и курсор на его конец, где `focus`).
    fn set_selection(&mut self, selection: Selection) {
        self.anchor = selection.anchor;
        self.cursor = selection.focus;
        self.goal_x = None;
    }

    /// Верстает документ заново после правки.
    fn relayout(&mut self, shaper: &mut Shaper, library: &FontLibrary) {
        match &mut self.story {
            Some(story) => {
                story.write_back(&self.doc);
                story.display =
                    layout_document_cached(&story.body, shaper, library, &mut self.cache);
                self.layout = story.edit_layout();
            }
            None => {
                let copy = view_document(&self.doc, self.view, &self.outline_view);
                let doc = copy.as_ref().unwrap_or(&self.doc);
                self.layout = layout_document_cached(doc, shaper, library, &mut self.cache);
                self.outline = match self.view {
                    LayoutView::Outline { .. } => outline_levels(&self.doc),
                    _ => Vec::new(),
                };
                self.outline_hidden = outline_hidden(&self.outline, &self.outline_view);
            }
        }
    }

    /// Разворачивает (`expand`) или сворачивает в режиме «Структура»
    /// заголовок у курсора (у обычного текста — заголовок над ним).
    fn expand_heading(&mut self, expand: bool) {
        let levels = outline_levels(&self.doc);
        let Some(heading) = (0..=self.cursor.paragraph)
            .rev()
            .find(|&i| levels.get(i).copied().flatten().is_some())
        else {
            return;
        };
        if expand {
            self.outline_view.collapsed.remove(&heading);
        } else {
            self.outline_view.collapsed.insert(heading);
        }
    }

    /// Курсор в скрытом абзаце режима «Структура» уходит из него: если
    /// он пришёл сверху (`forward`) — в начало ближайшего видимого абзаца
    /// ниже, иначе — в конец ближайшего видимого выше.
    fn leave_hidden_towards(&mut self, forward: bool) {
        let hidden = |i: usize| self.outline_hidden.get(i).copied().unwrap_or(false);
        if !hidden(self.cursor.paragraph) && !hidden(self.anchor.paragraph) {
            return;
        }
        let count = letter_editor::paragraph_count(&self.doc);
        let below = (self.cursor.paragraph..count).find(|&i| !hidden(i));
        let above = (0..=self.cursor.paragraph).rev().find(|&i| !hidden(i));
        let end = |index: usize| {
            let len =
                letter_editor::paragraph_ref(&self.doc, index).map_or(0, |p| p.plain_text().len());
            Position::new(index, len)
        };
        let at = match (forward, below, above) {
            (true, Some(index), _) => Position::new(index, 0),
            (_, _, Some(index)) => end(index),
            (_, Some(index), None) => Position::new(index, 0),
            (_, None, None) => Position::default(),
        };
        self.set_selection(Selection::caret(at));
    }

    /// То же, когда курсор поставлен командой (сворачивание и т. п.):
    /// в видимый абзац выше.
    fn leave_hidden(&mut self) {
        self.leave_hidden_towards(false);
    }

    /// Сам документ (и во время правки колонтитула — с ним).
    fn body(&self) -> &Document {
        self.story.as_ref().map_or(&self.doc, |s| &s.body)
    }

    /// Править документ нельзя: он открыт только для чтения или защищён
    /// («Ограничить редактирование»).
    fn locked(&self) -> bool {
        self.read_only
            || self
                .body()
                .settings
                .protection
                .as_ref()
                .is_some_and(|p| p.locks_editing())
    }

    /// Открывает для правки верхний (`header`) или нижний колонтитул
    /// страницы `page`.
    fn begin_story(
        &mut self,
        page: usize,
        header: bool,
        shaper: &mut Shaper,
        library: &FontLibrary,
    ) {
        if self.locked() {
            return;
        }
        if self.story.is_some() {
            self.end_story(shaper, library);
        }
        let Some(shown) = self.layout.pages.get(page) else {
            return;
        };
        let (section, kind) = (shown.section, shown.kind);
        let display = self.layout.clone();
        let body = std::mem::take(&mut self.doc);
        let history = std::mem::take(&mut self.history);
        let (edit, story) = StoryEdit::begin(
            body,
            history,
            self.cursor,
            self.anchor,
            display,
            page,
            section,
            kind,
            header,
        );
        self.doc = story;
        self.story = Some(Box::new(edit));
        self.cursor = Position::default();
        self.anchor = Position::default();
        self.goal_x = None;
        self.relayout(shaper, library);
    }

    /// Открывает для правки текст сноски `id` вида `kind` (на той
    /// странице, где он виден).
    fn begin_note(
        &mut self,
        kind: NoteKind,
        id: String,
        shaper: &mut Shaper,
        library: &FontLibrary,
    ) {
        if self.locked() {
            return;
        }
        if self.story.is_some() {
            self.end_story(shaper, library);
        }
        let Some(page) = self
            .layout
            .pages
            .iter()
            .position(|p| p.notes.iter().any(|n| n.kind == kind && n.id == id))
        else {
            return;
        };
        let display = self.layout.clone();
        let body = std::mem::take(&mut self.doc);
        let history = std::mem::take(&mut self.history);
        let (edit, story) = StoryEdit::begin_note(
            body,
            history,
            self.cursor,
            self.anchor,
            display,
            page,
            kind,
            id,
        );
        self.doc = story;
        self.story = Some(Box::new(edit));
        self.cursor = Position::default();
        self.anchor = Position::default();
        self.goal_x = None;
        self.relayout(shaper, library);
    }

    /// Раздел основного текста, в котором начинается страница `page`
    /// (и когда открыт колонтитул).
    fn page_section(&self, page: usize) -> Option<&letter_model::Section> {
        let (layout, doc) = match &self.story {
            Some(story) => (&story.display, &story.body),
            None => (&self.layout, &self.doc),
        };
        doc.sections.get(layout.pages.get(page)?.section)
    }

    /// Правка колонтитулов раздела страницы `page` («Особый колонтитул
    /// для первой страницы», «Формат номеров страниц» и т. п.; `command`
    /// получает номер раздела): открытый колонтитул закрывается,
    /// правка делается (шаг отмены), и колонтитул той же страницы
    /// открывается снова.
    fn section_command(
        &mut self,
        page: usize,
        command: impl FnOnce(usize) -> Command,
        shaper: &mut Shaper,
        library: &FontLibrary,
    ) {
        if self.locked() {
            return;
        }
        let header = self.story.as_ref().and_then(|s| s.header());
        self.end_story(shaper, library);
        let Some(section) = self.layout.pages.get(page).map(|p| p.section) else {
            return;
        };
        let selection = self.selection();
        let selection = self.history.run(&mut self.doc, selection, command(section));
        self.set_selection(selection);
        self.relayout(shaper, library);
        if let Some(header) = header {
            self.begin_story(page, header, shaper, library);
        }
    }

    /// Переходит по ссылке у места `at`: адрес открывается в браузере,
    /// закладка — в документе (курсор встаёт на неё). `true` — курсор
    /// переставлен.
    fn follow_link(&mut self, ctx: &Context, at: Position) -> bool {
        let Some(position) = follow_link(ctx, &self.doc, at) else {
            return false;
        };
        self.cursor = position;
        self.anchor = position;
        self.goal_x = None;
        self.history.seal();
        true
    }

    /// Что нужно для обновления полей (F9): время, файл, страницы.
    fn field_context(&self) -> FieldContext {
        let (now, offset) = crate::fields::now();
        let (pages, page_of_paragraph) = match &self.story {
            // В колонтитуле все абзацы — на странице, где идёт правка.
            Some(story) => {
                let count = letter_editor::paragraph_count(&self.doc);
                (story.display.pages.len(), vec![story.page + 1; count])
            }
            None => {
                let mut pages = Vec::new();
                for (number, page) in self.layout.pages.iter().enumerate() {
                    for line in &page.lines {
                        if pages.len() <= line.paragraph {
                            pages.resize(line.paragraph + 1, 0);
                        }
                        if pages[line.paragraph] == 0 {
                            pages[line.paragraph] = number + 1;
                        }
                    }
                }
                (self.layout.pages.len(), pages)
            }
        };
        FieldContext {
            now,
            utc_offset: offset,
            path: self.path.as_ref().map(|p| p.display().to_string()),
            pages,
            page_of_paragraph,
            russian: crate::i18n::language() == crate::i18n::Language::Russian,
            user_name: std::env::var("USERNAME")
                .or_else(|_| std::env::var("USER"))
                .ok(),
        }
    }

    /// Кусок документа для вставки: поле с кодом `code` и результатом,
    /// посчитанным для страницы `page` (с нуля).
    fn field_fragment(&self, code: &str, page: usize) -> Box<Document> {
        let item = |content: RunContent| Run {
            content: vec![content],
            ..Run::default()
        };
        let mark = |kind| item(RunContent::FieldChar(FieldChar::new(kind)));
        let mut fragment = Document::new();
        fragment.properties = self.doc.properties.clone();
        fragment.sections[0].blocks = vec![
            Paragraph::from_runs([
                mark(FieldCharKind::Begin),
                item(RunContent::FieldCode(format!(" {code} "))),
                mark(FieldCharKind::Separate),
                mark(FieldCharKind::End),
            ])
            .into(),
        ];
        let mut context = self.field_context();
        // Число слов и т. п. — по самому документу, а не по куску.
        let mut whole = self.doc.clone();
        whole.sections.push(fragment.sections[0].clone());
        let last = letter_editor::paragraph_count(&whole) - 1;
        context.page_of_paragraph = vec![page + 1; last + 1];
        letter_editor::update_fields(&mut whole, last, last, &context);
        if let Some(section) = whole.sections.pop() {
            fragment.sections[0] = section;
        }
        Box::new(fragment)
    }

    /// Кончает правку колонтитула: изменение — один шаг отмены.
    fn end_story(&mut self, shaper: &mut Shaper, library: &FontLibrary) {
        let Some(edit) = self.story.take() else {
            return;
        };
        let (cursor, anchor) = (edit.cursor, edit.anchor);
        let (body, history, command) = edit.finish(&self.doc);
        self.doc = body;
        self.history = history;
        self.cursor = cursor;
        self.anchor = anchor;
        self.goal_x = None;
        if let Some(command) = command {
            let selection = self.selection();
            let selection = self.history.run(&mut self.doc, selection, command);
            self.set_selection(selection);
        }
        self.relayout(shaper, library);
    }
}

/// Что открыть или закрыть двойным щелчком.
enum StoryRequest {
    /// Колонтитул страницы: верхний (`true`) или нижний; место щелчка.
    HeaderFooter(usize, bool, Vec2),
    /// Текст сноски: вид и номер; место щелчка.
    Note((NoteKind, String), Vec2),
    /// Закончить правку.
    End,
}

/// Что сделать по событию клавиатуры.
#[derive(Debug, Clone, PartialEq)]
enum Action {
    /// Сдвинуть курсор; `true` — с Shift, выделение растёт.
    Move(Motion, bool),
    SelectAll,
    Edit(Command),
    Undo,
    Redo,
    Copy,
    Cut,
    /// Вставить; система передала текст из буфера.
    Paste(String, PasteMode),
}

/// Нажатие клавиши: сочетание из таблицы Word или правка и движение.
enum Step {
    Command(RibbonCommand),
    Action(Action),
}

/// Забирает из событий нажатия сочетаний из таблицы (`global_only` —
/// только общих) и возвращает их команды по порядку.
fn take_shortcuts(ctx: &Context, table: &[Shortcut], global_only: bool) -> Vec<RibbonCommand> {
    ctx.input_mut(|i| {
        let mut commands = Vec::new();
        i.events.retain(|event| {
            let command = match event {
                egui::Event::Key {
                    key,
                    physical_key,
                    pressed: true,
                    modifiers,
                    ..
                } => shortcuts::find(table, *key, *physical_key, *modifiers, global_only),
                _ => None,
            };
            match command {
                Some(command) => {
                    commands.push(command);
                    false
                }
                None => true,
            }
        });
        commands
    })
}

/// Что сделать, когда пользователь ответит, сохранять ли изменения.
#[derive(Debug, Clone, PartialEq)]
enum Intent {
    /// Создать новый документ.
    New,
    /// Открыть файл; `None` — сначала выбрать его.
    Open(Option<PathBuf>),
    /// Закрыть документ, оставив программу открытой.
    CloseDocument,
    /// Открыть документ, оставшийся после сбоя (номер в списке).
    Recover(usize),
    /// Закрыть программу.
    Close,
    /// «Сравнить»: выбрать исходный и изменённый документы и показать
    /// результат сравнения.
    Compare,
    /// «Найти и объединить»: показать документ слияния.
    Merge,
}

/// Слияние (вкладка «Рассылки»): таблица получателей и просмотр.
#[derive(Debug, Clone, Default)]
struct Mailing {
    /// Таблица получателей.
    data: letter_editor::MergeData,
    /// Показанная запись: номер среди отмеченных (с 0).
    record: usize,
    /// Включён «Просмотр результатов».
    preview: bool,
}

impl Mailing {
    /// Команда, которая ставит в поля слияния то, что сейчас нужно
    /// видеть: значения записи или имена полей.
    fn show(&self) -> Command {
        Command::MergePreview(self.preview.then(|| {
            let records = self.data.records();
            let row = records.get(self.record).or(records.last()).copied();
            Box::new((self.data.clone(), row.unwrap_or(0)))
        }))
    }
}

/// Ответ на вопрос «сохранить изменения?».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    Save,
    Discard,
    Cancel,
}

/// «5 мин назад» — когда сохранён документ.
fn ago(time: Option<SystemTime>) -> String {
    let minutes = time
        .and_then(|t| t.elapsed().ok())
        .map_or(0, |d| d.as_secs() / 60);
    match minutes {
        0 => tr("time-just-now").to_owned(),
        m if m < 60 => tr_args("time-minutes-ago", &[("n", m.into())]),
        m if m < 60 * 24 => tr_args("time-hours-ago", &[("n", (m / 60).into())]),
        m => tr_args("time-days-ago", &[("n", (m / (60 * 24)).into())]),
    }
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
    /// Заголовок окошка с ошибкой.
    error_title: &'static str,
    /// Вопрос «сохранить изменения?» и что сделать после ответа.
    ask: Option<Intent>,
    /// Пользователь согласился закрыть программу.
    close_confirmed: bool,
    /// Недавние файлы.
    recent: Recent,
    /// Автосохранение (`None` — некуда сохранять).
    autosave: Option<Autosave>,
    /// Когда автосохраняли последний раз.
    last_autosave: Instant,
    /// Документы, оставшиеся после сбоя.
    recovered: Vec<Recovered>,
    /// Прокрутка и размер видимой части в прошлом кадре.
    offset: Vec2,
    view_size: Vec2,
    /// Левый верхний угол видимой части на экране.
    view_origin: Pos2,
    /// Прокрутка, которую надо поставить в следующем кадре.
    pending_offset: Option<Vec2>,
    /// Лист в середине окна, с нуля.
    current_page: usize,
    /// Принтеры для страницы «Печать» (ищутся при первом её открытии).
    printers: Option<crate::print::PrinterList>,
    /// Надо обновить заголовок окна.
    title_changed: bool,
    /// Пользователь попросил окно выбора файла.
    open_requested: bool,
    /// Пользователь попросил вставить рисунок из файла.
    picture_requested: bool,
    /// Размер рисунка меняют мышью: какой маркер тянут и новый размер.
    picture_drag: Option<PictureDrag>,
    /// Курсор сдвинули с клавиатуры: прокрутить так, чтобы он был виден.
    reveal_cursor: bool,
    /// Прокрутить к листу (номер с нуля) так, чтобы место `top` (пт от
    /// верха листа) оказалось вверху окна.
    scroll_to: Option<(usize, f32)>,
    /// Область навигации.
    navigation: crate::navigation::NavigationPane,
    /// Область примечаний.
    comments: crate::comments::CommentsPane,
    /// Абзац курсора в прошлом кадре режима «Структура»: куда он шёл,
    /// чтобы перешагнуть скрытые абзацы.
    outline_cursor: usize,
    /// Найденное для области навигации: строка поиска, состояние
    /// документа и результат (чтобы не искать каждый кадр).
    nav_found: Option<(String, u64, Vec<letter_editor::Found>)>,
    /// Буфер обмена.
    clipboard: Clipboard,
    /// Лента.
    ribbon: Ribbon,
    /// Показывать линейки.
    show_ruler: bool,
    /// Режим просмотра.
    view_mode: ViewMode,
    /// Показывать знаки форматирования (¶).
    show_marks: bool,
    /// Открыто окно «Статистика» (щелчок по числу слов).
    statistics_open: bool,
    /// Лист для линеек (с курсором) и где он на экране в прошлом кадре.
    ruler_page: Option<(usize, Rect)>,
    /// Горизонтальная линейка: вид новой табуляции, перетаскивание.
    ruler_state: RulerState,
    /// Окно «Абзац».
    paragraph_dialog: ParagraphDialog,
    /// Окно «Ссылка».
    link_dialog: LinkDialog,
    /// Окно «Закладка».
    bookmark_dialog: BookmarkDialog,
    /// Окно «Свойства элемента управления содержимым».
    control_dialog: ControlDialog,
    /// Окно «Изменить список получателей».
    recipients_dialog: crate::merge_dialogs::RecipientsDialog,
    /// Окно правила рассылки.
    rule_dialog: crate::merge_dialogs::RuleDialog,
    /// «Режим конструктора» (вкладка «Разработчик»).
    design_mode: bool,
    /// Слияние: выбранные получатели.
    mailing: Option<Mailing>,
    page_number_dialog: PageNumberDialog,
    /// Окно «Ограничить редактирование».
    protect_dialog: ProtectDialog,
    /// Проверка орфографии.
    spelling: crate::spelling::Spelling,
    password_dialog: PasswordDialog,
    /// Окно «Символ».
    symbol_dialog: SymbolDialog,
    /// Окно «Шрифт».
    font_dialog: FontDialog,
    /// Тема, уже переданная egui.
    applied_theme: Option<ThemeChoice>,
    /// Шрифты для списка на ленте.
    font_names: Arc<[String]>,
    /// Окно «Найти и заменить».
    find: FindDialog,
    /// Представление «Файл».
    backstage: Backstage,
    /// Параметры программы.
    settings: Settings,
    /// Папка программы (параметры, недавние файлы, автосохранения).
    dir: Option<PathBuf>,
}

/// Шрифты Word, которые заменяются похожими (см. `FontLibrary::find`):
/// их тоже предлагаем в списке, как Word.
const WORD_FONTS: [&str; 6] = [
    "Arial",
    "Calibri",
    "Calibri Light",
    "Cambria",
    "Courier New",
    "Times New Roman",
];

/// Цвет знаков форматирования.
const MARKS_COLOR: Color32 = Color32::from_rgb(0x50, 0x60, 0x80);

/// Рисует знаки форматирования листа `page`, левый верхний угол
/// которого на экране — `origin`; `scale` — точек экрана на пункт.
/// Стрелки табуляции и разрыва строки — значки (в шрифте интерфейса
/// таких букв нет), остальное — буквы.
/// Во время правки колонтитула текст страницы бледнее, а край места
/// колонтитула отмечен пунктиром с подписью.
fn paint_story_frame(
    ui: &egui::Ui,
    page: &letter_layout::Page,
    rect: Rect,
    scale: f32,
    header: bool,
) {
    let (header_end, footer_start) = crate::story::bounds(page);
    let painter = ui.painter();
    let body = Rect::from_min_max(
        Pos2::new(rect.min.x, rect.min.y + header_end * scale),
        Pos2::new(rect.max.x, rect.min.y + footer_start * scale),
    );
    painter.rect_filled(
        body,
        0.0,
        Color32::from_rgba_unmultiplied(255, 255, 255, 140),
    );
    let y = rect.min.y + if header { header_end } else { footer_start } * scale;
    let color = Color32::from_rgb(43, 87, 154);
    painter.extend(egui::Shape::dashed_line(
        &[Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)],
        egui::Stroke::new(1.0, color),
        4.0,
        3.0,
    ));
    let label = tr(if header {
        "header-label"
    } else {
        "footer-label"
    });
    let font = egui::FontId::proportional(11.0);
    let galley = painter.layout_no_wrap(label.to_owned(), font, Color32::WHITE);
    let size = galley.size() + Vec2::new(8.0, 2.0);
    let top = if header { y } else { y - size.y };
    let tag = Rect::from_min_size(Pos2::new(rect.min.x + 1.0, top), size);
    painter.rect_filled(tag, 0.0, color);
    painter.galley(tag.min + Vec2::new(4.0, 1.0), galley, Color32::WHITE);
}

fn paint_marks(painter: &egui::Painter, page: &letter_layout::Page, origin: Pos2, scale: f32) {
    for mark in formatting_marks(page) {
        let size = (mark.size * scale).max(4.0);
        let x = origin.x + mark.x * scale;
        let baseline = origin.y + mark.baseline * scale;
        let icon = match mark.kind {
            MarkKind::Tab => Some(Icon::TabMark),
            MarkKind::LineBreak => Some(Icon::LineBreakMark),
            _ => None,
        };
        if let Some(icon) = icon {
            let side = size * 0.6;
            let left = if mark.kind == MarkKind::Tab {
                x - side / 2.0
            } else {
                x
            };
            let square = Rect::from_min_size(Pos2::new(left, baseline - side), Vec2::splat(side));
            icons::paint(painter, square, icon, MARKS_COLOR);
            continue;
        }
        let galley = painter.layout_no_wrap(
            mark.kind.symbol().to_string(),
            egui::FontId::proportional(size),
            MARKS_COLOR,
        );
        let Some(row) = galley.rows.first() else {
            continue;
        };
        let row_baseline = row.pos.y + row.row.glyphs.first().map_or(0.0, |g| g.pos.y);
        let shift = if mark.kind == MarkKind::Paragraph {
            0.0
        } else {
            galley.size().x / 2.0
        };
        let at = Pos2::new(x - shift, baseline - row_baseline);
        painter.galley(at, galley, MARKS_COLOR);
    }
}

/// Титульная страница вида `layout`: название и автор — из свойств
/// документа (или подсказки в скобках), дата — сегодняшняя.
fn cover_page(doc: &letter_model::Document, layout: letter_editor::CoverLayout) -> CoverPage {
    let core = &doc.properties.core;
    let or = |value: &Option<String>, placeholder: &str| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or(placeholder)
            .to_string()
    };
    let russian = crate::i18n::language() == crate::i18n::Language::Russian;
    let picture = if russian {
        "dd.MM.yyyy"
    } else {
        "MMMM d, yyyy"
    };
    let (now, _) = crate::fields::now();
    CoverPage {
        layout,
        title: or(&core.title, tr("cover-title")),
        subtitle: or(&core.subject, tr("cover-subtitle")),
        author: or(&core.creator, tr("cover-author")),
        date: letter_editor::format_date(picture, now, russian),
    }
}

/// Список шрифтов для ленты: установленные и привычные шрифты Word.
fn font_names(library: &FontLibrary) -> Arc<[String]> {
    let mut names = library.families();
    names.extend(WORD_FONTS.iter().map(|s| s.to_string()));
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names.into()
}

impl Viewer {
    /// Пустой просмотрщик со шрифтами `library`. Ничего не хранит на
    /// диске (ни недавних файлов, ни автосохранений) — для тестов.
    #[cfg(test)]
    pub fn new(library: FontLibrary) -> Viewer {
        Viewer::with_storage(library, None)
    }

    /// Просмотрщик, который хранит недавние файлы и автосохранения
    /// в папке программы `dir` (см. [`files::app_dir`]).
    pub fn with_storage(library: FontLibrary, dir: Option<PathBuf>) -> Viewer {
        let autosave_dir = dir.as_ref().map(|d| d.join("autosave"));
        let settings = Settings::load(dir.as_deref());
        crate::i18n::set_language(settings.language);
        Viewer {
            error_title: "",
            ask: None,
            close_confirmed: false,
            recent: Recent::load(dir.as_deref()),
            recovered: autosave_dir
                .as_deref()
                .map(Autosave::leftovers)
                .unwrap_or_default(),
            autosave: autosave_dir.as_deref().and_then(Autosave::start),
            last_autosave: Instant::now(),
            font_names: font_names(&library),
            find: FindDialog::default(),
            backstage: Backstage::default(),
            show_ruler: settings.show_ruler,
            view_mode: ViewMode::Print,
            settings,
            dir: dir.clone(),
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
            printers: None,
            title_changed: false,
            open_requested: false,
            picture_requested: false,
            picture_drag: None,
            reveal_cursor: false,
            scroll_to: None,
            navigation: Default::default(),
            comments: Default::default(),
            outline_cursor: 0,
            nav_found: None,
            clipboard: Clipboard::new(Box::new(SystemBuffer::default())),
            ribbon: Ribbon::default(),
            show_marks: false,
            statistics_open: false,
            ruler_page: None,
            ruler_state: RulerState::default(),
            paragraph_dialog: ParagraphDialog::default(),
            link_dialog: LinkDialog::default(),
            bookmark_dialog: BookmarkDialog::default(),
            control_dialog: ControlDialog::default(),
            recipients_dialog: Default::default(),
            rule_dialog: Default::default(),
            design_mode: false,
            mailing: None,
            page_number_dialog: PageNumberDialog::default(),
            protect_dialog: ProtectDialog::default(),
            spelling: crate::spelling::Spelling::start(dir.clone()),
            password_dialog: PasswordDialog::default(),
            symbol_dialog: SymbolDialog::default(),
            font_dialog: FontDialog::default(),
            applied_theme: None,
        }
    }

    /// Открывает документ (.docx, .rtf, .odt, .txt, .html, .md) и
    /// верстает его. При ошибке прежний документ остаётся на экране.
    ///
    /// Шаблон (.dotx, .dotm) открывается, как в Word, новым документом без
    /// имени: «Сохранить» спросит, куда его записать.
    ///
    /// Для зашифрованного файла сначала спрашивается пароль, а документ
    /// открывается потом, в [`Viewer::open_with_password`].
    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        if files::is_encrypted(path) {
            self.password_dialog.ask_to_open(path.to_owned());
            return Ok(());
        }
        let doc = files::read(path)?;
        let name = (!files::is_template(path)).then(|| path.to_owned());
        self.show(name, doc);
        self.recent.add(path);
        Ok(())
    }

    /// Открывает зашифрованный документ паролем `password`. При ошибке
    /// (например, неверном пароле) прежний документ остаётся на экране.
    pub fn open_with_password(&mut self, path: &Path, password: String) -> Result<(), String> {
        let doc = files::read_with_password(path, &password)?;
        let name = (!files::is_template(path)).then(|| path.to_owned());
        self.show(name, doc);
        if let Some(document) = &mut self.document {
            document.password = Some(password);
        }
        self.recent.add(path);
        Ok(())
    }

    /// Ответ из окна пароля.
    fn password_answer(&mut self, answer: PasswordAnswer) {
        match answer {
            PasswordAnswer::Open(path, password) => {
                if let Err(e) = self.open_with_password(&path, password) {
                    self.report(tr("error-open"), e);
                }
            }
            PasswordAnswer::Encrypt(password) => {
                if let Some(document) = &mut self.document
                    && document.password != password
                {
                    document.password = password;
                    // Пароль — изменение: без сохранения его в файле нет.
                    document.saved = u64::MAX;
                    self.title_changed = true;
                }
            }
        }
    }

    /// Открывает файл, а ошибку показывает пользователю.
    pub fn open_or_report(&mut self, path: &Path) {
        if let Err(e) = self.open(path) {
            if !path.exists() {
                self.recent.remove(path);
            }
            self.report(tr("error-open"), e);
        }
    }

    /// Новый пустой документ (Ctrl+N).
    pub fn new_document(&mut self) {
        self.show(None, files::blank());
    }

    /// Показывает документ вместо прежнего.
    fn show(&mut self, path: Option<PathBuf>, doc: Document) {
        let mut cache = LayoutCache::default();
        let layout = layout_document_cached(&doc, &mut self.shaper, &self.library, &mut cache);
        // Примечания документа видны сразу, как в Word.
        self.comments = crate::comments::CommentsPane::default();
        self.comments.open = !doc.comments.is_empty();
        self.document = Some(OpenDocument::new(path, doc, layout, cache));
        self.connect_recipients();
        if let Some(autosave) = &self.autosave {
            autosave.clear();
        }
        self.pending_offset = Some(Vec2::ZERO);
        self.current_page = 0;
        self.title_changed = true;
    }

    /// Закрывает документ («Файл → Закрыть»); программа остаётся.
    fn close_document(&mut self) {
        self.document = None;
        if let Some(autosave) = &self.autosave {
            autosave.clear();
        }
        self.title_changed = true;
    }

    /// Применяет и сохраняет новые параметры программы.
    fn change_settings(&mut self, settings: Settings) {
        if settings.show_ruler != self.settings.show_ruler {
            self.show_ruler = settings.show_ruler;
        }
        self.settings = settings;
        crate::i18n::set_language(settings.language);
        if let Some(dir) = &self.dir
            && let Err(e) = settings.save(dir)
        {
            self.report(tr("error-save-settings"), e);
        }
    }

    /// Что показать в представлении «Файл».
    fn backstage_info(&self) -> BackstageInfo {
        BackstageInfo {
            document: self.document.as_ref().map(|document| {
                let stats = document.body().statistics();
                let core = &document.body().properties.core;
                DocumentInfo {
                    name: document.name(),
                    path: document.path.clone(),
                    modified: document.modified(),
                    encrypted: document.password.is_some(),
                    pages: document.layout.pages.len(),
                    current_page: letter_editor::caret(&document.layout, document.cursor)
                        .map_or(self.current_page, |c| c.page),
                    page_sizes: document
                        .layout
                        .pages
                        .iter()
                        .map(|p| (p.width, p.height))
                        .collect(),
                    words: stats.words,
                    characters: stats.characters,
                    characters_with_spaces: stats.characters_with_spaces,
                    paragraphs: stats.paragraphs,
                    title: core.title.clone(),
                    author: core.creator.clone(),
                    last_modified_by: core.last_modified_by.clone(),
                    created: core.created.clone(),
                    changed: core.modified.clone(),
                }
            }),
            recent: self.recent.items().to_vec(),
            settings: self.settings,
            printers: self.printers.as_ref().and_then(|p| p.get()),
        }
    }

    fn report(&mut self, title: &'static str, message: String) {
        self.error_title = title;
        self.error = Some(message);
    }

    /// Изменён ли открытый документ после сохранения.
    pub fn modified(&self) -> bool {
        self.document.as_ref().is_some_and(OpenDocument::modified)
    }

    /// Делает `intent`, а если документ изменён — сначала спрашивает,
    /// сохранить ли изменения.
    fn request(&mut self, intent: Intent) {
        if self.modified() {
            self.ask = Some(intent);
        } else {
            self.perform(intent);
        }
    }

    fn perform(&mut self, intent: Intent) {
        match intent {
            Intent::New => self.new_document(),
            Intent::Open(Some(path)) => self.open_or_report(&path),
            Intent::Open(None) => self.pick_file(),
            Intent::Recover(index) => self.recover(index),
            Intent::Close => self.close_confirmed = true,
            Intent::CloseDocument => self.close_document(),
            Intent::Compare => self.pick_compare(),
            Intent::Merge => self.finish_merge(),
        }
    }

    /// «Выбрать получателей»: окно выбора таблицы (CSV или документ с
    /// таблицей).
    fn pick_recipients(&mut self) -> Vec<Command> {
        let mut dialog = rfd::FileDialog::new()
            .set_title(tr("mailings-open"))
            .add_filter(
                tr("mailings-filter"),
                &["csv", "txt", "tsv", "docx", "doc", "odt", "rtf"],
            );
        if let Some(dir) = self.dialog_dir() {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.pick_file() else {
            return Vec::new();
        };
        match read_recipients(&path) {
            Ok(data) => {
                // Источник запоминается в документе (`w:mailMerge`): после
                // открытия файла получатели подключатся снова.
                let kind = self
                    .document
                    .as_ref()
                    .and_then(|d| d.doc.settings.mail_merge.as_ref())
                    .map(|m| m.document_type.clone())
                    .unwrap_or_else(|| "formLetters".into());
                let merge = letter_model::MailMerge {
                    document_type: kind,
                    data_type: Some(
                        if is_table_file(&path) {
                            "textFile"
                        } else {
                            "native"
                        }
                        .into(),
                    ),
                    query: Some(format!("SELECT * FROM {}", path.display())),
                    data_source: Some(file_url(&path)),
                };
                let mut commands = vec![Command::SetMailMerge(Some(Box::new(merge)))];
                commands.extend(self.set_recipients(data));
                commands
            }
            Err(message) => {
                self.report("error-mailings", message);
                Vec::new()
            }
        }
    }

    /// Получатели из источника, который записан в документе, если файл
    /// на месте.
    fn connect_recipients(&mut self) {
        self.mailing = self
            .document
            .as_ref()
            .and_then(|d| d.doc.settings.mail_merge.as_ref())
            .and_then(|m| m.data_source.as_deref())
            .map(url_path)
            .filter(|path| path.is_file())
            .and_then(|path| read_recipients(&path).ok())
            .map(|data| Mailing {
                data,
                record: 0,
                preview: false,
            });
    }

    /// «Найти и объединить → Сохранить в PDF»: документ слияния сразу
    /// в файл PDF (открытый документ не меняется).
    fn merge_to_pdf(&mut self) {
        let (Some(document), Some(mailing)) = (&self.document, &self.mailing) else {
            return;
        };
        let merged = letter_editor::merge_all(&document.doc, &mailing.data);
        let mut dialog = rfd::FileDialog::new()
            .set_title(tr("mailings-pdf"))
            .add_filter("PDF", &["pdf"])
            .set_file_name("merge.pdf");
        if let Some(dir) = self.dialog_dir() {
            dialog = dialog.set_directory(dir);
        }
        let Some(mut path) = dialog.save_file() else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("pdf");
        }
        let mut cache = LayoutCache::default();
        let layout = layout_document_cached(&merged, &mut self.shaper, &self.library, &mut cache);
        if let Err(message) = files::write_pdf(&merged, &layout, &self.library, &path) {
            self.report("error-mailings", message);
        }
    }

    /// Запоминает таблицу получателей; если просмотр включён — команда,
    /// которая покажет в полях первую запись.
    fn set_recipients(&mut self, data: letter_editor::MergeData) -> Option<Command> {
        let preview = self.mailing.as_ref().is_some_and(|m| m.preview);
        let mailing = Mailing {
            data,
            record: 0,
            preview,
        };
        let command = preview.then(|| mailing.show());
        self.mailing = Some(mailing);
        command
    }

    /// Показывает документ слияния: копия документа на каждую запись.
    fn finish_merge(&mut self) {
        let (Some(document), Some(mailing)) = (&self.document, &self.mailing) else {
            return;
        };
        let merged = letter_editor::merge_all(&document.doc, &mailing.data);
        self.show(None, merged);
    }

    /// Окна выбора исходного и изменённого документов для сравнения.
    fn pick_compare(&mut self) {
        let pick = |title: &'static str, dir: Option<PathBuf>| {
            let mut dialog = rfd::FileDialog::new().set_title(tr(title));
            for (name, extensions) in OPEN_FILTERS {
                dialog = dialog.add_filter(tr(name), extensions);
            }
            if let Some(dir) = dir {
                dialog = dialog.set_directory(dir);
            }
            dialog.pick_file()
        };
        let Some(original) = pick("compare-original", self.dialog_dir()) else {
            return;
        };
        let Some(revised) = pick("compare-revised", original.parent().map(Path::to_path_buf))
        else {
            return;
        };
        if let Err(message) = self.compare_files(&original, &revised) {
            self.report("error-compare", message);
        }
    }

    /// Сравнивает документы и показывает результат новым документом:
    /// изменённый, где отличия от исходного — исправления.
    fn compare_files(&mut self, original: &Path, revised: &Path) -> Result<(), String> {
        let (old, new) = (files::read(original)?, files::read(revised)?);
        let result = letter_editor::compare_documents(
            &old,
            &new,
            &user_name(),
            Some(letter_editor::revisions::now_w3c()),
        );
        self.show(None, result);
        Ok(())
    }

    /// Сохраняет документ туда, откуда он открыт (Ctrl+S); если
    /// это новый документ или формат записать нельзя — спрашивает куда.
    /// `true`, если сохранён.
    pub fn save(&mut self) -> bool {
        let path = self
            .document
            .as_ref()
            .and_then(|d| d.path.clone())
            .filter(|p| files::FileFormat::from_path(p).is_some_and(files::FileFormat::writable));
        match path {
            Some(path) => self.save_to(&path),
            None => self.save_as(),
        }
    }

    /// «Сохранить как» (F12): окно выбора места и формата.
    pub fn save_as(&mut self) -> bool {
        self.save_as_format("docx")
    }

    /// «Сохранить как» с форматом `extension` по умолчанию («Экспорт»).
    pub fn save_as_format(&mut self, extension: &str) -> bool {
        let Some(document) = &self.document else {
            return false;
        };
        let stem = document
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map_or_else(
                || tr(UNTITLED).to_owned(),
                |s| s.to_string_lossy().into_owned(),
            );
        let mut dialog = rfd::FileDialog::new()
            .set_title(tr("save-as"))
            .set_file_name(format!("{stem}.{extension}"));
        // Выбранный формат — первым в списке.
        let (chosen, others): (Vec<_>, Vec<_>) = SAVE_FILTERS
            .iter()
            .partition(|(_, extensions)| extensions.contains(&extension));
        for &(name, extensions) in chosen.into_iter().chain(others) {
            dialog = dialog.add_filter(tr(name), extensions);
        }
        if let Some(dir) = self.dialog_dir() {
            dialog = dialog.set_directory(dir);
        }
        let Some(mut path) = dialog.save_file() else {
            return false;
        };
        if files::is_pdf(&path) || (extension == "pdf" && path.extension().is_none()) {
            path.set_extension("pdf");
            return self.save_pdf(&path);
        }
        if files::FileFormat::from_path(&path).is_none() {
            path.set_extension(extension);
        }
        self.save_to(&path)
    }

    /// Сохраняет документ в `path` (формат — по расширению). Ошибку
    /// показывает пользователю. `true`, если сохранён.
    pub fn save_to(&mut self, path: &Path) -> bool {
        let Some(document) = &mut self.document else {
            return false;
        };
        // Колонтитул дописывается в документ до сохранения.
        document.end_story(&mut self.shaper, &self.library);
        if let Err(e) = files::write(&document.doc, path, document.password.as_deref()) {
            self.report(tr("error-save"), e);
            return false;
        }
        document.path = Some(path.to_owned());
        document.saved = document.history.state();
        document.autosaved = document.saved;
        if let Some(autosave) = &self.autosave {
            autosave.clear();
        }
        self.recent.add(path);
        self.title_changed = true;
        true
    }

    /// Сохраняет копию документа в PDF `path`. Ошибку показывает
    /// пользователю. `true`, если сохранён.
    pub fn save_pdf(&mut self, path: &Path) -> bool {
        let Some(document) = &mut self.document else {
            return false;
        };
        // В PDF — сам документ, а не открытый колонтитул.
        document.end_story(&mut self.shaper, &self.library);
        match files::write_pdf(&document.doc, &document.layout, &self.library, path) {
            Ok(()) => true,
            Err(e) => {
                self.report(tr("error-save"), e);
                false
            }
        }
    }

    /// Печатает страницы задания `job`: записывает их во временный PDF и
    /// отдаёт системе. Ошибку показывает пользователю. `true`, если
    /// задание отправлено.
    pub fn print(&mut self, job: &crate::print::PrintJob) -> bool {
        let Some(document) = &mut self.document else {
            return false;
        };
        document.end_story(&mut self.shaper, &self.library);
        let pages: Vec<_> = job
            .pages
            .iter()
            .filter_map(|&i| document.layout.pages.get(i).cloned())
            .collect();
        if pages.is_empty() {
            return false;
        }
        let info = letter_render::PdfInfo {
            title: Some(document.name()),
            ..letter_render::PdfInfo::default()
        };
        let path = crate::print::temp_pdf();
        let bytes = letter_render::write_pdf(&pages, &self.library, &info);
        let sent = std::fs::write(&path, bytes)
            .map_err(|e| format!("{}: {e}", path.display()))
            .and_then(|()| crate::print::send(&path, job.printer.as_deref()));
        // `lp` уже передал файл службе печати; на Windows его ещё читает
        // программа просмотра PDF, там файл остаётся во временной папке.
        if !cfg!(windows) {
            let _ = std::fs::remove_file(&path);
        }
        match sent {
            Ok(()) => true,
            Err(e) => {
                self.report(tr("error-print"), e);
                false
            }
        }
    }

    /// Открывает документ, оставшийся после сбоя: он считается
    /// изменённым, сохранять его — туда, откуда он был открыт.
    fn recover(&mut self, index: usize) {
        if index >= self.recovered.len() {
            return;
        }
        let item = self.recovered.remove(index);
        match files::read(&item.autosave) {
            Ok(doc) => {
                self.show(item.original.clone(), doc);
                if let Some(document) = &mut self.document {
                    document.saved = u64::MAX;
                    document.autosaved = u64::MAX;
                }
                // Сразу своя копия: старую можно убрать.
                self.autosave_now();
                Autosave::discard(&item);
            }
            Err(e) => self.report(tr("error-recover"), e),
        }
    }

    /// Автосохраняет изменённый документ, если пора.
    fn autosave_tick(&mut self, ctx: &Context) {
        let Some(document) = &self.document else {
            return;
        };
        if !document.modified() || document.autosaved == document.history.state() {
            return;
        }
        let every = Duration::from_secs(u64::from(self.settings.autosave_minutes) * 60);
        let wait = every.saturating_sub(self.last_autosave.elapsed());
        if wait.is_zero() {
            self.autosave_now();
        } else {
            ctx.request_repaint_after(wait);
        }
    }

    fn autosave_now(&mut self) {
        self.last_autosave = Instant::now();
        let (Some(autosave), Some(document)) = (&self.autosave, &mut self.document) else {
            return;
        };
        // Зашифрованный документ не пишется на диск открытым текстом.
        if document.password.is_some() {
            return;
        }
        // Не вышло — попробуем в следующий раз; работе это не мешает.
        if autosave
            .save(document.body(), document.path.as_deref())
            .is_ok()
        {
            document.autosaved = document.history.state();
        }
    }

    /// Папка для окон «Открыть» и «Сохранить как».
    fn dialog_dir(&self) -> Option<PathBuf> {
        self.document
            .as_ref()
            .and_then(|d| d.path.as_ref())
            .or_else(|| self.recent.items().first())
            .and_then(|p| p.parent())
            .filter(|d| !d.as_os_str().is_empty())
            .map(Path::to_owned)
    }

    /// Рисует окно целиком.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // Крестик окна: если документ изменён, сначала спрашиваем.
        if ctx.input(|i| i.viewport().close_requested()) && !self.close_confirmed && self.modified()
        {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.ask = Some(Intent::Close);
        }
        crate::i18n::set_language(self.settings.language);
        // Подсказки клавиш по Alt — только над лентой, без открытых окон.
        if !self.backstage.open
            && self.error.is_none()
            && self.ask.is_none()
            && !self.font_dialog.open
            && !self.paragraph_dialog.open
            && !self.link_dialog.open
            && !self.bookmark_dialog.open
            && !self.control_dialog.open
            && !self.recipients_dialog.open
            && !self.rule_dialog.open
            && !self.page_number_dialog.open
            && !self.password_dialog.open
            && !self.symbol_dialog.open
            && (self.ribbon.keytips.active() || !ctx.egui_wants_keyboard_input())
        {
            let commands = self.ribbon.handle_keys(&ctx);
            self.run_ribbon(&ctx, commands);
        }
        self.handle_input(&ctx);
        if self.applied_theme != Some(self.settings.theme) {
            ctx.set_theme(self.settings.theme.preference());
            self.applied_theme = Some(self.settings.theme);
        }

        if self.backstage.open {
            // «Файл» занимает всё окно вместо ленты и документа.
            if self.backstage.page == Page::Print {
                // Колонтитул закрывается: печатается и виден весь документ.
                if let Some(document) = &mut self.document {
                    document.end_story(&mut self.shaper, &self.library);
                }
                if self.printers.is_none() {
                    self.printers = Some(crate::print::PrinterList::start());
                }
                if self.printers.as_ref().is_some_and(|p| p.get().is_none()) {
                    ctx.request_repaint_after(Duration::from_millis(200));
                }
            }
            let info = self.backstage_info();
            let (library, painter, document) = (&self.library, &mut self.painter, &self.document);
            let mut preview = |p: &egui::Painter, index: usize, rect: egui::Rect| {
                if let Some(page) = document.as_ref().and_then(|d| d.layout.pages.get(index)) {
                    painter.paint_page(p, library, page, rect);
                }
            };
            let commands = egui::CentralPanel::no_frame()
                .show(ui, |ui| self.backstage.ui(ui, &info, &mut preview))
                .inner;
            self.run_ribbon(&ctx, commands);
        } else {
            if self.view_mode == ViewMode::Read {
                // В режиме чтения ленты нет: только имя и выход.
                let title = self.title();
                let close = egui::Panel::top("read-bar")
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(title);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.button(tr("read-close")).clicked()
                            })
                            .inner
                        })
                        .inner
                    })
                    .inner;
                if close {
                    self.set_view_mode(ViewMode::Print);
                }
            } else {
                let info = self.ribbon_info();
                let commands = egui::Panel::top("ribbon")
                    .show(ui, |ui| self.ribbon.ui(ui, &info))
                    .inner;
                self.run_ribbon(&ctx, commands);
            }
            if let Some(action) = self.find.ui(&ctx) {
                self.find_action(&ctx, action);
            }
            let (library, shaper, painter) = (&self.library, &mut self.shaper, &mut self.painter);
            let changes = self
                .font_dialog
                .ui(&ctx, &self.font_names, |p, rect, props| {
                    paint_font_sample(p, rect, props, library, shaper, painter);
                });
            if let Some(changes) = changes {
                let format = RunFormat::Set(Box::new(changes));
                self.run_ribbon(&ctx, vec![RibbonCommand::Format(format)]);
            }
            if let Some(changes) = self.paragraph_dialog.ui(&ctx) {
                let format = letter_editor::ParagraphFormat::Set(changes);
                self.run_ribbon(&ctx, vec![RibbonCommand::Paragraph(format)]);
            }
            if let Some(action) = self.link_dialog.ui(&ctx) {
                let command = match action {
                    LinkAction::Apply { link, text } => Command::Link {
                        link: Some(Box::new(link)),
                        text,
                    },
                    LinkAction::Remove => Command::Link {
                        link: None,
                        text: None,
                    },
                };
                self.run_ribbon(&ctx, vec![RibbonCommand::Edit(command)]);
            }
            if let Some(action) = self.bookmark_dialog.ui(&ctx) {
                self.bookmark_action(&ctx, action);
            }
            if let Some(data) = self.recipients_dialog.ui(&ctx) {
                let commands = self.set_recipients(data).map(RibbonCommand::Edit);
                self.run_ribbon(&ctx, commands.into_iter().collect());
            }
            if let Some(rule) = self.rule_dialog.ui(&ctx) {
                self.run_ribbon(&ctx, vec![RibbonCommand::InsertMergeRule(rule)]);
            }
            if let Some((at, control)) = self.control_dialog.ui(&ctx) {
                // Свойства меняются у элемента, для которого открыто окно.
                if let Some(document) = &mut self.document {
                    document.anchor = at;
                    document.cursor = at;
                }
                let command = Command::SetControl(Box::new(control));
                self.run_ribbon(&ctx, vec![RibbonCommand::Edit(command)]);
            }
            if let Some((page, numbering)) = self.page_number_dialog.ui(&ctx) {
                self.set_page_numbering(page, numbering);
            }
            if let Some(action) = self.protect_dialog.ui(&ctx) {
                self.protect_action(action);
            }
            if let Some(answer) = self.password_dialog.ui(&ctx) {
                self.password_answer(answer);
            }
            if let Some(c) = self.symbol_dialog.ui(&ctx) {
                crate::symbols::remember(&ctx, c);
                self.run_ribbon(&ctx, vec![RibbonCommand::InsertSymbol(c)]);
            }
            egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
            self.protection_bar(ui);
            if self.navigation.open && self.document.is_some() && self.view_mode != ViewMode::Read {
                let info = self.navigation_info();
                let (library, painter, document) =
                    (&self.library, &mut self.painter, &self.document);
                let mut thumbnail = |p: &egui::Painter, index: usize, rect: egui::Rect| {
                    if let Some(page) = document.as_ref().and_then(|d| d.layout.pages.get(index)) {
                        painter.paint_page(p, library, page, rect);
                    }
                };
                let navigation = &mut self.navigation;
                let action = egui::Panel::left("navigation")
                    .default_size(260.0)
                    .size_range(180.0..=500.0)
                    .show(ui, |ui| navigation.ui(ui, &info, &mut thumbnail))
                    .inner;
                if let Some(action) = action {
                    self.navigation_action(action);
                }
            }
            if self.comments.open && self.document.is_some() && self.view_mode != ViewMode::Read {
                let info = self.comments_info();
                let comments = &mut self.comments;
                let action = egui::Panel::right("comments")
                    .default_size(280.0)
                    .size_range(200.0..=500.0)
                    .show(ui, |ui| comments.ui(ui, &info))
                    .inner;
                if let Some(action) = action {
                    self.comment_action(&ctx, action);
                }
            }
            egui::CentralPanel::no_frame()
                .frame(egui::Frame::new().fill(Palette::of(&ctx).desk))
                .show(ui, |ui| self.document_area(ui));
        }
        self.statistics_window(&ctx);
        self.error_window(&ctx);
        self.save_changes_window(&ctx);
        self.recovery_window(&ctx);
        self.autosave_tick(&ctx);
        if self.close_confirmed {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }

        if self.title_changed {
            self.title_changed = false;
            ctx.send_viewport_cmd(ViewportCommand::Title(self.title()));
        }
        if std::mem::take(&mut self.open_requested) {
            self.request(Intent::Open(None));
        }
        if std::mem::take(&mut self.picture_requested) {
            self.pick_picture(&ctx);
        }
    }

    /// Заголовок окна: «имя.docx — letter».
    pub fn title(&self) -> String {
        match &self.document {
            Some(document) => format!("{} — {APP_NAME}", document.name()),
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
        // В режиме чтения два листа рядом всегда целиком в окне.
        if self.view_mode == ViewMode::Read {
            let first = self.document.as_ref().and_then(|d| d.layout.pages.first());
            return first.map_or(1.0, |p| {
                read_zoom((p.width, p.height), Vec2::new(view_width, self.view_size.y))
            });
        }
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

    /// Клавиши режима чтения: текст не правится, стрелки, PageUp/PageDown
    /// и пробел листают по два листа, Esc выходит.
    fn read_keys(&mut self, ctx: &Context) {
        let pages = self.page_count();
        let mut target = None;
        let mut close = false;
        let current = self.current_page - self.current_page % 2;
        ctx.input_mut(|i| {
            i.events.retain(|event| {
                let egui::Event::Key {
                    key, pressed: true, ..
                } = event
                else {
                    // Набранный текст в режиме чтения не нужен.
                    return !matches!(event, egui::Event::Text(_) | egui::Event::Paste(_));
                };
                match key {
                    Key::Escape => close = true,
                    Key::ArrowRight | Key::ArrowDown | Key::PageDown | Key::Space => {
                        target = Some((current + 2).min(pages.saturating_sub(1)));
                    }
                    Key::ArrowLeft | Key::ArrowUp | Key::PageUp => {
                        target = Some(current.saturating_sub(2));
                    }
                    Key::Home => target = Some(0),
                    Key::End => target = Some(pages.saturating_sub(1)),
                    _ => return true,
                }
                false
            });
        });
        if close {
            self.set_view_mode(ViewMode::Print);
        } else if let Some(page) = target {
            self.scroll_to = Some((page - page % 2, 0.0));
        }
    }

    /// Включает режим просмотра `mode`.
    fn set_view_mode(&mut self, mode: ViewMode) {
        if self.view_mode == mode {
            return;
        }
        self.view_mode = mode;
        // Колонтитулы правятся только в разметке страницы.
        if mode != ViewMode::Print
            && let Some(document) = &mut self.document
        {
            document.end_story(&mut self.shaper, &self.library);
        }
        self.reveal_cursor = true;
    }

    /// Верстает документ заново, если режим просмотра или ширина окна
    /// (в режимах без листов) изменились. `view_width` — ширина области
    /// листов, точки.
    fn sync_layout_view(&mut self, view_width: f32, zoom: f32) {
        // Поля вокруг листа и место под полосу прокрутки.
        let free = view_width - 2.0 * PAGE_GAP - 16.0;
        let width = (free / (zoom * SCREEN_PIXELS_PER_POINT)).floor();
        let wanted = match self.view_mode {
            ViewMode::Web => LayoutView::Web { width },
            ViewMode::Outline => LayoutView::Outline { width },
            ViewMode::Print | ViewMode::Read | ViewMode::Draft => LayoutView::Pages,
        };
        let Some(document) = &mut self.document else {
            return;
        };
        if document.story.is_none() && document.view != wanted {
            document.view = wanted;
            document.relayout(&mut self.shaper, &self.library);
        }
        if matches!(document.view, LayoutView::Outline { .. }) {
            let forward = document.cursor.paragraph > self.outline_cursor;
            document.leave_hidden_towards(forward);
            self.outline_cursor = document.cursor.paragraph;
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
        // Общие сочетания (Ctrl+N, Ctrl+S…) работают всегда, кроме
        // окон с вопросом.
        if self.error.is_none() && self.ask.is_none() {
            let table = shortcuts::table();
            let commands = take_shortcuts(ctx, &table, true);
            self.run_ribbon(ctx, commands);
        }
        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .find(|p| !p.as_os_str().is_empty())
        });
        if let Some(path) = dropped {
            self.request(Intent::Open(Some(path)));
        }
        if self.error.is_none() && self.ask.is_none() && !self.backstage.open {
            self.keyboard(ctx);
        }
    }

    /// Клавиатура: стрелки, Home/End, PageUp/PageDown двигают курсор,
    /// набранный текст и Enter, Backspace, Delete, Tab правят документ.
    /// События разбираются по порядку: «ab←c» даёт «acb».
    fn keyboard(&mut self, ctx: &Context) {
        let points = 1.0 / (self.zoom() * SCREEN_PIXELS_PER_POINT);
        let screen = self.view_size.y * points;
        // Клавиши достаются полю ввода (например, размер шрифта на ленте).
        if self.document.is_none() || ctx.egui_wants_keyboard_input() {
            return;
        }
        if self.view_mode == ViewMode::Read {
            self.read_keys(ctx);
            return;
        }
        // Esc кончает правку колонтитула.
        if self.document.as_ref().is_some_and(|d| d.story.is_some())
            && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
            && let Some(document) = &mut self.document
        {
            document.end_story(&mut self.shaper, &self.library);
            self.reveal_cursor = true;
            return;
        }
        // Сочетания Word и обычные клавиши — по порядку нажатия: «Ctrl+B,
        // текст» печатает текст полужирным.
        let table = shortcuts::table();
        let steps = ctx.input_mut(|i| {
            let mut steps = Vec::new();
            i.events.retain(|event| {
                let step = match event {
                    egui::Event::Key {
                        key,
                        physical_key,
                        pressed: true,
                        modifiers,
                        ..
                    } => shortcuts::find(&table, *key, *physical_key, *modifiers, false)
                        .map(Step::Command),
                    _ => None,
                }
                .or_else(|| key_action(event, screen).map(Step::Action));
                match step {
                    Some(step) => {
                        steps.push(step);
                        false
                    }
                    None => true,
                }
            });
            steps
        });
        // Tab в egui переводит фокус на следующий элемент окна (кнопку
        // ленты), а у нас он правит документ: фокус остаётся в тексте.
        if steps
            .iter()
            .any(|s| matches!(s, Step::Action(Action::Edit(Command::Tab { .. }))))
        {
            ctx.memory_mut(|m| {
                m.move_focus(egui::FocusDirection::None);
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
        }
        let mut actions = Vec::new();
        for step in steps {
            match step {
                Step::Action(action) => actions.push(action),
                Step::Command(command) => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    self.run_ribbon(ctx, vec![command]);
                }
            }
        }
        self.apply_actions(ctx, actions);
    }

    /// Выполняет действия с клавиатуры и ленты по порядку.
    fn apply_actions(&mut self, ctx: &Context, actions: Vec<Action>) {
        if actions.is_empty() {
            return;
        }
        let Some(document) = &mut self.document else {
            return;
        };
        // Вёрстка устаревает после правки; обновляем её перед следующим
        // движением курсора и в конце.
        let mut stale = false;
        let locked = document.locked();
        document
            .history
            .set_placeholders(crate::developer::placeholder_texts());
        document.history.set_design_mode(self.design_mode);
        for action in actions {
            // В защищённом документе можно двигать курсор, выделять
            // и копировать, но не править.
            if locked
                && matches!(
                    action,
                    Action::Cut | Action::Paste(..) | Action::Edit(_) | Action::Undo | Action::Redo
                )
            {
                continue;
            }
            match action.clone() {
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
                    document.history.seal();
                }
                Action::SelectAll => {
                    if std::mem::take(&mut stale) {
                        document.relayout(&mut self.shaper, &self.library);
                    }
                    document.set_selection(select_all(&document.layout));
                    document.history.seal();
                }
                Action::Copy | Action::Cut | Action::Paste(..) => {
                    let selection = document.selection();
                    let command = match action {
                        Action::Paste(text, mode) => self.clipboard.paste_as(&text, mode),
                        _ if selection.is_empty() => continue,
                        _ => {
                            let (start, end) = (selection.start(), selection.end());
                            self.clipboard.copy(copy_range(&document.doc, start, end));
                            if action == Action::Copy {
                                continue;
                            }
                            Command::DeleteBackward
                        }
                    };
                    let selection = document.history.run(&mut document.doc, selection, command);
                    document.set_selection(selection);
                    stale = true;
                }
                Action::Edit(command) => {
                    // В колонтитуле нет разрывов и разделов; новые рисунки
                    // в колонтитул пока не вставляются.
                    if document.story.is_some()
                        && matches!(
                            command,
                            Command::Break(_)
                                | Command::SectionBreak(_)
                                | Command::PageSetup(_)
                                | Command::PageColor(_)
                                | Command::Watermark(_)
                                | Command::AutoHyphenation(_)
                                | Command::InsertCoverPage(_)
                                | Command::BlankPage
                                | Command::HeaderFooter { .. }
                                | Command::InsertPicture(_)
                        )
                    {
                        continue;
                    }
                    let selection = document.selection();
                    let selection = document.history.run(&mut document.doc, selection, command);
                    document.set_selection(selection);
                    stale = true;
                }
                Action::Undo | Action::Redo => {
                    let history = &mut document.history;
                    let restored = if action == Action::Undo {
                        history.undo(&mut document.doc)
                    } else {
                        history.redo(&mut document.doc)
                    };
                    if let Some(selection) = restored {
                        document.set_selection(selection);
                        stale = true;
                    }
                }
            }
        }
        if stale {
            document.relayout(&mut self.shaper, &self.library);
        }
        document.cursor_since = ctx.input(|i| i.time);
        self.reveal_cursor = true;
    }

    /// Страница, чьи колонтитулы открывать: где открыт колонтитул, иначе
    /// страница с курсором.
    fn story_page(&self) -> usize {
        let Some(document) = &self.document else {
            return 0;
        };
        match &document.story {
            Some(story) => story.page,
            None => caret(&document.layout, document.cursor).map_or(self.current_page, |c| c.page),
        }
    }

    /// Нумерация страниц раздела страницы `page` из окна «Формат номеров
    /// страниц».
    fn set_page_numbering(&mut self, page: usize, numbering: Option<letter_model::PageNumbering>) {
        if let Some(document) = &mut self.document {
            let command = |section| Command::PageNumbering { section, numbering };
            document.section_command(page, command, &mut self.shaper, &self.library);
        }
    }

    /// «Номер страницы»: вверху или внизу страницы — колонтитул
    /// открывается и в нём остаётся один номер по центру; «текущее
    /// положение» — номер у курсора в открытом колонтитуле.
    fn insert_page_number(&mut self, ctx: &Context, place: crate::headers::PageNumberPlace) {
        use crate::headers::PageNumberPlace;
        let page = self.story_page();
        let Some(document) = &mut self.document else {
            return;
        };
        let fragment = |paragraph: Paragraph| {
            let mut doc = Document::new();
            doc.sections[0].blocks = vec![paragraph.into()];
            Box::new(doc)
        };
        let number = Paragraph::from_runs(letter_editor::page_number_runs());
        let mut actions = Vec::new();
        match place {
            PageNumberPlace::Top | PageNumberPlace::Bottom => {
                let header = place == PageNumberPlace::Top;
                document.begin_story(page, header, &mut self.shaper, &self.library);
                actions.push(Action::SelectAll);
                actions.push(Action::Edit(Command::Paste(fragment(number))));
                actions.push(Action::Edit(Command::Align(Alignment::Center)));
            }
            PageNumberPlace::Here if document.story.is_some() => {
                actions.push(Action::Edit(Command::Paste(fragment(number))));
            }
            PageNumberPlace::Here => {}
        }
        self.apply_actions(ctx, actions);
    }

    /// Ответ окна «Закладка»: добавить или удалить закладку (шаг
    /// отмены) или выделить её.
    fn bookmark_action(&mut self, ctx: &Context, action: BookmarkAction) {
        let command = match action {
            BookmarkAction::Add(name) => Command::AddBookmark(name),
            BookmarkAction::Delete(name) => Command::DeleteBookmark(name),
            BookmarkAction::GoTo(name) => {
                if let Some(document) = &mut self.document
                    && let Some((start, end)) = bookmark_range(&document.doc, &name)
                {
                    document.anchor = start;
                    document.cursor = end;
                    document.goal_x = None;
                    document.history.seal();
                    self.reveal_cursor = true;
                }
                return;
            }
        };
        self.run_ribbon(ctx, vec![RibbonCommand::Edit(command)]);
    }

    /// Окно «Ссылка»: с текстом выделенного (или ссылки у курсора —
    /// тогда она выделяется целиком) и закладками документа.
    /// Выполняет ответ окна «Ограничить редактирование»: проверяет
    /// пароль и меняет защиту (шагом, который можно отменить).
    fn protect_action(&mut self, action: ProtectAction) {
        use letter_editor::protection::{can_modify, can_unprotect, new_password, protect};
        let Some(document) = &mut self.document else {
            return;
        };
        document.end_story(&mut self.shaper, &self.library);
        let settings = &document.doc.settings;
        let command = match action {
            ProtectAction::Protect { edit, password } => {
                // «Запись исправлений» включает запись.
                if edit == letter_model::EditRestriction::TrackedChanges {
                    let selection = document.selection();
                    document
                        .history
                        .run(&mut document.doc, selection, Command::TrackChanges(true));
                }
                Command::Protection(Some(protect(edit, &password)))
            }
            ProtectAction::Unprotect(password) => match settings.protection.as_ref() {
                Some(p) if !can_unprotect(p, &password) => {
                    self.protect_dialog.fail();
                    return;
                }
                _ => Command::Protection(None),
            },
            ProtectAction::WriteProtect {
                password,
                recommended,
            } => Command::WriteProtection((recommended || !password.is_empty()).then(|| {
                letter_model::WriteProtection {
                    recommended,
                    password: (!password.is_empty()).then(|| new_password(&password)),
                    other: Vec::new(),
                }
            })),
            ProtectAction::AllowEditing(password) => {
                if settings
                    .write_protection
                    .as_ref()
                    .is_some_and(|w| !can_modify(w, &password))
                {
                    self.protect_dialog.fail();
                } else {
                    document.read_only = false;
                }
                return;
            }
        };
        let selection = document.selection();
        let selection = document.history.run(&mut document.doc, selection, command);
        document.set_selection(selection);
        document.relayout(&mut self.shaper, &self.library);
    }

    /// Полоса над документом, если править его нельзя, с кнопкой, которая
    /// снимает защиту.
    fn protection_bar(&mut self, ui: &mut egui::Ui) {
        let Some(document) = &self.document else {
            return;
        };
        if !document.locked() {
            return;
        }
        let read_only = document.read_only;
        let clicked = egui::Panel::top("protection-bar")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (text, button) = if read_only {
                        ("protect-bar-read-only", "protect-allow-editing")
                    } else {
                        ("protect-bar-locked", "protect-stop")
                    };
                    ui.label(tr(text));
                    ui.button(tr(button)).clicked()
                })
                .inner
            })
            .inner;
        if clicked {
            let mode = if read_only {
                ProtectMode::AllowEditing
            } else {
                ProtectMode::Unprotect
            };
            self.protect_dialog.show(mode, false);
        }
    }

    fn open_link_dialog(&mut self) {
        let Some(document) = &mut self.document else {
            return;
        };
        let selection = document.selection();
        let (start, end) = (selection.start(), selection.end());
        let existing = link_at(&document.doc, start);
        let text_of = |from: Position, to: Position| {
            let piece = copy_range(&document.doc, from, to);
            piece
                .blocks()
                .map(letter_model::Block::plain_text)
                .collect::<Vec<_>>()
                .join("")
        };
        let text = match &existing {
            Some((_, from, to)) if selection.is_empty() => {
                let text = text_of(*from, *to);
                document.anchor = *from;
                document.cursor = *to;
                Some(text)
            }
            _ if selection.is_empty() => Some(String::new()),
            _ if start.paragraph == end.paragraph => Some(text_of(start, end)),
            _ => None,
        };
        let names = bookmarks(&document.doc)
            .into_iter()
            .map(|(name, _)| name)
            .filter(|name| !name.starts_with('_'))
            .collect();
        self.link_dialog
            .show(text, existing.map(|(link, _, _)| link), names);
    }

    /// «Вставка → Рисунки»: окно выбора картинки, потом вставка.
    fn pick_picture(&mut self, ctx: &Context) {
        if self.document.is_none() {
            return;
        }
        let mut dialog = rfd::FileDialog::new()
            .set_title(tr("insert-picture"))
            .add_filter(tr("filter-pictures"), crate::picture::PICTURE_EXTENSIONS)
            .add_filter(tr("filter-all-files"), &["*"]);
        if let Some(dir) = self.dialog_dir() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.insert_picture_file(ctx, &path);
        }
    }

    /// Вставляет у курсора новую фигуру `name` и выделяет её: появляется
    /// вкладка «Формат фигуры».
    fn insert_shape(&mut self, ctx: &Context, name: &str) {
        let picture = letter_editor::new_shape(name);
        self.apply_actions(
            ctx,
            vec![Action::Edit(Command::InsertPicture(Box::new(picture)))],
        );
        if let Some(document) = &mut self.document {
            let end = document.cursor;
            let start = Position::new(
                end.paragraph,
                end.offset
                    .saturating_sub(letter_model::OBJECT_CHAR.len_utf8()),
            );
            if letter_editor::shape_ref(&document.doc, start).is_some() {
                document.set_selection(Selection::new(start, end));
                self.ribbon.tab = crate::ribbon::Tab::ShapeFormat;
            }
        }
    }

    /// Вставляет у курсора пустую формулу, выделяет её и открывает
    /// вкладку «Уравнение» с курсором в поле линейного вида.
    fn insert_equation(&mut self, ctx: &Context) {
        let Some(document) = &self.document else {
            return;
        };
        let at = document.selection().start();
        let math = letter_editor::new_math(&document.doc, at, "");
        self.apply_actions(ctx, vec![Action::Edit(Command::InsertMath(Box::new(math)))]);
        if let Some(document) = &mut self.document {
            let end = document.cursor;
            let start = Position::new(
                end.paragraph,
                end.offset
                    .saturating_sub(letter_model::OBJECT_CHAR.len_utf8()),
            );
            if letter_editor::math_ref(&document.doc, start).is_some() {
                document.set_selection(Selection::new(start, end));
                self.ribbon.tab = crate::ribbon::Tab::Equation;
                ctx.memory_mut(|m| m.request_focus(crate::equation::field_id()));
            }
        }
    }

    /// Вставляет рисунок из файла картинки у курсора.
    fn insert_picture_file(&mut self, ctx: &Context, path: &Path) {
        match crate::picture::picture_from_file(path) {
            Ok(picture) => {
                let command = Command::InsertPicture(Box::new(picture));
                self.apply_actions(ctx, vec![Action::Edit(command)]);
            }
            Err(message) => self.report("error-picture", message),
        }
    }

    /// Системное окно выбора файла.
    fn pick_file(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title(tr("open-document"));
        for (name, extensions) in OPEN_FILTERS {
            dialog = dialog.add_filter(tr(name), extensions);
        }
        dialog = dialog.add_filter(tr("filter-all-files"), &["*"]);
        if let Some(dir) = self.dialog_dir() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.open_or_report(&path);
        }
    }

    /// Что показать на ленте: состояние документа и оформление у курсора.
    /// Выполняет просьбу окна «Найти и заменить».
    fn find_action(&mut self, ctx: &Context, action: FindAction) {
        let Some(document) = &mut self.document else {
            return;
        };
        let find = &mut self.find;
        let (query, options) = (find.query.clone(), find.options);
        let selection = document.selection();
        let found = find_all(&document.doc, &query, options);
        let mut actions = Vec::new();
        match action {
            FindAction::ReplaceAll => {
                find.message = Some(if found.is_empty() {
                    tr("find-nothing").to_owned()
                } else {
                    tr_args("replaced-count", &[("n", found.len().into())])
                });
                if !found.is_empty() {
                    document.history.seal();
                    actions.push(Action::Edit(Command::ReplaceAll {
                        query,
                        replacement: find.replacement.clone(),
                        options,
                    }));
                }
                self.apply_actions(ctx, actions);
                return;
            }
            FindAction::Replace => {
                // Заменяется только выделенное найденное, как в Word.
                if found.contains(&(selection.start(), selection.end())) && !selection.is_empty() {
                    document.history.seal();
                    actions.push(Action::Edit(if find.replacement.is_empty() {
                        Command::DeleteBackward
                    } else {
                        Command::Type(find.replacement.clone())
                    }));
                }
            }
            FindAction::Next => {}
        }
        self.apply_actions(ctx, actions);
        let Some(document) = &mut self.document else {
            return;
        };
        document.history.seal();
        let from = document.selection().end();
        match find_next(&document.doc, from, &self.find.query, self.find.options) {
            Some((start, end)) => {
                document.set_selection(Selection::new(start, end));
                self.find.message = None;
                self.reveal_cursor = true;
            }
            None => self.find.message = Some(tr("find-nothing").to_owned()),
        }
    }

    /// Что показать в области навигации. Пока открыт колонтитул —
    /// только листы.
    fn navigation_info(&mut self) -> crate::navigation::NavInfo {
        use crate::navigation::{NavHeading, NavInfo, NavTab};
        let Some(document) = &self.document else {
            return NavInfo::default();
        };
        let pages = match &document.story {
            Some(story) => &story.display.pages,
            None => &document.layout.pages,
        };
        let mut info = NavInfo {
            pages: pages.iter().map(|p| (p.width, p.height)).collect(),
            current_page: self.current_page,
            ..NavInfo::default()
        };
        if document.story.is_some() {
            return info;
        }
        if self.navigation.tab == NavTab::Headings {
            let outline = letter_editor::document_outline(&document.doc, &document.layout);
            info.current_heading = outline
                .iter()
                .rposition(|h| h.paragraph <= document.cursor.paragraph);
            info.headings = outline
                .into_iter()
                .map(|h| NavHeading {
                    level: h.level,
                    title: h.title,
                })
                .collect();
        }
        let query = &self.navigation.query;
        if !query.is_empty() {
            let state = document.history.state();
            let stale = self
                .nav_found
                .as_ref()
                .is_none_or(|(q, s, _)| q != query || *s != state);
            if stale {
                let found = find_all(&document.doc, query, Default::default());
                self.nav_found = Some((query.clone(), state, found));
            }
            if let Some((_, _, found)) = &self.nav_found {
                let selection = document.selection();
                info.current_result = found
                    .iter()
                    .position(|f| (f.0, f.1) == (selection.start(), selection.end()));
                if self.navigation.tab == NavTab::Results {
                    info.results = found
                        .iter()
                        .map(|&f| letter_editor::found_context(&document.doc, f, 30))
                        .collect();
                }
            }
        }
        info
    }

    /// Ветка примечаний, в тексте которой курсор (номер первого
    /// примечания ветки); из вложенных — самая внутренняя.
    fn current_comment(&self) -> Option<String> {
        let document = self.document.as_ref()?;
        let at = (document.cursor.paragraph, document.cursor.offset);
        let doc = &document.doc;
        letter_editor::comment_places(doc)
            .into_iter()
            .filter(|p| {
                (p.start.paragraph, p.start.offset) <= at && at <= (p.end.paragraph, p.end.offset)
            })
            .filter_map(|p| letter_editor::thread_root(doc, &p.id))
            .next_back()
    }

    /// Что показать в области примечаний: ветки по порядку в тексте.
    fn comments_info(&self) -> crate::comments::CommentsInfo {
        use crate::comments::{CommentEntry, CommentThread, CommentsInfo};
        let Some(document) = &self.document else {
            return CommentsInfo::default();
        };
        let doc = &document.doc;
        let entry = |c: &letter_model::Comment| CommentEntry {
            id: c.id.clone(),
            author: c.author.clone(),
            date: c.date.as_deref().map(comment_date).unwrap_or_default(),
            text: c.plain_text(),
        };
        let mut threads = Vec::new();
        for place in letter_editor::comment_places(doc) {
            let Some(comment) = doc
                .comments
                .iter()
                .find(|c| c.id == place.id && c.parent.is_none())
            else {
                continue;
            };
            threads.push(CommentThread {
                comment: entry(comment),
                replies: doc
                    .comments
                    .iter()
                    .filter(|c| c.parent.is_some())
                    .filter(|c| {
                        letter_editor::thread_root(doc, &c.id).as_ref() == Some(&comment.id)
                    })
                    .map(entry)
                    .collect(),
                done: comment.done,
            });
        }
        CommentsInfo {
            threads,
            current: self.current_comment(),
        }
    }

    /// Новое примечание (или ответ) с текстом `text` от имени
    /// пользователя, с текущим временем.
    fn new_comment(&self, text: &str) -> Option<letter_model::Comment> {
        let document = self.document.as_ref()?;
        let author = std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .ok()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| tr("comment-author-default").to_owned());
        let initials: String = author
            .split_whitespace()
            .filter_map(|w| w.chars().next())
            .flat_map(char::to_uppercase)
            .collect();
        Some(letter_model::Comment {
            initials: Some(initials),
            author,
            date: Some(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
            blocks: letter_editor::comment_blocks(&document.doc, text),
            ..Default::default()
        })
    }

    /// Выполняет выбранное в области примечаний.
    fn comment_action(&mut self, ctx: &Context, action: crate::comments::CommentAction) {
        use crate::comments::CommentAction;
        if let Some(document) = &mut self.document {
            document.end_story(&mut self.shaper, &self.library);
        }
        let command = match action {
            CommentAction::Close => {
                self.comments.open = false;
                return;
            }
            CommentAction::Select(id) => {
                self.select_comment(&id);
                return;
            }
            CommentAction::Add { at, text } => {
                let Some(comment) = self.new_comment(&text) else {
                    return;
                };
                if let Some(document) = &mut self.document {
                    document.set_selection(at);
                }
                Command::AddComment(Box::new(comment))
            }
            CommentAction::Reply { parent, text } => {
                let Some(reply) = self.new_comment(&text) else {
                    return;
                };
                Command::ReplyComment {
                    parent,
                    reply: Box::new(reply),
                }
            }
            CommentAction::Edit { id, text } => {
                let Some(document) = &self.document else {
                    return;
                };
                Command::SetComment {
                    id,
                    blocks: letter_editor::comment_blocks(&document.doc, &text),
                }
            }
            CommentAction::Done { id, done } => Command::CommentDone { id, done },
            CommentAction::Delete(id) => Command::DeleteComment(id),
        };
        self.run_ribbon(ctx, vec![RibbonCommand::Edit(command)]);
    }

    /// Выделяет текст примечания `id` и прокручивает к нему.
    fn select_comment(&mut self, id: &str) {
        let Some(document) = &mut self.document else {
            return;
        };
        if let Some(place) = letter_editor::comment_places(&document.doc)
            .into_iter()
            .find(|p| p.id == id)
        {
            document.set_selection(Selection::new(place.start, place.end));
            self.reveal_cursor = true;
        }
    }

    /// «Далее» (`forward`) или «Назад»: к следующему примечанию после
    /// курсора или предыдущему до него, по кругу.
    fn next_comment(&mut self, forward: bool) {
        let Some(document) = &self.document else {
            return;
        };
        let doc = &document.doc;
        let at = (document.cursor.paragraph, document.cursor.offset);
        let starts: Vec<(String, (usize, usize))> = letter_editor::comment_places(doc)
            .into_iter()
            .filter(|p| {
                doc.comments
                    .iter()
                    .any(|c| c.id == p.id && c.parent.is_none())
            })
            .map(|p| (p.id, (p.start.paragraph, p.start.offset)))
            .collect();
        let found = if forward {
            starts
                .iter()
                .find(|(_, s)| *s > at)
                .or_else(|| starts.first())
        } else {
            let anchor = (document.anchor.paragraph, document.anchor.offset).min(at);
            starts
                .iter()
                .rev()
                .find(|(_, s)| *s < anchor)
                .or_else(|| starts.last())
        };
        if let Some((id, _)) = found.cloned() {
            self.comments.open = true;
            self.select_comment(&id);
        }
    }

    /// «Следующая сноска» (`forward`) или «Предыдущая»: выделяет ссылку
    /// на сноску вида `kind` после курсора или до него, по кругу.
    fn next_note(&mut self, kind: letter_model::NoteKind, forward: bool) {
        let Some(document) = &mut self.document else {
            return;
        };
        if document.story.is_some() {
            return;
        }
        let places = letter_editor::note_places(&document.doc, kind);
        let key = |p: Position| (p.paragraph, p.offset);
        let start = key(document.cursor).min(key(document.anchor));
        let found = if forward {
            places
                .iter()
                .find(|(p, _)| key(*p) > start)
                .or_else(|| places.first())
        } else {
            places
                .iter()
                .rev()
                .find(|(p, _)| key(*p) < start)
                .or_else(|| places.last())
        };
        if let Some(&(at, len)) = found {
            let end = Position::new(at.paragraph, at.offset + len);
            document.set_selection(Selection::new(at, end));
            self.reveal_cursor = true;
        }
    }

    /// Переход из области навигации.
    fn navigation_action(&mut self, action: crate::navigation::NavAction) {
        use crate::navigation::NavAction;
        if action == NavAction::Close {
            self.navigation.open = false;
            return;
        }
        if let NavAction::Page(page) = action {
            self.scroll_to = Some((page, 0.0));
            return;
        }
        let Some(document) = &mut self.document else {
            return;
        };
        document.end_story(&mut self.shaper, &self.library);
        match action {
            NavAction::Heading(index) => {
                let outline = letter_editor::document_outline(&document.doc, &document.layout);
                if let Some(heading) = outline.get(index) {
                    let at = Position::new(heading.paragraph, 0);
                    document.set_selection(Selection::new(at, at));
                    self.scroll_to = Some((heading.page, heading.top - 12.0));
                }
            }
            NavAction::Result(index) => {
                if let Some(&(start, end)) = self
                    .nav_found
                    .as_ref()
                    .and_then(|(_, _, found)| found.get(index))
                {
                    document.set_selection(Selection::new(start, end));
                    self.reveal_cursor = true;
                }
            }
            NavAction::Page(_) | NavAction::Close => {}
        }
    }

    fn ribbon_info(&self) -> RibbonInfo {
        let mut info = RibbonInfo {
            page_width: self.zoom == Zoom::PageWidth,
            ruler: self.show_ruler,
            navigation: self.navigation.open,
            comments: self.comments.open,
            show_level: self
                .document
                .as_ref()
                .and_then(|d| d.outline_view.show_level),
            view_mode: self.view_mode,
            marks: self.show_marks,
            fonts: self.font_names.clone(),
            ..RibbonInfo::default()
        };
        if let Some(document) = &self.document {
            info.has_document = true;
            info.design_mode = self.design_mode;
            if let Some(mailing) = &self.mailing {
                info.mailing = crate::mailings::MailingInfo {
                    fields: mailing.data.headers.clone(),
                    records: mailing.data.records().len(),
                    record: mailing.record,
                    preview: mailing.preview,
                    labels: false,
                };
            }
            info.mailing.labels = letter_editor::has_next_records(&document.doc);
            info.in_control =
                letter_editor::control_range(&document.doc, document.cursor).is_some();
            info.outline_level =
                letter_editor::paragraph_ref(&document.doc, document.cursor.paragraph)
                    .and_then(|p| letter_editor::outline_level(&document.doc, &p.props));
            info.can_undo = document.history.can_undo();
            info.styles = crate::styles::gallery(&document.doc);
            info.can_redo = document.history.can_redo();
            info.has_selection = !document.selection().is_empty();
            if let Some((paragraph, run)) = formatting_at(&document.doc, document.cursor) {
                info.bold = run.bold;
                info.italic = run.italic;
                info.underline = run.underline != letter_model::Underline::None;
                info.strike = run.strike;
                info.subscript = run.vertical_align == letter_model::VerticalAlign::Subscript;
                info.superscript = run.vertical_align == letter_model::VerticalAlign::Superscript;
                info.font = run.fonts.ascii;
                info.size = run.size.0;
                info.alignment = Some(paragraph.alignment);
                info.style = paragraph.style.clone();
                info.line_spacing = Some(paragraph.line_spacing);
                info.space_before = paragraph.space_before.0;
                info.space_after = paragraph.space_after.0;
                info.indent_left = paragraph.indent_left.0;
                info.indent_right = paragraph.indent_right.0;
            }
            info.list = list_kind(&document.doc, document.cursor.paragraph);
            info.in_table = letter_editor::in_table(&document.doc, document.cursor.paragraph);
            let selection = document.selection();
            info.can_merge = letter_editor::can_merge(
                &document.doc,
                selection.start().paragraph,
                selection.end().paragraph,
            );
            let picture = selected_picture(&document.layout, selection)
                .and_then(|p| picture_ref(&document.doc, p.at).map(|picture| (p.at, picture)));
            info.picture_wrap = picture.map(|(_, p)| PictureWrap::of(p));
            info.picture_size = picture.map(|(_, p)| (p.width, p.height));
            info.shape = picture
                .and_then(|(at, _)| letter_editor::shape_ref(&document.doc, at))
                .cloned();
            info.equation = equation_at(&document.doc, selection).map(|(at, m)| (at, m.clone()));
            info.section =
                letter_editor::section_props(&document.doc, document.cursor.paragraph).cloned();
            info.auto_hyphenation = document.doc.settings.auto_hyphenation;
            let settings = &document.body().settings;
            info.track_changes = settings.track_revisions;
            info.track_changes_forced = settings.protection.as_ref().is_some_and(|p| {
                p.enforced && p.edit == letter_model::EditRestriction::TrackedChanges
            });
            info.has_revisions = letter_editor::revisions::has_revisions(document.body(), None);
            info.spelling = self.spelling.enabled;
            info.has_comments = !document.doc.comments.is_empty();
            info.in_comment = info.has_comments && self.current_comment().is_some();
            if let Some(story) = &document.story
                && let Some(header) = story.header()
            {
                info.story = Some(header);
                let section = story.display.pages.get(story.page).map(|p| p.section);
                info.title_page = section
                    .and_then(|s| story.body.sections.get(s))
                    .is_some_and(|s| s.props.title_page);
                info.even_odd = story.body.settings.even_and_odd_headers;
            }
        }
        info
    }

    /// Выполняет команды ленты.
    fn run_ribbon(&mut self, ctx: &Context, commands: Vec<RibbonCommand>) {
        let mut actions = Vec::new();
        for command in commands {
            match command {
                RibbonCommand::New => self.request(Intent::New),
                RibbonCommand::Open => self.open_requested = true,
                RibbonCommand::OpenRecent(path) => self.request(Intent::Open(Some(path))),
                RibbonCommand::Save => {
                    self.save();
                }
                RibbonCommand::SaveAs => {
                    self.save_as();
                }
                RibbonCommand::EncryptDialog => {
                    if let Some(document) = &self.document {
                        self.password_dialog
                            .ask_to_encrypt(document.password.as_deref());
                    }
                }
                RibbonCommand::Exit => ctx.send_viewport_cmd(ViewportCommand::Close),
                RibbonCommand::Backstage => {
                    let page = if self.document.is_some() {
                        Page::Info
                    } else {
                        Page::Open
                    };
                    self.backstage.show(page);
                }
                RibbonCommand::PrintPreview => {
                    if self.document.is_some() {
                        self.backstage.show(Page::Print);
                    }
                }
                RibbonCommand::Print(job) => {
                    self.print(&job);
                }
                RibbonCommand::CloseDocument => self.request(Intent::CloseDocument),
                RibbonCommand::SaveAsFormat(extension) => {
                    self.save_as_format(extension);
                }
                RibbonCommand::ChangeSettings(settings) => self.change_settings(settings),
                RibbonCommand::Undo => actions.push(Action::Undo),
                RibbonCommand::Redo => actions.push(Action::Redo),
                RibbonCommand::Cut => actions.push(Action::Cut),
                RibbonCommand::Copy => actions.push(Action::Copy),
                RibbonCommand::Paste => {
                    if let Some(text) = self.clipboard.text() {
                        actions.push(Action::Paste(text, PasteMode::KeepSource));
                    }
                }
                RibbonCommand::PasteSpecial(mode) => {
                    if let Some(text) = self.clipboard.text() {
                        actions.push(Action::Paste(text, mode));
                    }
                }
                RibbonCommand::Format(format) => {
                    actions.push(Action::Edit(Command::Format(format)));
                }
                RibbonCommand::Align(alignment) => {
                    actions.push(Action::Edit(Command::Align(alignment)));
                }
                RibbonCommand::ZoomIn => self.zoom_step(true),
                RibbonCommand::ZoomOut => self.zoom_step(false),
                RibbonCommand::ZoomReset => self.set_zoom(Zoom::Fixed(1.0), None),
                RibbonCommand::PageWidth => self.set_zoom(Zoom::PageWidth, None),
                RibbonCommand::ToggleRuler => self.show_ruler = !self.show_ruler,
                RibbonCommand::ToggleNavigation => self.navigation.open = !self.navigation.open,
                RibbonCommand::ToggleComments => self.comments.open = !self.comments.open,
                RibbonCommand::ShowLevel(level) => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    if let Some(document) = &mut self.document {
                        document.outline_view.show_level = level;
                        document.relayout(&mut self.shaper, &self.library);
                        document.leave_hidden();
                    }
                }
                RibbonCommand::OutlineExpand(expand) => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    if let Some(document) = &mut self.document {
                        document.expand_heading(expand);
                        document.relayout(&mut self.shaper, &self.library);
                        document.leave_hidden();
                    }
                }
                RibbonCommand::NextComment(forward) => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    self.next_comment(forward);
                }
                RibbonCommand::NextNote { kind, forward } => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    self.next_note(kind, forward);
                }
                RibbonCommand::NewComment => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    if let Some(document) = &mut self.document {
                        document.end_story(&mut self.shaper, &self.library);
                        let mut at = document.selection();
                        // Без выделения примечание — к слову у курсора.
                        if at.is_empty() {
                            at = word_at(&document.layout, document.cursor);
                        }
                        self.comments.start_new(at);
                    }
                }
                RibbonCommand::DeleteComment => {
                    if let Some(id) = self.current_comment() {
                        actions.push(Action::Edit(Command::DeleteComment(id)));
                    }
                }
                RibbonCommand::DeleteAllComments => {
                    actions.push(Action::Edit(Command::DeleteAllComments));
                }
                RibbonCommand::ViewMode(mode) => self.set_view_mode(mode),
                RibbonCommand::ToggleMarks => self.show_marks = !self.show_marks,
                RibbonCommand::Paragraph(format) => {
                    actions.push(Action::Edit(Command::Paragraph(format)));
                }
                RibbonCommand::List(kind) => actions.push(Action::Edit(Command::List(kind))),
                RibbonCommand::MultilevelList => {
                    actions.push(Action::Edit(Command::MultilevelList));
                }
                RibbonCommand::RestartNumbering => {
                    actions.push(Action::Edit(Command::RestartNumbering));
                }
                RibbonCommand::ContinueNumbering => {
                    actions.push(Action::Edit(Command::ContinueNumbering));
                }
                RibbonCommand::ToggleCheckBox => {
                    actions.push(Action::Edit(Command::ToggleCheckBox))
                }
                RibbonCommand::ChooseListItem(index) => {
                    actions.push(Action::Edit(Command::ChooseListItem(index)));
                }
                RibbonCommand::ControlToday => {
                    let (today, _) = crate::fields::now();
                    actions.push(Action::Edit(Command::ControlDate(today)));
                }
                RibbonCommand::InsertControl(kind) => {
                    let setup = letter_editor::ControlSetup {
                        kind,
                        placeholder: crate::developer::placeholder(kind).to_string(),
                        russian: crate::i18n::language() == crate::i18n::Language::Russian,
                    };
                    actions.push(Action::Edit(Command::InsertControl(Box::new(setup))));
                }
                RibbonCommand::ControlDate(date) => {
                    actions.push(Action::Edit(Command::ControlDate(date)));
                }
                RibbonCommand::DesignMode => self.design_mode = !self.design_mode,
                RibbonCommand::PickRecipients => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    for command in self.pick_recipients() {
                        actions.push(Action::Edit(command));
                    }
                }
                RibbonCommand::InsertMergeField(name) => {
                    actions.push(Action::Edit(Command::InsertMergeField(name)));
                    if let Some(mailing) = self.mailing.as_ref().filter(|m| m.preview) {
                        actions.push(Action::Edit(mailing.show()));
                    }
                }
                RibbonCommand::MergePreview(on) => {
                    if let Some(mailing) = &mut self.mailing {
                        mailing.preview = on;
                        actions.push(Action::Edit(mailing.show()));
                    }
                }
                RibbonCommand::MergeRecord(step) => {
                    if let Some(mailing) = &mut self.mailing {
                        let last = mailing.data.records().len().saturating_sub(1);
                        let record = mailing.record as i64 + i64::from(step);
                        mailing.record = record.clamp(0, last as i64) as usize;
                        if mailing.preview {
                            actions.push(Action::Edit(mailing.show()));
                        }
                    }
                }
                RibbonCommand::EditRecipients => {
                    if let Some(mailing) = &self.mailing {
                        self.recipients_dialog.show(&mailing.data);
                    }
                }
                RibbonCommand::RuleDialog(kind) => {
                    if let Some(mailing) = &self.mailing {
                        self.rule_dialog.show(kind, &mailing.data.headers);
                    }
                }
                RibbonCommand::InsertMergeRule(rule) => {
                    actions.push(Action::Edit(Command::InsertMergeRule(Box::new(rule))));
                    if let Some(mailing) = self.mailing.as_ref().filter(|m| m.preview) {
                        actions.push(Action::Edit(mailing.show()));
                    }
                }
                RibbonCommand::MakeLabels(index) => {
                    if let Some(spec) = letter_editor::LABELS.get(index) {
                        let command = Command::MakeLabels(Box::new(*spec));
                        actions.push(Action::Edit(command));
                    }
                }
                RibbonCommand::UpdateLabels => {
                    actions.push(Action::Edit(Command::UpdateLabels));
                    if let Some(mailing) = self.mailing.as_ref().filter(|m| m.preview) {
                        actions.push(Action::Edit(mailing.show()));
                    }
                }
                RibbonCommand::MergeToPdf => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    self.merge_to_pdf();
                }
                RibbonCommand::FinishMerge => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    self.request(Intent::Merge);
                }
                RibbonCommand::ControlProperties => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    if let Some(document) = &self.document
                        && let Some((control, _, _)) =
                            letter_editor::control_range(&document.doc, document.cursor)
                    {
                        self.control_dialog.show(document.cursor, &control);
                    }
                }
                RibbonCommand::Style(id) => actions.push(Action::Edit(Command::Style(id))),
                // В колонтитул сноску не вставить.
                RibbonCommand::Edit(Command::InsertNote(_))
                    if self.document.as_ref().is_some_and(|d| d.story.is_some()) => {}
                // Новая сноска сразу открывается для правки, курсор — после
                // её номера, как в Word.
                RibbonCommand::Edit(Command::InsertNote(kind)) => {
                    actions.push(Action::Edit(Command::InsertNote(kind)));
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    if let Some(document) = &mut self.document
                        && let Some(id) = letter_editor::notes(&document.doc, kind)
                            .iter()
                            .rev()
                            .find(|n| n.is_regular())
                            .map(|n| n.id.clone())
                    {
                        document.begin_note(kind, id, &mut self.shaper, &self.library);
                        let end = move_cursor(
                            &document.layout,
                            Position::default(),
                            Motion::DocumentEnd,
                            None,
                        );
                        document.cursor = end.position;
                        document.anchor = end.position;
                        document.cursor_since = ctx.input(|i| i.time);
                        self.reveal_cursor = true;
                    }
                }
                RibbonCommand::Edit(command) => actions.push(Action::Edit(command)),
                RibbonCommand::InsertPicture => self.picture_requested = true,
                RibbonCommand::InsertShape(name) => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    self.insert_shape(ctx, name);
                }
                RibbonCommand::InsertEquation => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    self.insert_equation(ctx);
                }
                RibbonCommand::SetEquation(at, math) => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    if let Some(document) = &mut self.document {
                        let end = Position::new(
                            at.paragraph,
                            at.offset + letter_model::OBJECT_CHAR.len_utf8(),
                        );
                        document.set_selection(Selection::new(at, end));
                    }
                    actions.push(Action::Edit(Command::SetMath(math)));
                }
                RibbonCommand::EditHeaderFooter { header } => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    let page = self.story_page();
                    if let Some(document) = &mut self.document {
                        document.begin_story(page, header, &mut self.shaper, &self.library);
                        document.cursor_since = ctx.input(|i| i.time);
                        self.reveal_cursor = true;
                    }
                }
                RibbonCommand::ToggleTrackChanges => {
                    if let Some(document) = &self.document
                        && document.story.is_none()
                    {
                        let settings = &document.doc.settings;
                        let forced = settings.protection.as_ref().is_some_and(|p| {
                            p.enforced && p.edit == letter_model::EditRestriction::TrackedChanges
                        });
                        if !forced {
                            let on = !settings.track_revisions;
                            actions.push(Action::Edit(Command::TrackChanges(on)));
                        }
                    }
                }
                RibbonCommand::NextRevision(forward) => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    if let Some(document) = &mut self.document
                        && document.story.is_none()
                        && let Some(at) = letter_editor::revisions::next_revision(
                            &document.doc,
                            document.cursor,
                            forward,
                        )
                    {
                        document.set_selection(Selection::caret(at));
                        document.history.seal();
                        self.reveal_cursor = true;
                    }
                }
                RibbonCommand::CompareDocuments => self.request(Intent::Compare),
                RibbonCommand::SpellReplace { start, end, text } => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    if let Some(document) = &mut self.document {
                        document.set_selection(Selection::new(start, end));
                        document.history.seal();
                    }
                    actions.push(Action::Edit(Command::Type(text)));
                }
                RibbonCommand::SpellIgnore(word) => self.spelling.ignore(&word),
                RibbonCommand::SpellAdd(word) => self.spelling.add_word(&word),
                RibbonCommand::ToggleSpelling => {
                    let on = !self.spelling.enabled;
                    self.spelling.set_enabled(on);
                }
                RibbonCommand::NextSpellingError => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    if let Some(document) = &mut self.document
                        && document.story.is_none()
                    {
                        let errors = self
                            .spelling
                            .errors(&document.doc, document.history.state());
                        let key = |p: Position| (p.paragraph, p.offset);
                        let here = key(document.selection().end());
                        let next = errors
                            .iter()
                            .find(|(a, _)| key(*a) >= here)
                            .or_else(|| errors.first());
                        if let Some(&(a, b)) = next {
                            document.set_selection(Selection::new(a, b));
                            document.history.seal();
                            self.reveal_cursor = true;
                        }
                    }
                }
                RibbonCommand::RestrictEditing => {
                    if let Some(document) = &self.document {
                        let settings = &document.body().settings;
                        let mode = if document.read_only {
                            ProtectMode::AllowEditing
                        } else if settings.protection.as_ref().is_some_and(|p| p.enforced) {
                            ProtectMode::Unprotect
                        } else {
                            ProtectMode::Protect
                        };
                        let recommended = settings
                            .write_protection
                            .as_ref()
                            .is_some_and(|w| w.recommended);
                        self.protect_dialog.show(mode, recommended);
                    }
                }
                RibbonCommand::InsertToc | RibbonCommand::UpdateToc => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    if let Some(document) = &self.document
                        && document.story.is_none()
                    {
                        let context = Box::new(document.field_context());
                        actions.push(Action::Edit(if command == RibbonCommand::InsertToc {
                            Command::InsertToc(context)
                        } else {
                            Command::UpdateToc(context)
                        }));
                    }
                }
                RibbonCommand::UpdateFields => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    if let Some(document) = &self.document {
                        let context = document.field_context();
                        let command = Command::UpdateFields(Box::new(context));
                        actions.push(Action::Edit(command));
                    }
                }
                RibbonCommand::InsertField(code) => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    let page = self.story_page();
                    if let Some(document) = &self.document {
                        let fragment = document.field_fragment(&code, page);
                        actions.push(Action::Edit(Command::Paste(fragment)));
                    }
                }
                RibbonCommand::CloseHeaderFooter => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    if let Some(document) = &mut self.document {
                        document.end_story(&mut self.shaper, &self.library);
                        self.reveal_cursor = true;
                    }
                }
                RibbonCommand::TitlePage(on) => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    let page = self.story_page();
                    if let Some(document) = &mut self.document {
                        let command = |section| Command::TitlePage { section, on };
                        document.section_command(page, command, &mut self.shaper, &self.library);
                    }
                }
                RibbonCommand::EvenOddHeaders(on) => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    let page = self.story_page();
                    if let Some(document) = &mut self.document {
                        let command = |_| Command::EvenOddHeaders(on);
                        document.section_command(page, command, &mut self.shaper, &self.library);
                    }
                }
                RibbonCommand::PageNumberFormat => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    let page = self.story_page();
                    if let Some(document) = &self.document
                        && let Some(section) = document.page_section(page)
                    {
                        let current = section.props.page_numbering.clone();
                        self.page_number_dialog.show(page, current);
                    }
                }
                RibbonCommand::PageNumber(place) => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    self.insert_page_number(ctx, place);
                }
                RibbonCommand::PictureWrap(wrap) => {
                    if let Some(document) = &self.document
                        && let Some(placed) =
                            selected_picture(&document.layout, document.selection())
                        && let Some(picture) = picture_ref(&document.doc, placed.at)
                    {
                        let anchor = anchor_with_wrap(&document.layout, &placed, picture, wrap);
                        actions.push(Action::Edit(Command::PictureAnchor(anchor)));
                    }
                }
                RibbonCommand::BuiltInStyle(name) => {
                    if let Some(id) = self
                        .document
                        .as_ref()
                        .and_then(|d| crate::styles::built_in_id(&d.doc, name))
                    {
                        actions.push(Action::Edit(Command::Style(id)));
                    }
                }
                RibbonCommand::Insert(text) => {
                    actions.push(Action::Edit(Command::Type(text.to_owned())));
                }
                RibbonCommand::InsertSymbol(c) => {
                    actions.push(Action::Edit(Command::Type(c.to_string())));
                }
                RibbonCommand::CoverPage(layout) => {
                    if let Some(document) = &self.document {
                        let cover = cover_page(&document.doc, layout);
                        actions.push(Action::Edit(Command::InsertCoverPage(Box::new(cover))));
                    }
                }
                RibbonCommand::SymbolDialog => {
                    if self.document.is_some() {
                        self.symbol_dialog.show();
                    }
                }
                RibbonCommand::DeleteWord { forward } => {
                    // С выделением удаляется только оно, как в Word.
                    if self.document.as_ref().is_some_and(|d| d.anchor == d.cursor) {
                        let motion = if forward {
                            Motion::WordRight
                        } else {
                            Motion::WordLeft
                        };
                        actions.push(Action::Move(motion, true));
                    }
                    actions.push(Action::Edit(Command::DeleteBackward));
                }
                RibbonCommand::OpenPopup(id) => egui::Popup::open_id(ctx, id),
                RibbonCommand::Focus(id) => ctx.memory_mut(|m| m.request_focus(id)),
                RibbonCommand::Find => self.find.show(false),
                RibbonCommand::Replace => self.find.show(true),
                RibbonCommand::SelectAll => actions.push(Action::SelectAll),
                RibbonCommand::LinkDialog => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    self.open_link_dialog();
                }
                RibbonCommand::BookmarkDialog => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    if let Some(document) = &self.document {
                        let names = bookmarks(&document.doc)
                            .into_iter()
                            .map(|(name, _)| name)
                            .filter(|name| !name.starts_with('_'))
                            .collect();
                        self.bookmark_dialog.show(names);
                    }
                }
                RibbonCommand::OpenLink => {
                    self.apply_actions(ctx, std::mem::take(&mut actions));
                    if let Some(document) = &mut self.document {
                        let at = document.cursor;
                        if document.follow_link(ctx, at) {
                            self.reveal_cursor = true;
                        }
                    }
                }
                RibbonCommand::RemoveLink => actions.push(Action::Edit(Command::Link {
                    link: None,
                    text: None,
                })),
                RibbonCommand::ParagraphDialog => {
                    if let Some(document) = &self.document
                        && let Some((props, _)) =
                            formatting_at(&document.doc, document.selection().start())
                    {
                        self.paragraph_dialog.show(ParagraphValues::new(&props));
                    }
                }
                RibbonCommand::FontDialog => {
                    if let Some(document) = &self.document
                        && let Some((_, props)) =
                            formatting_at(&document.doc, document.selection().start())
                    {
                        self.font_dialog.show(FontValues::new(&props));
                    }
                }
            }
        }
        self.apply_actions(ctx, actions);
    }

    /// Область документа: линейки сверху и слева, листы.
    fn document_area(&mut self, ui: &mut egui::Ui) {
        let full = ui.available_rect_before_wrap();
        // Линейки размечают лист — они только в разметке страницы.
        let ruled = self.view_mode == ViewMode::Print;
        if !self.show_ruler || !ruled || self.document.is_none() {
            self.pages(ui);
            return;
        }
        let pages_rect = Rect::from_min_max(full.min + Vec2::splat(RULER_SIZE), full.max);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(pages_rect));
        self.pages(&mut child);

        let top = Rect::from_min_max(
            pos2(pages_rect.left(), full.top()),
            pos2(full.right(), pages_rect.top()),
        );
        let left = Rect::from_min_max(
            pos2(full.left(), pages_rect.top()),
            pos2(pages_rect.left(), full.bottom()),
        );
        ruler::tab_kind_ui(
            ui,
            Rect::from_min_max(full.min, pages_rect.min),
            &mut self.ruler_state,
        );
        // Точек экрана egui на пункт документа (как у листов).
        let scale = self.zoom() * SCREEN_PIXELS_PER_POINT;
        let Some((index, rect)) = self.ruler_page else {
            return;
        };
        let Some(document) = &self.document else {
            return;
        };
        let Some(page) = document.layout.pages.get(index) else {
            return;
        };
        let (width, height, margins) = (page.width, page.height, page.margins);
        let paragraph = formatting_at(&document.doc, document.cursor)
            .map(|(props, _)| RulerParagraph::new(&props, document.doc.settings.default_tab_stop));
        ruler::paint_vertical(
            &ui.painter_at(left),
            left,
            rect.top(),
            height,
            margins,
            scale,
        );
        let Some(paragraph) = paragraph else {
            ruler::paint_horizontal(&ui.painter_at(top), top, rect.left(), width, margins, scale);
            return;
        };
        let (edit, guide) = ruler::horizontal_ui(
            ui,
            top,
            rect.left(),
            width,
            margins,
            scale,
            &paragraph,
            &mut self.ruler_state,
        );
        if let Some(x) = guide {
            // Пунктир через лист, пока метку тащат.
            ui.painter_at(pages_rect).add(egui::Shape::dashed_line(
                &[pos2(x, pages_rect.top()), pos2(x, pages_rect.bottom())],
                egui::Stroke::new(1.0, Color32::from_gray(90)),
                4.0,
                3.0,
            ));
        }
        if let Some(edit) = edit {
            let ctx = ui.ctx().clone();
            self.run_ribbon(&ctx, vec![RibbonCommand::Paragraph(edit)]);
        }
    }

    /// Строка состояния: страница, число слов, язык; справа режимы
    /// просмотра и масштаб.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let current_page = self.current_page;
            let pages = self.page_count();
            if let Some(document) = &mut self.document {
                ui.label(tr_args(
                    "status-page",
                    &[("page", (current_page + 1).into()), ("pages", pages.into())],
                ));
                let counts = document.counts();
                let words = match counts.selected {
                    Some(selected) => tr_args(
                        "status-words-selected",
                        &[
                            ("selected", selected.words.into()),
                            ("words", counts.total.words.into()),
                        ],
                    ),
                    None => tr_args("status-words", &[("words", counts.total.words.into())]),
                };
                if ui
                    .add(egui::Button::new(words).frame(false))
                    .on_hover_text(tr("word-count"))
                    .clicked()
                {
                    self.statistics_open = true;
                }
                if let Some(code) = language_at(&document.doc, document.cursor) {
                    ui.label(crate::language::display_name(&code))
                        .on_hover_text(tr_args(
                            "status-language",
                            &[("code", code.as_str().into())],
                        ));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let zoom = self.zoom();
                if ui
                    .button(format!("{:.0} %", zoom * 100.0))
                    .on_hover_text(tr("zoom-100"))
                    .clicked()
                {
                    self.set_zoom(Zoom::Fixed(1.0), None);
                }
                if ui.small_button("+").on_hover_text(tr("zoom-in")).clicked() {
                    self.zoom_step(true);
                }
                let mut percent = zoom * 100.0;
                let slider = egui::Slider::new(&mut percent, MIN_ZOOM * 100.0..=MAX_ZOOM * 100.0)
                    .logarithmic(true)
                    .show_value(false);
                let slider = ui.add(slider).on_hover_text(tr("zoom"));
                slider
                    .widget_info(|| egui::WidgetInfo::slider(true, f64::from(percent), tr("zoom")));
                if slider.changed() {
                    self.set_zoom(Zoom::Fixed(percent / 100.0), None);
                }
                if ui.small_button("−").on_hover_text(tr("zoom-out")).clicked() {
                    self.zoom_step(false);
                }
                if ui
                    .selectable_label(self.zoom == Zoom::PageWidth, tr("zoom-fit"))
                    .on_hover_text(tr("zoom-page-width"))
                    .clicked()
                {
                    self.set_zoom(Zoom::PageWidth, None);
                }
                ui.separator();
                // Режимы просмотра справа налево, как в Word.
                let size = egui::vec2(22.0, 18.0);
                for (icon, mode) in [
                    (Icon::WebLayout, ViewMode::Web),
                    (Icon::PrintLayout, ViewMode::Print),
                    (Icon::ReadMode, ViewMode::Read),
                ] {
                    let on = self.view_mode == mode;
                    let has = self.document.is_some();
                    if icons::icon_button(ui, icon, tr(mode.key()), size, has, on).clicked() {
                        self.set_view_mode(mode);
                    }
                }
            });
        });
    }

    /// Окно «Статистика»: страницы, слова, знаки, абзацы — документа
    /// или выделения.
    fn statistics_window(&mut self, ctx: &Context) {
        if !self.statistics_open {
            return;
        }
        let pages = self.page_count();
        let Some(document) = &mut self.document else {
            self.statistics_open = false;
            return;
        };
        let counts = document.counts();
        let stats = counts.selected.unwrap_or(counts.total);
        let mut open = true;
        let mut close = false;
        egui::Window::new(tr("word-count"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                if counts.selected.is_some() {
                    ui.label(tr("stats-selection"));
                }
                egui::Grid::new("statistics")
                    .num_columns(2)
                    .spacing(egui::vec2(24.0, 4.0))
                    .show(ui, |ui| {
                        let mut row = |name: &str, value: String| {
                            ui.label(name);
                            ui.label(value);
                            ui.end_row();
                        };
                        if counts.selected.is_none() {
                            row(tr("stats-pages"), pages.to_string());
                        }
                        row(tr("stats-words"), stats.words.to_string());
                        row(tr("stats-characters"), stats.characters.to_string());
                        row(
                            tr("stats-characters-spaces"),
                            stats.characters_with_spaces.to_string(),
                        );
                        row(tr("stats-paragraphs"), stats.paragraphs.to_string());
                    });
                ui.separator();
                if ui.button(tr("close")).clicked() {
                    close = true;
                }
            });
        if !open || close || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.statistics_open = false;
        }
    }

    /// Листы документа в прокручиваемой области.
    fn pages(&mut self, ui: &mut egui::Ui) {
        if self.document.is_none() {
            ui.centered_and_justified(|ui| {
                ui.label(tr("empty-hint"));
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
        let spelling_errors = match &self.document {
            Some(d) if d.story.is_none() => self.spelling.errors(&d.doc, d.history.state()),
            _ => Vec::new(),
        };
        self.sync_layout_view(area_rect.width(), zoom);
        let mode = self.view_mode;
        let Viewer {
            library,
            painter,
            document,
            show_marks,
            picture_drag,
            spelling,
            design_mode,
            ..
        } = self;
        let Some(document) = document else {
            return;
        };
        let pages = match &document.story {
            Some(story) => &story.display.pages,
            None => &document.layout.pages,
        };
        let boxes: Vec<PageBox> = pages.iter().map(|p| page_box(p, mode)).collect();
        let placement = place_view(&boxes, zoom, area_rect.width(), mode);

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

        if let Some((page, top)) = self.scroll_to.take()
            && let Some(rect) = placement.rects.get(page)
        {
            let scale = zoom * SCREEN_PIXELS_PER_POINT;
            let base = self.pending_offset.unwrap_or(self.offset);
            let y = (rect.min.y + top.max(0.0) * scale - PAGE_GAP).max(0.0);
            self.pending_offset = Some(Vec2::new(base.x, y));
        }

        let mut area = egui::ScrollArea::both().auto_shrink(false);
        if let Some(offset) = self.pending_offset.take() {
            area = area.scroll_offset(offset);
        }
        let mut ruler_page = None;
        let mut menu = Vec::new();
        // Двойной щелчок по колонтитулу открывает его для правки
        // (страница, верхний ли, место щелчка), по тексту — закрывает.
        let mut story_request: Option<StoryRequest> = None;
        // Ctrl+щелчок по ссылке на закладку: показать курсор на ней.
        let mut reveal = false;
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
            let (press, dragging, pointer, shift, command) = ui.input(|i| {
                (
                    i.pointer.primary_pressed(),
                    i.pointer.primary_down(),
                    i.pointer.interact_pos(),
                    i.modifiers.shift,
                    i.modifiers.command,
                )
            });
            let spot = pointer.and_then(|pointer| {
                let page = nearest_page(&placement.clips, pointer - origin.to_vec2())?;
                Some((page, (pointer - screen_rect(page).min) * points))
            });
            let under_pointer =
                spot.and_then(|(page, local)| hit_test(&document.layout, page, local.x, local.y));

            // Рисунки: щелчок выделяет рисунок, маркеры рамки меняют размер.
            let picture_rect = |p: &PictureBox| {
                let page = screen_rect(p.page);
                Rect::from_min_size(
                    page.min + Vec2::new(p.left, p.top) / points,
                    Vec2::new(p.width, p.height) / points,
                )
            };
            let selected = selected_picture(
                &document.layout,
                Selection::new(document.anchor, document.cursor),
            );
            let hover = ui.input(|i| i.pointer.hover_pos());
            let handle_under = |at: Option<Pos2>| {
                let p = selected?;
                let handle = crate::picture::handle_at(picture_rect(&p), at?)?;
                Some((p, handle))
            };
            // Плавающий рисунок переносят, потянув за него самого.
            let move_under = |at: Option<Pos2>| {
                let p = selected.filter(|p| p.paragraph_top.is_some())?;
                picture_rect(&p).contains(at?).then_some(p)
            };
            if let Some((_, handle)) = handle_under(hover) {
                ui.ctx().set_cursor_icon(handle.cursor());
            } else if move_under(hover).is_some()
                || picture_drag.is_some_and(|d| d.handle == Handle::MOVE)
            {
                ui.ctx().set_cursor_icon(Handle::MOVE.cursor());
            }
            let mut on_picture = false;
            if press
                && response.hovered()
                && let Some((p, handle)) = handle_under(pointer)
                && let Some(start) = pointer
            {
                *picture_drag = Some(PictureDrag {
                    picture: p,
                    handle,
                    start,
                    size: Vec2::new(p.width, p.height),
                    offset: Vec2::ZERO,
                });
            } else if press
                && response.hovered()
                && let Some(p) = move_under(pointer)
                && let Some(start) = pointer
            {
                *picture_drag = Some(PictureDrag {
                    picture: p,
                    handle: Handle::MOVE,
                    start,
                    size: Vec2::new(p.width, p.height),
                    offset: Vec2::ZERO,
                });
            }
            if let Some(drag) = picture_drag.as_mut() {
                on_picture = true;
                if dragging {
                    if let Some(at) = pointer {
                        let size = Vec2::new(drag.picture.width, drag.picture.height);
                        if drag.handle == Handle::MOVE {
                            drag.offset = (at - drag.start) * points;
                        } else {
                            drag.size = crate::picture::resized(
                                size,
                                drag.handle,
                                (at - drag.start) * points,
                            );
                        }
                    }
                } else {
                    let p = drag.picture;
                    if drag.offset != Vec2::ZERO
                        && let Some(anchor) = picture_ref(&document.doc, p.at)
                            .and_then(|picture| picture.anchor.as_deref())
                    {
                        let moved = moved_anchor(
                            &document.layout,
                            &p,
                            anchor,
                            p.left + drag.offset.x,
                            p.top + drag.offset.y,
                        );
                        menu.push(RibbonCommand::Edit(Command::PictureAnchor(Some(Box::new(
                            moved,
                        )))));
                    }
                    if drag.size != Vec2::new(drag.picture.width, drag.picture.height) {
                        menu.push(RibbonCommand::Edit(Command::ResizePicture {
                            width: crate::picture::to_emu(drag.size.x),
                            height: crate::picture::to_emu(drag.size.y),
                        }));
                    }
                    *picture_drag = None;
                }
            }
            // Нажатие на сам выделенный рисунок не начинает выделение текста.
            let press_origin = ui.input(|i| i.pointer.press_origin());
            if dragging
                && let (Some(p), Some(at)) = (selected, press_origin)
                && picture_rect(&p).contains(at)
            {
                on_picture = true;
            }
            if press
                && !on_picture
                && response.hovered()
                && !shift
                && let Some((page, local)) = spot
                && let Some(p) = picture_at_point(&document.layout, page, local.x, local.y)
            {
                let picked = p.selection();
                document.anchor = picked.anchor;
                document.cursor = picked.focus;
                document.history.seal();
                document.cursor_since = now;
                document.goal_x = None;
                on_picture = true;
            }
            // Ссылка под указателем: подсказка с адресом, Ctrl+щелчок —
            // переход. Пока кнопка нажата после перехода, выделение не идёт.
            let followed_id = egui::Id::NULL.with("letter-link-followed");
            let mut followed = ui.data(|d| d.get_temp::<bool>(followed_id).unwrap_or(false));
            if !dragging {
                followed = false;
            }
            let link_under = under_pointer
                .filter(|_| !on_picture && response.hovered())
                .and_then(|p| link_at(&document.doc, p));
            if let Some((link, _, _)) = &link_under {
                if command {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                if !dragging {
                    let text = format!("{}\n{}", link.address(), tr("link-follow-hint"));
                    response.clone().on_hover_text_at_pointer(text);
                }
                if press
                    && command
                    && let Some(at) = under_pointer
                {
                    followed = true;
                    if let Some(position) = follow_link(ui.ctx(), &document.doc, at) {
                        document.cursor = position;
                        document.anchor = position;
                        document.goal_x = None;
                        document.history.seal();
                        document.cursor_since = now;
                        reveal = true;
                    }
                }
            }
            ui.data_mut(|d| d.insert_temp(followed_id, followed));
            if followed {
                on_picture = true;
            }
            // Щелчок по флажку ставит или снимает его, как в Word.
            if press
                && !command
                && !shift
                && !on_picture
                && response.hovered()
                && under_pointer
                    .is_some_and(|at| crate::context_menu::is_checkbox(&document.doc, at))
            {
                menu.push(RibbonCommand::ToggleCheckBox);
            }
            if let Some(position) = under_pointer.filter(|_| !on_picture) {
                if press && response.hovered() {
                    document.cursor = position;
                    if !shift {
                        document.anchor = position;
                    }
                    document.history.seal();
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
                // Правая кнопка вне выделения переносит курсор, как в Word.
                let secondary = ui.input(|i| i.pointer.secondary_pressed());
                let selection = Selection::new(document.anchor, document.cursor);
                let key = |p: Position| (p.paragraph, p.offset);
                let inside = !selection.is_empty()
                    && key(position) >= key(selection.start())
                    && key(position) <= key(selection.end());
                if secondary && response.hovered() && !inside {
                    document.cursor = position;
                    document.anchor = position;
                    document.history.seal();
                    document.cursor_since = now;
                    document.goal_x = None;
                }
            }
            if response.double_clicked()
                && let Some((page, local)) = spot
                && let Some(shown) = pages.get(page)
            {
                let zone = crate::story::zone(shown, local.y);
                let note = crate::story::note_at(shown, local.y);
                let editing = document.story.as_ref().map(|s| (s.page, s.header()));
                let editing_note = document.story.as_ref().and_then(|s| s.note.clone());
                match (note, zone, editing) {
                    (Some(note), _, _) if editing_note.as_ref() == Some(&note) => {}
                    (Some(note), _, _) => story_request = Some(StoryRequest::Note(note, local)),
                    (None, Some(header), Some(open)) if open == (page, Some(header)) => {}
                    (None, Some(header), _) => {
                        story_request = Some(StoryRequest::HeaderFooter(page, header, local));
                    }
                    (None, None, Some(_)) => story_request = Some(StoryRequest::End),
                    (None, None, None) => {}
                }
            }
            let has_selection = document.anchor != document.cursor;
            let on_link = link_at(&document.doc, document.cursor).is_some();
            let paragraph = document.cursor.paragraph;
            let numbered = (list_kind(&document.doc, paragraph) == Some(ListKind::Numbered))
                .then(|| can_continue_numbering(&document.doc, paragraph));
            let key = |p: Position| (p.paragraph, p.offset);
            let misspelled = spelling_errors
                .iter()
                .find(|(a, b)| key(*a) <= key(document.cursor) && key(document.cursor) <= key(*b))
                .copied();
            let control = crate::context_menu::control_menu_for(&document.doc, document.cursor);
            response.context_menu(|ui| {
                if let Some((a, b)) = misspelled
                    && let Some(word) = letter_editor::paragraph_ref(&document.doc, a.paragraph)
                        .map(|p| p.plain_text())
                        .and_then(|t| t.get(a.offset..b.offset).map(str::to_string))
                {
                    let suggestions = spelling.suggestions(&word);
                    crate::context_menu::spelling_items(ui, &word, &suggestions, a, b, &mut menu);
                } else if let Some(control) = &control {
                    crate::context_menu::control_menu(ui, control, &mut menu);
                }
                crate::context_menu::context_menu(ui, has_selection, numbered, on_link, &mut menu);
            });

            // Тень листа выходит за его края: рисуем и чуть невидимые.
            let visible = viewport.expand(PAGE_GAP);
            for (i, page) in pages.iter().enumerate() {
                if placement.clips[i].intersects(visible) {
                    // В черновике и без листов видна только полоса текста:
                    // ни полей, ни тени.
                    let clip = placement.clips[i].translate(origin.to_vec2());
                    let sheet = if mode.continuous() {
                        ui.painter().with_clip_rect(clip.intersect(ui.clip_rect()))
                    } else {
                        ui.painter().clone()
                    };
                    painter.paint_page(&sheet, library, page, screen_rect(i));
                    if mode == ViewMode::Draft && i + 1 < pages.len() {
                        paint_page_break(ui.painter(), clip);
                    }
                    if mode == ViewMode::Outline {
                        paint_outline_marks(
                            &sheet,
                            page,
                            screen_rect(i),
                            1.0 / points,
                            &document.outline,
                            &document.outline_hidden,
                        );
                    }
                    if *show_marks {
                        paint_marks(ui.painter(), page, screen_rect(i).min, 1.0 / points);
                    }
                    if let Some(header) = document.story.as_ref().and_then(|s| s.header()) {
                        paint_story_frame(ui, page, screen_rect(i), 1.0 / points, header);
                    }
                }
            }

            // Слова с ошибкой — красная волна.
            for &(a, b) in &spelling_errors {
                for r in selection_rects(&document.layout, Selection::new(a, b)) {
                    if r.page < placement.rects.len() && placement.rects[r.page].intersects(visible)
                    {
                        let page = screen_rect(r.page);
                        let scale = 1.0 / points;
                        let rect = Rect::from_min_max(
                            page.min + Vec2::new(r.left, r.top) * scale,
                            page.min + Vec2::new(r.right, r.bottom) * scale,
                        );
                        crate::spelling::paint_wave(ui.painter(), rect, scale);
                    }
                }
            }

            // Текст примечаний подсвечен (нерешённых), текущей ветки —
            // ярче, как в Word.
            if document.story.is_none() && !document.doc.comments.is_empty() {
                let doc = &document.doc;
                let at = (document.cursor.paragraph, document.cursor.offset);
                for place in letter_editor::comment_places(doc) {
                    let open = doc
                        .comments
                        .iter()
                        .any(|c| c.id == place.id && c.parent.is_none() && !c.done);
                    if !open {
                        continue;
                    }
                    let inside = (place.start.paragraph, place.start.offset) <= at
                        && at <= (place.end.paragraph, place.end.offset);
                    let color = if inside {
                        COMMENT_CURRENT_COLOR
                    } else {
                        COMMENT_COLOR
                    };
                    let range = Selection::new(place.start, place.end);
                    for r in selection_rects(&document.layout, range) {
                        if r.page < placement.rects.len() {
                            let page = screen_rect(r.page);
                            let scale = 1.0 / points;
                            let rect = Rect::from_min_max(
                                page.min + Vec2::new(r.left, r.top) * scale,
                                page.min + Vec2::new(r.right, r.bottom) * scale,
                            );
                            ui.painter().rect_filled(rect, 0.0, color);
                        }
                    }
                }
            }

            // Рамка элемента управления содержимым, как в Word: у курсора —
            // с названием, под мышью — бледная.
            if document.story.is_none() {
                let doc = &document.doc;
                let current = letter_editor::control_range(doc, document.cursor);
                let hovered = ui
                    .input(|i| i.pointer.hover_pos())
                    .and_then(|p| {
                        let page = nearest_page(&placement.clips, p - origin.to_vec2())?;
                        let local = (p - screen_rect(page).min) * points;
                        hit_test(&document.layout, page, local.x, local.y)
                    })
                    .and_then(|at| letter_editor::control_range(doc, at))
                    .filter(|(c, _, _)| {
                        !current
                            .as_ref()
                            .is_some_and(|(now, _, _)| Arc::ptr_eq(now, c))
                    });
                // В «Режиме конструктора» видны рамки всех элементов.
                let frames: Vec<_> = if *design_mode {
                    letter_editor::controls_in(doc)
                        .into_iter()
                        .map(|found| (Some(found), true))
                        .collect()
                } else {
                    vec![(hovered, false), (current, true)]
                };
                for (found, strong) in frames {
                    let Some((control, start, end)) = found else {
                        continue;
                    };
                    let mut boxes: Vec<(usize, Rect)> = Vec::new();
                    let mut add = |page: usize, rect: Rect| match boxes.last_mut() {
                        Some((p, last)) if *p == page => *last = last.union(rect),
                        _ => boxes.push((page, rect)),
                    };
                    let rects = selection_rects(&document.layout, Selection::new(start, end));
                    if rects.is_empty()
                        && let Some(c) = caret(&document.layout, start)
                    {
                        add(
                            c.page,
                            Rect::from_min_size(pos2(c.x, c.top), Vec2::new(4.0, c.height)),
                        );
                    }
                    for r in rects {
                        add(
                            r.page,
                            Rect::from_min_max(pos2(r.left, r.top), pos2(r.right, r.bottom)),
                        );
                    }
                    let screen: Vec<Rect> = boxes
                        .into_iter()
                        .filter(|(page, _)| *page < placement.rects.len())
                        .map(|(page, r)| {
                            let min = screen_rect(page).min;
                            Rect::from_min_max(
                                min + r.min.to_vec2() / points,
                                min + r.max.to_vec2() / points,
                            )
                        })
                        .collect();
                    paint_control_frame(ui.painter(), &screen, control.title().as_deref(), strong);
                }
            }

            // Выделение — полупрозрачная заливка поверх текста. У
            // выделенного рисунка вместо неё — рамка с маркерами.
            let selection = Selection::new(document.anchor, document.cursor);
            let framed = selected_picture(&document.layout, selection);
            if let Some(p) = framed {
                let rect = picture_rect(&p);
                let rect = match *picture_drag {
                    Some(drag) if drag.handle == Handle::MOVE => {
                        rect.translate(drag.offset / points)
                    }
                    Some(drag) => crate::picture::preview(rect, drag.handle, drag.size / points),
                    None => rect,
                };
                crate::picture::paint_frame(ui.painter(), rect);
            }
            let rects = if framed.is_some() {
                Vec::new()
            } else {
                selection_rects(&document.layout, selection)
            };
            for r in rects {
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

            // Линейки показывают лист с курсором, а если его не видно — тот,
            // что посередине окна.
            let caret_page = caret(&document.layout, document.cursor)
                .map(|c| c.page)
                .filter(|&p| p < placement.clips.len() && placement.clips[p].intersects(viewport))
                .unwrap_or_else(|| current_page(&placement, viewport.top(), viewport.height()));
            if caret_page < placement.rects.len() {
                ruler_page = Some((caret_page, screen_rect(caret_page)));
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
        self.ruler_page = ruler_page;
        if reveal {
            self.reveal_cursor = true;
        }
        if let (Some(request), Some(document)) = (story_request, &mut self.document) {
            let open = match request {
                StoryRequest::HeaderFooter(page, header, local) => {
                    document.begin_story(page, header, &mut self.shaper, &self.library);
                    Some((page, local))
                }
                StoryRequest::Note((kind, id), local) => {
                    document.begin_note(kind, id, &mut self.shaper, &self.library);
                    document.story.as_ref().map(|s| (s.page, local))
                }
                StoryRequest::End => {
                    document.end_story(&mut self.shaper, &self.library);
                    None
                }
            };
            if let Some((page, local)) = open
                && let Some(position) = hit_test(&document.layout, page, local.x, local.y)
            {
                document.cursor = position;
                document.anchor = position;
            }
            document.cursor_since = ui.input(|i| i.time);
        }
        if !menu.is_empty() {
            let ctx = ui.ctx().clone();
            self.run_ribbon(&ctx, menu);
        }
        self.offset = output.state.clamped_offset();
        self.view_size = output.inner_rect.size();
        self.current_page = current_page(&placement, self.offset.y, self.view_size.y);
    }

    fn error_window(&mut self, ctx: &Context) {
        let Some(message) = &self.error else {
            return;
        };
        let mut close = false;
        egui::Window::new(self.error_title)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(message.as_str());
                ui.add_space(8.0);
                if ui.button(tr("ok")).clicked() {
                    close = true;
                }
            });
        if close || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.error = None;
        }
    }

    /// «Сохранить изменения в документе?» — перед закрытием, открытием
    /// другого файла и созданием нового.
    fn save_changes_window(&mut self, ctx: &Context) {
        let Some(intent) = self.ask.clone() else {
            return;
        };
        let name = self
            .document
            .as_ref()
            .map_or_else(String::new, OpenDocument::name);
        let mut answer = None;
        egui::Window::new(APP_NAME)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(tr_args(
                    "save-changes-question",
                    &[("name", name.as_str().into())],
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(tr("save")).clicked() {
                        answer = Some(Answer::Save);
                    }
                    if ui.button(tr("dont-save")).clicked() {
                        answer = Some(Answer::Discard);
                    }
                    if ui.button(tr("cancel")).clicked() {
                        answer = Some(Answer::Cancel);
                    }
                });
            });
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            answer = Some(Answer::Cancel);
        }
        if let Some(answer) = answer {
            self.answer(intent, answer);
        }
    }

    /// Ответ на вопрос «сохранить изменения?».
    fn answer(&mut self, intent: Intent, answer: Answer) {
        self.ask = None;
        match answer {
            Answer::Save => {
                if self.save() {
                    self.perform(intent);
                }
            }
            Answer::Discard => self.perform(intent),
            Answer::Cancel => {}
        }
    }

    /// Окно «Восстановление документов» после сбоя.
    fn recovery_window(&mut self, ctx: &Context) {
        if self.recovered.is_empty() || self.ask.is_some() {
            return;
        }
        let mut open = None;
        let mut discard = None;
        egui::Window::new(tr("recovery-title"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(tr("recovery-text"));
                ui.add_space(8.0);
                for (i, item) in self.recovered.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let name = item
                            .original
                            .as_ref()
                            .and_then(|p| p.file_name())
                            .map_or_else(
                                || tr(UNTITLED).to_owned(),
                                |n| n.to_string_lossy().into_owned(),
                            );
                        ui.label(format!("{name} ({})", ago(item.saved)));
                        if ui.button(tr("backstage-open")).clicked() {
                            open = Some(i);
                        }
                        if ui.button(tr("delete")).clicked() {
                            discard = Some(i);
                        }
                    });
                }
            });
        if let Some(i) = open {
            self.request(Intent::Recover(i));
        } else if let Some(i) = discard {
            let item = self.recovered.remove(i);
            Autosave::discard(&item);
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
        Event::Copy => Some(Action::Copy),
        Event::Cut => Some(Action::Cut),
        Event::Paste(text) => Some(Action::Paste(text.clone(), PasteMode::KeepSource)),
        Event::Text(text) | Event::Ime(ImeEvent::Commit(text)) => {
            let text: String = text.chars().filter(|c| !c.is_control()).collect();
            (!text.is_empty()).then_some(Action::Edit(Command::Type(text)))
        }
        Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => {
            let plain = !modifiers.command && !modifiers.alt;
            let command = modifiers.command && !modifiers.alt;
            let edit = match key {
                // Ctrl+Enter — разрыв страницы, Ctrl+Shift+Enter — колонки.
                Key::Enter if command && modifiers.shift => Some(Command::Break(BreakKind::Column)),
                Key::Enter if command => Some(Command::Break(BreakKind::Page)),
                Key::Enter if plain && modifiers.shift => Some(Command::LineBreak),
                Key::Enter if plain => Some(Command::SplitParagraph),
                Key::Tab if plain => Some(Command::Tab {
                    shift: modifiers.shift,
                }),
                Key::Backspace if plain => Some(Command::DeleteBackward),
                Key::Delete if plain => Some(Command::DeleteForward),
                _ => None,
            };
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

/// Лист для расстановки в режиме `mode`. В режимах без листов «лист» —
/// одна длинная полоса: видна только часть с текстом.
fn page_box(page: &letter_layout::Page, mode: ViewMode) -> PageBox {
    let (top, bottom) = match mode {
        ViewMode::Draft => (page.margins.top, page.margins.bottom),
        ViewMode::Web | ViewMode::Outline => {
            let end = page
                .lines
                .iter()
                .map(|l| l.top + l.height)
                .fold(page.margins.top, f32::max);
            (0.0, (page.height - end - WEB_MARGIN).max(0.0))
        }
        ViewMode::Print | ViewMode::Read => (0.0, 0.0),
    };
    PageBox {
        width: page.width,
        height: page.height,
        top,
        bottom,
    }
}

/// Пунктир между листами в черновике, по нижнему краю полосы `band`.
/// Рамка элемента управления содержимым вокруг его кусков `rects` (по
/// одному на страницу) и ярлык с названием над первым; `strong` — курсор
/// внутри (иначе элемент только под мышью, рамка бледнее и без ярлыка).
fn paint_control_frame(painter: &egui::Painter, rects: &[Rect], title: Option<&str>, strong: bool) {
    let color = if strong {
        CONTROL_FRAME_COLOR
    } else {
        CONTROL_FRAME_COLOR.gamma_multiply(0.5)
    };
    for rect in rects {
        painter.rect_stroke(
            rect.expand2(Vec2::new(2.0, 1.0)),
            0.0,
            egui::Stroke::new(1.0, color),
            egui::StrokeKind::Outside,
        );
    }
    let (Some(first), Some(title), true) = (rects.first(), title, strong) else {
        return;
    };
    let font = egui::FontId::proportional(11.0);
    let galley = painter.layout_no_wrap(title.to_owned(), font, Color32::WHITE);
    let size = galley.size() + Vec2::new(8.0, 2.0);
    let tab = Rect::from_min_size(first.left_top() - Vec2::new(3.0, size.y + 2.0), size);
    painter.rect_filled(tab, 0.0, color);
    painter.galley(tab.min + Vec2::new(4.0, 1.0), galley, Color32::WHITE);
}

fn paint_page_break(painter: &egui::Painter, band: Rect) {
    let y = band.bottom();
    let stroke = egui::Stroke::new(1.0, Color32::from_gray(150));
    let shapes = egui::Shape::dashed_line(
        &[Pos2::new(band.left(), y), Pos2::new(band.right(), y)],
        stroke,
        3.0,
        3.0,
    );
    painter.extend(shapes);
}

/// Значки режима «Структура» у первой строки каждого абзаца: у заголовка
/// с текстом под ним — кружок с плюсом, у заголовка без него — с минусом,
/// у основного текста — точка. `levels` — из [`outline_levels`].
fn paint_outline_marks(
    painter: &egui::Painter,
    page: &letter_layout::Page,
    rect: Rect,
    scale: f32,
    levels: &[Option<u8>],
    hidden: &[bool],
) {
    let color = Color32::from_gray(90);
    let is_hidden = |i: usize| hidden.get(i).copied().unwrap_or(false);
    let stroke = egui::Stroke::new(1.0, color);
    for line in page
        .lines
        .iter()
        .filter(|l| l.line.range.start == 0 && l.cell.is_none())
    {
        let index = line.paragraph;
        let Some(&level) = levels.get(index) else {
            continue;
        };
        if is_hidden(index) {
            continue;
        }
        let x = rect.left() + (line.x + outline_indent(levels, index) - 9.0) * scale;
        let y = rect.top() + (line.baseline - 3.5) * scale;
        let center = Pos2::new(x, y);
        match level {
            Some(level) => {
                let r = 4.0 * scale;
                painter.circle_stroke(center, r, stroke);
                let arm = 2.5 * scale;
                painter.hline(x - arm..=x + arm, y, stroke);
                // Плюс, если под заголовком есть что-то до следующего
                // заголовка того же или старшего уровня.
                let children = levels
                    .get(index + 1)
                    .is_some_and(|next| next.is_none_or(|n| n > level));
                if children {
                    painter.vline(x, y - arm..=y + arm, stroke);
                }
                // Под свёрнутым заголовком — серая черта, как в Word.
                if children && is_hidden(index + 1) {
                    let left = rect.left() + (line.x + outline_indent(levels, index)) * scale;
                    let right = left + line.line.width.max(20.0) * scale;
                    let below = rect.top() + (line.baseline + 3.0) * scale;
                    painter.hline(
                        left..=right,
                        below,
                        egui::Stroke::new(1.0, Color32::from_gray(150)),
                    );
                }
            }
            None => {
                painter.circle_filled(center, 1.6 * scale, color);
            }
        }
    }
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

/// Дата примечания из файла (`2026-10-23T12:00:00Z`) по местному
/// времени: «23.10.2026 15:00». Непонятная дата показывается как есть.
fn comment_date(date: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(date) {
        Ok(at) => at
            .with_timezone(&chrono::Local)
            .format("%d.%m.%Y %H:%M")
            .to_string(),
        Err(_) => date.to_string(),
    }
}

/// Таблица ли файл по расширению (`csv`, `txt`, `tsv`), а не документ.
fn is_table_file(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    matches!(extension.as_str(), "csv" | "txt" | "tsv")
}

/// Адрес файла для `w:dataSource`: `file:///…`, как пишет Word.
fn file_url(path: &Path) -> String {
    let text = path.display().to_string();
    match text.strip_prefix('/') {
        Some(rest) => format!("file:///{rest}"),
        None => format!("file:///{text}"),
    }
}

/// Путь к файлу из адреса `file:///…` (или из пути как есть); `%XX`
/// раскрываются.
fn url_path(url: &str) -> PathBuf {
    let decoded = percent_decode(url);
    let Some(rest) = decoded
        .strip_prefix("file:///")
        .or_else(|| decoded.strip_prefix("file://"))
    else {
        return PathBuf::from(decoded);
    };
    // `C:\…` — путь Windows, иначе — от корня.
    let windows = rest.as_bytes().get(1) == Some(&b':');
    if windows || rest.starts_with('/') {
        PathBuf::from(rest)
    } else {
        PathBuf::from(format!("/{rest}"))
    }
}

/// Раскрывает `%XX` в адресе.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Таблица получателей из файла: CSV (по расширению `csv`, `txt`, `tsv`)
/// или первая таблица документа.
fn read_recipients(path: &Path) -> Result<letter_editor::MergeData, String> {
    let data = if is_table_file(path) {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        letter_editor::MergeData::from_csv_bytes(&bytes)
    } else {
        let doc = files::read(path)?;
        letter_editor::MergeData::from_document(&doc).unwrap_or_default()
    };
    if data.headers.iter().all(|h| h.is_empty()) {
        return Err(format!("{}: {}", path.display(), tr("mailings-empty")));
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::place_pages;
    use eframe::egui::{Event, MouseWheelUnit, Pos2, RawInput, Rect, vec2};
    use letter_editor::ParagraphFormat;

    #[test]
    fn interface_font_has_the_mark_symbols() {
        let mut h = egui_kittest::Harness::new_ui(|_| {});
        h.run();
        let font = egui::FontId::proportional(12.0);
        for kind in [
            MarkKind::Space,
            MarkKind::NonBreakingSpace,
            MarkKind::Paragraph,
        ] {
            let c = kind.symbol();
            assert!(h.ctx.fonts_mut(|f| f.has_glyph(&font, c)), "{c}");
        }
    }

    #[test]
    fn marks_and_paragraph_commands() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.new_document();
        frame(&ctx, &mut viewer, screen());
        frame(
            &ctx,
            &mut viewer,
            events(vec![press(
                Key::Num8,
                Modifiers::COMMAND | Modifiers::SHIFT,
            )]),
        );
        assert!(viewer.ribbon_info().marks);
        viewer.run_ribbon(&ctx, vec![RibbonCommand::ToggleMarks]);
        assert!(!viewer.ribbon_info().marks);
        viewer.run_ribbon(
            &ctx,
            vec![
                RibbonCommand::Paragraph(ParagraphFormat::IndentMore),
                RibbonCommand::Paragraph(ParagraphFormat::LineSpacing(
                    letter_model::LineSpacing::DOUBLE,
                )),
                RibbonCommand::Paragraph(ParagraphFormat::SpaceBefore(letter_model::Twips(240))),
            ],
        );
        viewer.run_ribbon(&ctx, vec![RibbonCommand::Style("Heading1".into())]);
        let info = viewer.ribbon_info();
        assert_eq!(info.style.as_deref(), Some("Heading1"));
        assert!(info.styles.iter().any(|s| s.name == "Заголовок 1"));
        assert_eq!(info.line_spacing, Some(letter_model::LineSpacing::DOUBLE));
        assert_eq!(info.space_before, 240);
        let document = viewer.document.as_mut().unwrap();
        let first = letter_editor::paragraph_mut(&mut document.doc, 0).unwrap();
        assert!(first.props.indentation.left.is_some_and(|l| l.0 > 0));
        // Окно «Абзац» открывается со значениями абзаца у курсора.
        viewer.run_ribbon(&ctx, vec![RibbonCommand::ParagraphDialog]);
        assert!(viewer.paragraph_dialog.open);
        let values = viewer.paragraph_dialog.values.unwrap();
        assert_eq!(values.before, 240);
        assert_eq!(values.line, crate::paragraph_dialog::LineKind::Double);
        viewer.run_ribbon(&ctx, vec![RibbonCommand::FontDialog]);
        assert!(viewer.font_dialog.open);
        assert!(
            viewer
                .font_dialog
                .values
                .as_ref()
                .is_some_and(|v| v.size.0 > 0)
        );
    }

    #[test]
    fn template_opens_as_a_new_document() {
        let dir = crate::files::tests::temp_dir("template");
        let template = dir.join("бланк.dotx");
        let doc = letter_docx::read_file(fixture("plain-text.docx")).unwrap();
        files::write(&doc, &template, None).unwrap();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&template).unwrap();
        let document = viewer.document.as_ref().unwrap();
        assert_eq!(document.path, None);
        assert_eq!(document.doc.plain_text(), doc.plain_text());
        let docx = dir.join("бланк.docx");
        viewer.open(&docx).unwrap_err();
        files::write(&doc, &docx, None).unwrap();
        viewer.open(&docx).unwrap();
        assert_eq!(viewer.document.as_ref().unwrap().path, Some(docx));
    }

    #[test]
    fn encrypted_document_asks_for_password_and_stays_encrypted() {
        let dir = crate::files::tests::temp_dir("encrypted-viewer");
        let path = dir.join("тайна.docx");
        let doc = letter_docx::read_file(fixture("plain-text.docx")).unwrap();
        files::write(&doc, &path, Some("пароль")).unwrap();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&path).unwrap();
        assert!(viewer.document.is_none());
        assert!(viewer.password_dialog.open);
        viewer.password_answer(PasswordAnswer::Open(path.clone(), "не тот".into()));
        assert!(viewer.document.is_none() && viewer.error.is_some());
        viewer.error = None;
        viewer.password_answer(PasswordAnswer::Open(path.clone(), "пароль".into()));
        let document = viewer.document.as_ref().unwrap();
        assert_eq!(document.doc.plain_text(), doc.plain_text());
        assert!(viewer.backstage_info().document.unwrap().encrypted);
        // Сохранение оставляет пароль.
        assert!(viewer.save());
        assert!(files::is_encrypted(&path));

        // «Зашифровать паролем» с пустым паролем снимает шифрование.
        viewer.run_ribbon(&Context::default(), vec![RibbonCommand::EncryptDialog]);
        assert!(viewer.password_dialog.open);
        viewer.password_dialog.open = false;
        viewer.password_answer(PasswordAnswer::Encrypt(None));
        assert!(viewer.modified());
        assert!(viewer.save());
        assert!(!files::is_encrypted(&path));
        assert!(files::read(&path).is_ok());
    }

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
    fn comments_pane_adds_replies_resolves_and_deletes() {
        use crate::comments::CommentAction;
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("comments.docx")).unwrap();
        // Примечания документа видны сразу.
        assert!(viewer.ribbon_info().comments);
        frame(&ctx, &mut viewer, screen());
        let info = viewer.comments_info();
        assert_eq!(info.threads.len(), 2);
        assert_eq!(info.threads[0].comment.author, "Anna Reviewer");
        assert_eq!(info.threads[0].replies.len(), 1);
        assert!(info.threads[1].done);

        // «Далее» выделяет текст первого примечания, курсор в нём.
        viewer.run_ribbon(&ctx, vec![RibbonCommand::NextComment(true)]);
        assert_eq!(viewer.comments_info().current.as_deref(), Some("0"));
        assert!(viewer.ribbon_info().in_comment);
        viewer.run_ribbon(&ctx, vec![RibbonCommand::NextComment(true)]);
        assert_eq!(viewer.comments_info().current.as_deref(), Some("2"));

        // Новое примечание к выделенному.
        let at = Selection::new(Position::new(1, 0), Position::new(1, 5));
        viewer.comment_action(
            &ctx,
            CommentAction::Add {
                at,
                text: "New one".into(),
            },
        );
        let doc = &viewer.document.as_ref().unwrap().doc;
        let added = doc.comments.last().unwrap().clone();
        assert_eq!(added.plain_text(), "New one");
        assert!(added.date.is_some());
        assert!(!added.author.is_empty());
        assert_eq!(viewer.comments_info().threads.len(), 3);

        viewer.comment_action(
            &ctx,
            CommentAction::Reply {
                parent: added.id.clone(),
                text: "Agreed".into(),
            },
        );
        viewer.comment_action(
            &ctx,
            CommentAction::Edit {
                id: added.id.clone(),
                text: "Changed".into(),
            },
        );
        viewer.comment_action(
            &ctx,
            CommentAction::Done {
                id: added.id.clone(),
                done: true,
            },
        );
        let info = viewer.comments_info();
        let thread = info
            .threads
            .iter()
            .find(|t| t.comment.id == added.id)
            .unwrap();
        assert_eq!(thread.comment.text, "Changed");
        assert_eq!(thread.replies[0].text, "Agreed");
        assert!(thread.done);

        viewer.comment_action(&ctx, CommentAction::Delete(added.id.clone()));
        assert_eq!(viewer.comments_info().threads.len(), 2);
        // Всё отменяется по шагам.
        viewer.run_ribbon(&ctx, vec![RibbonCommand::Undo]);
        assert_eq!(viewer.comments_info().threads.len(), 3);
        viewer.run_ribbon(&ctx, vec![RibbonCommand::DeleteAllComments]);
        assert!(viewer.comments_info().threads.is_empty());
        frame(&ctx, &mut viewer, screen());

        viewer.comment_action(&ctx, CommentAction::Close);
        assert!(!viewer.ribbon_info().comments);
    }

    #[test]
    fn navigation_pane_moves_to_headings_pages_and_results() {
        use crate::navigation::{NavAction, NavTab};
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("pages.docx")).unwrap();
        viewer.run_ribbon(&ctx, vec![RibbonCommand::ToggleNavigation]);
        assert!(viewer.ribbon_info().navigation);
        frame(&ctx, &mut viewer, screen());

        let info = viewer.navigation_info();
        assert!(info.pages.len() >= 3);
        let last = info.headings.len() - 1;
        assert!(info.headings[last].title.starts_with("Chapter"));
        assert_eq!(info.current_heading, Some(0));
        viewer.navigation_action(NavAction::Heading(last));
        let document = viewer.document.as_ref().unwrap();
        let heading_page = caret(&document.layout, document.cursor).unwrap().page;
        assert!(heading_page > 0);
        assert_eq!(viewer.navigation_info().current_heading, Some(last));
        // Окно прокручивается к заголовку.
        frame(&ctx, &mut viewer, screen());
        frame(&ctx, &mut viewer, screen());
        assert_eq!(viewer.current_page, heading_page);

        viewer.navigation_action(NavAction::Page(0));
        frame(&ctx, &mut viewer, screen());
        frame(&ctx, &mut viewer, screen());
        assert_eq!(viewer.current_page, 0);

        viewer.navigation.tab = NavTab::Results;
        viewer.navigation.query = "Chapter".into();
        let info = viewer.navigation_info();
        assert_eq!(info.results.len(), info_headings_named(&viewer, "Chapter"));
        assert_eq!(info.results[0].1, "Chapter");
        viewer.navigation_action(NavAction::Result(1));
        assert_eq!(viewer.navigation_info().current_result, Some(1));
        assert!(viewer.ribbon_info().has_selection);

        viewer.navigation_action(NavAction::Close);
        assert!(!viewer.navigation.open);
    }

    fn info_headings_named(viewer: &Viewer, prefix: &str) -> usize {
        let document = viewer.document.as_ref().unwrap();
        letter_editor::document_outline(&document.doc, &document.layout)
            .iter()
            .filter(|h| h.title.starts_with(prefix))
            .count()
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

        // Кнопки: 100 %, потом мельче — 90 %. Клавиш для масштаба в
        // Word нет: Ctrl+0 и Ctrl+= заняты оформлением.
        for (command, expected) in [
            (RibbonCommand::ZoomReset, 1.0),
            (RibbonCommand::ZoomOut, 0.9),
        ] {
            viewer.run_ribbon(&ctx, vec![command]);
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
        frame(&ctx, &mut viewer, screen());

        // Щелчок далеко внизу первого листа — конец последнего абзаца.
        let point = Pos2::new(500.0, 700.0);
        let mut input = screen();
        input.time = Some(0.0);
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
        let mut now = screen();
        now.time = Some(0.05);
        let shown = frame(&ctx, &mut viewer, now);
        let mut later = screen();
        later.time = Some(CARET_BLINK + 0.1);
        let hidden = frame(&ctx, &mut viewer, later);
        assert_eq!(hidden + 1, shown, "{shown} → {hidden}");
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
            Some(Action::Edit(Command::Type("ы".into())))
        );
        assert_eq!(action(Event::Text("\r".into())), None);
        assert_eq!(
            action(Event::Ime(egui::ImeEvent::Commit("漢字".into()))),
            Some(Action::Edit(Command::Type("漢字".into())))
        );
        assert_eq!(
            action(press(Key::Enter, none)),
            Some(Action::Edit(Command::SplitParagraph))
        );
        assert_eq!(
            action(press(Key::Enter, Modifiers::SHIFT)),
            Some(Action::Edit(Command::LineBreak))
        );
        assert_eq!(
            action(press(Key::Tab, none)),
            Some(Action::Edit(Command::Tab { shift: false }))
        );
        assert_eq!(
            action(press(Key::Tab, Modifiers::SHIFT)),
            Some(Action::Edit(Command::Tab { shift: true }))
        );
        assert_eq!(
            action(press(Key::Backspace, none)),
            Some(Action::Edit(Command::DeleteBackward))
        );
        assert_eq!(
            action(press(Key::Delete, none)),
            Some(Action::Edit(Command::DeleteForward))
        );
        assert_eq!(
            action(press(Key::Enter, Modifiers::COMMAND)),
            Some(Action::Edit(Command::Break(BreakKind::Page)))
        );
        assert_eq!(
            action(press(Key::Enter, Modifiers::COMMAND | Modifiers::SHIFT)),
            Some(Action::Edit(Command::Break(BreakKind::Column)))
        );
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
        // Заново свёрстан только изменённый абзац.
        assert_eq!(viewer.document.as_ref().unwrap().cache.rebuilt(), 1);
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
    fn track_changes_from_the_keyboard_and_ribbon() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("pages.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        let before = first_text(&mut viewer);
        frame(
            &ctx,
            &mut viewer,
            key(Key::E, Modifiers::COMMAND | Modifiers::SHIFT),
        );
        assert!(viewer.ribbon_info().track_changes);
        frame(&ctx, &mut viewer, events(vec![Event::Text("a".into())]));
        assert_eq!(first_text(&mut viewer), format!("a{before}"));
        assert!(viewer.ribbon_info().has_revisions);
        viewer.run_ribbon(&ctx, vec![RibbonCommand::NextRevision(false)]);
        assert_eq!(viewer.cursor(), Some(Position::new(0, 0)));
        viewer.run_ribbon(
            &ctx,
            vec![RibbonCommand::Edit(Command::RejectRevisions { all: true })],
        );
        assert_eq!(first_text(&mut viewer), before);
        assert!(!viewer.ribbon_info().has_revisions);
        // «Запись исправлений» в защите не даёт выключить запись.
        viewer.protect_action(ProtectAction::Protect {
            edit: letter_model::EditRestriction::TrackedChanges,
            password: String::new(),
        });
        assert!(viewer.ribbon_info().track_changes_forced);
        viewer.run_ribbon(&ctx, vec![RibbonCommand::ToggleTrackChanges]);
        assert!(viewer.ribbon_info().track_changes);
    }

    #[test]
    fn spelling_errors_are_found_and_replaced() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.spelling =
            crate::spelling::Spelling::with_speller(crate::spelling::tests::speller());
        viewer.new_document();
        frame(
            &ctx,
            &mut viewer,
            events(vec![Event::Text("hello wrld".into())]),
        );
        frame(&ctx, &mut viewer, key(Key::Home, Modifiers::NONE));
        frame(&ctx, &mut viewer, key(Key::F7, Modifiers::NONE));
        let selection = viewer.selection().unwrap();
        assert_eq!(
            (selection.start(), selection.end()),
            (Position::new(0, 6), Position::new(0, 10))
        );
        viewer.run_ribbon(
            &ctx,
            vec![RibbonCommand::SpellReplace {
                start: Position::new(0, 6),
                end: Position::new(0, 10),
                text: "world".into(),
            }],
        );
        assert_eq!(first_text(&mut viewer), "hello world");
        let document = viewer.document.as_ref().unwrap();
        let state = document.history.state();
        let doc = document.doc.clone();
        assert!(viewer.spelling.errors(&doc, state).is_empty());
        viewer.run_ribbon(&ctx, vec![RibbonCommand::ToggleSpelling]);
        assert!(!viewer.ribbon_info().spelling);
    }

    #[test]
    fn compared_documents_show_revisions() {
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer
            .compare_files(&fixture("plain-text.docx"), &fixture("headings.docx"))
            .unwrap();
        assert!(viewer.ribbon_info().has_revisions);
        assert!(viewer.document.as_ref().unwrap().path.is_none());
        assert!(
            viewer
                .compare_files(&fixture("plain-text.docx"), Path::new("нет.docx"))
                .is_err()
        );
    }

    #[test]
    fn protected_document_is_not_edited() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("pages.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        let before = first_text(&mut viewer);
        viewer.protect_action(ProtectAction::Protect {
            edit: letter_model::EditRestriction::ReadOnly,
            password: "пароль".into(),
        });
        assert!(viewer.document.as_ref().unwrap().locked());
        frame(&ctx, &mut viewer, events(vec![Event::Text("a".into())]));
        assert_eq!(first_text(&mut viewer), before);
        // Неверный пароль защиту не снимает, верный — снимает.
        viewer.protect_action(ProtectAction::Unprotect("нет".into()));
        assert!(viewer.document.as_ref().unwrap().locked());
        assert!(viewer.protect_dialog.open);
        viewer.protect_action(ProtectAction::Unprotect("пароль".into()));
        assert!(!viewer.document.as_ref().unwrap().locked());
        frame(&ctx, &mut viewer, events(vec![Event::Text("a".into())]));
        assert_eq!(first_text(&mut viewer), format!("a{before}"));

        // Пароль на изменение: после сохранения и открытия — только
        // чтение, пока не введён пароль.
        viewer.protect_action(ProtectAction::WriteProtect {
            password: "2".into(),
            recommended: false,
        });
        let dir = std::env::temp_dir().join(format!("letter-protect-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.docx");
        assert!(viewer.save_to(&path));
        viewer.open(&path).unwrap();
        frame(&ctx, &mut viewer, screen());
        assert!(viewer.document.as_ref().unwrap().locked());
        viewer.protect_action(ProtectAction::AllowEditing("1".into()));
        assert!(viewer.document.as_ref().unwrap().locked());
        viewer.protect_action(ProtectAction::AllowEditing("2".into()));
        assert!(!viewer.document.as_ref().unwrap().locked());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shift_selects_and_typing_replaces_the_selection() {
        let none = Modifiers::NONE;
        assert_eq!(
            key_action(&press(Key::ArrowRight, Modifiers::SHIFT), 0.0),
            Some(Action::Move(Motion::Right, true))
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
    fn pictures_are_selected_and_resized_with_the_mouse() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("image.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        frame(&ctx, &mut viewer, screen());

        let document = viewer.document.as_ref().unwrap();
        let [picture] = letter_editor::pictures_on_page(&document.layout, 0)[..] else {
            panic!("рисунок не найден");
        };
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
        let at = move |x: f32, y: f32| origin + page.min.to_vec2() + vec2(x, y) * scale;
        let button = |pos: Pos2, pressed: bool| Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };

        // Щелчок по рисунку выделяет его.
        let middle = at(
            picture.left + picture.width / 2.0,
            picture.top + picture.height / 2.0,
        );
        frame(
            &ctx,
            &mut viewer,
            events(vec![Event::PointerMoved(middle), button(middle, true)]),
        );
        frame(&ctx, &mut viewer, events(vec![button(middle, false)]));
        assert_eq!(viewer.selection(), Some(picture.selection()));

        // Правый нижний маркер на 36 пт вправо: шире и выше в той же
        // пропорции.
        let corner = at(picture.left + picture.width, picture.top + picture.height);
        let moved = at(
            picture.left + picture.width + 36.0,
            picture.top + picture.height,
        );
        frame(
            &ctx,
            &mut viewer,
            events(vec![Event::PointerMoved(corner), button(corner, true)]),
        );
        frame(&ctx, &mut viewer, events(vec![Event::PointerMoved(moved)]));
        frame(
            &ctx,
            &mut viewer,
            events(vec![Event::PointerMoved(moved), button(moved, false)]),
        );
        frame(&ctx, &mut viewer, screen());
        let document = viewer.document.as_ref().unwrap();
        let resized = letter_editor::picture_ref(&document.doc, picture.at).unwrap();
        let width = resized.width_points();
        let height = resized.height_points();
        assert!((width - (picture.width + 36.0)).abs() < 0.5, "{width}");
        assert!(
            (height / width - picture.height / picture.width).abs() < 0.01,
            "{width} × {height}"
        );
        // Рисунок остался выделенным; Ctrl+Z возвращает размер.
        assert_eq!(viewer.selection(), Some(picture.selection()));
        frame(&ctx, &mut viewer, key(Key::Z, Modifiers::COMMAND));
        let document = viewer.document.as_ref().unwrap();
        let back = letter_editor::picture_ref(&document.doc, picture.at).unwrap();
        assert!((back.width_points() - picture.width).abs() < 0.01);

        // «Обтекание текстом → Вокруг рамки»: рисунок становится плавающим
        // и остаётся на месте; появляется вкладка «Формат рисунка».
        frame(&ctx, &mut viewer, screen());
        assert_eq!(viewer.ribbon_info().picture_wrap, Some(PictureWrap::InLine));
        viewer.run_ribbon(&ctx, vec![RibbonCommand::PictureWrap(PictureWrap::Square)]);
        frame(&ctx, &mut viewer, screen());
        let document = viewer.document.as_ref().unwrap();
        let floating = letter_editor::picture_ref(&document.doc, picture.at).unwrap();
        assert_eq!(PictureWrap::of(floating), PictureWrap::Square);
        let [placed] = letter_editor::pictures_on_page(&document.layout, 0)[..] else {
            panic!("рисунок не найден");
        };
        assert!(placed.paragraph_top.is_some());
        assert!((placed.left - picture.left).abs() < 0.5, "{placed:?}");
        assert!((placed.top - picture.top).abs() < 0.5, "{placed:?}");
        assert_eq!(viewer.selection(), Some(picture.selection()));

        // Плавающий рисунок переносят мышью: на 50 пт вправо и 20 вниз.
        let middle = at(
            placed.left + placed.width / 2.0,
            placed.top + placed.height / 2.0,
        );
        let target = middle + vec2(50.0, 20.0) * scale;
        frame(
            &ctx,
            &mut viewer,
            events(vec![Event::PointerMoved(middle), button(middle, true)]),
        );
        frame(&ctx, &mut viewer, events(vec![Event::PointerMoved(target)]));
        frame(
            &ctx,
            &mut viewer,
            events(vec![Event::PointerMoved(target), button(target, false)]),
        );
        frame(&ctx, &mut viewer, screen());
        let document = viewer.document.as_ref().unwrap();
        let [after] = letter_editor::pictures_on_page(&document.layout, 0)[..] else {
            panic!("рисунок не найден");
        };
        assert!((after.left - placed.left - 50.0).abs() < 0.5, "{after:?}");
        assert!((after.top - placed.top - 20.0).abs() < 0.5, "{after:?}");

        // «В тексте» возвращает рисунок в строку.
        viewer.run_ribbon(&ctx, vec![RibbonCommand::PictureWrap(PictureWrap::InLine)]);
        let document = viewer.document.as_ref().unwrap();
        let inline = letter_editor::picture_ref(&document.doc, picture.at).unwrap();
        assert_eq!(inline.anchor, None);
    }

    #[test]
    fn picture_file_is_inserted_at_the_cursor() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.new_document();
        let dir = std::env::temp_dir().join(format!("letter-picture-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("точка.png");
        let mut png = Vec::new();
        image_png(&mut png);
        std::fs::write(&path, &png).unwrap();
        viewer.insert_picture_file(&ctx, &path);
        let document = viewer.document.as_ref().unwrap();
        assert_eq!(document.doc.plain_text(), "\u{fffc}");
        assert_eq!(viewer.cursor(), Some(Position::new(0, 3)));
        // Не картинка — сообщение об ошибке, документ не меняется.
        let bad = dir.join("плохой.png");
        std::fs::write(&bad, b"not a picture").unwrap();
        viewer.insert_picture_file(&ctx, &bad);
        assert!(viewer.error.is_some());
        assert_eq!(
            viewer.document.as_ref().unwrap().doc.plain_text(),
            "\u{fffc}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn shape_is_inserted_and_restyled() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.new_document();
        viewer.run_ribbon(&ctx, vec![RibbonCommand::InsertShape("ellipse")]);
        assert_eq!(viewer.ribbon.tab, crate::ribbon::Tab::ShapeFormat);
        let info = viewer.ribbon_info();
        let shape = info.shape.expect("фигура выделена");
        assert_eq!(info.picture_size, Some((914400, 914400)));
        // Красная заливка из вкладки «Формат фигуры»; отмена её снимает.
        let red = letter_editor::with_fill(&shape, Some([255, 0, 0]));
        viewer.run_ribbon(
            &ctx,
            vec![RibbonCommand::Edit(Command::PictureShape(Box::new(red)))],
        );
        let fill = |viewer: &Viewer| viewer.ribbon_info().shape.and_then(|s| s.fill);
        assert_eq!(
            fill(&viewer),
            Some(letter_model::ShapeColor::rgb([255, 0, 0]))
        );
        viewer.run_ribbon(&ctx, vec![RibbonCommand::Undo]);
        assert_eq!(fill(&viewer), shape.fill);
        // Фигура сохраняется в .docx и читается обратно.
        let doc = &viewer.document.as_ref().unwrap().doc;
        let reread = letter_docx::read_bytes(&letter_docx::write_bytes(doc).unwrap()).unwrap();
        let at = Position::new(0, 0);
        let again = letter_editor::shape_ref(&reread, at).expect("фигура прочитана");
        assert_eq!(again.geometry, shape.geometry);
        // Цвета темы при чтении заменяются настоящими — те же на экране.
        let scheme = letter_model::ColorScheme::default();
        let rgba = |s: &letter_model::Shape| s.fill.as_ref().map(|c| c.rgba(&scheme));
        assert_eq!(rgba(again), rgba(&shape));
        // Без выделенной фигуры вкладка уходит.
        let document = viewer.document.as_mut().unwrap();
        let end = document.cursor;
        document.set_selection(Selection::new(end, end));
        assert!(viewer.ribbon_info().shape.is_none());
    }

    #[test]
    fn content_control_is_inserted_typed_and_saved() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.new_document();
        let kind = letter_editor::NewControl::PlainText;
        viewer.run_ribbon(&ctx, vec![RibbonCommand::InsertControl(kind)]);
        assert_eq!(first_text(&mut viewer), "Место для ввода текста.");
        viewer.run_ribbon(
            &ctx,
            vec![RibbonCommand::Edit(Command::Type("Иван".into()))],
        );
        assert_eq!(first_text(&mut viewer), "Иван");
        let doc = &viewer.document.as_ref().unwrap().doc;
        let reread = letter_docx::read_bytes(&letter_docx::write_bytes(doc).unwrap()).unwrap();
        let (control, _, _) = letter_editor::control_at(&reread, Position::new(0, 0)).unwrap();
        assert_eq!(control.kind(), letter_model::ControlKind::Text);
        assert!(!control.showing_placeholder());
        // «Свойства»: окно открывается для элемента у курсора, новое
        // название сохраняется в файл.
        assert!(viewer.ribbon_info().in_control);
        viewer.run_ribbon(&ctx, vec![RibbonCommand::ControlProperties]);
        assert!(viewer.control_dialog.open);
        let named = control.with_settings("Имя", "name", letter_model::ControlLock::Control);
        let command = Command::SetControl(Box::new(named));
        viewer.run_ribbon(&ctx, vec![RibbonCommand::Edit(command)]);
        let doc = &viewer.document.as_ref().unwrap().doc;
        let reread = letter_docx::read_bytes(&letter_docx::write_bytes(doc).unwrap()).unwrap();
        let (control, _, _) = letter_editor::control_at(&reread, Position::new(0, 0)).unwrap();
        assert_eq!(control.title().as_deref(), Some("Имя"));
        assert_eq!(control.tag().as_deref(), Some("name"));
        assert_eq!(control.lock(), letter_model::ControlLock::Control);
    }

    #[test]
    fn erased_control_shows_placeholder_again() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.new_document();
        let kind = letter_editor::NewControl::RichText;
        viewer.run_ribbon(&ctx, vec![RibbonCommand::InsertControl(kind)]);
        let typed = Command::Type("Иван".into());
        viewer.run_ribbon(&ctx, vec![RibbonCommand::Edit(typed)]);
        let document = viewer.document.as_mut().unwrap();
        let end = Position::new(0, "Иван".len());
        document.set_selection(Selection::new(Position::new(0, 0), end));
        viewer.run_ribbon(&ctx, vec![RibbonCommand::Edit(Command::DeleteBackward)]);
        assert_eq!(first_text(&mut viewer), "Место для ввода текста.");
        // «Режим конструктора»: набор не заменяет подсказку.
        viewer.run_ribbon(&ctx, vec![RibbonCommand::DesignMode]);
        assert!(viewer.ribbon_info().design_mode);
        let typed = Command::Type("!".into());
        viewer.run_ribbon(&ctx, vec![RibbonCommand::Edit(typed)]);
        assert_eq!(first_text(&mut viewer), "!Место для ввода текста.");
        viewer.run_ribbon(&ctx, vec![RibbonCommand::DesignMode]);
        assert!(!viewer.ribbon_info().design_mode);
    }

    #[test]
    fn mail_merge() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.new_document();
        assert!(viewer.ribbon_info().mailing.fields.is_empty());
        let data = letter_editor::MergeData::from_csv("Имя;Город\nАнна;Тверь\nБорис;Омск\n");
        assert!(viewer.set_recipients(data).is_none());
        assert_eq!(viewer.ribbon_info().mailing.fields, ["Имя", "Город"]);
        let commands = vec![
            RibbonCommand::Edit(Command::Type("Здравствуйте, ".into())),
            RibbonCommand::InsertMergeField("Имя".into()),
        ];
        viewer.run_ribbon(&ctx, commands);
        assert_eq!(first_text(&mut viewer), "Здравствуйте, «Имя»");
        viewer.run_ribbon(&ctx, vec![RibbonCommand::MergePreview(true)]);
        assert_eq!(first_text(&mut viewer), "Здравствуйте, Анна");
        viewer.run_ribbon(&ctx, vec![RibbonCommand::MergeRecord(1)]);
        assert_eq!(first_text(&mut viewer), "Здравствуйте, Борис");
        // Дальше последней записи не уходим.
        viewer.run_ribbon(&ctx, vec![RibbonCommand::MergeRecord(1)]);
        assert_eq!(viewer.ribbon_info().mailing.record, 1);
        viewer.run_ribbon(&ctx, vec![RibbonCommand::MergePreview(false)]);
        assert_eq!(first_text(&mut viewer), "Здравствуйте, «Имя»");
        // Поле слияния сохраняется в .docx.
        let doc = &viewer.document.as_ref().unwrap().doc;
        let reread = letter_docx::read_bytes(&letter_docx::write_bytes(doc).unwrap()).unwrap();
        assert!(letter_editor::has_merge_fields(&reread));
        viewer.finish_merge();
        let merged = &viewer.document.as_ref().unwrap().doc;
        let texts: Vec<String> = merged.blocks().map(|b| b.plain_text()).collect();
        assert_eq!(texts, ["Здравствуйте, Анна", "Здравствуйте, Борис"]);
        assert!(!letter_editor::has_merge_fields(merged));
    }

    #[test]
    fn mail_merge_rules_and_chosen_recipients() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.new_document();
        let mut data =
            letter_editor::MergeData::from_csv("Имя;Город\nАнна;Тверь\nБорис;Омск\nВера;Сочи\n");
        data.set_included(1, false);
        viewer.set_recipients(data);
        assert_eq!(viewer.ribbon_info().mailing.records, 2);
        let rule = letter_editor::MergeRule::If {
            field: "Город".into(),
            compare: letter_editor::Compare::Equal,
            value: "Сочи".into(),
            then: "юг".into(),
            otherwise: "север".into(),
        };
        let commands = vec![
            RibbonCommand::InsertMergeField("Имя".into()),
            RibbonCommand::Edit(Command::Type(" ".into())),
            RibbonCommand::InsertMergeRule(rule),
            RibbonCommand::MergePreview(true),
            RibbonCommand::MergeRecord(1),
        ];
        viewer.run_ribbon(&ctx, commands);
        // Вторая отмеченная запись — Вера (Борис снят); «Сочи» — скрытый
        // результат поля внутри кода IF.
        assert_eq!(first_text(&mut viewer), "Вера Сочиюг");
        let doc = &viewer.document.as_ref().unwrap().doc;
        let reread = letter_docx::read_bytes(&letter_docx::write_bytes(doc).unwrap()).unwrap();
        assert_eq!(
            letter_editor::paragraph_ref(&reread, 0)
                .unwrap()
                .plain_text(),
            "Вера Сочиюг"
        );
        viewer.finish_merge();
        let merged = &viewer.document.as_ref().unwrap().doc;
        let texts: Vec<String> = merged.blocks().map(|b| b.plain_text()).collect();
        assert_eq!(texts, ["Анна север", "Вера юг"]);
    }

    #[test]
    fn recipients_reconnect_when_the_file_is_opened() {
        assert_eq!(
            url_path("file:///home/nikita/%D0%9A%D0%BB%D0%B8%D0%B5%D0%BD%D1%82%D1%8B.csv"),
            PathBuf::from("/home/nikita/Клиенты.csv")
        );
        assert_eq!(
            url_path("file:///C:\\Data\\a.csv"),
            PathBuf::from("C:\\Data\\a.csv")
        );
        assert_eq!(url_path("/tmp/a.csv"), PathBuf::from("/tmp/a.csv"));
        assert_eq!(file_url(Path::new("/tmp/a b.csv")), "file:///tmp/a b.csv");
        assert_eq!(
            url_path(&file_url(Path::new("/tmp/a b.csv"))),
            PathBuf::from("/tmp/a b.csv")
        );

        let dir = std::env::temp_dir().join(format!("letter-merge-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let csv = dir.join("Клиенты.csv");
        std::fs::write(&csv, "Имя\nАнна\nБорис\n").unwrap();
        let mut doc = letter_model::Document::new();
        doc.settings.mail_merge = Some(letter_model::MailMerge {
            document_type: "formLetters".into(),
            data_type: Some("textFile".into()),
            query: None,
            data_source: Some(file_url(&csv)),
        });
        let path = dir.join("письмо.docx");
        std::fs::write(&path, letter_docx::write_bytes(&doc).unwrap()).unwrap();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&path).unwrap();
        assert_eq!(viewer.ribbon_info().mailing.records, 2);
        // Документ слияния — уже без источника.
        viewer.finish_merge();
        assert!(viewer.mailing.is_none());
        let merged = &viewer.document.as_ref().unwrap().doc;
        assert_eq!(merged.settings.mail_merge, None);
        // Нет файла — получателей нет, документ открывается.
        std::fs::remove_file(&csv).unwrap();
        viewer.open(&path).unwrap();
        assert!(viewer.mailing.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn equation_is_inserted_and_edited() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.new_document();
        viewer.run_ribbon(&ctx, vec![RibbonCommand::InsertEquation]);
        assert_eq!(viewer.ribbon.tab, crate::ribbon::Tab::Equation);
        let (at, math) = viewer.ribbon_info().equation.expect("формула выделена");
        assert!(math.display);
        // Правки в поле линейного вида меняют документ и отменяются разом.
        for text in ["x", "x^", "x^2"] {
            let math = Box::new(letter_model::Math::from_linear(text, true));
            viewer.run_ribbon(&ctx, vec![RibbonCommand::SetEquation(at, math)]);
        }
        let linear = |viewer: &Viewer| viewer.ribbon_info().equation.map(|(_, m)| m.linear_text());
        assert_eq!(linear(&viewer).as_deref(), Some("x^2"));
        let doc = &viewer.document.as_ref().unwrap().doc;
        let reread = letter_docx::read_bytes(&letter_docx::write_bytes(doc).unwrap()).unwrap();
        let again = letter_editor::math_ref(&reread, at).expect("формула прочитана");
        assert_eq!(again.linear_text(), "x^2");
        assert!(again.display);
        viewer.run_ribbon(&ctx, vec![RibbonCommand::Undo]);
        assert_eq!(linear(&viewer).as_deref(), Some(""));
        // Курсор сразу за формулой — вкладка остаётся; дальше — нет.
        let document = viewer.document.as_mut().unwrap();
        let end = Position::new(0, letter_model::OBJECT_CHAR.len_utf8());
        document.set_selection(Selection::new(end, end));
        assert!(viewer.ribbon_info().equation.is_some());
        viewer.run_ribbon(&ctx, vec![RibbonCommand::Edit(Command::Type(" y".into()))]);
        assert!(viewer.ribbon_info().equation.is_none());
    }

    /// PNG 1 × 1 с прозрачной точкой.
    fn image_png(out: &mut Vec<u8>) {
        let picture = crate::picture::tests_png();
        out.extend_from_slice(&picture);
    }

    #[test]
    fn double_click_edits_the_header() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("header-footer.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        frame(&ctx, &mut viewer, screen());
        let header_text = |viewer: &Viewer| {
            let document = viewer.document.as_ref().unwrap();
            let props = &document.body().sections[0].props;
            props.headers.default.as_ref().unwrap().blocks[0].plain_text()
        };
        let shown_header = |viewer: &Viewer| {
            let document = viewer.document.as_ref().unwrap();
            let page = match &document.story {
                Some(story) => &story.display.pages[0],
                None => &document.layout.pages[0],
            };
            let line = &page.header.lines[0];
            line.text[line.line.range.clone()].to_string()
        };

        // Двойной щелчок по строке верхнего колонтитула.
        let document = viewer.document.as_ref().unwrap();
        let first = &document.layout.pages[0];
        let line = first.header.lines[0].clone();
        let scale = viewer.zoom() * SCREEN_PIXELS_PER_POINT;
        let page = place_pages(
            &[(first.width, first.height)],
            viewer.zoom(),
            viewer.view_size.x,
        )
        .rects[0];
        let at = viewer.view_origin
            + page.min.to_vec2()
            + vec2(line.x + 1.0, line.top + line.height / 2.0) * scale;
        let button = |pressed: bool| Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let mut input = events(vec![
            Event::PointerMoved(at),
            button(true),
            button(false),
            button(true),
            button(false),
        ]);
        input.time = Some(5.0);
        frame(&ctx, &mut viewer, input);
        assert!(viewer.document.as_ref().unwrap().story.is_some());
        assert_eq!(viewer.cursor(), Some(Position::new(0, 0)));

        // Набранное сразу видно в колонтитуле.
        frame(&ctx, &mut viewer, events(vec![Event::Text("New ".into())]));
        assert_eq!(header_text(&viewer), "New Header text");
        assert_eq!(shown_header(&viewer), "New Header text");
        // Разрыв страницы в колонтитул не вставляется.
        frame(&ctx, &mut viewer, key(Key::Enter, Modifiers::COMMAND));
        assert_eq!(header_text(&viewer), "New Header text");

        // Esc кончает правку; изменение — один шаг отмены.
        frame(&ctx, &mut viewer, key(Key::Escape, Modifiers::NONE));
        let document = viewer.document.as_ref().unwrap();
        assert!(document.story.is_none());
        assert!(document.modified());
        assert_eq!(header_text(&viewer), "New Header text");
        assert_eq!(shown_header(&viewer), "New Header text");
        frame(&ctx, &mut viewer, key(Key::Z, Modifiers::COMMAND));
        assert_eq!(header_text(&viewer), "Header text");
        assert_eq!(shown_header(&viewer), "Header text");
    }

    #[test]
    fn links_are_followed_and_edited() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("links.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        let set_cursor = |viewer: &mut Viewer, at: Position| {
            let document = viewer.document.as_mut().unwrap();
            document.cursor = at;
            document.anchor = at;
        };
        // Ссылка на закладку переводит курсор на закладку.
        set_cursor(&mut viewer, Position::new(1, 8));
        viewer.run_ribbon(&ctx, vec![RibbonCommand::OpenLink]);
        assert_eq!(viewer.cursor(), Some(Position::new(2, 8)));

        // Окно на ссылке выделяет её целиком.
        set_cursor(&mut viewer, Position::new(0, 9));
        viewer.run_ribbon(&ctx, vec![RibbonCommand::LinkDialog]);
        assert!(viewer.link_dialog.open);
        assert!(viewer.link_dialog.editing);
        assert_eq!(viewer.link_dialog.text, "example site");
        let document = viewer.document.as_ref().unwrap();
        assert_eq!(document.anchor, Position::new(0, 6));
        assert_eq!(document.cursor, Position::new(0, 18));
        viewer.link_dialog.open = false;

        // Новая ссылка с текстом у курсора, потом удаление.
        set_cursor(&mut viewer, Position::new(0, 19));
        let link = letter_model::Hyperlink::to("https://letter.example/");
        viewer.run_ribbon(
            &ctx,
            vec![RibbonCommand::Edit(Command::Link {
                link: Some(Box::new(link.clone())),
                text: Some(" more".into()),
            })],
        );
        let document = viewer.document.as_ref().unwrap();
        assert_eq!(
            document.doc.sections[0].blocks[0].plain_text(),
            "Site: example site. more"
        );
        let found = link_at(&document.doc, Position::new(0, 21)).unwrap();
        assert_eq!(found.0, link);
        viewer.run_ribbon(&ctx, vec![RibbonCommand::RemoveLink]);
        let document = viewer.document.as_ref().unwrap();
        assert!(link_at(&document.doc, Position::new(0, 21)).is_none());
        assert!(link_at(&document.doc, Position::new(0, 9)).is_some());
    }

    #[test]
    fn cover_page_takes_document_properties() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("links.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        viewer.document.as_mut().unwrap().doc.properties.core.title = Some("Отчёт".into());
        viewer.run_ribbon(
            &ctx,
            vec![RibbonCommand::CoverPage(letter_editor::CoverLayout::Band)],
        );
        let document = viewer.document.as_ref().unwrap();
        let blocks = &document.doc.sections[0].blocks;
        assert_eq!(blocks[0].plain_text(), "Отчёт");
        assert_eq!(blocks[1].plain_text(), tr("cover-subtitle"));
        assert_eq!(blocks[4].plain_text(), "\u{c}");
        assert_eq!(document.cursor, Position::new(0, 0));
        // Обложка — на своей странице.
        assert!(document.layout.pages.len() >= 2);
    }

    #[test]
    fn symbols_are_inserted() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("links.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        {
            let document = viewer.document.as_mut().unwrap();
            document.anchor = Position::new(0, 0);
            document.cursor = Position::new(0, 0);
        }
        viewer.run_ribbon(&ctx, vec![RibbonCommand::InsertSymbol('©')]);
        let document = viewer.document.as_ref().unwrap();
        assert!(
            document.doc.sections[0].blocks[0]
                .plain_text()
                .starts_with("©Site")
        );
        assert_eq!(document.cursor, Position::new(0, '©'.len_utf8()));
        viewer.run_ribbon(&ctx, vec![RibbonCommand::SymbolDialog]);
        assert!(viewer.symbol_dialog.open);
        // Окно рисуется без ошибок на обеих страницах.
        frame(&ctx, &mut viewer, screen());
        viewer.symbol_dialog.special = true;
        frame(&ctx, &mut viewer, screen());
    }

    #[test]
    fn bookmarks_from_the_dialog() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("links.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        viewer.run_ribbon(&ctx, vec![RibbonCommand::BookmarkDialog]);
        assert!(viewer.bookmark_dialog.open);
        assert_eq!(viewer.bookmark_dialog.bookmarks, ["target"]);
        // Закладка на слово «Site».
        {
            let document = viewer.document.as_mut().unwrap();
            document.anchor = Position::new(0, 0);
            document.cursor = Position::new(0, 4);
        }
        viewer.bookmark_action(&ctx, BookmarkAction::Add("Start".into()));
        let document = viewer.document.as_mut().unwrap();
        assert_eq!(
            bookmark_range(&document.doc, "start"),
            Some((Position::new(0, 0), Position::new(0, 4)))
        );
        document.anchor = Position::new(2, 0);
        document.cursor = Position::new(2, 0);
        viewer.bookmark_action(&ctx, BookmarkAction::GoTo("target".into()));
        let document = viewer.document.as_ref().unwrap();
        assert_eq!(document.anchor, Position::new(2, 8));
        assert_eq!(document.cursor, Position::new(2, 18));
        viewer.bookmark_action(&ctx, BookmarkAction::Delete("Start".into()));
        let document = viewer.document.as_ref().unwrap();
        assert_eq!(bookmark_range(&document.doc, "start"), None);
        // Отмена возвращает закладку.
        viewer.run_ribbon(&ctx, vec![RibbonCommand::Undo]);
        let document = viewer.document.as_ref().unwrap();
        assert!(bookmark_range(&document.doc, "start").is_some());
    }

    #[test]
    fn toc_is_inserted_and_saved() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("headings.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        viewer.run_ribbon(&ctx, vec![RibbonCommand::InsertToc]);
        let document = viewer.document.as_ref().unwrap();
        let lines: Vec<String> = document.layout.pages[0]
            .lines
            .iter()
            .map(|l| {
                l.text
                    .get(l.line.range.clone())
                    .unwrap_or_default()
                    .to_string()
            })
            .collect();
        assert_eq!(lines[0], "Оглавление");
        assert!(lines[1].starts_with("Heading 1\t1"), "{lines:?}");
        assert!(lines[3].starts_with("Heading 3\t1"), "{lines:?}");
        // Оглавление переживает сохранение и обновляется.
        let bytes = letter_docx::write_bytes(&document.doc).unwrap();
        let reopened = letter_docx::read_bytes(&bytes).unwrap();
        assert_eq!(reopened.sections, document.doc.sections);
        viewer.run_ribbon(&ctx, vec![RibbonCommand::UpdateToc]);
        let document = viewer.document.as_ref().unwrap();
        assert_eq!(document.layout.pages[0].lines.len(), lines.len());
    }

    #[test]
    fn toc_is_deleted_and_add_text_sets_levels() {
        use letter_editor::OutlineChange;
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("headings.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        let text = |viewer: &Viewer| viewer.document.as_ref().unwrap().doc.plain_text();
        let before = text(&viewer);
        viewer.run_ribbon(&ctx, vec![RibbonCommand::InsertToc]);
        assert_ne!(text(&viewer), before);
        viewer.run_ribbon(&ctx, vec![RibbonCommand::Edit(Command::DeleteToc)]);
        assert_eq!(text(&viewer), before);

        // «Добавить текст → Уровень 2» у обычного текста и обратно.
        let document = viewer.document.as_mut().unwrap();
        document.set_selection(Selection::caret(Position::new(2, 0)));
        assert_eq!(viewer.ribbon_info().outline_level, None);
        let add = |change| vec![RibbonCommand::Edit(Command::OutlineLevel(change))];
        viewer.run_ribbon(&ctx, add(OutlineChange::Level(2)));
        assert_eq!(viewer.ribbon_info().outline_level, Some(2));
        viewer.run_ribbon(&ctx, add(OutlineChange::Body));
        assert_eq!(viewer.ribbon_info().outline_level, None);
    }

    #[test]
    fn next_footnote_goes_round() {
        use letter_model::NoteKind;
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("notes.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        let next = |kind, forward| vec![RibbonCommand::NextNote { kind, forward }];
        let start = |viewer: &Viewer| {
            let document = viewer.document.as_ref().unwrap();
            assert!(!document.selection().is_empty());
            document.selection().start()
        };
        let first = Position::new(0, "First claim".len());
        viewer.run_ribbon(&ctx, next(NoteKind::Footnote, true));
        assert_eq!(start(&viewer), first);
        viewer.run_ribbon(&ctx, next(NoteKind::Footnote, true));
        let second = start(&viewer);
        assert!(second.offset > first.offset && second.paragraph == 0);
        // По кругу к первой и назад к последней.
        viewer.run_ribbon(&ctx, next(NoteKind::Footnote, true));
        assert_eq!(start(&viewer), first);
        viewer.run_ribbon(&ctx, next(NoteKind::Footnote, false));
        assert_eq!(start(&viewer), second);
        viewer.run_ribbon(&ctx, next(NoteKind::Endnote, true));
        assert_eq!(start(&viewer).paragraph, 1);
    }

    #[test]
    fn ctrl_alt_f_inserts_a_footnote() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("pages.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        let shortcut = key(Key::F, Modifiers::COMMAND | Modifiers::ALT);
        frame(&ctx, &mut viewer, shortcut);
        // Сноска открыта для правки: набранное идёт в её текст.
        assert!(viewer.document.as_ref().unwrap().story.is_some());
        let mut input = screen();
        input.events = vec![Event::Text("note".into())];
        frame(&ctx, &mut viewer, input);
        frame(&ctx, &mut viewer, key(Key::Escape, Modifiers::NONE));
        let document = viewer.document.as_ref().unwrap();
        assert!(document.story.is_none());
        let regular = |doc: &Document| {
            doc.footnotes
                .iter()
                .filter(|n| n.is_regular())
                .map(|n| n.blocks[0].plain_text())
                .collect::<Vec<_>>()
        };
        let texts = regular(&document.doc);
        assert_eq!(texts.len(), 1);
        assert!(texts[0].ends_with(" note"), "{texts:?}");
        assert_eq!(document.layout.pages[0].notes.len(), 1);
        assert!(document.modified());
        // Отмена убирает текст сноски, потом — саму ссылку.
        frame(&ctx, &mut viewer, key(Key::Z, Modifiers::COMMAND));
        let document = viewer.document.as_ref().unwrap();
        assert!(!regular(&document.doc)[0].contains("note"));
        frame(&ctx, &mut viewer, key(Key::Z, Modifiers::COMMAND));
        let document = viewer.document.as_ref().unwrap();
        assert!(document.layout.pages[0].notes.is_empty());
    }

    #[test]
    fn f9_updates_fields() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("pages.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        // В конец документа — абзац с полями PAGE и NUMPAGES.
        let mut runs = letter_editor::page_number_runs();
        runs.push(letter_model::Run::text(" / "));
        for mut run in letter_editor::page_number_runs() {
            for item in &mut run.content {
                if let letter_model::RunContent::FieldCode(code) = item {
                    *code = "NUMPAGES".into();
                }
            }
            runs.push(run);
        }
        let Viewer {
            document,
            shaper,
            library,
            ..
        } = &mut viewer;
        let document = document.as_mut().unwrap();
        let last = document.doc.sections.len() - 1;
        document.doc.sections[last]
            .blocks
            .push(Paragraph::from_runs(runs).into());
        document.relayout(shaper, library);
        let pages = document.layout.pages.len();
        assert!(pages >= 2);

        frame(&ctx, &mut viewer, key(Key::A, Modifiers::COMMAND));
        frame(&ctx, &mut viewer, key(Key::F9, Modifiers::NONE));
        let document = viewer.document.as_ref().unwrap();
        let blocks = &document.doc.sections[last].blocks;
        let text = blocks[blocks.len() - 1].plain_text();
        assert_eq!(text, format!("{pages} / {pages}"));
        assert!(document.modified());
    }

    #[test]
    fn fields_are_inserted_from_the_ribbon() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("pages.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        let pages = viewer.document.as_ref().unwrap().layout.pages.len();
        let before = viewer.document.as_ref().unwrap().doc.sections[0].blocks[0].plain_text();
        viewer.run_ribbon(&ctx, vec![RibbonCommand::InsertField("NUMPAGES".into())]);
        let document = viewer.document.as_ref().unwrap();
        let Some(letter_model::Block::Paragraph(p)) = document.doc.sections[0].blocks.first()
        else {
            panic!("нет абзаца");
        };
        assert_eq!(p.plain_text(), format!("{pages}{before}"));
        let found = letter_model::fields(p);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name(), "NUMPAGES");
        // Отмена убирает поле целиком.
        viewer.run_ribbon(&ctx, vec![RibbonCommand::Undo]);
        let document = viewer.document.as_ref().unwrap();
        assert_eq!(document.doc.sections[0].blocks[0].plain_text(), before);
    }

    #[test]
    fn page_numbers_and_first_page_from_the_ribbon() {
        use crate::headers::PageNumberPlace;
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("pages.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        viewer.run_ribbon(
            &ctx,
            vec![RibbonCommand::PageNumber(PageNumberPlace::Bottom)],
        );
        assert_eq!(viewer.ribbon_info().story, Some(false));
        viewer.run_ribbon(&ctx, vec![RibbonCommand::CloseHeaderFooter]);
        let document = viewer.document.as_ref().unwrap();
        assert!(document.story.is_none());
        // Номер — на каждой странице, по центру.
        let footer = |i: usize| {
            let line = &document.layout.pages[i].footer.lines[0];
            line.text[line.line.range.clone()].to_string()
        };
        assert_eq!(footer(0), "1");
        assert_eq!(footer(1), "2");
        let line = &document.layout.pages[0].footer.lines[0];
        assert!(line.line.x > 100.0, "{}", line.line.x);

        // «Особый колонтитул для первой страницы»: на первой странице
        // нижнего колонтитула больше нет, правка остаётся открытой.
        viewer.run_ribbon(
            &ctx,
            vec![RibbonCommand::EditHeaderFooter { header: false }],
        );
        viewer.run_ribbon(&ctx, vec![RibbonCommand::TitlePage(true)]);
        let info = viewer.ribbon_info();
        assert_eq!(info.story, Some(false));
        assert!(info.title_page);
        viewer.run_ribbon(&ctx, vec![RibbonCommand::CloseHeaderFooter]);
        let document = viewer.document.as_ref().unwrap();
        assert!(document.layout.pages[0].footer.is_empty());
        assert!(!document.layout.pages[1].footer.is_empty());
        // Отмена — по шагам: флаг, потом номер страницы.
        viewer.run_ribbon(&ctx, vec![RibbonCommand::Undo]);
        let document = viewer.document.as_ref().unwrap();
        assert!(!document.layout.pages[0].footer.is_empty());
        viewer.run_ribbon(&ctx, vec![RibbonCommand::Undo]);
        let document = viewer.document.as_ref().unwrap();
        assert!(document.layout.pages[0].footer.is_empty());
    }

    #[test]
    fn page_number_format_and_even_pages_from_the_ribbon() {
        use crate::headers::PageNumberPlace;
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("pages.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        viewer.run_ribbon(
            &ctx,
            vec![RibbonCommand::PageNumber(PageNumberPlace::Bottom)],
        );
        // «Формат номеров страниц»: начать с 10, римскими.
        viewer.run_ribbon(&ctx, vec![RibbonCommand::PageNumberFormat]);
        assert!(viewer.page_number_dialog.open);
        let dialog = &mut viewer.page_number_dialog;
        dialog.restart = true;
        dialog.start = 10;
        dialog.format = 4;
        let (page, numbering) = (dialog.page, dialog.numbering());
        viewer.set_page_numbering(page, numbering);
        // Колонтитул остаётся открытым.
        assert_eq!(viewer.ribbon_info().story, Some(false));
        viewer.run_ribbon(&ctx, vec![RibbonCommand::CloseHeaderFooter]);
        let footer = |viewer: &Viewer, i: usize| {
            let document = viewer.document.as_ref().unwrap();
            let line = &document.layout.pages[i].footer.lines.first()?;
            Some(line.text[line.line.range.clone()].to_string())
        };
        assert_eq!(footer(&viewer, 0).as_deref(), Some("X"));
        assert_eq!(footer(&viewer, 1).as_deref(), Some("XI"));

        // «Разные колонтитулы для чётных и нечётных»: у чётной (10-й)
        // страницы своего нижнего колонтитула нет.
        viewer.run_ribbon(
            &ctx,
            vec![RibbonCommand::EditHeaderFooter { header: false }],
        );
        viewer.run_ribbon(&ctx, vec![RibbonCommand::EvenOddHeaders(true)]);
        assert!(viewer.ribbon_info().even_odd);
        viewer.run_ribbon(&ctx, vec![RibbonCommand::CloseHeaderFooter]);
        assert_eq!(footer(&viewer, 0), None);
        assert_eq!(footer(&viewer, 1).as_deref(), Some("XI"));
        // Флаг переживает сохранение.
        let document = viewer.document.as_ref().unwrap();
        let bytes = letter_docx::write_bytes(&document.doc).unwrap();
        let reopened = letter_docx::read_bytes(&bytes).unwrap();
        assert!(reopened.settings.even_and_odd_headers);
        assert_eq!(
            reopened.sections[0].props.page_numbering,
            document.doc.sections[0].props.page_numbering
        );
        viewer.run_ribbon(&ctx, vec![RibbonCommand::Undo]);
        assert_eq!(footer(&viewer, 0).as_deref(), Some("X"));
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

    #[test]
    fn ctrl_z_undoes_and_ctrl_y_redoes() {
        let ctrl = Modifiers::COMMAND;

        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("pages.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        let before = first_text(&mut viewer);
        let lines = viewer.document.as_ref().unwrap().layout.pages[0]
            .lines
            .len();

        let text = |t: &str| Event::Text(t.into());
        frame(
            &ctx,
            &mut viewer,
            events(vec![
                text("a"),
                text("b"),
                text(" "),
                text("c"),
                press(Key::Enter, Modifiers::NONE),
            ]),
        );
        assert_eq!(first_text(&mut viewer), "ab c");

        // Enter, потом слово «c», потом «ab ».
        frame(&ctx, &mut viewer, events(vec![press(Key::Z, ctrl)]));
        assert_eq!(first_text(&mut viewer), format!("ab c{before}"));
        assert_eq!(viewer.cursor(), Some(Position::new(0, 4)));
        frame(&ctx, &mut viewer, events(vec![press(Key::Z, ctrl)]));
        assert_eq!(first_text(&mut viewer), format!("ab {before}"));
        frame(&ctx, &mut viewer, events(vec![press(Key::Z, ctrl)]));
        assert_eq!(first_text(&mut viewer), before);
        assert_eq!(viewer.cursor(), Some(Position::default()));
        // Вёрстка тоже вернулась.
        let document = viewer.document.as_ref().unwrap();
        assert_eq!(document.layout.pages[0].lines.len(), lines);

        frame(&ctx, &mut viewer, events(vec![press(Key::Y, ctrl)]));
        assert_eq!(first_text(&mut viewer), format!("ab {before}"));
        assert_eq!(viewer.cursor(), Some(Position::new(0, 3)));
    }

    #[test]
    fn copy_cut_and_paste() {
        use crate::clipboard::tests::FakeBuffer;

        let fake = FakeBuffer::default();
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.clipboard = Clipboard::new(Box::new(fake.clone()));
        viewer.open(&fixture("pages.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        let before = first_text(&mut viewer);
        let right = || press(Key::ArrowRight, Modifiers::SHIFT);

        // Без выделения копировать нечего.
        frame(&ctx, &mut viewer, events(vec![Event::Copy]));
        assert_eq!(fake.content.borrow().0, "");

        // Выделили две буквы, скопировали — в буфере они и HTML.
        frame(
            &ctx,
            &mut viewer,
            events(vec![right(), right(), Event::Copy]),
        );
        let copied = fake.content.borrow().0.clone();
        assert_eq!(copied, before[..2]);
        assert!(fake.content.borrow().1.is_some());
        assert_eq!(first_text(&mut viewer), before);

        // Вырезали — буквы пропали; вставили дважды — их стало вдвое.
        frame(&ctx, &mut viewer, events(vec![Event::Cut]));
        assert_eq!(first_text(&mut viewer), before[2..]);
        frame(
            &ctx,
            &mut viewer,
            events(vec![
                Event::Paste(copied.clone()),
                Event::Paste(copied.clone()),
            ]),
        );
        assert_eq!(
            first_text(&mut viewer),
            format!("{copied}{copied}{}", &before[2..])
        );
        assert_eq!(viewer.cursor(), Some(Position::new(0, 4)));

        // Текст из другой программы: строки становятся абзацами.
        *fake.content.borrow_mut() = ("x\ny".into(), None);
        frame(&ctx, &mut viewer, events(vec![Event::Paste("x\ny".into())]));
        assert_eq!(viewer.cursor(), Some(Position::new(1, 1)));
        // Ctrl+Z снимает вставку целиком.
        frame(
            &ctx,
            &mut viewer,
            events(vec![press(Key::Z, Modifiers::COMMAND)]),
        );
        assert_eq!(viewer.cursor(), Some(Position::new(0, 4)));
    }

    #[test]
    fn word_shortcuts_for_styles_words_and_dialogs() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("headings.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        let style = |viewer: &mut Viewer| {
            let document = viewer.document.as_mut().unwrap();
            let paragraph = letter_editor::paragraph_mut(&mut document.doc, 0).unwrap();
            paragraph.props.style.clone()
        };
        let before = style(&mut viewer);
        let ctrl_alt = Modifiers::COMMAND | Modifiers::ALT;
        frame(&ctx, &mut viewer, events(vec![press(Key::Num1, ctrl_alt)]));
        assert_eq!(style(&mut viewer).as_deref(), Some("Heading1"));
        assert_ne!(style(&mut viewer), before);
        // Ctrl+Shift+= приходит как «+» на месте клавиши «=».
        let superscript = Event::Key {
            key: Key::Plus,
            physical_key: Some(Key::Equals),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::COMMAND | Modifiers::SHIFT,
        };
        frame(
            &ctx,
            &mut viewer,
            events(vec![
                press(Key::End, Modifiers::NONE),
                superscript,
                Event::Text("2".into()),
            ]),
        );
        let text = first_text(&mut viewer);
        assert!(text.ends_with('2'), "{text}");
        // Ctrl+Backspace стирает слово перед курсором (вместе с «2»).
        frame(
            &ctx,
            &mut viewer,
            events(vec![press(Key::Backspace, Modifiers::COMMAND)]),
        );
        assert_eq!(first_text(&mut viewer), "Document ");
        frame(
            &ctx,
            &mut viewer,
            events(vec![press(Key::D, Modifiers::COMMAND)]),
        );
        assert!(viewer.font_dialog.open);
    }

    #[test]
    fn alt_shows_key_tips() {
        use crate::keytips::Level;
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("pages.docx")).unwrap();
        viewer.ribbon.tab = crate::ribbon::Tab::View;
        frame(&ctx, &mut viewer, screen());
        let right = || press(Key::ArrowRight, Modifiers::SHIFT);
        frame(&ctx, &mut viewer, events(vec![right(), right()]));
        // Нажали и отпустили Alt — видны буквы вкладок.
        let modifiers = |m| events(vec![Event::ModifiersChanged(m)]);
        frame(&ctx, &mut viewer, modifiers(Modifiers::ALT));
        frame(&ctx, &mut viewer, modifiers(Modifiers::NONE));
        assert_eq!(viewer.ribbon.keytips.level, Some(Level::Tabs));
        // «Я» (клавиша Z) — «Главная»; набранная буква не попадает в текст.
        let before = first_text(&mut viewer);
        frame(
            &ctx,
            &mut viewer,
            events(vec![
                press(Key::Z, Modifiers::NONE),
                Event::Text("я".into()),
            ]),
        );
        assert_eq!(viewer.ribbon.tab, crate::ribbon::Tab::Home);
        assert_eq!(viewer.ribbon.keytips.level, Some(Level::Commands));
        assert_eq!(first_text(&mut viewer), before);
        frame(&ctx, &mut viewer, screen());
        assert!(crate::keytips::collected(&ctx).len() >= 25);
        // «1» — полужирный; подсказки скрываются.
        frame(
            &ctx,
            &mut viewer,
            events(vec![press(Key::Num1, Modifiers::NONE)]),
        );
        assert_eq!(viewer.ribbon.keytips.level, None);
        assert!(viewer.modified());
        // Многобуквенная: «ШР» — фокус в поле размера шрифта.
        frame(
            &ctx,
            &mut viewer,
            events(vec![press(Key::Z, Modifiers::ALT)]),
        );
        assert_eq!(viewer.ribbon.keytips.level, Some(Level::Commands));
        frame(&ctx, &mut viewer, screen());
        frame(
            &ctx,
            &mut viewer,
            events(vec![
                press(Key::I, Modifiers::NONE),
                press(Key::H, Modifiers::NONE),
            ]),
        );
        assert_eq!(viewer.ribbon.keytips.level, None);
        frame(&ctx, &mut viewer, screen());
        assert!(ctx.egui_wants_keyboard_input());
        ctx.memory_mut(|m| m.stop_text_input());
        // Esc — на уровень назад, ещё раз — выход.
        frame(
            &ctx,
            &mut viewer,
            events(vec![press(Key::Z, Modifiers::ALT)]),
        );
        frame(
            &ctx,
            &mut viewer,
            events(vec![press(Key::Escape, Modifiers::NONE)]),
        );
        assert_eq!(viewer.ribbon.keytips.level, Some(Level::Tabs));
        frame(
            &ctx,
            &mut viewer,
            events(vec![press(Key::Escape, Modifiers::NONE)]),
        );
        assert_eq!(viewer.ribbon.keytips.level, None);
    }

    #[test]
    fn ctrl_b_and_ctrl_e_format_the_selection() {
        let ctrl = Modifiers::COMMAND;

        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("pages.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        let right = || press(Key::ArrowRight, Modifiers::SHIFT);
        frame(
            &ctx,
            &mut viewer,
            events(vec![
                right(),
                right(),
                press(Key::B, ctrl),
                press(Key::E, ctrl),
            ]),
        );
        // Выделение осталось, первые две буквы полужирные, абзац по центру.
        let selection = viewer.selection().unwrap();
        assert_eq!(selection.end().offset, 2);
        let document = viewer.document.as_mut().unwrap();
        let paragraph = letter_editor::paragraph_mut(&mut document.doc, 0).unwrap();
        assert_eq!(paragraph.props.alignment, Some(Alignment::Center));
        let first = paragraph.runs().next().unwrap();
        // Заголовок образца уже полужирный (по стилю) — Ctrl+B его снял.
        assert!(first.props.bold.is_some());
        assert_eq!(first.plain_text().len(), 2);
        frame(&ctx, &mut viewer, events(vec![press(Key::Z, ctrl)]));
        let document = viewer.document.as_mut().unwrap();
        let paragraph = letter_editor::paragraph_mut(&mut document.doc, 0).unwrap();
        assert_eq!(paragraph.props.alignment, None);
    }

    #[test]
    fn new_type_save_and_reopen() {
        let dir = crate::files::tests::temp_dir("viewer-save");
        let ctx = Context::default();
        let mut viewer = Viewer::with_storage(FontLibrary::embedded_only(), Some(dir.join("app")));
        viewer.new_document();
        assert_eq!(viewer.title(), format!("{} — {APP_NAME}", tr(UNTITLED)));
        assert!(!viewer.modified());
        frame(
            &ctx,
            &mut viewer,
            events(vec![Event::Text("Привет".into())]),
        );
        assert!(viewer.modified());

        let path = dir.join("письмо.docx");
        assert!(viewer.save_to(&path));
        assert!(!viewer.modified());
        assert_eq!(viewer.title(), format!("письмо.docx — {APP_NAME}"));
        // Отмена после сохранения — снова изменён, повтор — нет.
        frame(
            &ctx,
            &mut viewer,
            events(vec![press(Key::Z, Modifiers::COMMAND)]),
        );
        assert!(viewer.modified());
        frame(
            &ctx,
            &mut viewer,
            events(vec![press(Key::Y, Modifiers::COMMAND)]),
        );
        assert!(!viewer.modified());

        // Файл открывается снова, он — в списке недавних.
        let mut other = Viewer::with_storage(FontLibrary::embedded_only(), Some(dir.join("app")));
        other.open(&path).unwrap();
        assert_eq!(first_text(&mut other), "Привет");
        assert_eq!(other.recent.items().len(), 1);
        assert!(other.recent.items()[0].ends_with("письмо.docx"));

        // Ошибка записи показывается, документ остаётся изменённым.
        frame(&ctx, &mut viewer, events(vec![Event::Text("!".into())]));
        assert!(!viewer.save_to(&dir.join("нет такой папки").join("x.docx")));
        assert!(viewer.error.is_some());
        assert!(viewer.modified());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unsaved_changes_are_asked_about() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.new_document();
        // Без изменений Ctrl+N сразу создаёт новый документ.
        frame(
            &ctx,
            &mut viewer,
            events(vec![press(Key::N, Modifiers::COMMAND)]),
        );
        assert_eq!(viewer.ask, None);

        frame(&ctx, &mut viewer, events(vec![Event::Text("x".into())]));
        frame(
            &ctx,
            &mut viewer,
            events(vec![press(Key::N, Modifiers::COMMAND)]),
        );
        assert_eq!(viewer.ask, Some(Intent::New));
        // Пока висит вопрос, набор не попадает в документ.
        frame(&ctx, &mut viewer, events(vec![Event::Text("y".into())]));
        assert_eq!(first_text(&mut viewer), "x");
        viewer.answer(Intent::New, Answer::Cancel);
        assert_eq!(first_text(&mut viewer), "x");

        // Крестик окна: закрытие отменяется и задаётся вопрос.
        let mut input = screen();
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .events
            .push(egui::ViewportEvent::Close);
        frame(&ctx, &mut viewer, input);
        assert_eq!(viewer.ask, Some(Intent::Close));
        assert!(!viewer.close_confirmed);
        viewer.answer(Intent::Close, Answer::Discard);
        assert!(viewer.close_confirmed);

        // «Не сохранять» при создании — новый пустой документ.
        viewer.answer(Intent::New, Answer::Discard);
        assert_eq!(first_text(&mut viewer), "");
        assert!(!viewer.modified());
    }

    #[test]
    fn documents_left_after_a_crash_can_be_recovered() {
        let dir = crate::files::tests::temp_dir("viewer-recover");
        let app = dir.join("app");
        let autosave = app.join("autosave");
        std::fs::create_dir_all(&autosave).unwrap();
        let mut doc = crate::files::blank();
        doc.sections[0].blocks = vec![letter_model::Paragraph::text("спасено").into()];
        std::fs::write(
            autosave.join("1-1.docx"),
            letter_docx::write_bytes(&doc).unwrap(),
        )
        .unwrap();
        let original = dir.join("отчёт.docx");
        std::fs::write(autosave.join("1-1.path"), original.display().to_string()).unwrap();
        std::fs::write(autosave.join("1-1.lock"), "").unwrap();

        let mut viewer = Viewer::with_storage(FontLibrary::embedded_only(), Some(app.clone()));
        assert_eq!(viewer.recovered.len(), 1);
        viewer.request(Intent::Recover(0));
        assert!(viewer.recovered.is_empty());
        assert_eq!(first_text(&mut viewer), "спасено");
        assert!(viewer.modified());
        assert_eq!(viewer.title(), format!("отчёт.docx — {APP_NAME}"));
        // Старая копия убрана, своя — записана.
        assert!(!autosave.join("1-1.docx").exists());
        assert!(
            std::fs::read_dir(&autosave)
                .unwrap()
                .flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == "docx"))
        );
        // Сохранили — своя копия больше не нужна.
        assert!(viewer.save_to(&original));
        assert!(
            !std::fs::read_dir(&autosave)
                .unwrap()
                .flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == "docx"))
        );
        drop(viewer);
        assert_eq!(std::fs::read_dir(&autosave).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn backstage_commands() {
        let dir = crate::files::tests::temp_dir("backstage");
        let ctx = Context::default();
        let mut viewer = Viewer::with_storage(FontLibrary::embedded_only(), Some(dir.clone()));
        viewer.new_document();
        viewer.run_ribbon(&ctx, vec![RibbonCommand::Backstage]);
        assert!(viewer.backstage.open);
        // Пока открыт «Файл», клавиши не идут в документ.
        frame(&ctx, &mut viewer, events(vec![Event::Text("x".into())]));
        assert_eq!(first_text(&mut viewer), "");
        let info = viewer.backstage_info();
        assert_eq!(info.document.map(|d| d.pages), Some(1));
        let settings = Settings {
            autosave_minutes: 7,
            show_ruler: false,
            theme: ThemeChoice::Dark,
            language: crate::i18n::Language::English,
        };
        viewer.run_ribbon(&ctx, vec![RibbonCommand::ChangeSettings(settings)]);
        assert!(!viewer.show_ruler);
        frame(&ctx, &mut viewer, screen());
        assert_eq!(ctx.theme(), egui::Theme::Dark);
        assert_eq!(crate::i18n::language(), crate::i18n::Language::English);
        assert_eq!(viewer.title(), "New document — letter");
        crate::i18n::set_language(crate::i18n::Language::Russian);
        assert_eq!(Settings::load(Some(&dir)), settings);
        viewer.run_ribbon(&ctx, vec![RibbonCommand::CloseDocument]);
        assert!(viewer.document.is_none());
        // Новый запуск помнит параметры.
        let again = Viewer::with_storage(FontLibrary::embedded_only(), Some(dir));
        assert!(!again.show_ruler);
    }

    #[test]
    fn find_and_replace() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.new_document();
        frame(&ctx, &mut viewer, screen());
        viewer.apply_actions(
            &ctx,
            vec![Action::Edit(Command::Type("кот и кот, Кот".into()))],
        );
        frame(
            &ctx,
            &mut viewer,
            events(vec![press(Key::H, Modifiers::COMMAND)]),
        );
        assert!(viewer.find.open && viewer.find.replace);
        viewer.find.query = "кот".into();
        viewer.find.replacement = "пёс".into();
        viewer.find_action(&ctx, FindAction::Next);
        let at = |o| Position::new(0, o);
        assert_eq!(viewer.selection(), Some(Selection::new(at(0), at(6))));
        // Курсор после первого — находится второй.
        viewer.find_action(&ctx, FindAction::Next);
        assert_eq!(viewer.selection(), Some(Selection::new(at(10), at(16))));
        viewer.find_action(&ctx, FindAction::Replace);
        assert_eq!(first_text(&mut viewer), "кот и пёс, Кот");
        assert_eq!(viewer.selection(), Some(Selection::new(at(18), at(24))));
        viewer.find.options.match_case = true;
        viewer.find_action(&ctx, FindAction::ReplaceAll);
        assert_eq!(first_text(&mut viewer), "пёс и пёс, Кот");
        assert_eq!(viewer.find.message.as_deref(), Some("Выполнено замен: 1."));
        viewer.run_ribbon(&ctx, vec![RibbonCommand::Undo]);
        assert_eq!(first_text(&mut viewer), "кот и пёс, Кот");
        viewer.find.query = "собака".into();
        viewer.find_action(&ctx, FindAction::Next);
        assert_eq!(viewer.find.message.as_deref(), Some("Ничего не найдено."));
    }

    #[test]
    fn ribbon_commands_reach_the_document() {
        use crate::clipboard::tests::FakeBuffer;

        let fake = FakeBuffer::default();
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.clipboard = Clipboard::new(Box::new(fake.clone()));
        viewer.new_document();
        frame(&ctx, &mut viewer, screen());
        // Линейки видны и показывают первый лист.
        assert!(viewer.show_ruler);
        assert_eq!(viewer.ruler_page.map(|(page, _)| page), Some(0));

        *fake.content.borrow_mut() = ("слово".into(), None);
        viewer.run_ribbon(&ctx, vec![RibbonCommand::Paste]);
        assert_eq!(first_text(&mut viewer), "слово");
        viewer.run_ribbon(&ctx, vec![RibbonCommand::PasteSpecial(PasteMode::TextOnly)]);
        assert_eq!(first_text(&mut viewer), "словослово");
        viewer.run_ribbon(&ctx, vec![RibbonCommand::Undo]);
        let info = viewer.ribbon_info();
        assert!(info.can_undo && !info.bold && !info.has_selection);

        frame(
            &ctx,
            &mut viewer,
            events(vec![press(Key::A, Modifiers::COMMAND)]),
        );
        viewer.run_ribbon(
            &ctx,
            vec![
                RibbonCommand::Format(RunFormat::Bold),
                RibbonCommand::Align(Alignment::Center),
            ],
        );
        let info = viewer.ribbon_info();
        assert!(info.bold && info.has_selection);
        assert_eq!(info.alignment, Some(Alignment::Center));

        viewer.run_ribbon(&ctx, vec![RibbonCommand::Undo, RibbonCommand::Undo]);
        assert!(!viewer.ribbon_info().bold);

        viewer.run_ribbon(
            &ctx,
            vec![
                RibbonCommand::Format(RunFormat::Font("Arial".into())),
                RibbonCommand::Format(RunFormat::Size(letter_model::HalfPoints(28))),
                RibbonCommand::Format(RunFormat::Superscript),
            ],
        );
        let info = viewer.ribbon_info();
        assert_eq!((info.font.as_str(), info.size), ("Arial", 28));
        assert!(info.superscript && !info.subscript);
        assert!(info.fonts.iter().any(|f| f == "Times New Roman"));
        // Ctrl+Пробел убирает оформление.
        frame(
            &ctx,
            &mut viewer,
            events(vec![press(Key::Space, Modifiers::COMMAND)]),
        );
        assert!(!viewer.ribbon_info().superscript);

        viewer.run_ribbon(&ctx, vec![RibbonCommand::ToggleRuler]);
        assert!(!viewer.show_ruler);
        viewer.run_ribbon(&ctx, vec![RibbonCommand::PageWidth]);
        assert_eq!(viewer.zoom, Zoom::PageWidth);
    }

    #[test]
    fn status_bar_counts_words_and_shows_the_language() {
        use egui_kittest::kittest::Queryable;
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.new_document();
        {
            let document = viewer.document.as_mut().unwrap();
            document.doc.styles.defaults.run.unknown =
                vec![letter_model::RawXml::new(r#"<w:lang w:val="ru-RU"/>"#)];
            letter_editor::insert_text(&mut document.doc, Position::default(), "раз два три");
        }
        let mut h = egui_kittest::Harness::builder()
            .with_size(vec2(1200.0, 800.0))
            .build_ui_state(|ui, viewer: &mut Viewer| viewer.ui(ui), viewer);
        h.run();
        h.get_by_label("Страница 1 из 1");
        h.get_by_label("русский");
        h.get_by_label("Число слов: 3").click();
        h.run();
        assert!(h.state().statistics_open);
        h.get_by_label("Знаков (с пробелами)");
        h.key_press(Key::Escape);
        h.run();
        assert!(!h.state().statistics_open);
        // Выделено одно слово.
        let document = h.state_mut().document.as_mut().unwrap();
        document.anchor = Position::new(0, 0);
        document.cursor = Position::new(0, "раз".len());
        h.run();
        h.get_by_label("Число слов: 1 из 3");
        h.get_by_label("Разметка страницы");
        h.get_by_label("Режим чтения");
    }

    #[test]
    fn outline_collapses_and_changes_levels() {
        use letter_editor::OutlineChange;
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("headings.docx")).unwrap();
        viewer.run_ribbon(&ctx, vec![RibbonCommand::ViewMode(ViewMode::Outline)]);
        frame(&ctx, &mut viewer, screen());
        frame(&ctx, &mut viewer, screen());
        let hidden = |viewer: &Viewer| viewer.document.as_ref().unwrap().outline_hidden.clone();
        assert!(hidden(&viewer).iter().all(|h| !h));

        // Свернуть «Heading 1» (курсор в тексте под ним): всё ниже скрыто.
        let document = viewer.document.as_mut().unwrap();
        document.set_selection(Selection::caret(Position::new(2, 3)));
        viewer.run_ribbon(&ctx, vec![RibbonCommand::OutlineExpand(false)]);
        assert_eq!(
            hidden(&viewer),
            [false, false, true, true, true, true, true]
        );
        // Курсор ушёл из скрытого абзаца в конец заголовка.
        let document = viewer.document.as_ref().unwrap();
        assert_eq!(document.cursor, Position::new(1, "Heading 1".len()));
        viewer.run_ribbon(&ctx, vec![RibbonCommand::OutlineExpand(true)]);
        assert!(hidden(&viewer).iter().all(|h| !h));

        // «Показать уровень 2»: без основного текста и третьего уровня.
        viewer.run_ribbon(&ctx, vec![RibbonCommand::ShowLevel(Some(2))]);
        assert_eq!(viewer.ribbon_info().show_level, Some(2));
        assert_eq!(
            hidden(&viewer),
            [true, false, true, false, true, true, true]
        );
        viewer.run_ribbon(&ctx, vec![RibbonCommand::ShowLevel(None)]);

        // Понизить «Heading 1» до второго уровня и вернуть.
        let document = viewer.document.as_mut().unwrap();
        document.set_selection(Selection::caret(Position::new(1, 0)));
        let level = |viewer: &Viewer| viewer.document.as_ref().unwrap().outline[1];
        viewer.run_ribbon(
            &ctx,
            vec![RibbonCommand::Edit(Command::OutlineLevel(
                OutlineChange::Demote,
            ))],
        );
        assert_eq!(level(&viewer), Some(2));
        frame(
            &ctx,
            &mut viewer,
            key(Key::ArrowLeft, Modifiers::ALT | Modifiers::SHIFT),
        );
        assert_eq!(level(&viewer), Some(1));

        // «Закрыть режим структуры».
        viewer.run_ribbon(&ctx, vec![RibbonCommand::ViewMode(ViewMode::Print)]);
        frame(&ctx, &mut viewer, screen());
        assert!(viewer.document.as_ref().unwrap().outline_hidden.is_empty());
    }

    #[test]
    fn view_modes() {
        let ctx = Context::default();
        let mut viewer = Viewer::new(FontLibrary::embedded_only());
        viewer.open(&fixture("headings.docx")).unwrap();
        frame(&ctx, &mut viewer, screen());
        let print_width = viewer.document.as_ref().unwrap().layout.pages[0].width;

        // Веб-документ: «лист» шириной в окно, без колонтитулов.
        viewer.run_ribbon(&ctx, vec![RibbonCommand::ViewMode(ViewMode::Web)]);
        frame(&ctx, &mut viewer, screen());
        frame(&ctx, &mut viewer, screen());
        let page = &viewer.document.as_ref().unwrap().layout.pages[0];
        assert!(page.width > print_width, "{} vs {print_width}", page.width);
        assert!((page.margins.left - WEB_MARGIN).abs() < 0.1);
        // Текст по-прежнему правится — в самом документе.
        frame(&ctx, &mut viewer, events(vec![Event::Text("Z".into())]));
        assert!(first_text(&mut viewer).starts_with('Z'));
        let document = viewer.document.as_ref().unwrap();
        assert!(document.layout.pages[0].lines[0].text.starts_with('Z'));
        assert_eq!(document.doc.sections[0].props.margins, {
            let doc = letter_docx::read_file(fixture("headings.docx")).unwrap();
            doc.sections[0].props.margins
        });

        // Структура: заголовки второго уровня с отступом, у абзацев — значки.
        frame(
            &ctx,
            &mut viewer,
            key(Key::O, Modifiers::COMMAND | Modifiers::ALT),
        );
        assert_eq!(viewer.view_mode, ViewMode::Outline);
        let plain = frame(&ctx, &mut viewer, screen());
        let document = viewer.document.as_ref().unwrap();
        assert!(document.outline.iter().any(Option::is_some));
        let lines = &document.layout.pages[0].lines;
        let xs: Vec<f32> = lines.iter().map(|l| l.line.x).collect();
        assert!(xs.iter().any(|&x| x > 1.0), "{xs:?}");

        // Черновик: вёрстка по листам, как в разметке страницы.
        viewer.run_ribbon(&ctx, vec![RibbonCommand::ViewMode(ViewMode::Draft)]);
        let draft = frame(&ctx, &mut viewer, screen());
        let page = &viewer.document.as_ref().unwrap().layout.pages[0];
        assert_eq!(page.width, print_width);
        assert!(draft > 0 && plain > 0);

        // Режим чтения: ленты нет, текст не правится, Esc — обратно.
        viewer.run_ribbon(&ctx, vec![RibbonCommand::ViewMode(ViewMode::Read)]);
        frame(&ctx, &mut viewer, screen());
        let before = first_text(&mut viewer);
        frame(&ctx, &mut viewer, events(vec![Event::Text("Q".into())]));
        assert_eq!(first_text(&mut viewer), before);
        let zoom = viewer.zoom();
        assert!(zoom < 1.0, "{zoom}");
        frame(&ctx, &mut viewer, key(Key::Escape, Modifiers::NONE));
        assert_eq!(viewer.view_mode, ViewMode::Print);
        frame(&ctx, &mut viewer, screen());
        let page = &viewer.document.as_ref().unwrap().layout.pages[0];
        assert_eq!(page.width, print_width);
    }
}
