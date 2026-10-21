//! Где на странице стоит курсор и какая позиция под указателем мыши.
//!
//! Обе задачи решаются через «места курсора» строки: для каждой границы
//! графем в строке — её координата по горизонтали. Внутри лигатуры
//! («ffi» — один глиф) ширина делится поровну между буквами, в кусках
//! справа налево начало буквы — справа.

use letter_layout::{DocumentLayout, LineEnd, LineItem, PageLine, TextItem};
use unicode_segmentation::UnicodeSegmentation;

use crate::position::{Affinity, Position};

/// Место курсора в строке.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaretStop {
    /// Смещение в тексте абзаца, байты.
    pub offset: usize,
    /// Координата, пт от левого края листа.
    pub x: f32,
}

/// Курсор на странице.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Caret {
    /// Номер страницы, с нуля.
    pub page: usize,
    /// Номер строки на странице, с нуля.
    pub line: usize,
    /// Координата, пт от левого края листа.
    pub x: f32,
    /// Верх курсора, пт от верхнего края листа.
    pub top: f32,
    /// Высота курсора (высота букв строки), пт.
    pub height: f32,
}

/// Места курсора в строке в порядке показа слева направо, каждое
/// смещение — один раз (на стыке кусков разного направления — левое).
/// Только места, где курсор может стоять: после разрыва строки или
/// страницы курсор уже на следующей строке.
pub fn line_stops(line: &PageLine) -> Vec<CaretStop> {
    let text = &*line.text;
    let mut stops = Vec::new();
    for item in &line.line.items {
        match item {
            LineItem::Text(t) => run_stops(text, t, line.x, &mut stops),
            LineItem::Tab(tab) => {
                let left = line.x + tab.x;
                stops.push(CaretStop {
                    offset: tab.at,
                    x: left,
                });
                stops.push(CaretStop {
                    offset: tab.at + 1,
                    x: left + tab.width,
                });
            }
        }
    }

    // Места без глифов (знак разрыва, пустая строка) — сразу после
    // предыдущей буквы или в начале строки.
    let range = line.line.range.clone();
    let start_x = line.x + line.line.x;
    let mut known: Vec<CaretStop> = stops.clone();
    known.sort_by_key(|s| s.offset);
    for offset in boundaries(text, range.clone()) {
        if offset == range.end && breaks_after(line) {
            continue;
        }
        if stops.iter().any(|s| s.offset == offset) {
            continue;
        }
        let x = known
            .iter()
            .rev()
            .find(|s| s.offset < offset)
            .map_or(start_x, |s| s.x);
        stops.push(CaretStop { offset, x });
    }
    // Разрыв уже не место курсора, даже если глиф для него есть.
    if breaks_after(line) && range.end > range.start {
        stops.retain(|s| s.offset != range.end);
    }
    stops.sort_by(|a, b| a.x.total_cmp(&b.x));
    // Стык кусков (конец одного — начало другого) — одно место.
    let mut seen = Vec::with_capacity(stops.len());
    stops.retain(|s| {
        let new = !seen.contains(&s.offset);
        seen.push(s.offset);
        new
    });
    stops
}

/// Строка кончается знаком разрыва: позиция после него — на следующей
/// строке.
fn breaks_after(line: &PageLine) -> bool {
    matches!(line.line.end, LineEnd::Break | LineEnd::Page)
}

/// Границы графем в куске `range` текста, с обоими концами.
fn boundaries(text: &str, range: std::ops::Range<usize>) -> Vec<usize> {
    let Some(piece) = text.get(range.clone()) else {
        return vec![range.start];
    };
    let mut out: Vec<usize> = piece
        .grapheme_indices(true)
        .map(|(i, _)| range.start + i)
        .collect();
    out.push(range.end);
    out
}

/// Места курсора в куске текста одного шрифта и направления.
fn run_stops(text: &str, item: &TextItem, margin: f32, out: &mut Vec<CaretStop>) {
    let run = &item.run;
    // Кластеры (буква или лигатура) в порядке показа: начало в тексте,
    // левый край и ширина.
    let mut clusters: Vec<(usize, f32, f32)> = Vec::new();
    let mut pen = margin + item.x;
    for glyph in &run.glyphs {
        match clusters.last_mut() {
            Some(last) if last.0 == glyph.cluster => last.2 += glyph.x_advance,
            _ => clusters.push((glyph.cluster, pen, glyph.x_advance)),
        }
        pen += glyph.x_advance;
    }
    let mut starts: Vec<usize> = clusters.iter().map(|c| c.0).collect();
    starts.sort_unstable();
    starts.dedup();
    let rtl = run.is_rtl();
    for &(start, left, width) in &clusters {
        let end = starts
            .iter()
            .find(|&&s| s > start)
            .copied()
            .unwrap_or(run.range.end);
        let inner = boundaries(text, start..end);
        let count = inner.len().saturating_sub(1).max(1) as f32;
        for (j, &offset) in inner.iter().enumerate() {
            if offset == end && end != run.range.end {
                // Конец кластера — начало следующего, его место даст он.
                continue;
            }
            let share = width * j as f32 / count;
            let x = if rtl {
                left + width - share
            } else {
                left + share
            };
            out.push(CaretStop { offset, x });
        }
    }
}

/// Позиция под точкой (`x`, `y`, пт от левого верхнего угла листа)
/// на странице `page`: ближайшая строка, в ней — ближайшее место курсора.
/// `None`, если на странице нет строк.
pub fn hit_test(layout: &DocumentLayout, page: usize, x: f32, y: f32) -> Option<Position> {
    let lines = &layout.pages.get(page)?.lines;
    let line = lines
        .iter()
        .find(|l| y < l.top + l.height)
        .or_else(|| lines.last())?;
    let stop = line_stops(line)
        .into_iter()
        .min_by(|a, b| (a.x - x).abs().total_cmp(&(b.x - x).abs()))?;
    let range = &line.line.range;
    let affinity = if stop.offset == range.end && line.line.end == LineEnd::Wrap {
        Affinity::Upstream
    } else {
        Affinity::Downstream
    };
    Some(Position {
        paragraph: line.paragraph,
        offset: stop.offset,
        affinity,
    })
}

/// Где нарисовать курсор для позиции `position`. `None`, если такой
/// позиции в документе нет.
pub fn caret(layout: &DocumentLayout, position: Position) -> Option<Caret> {
    // Строки, где позиция может стоять: одна, а на месте переноса — две.
    let mut candidates = layout.pages.iter().enumerate().flat_map(|(p, page)| {
        page.lines.iter().enumerate().filter_map(move |(l, line)| {
            let range = &line.line.range;
            let inside = line.paragraph == position.paragraph
                && range.start <= position.offset
                && position.offset <= range.end
                && !(position.offset == range.end && range.end > range.start && breaks_after(line));
            inside.then_some((p, l, line))
        })
    });
    let found = match position.affinity {
        Affinity::Upstream => candidates.next(),
        Affinity::Downstream => candidates.take(2).last(),
    };
    let (page, line_index, line) = found?;
    let stop = line_stops(line)
        .into_iter()
        .find(|s| s.offset == position.offset)?;
    Some(Caret {
        page,
        line: line_index,
        x: stop.x,
        top: line.baseline - line.line.ascent,
        height: line.line.ascent + line.line.descent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_layout::{FontLibrary, Shaper, layout_document};
    use letter_model::{Block, Document, Paragraph, Run, RunContent};

    fn layout(paragraphs: Vec<Paragraph>) -> DocumentLayout {
        let mut doc = Document::new();
        doc.sections[0].blocks = paragraphs.into_iter().map(Block::Paragraph).collect();
        layout_document(&doc, &mut Shaper::new(), &FontLibrary::embedded_only())
    }

    fn offsets(stops: &[CaretStop]) -> Vec<usize> {
        stops.iter().map(|s| s.offset).collect()
    }

    #[test]
    fn every_grapheme_boundary_is_a_stop() {
        // «й» из двух кодов — одна графема.
        let doc = layout(vec![Paragraph::text("ай\u{306}b")]);
        let line = &doc.pages[0].lines[0];
        let stops = line_stops(line);
        assert_eq!(offsets(&stops), vec![0, 2, 6, 7]);
        // Слева направо и с левого поля.
        assert!(stops.windows(2).all(|w| w[0].x < w[1].x), "{stops:?}");
        assert!((stops[0].x - line.x).abs() < 0.01);
    }

    #[test]
    fn empty_paragraph_has_one_stop() {
        let doc = layout(vec![Paragraph::default()]);
        let line = &doc.pages[0].lines[0];
        let stops = line_stops(line);
        assert_eq!(offsets(&stops), vec![0]);
        let caret = caret(&doc, Position::new(0, 0)).unwrap();
        assert_eq!((caret.page, caret.line), (0, 0));
        assert!(caret.height > 5.0);
    }

    #[test]
    fn tabs_and_breaks_take_one_place() {
        let run = Run {
            props: Default::default(),
            content: vec![
                RunContent::Text("a".into()),
                RunContent::Tab,
                RunContent::Text("b".into()),
                RunContent::Break(letter_model::BreakKind::Line),
                RunContent::Text("c".into()),
            ],
        };
        let doc = layout(vec![Paragraph::from_runs([run])]);
        let lines = &doc.pages[0].lines;
        assert_eq!(lines.len(), 2);
        // a|\t|b|\n — после разрыва курсор уже на второй строке.
        assert_eq!(offsets(&line_stops(&lines[0])), vec![0, 1, 2, 3]);
        assert_eq!(offsets(&line_stops(&lines[1])), vec![4, 5]);
        // Табуляция занимает место до позиции 36 пт.
        let stops = line_stops(&lines[0]);
        assert!((stops[2].x - stops[1].x) > 20.0, "{stops:?}");
        // Перед разрывом — сразу после «b».
        assert_eq!(caret(&doc, Position::new(0, 3)).unwrap().line, 0);
        assert_eq!(caret(&doc, Position::new(0, 4)).unwrap().line, 1);
    }

    #[test]
    fn click_finds_the_nearest_place() {
        let doc = layout(vec![Paragraph::text("Hello"), Paragraph::text("World")]);
        let page = &doc.pages[0];
        let [first, second] = [&page.lines[0], &page.lines[1]];
        let stops = line_stops(first);
        // Чуть правее середины между «H» и «e» — после «H».
        let x = (stops[1].x * 3.0 + stops[2].x) / 4.0;
        let hit = hit_test(&doc, 0, x, first.baseline).unwrap();
        assert_eq!(hit, Position::new(0, 1));
        // Правее конца — конец абзаца; выше первой строки — первая строка.
        assert_eq!(
            hit_test(&doc, 0, 600.0, first.top - 50.0).unwrap().offset,
            5
        );
        // Вторая строка и ниже последней — второй абзац.
        let hit = hit_test(&doc, 0, 0.0, second.baseline).unwrap();
        assert_eq!(hit, Position::new(1, 0));
        assert_eq!(hit_test(&doc, 0, 0.0, 10_000.0).unwrap().paragraph, 1);
        assert_eq!(hit_test(&doc, 5, 0.0, 0.0), None);

        // Курсор там, куда щёлкнули, высотой с буквы.
        let caret = caret(&doc, hit).unwrap();
        assert_eq!((caret.page, caret.line), (0, 1));
        assert!((caret.x - line_stops(second)[0].x).abs() < 0.01);
        assert!(caret.top < second.baseline && caret.top + caret.height > second.baseline);
        assert_eq!(super::caret(&doc, Position::new(7, 0)), None);
        assert_eq!(super::caret(&doc, Position::new(0, 99)), None);
    }

    #[test]
    fn wrap_point_belongs_to_either_line() {
        let words = "word ".repeat(40);
        let doc = layout(vec![Paragraph::text(words.trim_end())]);
        let lines = &doc.pages[0].lines;
        assert!(lines.len() >= 2);
        let wrap = lines[0].line.range.end;
        assert_eq!(lines[1].line.range.start, wrap);

        let down = caret(&doc, Position::new(0, wrap)).unwrap();
        assert_eq!(down.line, 1);
        let up = caret(
            &doc,
            Position::new(0, wrap).with_affinity(Affinity::Upstream),
        )
        .unwrap();
        assert_eq!(up.line, 0);
        assert!(up.x > down.x);

        // Щелчок правее конца первой строки — конец этой строки.
        let hit = hit_test(&doc, 0, 600.0, lines[0].baseline).unwrap();
        assert_eq!(hit.offset, wrap);
        assert_eq!(hit.affinity, Affinity::Upstream);
    }

    #[test]
    fn right_to_left_runs_start_on_the_right() {
        let doc = layout(vec![Paragraph::text("אבג")]);
        let stops = line_stops(&doc.pages[0].lines[0]);
        // Начало слова (смещение 0) — справа.
        assert_eq!(stops.first().map(|s| s.offset), Some(6));
        assert_eq!(stops.last().map(|s| s.offset), Some(0));
    }
}
