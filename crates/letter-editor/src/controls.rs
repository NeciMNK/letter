//! Элементы управления содержимым внутри абзаца: флажки, списки, даты,
//! текст-подсказка и запрет правки.

use std::sync::Arc;

use letter_model::{
    Color, ContentControl, ContentControlRef, ControlKind, ControlLock, Document, Inline,
    Paragraph, RawXml, Run, RunContent, RunProps, StyleKind,
};

use crate::edit::{
    clamp_offset, item_len, paragraph_mut, paragraph_ref, plain_text, split_content, text_len, tidy,
};
use crate::fields::{DateTime, format_date};
use crate::position::Position;

/// Элемент внутри абзаца у места `at` и где он: начало и конец (в одном
/// абзаце). Смотрим знак после места, а в конце элемента — знак перед ним.
pub fn control_at(doc: &Document, at: Position) -> Option<(ContentControlRef, Position, Position)> {
    let paragraph = paragraph_ref(doc, at.paragraph)?;
    let segments = segments(paragraph);
    let index = segments
        .iter()
        .position(|(a, b, c)| *a <= at.offset && at.offset < *b && c.is_some())
        .or_else(|| {
            segments
                .iter()
                .position(|(a, b, c)| *a <= at.offset && at.offset == *b && c.is_some())
        })?;
    let control = segments[index].2.clone()?;
    let same = |s: &(usize, usize, Option<ContentControlRef>)| {
        s.2.as_ref().is_some_and(|c| Arc::ptr_eq(c, &control))
    };
    let mut first = index;
    while first > 0 && same(&segments[first - 1]) {
        first -= 1;
    }
    let mut last = index;
    while segments.get(last + 1).is_some_and(same) {
        last += 1;
    }
    let start = Position::new(at.paragraph, segments[first].0);
    let end = Position::new(at.paragraph, segments[last].1);
    Some((control, start, end))
}

/// Элемент вокруг места `at`: внутри абзаца (см. [`control_at`]), а если
/// такого нет — самый внутренний элемент из целых абзацев (блочный), от
/// начала первого его абзаца до конца последнего.
pub fn control_range(
    doc: &Document,
    at: Position,
) -> Option<(ContentControlRef, Position, Position)> {
    if let Some(found) = control_at(doc, at) {
        return Some(found);
    }
    let control = paragraph_ref(doc, at.paragraph)?
        .props
        .controls
        .last()?
        .clone();
    let (start, end) = block_range(doc, at.paragraph, &control);
    Some((control, start, end))
}

/// Удалит ли правка текста от `start` до `end` (для Backspace и Delete
/// без выделения — `forward` и знак рядом) целиком элемент, который
/// запрещено удалять (`w:lock` — `sdtLocked` или `sdtContentLocked`).
/// Тогда правку не выполняем, как Word.
pub fn removes_locked_control(
    doc: &Document,
    start: Position,
    end: Position,
    forward: Option<bool>,
) -> bool {
    let (start, end) = match forward {
        Some(forward) if start == end => {
            let Some(paragraph) = paragraph_ref(doc, start.paragraph) else {
                return false;
            };
            let text = plain_text(paragraph);
            let len = if forward {
                text.get(start.offset..)
                    .and_then(|t| t.chars().next())
                    .map_or(0, char::len_utf8)
            } else {
                text.get(..start.offset)
                    .and_then(|t| t.chars().next_back())
                    .map_or(0, char::len_utf8)
            };
            if forward {
                (start, Position::new(start.paragraph, start.offset + len))
            } else {
                (Position::new(start.paragraph, start.offset - len), start)
            }
        }
        _ => (start, end),
    };
    if start == end {
        return false;
    }
    let locked =
        |c: &ContentControlRef| matches!(c.lock(), ControlLock::Control | ControlLock::Both);
    let before = |a: Position, b: Position| (a.paragraph, a.offset) <= (b.paragraph, b.offset);
    for index in start.paragraph..=end.paragraph {
        let Some(paragraph) = paragraph_ref(doc, index) else {
            continue;
        };
        // Элемент внутри абзаца целиком попал в удаляемое.
        for (a, b, control) in segments(paragraph) {
            if a == b || !control.as_ref().is_some_and(locked) {
                continue;
            }
            if let Some((_, from, to)) = control_at(doc, Position::new(index, a))
                && before(start, from)
                && before(to, end)
            {
                return true;
            }
        }
        // Блочный элемент целиком попал в удаляемое.
        for control in paragraph.props.controls.iter().filter(|c| locked(c)) {
            let (from, to) = block_range(doc, index, control);
            if before(start, from) && before(to, end) {
                return true;
            }
        }
    }
    false
}

/// Абзацы блочного элемента `control`, в который входит абзац `index`.
fn block_range(doc: &Document, index: usize, control: &ContentControlRef) -> (Position, Position) {
    let has = |i: usize| {
        paragraph_ref(doc, i)
            .is_some_and(|p| p.props.controls.iter().any(|c| Arc::ptr_eq(c, control)))
    };
    let mut first = index;
    while first > 0 && has(first - 1) {
        first -= 1;
    }
    let mut last = index;
    while has(last + 1) {
        last += 1;
    }
    let end = paragraph_ref(doc, last).map_or(0, text_len);
    (Position::new(first, 0), Position::new(last, end))
}

/// Вид нового элемента управления (вкладка «Разработчик»).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NewControl {
    /// Форматированный текст.
    RichText,
    /// Обычный текст.
    PlainText,
    /// Флажок.
    CheckBox,
    /// Поле со списком (можно вписать своё).
    ComboBox,
    /// Раскрывающийся список.
    DropDownList,
    /// Выбор даты.
    Date,
}

/// Новый элемент управления: вид, текст-подсказка и язык (формат даты:
/// русский — `dd.MM.yyyy`, иначе `M/d/yyyy`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ControlSetup {
    /// Вид.
    pub kind: NewControl,
    /// Текст-подсказка («Место для ввода текста.»).
    pub placeholder: String,
    /// Русский язык.
    pub russian: bool,
}

/// Пространства имён свойств нового элемента (свойства пишутся в файл
/// как есть).
const CONTROL_NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml""#;

/// Текст для значения атрибута XML.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Шрифт флажка, как у Word.
const CHECKBOX_FONT: &str = "MS Gothic";

impl ControlSetup {
    /// Свойства элемента (`w:sdtPr`); `placeholder` — показывает подсказку.
    fn props(&self, id: u32, placeholder: bool) -> String {
        let shown = if placeholder {
            "<w:showingPlcHdr/>"
        } else {
            ""
        };
        let item = format!(
            r#"<w:listItem w:displayText="{}" w:value=""/>"#,
            escape(&self.placeholder)
        );
        let kind = match self.kind {
            NewControl::RichText => String::new(),
            NewControl::PlainText => "<w:text/>".into(),
            NewControl::CheckBox => format!(
                r#"<w14:checkbox><w14:checked w14:val="0"/><w14:checkedState w14:val="2612" w14:font="{CHECKBOX_FONT}"/><w14:uncheckedState w14:val="2610" w14:font="{CHECKBOX_FONT}"/></w14:checkbox>"#
            ),
            NewControl::ComboBox => format!("<w:comboBox>{item}</w:comboBox>"),
            NewControl::DropDownList => format!("<w:dropDownList>{item}</w:dropDownList>"),
            NewControl::Date => {
                let (format, lid) = if self.russian {
                    ("dd.MM.yyyy", "ru-RU")
                } else {
                    ("M/d/yyyy", "en-US")
                };
                format!(
                    r#"<w:date><w:dateFormat w:val="{format}"/><w:lid w:val="{lid}"/><w:storeMappedDataAs w:val="dateTime"/><w:calendar w:val="gregorian"/></w:date>"#
                )
            }
        };
        let shown = if self.kind == NewControl::CheckBox {
            ""
        } else {
            shown
        };
        format!(r#"<w:sdtPr {CONTROL_NS}><w:id w:val="{id}"/>{shown}{kind}</w:sdtPr>"#)
    }
}

/// Вставляет новый элемент у `start`. Если выделен текст `start..end`
/// в одном абзаце, текстовый элемент охватывает его (как в Word); иначе
/// элемент вставляется с текстом-подсказкой (флажок — со знаком ☐).
/// Внутри другого элемента ничего не вставляется. Возвращает место
/// курсора: начало элемента (набор заменит подсказку), у флажка — за ним.
pub fn insert_control(
    doc: &mut Document,
    start: Position,
    end: Position,
    setup: &ControlSetup,
) -> Position {
    if control_range(doc, start).is_some() || control_range(doc, end).is_some() {
        return start;
    }
    let id = new_control_id(doc, start);
    let text_kind = matches!(setup.kind, NewControl::RichText | NewControl::PlainText);
    if text_kind && start.paragraph == end.paragraph && start.offset < end.offset {
        let control = Arc::new(ContentControl {
            props: Some(RawXml::new(setup.props(id, false))),
            end_props: None,
        });
        if let Some(paragraph) = paragraph_mut(doc, start.paragraph) {
            crate::format::apply(paragraph, start.offset, end.offset, |props| {
                props.control = Some(control.clone());
            });
        }
        return end;
    }
    let checkbox = setup.kind == NewControl::CheckBox;
    let style = (!checkbox).then(|| placeholder_style(doc));
    let control = Arc::new(ContentControl {
        props: Some(RawXml::new(setup.props(id, !checkbox))),
        end_props: None,
    });
    let Some(paragraph) = paragraph_mut(doc, start.paragraph) else {
        return start;
    };
    let offset = clamp_offset(paragraph, start.offset);
    let mut props = props_before(paragraph, offset);
    props.control = Some(control);
    let text = if checkbox {
        let font = Some(CHECKBOX_FONT.to_string());
        props.fonts.ascii = font.clone();
        props.fonts.h_ansi = font.clone();
        props.fonts.east_asia = font;
        "☐".to_string()
    } else {
        props.style = style;
        setup.placeholder.clone()
    };
    let len = text.len();
    let tail = split_content(paragraph, offset);
    paragraph.content.push(Inline::Run(Run {
        props,
        content: vec![RunContent::Text(text)],
    }));
    paragraph.content.extend(tail);
    tidy(paragraph);
    let at = if checkbox { offset + len } else { offset };
    Position::new(start.paragraph, at)
}

/// Оформление фрагмента перед местом `offset` (без элемента и стиля
/// знака) — новый текст выглядит как текст вокруг.
fn props_before(paragraph: &Paragraph, offset: usize) -> RunProps {
    let mut pos = 0;
    let mut found = None;
    for run in paragraph.runs() {
        if found.is_none() || pos < offset {
            found = Some(&run.props);
        }
        pos += run.content.iter().map(item_len).sum::<usize>();
        if pos >= offset {
            break;
        }
    }
    let mut props = found.cloned().unwrap_or_default();
    props.control = None;
    props.style = None;
    props
}

/// Стиль текста-подсказки «Placeholder Text» (серый, как в Word);
/// добавляется в документ, если его нет.
fn placeholder_style(doc: &mut Document) -> String {
    crate::notes::ensure_style(
        doc,
        "PlaceholderText",
        "Placeholder Text",
        StyleKind::Character,
        |style| style.run.color = Some(Color::Rgb(0x66, 0x66, 0x66)),
    )
}

/// Номер нового элемента (`w:id`): по месту и тексту документа, чтобы
/// разные элементы не совпадали.
fn new_control_id(doc: &Document, at: Position) -> u32 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (at.paragraph, at.offset).hash(&mut hasher);
    doc.plain_text().len().hash(&mut hasher);
    (hasher.finish() as u32) & 0x7fff_ffff
}

/// Фрагменты абзаца: начало, конец (байты) и элемент управления.
fn segments(paragraph: &Paragraph) -> Vec<(usize, usize, Option<ContentControlRef>)> {
    let mut out = Vec::new();
    let mut offset = 0;
    for run in paragraph.runs() {
        let len: usize = run.content.iter().map(item_len).sum();
        out.push((offset, offset + len, run.props.control.clone()));
        offset += len;
    }
    out
}

/// Показывает ли элемент у места `at` текст-подсказку: тогда набранный
/// текст заменяет её (см. [`set_control_text`]).
pub fn placeholder_at(doc: &Document, at: Position) -> bool {
    control_range(doc, at).is_some_and(|(c, _, _)| c.showing_placeholder())
}

/// Все элементы управления документа (в строке и блочные) и где они, по
/// порядку — для «Режима конструктора».
pub fn controls_in(doc: &Document) -> Vec<(ContentControlRef, Position, Position)> {
    let mut out: Vec<(ContentControlRef, Position, Position)> = Vec::new();
    let seen = |out: &[(ContentControlRef, Position, Position)], c: &ContentControlRef| {
        out.iter().any(|(o, _, _)| Arc::ptr_eq(o, c))
    };
    for index in 0..crate::edit::paragraph_count(doc) {
        let Some(paragraph) = paragraph_ref(doc, index) else {
            continue;
        };
        for control in &paragraph.props.controls {
            if !seen(&out, control) {
                let (start, end) = block_range(doc, index, control);
                out.push((control.clone(), start, end));
            }
        }
        for (start, end, control) in segments(paragraph) {
            if let Some(control) = control
                && start < end
                && !seen(&out, &control)
                && let Some(found) = control_at(doc, Position::new(index, start))
            {
                out.push(found);
            }
        }
    }
    out
}

/// Тексты-подсказки, которые снова появляются в элементе, когда его
/// содержимое стёрли (как в Word). По умолчанию — английские тексты Word;
/// программа ставит их на языке интерфейса.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceholderTexts {
    /// У текстовых элементов.
    pub text: String,
    /// У списков.
    pub list: String,
    /// У выбора даты.
    pub date: String,
}

impl Default for PlaceholderTexts {
    fn default() -> PlaceholderTexts {
        PlaceholderTexts {
            text: "Click or tap here to enter text.".into(),
            list: "Choose an item.".into(),
            date: "Click or tap to enter a date.".into(),
        }
    }
}

impl PlaceholderTexts {
    /// Подсказка элемента вида `kind`; у флажка, рисунка и блока из
    /// коллекции подсказки нет.
    fn for_kind(&self, kind: &ControlKind) -> Option<&str> {
        match kind {
            ControlKind::CheckBox { .. } | ControlKind::Picture | ControlKind::Gallery => None,
            ControlKind::List { .. } => Some(&self.list),
            ControlKind::Date { .. } => Some(&self.date),
            ControlKind::RichText | ControlKind::Text => Some(&self.text),
        }
    }
}

/// Стоит ли `a` раньше `b`.
fn earlier(a: Position, b: Position) -> bool {
    (a.paragraph, a.offset) < (b.paragraph, b.offset)
}

/// Элемент, внутри которого целиком лежит правка `start..end` (у
/// Backspace и Delete без выделения — стираемый знак: `forward` —
/// направление), и стоит ли он внутри абзаца (а не блочный).
pub(crate) fn enclosing_control(
    doc: &Document,
    start: Position,
    end: Position,
    forward: Option<bool>,
) -> Option<(ContentControlRef, bool)> {
    let (control, from, to) = control_range(doc, start)?;
    let inside = if start != end {
        !earlier(start, from) && !earlier(to, end)
    } else {
        match forward {
            Some(false) => earlier(from, start) && !earlier(to, start),
            Some(true) => !earlier(start, from) && earlier(start, to),
            None => false,
        }
    };
    let inline = control_at(doc, start).is_some_and(|(c, _, _)| Arc::ptr_eq(&c, &control));
    inside.then_some((control, inline))
}

/// После правки внутри элемента `control` (см. [`enclosing_control`]):
/// если стёрто всё его содержимое, элемент не пропадает, а снова
/// показывает подсказку у места `at`; набранное на месте стёртого
/// (`at..typed_end`) остаётся внутри элемента.
pub(crate) fn keep_control(
    doc: &mut Document,
    (control, inline): (ContentControlRef, bool),
    at: Position,
    typed_end: Position,
    texts: &PlaceholderTexts,
) {
    let typed = at.paragraph == typed_end.paragraph && at.offset < typed_end.offset;
    let Some(paragraph) = paragraph_ref(doc, at.paragraph) else {
        return;
    };
    let same = |c: &ContentControlRef| Arc::ptr_eq(c, &control);
    if inline {
        // Пустые фрагменты после стирания не в счёт.
        if paragraph.runs().any(|r| {
            r.props.control.as_ref().is_some_and(same)
                && r.content.iter().map(item_len).sum::<usize>() > 0
        }) {
            return;
        }
        if typed {
            let kept = Arc::new(control.without_placeholder());
            if let Some(paragraph) = paragraph_mut(doc, at.paragraph) {
                crate::format::apply(paragraph, at.offset, typed_end.offset, |props| {
                    props.control = Some(kept.clone());
                    if is_placeholder_style(props.style.as_deref()) {
                        props.style = None;
                    }
                });
            }
            return;
        }
        insert_placeholder(doc, at, &control, texts);
        return;
    }
    // Блочный элемент остаётся в абзаце, но абзац опустел.
    if typed || text_len(paragraph) > 0 || !paragraph.props.controls.iter().any(same) {
        return;
    }
    insert_placeholder(doc, at, &control, texts);
}

/// Вставляет подсказку элемента `control` у места `at` и помечает элемент
/// «показывает подсказку» (у блочного — в свойствах абзаца).
fn insert_placeholder(
    doc: &mut Document,
    at: Position,
    control: &ContentControlRef,
    texts: &PlaceholderTexts,
) {
    let Some(text) = texts.for_kind(&control.kind()).map(str::to_string) else {
        return;
    };
    let style = placeholder_style(doc);
    let shown = Arc::new(control.with_placeholder());
    let Some(paragraph) = paragraph_mut(doc, at.paragraph) else {
        return;
    };
    let mut block = false;
    for c in &mut paragraph.props.controls {
        if Arc::ptr_eq(c, control) {
            *c = shown.clone();
            block = true;
        }
    }
    let offset = clamp_offset(paragraph, at.offset);
    let mut props = props_before(paragraph, offset);
    props.style = Some(style);
    if !block {
        props.control = Some(shown);
    }
    let tail = split_content(paragraph, offset);
    paragraph.content.push(Inline::Run(Run {
        props,
        content: vec![RunContent::Text(text)],
    }));
    paragraph.content.extend(tail);
    tidy(paragraph);
}

/// Стиль подсказки («Placeholder Text»).
fn is_placeholder_style(style: Option<&str>) -> bool {
    style.is_some_and(|s| s.eq_ignore_ascii_case("PlaceholderText"))
}

/// Можно ли править текст от `start` до `end`: ни он, ни абзацы вокруг
/// не лежат в элементе с запретом правки содержимого.
pub fn editable(doc: &Document, start: Position, end: Position) -> bool {
    for index in start.paragraph..=end.paragraph {
        let Some(paragraph) = paragraph_ref(doc, index) else {
            continue;
        };
        if paragraph
            .props
            .controls
            .iter()
            .any(|c| !c.content_editable())
        {
            return false;
        }
        let from = if index == start.paragraph {
            start.offset
        } else {
            0
        };
        let to = if index == end.paragraph {
            end.offset
        } else {
            usize::MAX
        };
        let locked = segments(paragraph).into_iter().any(|(a, b, c)| {
            c.is_some_and(|c| !c.content_editable())
                && (a < to && from < b || a <= from && from <= b && from == to)
        });
        if locked {
            return false;
        }
    }
    true
}

/// Заменяет текст элемента у места `at` на `text` (оформление — как у
/// первого фрагмента, без стиля подсказки) и снимает пометку
/// «показывает подсказку». Возвращает место после текста; `None`, если
/// элемента нет или его содержимое менять нельзя.
pub fn set_control_text(doc: &mut Document, at: Position, text: &str) -> Option<Position> {
    replace_control(doc, at, text, ContentControl::without_placeholder)
}

/// Ставит или снимает флажок у места `at`. Возвращает место после флажка;
/// `None`, если там не флажок.
pub fn toggle_checkbox(doc: &mut Document, at: Position) -> Option<Position> {
    let (control, _, _) = control_at(doc, at)?;
    let ControlKind::CheckBox { checked, symbols } = control.kind() else {
        return None;
    };
    let symbol = if checked {
        symbols.unchecked
    } else {
        symbols.checked
    };
    replace_control(doc, at, &symbol.to_string(), |c| {
        c.without_placeholder().with_checked(!checked)
    })
}

/// Выбирает пункт `index` списка у места `at`.
pub fn choose_list_item(doc: &mut Document, at: Position, index: usize) -> Option<Position> {
    let (control, _, _) = control_at(doc, at)?;
    let ControlKind::List { items, .. } = control.kind() else {
        return None;
    };
    let item = items.get(index)?;
    set_control_text(doc, at, &item.text)
}

/// Ставит дату `date` в элемент выбора даты у места `at`: текст по образцу
/// элемента (без образца — `dd.MM.yyyy`) и полную дату в свойствах.
pub fn set_control_date(doc: &mut Document, at: Position, date: DateTime) -> Option<Position> {
    let (control, _, _) = control_at(doc, at)?;
    let ControlKind::Date { format } = control.kind() else {
        return None;
    };
    let russian = control
        .property("lid")
        .is_none_or(|lid| lid.to_ascii_lowercase().starts_with("ru"));
    let text = format_date(format.as_deref().unwrap_or("dd.MM.yyyy"), date, russian);
    let full = format!(
        "{:04}-{:02}-{:02}T00:00:00Z",
        date.year, date.month, date.day
    );
    replace_control(doc, at, &text, |c| {
        c.without_placeholder().with_full_date(&full)
    })
}

/// Меняет свойства элемента у места `at` (внутри абзаца или блочного) на
/// `new` — окно «Свойства элемента». У даты с выбранным числом текст
/// переписывается по новому образцу. `false`, если элемента нет.
pub fn set_control(doc: &mut Document, at: Position, new: ContentControl) -> bool {
    let Some((old, start, end)) = control_range(doc, at) else {
        return false;
    };
    let new = Arc::new(new);
    let same = |c: &ContentControlRef| Arc::ptr_eq(c, &old);
    for index in start.paragraph..=end.paragraph {
        let Some(paragraph) = paragraph_mut(doc, index) else {
            continue;
        };
        for control in &mut paragraph.props.controls {
            if same(control) {
                *control = new.clone();
            }
        }
        for inline in &mut paragraph.content {
            if let Inline::Run(run) = inline
                && run.props.control.as_ref().is_some_and(same)
            {
                run.props.control = Some(new.clone());
            }
        }
    }
    let redate = matches!(new.kind(), ControlKind::Date { .. }) && !new.showing_placeholder();
    if redate && let Some((year, month, day)) = new.full_date() {
        let date = DateTime {
            year,
            month,
            day,
            hour: 0,
            minute: 0,
            second: 0,
        };
        set_control_date(doc, start, date);
    }
    true
}

/// Заменяет текст элемента у `at` и сам элемент (на `change` от него).
fn replace_control(
    doc: &mut Document,
    at: Position,
    text: &str,
    change: impl FnOnce(&ContentControl) -> ContentControl,
) -> Option<Position> {
    let Some((control, start, _)) = control_at(doc, at) else {
        return replace_block_control(doc, at, text, change);
    };
    if !control.content_editable() {
        return None;
    }
    let placeholder_style = control.showing_placeholder();
    let new = Arc::new(change(&control));
    let paragraph = paragraph_mut(doc, at.paragraph)?;
    let mut first = true;
    paragraph.content.retain_mut(|inline| {
        let Inline::Run(run) = inline else {
            return true;
        };
        if !run
            .props
            .control
            .as_ref()
            .is_some_and(|c| Arc::ptr_eq(c, &control))
        {
            return true;
        }
        if !first {
            return false;
        }
        first = false;
        run.props.control = Some(new.clone());
        run.content = if text.is_empty() {
            Vec::new()
        } else {
            vec![RunContent::Text(text.to_string())]
        };
        // Подсказка у Word оформлена стилем «Текст заполнителя» (серым).
        if placeholder_style && is_placeholder_style(run.props.style.as_deref()) {
            run.props.style = None;
        }
        true
    });
    Some(Position::new(at.paragraph, start.offset + text.len()))
}

/// Как [`replace_control`], но у блочного элемента: все его абзацы
/// становятся одним абзацем с текстом `text`.
fn replace_block_control(
    doc: &mut Document,
    at: Position,
    text: &str,
    change: impl FnOnce(&ContentControl) -> ContentControl,
) -> Option<Position> {
    let (control, start, end) = control_range(doc, at)?;
    if !control.content_editable() {
        return None;
    }
    let new = Arc::new(change(&control));
    let start = crate::edit::delete_range(doc, start, end);
    let after = crate::edit::insert_text(doc, start, text);
    let paragraph = paragraph_mut(doc, start.paragraph)?;
    for c in &mut paragraph.props.controls {
        if Arc::ptr_eq(c, &control) {
            *c = new.clone();
        }
    }
    for inline in &mut paragraph.content {
        if let Inline::Run(run) = inline
            && is_placeholder_style(run.props.style.as_deref())
        {
            run.props.style = None;
        }
    }
    Some(after)
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::{RawXml, Run, RunProps};

    fn control(props: &str) -> ContentControlRef {
        Arc::new(ContentControl {
            props: Some(RawXml::new(format!(
                r#"<w:sdtPr xmlns:w="urn:w" xmlns:w14="urn:w14">{props}</w:sdtPr>"#
            ))),
            end_props: None,
        })
    }

    /// Абзац «до [элемент] после», элемент из двух фрагментов.
    fn doc_with(control: &ContentControlRef, style: Option<&str>) -> Document {
        let inside = RunProps {
            control: Some(control.clone()),
            style: style.map(str::to_string),
            ..RunProps::default()
        };
        let paragraph = Paragraph::from_runs([
            Run::text("до "),
            Run::text("эл").with_props(inside.clone()),
            Run::text("емент").with_props(RunProps {
                bold: Some(true),
                ..inside
            }),
            Run::text(" после"),
        ]);
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![paragraph.into()];
        doc
    }

    fn text(doc: &Document) -> String {
        doc.plain_text()
    }

    #[test]
    fn finds_control_around_the_caret() {
        let c = control("");
        let doc = doc_with(&c, None);
        assert!(control_at(&doc, Position::new(0, 0)).is_none());
        let (found, start, end) = control_at(&doc, Position::new(0, 5)).unwrap();
        assert!(Arc::ptr_eq(&found, &c));
        assert_eq!((start.offset, end.offset), (5, 5 + "элемент".len()));
        // В конце элемента — тоже он.
        assert!(control_at(&doc, end).is_some());
    }

    #[test]
    fn typing_replaces_placeholder() {
        let c = control("<w:showingPlcHdr/>");
        let mut doc = doc_with(&c, Some("PlaceholderText"));
        assert!(placeholder_at(&doc, Position::new(0, 5)));
        let after = set_control_text(&mut doc, Position::new(0, 5), "Иван").unwrap();
        assert_eq!(text(&doc), "до Иван после");
        assert_eq!(after.offset, 5 + "Иван".len());
        assert!(!placeholder_at(&doc, Position::new(0, 5)));
        let (_, start, end) = control_at(&doc, Position::new(0, 5)).unwrap();
        assert_eq!((start.offset, end.offset), (5, after.offset));
        let Some(letter_model::Block::Paragraph(p)) = doc.blocks().next() else {
            panic!("expected paragraph")
        };
        assert!(p.runs().all(|r| r.props.style.is_none()));
    }

    #[test]
    fn checkbox_toggles() {
        let c = control(
            r#"<w14:checkbox><w14:checked w14:val="0"/><w14:checkedState w14:val="2612"/><w14:uncheckedState w14:val="2610"/></w14:checkbox>"#,
        );
        let mut doc = doc_with(&c, None);
        toggle_checkbox(&mut doc, Position::new(0, 5)).unwrap();
        assert_eq!(text(&doc), "до ☒ после");
        let (now, _, _) = control_at(&doc, Position::new(0, 5)).unwrap();
        assert!(matches!(
            now.kind(),
            ControlKind::CheckBox { checked: true, .. }
        ));
        toggle_checkbox(&mut doc, Position::new(0, 5)).unwrap();
        assert_eq!(text(&doc), "до ☐ после");
        // Не флажок — ничего не делаем.
        let mut plain = doc_with(&control(""), None);
        assert!(toggle_checkbox(&mut plain, Position::new(0, 5)).is_none());
    }

    #[test]
    fn list_and_date() {
        let list = control(
            r#"<w:dropDownList><w:listItem w:displayText="Да" w:value="1"/><w:listItem w:displayText="Нет" w:value="0"/></w:dropDownList>"#,
        );
        let mut doc = doc_with(&list, None);
        choose_list_item(&mut doc, Position::new(0, 5), 1).unwrap();
        assert_eq!(text(&doc), "до Нет после");
        assert!(choose_list_item(&mut doc, Position::new(0, 5), 7).is_none());

        let date = control(r#"<w:date><w:dateFormat w:val="d MMMM yyyy"/></w:date>"#);
        let mut doc = doc_with(&date, None);
        let day = DateTime {
            year: 2026,
            month: 10,
            day: 23,
            ..DateTime::default()
        };
        set_control_date(&mut doc, Position::new(0, 5), day).unwrap();
        assert_eq!(text(&doc), "до 23 октября 2026 после");
        let (now, _, _) = control_at(&doc, Position::new(0, 5)).unwrap();
        assert!(
            now.props
                .as_ref()
                .unwrap()
                .as_str()
                .contains("2026-10-23T00:00:00Z")
        );
    }

    #[test]
    fn locked_control_is_not_deleted() {
        use crate::{Command, History, Selection};
        let c = control(r#"<w:lock w:val="sdtLocked"/>"#);
        let mut doc = doc_with(&c, None);
        let (from, to) = (Position::new(0, 0), Position::new(0, 30));
        assert!(removes_locked_control(&doc, from, to, None));
        // Часть элемента удалять можно.
        assert!(!removes_locked_control(
            &doc,
            Position::new(0, 5),
            Position::new(0, 7),
            None
        ));
        let mut history = History::default();
        let all = Selection::new(from, Position::new(0, doc.plain_text().len()));
        history.run(&mut doc, all, Command::DeleteBackward);
        assert_eq!(text(&doc), "до элемент после");
        // Элемент из одного знака не стирается Backspace.
        let one = control(r#"<w:lock w:val="sdtContentLocked"/>"#);
        let paragraph = Paragraph::from_runs([
            Run::text("a"),
            Run::text("x").with_props(RunProps {
                control: Some(one),
                ..RunProps::default()
            }),
        ]);
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![paragraph.into()];
        assert!(removes_locked_control(
            &doc,
            Position::new(0, 2),
            Position::new(0, 2),
            Some(false)
        ));
        assert!(!removes_locked_control(
            &doc,
            Position::new(0, 1),
            Position::new(0, 1),
            Some(false)
        ));
        assert!(removes_locked_control(
            &doc,
            Position::new(0, 1),
            Position::new(0, 1),
            Some(true)
        ));
    }

    fn setup(kind: NewControl) -> ControlSetup {
        ControlSetup {
            kind,
            placeholder: "Место для ввода текста.".into(),
            russian: true,
        }
    }

    #[test]
    fn new_controls() {
        use crate::{Command, History, Selection};
        let fresh = || {
            let mut doc = Document::new();
            doc.sections[0].blocks = vec![Paragraph::from_runs([Run::text("Имя: .")]).into()];
            doc
        };
        let at = Position::new(0, "Имя: ".len());
        // Текст с подсказкой: набор заменяет её, стиль подсказки уходит.
        let mut doc = fresh();
        let mut history = History::default();
        let picked = Selection::new(at, at);
        let command = Command::InsertControl(Box::new(setup(NewControl::PlainText)));
        let after = history.run(&mut doc, picked, command);
        assert_eq!(after, picked);
        assert_eq!(text(&doc), "Имя: Место для ввода текста..");
        assert!(placeholder_at(&doc, at));
        let (control, _, _) = control_at(&doc, at).unwrap();
        assert_eq!(control.kind(), ControlKind::Text);
        assert!(doc.styles.styles.iter().any(|s| s.id == "PlaceholderText"));
        history.run(&mut doc, after, Command::Type("Иван".into()));
        assert_eq!(text(&doc), "Имя: Иван.");
        assert!(!placeholder_at(&doc, at));
        // Выделенный текст оборачивается без подсказки.
        let mut doc = fresh();
        let end = Position::new(0, "Имя:".len());
        insert_control(
            &mut doc,
            Position::new(0, 0),
            end,
            &setup(NewControl::RichText),
        );
        let (_, from, to) = control_at(&doc, Position::new(0, 0)).unwrap();
        assert_eq!((from.offset, to.offset), (0, end.offset));
        assert!(!placeholder_at(&doc, from));
        // Внутри элемента новый не вставляется.
        let before = doc.clone();
        insert_control(
            &mut doc,
            Position::new(0, 2),
            Position::new(0, 2),
            &setup(NewControl::Date),
        );
        assert_eq!(doc, before);
        // Флажок, список, дата.
        let mut doc = fresh();
        let after = insert_control(&mut doc, at, at, &setup(NewControl::CheckBox));
        assert_eq!(after.offset, at.offset + "☐".len());
        assert!(matches!(
            control_at(&doc, at).unwrap().0.kind(),
            ControlKind::CheckBox { checked: false, .. }
        ));
        toggle_checkbox(&mut doc, at).unwrap();
        assert_eq!(text(&doc), "Имя: ☒.");
        let mut doc = fresh();
        insert_control(&mut doc, at, at, &setup(NewControl::DropDownList));
        let ControlKind::List { items, editable } = control_at(&doc, at).unwrap().0.kind() else {
            panic!("ждали список");
        };
        assert!(!editable);
        assert_eq!(items[0].text, "Место для ввода текста.");
        let mut doc = fresh();
        insert_control(&mut doc, at, at, &setup(NewControl::Date));
        let day = DateTime {
            year: 2026,
            month: 10,
            day: 26,
            ..DateTime::default()
        };
        set_control_date(&mut doc, at, day).unwrap();
        assert_eq!(text(&doc), "Имя: 26.10.2026.");
    }

    #[test]
    fn enter_after_control_leaves_it() {
        let c = control("<w:text/>");
        let mut doc = doc_with(&c, Some("PlaceholderText"));
        let end = Position::new(0, "до элемент".len());
        // Абзац кончается элементом: Enter в его конце.
        let paragraph = paragraph_mut(&mut doc, 0).unwrap();
        paragraph.content.pop();
        let next = crate::edit::split_paragraph(&mut doc, end);
        let next = crate::edit::insert_text(&mut doc, next, "дальше");
        assert!(control_at(&doc, next).is_none());
        let typed = paragraph_ref(&doc, 1).unwrap().runs().next().unwrap();
        assert!(typed.props.style.is_none());
        assert!(control_at(&doc, Position::new(0, 5)).is_some());
    }

    #[test]
    fn block_control_range() {
        let c = control("");
        let mut doc = Document::new();
        let mut inside = Paragraph::from_runs([Run::text("один")]);
        inside.props.controls = vec![c.clone()];
        let mut second = Paragraph::from_runs([Run::text("два")]);
        second.props.controls = vec![c.clone()];
        doc.sections[0].blocks = vec![
            Paragraph::from_runs([Run::text("до")]).into(),
            inside.into(),
            second.into(),
            Paragraph::from_runs([Run::text("после")]).into(),
        ];
        let (found, start, end) = control_range(&doc, Position::new(2, 1)).unwrap();
        assert!(Arc::ptr_eq(&found, &c));
        assert_eq!(
            (start, end),
            (Position::new(1, 0), Position::new(2, "два".len()))
        );
        assert!(control_range(&doc, Position::new(3, 0)).is_none());
    }

    #[test]
    fn properties_change_and_undo() {
        use crate::{Command, History, Selection};
        let date = control(
            r#"<w:id w:val="5"/><w:date w:fullDate="2026-10-23T00:00:00Z"><w:dateFormat w:val="dd.MM.yyyy"/></w:date>"#,
        );
        let mut doc = doc_with(&date, None);
        let at = Position::new(0, 5);
        set_control_date(
            &mut doc,
            at,
            DateTime {
                year: 2026,
                month: 10,
                day: 23,
                ..DateTime::default()
            },
        )
        .unwrap();
        assert_eq!(text(&doc), "до 23.10.2026 после");
        let (now, _, _) = control_at(&doc, at).unwrap();
        let changed = now
            .with_settings("Срок", "due", ControlLock::Control)
            .with_date_format("d MMMM yyyy");
        let mut history = History::default();
        history.run(
            &mut doc,
            Selection::caret(at),
            Command::SetControl(Box::new(changed)),
        );
        assert_eq!(text(&doc), "до 23 октября 2026 после");
        let (now, _, _) = control_at(&doc, at).unwrap();
        assert_eq!(now.title().as_deref(), Some("Срок"));
        assert_eq!(now.lock(), ControlLock::Control);
        history.undo(&mut doc);
        assert_eq!(text(&doc), "до 23.10.2026 после");
        let (now, _, _) = control_at(&doc, at).unwrap();
        assert_eq!(now.title(), None);

        // Блочный элемент: свойства меняются во всех его абзацах.
        let block = control("");
        let mut doc = Document::new();
        let mut one = Paragraph::from_runs([Run::text("один")]);
        one.props.controls = vec![block.clone()];
        let mut two = Paragraph::from_runs([Run::text("два")]);
        two.props.controls = vec![block.clone()];
        doc.sections[0].blocks = vec![one.into(), two.into()];
        let named = block.with_settings("Раздел", "", ControlLock::None);
        assert!(set_control(&mut doc, Position::new(1, 0), named));
        for i in 0..2 {
            let p = paragraph_ref(&doc, i).unwrap();
            assert_eq!(p.props.controls[0].title().as_deref(), Some("Раздел"));
        }
        assert!(!set_control(
            &mut doc_with(&date, None),
            Position::new(0, 0),
            ContentControl::default()
        ));
    }

    #[test]
    fn erased_control_shows_placeholder() {
        use crate::{Command, History, Selection};
        let c = control(r#"<w:id w:val="1"/><w:text/>"#);
        let mut doc = doc_with(&c, None);
        let mut history = History::default();
        let texts = PlaceholderTexts {
            text: "Введите.".into(),
            ..PlaceholderTexts::default()
        };
        history.set_placeholders(texts);
        let (from, to) = (Position::new(0, 5), Position::new(0, 5 + "элемент".len()));
        // Всё содержимое выделено и стёрто — на месте подсказка.
        let after = history.run(&mut doc, Selection::new(from, to), Command::DeleteBackward);
        assert_eq!(text(&doc), "до Введите. после");
        assert_eq!(after.focus, from);
        assert!(placeholder_at(&doc, from));
        let p = paragraph_ref(&doc, 0).unwrap();
        let shown = p.runs().nth(1).unwrap();
        assert_eq!(shown.props.style.as_deref(), Some("PlaceholderText"));
        // Набор заменяет подсказку, текст снова в элементе.
        history.run(&mut doc, Selection::caret(from), Command::Type("Я".into()));
        assert_eq!(text(&doc), "до Я после");
        assert!(control_at(&doc, from).is_some());
        // Backspace по последнему знаку — снова подсказка.
        let end = Position::new(0, 5 + "Я".len());
        history.run(&mut doc, Selection::caret(end), Command::DeleteBackward);
        assert_eq!(text(&doc), "до Введите. после");
        // Набор поверх выделенного содержимого остаётся в элементе.
        history.undo(&mut doc);
        history.undo(&mut doc);
        history.undo(&mut doc);
        assert_eq!(text(&doc), "до элемент после");
        history.run(
            &mut doc,
            Selection::new(from, to),
            Command::Type("Ия".into()),
        );
        assert_eq!(text(&doc), "до Ия после");
        let (_, start, end) = control_at(&doc, from).expect("элемент на месте");
        assert_eq!((start.offset, end.offset), (5, 5 + "Ия".len()));
        // Стирание вместе с краями элемента удаляет его.
        let all = Selection::new(Position::new(0, 0), Position::new(0, 9));
        history.run(&mut doc, all, Command::DeleteBackward);
        assert_eq!(text(&doc), " после");
        assert!(!placeholder_at(&doc, Position::new(0, 0)));
        // «Режим конструктора»: подсказку правят как текст.
        let shown = control(r#"<w:showingPlcHdr/>"#);
        let mut doc = doc_with(&shown, Some("PlaceholderText"));
        let mut history = History::default();
        history.set_design_mode(true);
        history.run(
            &mut doc,
            Selection::caret(Position::new(0, 7)),
            Command::Type("!".into()),
        );
        assert_eq!(text(&doc), "до э!лемент после");
    }

    #[test]
    fn lists_all_controls() {
        let inline = control("<w:text/>");
        let mut doc = doc_with(&inline, None);
        let block = control("");
        let mut one = Paragraph::from_runs([Run::text("один")]);
        one.props.controls = vec![block.clone()];
        let mut two = Paragraph::from_runs([Run::text("два")]);
        two.props.controls = vec![block.clone()];
        doc.sections[0].blocks.extend([one.into(), two.into()]);
        let all = controls_in(&doc);
        assert_eq!(all.len(), 2);
        assert!(Arc::ptr_eq(&all[0].0, &inline));
        assert_eq!((all[0].1.offset, all[0].2.offset), (5, 5 + "элемент".len()));
        assert!(Arc::ptr_eq(&all[1].0, &block));
        assert_eq!(
            (all[1].1, all[1].2),
            (Position::new(1, 0), Position::new(2, 6))
        );
    }

    #[test]
    fn block_placeholder() {
        use crate::{Command, History, Selection};
        let c = control("<w:showingPlcHdr/>");
        let mut doc = Document::new();
        let mut one = Paragraph::from_runs([Run::text("Подсказка").with_props(RunProps {
            style: Some("PlaceholderText".into()),
            ..RunProps::default()
        })]);
        one.props.controls = vec![c.clone()];
        let mut two = Paragraph::from_runs([Run::text("вторая")]);
        two.props.controls = vec![c.clone()];
        doc.sections[0].blocks = vec![one.into(), two.into()];
        let mut history = History::default();
        assert!(placeholder_at(&doc, Position::new(0, 2)));
        history.run(
            &mut doc,
            Selection::caret(Position::new(0, 2)),
            Command::Type("Текст".into()),
        );
        assert_eq!(text(&doc), "Текст");
        let p = paragraph_ref(&doc, 0).unwrap();
        assert!(!p.props.controls[0].showing_placeholder());
        assert!(p.runs().all(|r| r.props.style.is_none()));
        // Всё стёрли — подсказка возвращается.
        let all = Selection::new(Position::new(0, 0), Position::new(0, "Текст".len()));
        history.run(&mut doc, all, Command::DeleteBackward);
        assert_eq!(text(&doc), "Click or tap here to enter text.");
        assert!(placeholder_at(&doc, Position::new(0, 0)));
    }

    #[test]
    fn locked_content_is_not_editable() {
        let c = control(r#"<w:lock w:val="contentLocked"/>"#);
        let mut doc = doc_with(&c, None);
        assert!(editable(&doc, Position::new(0, 0), Position::new(0, 2)));
        assert!(!editable(&doc, Position::new(0, 6), Position::new(0, 6)));
        assert!(!editable(&doc, Position::new(0, 0), Position::new(0, 20)));
        assert!(set_control_text(&mut doc, Position::new(0, 6), "x").is_none());
        // Запрет только удаления самого элемента не мешает правке.
        let doc = doc_with(&control(r#"<w:lock w:val="sdtLocked"/>"#), None);
        assert!(editable(&doc, Position::new(0, 6), Position::new(0, 6)));
    }
}
