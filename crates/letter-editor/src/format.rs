//! Оформление выделенного: шрифт и размер, полужирный, курсив,
//! подчёркивание, зачёркивание, индексы, цвет, выделение цветом,
//! очистка оформления (Ctrl+B, Ctrl+I, Ctrl+U, Ctrl+], Ctrl+[, Ctrl+=,
//! Ctrl+Shift+=, Ctrl+Пробел) и выравнивание абзацев (Ctrl+L, Ctrl+E,
//! Ctrl+R, Ctrl+J).

use letter_model::{
    Alignment, Border, Color, Document, FirstLineIndent, HalfPoints, Highlight, Inline,
    LineSpacing, Paragraph, ParagraphBorders, ParagraphProps, ResolvedParagraphProps,
    ResolvedRunProps, Run, RunProps, Shading, StyleKind, TabLeader, TabStop, TabStopKind, Twips,
    Underline, VerticalAlign,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::edit::{paragraph_mut, paragraph_ref, split_content, text_len, tidy};
use crate::position::Position;

/// Что поменять в оформлении символов.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunFormat {
    /// Ctrl+B: полужирный — включить или выключить.
    Bold,
    /// Ctrl+I: курсив.
    Italic,
    /// Ctrl+U: одинарное подчёркивание.
    Underline,
    /// Ctrl+]: шрифт на 1 пт больше.
    Grow,
    /// Ctrl+[: шрифт на 1 пт меньше.
    Shrink,
    /// Ctrl+Shift+>: следующий размер из списка размеров Word.
    GrowStep,
    /// Ctrl+Shift+<: предыдущий размер из списка.
    ShrinkStep,
    /// Ctrl+Shift+A: все прописные — включить или выключить.
    Caps,
    /// Ctrl+Shift+K: малые прописные — включить или выключить.
    SmallCaps,
    /// Ctrl+Shift+H: скрытый текст — включить или выключить.
    Hidden,
    /// Ctrl+Shift+D: двойное подчёркивание — включить или выключить.
    DoubleUnderline,
    /// Ctrl+Shift+W: подчёркивание только слов — включить или выключить.
    WordUnderline,
    /// Зачёркнутый — включить или выключить.
    Strike,
    /// Ctrl+=: подстрочный знак — включить или выключить.
    Subscript,
    /// Ctrl+Shift+=: надстрочный знак — включить или выключить.
    Superscript,
    /// Шрифт с этим именем (для латиницы, кириллицы и сложных письменностей,
    /// как список «Шрифт» на ленте Word).
    Font(String),
    /// Размер шрифта.
    Size(HalfPoints),
    /// Цвет текста; [`Color::Auto`] — «Авто».
    Color(Color),
    /// Выделение цветом (маркер); [`Highlight::None`] — «Нет цвета».
    Highlight(Highlight),
    /// Ctrl+Пробел: убрать оформление символов, оставив оформление стиля.
    Clear,
    /// Стиль символов с этим идентификатором.
    CharStyle(String),
    /// Изменения из окна «Шрифт».
    Set(Box<RunChanges>),
}

/// Изменения из окна «Шрифт»: `None` — поле не трогали.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunChanges {
    /// Шрифт (для латиницы, кириллицы и сложных письменностей).
    pub font: Option<String>,
    /// Полужирный.
    pub bold: Option<bool>,
    /// Курсив.
    pub italic: Option<bool>,
    /// Размер.
    pub size: Option<HalfPoints>,
    /// Цвет текста.
    pub color: Option<Color>,
    /// Подчёркивание.
    pub underline: Option<Underline>,
    /// Цвет подчёркивания: `Some(None)` — «Авто».
    pub underline_color: Option<Option<Color>>,
    /// Зачёркнутый.
    pub strike: Option<bool>,
    /// Двойное зачёркивание.
    pub double_strike: Option<bool>,
    /// Над- или подстрочный.
    pub vertical_align: Option<VerticalAlign>,
    /// Малые прописные.
    pub small_caps: Option<bool>,
    /// Все прописные.
    pub caps: Option<bool>,
    /// Скрытый.
    pub hidden: Option<bool>,
    /// Масштаб, проценты.
    pub scale: Option<u16>,
    /// Интервал между буквами.
    pub spacing: Option<Twips>,
    /// Смещение, полупункты.
    pub position: Option<i32>,
    /// Кернинг от этого размера; ноль — выключен.
    pub kerning: Option<HalfPoints>,
}

impl RunChanges {
    /// Ничего не меняется.
    pub fn is_empty(&self) -> bool {
        *self == RunChanges::default()
    }
}

/// Самый маленький и самый большой размер шрифта Word, полупункты.
const MIN_SIZE: u32 = 2;
const MAX_SIZE: u32 = 3276;

/// Размеры из списка Word в полупунктах: по ним ходят Ctrl+Shift+> и
/// Ctrl+Shift+<. Меньше 8 пт шаг — 1 пт, больше 72 пт — 10 пт.
const SIZE_STEPS: [u32; 17] = [
    16, 18, 20, 21, 22, 24, 28, 32, 36, 40, 44, 48, 52, 56, 72, 96, 144,
];

/// Следующий (`grow`) или предыдущий размер шрифта по списку Word.
fn size_step(size: u32, grow: bool) -> u32 {
    const FIRST: u32 = SIZE_STEPS[0];
    const LAST: u32 = SIZE_STEPS[SIZE_STEPS.len() - 1];
    if grow {
        match SIZE_STEPS.iter().find(|&&s| s > size) {
            _ if size < FIRST => size + 2,
            Some(&next) => next,
            None => (size / 20 + 1) * 20,
        }
    } else {
        match SIZE_STEPS.iter().rev().find(|&&s| s < size) {
            _ if size > LAST => ((size - 1) / 20 * 20).max(LAST),
            Some(&previous) if size > FIRST => previous,
            _ => size.saturating_sub(2),
        }
    }
}

/// Кусок абзаца, который меняется: номер абзаца, начало и конец.
type Piece = (usize, usize, usize);

/// Меняет оформление текста от `start` до `end`. Если ничего не
/// выделено, как в Word меняется слово под курсором (курсор внутри
/// слова) или пустой абзац (набранный в нём текст получит это
/// оформление). Полужирный, курсив и подчёркивание включаются, если
/// хоть где-то в выделенном их нет, иначе выключаются.
pub fn format_runs(doc: &mut Document, start: Position, end: Position, format: RunFormat) {
    let pieces = pieces(doc, start, end);
    if pieces.is_empty() {
        return;
    }
    // Включить или выключить: смотрим итоговое оформление выделенного.
    let mut all_on = true;
    for &(index, from, to) in &pieces {
        let Some(paragraph) = paragraph_ref(doc, index) else {
            continue;
        };
        let default = RunProps::default();
        let mut touched: Vec<&RunProps> = run_props_in(paragraph, from, to).collect();
        // Пустой абзац без фрагментов: оформление по умолчанию.
        if touched.is_empty() {
            touched.push(&default);
        }
        for props in touched {
            let resolved = doc.resolve_run(&paragraph.props, props);
            all_on &= match format {
                RunFormat::Bold => resolved.bold,
                RunFormat::Italic => resolved.italic,
                RunFormat::Underline => resolved.underline != Underline::None,
                RunFormat::Strike => resolved.strike,
                RunFormat::Subscript => resolved.vertical_align == VerticalAlign::Subscript,
                RunFormat::Superscript => resolved.vertical_align == VerticalAlign::Superscript,
                RunFormat::Caps => resolved.caps,
                RunFormat::SmallCaps => resolved.small_caps,
                RunFormat::Hidden => resolved.hidden,
                RunFormat::DoubleUnderline => resolved.underline == Underline::Double,
                RunFormat::WordUnderline => resolved.underline == Underline::Words,
                _ => true,
            };
        }
    }
    for (index, from, to) in pieces {
        // Размер считается от итогового каждого фрагмента: нужен доступ
        // к стилям, поэтому сначала собираем итоговые размеры.
        let sizes: Vec<u32> = match paragraph_ref(doc, index) {
            Some(p) => run_props_in(p, from, to)
                .map(|r| doc.resolve_run(&p.props, r).size.0)
                .collect(),
            None => continue,
        };
        let Some(paragraph) = paragraph_mut(doc, index) else {
            continue;
        };
        let mut sizes = sizes.into_iter();
        apply(paragraph, from, to, |props| match &format {
            RunFormat::Bold => props.bold = Some(!all_on),
            RunFormat::Italic => props.italic = Some(!all_on),
            RunFormat::Underline => {
                props.underline = Some(if all_on {
                    Underline::None
                } else {
                    Underline::Single
                });
            }
            RunFormat::Grow | RunFormat::Shrink => {
                let size = sizes.next().unwrap_or(22);
                let size = if format == RunFormat::Grow {
                    size + 2
                } else {
                    size.saturating_sub(2)
                };
                props.size = Some(HalfPoints(size.clamp(MIN_SIZE, MAX_SIZE)));
            }
            RunFormat::GrowStep | RunFormat::ShrinkStep => {
                let size = sizes.next().unwrap_or(22);
                let size = size_step(size, format == RunFormat::GrowStep);
                props.size = Some(HalfPoints(size.clamp(MIN_SIZE, MAX_SIZE)));
            }
            RunFormat::Caps => {
                props.caps = Some(!all_on);
                if !all_on {
                    props.small_caps = Some(false);
                }
            }
            RunFormat::SmallCaps => {
                props.small_caps = Some(!all_on);
                if !all_on {
                    props.caps = Some(false);
                }
            }
            RunFormat::Hidden => props.hidden = Some(!all_on),
            RunFormat::DoubleUnderline | RunFormat::WordUnderline => {
                props.underline = Some(if all_on {
                    Underline::None
                } else if format == RunFormat::DoubleUnderline {
                    Underline::Double
                } else {
                    Underline::Words
                });
            }
            RunFormat::Strike => {
                props.strike = Some(!all_on);
                if !all_on {
                    props.double_strike = Some(false);
                }
            }
            RunFormat::Subscript | RunFormat::Superscript => {
                props.vertical_align = Some(if all_on {
                    VerticalAlign::Baseline
                } else if format == RunFormat::Subscript {
                    VerticalAlign::Subscript
                } else {
                    VerticalAlign::Superscript
                });
            }
            RunFormat::Font(name) => set_font(props, name),
            RunFormat::Set(changes) => {
                if let Some(name) = &changes.font {
                    set_font(props, name);
                }
                if let Some(size) = changes.size {
                    props.size = Some(HalfPoints(size.0.clamp(MIN_SIZE, MAX_SIZE)));
                }
                if let Some(color) = changes.color {
                    props.color = Some(color);
                    props.theme_color = None;
                }
                if let Some(color) = changes.underline_color {
                    props.underline_color = color;
                }
                set(&mut props.bold, changes.bold);
                set(&mut props.italic, changes.italic);
                set(&mut props.underline, changes.underline);
                set(&mut props.strike, changes.strike);
                set(&mut props.double_strike, changes.double_strike);
                set(&mut props.vertical_align, changes.vertical_align);
                set(&mut props.small_caps, changes.small_caps);
                set(&mut props.caps, changes.caps);
                set(&mut props.hidden, changes.hidden);
                set(&mut props.scale, changes.scale.map(|s| s.clamp(1, 600)));
                set(&mut props.spacing, changes.spacing);
                set(&mut props.position, changes.position);
                set(&mut props.kerning, changes.kerning);
            }
            RunFormat::Size(size) => {
                props.size = Some(HalfPoints(size.0.clamp(MIN_SIZE, MAX_SIZE)));
            }
            RunFormat::Color(color) => {
                props.color = Some(*color);
                props.theme_color = None;
            }
            RunFormat::Highlight(highlight) => props.highlight = Some(*highlight),
            RunFormat::CharStyle(id) => props.style = Some(id.clone()),
            // Неизвестные свойства (их нам не показать) остаются.
            RunFormat::Clear => {
                // Ссылка — не оформление: остаётся.
                *props = RunProps {
                    link: props.link.take(),
                    unknown: std::mem::take(&mut props.unknown),
                    ..RunProps::default()
                };
            }
        });
    }
}

/// Выравнивает абзацы от `start` до `end`. Как в Word, повторное
/// нажатие (все абзацы уже так выровнены) возвращает выравнивание
/// по левому краю.
pub fn align_paragraphs(doc: &mut Document, start: Position, end: Position, alignment: Alignment) {
    let (first, last) = ordered(start, end);
    let range = first.paragraph..=last.paragraph;
    let all = range.clone().all(|i| {
        paragraph_ref(doc, i).is_none_or(|p| doc.resolve_paragraph(&p.props).alignment == alignment)
    });
    let value = if all && alignment != Alignment::Left {
        Alignment::Left
    } else {
        alignment
    };
    for index in range {
        if let Some(paragraph) = paragraph_mut(doc, index) {
            paragraph.props.alignment = Some(value);
        }
    }
}

/// Что поменять в оформлении абзацев (группа «Абзац» на ленте).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParagraphFormat {
    /// «Увеличить отступ»: отступ слева до следующей позиции табуляции
    /// по умолчанию.
    IndentMore,
    /// «Уменьшить отступ»: до предыдущей позиции, не меньше нуля.
    IndentLess,
    /// Межстрочный интервал.
    LineSpacing(LineSpacing),
    /// Интервал перед абзацем.
    SpaceBefore(Twips),
    /// Интервал после абзаца.
    SpaceAfter(Twips),
    /// Отступы, как их ставит линейка: слева, справа и первой строки.
    Indents {
        /// Отступ слева.
        left: Twips,
        /// Отступ справа.
        right: Twips,
        /// Отступ или выступ первой строки.
        first_line: Option<FirstLineIndent>,
    },
    /// Добавить позицию табуляции (щелчок по линейке). Позиция в том же
    /// месте заменяется.
    AddTab(TabStop),
    /// Перенести позицию табуляции (перетаскивание по линейке).
    MoveTab {
        /// Где была.
        from: Twips,
        /// Куда перенесена.
        to: Twips,
    },
    /// Убрать позицию табуляции (стащить с линейки).
    RemoveTab(Twips),
    /// Ctrl+T: выступ первой строки на шаг табуляции больше (отступ
    /// слева растёт, первая строка остаётся на месте).
    HangingMore,
    /// Ctrl+Shift+T: выступ на шаг меньше.
    HangingLess,
    /// Ctrl+0: добавить интервал 12 пт перед абзацем или убрать его.
    ToggleSpaceBefore,
    /// Ctrl+Q: убрать оформление абзаца, оставив оформление стиля.
    Reset,
    /// Изменения из окна «Абзац».
    Set(ParagraphChanges),
    /// «Заливка»: цвет фона абзаца; `None` — «Нет цвета».
    Shading(Option<(u8, u8, u8)>),
    /// «Границы»: поставить или убрать линии.
    Border(BorderEdge),
}

/// Пункт меню «Границы» для абзацев. Линия ставится, если её ещё нет
/// у первого абзаца, иначе убирается, как в Word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BorderEdge {
    /// «Нижняя граница».
    Bottom,
    /// «Верхняя граница».
    Top,
    /// «Левая граница».
    Left,
    /// «Правая граница».
    Right,
    /// «Нет границы»: убрать все линии.
    None,
    /// «Все границы»: рамка и линии между абзацами.
    All,
    /// «Внешние границы»: рамка вокруг абзацев.
    Outside,
    /// «Внутренние границы»: линии между абзацами.
    Inside,
}

impl BorderEdge {
    /// Все пункты в порядке меню Word.
    pub const ALL: [BorderEdge; 8] = [
        BorderEdge::Bottom,
        BorderEdge::Top,
        BorderEdge::Left,
        BorderEdge::Right,
        BorderEdge::None,
        BorderEdge::All,
        BorderEdge::Outside,
        BorderEdge::Inside,
    ];

    /// Стороны, которые меняет пункт: сверху, слева, снизу, справа,
    /// между абзацами.
    fn sides(self) -> [bool; 5] {
        match self {
            BorderEdge::Top => [true, false, false, false, false],
            BorderEdge::Left => [false, true, false, false, false],
            BorderEdge::Bottom => [false, false, true, false, false],
            BorderEdge::Right => [false, false, false, true, false],
            BorderEdge::Inside => [false, false, false, false, true],
            BorderEdge::Outside => [true, true, true, true, false],
            BorderEdge::None | BorderEdge::All => [true; 5],
        }
    }

    /// Линии стоят у абзаца с границами `borders`.
    pub fn is_set(self, borders: &ParagraphBorders) -> bool {
        self != BorderEdge::None
            && sides_of(borders)
                .iter()
                .zip(self.sides())
                .filter(|(_, used)| *used)
                .all(|(side, _)| ParagraphBorders::visible(side).is_some())
    }
}

/// Стороны границ абзаца по порядку [`BorderEdge::sides`].
fn sides_of(b: &ParagraphBorders) -> [&Option<Border>; 5] {
    [&b.top, &b.left, &b.bottom, &b.right, &b.between]
}

/// Линия, которую ставит кнопка «Границы»: сплошная 0,5 пт цвета
/// «Авто», от текста на 1 пт сверху и снизу и на 4 пт сбоку.
fn default_border(side: usize) -> Border {
    Border {
        space: Some(if side == 1 || side == 3 { 4 } else { 1 }),
        ..Border::single(4)
    }
}

/// Ставит (`on`) или убирает линии `edge` в прямом оформлении абзаца;
/// убранная линия, которая есть в стиле, записывается видом `nil`.
fn set_borders(props: &mut ParagraphProps, style: &ParagraphBorders, edge: BorderEdge, on: bool) {
    let mut borders = props.borders.take().unwrap_or_default();
    let styled = sides_of(style).map(|s| ParagraphBorders::visible(s).is_some());
    let slots = [
        &mut borders.top,
        &mut borders.left,
        &mut borders.bottom,
        &mut borders.right,
        &mut borders.between,
    ];
    for (k, (slot, used)) in slots.into_iter().zip(edge.sides()).enumerate() {
        if !used {
            continue;
        }
        *slot = if on {
            Some(default_border(k))
        } else if styled[k] {
            Some(Border {
                style: "nil".into(),
                size: None,
                space: None,
                color: None,
            })
        } else {
            None
        };
    }
    props.borders = (!borders.is_empty()).then_some(borders);
}

/// Изменения из окна «Абзац»: `None` — поле не трогали, у абзацев
/// остаётся своё значение.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParagraphChanges {
    /// Выравнивание.
    pub alignment: Option<Alignment>,
    /// Отступ слева.
    pub left: Option<Twips>,
    /// Отступ справа.
    pub right: Option<Twips>,
    /// Первая строка: `Some(None)` — «(нет)».
    pub first_line: Option<Option<FirstLineIndent>>,
    /// Интервал перед.
    pub before: Option<Twips>,
    /// Интервал после.
    pub after: Option<Twips>,
    /// Межстрочный интервал.
    pub line: Option<LineSpacing>,
    /// «Не добавлять интервал между абзацами одного стиля».
    pub contextual_spacing: Option<bool>,
    /// «Запрет висячих строк».
    pub widow_control: Option<bool>,
    /// «Не отрывать от следующего».
    pub keep_next: Option<bool>,
    /// «Не разрывать абзац».
    pub keep_lines: Option<bool>,
    /// «С новой страницы».
    pub page_break_before: Option<bool>,
}

impl ParagraphChanges {
    /// Ничего не меняется.
    pub fn is_empty(&self) -> bool {
        *self == ParagraphChanges::default()
    }
}

/// Меняет оформление абзацев от `start` до `end`.
pub fn format_paragraphs(
    doc: &mut Document,
    start: Position,
    end: Position,
    format: ParagraphFormat,
) {
    let (first, last) = ordered(start, end);
    // Шаг отступа — табуляция по умолчанию (в русском Word 1,25 см).
    let step = doc.settings.default_tab_stop.0.max(1);
    // Ctrl+0 смотрит на первый абзац: есть интервал — убрать у всех.
    let had_space_before = paragraph_ref(doc, first.paragraph)
        .is_some_and(|p| doc.resolve_paragraph(&p.props).space_before.0 > 0);
    // «Границы» ставят линии, если у первого абзаца их нет.
    let borders_on = match format {
        ParagraphFormat::Border(BorderEdge::None) => false,
        ParagraphFormat::Border(edge) => !paragraph_ref(doc, first.paragraph)
            .is_some_and(|p| edge.is_set(&doc.resolve_paragraph(&p.props).borders)),
        _ => false,
    };
    for index in first.paragraph..=last.paragraph {
        let Some(resolved) = paragraph_ref(doc, index).map(|p| doc.resolve_paragraph(&p.props))
        else {
            continue;
        };
        let left = resolved.indent_left.0;
        // Оформление стиля без прямого оформления абзаца.
        let styled = paragraph_ref(doc, index).map(|p| {
            doc.resolve_paragraph(&ParagraphProps {
                style: p.props.style.clone(),
                ..ParagraphProps::default()
            })
        });
        let Some(paragraph) = paragraph_mut(doc, index) else {
            continue;
        };
        let props = &mut paragraph.props;
        match format {
            ParagraphFormat::IndentMore => {
                let next = (left.div_euclid(step) + 1) * step;
                props.indentation.left = Some(Twips(next));
            }
            ParagraphFormat::IndentLess => {
                let previous = (left + step - 1).div_euclid(step) - 1;
                props.indentation.left = Some(Twips((previous * step).max(0)));
            }
            ParagraphFormat::HangingMore | ParagraphFormat::HangingLess => {
                // Где начинается первая строка — там она и остаётся.
                let hanging = match resolved.first_line {
                    Some(FirstLineIndent::Hanging(h)) => h.0,
                    Some(FirstLineIndent::FirstLine(f)) => -f.0,
                    None => 0,
                };
                let start = left - hanging;
                let new_left = if format == ParagraphFormat::HangingMore {
                    (left.div_euclid(step) + 1) * step
                } else {
                    (((left + step - 1).div_euclid(step) - 1) * step).max(start)
                };
                props.indentation.left = Some(Twips(new_left));
                props.indentation.first_line = Some(if new_left > start {
                    FirstLineIndent::Hanging(Twips(new_left - start))
                } else {
                    FirstLineIndent::FirstLine(Twips(start - new_left))
                });
            }
            ParagraphFormat::ToggleSpaceBefore => {
                props.spacing.before = Some(Twips(if had_space_before { 0 } else { 240 }));
            }
            ParagraphFormat::Reset => {
                // Список и непонятые нами свойства не трогаем.
                let style = props.style.take();
                let numbering = props.numbering.take();
                let unknown = std::mem::take(&mut props.unknown);
                *props = letter_model::ParagraphProps {
                    style,
                    numbering,
                    unknown,
                    ..Default::default()
                };
            }
            ParagraphFormat::LineSpacing(line) => props.spacing.line = Some(line),
            ParagraphFormat::SpaceBefore(before) => props.spacing.before = Some(before),
            ParagraphFormat::SpaceAfter(after) => props.spacing.after = Some(after),
            ParagraphFormat::Indents {
                left,
                right,
                first_line,
            } => {
                props.indentation.left = Some(left);
                props.indentation.right = Some(right);
                props.indentation.first_line = first_line;
            }
            ParagraphFormat::AddTab(stop) => {
                change_tabs(doc, index, |tabs| {
                    tabs.retain(|t| t.position != stop.position);
                    tabs.push(stop);
                });
            }
            ParagraphFormat::MoveTab { from, to } => change_tabs(doc, index, |tabs| {
                if let Some(moved) = tabs.iter().find(|t| t.position == from).copied() {
                    tabs.retain(|t| t.position != from && t.position != to);
                    tabs.push(TabStop {
                        position: to,
                        ..moved
                    });
                }
            }),
            ParagraphFormat::RemoveTab(at) => {
                change_tabs(doc, index, |tabs| tabs.retain(|t| t.position != at));
            }
            ParagraphFormat::Set(changes) => {
                set(&mut props.alignment, changes.alignment);
                set(&mut props.indentation.left, changes.left);
                set(&mut props.indentation.right, changes.right);
                if let Some(first_line) = changes.first_line {
                    // «(нет)» записывается нулём: иначе останется отступ
                    // из стиля.
                    props.indentation.first_line =
                        Some(first_line.unwrap_or(FirstLineIndent::FirstLine(Twips(0))));
                }
                set(&mut props.spacing.before, changes.before);
                set(&mut props.spacing.after, changes.after);
                set(&mut props.spacing.line, changes.line);
                set(&mut props.contextual_spacing, changes.contextual_spacing);
                set(&mut props.widow_control, changes.widow_control);
                set(&mut props.keep_next, changes.keep_next);
                set(&mut props.keep_lines, changes.keep_lines);
                set(&mut props.page_break_before, changes.page_break_before);
            }
            ParagraphFormat::Shading(fill) => {
                let in_style = styled.as_ref().is_some_and(|s| s.shading.is_some());
                props.shading = match fill {
                    Some((r, g, b)) => Some(Shading::fill(Color::Rgb(r, g, b))),
                    None if in_style => Some(Shading::fill(Color::Auto)),
                    None => None,
                };
            }
            ParagraphFormat::Border(edge) => {
                let style = styled.map(|s| s.borders).unwrap_or_default();
                set_borders(props, &style, edge, borders_on);
            }
        }
    }
}

/// Шрифт для латиницы, кириллицы и сложных письменностей (ссылки на
/// шрифты темы снимаются).
fn set_font(props: &mut RunProps, name: &str) {
    let fonts = &mut props.fonts;
    fonts.ascii = Some(name.to_owned());
    fonts.h_ansi = Some(name.to_owned());
    fonts.complex_script = Some(name.to_owned());
    fonts.ascii_theme = None;
    fonts.h_ansi_theme = None;
    fonts.complex_script_theme = None;
}

/// Ставит `value` в `target`, если оно задано.
fn set<T>(target: &mut Option<T>, value: Option<T>) {
    if value.is_some() {
        *target = value;
    }
}

/// Меняет итоговые позиции табуляции абзаца `index` и записывает в абзац
/// только отличия от стиля: новые позиции и [`TabStopKind::Clear`] для
/// убранных позиций стиля.
fn change_tabs(doc: &mut Document, index: usize, change: impl FnOnce(&mut Vec<TabStop>)) {
    let Some(paragraph) = paragraph_ref(doc, index) else {
        return;
    };
    let mut tabs = doc.resolve_paragraph(&paragraph.props).tabs;
    change(&mut tabs);
    let mut own = paragraph.props.clone();
    own.tabs.clear();
    let inherited = doc.resolve_paragraph(&own).tabs;
    let mut direct: Vec<TabStop> = inherited
        .iter()
        .filter(|t| !tabs.iter().any(|n| n.position == t.position))
        .map(|t| TabStop {
            position: t.position,
            kind: TabStopKind::Clear,
            leader: TabLeader::None,
        })
        .chain(tabs.iter().filter(|t| !inherited.contains(t)).copied())
        .collect();
    direct.sort_by_key(|t| t.position);
    if let Some(paragraph) = paragraph_mut(doc, index) {
        paragraph.props.tabs = direct;
    }
}

/// Применяет стиль `id` (галерея «Стили» на ленте): стиль абзаца —
/// к абзацам выделенного, стиль символов — к выделенному тексту (или
/// слову под курсором). Неизвестный стиль ничего не меняет.
pub fn apply_style(doc: &mut Document, start: Position, end: Position, id: &str) {
    match doc.styles.get(id).map(|s| s.kind) {
        Some(StyleKind::Paragraph) => {
            let (first, last) = ordered(start, end);
            for index in first.paragraph..=last.paragraph {
                if let Some(paragraph) = paragraph_mut(doc, index) {
                    paragraph.props.style = Some(id.to_owned());
                }
            }
        }
        Some(StyleKind::Character) => {
            format_runs(doc, start, end, RunFormat::CharStyle(id.to_owned()));
        }
        _ => {}
    }
}

/// Оформление в позиции `at`, как его показывает лента Word: абзаца
/// и буквы перед курсором (в начале абзаца — первой буквы). `None`,
/// если такого абзаца нет.
pub fn formatting_at(
    doc: &Document,
    at: Position,
) -> Option<(ResolvedParagraphProps, ResolvedRunProps)> {
    let (paragraph, run) = run_at(doc, at)?;
    let default = RunProps::default();
    let run = run.unwrap_or(&default);
    Some((
        doc.resolve_paragraph(&paragraph.props),
        doc.resolve_run(&paragraph.props, run),
    ))
}

/// Язык текста в позиции `at` (`"ru-RU"`), как его показывает строка
/// состояния Word: буквы перед курсором (в начале абзаца — первой).
/// `None`, если язык нигде не указан или абзаца нет.
pub fn language_at(doc: &Document, at: Position) -> Option<String> {
    let (paragraph, run) = run_at(doc, at)?;
    let default = RunProps::default();
    doc.run_language(&paragraph.props, run.unwrap_or(&default))
}

/// Абзац в позиции `at` и оформление буквы перед ней (в начале абзаца —
/// первой буквы; `None`, если в абзаце нет фрагментов).
fn run_at(doc: &Document, at: Position) -> Option<(&letter_model::Paragraph, Option<&RunProps>)> {
    let paragraph = paragraph_ref(doc, at.paragraph)?;
    let mut pos = 0;
    let mut chosen: Option<&RunProps> = None;
    for inline in &paragraph.content {
        let Inline::Run(run) = inline else { continue };
        let len: usize = run
            .content
            .iter()
            .map(letter_model::RunContent::text_len)
            .sum();
        if chosen.is_none() || (len > 0 && pos < at.offset) {
            chosen = Some(&run.props);
        }
        pos += len;
        if pos >= at.offset && at.offset > 0 && len > 0 {
            break;
        }
    }
    Some((paragraph, chosen))
}

fn ordered(a: Position, b: Position) -> (Position, Position) {
    if (a.paragraph, a.offset) <= (b.paragraph, b.offset) {
        (a, b)
    } else {
        (b, a)
    }
}

/// Куски абзацев, которые надо оформить.
fn pieces(doc: &Document, start: Position, end: Position) -> Vec<Piece> {
    let (start, end) = ordered(start, end);
    if (start.paragraph, start.offset) == (end.paragraph, end.offset) {
        let Some(paragraph) = paragraph_ref(doc, start.paragraph) else {
            return Vec::new();
        };
        let text = paragraph.plain_text();
        if text.is_empty() {
            return vec![(start.paragraph, 0, 0)];
        }
        // Слово, внутри которого курсор (не на его краю).
        return text
            .split_word_bound_indices()
            .find(|(i, w)| {
                *i < start.offset
                    && start.offset < i + w.len()
                    && w.chars().any(char::is_alphanumeric)
            })
            .map(|(i, w)| vec![(start.paragraph, i, i + w.len())])
            .unwrap_or_default();
    }
    (start.paragraph..=end.paragraph)
        .filter_map(|index| {
            let len = text_len(paragraph_ref(doc, index)?);
            let from = if index == start.paragraph {
                start.offset.min(len)
            } else {
                0
            };
            let to = if index == end.paragraph {
                end.offset.min(len)
            } else {
                len
            };
            // Пустой абзац внутри выделения тоже оформляется.
            (from < to || len == 0).then_some((index, from, to))
        })
        .collect()
}

/// Оформление фрагментов, задетых куском от `from` до `to`. У пустого
/// куска пустого абзаца — оформление его (пустого) фрагмента.
fn run_props_in(paragraph: &Paragraph, from: usize, to: usize) -> impl Iterator<Item = &RunProps> {
    let mut pos = 0;
    paragraph.content.iter().filter_map(move |inline| {
        let Inline::Run(run) = inline else {
            return None;
        };
        let len: usize = run
            .content
            .iter()
            .map(letter_model::RunContent::text_len)
            .sum();
        let (a, b) = (pos, pos + len);
        pos = b;
        let touched = if from == to {
            len == 0
        } else {
            a < to && b > from
        };
        touched.then_some(&run.props)
    })
}

/// Применяет `change` к фрагментам абзаца от `from` до `to`, разрезая
/// фрагменты на краях.
pub(crate) fn apply(
    paragraph: &mut Paragraph,
    from: usize,
    to: usize,
    mut change: impl FnMut(&mut RunProps),
) {
    if from == to {
        if !paragraph
            .content
            .iter()
            .any(|i| matches!(i, Inline::Run(_)))
        {
            paragraph.content.push(Inline::Run(Run::default()));
        }
        for inline in &mut paragraph.content {
            if let Inline::Run(run) = inline
                && run.content.is_empty()
            {
                change(&mut run.props);
            }
        }
        return;
    }
    let tail = split_content(paragraph, to);
    let mut middle = split_content(paragraph, from);
    for inline in &mut middle {
        if let Inline::Run(run) = inline
            && !run.content.is_empty()
        {
            change(&mut run.props);
        }
    }
    paragraph.content.extend(middle);
    paragraph.content.extend(tail);
    tidy(paragraph);
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::{Block, ParagraphProps, Style, StyleKind};

    fn doc(paragraphs: Vec<Paragraph>) -> Document {
        let mut doc = Document::new();
        doc.sections[0].blocks = paragraphs.into_iter().map(Block::from).collect();
        doc
    }

    fn para(doc: &Document, i: usize) -> &Paragraph {
        paragraph_ref(doc, i).unwrap()
    }

    fn para_mut(doc: &mut Document, i: usize) -> &mut Paragraph {
        paragraph_mut(doc, i).unwrap()
    }

    fn runs(doc: &Document, i: usize) -> Vec<(String, Option<bool>)> {
        para(doc, i)
            .runs()
            .map(|r| (r.plain_text(), r.props.bold))
            .collect()
    }

    #[test]
    fn bold_toggles_on_then_off() {
        let mut d = doc(vec![Paragraph::text("abcdef"), Paragraph::text("gh")]);
        format_runs(
            &mut d,
            Position::new(0, 2),
            Position::new(0, 4),
            RunFormat::Bold,
        );
        assert_eq!(
            runs(&d, 0),
            vec![
                ("ab".into(), None),
                ("cd".into(), Some(true)),
                ("ef".into(), None)
            ]
        );
        // Выделение шире полужирного — включается для всего.
        format_runs(
            &mut d,
            Position::new(1, 1),
            Position::new(0, 3),
            RunFormat::Bold,
        );
        assert_eq!(
            runs(&d, 0),
            vec![("ab".into(), None), ("cdef".into(), Some(true))]
        );
        assert_eq!(
            runs(&d, 1),
            vec![("g".into(), Some(true)), ("h".into(), None)]
        );
        // Теперь всё полужирное — выключается.
        format_runs(
            &mut d,
            Position::new(0, 2),
            Position::new(1, 1),
            RunFormat::Bold,
        );
        assert_eq!(
            runs(&d, 0),
            vec![("ab".into(), None), ("cdef".into(), Some(false))]
        );
        assert_eq!(para(&d, 0).plain_text(), "abcdef");
    }

    #[test]
    fn style_bold_counts_as_bold() {
        let mut strong = Style::new("Head", StyleKind::Paragraph);
        strong.run.bold = Some(true);
        let mut d = doc(vec![Paragraph::text("title").with_props(ParagraphProps {
            style: Some("Head".into()),
            ..ParagraphProps::default()
        })]);
        d.styles.styles.push(strong);
        format_runs(
            &mut d,
            Position::new(0, 0),
            Position::new(0, 5),
            RunFormat::Bold,
        );
        assert_eq!(runs(&d, 0), vec![("title".into(), Some(false))]);
    }

    #[test]
    fn cursor_inside_a_word_formats_the_word() {
        let mut d = doc(vec![Paragraph::text("one two"), Paragraph::default()]);
        format_runs(
            &mut d,
            Position::new(0, 5),
            Position::new(0, 5),
            RunFormat::Italic,
        );
        let italic: Vec<_> = para(&d, 0)
            .runs()
            .map(|r| (r.plain_text(), r.props.italic))
            .collect();
        assert_eq!(
            italic,
            vec![("one ".into(), None), ("two".into(), Some(true))]
        );
        // На краю слова — ничего.
        let before = d.clone();
        format_runs(
            &mut d,
            Position::new(0, 3),
            Position::new(0, 3),
            RunFormat::Italic,
        );
        assert_eq!(d, before);
        // Пустой абзац запоминает оформление для набора.
        format_runs(
            &mut d,
            Position::new(1, 0),
            Position::new(1, 0),
            RunFormat::Underline,
        );
        crate::edit::insert_text(&mut d, Position::new(1, 0), "x");
        assert_eq!(
            para(&d, 1).runs().next().map(|r| r.props.underline),
            Some(Some(Underline::Single))
        );
    }

    #[test]
    fn font_size_steps_by_a_point() {
        let mut d = doc(vec![Paragraph::from_runs([
            Run::text("a"),
            Run::text("b").with_props(RunProps {
                size: Some(HalfPoints(3)),
                ..RunProps::default()
            }),
        ])]);
        let base = d
            .resolve_run(&ParagraphProps::default(), &RunProps::default())
            .size
            .0;
        format_runs(
            &mut d,
            Position::new(0, 0),
            Position::new(0, 2),
            RunFormat::Grow,
        );
        let sizes: Vec<_> = para(&d, 0).runs().map(|r| r.props.size).collect();
        assert_eq!(sizes, vec![Some(HalfPoints(base + 2)), Some(HalfPoints(5))]);
        for _ in 0..3 {
            format_runs(
                &mut d,
                Position::new(0, 0),
                Position::new(0, 2),
                RunFormat::Shrink,
            );
        }
        let sizes: Vec<_> = para(&d, 0).runs().map(|r| r.props.size).collect();
        assert_eq!(
            sizes,
            vec![Some(HalfPoints(base - 4)), Some(HalfPoints(MIN_SIZE))]
        );
    }

    #[test]
    fn formatting_is_read_before_the_cursor() {
        let mut d = doc(vec![Paragraph::from_runs([
            Run::text("ab"),
            Run::text("cd").with_props(RunProps {
                bold: Some(true),
                ..RunProps::default()
            }),
        ])]);
        let bold = |d: &Document, o| formatting_at(d, Position::new(0, o)).map(|(_, r)| r.bold);
        assert_eq!(bold(&d, 0), Some(false));
        assert_eq!(bold(&d, 2), Some(false));
        assert_eq!(bold(&d, 3), Some(true));
        assert_eq!(bold(&d, 4), Some(true));
        assert_eq!(bold(&d, 9), Some(true));
        align_paragraphs(
            &mut d,
            Position::new(0, 0),
            Position::new(0, 0),
            Alignment::Right,
        );
        let (paragraph, _) = formatting_at(&d, Position::new(0, 1)).unwrap();
        assert_eq!(paragraph.alignment, Alignment::Right);
        assert!(formatting_at(&d, Position::new(5, 0)).is_none());
        // Первая буква, если курсор в начале абзаца.
        let d = doc(vec![Paragraph::from_runs([Run::text("x").with_props(
            RunProps {
                italic: Some(true),
                ..RunProps::default()
            },
        )])]);
        let (_, run) = formatting_at(&d, Position::new(0, 0)).unwrap();
        assert!(run.italic);
    }

    #[test]
    fn language_is_read_before_the_cursor() {
        let english = RunProps {
            unknown: vec![letter_model::RawXml::new(r#"<w:lang w:val="en-US"/>"#)],
            ..RunProps::default()
        };
        let mut d = doc(vec![Paragraph::from_runs([
            Run::text("да"),
            Run::text("yes").with_props(english),
        ])]);
        d.styles.defaults.run.unknown =
            vec![letter_model::RawXml::new(r#"<w:lang w:val="ru-RU"/>"#)];
        let at = |o| language_at(&d, Position::new(0, o));
        assert_eq!(at(0).as_deref(), Some("ru-RU"));
        assert_eq!(at(4).as_deref(), Some("ru-RU"));
        assert_eq!(at(5).as_deref(), Some("en-US"));
        assert_eq!(language_at(&d, Position::new(3, 0)), None);
    }

    #[test]
    fn strike_indexes_and_values() {
        let mut d = doc(vec![Paragraph::text("abcd")]);
        let all = |d: &mut Document, f: RunFormat| {
            format_runs(d, Position::new(0, 0), Position::new(0, 4), f);
        };
        let props = |d: &Document| para(d, 0).runs().next().unwrap().props.clone();
        all(&mut d, RunFormat::Strike);
        assert_eq!(props(&d).strike, Some(true));
        all(&mut d, RunFormat::Strike);
        assert_eq!(props(&d).strike, Some(false));
        // Подстрочный сменяется надстрочным, повтор возвращает обычный.
        all(&mut d, RunFormat::Subscript);
        assert_eq!(props(&d).vertical_align, Some(VerticalAlign::Subscript));
        all(&mut d, RunFormat::Superscript);
        assert_eq!(props(&d).vertical_align, Some(VerticalAlign::Superscript));
        all(&mut d, RunFormat::Superscript);
        assert_eq!(props(&d).vertical_align, Some(VerticalAlign::Baseline));
        all(&mut d, RunFormat::Font("Arial".into()));
        all(&mut d, RunFormat::Size(HalfPoints(28)));
        all(&mut d, RunFormat::Color(Color::Rgb(255, 0, 0)));
        all(&mut d, RunFormat::Highlight(Highlight::Yellow));
        let p = props(&d);
        assert_eq!(p.fonts.ascii.as_deref(), Some("Arial"));
        assert_eq!(p.fonts.h_ansi.as_deref(), Some("Arial"));
        assert_eq!(p.size, Some(HalfPoints(28)));
        assert_eq!(p.color, Some(Color::Rgb(255, 0, 0)));
        assert_eq!(p.highlight, Some(Highlight::Yellow));
        let resolved = d.resolve_run(&ParagraphProps::default(), &p);
        assert_eq!(resolved.fonts.ascii, "Arial");
        // Очистка возвращает оформление по умолчанию.
        all(&mut d, RunFormat::Bold);
        all(&mut d, RunFormat::Clear);
        assert_eq!(props(&d), RunProps::default());
    }

    #[test]
    fn font_dialog_changes_only_what_was_touched() {
        let mut d = doc(vec![Paragraph::from_runs([
            Run::text("ab"),
            Run::text("cd").with_props(RunProps {
                bold: Some(true),
                ..RunProps::default()
            }),
        ])]);
        let changes = RunChanges {
            font: Some("Arial".into()),
            caps: Some(true),
            scale: Some(900),
            underline: Some(Underline::Double),
            underline_color: Some(Some(Color::Rgb(1, 2, 3))),
            ..RunChanges::default()
        };
        assert!(!changes.is_empty() && RunChanges::default().is_empty());
        format_runs(
            &mut d,
            Position::new(0, 0),
            Position::new(0, 4),
            RunFormat::Set(Box::new(changes)),
        );
        let runs: Vec<&RunProps> = para(&d, 0).runs().map(|r| &r.props).collect();
        assert_eq!(runs.len(), 2);
        for props in &runs {
            assert_eq!(props.fonts.h_ansi.as_deref(), Some("Arial"));
            assert_eq!(props.caps, Some(true));
            assert_eq!(props.scale, Some(600));
            assert_eq!(props.underline_color, Some(Color::Rgb(1, 2, 3)));
        }
        // Полужирный остался только у второго куска.
        assert_eq!((runs[0].bold, runs[1].bold), (None, Some(true)));
    }

    #[test]
    fn dialog_changes_only_what_was_touched() {
        let mut d = doc(vec![Paragraph::text("a"), Paragraph::text("b")]);
        para_mut(&mut d, 1).props.alignment = Some(Alignment::Center);
        let changes = ParagraphChanges {
            left: Some(Twips(567)),
            first_line: Some(None),
            line: Some(LineSpacing::Exact(Twips(300))),
            keep_next: Some(true),
            ..ParagraphChanges::default()
        };
        assert!(!changes.is_empty() && ParagraphChanges::default().is_empty());
        format_paragraphs(
            &mut d,
            Position::new(0, 0),
            Position::new(1, 0),
            ParagraphFormat::Set(changes),
        );
        for i in 0..2 {
            let r = d.resolve_paragraph(&para(&d, i).props);
            assert_eq!(r.indent_left, Twips(567));
            assert_eq!(r.line_spacing, LineSpacing::Exact(Twips(300)));
            assert!(r.keep_next);
            assert!(matches!(
                r.first_line,
                None | Some(FirstLineIndent::FirstLine(Twips(0)))
            ));
        }
        assert_eq!(para(&d, 0).props.alignment, None);
        assert_eq!(para(&d, 1).props.alignment, Some(Alignment::Center));
    }

    #[test]
    fn ruler_indents_and_tabs() {
        let mut d = doc(vec![Paragraph::text("a"), Paragraph::text("b")]);
        let stop = |at: i32, kind| TabStop {
            position: Twips(at),
            kind,
            leader: TabLeader::None,
        };
        // Стиль абзаца даёт позицию 1000.
        let mut normal = letter_model::Style::new("Normal", StyleKind::Paragraph);
        normal.is_default = true;
        normal.paragraph.tabs = vec![stop(1000, TabStopKind::Left)];
        d.styles.styles.push(normal);
        let run = |d: &mut Document, f| {
            format_paragraphs(d, Position::new(0, 0), Position::new(1, 0), f);
        };
        let tabs = |d: &Document, i| d.resolve_paragraph(&para(d, i).props).tabs;
        run(
            &mut d,
            ParagraphFormat::Indents {
                left: Twips(720),
                right: Twips(360),
                first_line: Some(FirstLineIndent::Hanging(Twips(360))),
            },
        );
        let resolved = d.resolve_paragraph(&para(&d, 1).props);
        assert_eq!(
            (
                resolved.indent_left,
                resolved.indent_right,
                resolved.first_line
            ),
            (
                Twips(720),
                Twips(360),
                Some(FirstLineIndent::Hanging(Twips(360)))
            )
        );
        run(
            &mut d,
            ParagraphFormat::AddTab(stop(2000, TabStopKind::Center)),
        );
        assert_eq!(
            tabs(&d, 0),
            [
                stop(1000, TabStopKind::Left),
                stop(2000, TabStopKind::Center)
            ]
        );
        // В абзаце — только новая позиция, позиция стиля не повторяется.
        assert_eq!(para(&d, 0).props.tabs, [stop(2000, TabStopKind::Center)]);
        run(
            &mut d,
            ParagraphFormat::MoveTab {
                from: Twips(1000),
                to: Twips(1500),
            },
        );
        assert_eq!(
            tabs(&d, 1),
            [
                stop(1500, TabStopKind::Left),
                stop(2000, TabStopKind::Center)
            ]
        );
        // Позиция стиля убрана знаком «очистить».
        assert!(
            para(&d, 1)
                .props
                .tabs
                .contains(&stop(1000, TabStopKind::Clear))
        );
        run(&mut d, ParagraphFormat::RemoveTab(Twips(2000)));
        run(&mut d, ParagraphFormat::RemoveTab(Twips(1500)));
        assert!(tabs(&d, 0).is_empty());
        // Возврат позиции стиля снимает «очистить».
        run(
            &mut d,
            ParagraphFormat::AddTab(stop(1000, TabStopKind::Left)),
        );
        assert!(para(&d, 0).props.tabs.is_empty());
        // Перенос несуществующей позиции ничего не меняет.
        run(
            &mut d,
            ParagraphFormat::MoveTab {
                from: Twips(7),
                to: Twips(8),
            },
        );
        assert_eq!(tabs(&d, 0), [stop(1000, TabStopKind::Left)]);
    }

    #[test]
    fn word_size_steps() {
        let grow = |s| size_step(s, true);
        let shrink = |s| size_step(s, false);
        assert_eq!((grow(22), grow(21), grow(20)), (24, 22, 21));
        assert_eq!(
            (grow(12), grow(56), grow(144), grow(150)),
            (14, 72, 160, 160)
        );
        assert_eq!((shrink(24), shrink(21), shrink(17)), (22, 20, 16));
        assert_eq!((shrink(16), shrink(160), shrink(200)), (14, 144, 180));
        assert_eq!(shrink(2), 0);
    }

    #[test]
    fn caps_hidden_and_underlines_toggle() {
        let mut d = doc(vec![Paragraph::text("abcd")]);
        let all = |d: &mut Document, f: RunFormat| {
            format_runs(d, Position::new(0, 0), Position::new(0, 4), f);
        };
        let props = |d: &Document| para(d, 0).runs().next().unwrap().props.clone();
        all(&mut d, RunFormat::SmallCaps);
        assert_eq!(props(&d).small_caps, Some(true));
        // Все прописные выключают малые.
        all(&mut d, RunFormat::Caps);
        assert_eq!(
            (props(&d).caps, props(&d).small_caps),
            (Some(true), Some(false))
        );
        all(&mut d, RunFormat::Caps);
        assert_eq!(props(&d).caps, Some(false));
        all(&mut d, RunFormat::Hidden);
        assert_eq!(props(&d).hidden, Some(true));
        all(&mut d, RunFormat::DoubleUnderline);
        assert_eq!(props(&d).underline, Some(Underline::Double));
        all(&mut d, RunFormat::WordUnderline);
        assert_eq!(props(&d).underline, Some(Underline::Words));
        all(&mut d, RunFormat::WordUnderline);
        assert_eq!(props(&d).underline, Some(Underline::None));
        all(&mut d, RunFormat::GrowStep);
        assert_eq!(props(&d).size, Some(HalfPoints(21)));
    }

    #[test]
    fn hanging_space_before_and_reset() {
        let mut d = doc(vec![Paragraph::text("a"), Paragraph::text("b")]);
        d.settings.default_tab_stop = Twips(709);
        let run = |d: &mut Document, f| {
            format_paragraphs(d, Position::new(0, 0), Position::new(1, 0), f);
        };
        let indents = |d: &Document| para(d, 1).props.indentation;
        run(&mut d, ParagraphFormat::HangingMore);
        assert_eq!(indents(&d).left, Some(Twips(709)));
        assert_eq!(
            indents(&d).first_line,
            Some(FirstLineIndent::Hanging(Twips(709)))
        );
        run(&mut d, ParagraphFormat::HangingMore);
        assert_eq!(
            indents(&d).first_line,
            Some(FirstLineIndent::Hanging(Twips(1418)))
        );
        run(&mut d, ParagraphFormat::HangingLess);
        run(&mut d, ParagraphFormat::HangingLess);
        run(&mut d, ParagraphFormat::HangingLess);
        assert_eq!(indents(&d).left, Some(Twips(0)));
        assert_eq!(
            indents(&d).first_line,
            Some(FirstLineIndent::FirstLine(Twips(0)))
        );
        run(&mut d, ParagraphFormat::ToggleSpaceBefore);
        assert_eq!(para(&d, 1).props.spacing.before, Some(Twips(240)));
        run(&mut d, ParagraphFormat::ToggleSpaceBefore);
        assert_eq!(para(&d, 1).props.spacing.before, Some(Twips(0)));
        para_mut(&mut d, 0).props.style = Some("Heading1".into());
        run(&mut d, ParagraphFormat::Reset);
        assert_eq!(para(&d, 0).props.style.as_deref(), Some("Heading1"));
        assert_eq!(para(&d, 0).props.indentation, Default::default());
        assert_eq!(para(&d, 1).props.spacing.before, None);
    }

    #[test]
    fn indents_step_by_the_default_tab() {
        let mut d = doc(vec![Paragraph::text("a"), Paragraph::text("b")]);
        d.settings.default_tab_stop = Twips(709);
        let left = |d: &Document, i| para(d, i).props.indentation.left;
        let run = |d: &mut Document, f| {
            format_paragraphs(d, Position::new(0, 0), Position::new(1, 0), f);
        };
        run(&mut d, ParagraphFormat::IndentMore);
        assert_eq!(left(&d, 0), Some(Twips(709)));
        para_mut(&mut d, 1).props.indentation.left = Some(Twips(1000));
        run(&mut d, ParagraphFormat::IndentMore);
        assert_eq!(
            (left(&d, 0), left(&d, 1)),
            (Some(Twips(1418)), Some(Twips(1418)))
        );
        para_mut(&mut d, 1).props.indentation.left = Some(Twips(1000));
        run(&mut d, ParagraphFormat::IndentLess);
        assert_eq!(
            (left(&d, 0), left(&d, 1)),
            (Some(Twips(709)), Some(Twips(709)))
        );
        run(&mut d, ParagraphFormat::IndentLess);
        run(&mut d, ParagraphFormat::IndentLess);
        assert_eq!(left(&d, 0), Some(Twips(0)));
        run(
            &mut d,
            ParagraphFormat::LineSpacing(LineSpacing::ONE_AND_HALF),
        );
        run(&mut d, ParagraphFormat::SpaceBefore(Twips(240)));
        run(&mut d, ParagraphFormat::SpaceAfter(Twips(0)));
        let resolved = d.resolve_paragraph(&para(&d, 1).props);
        assert_eq!(resolved.line_spacing, LineSpacing::ONE_AND_HALF);
        assert_eq!(
            (resolved.space_before, resolved.space_after),
            (Twips(240), Twips(0))
        );
    }

    #[test]
    fn styles_apply_to_paragraphs_or_text() {
        let mut d = doc(vec![Paragraph::text("one two"), Paragraph::text("b")]);
        let mut strong = letter_model::Style::new("Strong", StyleKind::Character);
        strong.run.bold = Some(true);
        d.styles.styles.push(strong);
        d.styles
            .styles
            .push(letter_model::Style::new("Heading1", StyleKind::Paragraph));
        apply_style(&mut d, Position::new(0, 1), Position::new(1, 0), "Heading1");
        assert_eq!(para(&d, 0).props.style.as_deref(), Some("Heading1"));
        assert_eq!(para(&d, 1).props.style.as_deref(), Some("Heading1"));
        apply_style(&mut d, Position::new(0, 5), Position::new(0, 5), "Strong");
        let styles: Vec<_> = para(&d, 0)
            .runs()
            .map(|r| (r.plain_text(), r.props.style.clone()))
            .collect();
        assert_eq!(
            styles,
            vec![("one ".into(), None), ("two".into(), Some("Strong".into()))]
        );
        let before = d.clone();
        apply_style(&mut d, Position::new(0, 0), Position::new(1, 0), "Missing");
        assert_eq!(d, before);
    }

    #[test]
    fn borders_and_shading_toggle() {
        let mut d = doc(vec![Paragraph::text("a"), Paragraph::text("b")]);
        let (start, end) = (Position::new(0, 0), Position::new(1, 0));
        let border = |edge| ParagraphFormat::Border(edge);
        format_paragraphs(&mut d, start, end, border(BorderEdge::Bottom));
        for i in 0..2 {
            let b = para(&d, i).props.borders.clone().unwrap_or_default();
            assert_eq!(b.bottom.as_ref().map(|b| b.style.as_str()), Some("single"));
            assert!(b.top.is_none());
        }
        // Второй раз — убрать.
        format_paragraphs(&mut d, start, end, border(BorderEdge::Bottom));
        assert_eq!(para(&d, 0).props.borders, None);
        format_paragraphs(&mut d, start, end, border(BorderEdge::All));
        let all = para(&d, 1).props.borders.clone().unwrap_or_default();
        assert!(BorderEdge::All.is_set(&all) && BorderEdge::Inside.is_set(&all));
        assert_eq!(all.left.and_then(|b| b.space), Some(4));
        format_paragraphs(&mut d, start, end, border(BorderEdge::None));
        assert_eq!(para(&d, 1).props.borders, None);

        // Линия из стиля убирается видом «nil».
        let mut style = Style::new("Boxed", StyleKind::Paragraph);
        style.paragraph.borders = Some(ParagraphBorders::frame(Border::single(4)));
        style.paragraph.shading = Some(Shading::fill(Color::Rgb(1, 2, 3)));
        d.styles.styles.push(style);
        para_mut(&mut d, 0).props.style = Some("Boxed".into());
        format_paragraphs(&mut d, start, start, border(BorderEdge::Top));
        let top = para(&d, 0).props.borders.clone().and_then(|b| b.top);
        assert_eq!(top.map(|b| b.style), Some("nil".to_string()));
        let resolved = d.resolve_paragraph(&para(&d, 0).props);
        assert!(!BorderEdge::Top.is_set(&resolved.borders));
        assert!(BorderEdge::Bottom.is_set(&resolved.borders));

        let shade = |fill| ParagraphFormat::Shading(fill);
        format_paragraphs(&mut d, start, end, shade(Some((255, 0, 0))));
        assert_eq!(
            para(&d, 1)
                .props
                .shading
                .as_ref()
                .and_then(Shading::background),
            Some((255, 0, 0))
        );
        format_paragraphs(&mut d, start, end, shade(None));
        assert_eq!(para(&d, 1).props.shading, None);
        let styled = d.resolve_paragraph(&para(&d, 0).props);
        assert_eq!(styled.shading.and_then(|s| s.background()), None);
    }

    #[test]
    fn alignment_toggles_back_to_left() {
        let mut d = doc(vec![Paragraph::text("a"), Paragraph::text("b")]);
        let align = |d: &Document, i| para(d, i).props.alignment;
        align_paragraphs(
            &mut d,
            Position::new(0, 0),
            Position::new(1, 0),
            Alignment::Center,
        );
        assert_eq!(align(&d, 0), Some(Alignment::Center));
        assert_eq!(align(&d, 1), Some(Alignment::Center));
        align_paragraphs(
            &mut d,
            Position::new(1, 0),
            Position::new(1, 0),
            Alignment::Center,
        );
        assert_eq!(align(&d, 1), Some(Alignment::Left));
        assert_eq!(align(&d, 0), Some(Alignment::Center));
        align_paragraphs(
            &mut d,
            Position::new(0, 0),
            Position::new(0, 0),
            Alignment::Left,
        );
        assert_eq!(align(&d, 0), Some(Alignment::Left));
    }
}
