//! Итоговое оформление с учётом стилей (задача F1.4).
//!
//! Word складывает оформление слоями, от общего к частному:
//!
//! 1. значения документа по умолчанию (`w:docDefaults`);
//! 2. стиль абзаца и все его родители (`w:basedOn`);
//! 3. стиль символов и его родители (только для символов);
//! 4. прямое оформление абзаца или фрагмента.
//!
//! Каждый следующий слой перекрывает то, что задано в нём явно.
//! Исключение — «переключатели» (жирный, курсив, зачёркивание): в стилях
//! они не перекрываются, а *переключаются*. Жирный стиль символов внутри
//! жирного стиля абзаца даёт обычный текст. Прямое оформление задаёт
//! значение окончательно.

use crate::{
    Alignment, Borders, Color, Document, FirstLineIndent, HalfPoints, Highlight, LineSpacing,
    NumberingRef, ParagraphBorders, ParagraphProps, RunFonts, RunProps, Shading, Style, StyleKind,
    Styles, TabStop, TabStopKind, TableArea, TableAreaStyle, TableAreas, TableProps, ThemeFont,
    Twips, Underline, VerticalAlign,
};

/// Где стоит абзац таблицы: стиль таблицы и части таблицы (строка
/// заголовка, полосы…), к которым относится его ячейка.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct TableCellStyle {
    /// Стиль таблицы; `""` — стиль таблиц по умолчанию.
    pub style: String,
    /// Части таблицы.
    pub areas: TableAreas,
}

/// Шрифт, который используется, если ни один слой его не задал.
const FALLBACK_FONT: &str = "Times New Roman";

/// Сколько уровней `basedOn` проходить. Защищает от бесконечных циклов
/// в испорченных файлах.
const MAX_STYLE_DEPTH: usize = 64;

/// Итоговое оформление абзаца: все значения известны.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedParagraphProps {
    /// Стиль, который на самом деле применён (с учётом стиля по умолчанию).
    pub style: Option<String>,
    /// Выравнивание.
    pub alignment: Alignment,
    /// Отступ слева.
    pub indent_left: Twips,
    /// Отступ справа.
    pub indent_right: Twips,
    /// Отступ или выступ первой строки.
    pub first_line: Option<FirstLineIndent>,
    /// Интервал перед абзацем.
    pub space_before: Twips,
    /// Интервал после абзаца.
    pub space_after: Twips,
    /// Межстрочный интервал.
    pub line_spacing: LineSpacing,
    /// «Не добавлять интервал между абзацами одного стиля».
    pub contextual_spacing: bool,
    /// «Не отрывать от следующего».
    pub keep_next: bool,
    /// «Не разрывать абзац».
    pub keep_lines: bool,
    /// «С новой страницы».
    pub page_break_before: bool,
    /// «Запрет висячих строк». Если ни один слой его не задал —
    /// включён, как в Word.
    pub widow_control: bool,
    /// «Запретить нумерацию строк».
    pub suppress_line_numbers: bool,
    /// Позиции табуляции из всех слоёв по возрастанию, без
    /// [`TabStopKind::Clear`].
    pub tabs: Vec<TabStop>,
    /// Список и уровень (`numId`, `ilvl`), если абзац в списке.
    pub numbering: Option<(u32, u8)>,
    /// Границы абзаца из всех слоёв по сторонам.
    pub borders: ParagraphBorders,
    /// Заливка абзаца.
    pub shading: Option<Shading>,
    /// Автоматический перенос слов в абзаце запрещён.
    pub suppress_auto_hyphens: bool,
}

/// Итоговые шрифты фрагмента: ссылки на тему заменены именами.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedFonts {
    /// Шрифт для символов ASCII.
    pub ascii: String,
    /// Шрифт для остальных символов, в том числе кириллицы.
    pub h_ansi: String,
    /// Шрифт для восточноазиатского письма.
    pub east_asia: String,
    /// Шрифт для сложного письма.
    pub complex_script: String,
    /// Подсказка для общих символов (`w:hint`).
    pub hint: Option<String>,
}

/// Итоговое оформление символов: все значения известны.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRunProps {
    /// Шрифты.
    pub fonts: ResolvedFonts,
    /// Размер шрифта.
    pub size: HalfPoints,
    /// Полужирный.
    pub bold: bool,
    /// Курсив.
    pub italic: bool,
    /// Подчёркивание.
    pub underline: Underline,
    /// Зачёркивание.
    pub strike: bool,
    /// Двойное зачёркивание.
    pub double_strike: bool,
    /// Цвет текста.
    pub color: Color,
    /// Выделение цветом.
    pub highlight: Highlight,
    /// Над/подстрочный знак.
    pub vertical_align: VerticalAlign,
    /// Все прописные.
    pub caps: bool,
    /// Малые прописные.
    pub small_caps: bool,
    /// Скрытый текст.
    pub hidden: bool,
    /// Цвет подчёркивания; `None` — цвет текста.
    pub underline_color: Option<Color>,
    /// Интервал между буквами (может быть меньше нуля).
    pub spacing: Twips,
    /// Масштаб по ширине, проценты.
    pub scale: u16,
    /// Смещение вверх (вниз — меньше нуля), полупункты.
    pub position: i32,
    /// Кернинг от этого размера; ноль — выключен.
    pub kerning: HalfPoints,
    /// Гиперссылка, частью которой является фрагмент (только прямое
    /// оформление: у стилей ссылок нет).
    pub link: Option<crate::Hyperlink>,
}

impl ResolvedRunProps {
    /// Включён ли кернинг для этого размера шрифта.
    pub fn kerned(&self) -> bool {
        self.kerning.0 > 0 && self.size >= self.kerning
    }
}

impl Styles {
    /// Цепочка стилей от самого дальнего предка к самому стилю.
    /// Если стиль не найден или не задан, берётся стиль по умолчанию
    /// этого вида — так поступает Word.
    fn chain(&self, id: Option<&str>, kind: StyleKind) -> Vec<&Style> {
        let start = id
            .and_then(|id| self.get(id))
            .filter(|s| s.kind == kind)
            .or_else(|| self.default_for(kind));
        let mut chain: Vec<&Style> = Vec::new();
        let mut next = start;
        while let Some(style) = next {
            if chain.len() >= MAX_STYLE_DEPTH || chain.iter().any(|s| s.id == style.id) {
                break;
            }
            chain.push(style);
            next = style
                .based_on
                .as_deref()
                .and_then(|id| self.get(id))
                .filter(|s| s.kind == kind);
        }
        chain.reverse();
        chain
    }
}

impl Document {
    /// Итоговое оформление абзаца с учётом стилей.
    pub fn resolve_paragraph(&self, direct: &ParagraphProps) -> ResolvedParagraphProps {
        self.resolve_paragraph_in(direct, None)
    }

    /// Итоговое оформление абзаца в ячейке таблицы `table`:
    /// оформление абзаца из стиля таблицы (и из оформления частей
    /// таблицы, к которым относится ячейка) ложится сразу после значений
    /// по умолчанию, под стилем абзаца. `None` — абзац вне таблицы.
    pub fn resolve_paragraph_in(
        &self,
        direct: &ParagraphProps,
        table: Option<&TableCellStyle>,
    ) -> ResolvedParagraphProps {
        let table_chain = self.table_chain(table.map(|t| t.style.as_str()));
        let areas = self.table_areas(table);
        let chain = self
            .styles
            .chain(direct.style.as_deref(), StyleKind::Paragraph);
        // Список: номер и уровень могут прийти из разных слоёв.
        let mut list = NumberingRef::default();
        for layer in std::iter::once(&self.styles.defaults.paragraph)
            .chain(chain.iter().map(|s| &s.paragraph))
            .chain(std::iter::once(direct))
        {
            if let Some(numbering) = layer.numbering {
                overlay(&mut list.id, &numbering.id);
                overlay(&mut list.level, &numbering.level);
            }
        }
        let numbering = match list.id {
            Some(id) if id != 0 => Some((id, list.level.unwrap_or(0).min(8))),
            _ => None,
        };
        // Оформление уровня списка (отступы) — после стиля, но до
        // оформления самого абзаца, как в Word.
        let level = numbering.and_then(|(id, level)| self.numbering.level(id, level));
        let layers = std::iter::once(&self.styles.defaults.paragraph)
            .chain(table_chain.iter().map(|s| &s.paragraph))
            .chain(areas.iter().map(|a| &a.paragraph))
            .chain(chain.iter().map(|s| &s.paragraph))
            .chain(level.as_ref().map(|l| &l.paragraph))
            .chain(std::iter::once(direct));

        let mut acc = ParagraphProps::default();
        let mut tabs: Vec<TabStop> = Vec::new();
        let mut borders = ParagraphBorders::default();
        for layer in layers {
            if let Some(layer) = &layer.borders {
                borders.overlay(layer);
            }
            for stop in &layer.tabs {
                tabs.retain(|t| t.position != stop.position);
                if stop.kind != TabStopKind::Clear {
                    tabs.push(*stop);
                }
            }
            overlay(&mut acc.alignment, &layer.alignment);
            overlay(&mut acc.indentation.left, &layer.indentation.left);
            overlay(&mut acc.indentation.right, &layer.indentation.right);
            overlay(
                &mut acc.indentation.first_line,
                &layer.indentation.first_line,
            );
            overlay(&mut acc.spacing.before, &layer.spacing.before);
            overlay(&mut acc.spacing.after, &layer.spacing.after);
            overlay(&mut acc.spacing.line, &layer.spacing.line);
            overlay(&mut acc.contextual_spacing, &layer.contextual_spacing);
            overlay(&mut acc.keep_next, &layer.keep_next);
            overlay(&mut acc.keep_lines, &layer.keep_lines);
            overlay(&mut acc.page_break_before, &layer.page_break_before);
            overlay(&mut acc.widow_control, &layer.widow_control);
            overlay(&mut acc.suppress_line_numbers, &layer.suppress_line_numbers);
            overlay(&mut acc.shading, &layer.shading);
            overlay(&mut acc.suppress_auto_hyphens, &layer.suppress_auto_hyphens);
        }
        tabs.sort_by_key(|t| t.position);
        ResolvedParagraphProps {
            style: chain.last().map(|s| s.id.clone()),
            alignment: acc.alignment.unwrap_or(Alignment::Left),
            indent_left: acc.indentation.left.unwrap_or_default(),
            indent_right: acc.indentation.right.unwrap_or_default(),
            first_line: acc.indentation.first_line,
            space_before: acc.spacing.before.unwrap_or_default(),
            space_after: acc.spacing.after.unwrap_or_default(),
            line_spacing: acc.spacing.line.unwrap_or(LineSpacing::SINGLE),
            contextual_spacing: acc.contextual_spacing.unwrap_or(false),
            keep_next: acc.keep_next.unwrap_or(false),
            keep_lines: acc.keep_lines.unwrap_or(false),
            page_break_before: acc.page_break_before.unwrap_or(false),
            widow_control: acc.widow_control.unwrap_or(true),
            suppress_line_numbers: acc.suppress_line_numbers.unwrap_or(false),
            tabs,
            numbering,
            borders,
            shading: acc.shading,
            suppress_auto_hyphens: acc.suppress_auto_hyphens.unwrap_or(false),
        }
    }

    /// Итоговое оформление фрагмента `run` в абзаце с оформлением `paragraph`.
    pub fn resolve_run(&self, paragraph: &ParagraphProps, run: &RunProps) -> ResolvedRunProps {
        self.resolve_run_in(paragraph, run, None)
    }

    /// Итоговое оформление фрагмента в ячейке таблицы `table`
    /// (см. [`Document::resolve_paragraph_in`]).
    pub fn resolve_run_in(
        &self,
        paragraph: &ParagraphProps,
        run: &RunProps,
        table: Option<&TableCellStyle>,
    ) -> ResolvedRunProps {
        let table_chain = self.table_chain(table.map(|t| t.style.as_str()));
        let areas = self.table_areas(table);
        let defaults = &self.styles.defaults.run;
        let para_chain = self
            .styles
            .chain(paragraph.style.as_deref(), StyleKind::Paragraph);
        let char_chain = self
            .styles
            .chain(run.style.as_deref(), StyleKind::Character);
        let para_layers: Vec<&RunProps> = table_chain
            .iter()
            .map(|s| &s.run)
            .chain(areas.iter().map(|a| &a.run))
            .chain(para_chain.iter().map(|s| &s.run))
            .collect();
        let char_layers: Vec<&RunProps> = char_chain.iter().map(|s| &s.run).collect();
        let layers: Vec<&RunProps> = std::iter::once(defaults)
            .chain(para_layers.iter().copied())
            .chain(char_layers.iter().copied())
            .chain(std::iter::once(run))
            .collect();

        let mut acc = RunProps::default();
        let mut fonts = FontSlots::default();
        for layer in &layers {
            fonts.overlay(&layer.fonts);
            overlay(&mut acc.size, &layer.size);
            overlay(&mut acc.underline, &layer.underline);
            overlay(&mut acc.color, &layer.color);
            overlay(&mut acc.highlight, &layer.highlight);
            overlay(&mut acc.vertical_align, &layer.vertical_align);
            overlay(&mut acc.underline_color, &layer.underline_color);
            overlay(&mut acc.spacing, &layer.spacing);
            overlay(&mut acc.scale, &layer.scale);
            overlay(&mut acc.position, &layer.position);
            overlay(&mut acc.kerning, &layer.kerning);
        }

        let toggle = |get: fn(&RunProps) -> Option<bool>| {
            resolve_toggle(get(defaults), &para_layers, &char_layers, get(run), get)
        };
        ResolvedRunProps {
            fonts: fonts.resolve(self),
            size: acc.size.unwrap_or(HalfPoints(20)),
            bold: toggle(|p| p.bold),
            italic: toggle(|p| p.italic),
            underline: acc.underline.unwrap_or(Underline::None),
            strike: toggle(|p| p.strike),
            double_strike: toggle(|p| p.double_strike),
            color: acc.color.unwrap_or(Color::Auto),
            highlight: acc.highlight.unwrap_or(Highlight::None),
            vertical_align: acc.vertical_align.unwrap_or(VerticalAlign::Baseline),
            caps: toggle(|p| p.caps),
            small_caps: toggle(|p| p.small_caps),
            hidden: toggle(|p| p.hidden),
            underline_color: acc.underline_color,
            spacing: acc.spacing.unwrap_or_default(),
            scale: acc.scale.unwrap_or(100),
            position: acc.position.unwrap_or(0),
            kerning: acc.kerning.unwrap_or_default(),
            link: run.link.clone(),
        }
    }
}

impl Document {
    /// Стиль таблицы и его родители; `None` — не в таблице. Если стиль
    /// таблицы не задан или не найден — стиль таблиц по умолчанию.
    fn table_chain(&self, table: Option<&str>) -> Vec<&Style> {
        match table {
            Some(id) => self.styles.chain(Some(id), StyleKind::Table),
            None => Vec::new(),
        }
    }

    /// Оформление частей таблицы, к которым относится ячейка `table`,
    /// в том порядке, в каком Word его накладывает: по частям (см.
    /// [`TableArea::ORDER`]), внутри части — от родительского стиля
    /// к самому стилю таблицы.
    pub fn table_areas(&self, table: Option<&TableCellStyle>) -> Vec<&TableAreaStyle> {
        let Some(table) = table else {
            return Vec::new();
        };
        let chain = self.table_chain(Some(&table.style));
        TableArea::ORDER
            .iter()
            .filter(|area| table.areas.contains(**area))
            .flat_map(|area| {
                chain
                    .iter()
                    .flat_map(|s| &s.table_areas)
                    .filter(move |a| a.area == *area)
            })
            .collect()
    }

    /// Итоговые свойства таблицы: стиль таблицы (или стиль таблиц по
    /// умолчанию) с родителями, поверх — прямое оформление. Границы
    /// и поля ячеек перекрываются по сторонам. В `style` — стиль, который
    /// на самом деле применён.
    pub fn resolve_table(&self, direct: &TableProps) -> TableProps {
        let chain = self.table_chain(Some(direct.style.as_deref().unwrap_or("")));
        let mut acc = TableProps::default();
        for layer in chain
            .iter()
            .map(|s| &s.table)
            .chain(std::iter::once(direct))
        {
            overlay(&mut acc.width, &layer.width);
            overlay(&mut acc.alignment, &layer.alignment);
            overlay(&mut acc.indent, &layer.indent);
            overlay(&mut acc.shading, &layer.shading);
            overlay(&mut acc.layout, &layer.layout);
            overlay(&mut acc.look, &layer.look);
            overlay(&mut acc.row_band_size, &layer.row_band_size);
            overlay(&mut acc.column_band_size, &layer.column_band_size);
            if let Some(borders) = &layer.borders {
                acc.borders = Some(overlay_borders(acc.borders.take(), borders));
            }
            if let Some(margins) = &layer.cell_margins {
                let mut m = acc.cell_margins.unwrap_or_default();
                overlay(&mut m.top, &margins.top);
                overlay(&mut m.left, &margins.left);
                overlay(&mut m.bottom, &margins.bottom);
                overlay(&mut m.right, &margins.right);
                acc.cell_margins = Some(m);
            }
        }
        acc.style = chain.last().map(|s| s.id.clone());
        acc
    }
}

/// Границы `layer` поверх `acc` по сторонам.
pub fn overlay_borders(acc: Option<Borders>, layer: &Borders) -> Borders {
    let mut acc = acc.unwrap_or_default();
    overlay(&mut acc.top, &layer.top);
    overlay(&mut acc.left, &layer.left);
    overlay(&mut acc.bottom, &layer.bottom);
    overlay(&mut acc.right, &layer.right);
    overlay(&mut acc.inside_h, &layer.inside_h);
    overlay(&mut acc.inside_v, &layer.inside_v);
    acc
}

impl Document {
    /// Язык текста фрагмента `run` в абзаце с оформлением `paragraph`
    /// (`w:lang w:val`, например `"ru-RU"`): из самого фрагмента, его
    /// стиля, стиля абзаца или оформления по умолчанию. `None`, если язык
    /// нигде не указан.
    pub fn run_language(&self, paragraph: &ParagraphProps, run: &RunProps) -> Option<String> {
        let language = |props: &RunProps| {
            props
                .unknown
                .iter()
                .filter(|raw| raw.local_name() == "lang")
                .find_map(|raw| raw.attribute("val"))
                .filter(|val| !val.is_empty())
                .map(str::to_owned)
        };
        let para_chain = self
            .styles
            .chain(paragraph.style.as_deref(), StyleKind::Paragraph);
        let char_chain = self
            .styles
            .chain(run.style.as_deref(), StyleKind::Character);
        // Цепочки идут от родителей к стилю: ближний слой — последний.
        language(run)
            .or_else(|| char_chain.iter().rev().find_map(|s| language(&s.run)))
            .or_else(|| para_chain.iter().rev().find_map(|s| language(&s.run)))
            .or_else(|| language(&self.styles.defaults.run))
    }
}

fn overlay<T: Clone>(acc: &mut Option<T>, layer: &Option<T>) {
    if layer.is_some() {
        acc.clone_from(layer);
    }
}

/// Значение переключателя. Внутри одного стиля и его родителей действует
/// обычное перекрытие; между стилем абзаца и стилем символов значения
/// переключаются (исключающее ИЛИ).
fn resolve_toggle(
    default: Option<bool>,
    para_layers: &[&RunProps],
    char_layers: &[&RunProps],
    direct: Option<bool>,
    get: fn(&RunProps) -> Option<bool>,
) -> bool {
    if let Some(value) = direct {
        return value;
    }
    let last = |layers: &[&RunProps]| layers.iter().rev().find_map(|p| get(p));
    match (last(para_layers), last(char_layers)) {
        (None, None) => default.unwrap_or(false),
        (para, chr) => para.unwrap_or(false) ^ chr.unwrap_or(false),
    }
}

/// Шрифт в одном «гнезде»: имя или ссылка на тему.
#[derive(Debug, Clone)]
enum FontChoice {
    Name(String),
    Theme(ThemeFont),
}

#[derive(Debug, Default)]
struct FontSlots {
    ascii: Option<FontChoice>,
    h_ansi: Option<FontChoice>,
    east_asia: Option<FontChoice>,
    complex_script: Option<FontChoice>,
    hint: Option<String>,
}

impl FontSlots {
    fn overlay(&mut self, fonts: &RunFonts) {
        fn pick(slot: &mut Option<FontChoice>, name: &Option<String>, theme: Option<ThemeFont>) {
            if let Some(theme) = theme {
                *slot = Some(FontChoice::Theme(theme));
            } else if let Some(name) = name {
                *slot = Some(FontChoice::Name(name.clone()));
            }
        }
        pick(&mut self.ascii, &fonts.ascii, fonts.ascii_theme);
        pick(&mut self.h_ansi, &fonts.h_ansi, fonts.h_ansi_theme);
        pick(&mut self.east_asia, &fonts.east_asia, fonts.east_asia_theme);
        pick(
            &mut self.complex_script,
            &fonts.complex_script,
            fonts.complex_script_theme,
        );
        overlay(&mut self.hint, &fonts.hint);
    }

    fn resolve(self, doc: &Document) -> ResolvedFonts {
        let name = |slot: Option<FontChoice>| match slot {
            Some(FontChoice::Name(name)) => name,
            Some(FontChoice::Theme(theme)) => {
                doc.theme.font(theme).unwrap_or(FALLBACK_FONT).to_string()
            }
            None => FALLBACK_FONT.to_string(),
        };
        ResolvedFonts {
            ascii: name(self.ascii),
            h_ansi: name(self.h_ansi),
            east_asia: name(self.east_asia),
            complex_script: name(self.complex_script),
            hint: self.hint,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocDefaults, Spacing, Theme, ThemeFontSet};

    fn style(id: &str, kind: StyleKind, based_on: Option<&str>) -> Style {
        let mut s = Style::new(id, kind);
        s.based_on = based_on.map(str::to_string);
        s
    }

    fn bold(value: bool) -> RunProps {
        RunProps {
            bold: Some(value),
            ..RunProps::default()
        }
    }

    /// Документ: Normal (по умолчанию) ← Heading1 ← Title; стиль символов Strong.
    fn sample() -> Document {
        let mut normal = style("Normal", StyleKind::Paragraph, None);
        normal.is_default = true;
        normal.paragraph.spacing.after = Some(Twips(200));
        normal.run.size = Some(HalfPoints(22));

        let mut heading = style("Heading1", StyleKind::Paragraph, Some("Normal"));
        heading.paragraph.spacing.before = Some(Twips(480));
        heading.paragraph.spacing.after = Some(Twips(0));
        heading.run = bold(true);
        heading.run.size = Some(HalfPoints(28));
        heading.run.fonts.ascii_theme = Some(ThemeFont::MajorHAnsi);

        let mut title = style("Title", StyleKind::Paragraph, Some("Heading1"));
        title.paragraph.alignment = Some(Alignment::Center);

        let mut strong = style("Strong", StyleKind::Character, None);
        strong.run = bold(true);

        let mut doc = Document::new();
        doc.styles = Styles {
            defaults: DocDefaults {
                run: RunProps {
                    fonts: RunFonts {
                        ascii_theme: Some(ThemeFont::MinorHAnsi),
                        ..RunFonts::default()
                    },
                    ..RunProps::default()
                },
                paragraph: ParagraphProps {
                    spacing: Spacing {
                        line: Some(LineSpacing::Multiple(276)),
                        ..Spacing::default()
                    },
                    ..ParagraphProps::default()
                },
            },
            styles: vec![normal, heading, title, strong],
            unknown: Vec::new(),
        };
        doc.theme = Theme {
            major: ThemeFontSet {
                latin: Some("Cambria".into()),
                ..ThemeFontSet::default()
            },
            minor: ThemeFontSet {
                latin: Some("Calibri".into()),
                ..ThemeFontSet::default()
            },
            ..Theme::default()
        };
        doc
    }

    fn with_style(id: &str) -> ParagraphProps {
        ParagraphProps {
            style: Some(id.into()),
            ..ParagraphProps::default()
        }
    }

    #[test]
    fn paragraph_without_style_uses_default_style() {
        let doc = sample();
        let p = doc.resolve_paragraph(&ParagraphProps::default());
        assert_eq!(p.style.as_deref(), Some("Normal"));
        assert_eq!(p.space_after, Twips(200));
        assert_eq!(p.line_spacing, LineSpacing::Multiple(276));
        assert_eq!(p.alignment, Alignment::Left);
    }

    #[test]
    fn based_on_chain_and_direct_formatting() {
        let doc = sample();
        let title = doc.resolve_paragraph(&with_style("Title"));
        assert_eq!(title.alignment, Alignment::Center);
        assert_eq!(title.space_before, Twips(480));
        assert_eq!(title.space_after, Twips::ZERO);

        let mut direct = with_style("Title");
        direct.alignment = Some(Alignment::Right);
        direct.indentation.left = Some(Twips(720));
        let p = doc.resolve_paragraph(&direct);
        assert_eq!(p.alignment, Alignment::Right);
        assert_eq!(p.indent_left, Twips(720));
    }

    #[test]
    fn page_flow_and_tab_stops() {
        let mut doc = sample();
        let tab = |position, kind| TabStop {
            position: Twips(position),
            kind,
            leader: crate::TabLeader::None,
        };
        doc.styles.styles[1].paragraph.keep_next = Some(true);
        doc.styles.styles[1].paragraph.tabs =
            vec![tab(2000, TabStopKind::Right), tab(1000, TabStopKind::Left)];
        let p = doc.resolve_paragraph(&ParagraphProps::default());
        assert!(!p.keep_next && !p.keep_lines && !p.page_break_before);
        // Запрет висячих строк включён, если его никто не выключал.
        assert!(p.widow_control && p.tabs.is_empty());

        let mut direct = with_style("Title");
        direct.widow_control = Some(false);
        direct.page_break_before = Some(true);
        direct.tabs = vec![
            tab(1000, TabStopKind::Clear),
            tab(3000, TabStopKind::Center),
            tab(2000, TabStopKind::Decimal),
        ];
        let p = doc.resolve_paragraph(&direct);
        assert!(p.keep_next && p.page_break_before && !p.widow_control);
        // Позиция 1000 убрана, 2000 заменена, 3000 добавлена.
        assert_eq!(
            p.tabs,
            [
                tab(2000, TabStopKind::Decimal),
                tab(3000, TabStopKind::Center)
            ]
        );
    }

    #[test]
    fn missing_style_falls_back_to_default() {
        let doc = sample();
        let p = doc.resolve_paragraph(&with_style("NoSuchStyle"));
        assert_eq!(p.style.as_deref(), Some("Normal"));
    }

    #[test]
    fn run_inherits_from_paragraph_style_and_theme() {
        let doc = sample();
        let normal = doc.resolve_run(&ParagraphProps::default(), &RunProps::default());
        assert_eq!(normal.size, HalfPoints(22));
        assert_eq!(normal.fonts.ascii, "Calibri");
        assert_eq!(normal.fonts.h_ansi, FALLBACK_FONT);
        assert!(!normal.bold);

        let heading = doc.resolve_run(&with_style("Heading1"), &RunProps::default());
        assert_eq!(heading.size, HalfPoints(28));
        assert_eq!(heading.fonts.ascii, "Cambria");
        assert!(heading.bold);
        assert_eq!(heading.color, Color::Auto);
    }

    #[test]
    fn explicit_font_name_overrides_theme_of_lower_layer() {
        let doc = sample();
        let run = RunProps {
            fonts: RunFonts::new("Arial"),
            ..RunProps::default()
        };
        let r = doc.resolve_run(&with_style("Heading1"), &run);
        assert_eq!(r.fonts.ascii, "Arial");
    }

    #[test]
    fn toggles_flip_between_styles_but_direct_wins() {
        let doc = sample();
        let strong = RunProps {
            style: Some("Strong".into()),
            ..RunProps::default()
        };
        // Жирный стиль символов в обычном абзаце — жирный.
        assert!(doc.resolve_run(&ParagraphProps::default(), &strong).bold);
        // Жирный стиль символов внутри жирного заголовка — обычный.
        assert!(!doc.resolve_run(&with_style("Heading1"), &strong).bold);
        // Прямое оформление задаёт значение окончательно.
        let mut direct = strong.clone();
        direct.bold = Some(true);
        assert!(doc.resolve_run(&with_style("Heading1"), &direct).bold);
        let not_bold = bold(false);
        assert!(!doc.resolve_run(&with_style("Heading1"), &not_bold).bold);
    }

    #[test]
    fn based_on_cycle_does_not_hang() {
        let mut doc = Document::new();
        doc.styles.styles = vec![
            style("A", StyleKind::Paragraph, Some("B")),
            style("B", StyleKind::Paragraph, Some("A")),
        ];
        let p = doc.resolve_paragraph(&with_style("A"));
        assert_eq!(p.style.as_deref(), Some("A"));
    }

    #[test]
    fn empty_document_uses_word_defaults() {
        let doc = Document::new();
        let r = doc.resolve_run(&ParagraphProps::default(), &RunProps::default());
        assert_eq!(r.size, HalfPoints(20));
        assert_eq!(r.fonts.ascii, FALLBACK_FONT);
        assert_eq!(r.underline, Underline::None);
        let p = doc.resolve_paragraph(&ParagraphProps::default());
        assert_eq!(p.line_spacing, LineSpacing::SINGLE);
        assert_eq!(p.style, None);
    }

    #[test]
    fn language_from_the_nearest_layer() {
        let lang = |code: &str| vec![crate::RawXml::new(format!(r#"<w:lang w:val="{code}"/>"#))];
        let mut doc = sample();
        let paragraph = ParagraphProps {
            style: Some("Title".into()),
            ..ParagraphProps::default()
        };
        let plain = RunProps::default();
        assert_eq!(doc.run_language(&paragraph, &plain), None);
        doc.styles.defaults.run.unknown = lang("ru-RU");
        assert_eq!(
            doc.run_language(&paragraph, &plain).as_deref(),
            Some("ru-RU")
        );
        doc.styles.styles[0].run.unknown = lang("de-DE");
        doc.styles.styles[1].run.unknown = lang("fr-FR");
        assert_eq!(
            doc.run_language(&paragraph, &plain).as_deref(),
            Some("fr-FR")
        );
        let english = RunProps {
            unknown: lang("en-US"),
            ..RunProps::default()
        };
        assert_eq!(
            doc.run_language(&paragraph, &english).as_deref(),
            Some("en-US")
        );
    }
}
