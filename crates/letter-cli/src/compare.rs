//! Сверка нашей вёрстки с эталонным PDF (сохранённым из Word): совпадают
//! ли строки, их положение и разрывы страниц.
//!
//! Строки эталона и наши сопоставляются по тексту (пробелы не считаются),
//! затем для каждой пары сравниваются страница и положение. Строка
//! «совпала», если она есть в обоих, стоит на той же странице и сдвинута
//! не больше допуска по вертикали (базовая линия) и по горизонтали (левый
//! край текста).

use std::fmt::{self, Write as _};

use letter_layout::DocumentLayout;

use crate::pdf::PdfPage;

/// Допуск по умолчанию, пт: примерно пиксель при 100 % на обычном экране.
pub const DEFAULT_TOLERANCE: f32 = 1.0;

/// Сколько строк вперёд ищем пару, если тексты разошлись.
const LOOKAHEAD: usize = 40;

/// Сколько расхождений печатать в отчёте, не больше.
const MAX_ISSUES_IN_REPORT: usize = 200;

/// Строка для сверки: страница, положение и текст.
#[derive(Debug, Clone, PartialEq)]
pub struct CompareLine {
    /// Номер страницы, с 1.
    pub page: usize,
    /// Левый край текста, пт от левого края листа.
    pub x: f32,
    /// Базовая линия, пт от верхнего края листа.
    pub baseline: f32,
    /// Текст строки.
    pub text: String,
}

/// Строки эталонного PDF. Строки за верхним и нижним полями наших
/// страниц (колонтитулы, номера страниц) не берутся, пока вёрстка их
/// не умеет.
pub fn reference_lines(pdf: &[PdfPage], layout: &DocumentLayout) -> Vec<CompareLine> {
    let mut out = Vec::new();
    for (i, page) in pdf.iter().enumerate() {
        let body = layout.pages.get(i).or(layout.pages.last());
        for line in &page.lines {
            if let Some(body) = body
                && (line.baseline < body.margins.top
                    || line.baseline > body.height - body.margins.bottom + 1.0)
            {
                continue;
            }
            out.push(CompareLine {
                page: i + 1,
                x: line.x,
                baseline: line.baseline,
                text: line.text.clone(),
            });
        }
    }
    out
}

/// Наши строки с текстом (пустые абзацы в PDF не видны).
pub fn layout_lines(layout: &DocumentLayout) -> Vec<CompareLine> {
    let mut out = Vec::new();
    for (i, page) in layout.pages.iter().enumerate() {
        for line in &page.lines {
            let text = line.text.get(line.line.range.clone()).unwrap_or_default();
            if text.trim().is_empty() {
                continue;
            }
            out.push(CompareLine {
                page: i + 1,
                x: line.x + line.line.x,
                baseline: line.baseline,
                text: text.trim().to_string(),
            });
        }
    }
    out
}

/// Итог сверки одного документа.
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    /// Допуск, пт.
    pub tolerance: f32,
    /// Страниц в эталоне.
    pub reference_pages: usize,
    /// Страниц у нас.
    pub our_pages: usize,
    /// Строк с текстом в эталоне.
    pub reference_lines: usize,
    /// Строк с текстом у нас.
    pub our_lines: usize,
    /// Сколько строк стоит на своём месте по тексту: перенос строк и
    /// страница как в эталоне (положение может отличаться).
    pub paired: usize,
    /// Сколько строк совпало полностью: ещё и положение в пределах
    /// допуска.
    pub matched: usize,
    /// Средний сдвиг по вертикали у строк из [`Self::paired`], пт
    /// (у нас минус эталон).
    pub mean_shift: f32,
    /// Самый большой по модулю сдвиг по вертикали у этих строк, пт.
    pub max_shift: f32,
    /// Расхождения в порядке документа.
    pub issues: Vec<Issue>,
}

/// Одно расхождение.
#[derive(Debug, Clone, PartialEq)]
pub enum Issue {
    /// Строка на той же странице, но сдвинута больше допуска.
    Shifted {
        /// Строка эталона.
        reference: CompareLine,
        /// Наша строка.
        ours: CompareLine,
    },
    /// Строка попала на другую страницу.
    OtherPage {
        /// Строка эталона.
        reference: CompareLine,
        /// Наша строка.
        ours: CompareLine,
    },
    /// Строка есть только в эталоне (другой перенос или то, чего мы
    /// пока не рисуем).
    OnlyReference(CompareLine),
    /// Строка есть только у нас.
    OnlyOurs(CompareLine),
}

impl Comparison {
    /// Доля совпавших строк от строк эталона, в процентах.
    pub fn score(&self) -> f64 {
        let total = self.reference_lines.max(self.our_lines);
        if total == 0 {
            100.0
        } else {
            self.matched as f64 * 100.0 / total as f64
        }
    }

    /// Отчёт для человека.
    pub fn report(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "Страниц: эталон {}, у нас {}{}",
            self.reference_pages,
            self.our_pages,
            if self.reference_pages == self.our_pages {
                ""
            } else {
                " — РАЗНОЕ ЧИСЛО СТРАНИЦ"
            }
        );
        let _ = writeln!(
            out,
            "Строк: эталон {}, у нас {}; перенос и страница как в эталоне у {}, \
             совпало и положение у {} ({:.1} %), допуск {:.2} пт",
            self.reference_lines,
            self.our_lines,
            self.paired,
            self.matched,
            self.score(),
            self.tolerance
        );
        if self.paired > 0 {
            let _ = writeln!(
                out,
                "Сдвиг по вертикали: в среднем {:+.2} пт, самый большой {:+.2} пт",
                self.mean_shift, self.max_shift
            );
        }
        if !self.issues.is_empty() {
            let _ = writeln!(out, "Расхождения (сдвиг = у нас минус эталон):");
        }
        for issue in self.issues.iter().take(MAX_ISSUES_IN_REPORT) {
            let _ = writeln!(out, "  {issue}");
        }
        if self.issues.len() > MAX_ISSUES_IN_REPORT {
            let _ = writeln!(
                out,
                "  … и ещё {}",
                self.issues.len() - MAX_ISSUES_IN_REPORT
            );
        }
        out
    }
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Issue::Shifted { reference, ours } => write!(
                f,
                "стр. {} {}: сдвиг по вертикали {:+.2} пт, по горизонтали {:+.2} пт \
                 (базовая линия: эталон {:.2}, у нас {:.2})",
                reference.page,
                quote(&ours.text),
                ours.baseline - reference.baseline,
                ours.x - reference.x,
                reference.baseline,
                ours.baseline
            ),
            Issue::OtherPage { reference, ours } => write!(
                f,
                "{}: на другой странице — в эталоне на {}-й, у нас на {}-й",
                quote(&ours.text),
                reference.page,
                ours.page
            ),
            Issue::OnlyReference(line) => write!(
                f,
                "стр. {} {}: только в эталоне ({:.2} пт)",
                line.page,
                quote(&line.text),
                line.baseline
            ),
            Issue::OnlyOurs(line) => write!(
                f,
                "стр. {} {}: только у нас ({:.2} пт)",
                line.page,
                quote(&line.text),
                line.baseline
            ),
        }
    }
}

/// Текст строки в кавычках, длинный — с многоточием.
fn quote(text: &str) -> String {
    const MAX: usize = 50;
    if text.chars().count() > MAX {
        let short: String = text.chars().take(MAX).collect();
        format!("«{short}…»")
    } else {
        format!("«{text}»")
    }
}

/// Текст для сопоставления: без пробелов и мягких переносов.
fn key(text: &str) -> Vec<char> {
    text.chars()
        .filter(|c| !c.is_whitespace() && *c != '\u{AD}')
        .collect()
}

/// Одна и та же ли строка. Наша может быть без начала эталонной: маркеры
/// и номера списков мы пока не рисуем.
fn same_text(reference: &[char], ours: &[char]) -> bool {
    reference == ours
        || (!ours.is_empty() && reference.ends_with(ours) && ours.len() * 2 >= reference.len())
}

/// Сопоставляет строки эталона с нашими и считает расхождения.
pub fn compare(
    reference: &[CompareLine],
    reference_pages: usize,
    ours: &[CompareLine],
    our_pages: usize,
    tolerance: f32,
) -> Comparison {
    let reference_keys: Vec<Vec<char>> = reference.iter().map(|l| key(&l.text)).collect();
    let our_keys: Vec<Vec<char>> = ours.iter().map(|l| key(&l.text)).collect();
    let matches = |i: usize, j: usize| same_text(&reference_keys[i], &our_keys[j]);
    let mut result = Comparison {
        tolerance,
        reference_pages,
        our_pages,
        reference_lines: reference.len(),
        our_lines: ours.len(),
        paired: 0,
        matched: 0,
        mean_shift: 0.0,
        max_shift: 0.0,
        issues: Vec::new(),
    };
    let mut shift_sum = 0.0;
    let (mut i, mut j) = (0, 0);
    while i < reference.len() && j < ours.len() {
        if matches(i, j) {
            let (r, o) = (&reference[i], &ours[j]);
            if r.page != o.page {
                result.issues.push(Issue::OtherPage {
                    reference: r.clone(),
                    ours: o.clone(),
                });
            } else {
                let shift = o.baseline - r.baseline;
                result.paired += 1;
                shift_sum += shift;
                if shift.abs() > result.max_shift.abs() {
                    result.max_shift = shift;
                }
                if shift.abs() > tolerance || (r.x - o.x).abs() > tolerance {
                    result.issues.push(Issue::Shifted {
                        reference: r.clone(),
                        ours: o.clone(),
                    });
                } else {
                    result.matched += 1;
                }
            }
            i += 1;
            j += 1;
            continue;
        }
        // Ближайшая следующая пара: пропускаем меньше всего строк.
        let next = (1..2 * LOOKAHEAD).find_map(|skip| {
            (0..=skip.min(LOOKAHEAD)).find_map(|a| {
                let b = skip - a;
                (b <= LOOKAHEAD
                    && i + a < reference.len()
                    && j + b < ours.len()
                    && matches(i + a, j + b))
                .then_some((a, b))
            })
        });
        let (a, b) = next.unwrap_or((1, 1));
        for line in &reference[i..(i + a).min(reference.len())] {
            result.issues.push(Issue::OnlyReference(line.clone()));
        }
        for line in &ours[j..(j + b).min(ours.len())] {
            result.issues.push(Issue::OnlyOurs(line.clone()));
        }
        i += a;
        j += b;
    }
    for line in reference.iter().skip(i) {
        result.issues.push(Issue::OnlyReference(line.clone()));
    }
    for line in ours.iter().skip(j) {
        result.issues.push(Issue::OnlyOurs(line.clone()));
    }
    if result.paired > 0 {
        result.mean_shift = shift_sum / result.paired as f32;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(page: usize, baseline: f32, text: &str) -> CompareLine {
        CompareLine {
            page,
            x: 72.0,
            baseline,
            text: text.into(),
        }
    }

    #[test]
    fn identical_lines_match() {
        let lines = [line(1, 80.0, "Hello, world!"), line(1, 95.0, "Second")];
        let result = compare(&lines, 1, &lines, 1, 0.5);
        assert_eq!(result.matched, 2);
        assert!(result.issues.is_empty());
        assert_eq!(result.score(), 100.0);
    }

    #[test]
    fn shift_page_and_line_break_are_reported() {
        let reference = [
            line(1, 80.0, "One two three"),
            line(1, 95.0, "four five"),
            line(1, 110.0, "Next"),
            line(2, 80.0, "Last"),
        ];
        let ours = [
            line(1, 80.3, "One two"),
            line(1, 95.0, "three four five"),
            line(1, 112.0, "Next"),
            line(1, 700.0, "Last"),
        ];
        let result = compare(&reference, 2, &ours, 1, 0.5);
        assert_eq!(result.matched, 0);
        assert_eq!(result.paired, 1);
        assert_eq!(result.max_shift, 2.0);
        assert!(matches!(result.issues[0], Issue::OnlyReference(_)));
        assert!(matches!(result.issues[3], Issue::OnlyOurs(_)));
        assert!(matches!(result.issues[4], Issue::Shifted { .. }));
        assert!(matches!(result.issues[5], Issue::OtherPage { .. }));
        let report = result.report();
        assert!(report.contains("РАЗНОЕ ЧИСЛО СТРАНИЦ"), "{report}");
        assert!(report.contains("сдвиг по вертикали +2.00 пт"), "{report}");
    }

    #[test]
    fn missing_line_does_not_break_the_rest() {
        let reference = [
            line(1, 80.0, "Header text"),
            line(1, 95.0, "A"),
            line(1, 110.0, "B"),
        ];
        let ours = [line(1, 95.0, "A"), line(1, 110.0, "B")];
        let result = compare(&reference, 1, &ours, 1, 0.5);
        assert_eq!(result.matched, 2);
        assert_eq!(result.issues.len(), 1);
    }

    #[test]
    fn list_markers_and_spaces_are_ignored() {
        let reference = [line(1, 80.0, "• First item"), line(1, 95.0, "1. Second")];
        let ours = [line(1, 80.0, "First  item"), line(1, 95.0, "Second")];
        assert_eq!(compare(&reference, 1, &ours, 1, 0.5).matched, 2);
    }
}
