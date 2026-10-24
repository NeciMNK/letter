//! Диаграммы: рисуются по данным из кеша документа ([`Chart`]) теми же
//! прямоугольниками, многоугольниками и глифами, что и остальная
//! страница, поэтому одинаково выглядят на экране, в картинке и в PDF.
//!
//! Устройство как в Word по умолчанию: белая область с тонкой серой
//! рамкой, заголовок сверху, легенда, область построения с подписями
//! осей и линиями сетки. Подписи данных, оси с настройками, объёмные
//! диаграммы и стили диаграмм пока не поддерживаются.

use std::f32::consts::{FRAC_PI_2, TAU};

use letter_layout::{FontLibrary, ShapedRun, TextStyle};
use letter_model::{Chart, ChartGrouping, ChartKind, LegendPosition};

use crate::glyphs::{GlyphCache, GlyphKey, SUBPIXEL_STEPS};
use crate::scene::{DrawItem, Rgba};

/// Шрифт подписей (как у диаграмм Word до темы 2023 года; если его
/// нет, подбирается похожий).
const FONT: &str = "Calibri";
/// Размер подписей осей и легенды, пт.
const LABEL_SIZE: f32 = 9.0;
/// Размер заголовка, пт.
const TITLE_SIZE: f32 = 14.0;
/// Отступ от края области диаграммы, пт.
const PADDING: f32 = 7.0;
/// Цвет подписей.
const TEXT: Rgba = [0x59, 0x59, 0x59, 255];
/// Цвет линий сетки и рамки.
const GRID: Rgba = [0xD9, 0xD9, 0xD9, 255];
/// Цвет оси категорий.
const AXIS: Rgba = [0xBF, 0xBF, 0xBF, 255];
const WHITE: Rgba = [255, 255, 255, 255];
/// Толщина линий графика, пт.
const LINE_WIDTH: f32 = 2.25;
/// Толщина линий сетки, пт.
const GRID_WIDTH: f32 = 0.75;

/// Что нарисовать для диаграммы `chart` в прямоугольнике `x`, `y`,
/// `width`, `height` (пт от угла листа) при масштабе `scale` пикселей
/// на пункт.
#[allow(clippy::too_many_arguments)]
pub fn chart_items(
    chart: &Chart,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    scale: f32,
    library: &FontLibrary,
    cache: &mut GlyphCache,
) -> Vec<DrawItem> {
    let mut canvas = Canvas {
        out: Vec::new(),
        scale,
        x,
        y,
        library,
        cache,
    };
    if width > 0.0 && height > 0.0 {
        draw(&mut canvas, chart, width, height);
    }
    canvas.out
}

/// Область на диаграмме, пт от её левого верхнего угла.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Area {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Area {
    fn width(&self) -> f32 {
        self.right - self.left
    }

    fn height(&self) -> f32 {
        self.bottom - self.top
    }
}

fn draw(canvas: &mut Canvas<'_>, chart: &Chart, width: f32, height: f32) {
    canvas.rect(0.0, 0.0, width, height, WHITE);
    for (x, y, w, h) in [
        (0.0, 0.0, width, GRID_WIDTH),
        (0.0, height - GRID_WIDTH, width, GRID_WIDTH),
        (0.0, 0.0, GRID_WIDTH, height),
        (width - GRID_WIDTH, 0.0, GRID_WIDTH, height),
    ] {
        canvas.rect(x, y, w, h, GRID);
    }
    let mut area = Area {
        left: PADDING,
        top: PADDING,
        right: width - PADDING,
        bottom: height - PADDING,
    };
    if let Some(title) = &chart.title {
        let runs = canvas.shape(title, TITLE_SIZE);
        let (ascent, descent) = canvas.extent(&runs, TITLE_SIZE);
        let w = text_width(&runs);
        canvas.text(&runs, (width - w) / 2.0, area.top + ascent, TEXT);
        area.top += ascent + descent + 4.0;
    }
    if let Some(position) = chart.legend {
        legend(canvas, chart, position, &mut area);
    }
    if area.width() <= 4.0 || area.height() <= 4.0 {
        return;
    }
    match chart.kind {
        ChartKind::Pie | ChartKind::Doughnut => round(canvas, chart, area),
        ChartKind::Scatter => scatter(canvas, chart, area),
        ChartKind::Column | ChartKind::Bar | ChartKind::Line | ChartKind::Area => {
            categorized(canvas, chart, area)
        }
    }
}

/// Цвет ряда номер `index`.
fn series_color(chart: &Chart, index: usize) -> Rgba {
    let (r, g, b) = chart
        .series
        .get(index)
        .and_then(|s| s.color)
        .unwrap_or_else(|| chart.series_color(index));
    [r, g, b, 255]
}

/// Каждая точка своим цветом (круговая или `c:varyColors` у одного ряда).
fn colors_by_point(chart: &Chart) -> bool {
    chart.is_round() || (chart.vary_colors && chart.series.len() == 1)
}

/// Цвет точки `point` ряда `series`.
fn point_color(chart: &Chart, series: usize, point: usize) -> Rgba {
    let own = chart
        .series
        .get(series)
        .and_then(|s| s.point_colors.iter().find(|(i, _)| *i == point))
        .map(|(_, c)| *c);
    match own {
        Some((r, g, b)) => [r, g, b, 255],
        None if colors_by_point(chart) => {
            let (r, g, b) = chart.series_color(point);
            [r, g, b, 255]
        }
        None => series_color(chart, series),
    }
}

/// Легенда: подписи рядов (или точек круговой) с цветными значками.
/// Уменьшает `area` на занятое место.
fn legend(canvas: &mut Canvas<'_>, chart: &Chart, position: LegendPosition, area: &mut Area) {
    let entries: Vec<(String, Rgba)> = if colors_by_point(chart) {
        let count = chart
            .series
            .first()
            .map_or(0, |s| s.values.len())
            .max(chart.categories.len());
        (0..count)
            .map(|i| {
                let name = chart.categories.get(i).cloned().unwrap_or_default();
                (name, point_color(chart, 0, i))
            })
            .collect()
    } else {
        (0..chart.series.len())
            .map(|i| (chart.series[i].name.clone(), series_color(chart, i)))
            .collect()
    };
    if entries.is_empty() {
        return;
    }
    let line = matches!(chart.kind, ChartKind::Line | ChartKind::Scatter);
    let mark = if line { 14.0 } else { 6.0 };
    let shaped: Vec<(Vec<ShapedRun>, Rgba)> = entries
        .into_iter()
        .map(|(name, color)| (canvas.shape(&name, LABEL_SIZE), color))
        .collect();
    let (ascent, descent) = shaped
        .first()
        .map_or((LABEL_SIZE * 0.75, LABEL_SIZE * 0.25), |(runs, _)| {
            canvas.extent(runs, LABEL_SIZE)
        });
    let row = ascent + descent + 2.0;
    let widths: Vec<f32> = shaped
        .iter()
        .map(|(runs, _)| mark + 3.0 + text_width(runs))
        .collect();
    let draw_entry = |canvas: &mut Canvas<'_>, i: usize, x: f32, top: f32| {
        let (runs, color) = &shaped[i];
        let middle = top + row / 2.0;
        if line {
            canvas.rect(x, middle - LINE_WIDTH / 2.0, mark, LINE_WIDTH, *color);
        } else {
            canvas.rect(x, middle - mark / 2.0, mark, mark, *color);
        }
        let baseline = middle + (ascent - descent) / 2.0;
        canvas.text(runs, x + mark + 3.0, baseline, TEXT);
    };
    match position {
        LegendPosition::Bottom | LegendPosition::Top => {
            // Строки легенды: сколько поместится в ширину.
            let gap = 10.0;
            let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
            let mut used = 0.0;
            for (i, w) in widths.iter().enumerate() {
                let need = if used > 0.0 { used + gap + w } else { *w };
                if need > area.width() && used > 0.0 {
                    rows.push(vec![i]);
                    used = *w;
                } else {
                    if let Some(last) = rows.last_mut() {
                        last.push(i);
                    }
                    used = need;
                }
            }
            let total = rows.len() as f32 * row;
            let mut top = if position == LegendPosition::Bottom {
                area.bottom - total
            } else {
                area.top
            };
            for entries in &rows {
                let line_width: f32 = entries.iter().map(|&i| widths[i]).sum::<f32>()
                    + gap * entries.len().saturating_sub(1) as f32;
                let mut x = area.left + (area.width() - line_width) / 2.0;
                for &i in entries {
                    draw_entry(canvas, i, x, top);
                    x += widths[i] + gap;
                }
                top += row;
            }
            if position == LegendPosition::Bottom {
                area.bottom -= total + 4.0;
            } else {
                area.top += total + 4.0;
            }
        }
        LegendPosition::Left | LegendPosition::Right => {
            let column = widths
                .iter()
                .copied()
                .fold(0.0, f32::max)
                .min(area.width() / 2.0);
            let total = widths.len() as f32 * row;
            let mut top = area.top + ((area.height() - total) / 2.0).max(0.0);
            let x = if position == LegendPosition::Right {
                area.right - column
            } else {
                area.left
            };
            for i in 0..widths.len() {
                draw_entry(canvas, i, x, top);
                top += row;
            }
            if position == LegendPosition::Right {
                area.right -= column + 8.0;
            } else {
                area.left += column + 8.0;
            }
        }
    }
}

/// Круговая или кольцевая: секторы первого ряда по часовой стрелке
/// от «двенадцати часов».
fn round(canvas: &mut Canvas<'_>, chart: &Chart, area: Area) {
    let Some(series) = chart.series.first() else {
        return;
    };
    let values: Vec<f64> = series
        .values
        .iter()
        .map(|v| v.unwrap_or(0.0).abs())
        .collect();
    let total: f64 = values.iter().sum();
    if total <= 0.0 {
        return;
    }
    let cx = (area.left + area.right) / 2.0;
    let cy = (area.top + area.bottom) / 2.0;
    let radius = (area.width().min(area.height()) / 2.0 - 2.0).max(1.0);
    let hole = if chart.kind == ChartKind::Doughnut {
        radius * 0.5
    } else {
        0.0
    };
    let at = |angle: f32, r: f32| (cx + r * angle.cos(), cy + r * angle.sin());
    let mut start = -FRAC_PI_2;
    let mut edges = Vec::new();
    for (i, value) in values.iter().enumerate() {
        let sweep = (value / total) as f32 * TAU;
        if sweep <= 0.0 {
            continue;
        }
        let color = point_color(chart, 0, i);
        // Дуга по кусочкам не больше 3°: каждый кусок — выпуклый.
        let steps = (sweep / 3f32.to_radians()).ceil().max(1.0) as usize;
        let step = sweep / steps as f32;
        // Соседние куски одного сектора чуть заходят друг на друга,
        // чтобы между ними не просвечивал шов.
        let overlap = |k: usize| if k < steps { step * 0.5 } else { 0.0 };
        if hole > 0.0 {
            for s in 0..steps {
                let a0 = start + step * s as f32;
                let a1 = start + step * (s + 1) as f32 + overlap(s + 1);
                canvas.polygon(
                    &[at(a0, hole), at(a0, radius), at(a1, radius), at(a1, hole)],
                    color,
                );
            }
        } else {
            // Веер из кусков по 90°, у каждого вершина в центре.
            let per_piece = 30;
            let mut s = 0;
            while s < steps {
                let end = (s + per_piece).min(steps);
                let mut points = vec![(cx, cy)];
                for k in s..=end {
                    points.push(at(start + step * k as f32, radius));
                }
                if end < steps {
                    points.push(at(start + step * end as f32 + overlap(end), radius));
                }
                canvas.polygon(&points, color);
                s = end;
            }
        }
        edges.push(start);
        start += sweep;
    }
    // Белые линии между секторами.
    if edges.len() > 1 {
        for angle in edges {
            canvas.line(at(angle, hole), at(angle, radius), 1.0, WHITE);
        }
    }
}

/// Шкала значений: начало, конец, шаг делений.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Scale {
    min: f64,
    max: f64,
    step: f64,
}

impl Scale {
    /// Красивая шкала (шаг 1, 2 или 5, умноженные на степень десяти),
    /// в которую входят `min`, `max` и ноль; делений примерно `ticks`.
    fn nice(min: f64, max: f64, ticks: usize) -> Scale {
        let (mut min, mut max) = (min.min(0.0), max.max(0.0));
        if !(min.is_finite() && max.is_finite()) {
            (min, max) = (0.0, 1.0);
        }
        if max - min <= f64::EPSILON * max.abs().max(1.0) {
            max = min + 1.0;
        }
        // Как в Word: над самым большим значением (и под самым малым)
        // остаётся запас в двадцатую часть размаха.
        let headroom = (max - min) / 20.0;
        if max > 0.0 {
            max += headroom;
        }
        if min < 0.0 {
            min -= headroom;
        }
        let raw = (max - min) / ticks.max(1) as f64;
        let magnitude = 10f64.powf(raw.log10().floor());
        let fraction = raw / magnitude;
        let nice = if fraction < 1.5 {
            1.0
        } else if fraction < 3.0 {
            2.0
        } else if fraction < 7.0 {
            5.0
        } else {
            10.0
        };
        let step = nice * magnitude;
        Scale {
            min: (min / step + 1e-9).floor() * step,
            max: (max / step - 1e-9).ceil() * step,
            step,
        }
    }

    /// Деления от начала до конца.
    fn ticks(&self) -> Vec<f64> {
        let count = ((self.max - self.min) / self.step).round() as usize;
        (0..=count.min(100))
            .map(|i| self.min + self.step * i as f64)
            .collect()
    }

    /// Доля пути от начала до конца шкалы.
    fn fraction(&self, value: f64) -> f32 {
        ((value - self.min) / (self.max - self.min)) as f32
    }
}

/// Число для подписи оси: столько знаков после запятой, сколько нужно
/// для шага шкалы; дробная часть — через запятую.
fn format_number(value: f64, step: f64, percent: bool) -> String {
    let decimals = if step >= 1.0 {
        0
    } else {
        (-step.log10().floor()).clamp(0.0, 6.0) as usize
    };
    let value = if value.abs() < step * 1e-3 {
        0.0
    } else {
        value
    };
    let text = format!("{value:.decimals$}").replace('.', ",");
    if percent { format!("{text}%") } else { text }
}

/// Сколько категорий у диаграммы.
fn category_count(chart: &Chart) -> usize {
    chart
        .series
        .iter()
        .map(|s| s.values.len())
        .max()
        .unwrap_or(0)
        .max(chart.categories.len())
}

/// Значения, как их рисовать: для «друг на друге» — нижний и верхний
/// край каждой точки, иначе — от нуля до значения. `[ряд][категория]`.
fn stacked_values(chart: &Chart, count: usize) -> Vec<Vec<Option<(f64, f64)>>> {
    let percent = chart.grouping == ChartGrouping::PercentStacked;
    let stacked = chart.grouping != ChartGrouping::Standard;
    let totals: Vec<f64> = (0..count)
        .map(|c| {
            chart
                .series
                .iter()
                .filter_map(|s| s.values.get(c).copied().flatten())
                .map(f64::abs)
                .sum()
        })
        .collect();
    let mut positive = vec![0.0; count];
    let mut negative = vec![0.0; count];
    chart
        .series
        .iter()
        .map(|s| {
            (0..count)
                .map(|c| {
                    let mut v = s.values.get(c).copied().flatten()?;
                    if percent {
                        v = if totals[c] > 0.0 {
                            v / totals[c] * 100.0
                        } else {
                            0.0
                        };
                    }
                    if !stacked {
                        return Some((0.0, v));
                    }
                    let base = if v >= 0.0 {
                        &mut positive[c]
                    } else {
                        &mut negative[c]
                    };
                    let from = *base;
                    *base += v;
                    Some((from, *base))
                })
                .collect()
        })
        .collect()
}

/// Подписи шкалы значений и их ширина, пт.
fn value_labels(
    canvas: &mut Canvas<'_>,
    scale: &Scale,
    percent: bool,
) -> (Vec<(f64, Vec<ShapedRun>)>, f32) {
    let labels: Vec<(f64, Vec<ShapedRun>)> = scale
        .ticks()
        .into_iter()
        .map(|t| {
            let text = format_number(t, scale.step, percent);
            (t, canvas.shape(&text, LABEL_SIZE))
        })
        .collect();
    let widest = labels
        .iter()
        .map(|(_, runs)| text_width(runs))
        .fold(0.0, f32::max);
    (labels, widest)
}

/// Диаграммы с категориями: гистограмма, линейчатая, график, с областями.
fn categorized(canvas: &mut Canvas<'_>, chart: &Chart, area: Area) {
    let count = category_count(chart);
    if count == 0 {
        return;
    }
    let horizontal = chart.kind == ChartKind::Bar;
    let percent = chart.grouping == ChartGrouping::PercentStacked;
    let values = stacked_values(chart, count);
    let (mut lo, mut hi) = (0.0f64, 0.0f64);
    for (from, to) in values.iter().flatten().flatten() {
        lo = lo.min(from.min(*to));
        hi = hi.max(from.max(*to));
    }
    let (ascent, descent) = {
        let probe = canvas.shape("0", LABEL_SIZE);
        canvas.extent(&probe, LABEL_SIZE)
    };
    let line = ascent + descent;
    let along = if horizontal {
        area.width()
    } else {
        area.height()
    };
    let ticks = ((along / (line * 2.5)) as usize).clamp(2, 10);
    let mut scale = Scale::nice(lo, hi, ticks);
    if percent {
        scale = Scale::nice(lo.max(-100.0), hi.min(100.0), ticks);
    }
    let (labels, widest) = value_labels(canvas, &scale, percent);
    let names: Vec<Vec<ShapedRun>> = (0..count)
        .map(|i| {
            let name = chart.categories.get(i).cloned().unwrap_or_default();
            canvas.shape(&name, LABEL_SIZE)
        })
        .collect();
    let widest_name = names.iter().map(|r| text_width(r)).fold(0.0, f32::max);
    // Область построения без подписей осей.
    let plot = if horizontal {
        Area {
            left: area.left + widest_name.min(area.width() / 3.0) + 6.0,
            top: area.top + 2.0,
            right: area.right - widest / 2.0,
            bottom: area.bottom - line - 4.0,
        }
    } else {
        Area {
            left: area.left + widest + 6.0,
            top: area.top + line / 2.0,
            right: area.right - 2.0,
            bottom: area.bottom - line - 4.0,
        }
    };
    if plot.width() <= 2.0 || plot.height() <= 2.0 {
        return;
    }
    // Положение значения: по вертикали сверху вниз или по горизонтали.
    let value_at = |v: f64| {
        let f = scale.fraction(v);
        if horizontal {
            plot.left + f * plot.width()
        } else {
            plot.bottom - f * plot.height()
        }
    };
    // Линии сетки и подписи шкалы.
    for (tick, runs) in &labels {
        let p = value_at(*tick);
        let w = text_width(runs);
        if horizontal {
            canvas.rect(
                p - GRID_WIDTH / 2.0,
                plot.top,
                GRID_WIDTH,
                plot.height(),
                GRID,
            );
            canvas.text(runs, p - w / 2.0, plot.bottom + 3.0 + ascent, TEXT);
        } else {
            canvas.rect(
                plot.left,
                p - GRID_WIDTH / 2.0,
                plot.width(),
                GRID_WIDTH,
                GRID,
            );
            let baseline = p + (ascent - descent) / 2.0;
            canvas.text(runs, plot.left - 4.0 - w, baseline, TEXT);
        }
    }
    // Категории: у областей точки на краях, у остальных — в середине
    // своей полосы.
    let edges = chart.kind == ChartKind::Area && count > 1;
    let slot = if horizontal {
        plot.height()
    } else {
        plot.width()
    } / count as f32;
    let category_at = |i: f32| {
        if horizontal {
            // Первая категория внизу, как в Word.
            plot.bottom - i * slot
        } else {
            plot.left + i * slot
        }
    };
    let point_at = |c: usize| {
        if edges {
            let step = if horizontal {
                plot.height()
            } else {
                plot.width()
            } / (count - 1) as f32;
            if horizontal {
                plot.bottom - c as f32 * step
            } else {
                plot.left + c as f32 * step
            }
        } else {
            category_at(c as f32 + 0.5)
        }
    };
    for (i, runs) in names.iter().enumerate() {
        let w = text_width(runs);
        let p = point_at(i);
        if horizontal {
            let baseline = p + (ascent - descent) / 2.0;
            canvas.text(runs, plot.left - 4.0 - w, baseline, TEXT);
        } else {
            canvas.text(runs, p - w / 2.0, plot.bottom + 3.0 + ascent, TEXT);
        }
    }
    let zero = value_at(0f64.clamp(scale.min, scale.max));
    match chart.kind {
        ChartKind::Column | ChartKind::Bar => {
            let stacked = chart.grouping != ChartGrouping::Standard;
            let k = if stacked {
                1
            } else {
                chart.series.len().max(1)
            } as f32;
            let overlap = if stacked {
                1.0
            } else {
                f32::from(chart.overlap) / 100.0
            };
            let gap = f32::from(chart.gap_width) / 100.0;
            let bar = slot / (k - (k - 1.0) * overlap + gap);
            for (s, series) in values.iter().enumerate() {
                let index = if stacked { 0.0 } else { s as f32 };
                let offset = gap * bar / 2.0 + index * bar * (1.0 - overlap);
                for (c, value) in series.iter().enumerate() {
                    let Some((from, to)) = value else {
                        continue;
                    };
                    let (a, b) = (value_at(*from), value_at(*to));
                    let color = point_color(chart, s, c);
                    if horizontal {
                        let top = category_at(c as f32 + 1.0) + offset;
                        canvas.rect(a.min(b), top, (b - a).abs(), bar, color);
                    } else {
                        let left = category_at(c as f32) + offset;
                        canvas.rect(left, a.min(b), bar, (b - a).abs(), color);
                    }
                }
            }
        }
        ChartKind::Area => {
            for (s, series) in values.iter().enumerate() {
                let color = series_color(chart, s);
                for c in 0..count.saturating_sub(1) {
                    let (Some((f0, t0)), Some((f1, t1))) = (series[c], series[c + 1]) else {
                        continue;
                    };
                    let (x0, x1) = (point_at(c), point_at(c + 1));
                    let quad = [
                        (x0, value_at(f0)),
                        (x0, value_at(t0)),
                        (x1, value_at(t1)),
                        (x1, value_at(f1)),
                    ];
                    canvas.polygon(&swap(&quad, horizontal), color);
                }
            }
        }
        ChartKind::Line => {
            for (s, series) in values.iter().enumerate() {
                let color = series_color(chart, s);
                let points: Vec<Option<(f32, f32)>> = series
                    .iter()
                    .enumerate()
                    .map(|(c, v)| v.map(|(_, to)| (point_at(c), value_at(to))))
                    .collect();
                polyline(canvas, &points, horizontal, color);
            }
        }
        ChartKind::Pie | ChartKind::Doughnut | ChartKind::Scatter => {}
    }
    // Ось категорий — по нулю.
    if horizontal {
        canvas.rect(
            zero - GRID_WIDTH / 2.0,
            plot.top,
            GRID_WIDTH,
            plot.height(),
            AXIS,
        );
    } else {
        canvas.rect(
            plot.left,
            zero - GRID_WIDTH / 2.0,
            plot.width(),
            GRID_WIDTH,
            AXIS,
        );
    }
}

/// Точки (положение вдоль категорий, положение значения) в координаты
/// диаграммы: у линейчатой категории идут по вертикали.
fn swap(points: &[(f32, f32)], horizontal: bool) -> Vec<(f32, f32)> {
    points
        .iter()
        .map(|&(a, v)| if horizontal { (v, a) } else { (a, v) })
        .collect()
}

/// Ломаная толщиной [`LINE_WIDTH`]; пустые точки разрывают её.
fn polyline(canvas: &mut Canvas<'_>, points: &[Option<(f32, f32)>], horizontal: bool, color: Rgba) {
    let place = |p: (f32, f32)| if horizontal { (p.1, p.0) } else { p };
    for pair in points.windows(2) {
        if let [Some(a), Some(b)] = pair {
            canvas.line(place(*a), place(*b), LINE_WIDTH, color);
        }
    }
    // Скругления на изломах и одиночные точки.
    for p in points.iter().flatten() {
        canvas.dot(place(*p), LINE_WIDTH / 2.0, color);
    }
}

/// Точечная: значения X по горизонтальной шкале, Y — по вертикальной,
/// точки — ромбиками.
fn scatter(canvas: &mut Canvas<'_>, chart: &Chart, area: Area) {
    let point = |s: usize, i: usize| -> Option<(f64, f64)> {
        let series = &chart.series[s];
        let y = series.values.get(i).copied().flatten()?;
        let x = if series.x_values.iter().any(Option::is_some) {
            series.x_values.get(i).copied().flatten()?
        } else {
            (i + 1) as f64
        };
        Some((x, y))
    };
    let all: Vec<(f64, f64)> = (0..chart.series.len())
        .flat_map(|s| (0..chart.series[s].values.len()).filter_map(move |i| point(s, i)))
        .collect();
    if all.is_empty() {
        return;
    }
    let fold = |f: fn(f64, f64) -> f64, start: f64, pick: fn(&(f64, f64)) -> f64| {
        all.iter().map(pick).fold(start, f)
    };
    let x_scale = Scale::nice(
        fold(f64::min, f64::INFINITY, |p| p.0),
        fold(f64::max, f64::NEG_INFINITY, |p| p.0),
        6,
    );
    let (ascent, descent) = {
        let probe = canvas.shape("0", LABEL_SIZE);
        canvas.extent(&probe, LABEL_SIZE)
    };
    let line = ascent + descent;
    let ticks = ((area.height() / (line * 2.5)) as usize).clamp(2, 10);
    let y_scale = Scale::nice(
        fold(f64::min, f64::INFINITY, |p| p.1),
        fold(f64::max, f64::NEG_INFINITY, |p| p.1),
        ticks,
    );
    let (y_labels, widest) = value_labels(canvas, &y_scale, false);
    let (x_labels, x_widest) = value_labels(canvas, &x_scale, false);
    let plot = Area {
        left: area.left + widest + 6.0,
        top: area.top + line / 2.0,
        right: area.right - x_widest / 2.0,
        bottom: area.bottom - line - 4.0,
    };
    if plot.width() <= 2.0 || plot.height() <= 2.0 {
        return;
    }
    let to_x = |x: f64| plot.left + x_scale.fraction(x) * plot.width();
    let to_y = |y: f64| plot.bottom - y_scale.fraction(y) * plot.height();
    for (tick, runs) in &y_labels {
        let p = to_y(*tick);
        canvas.rect(
            plot.left,
            p - GRID_WIDTH / 2.0,
            plot.width(),
            GRID_WIDTH,
            GRID,
        );
        let baseline = p + (ascent - descent) / 2.0;
        let w = text_width(runs);
        canvas.text(runs, plot.left - 4.0 - w, baseline, TEXT);
    }
    for (tick, runs) in &x_labels {
        let p = to_x(*tick);
        canvas.rect(
            p - GRID_WIDTH / 2.0,
            plot.top,
            GRID_WIDTH,
            plot.height(),
            GRID,
        );
        let w = text_width(runs);
        canvas.text(runs, p - w / 2.0, plot.bottom + 3.0 + ascent, TEXT);
    }
    let zero = to_y(0f64.clamp(y_scale.min, y_scale.max));
    canvas.rect(
        plot.left,
        zero - GRID_WIDTH / 2.0,
        plot.width(),
        GRID_WIDTH,
        AXIS,
    );
    for s in 0..chart.series.len() {
        for i in 0..chart.series[s].values.len() {
            let Some((x, y)) = point(s, i) else {
                continue;
            };
            let (px, py) = (to_x(x), to_y(y));
            let r = 3.5;
            let color = point_color(chart, s, i);
            canvas.polygon(
                &[(px, py - r), (px + r, py), (px, py + r), (px - r, py)],
                color,
            );
        }
    }
}

/// Ширина набранного текста, пт.
fn text_width(runs: &[ShapedRun]) -> f32 {
    runs.iter().map(ShapedRun::width).sum()
}

/// Куда складываются элементы рисунка: пересчитывает пункты диаграммы
/// в пиксели листа.
struct Canvas<'a> {
    out: Vec<DrawItem>,
    scale: f32,
    /// Левый верхний угол диаграммы на листе, пт.
    x: f32,
    y: f32,
    library: &'a FontLibrary,
    cache: &'a mut GlyphCache,
}

impl Canvas<'_> {
    fn point(&self, (x, y): (f32, f32)) -> (f32, f32) {
        ((self.x + x) * self.scale, (self.y + y) * self.scale)
    }

    fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: Rgba) {
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let (px, py) = self.point((x, y));
        self.out.push(DrawItem::Rect {
            x: px,
            y: py,
            width: width * self.scale,
            height: height * self.scale,
            color,
        });
    }

    /// Выпуклый многоугольник.
    fn polygon(&mut self, points: &[(f32, f32)], color: Rgba) {
        if points.len() < 3 {
            return;
        }
        let points = points.iter().map(|&p| self.point(p)).collect();
        self.out.push(DrawItem::Polygon { points, color });
    }

    /// Отрезок толщиной `width`.
    fn line(&mut self, a: (f32, f32), b: (f32, f32), width: f32, color: Rgba) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let length = (dx * dx + dy * dy).sqrt();
        if length <= f32::EPSILON {
            return;
        }
        let (nx, ny) = (-dy / length * width / 2.0, dx / length * width / 2.0);
        self.polygon(
            &[
                (a.0 + nx, a.1 + ny),
                (b.0 + nx, b.1 + ny),
                (b.0 - nx, b.1 - ny),
                (a.0 - nx, a.1 - ny),
            ],
            color,
        );
    }

    /// Кружок (восьмиугольник) радиуса `r`.
    fn dot(&mut self, (x, y): (f32, f32), r: f32, color: Rgba) {
        let points: Vec<(f32, f32)> = (0..8)
            .map(|i| {
                let a = i as f32 * TAU / 8.0;
                (x + r * a.cos(), y + r * a.sin())
            })
            .collect();
        self.polygon(&points, color);
    }

    fn shape(&mut self, text: &str, size: f32) -> Vec<ShapedRun> {
        if text.is_empty() {
            return Vec::new();
        }
        self.cache
            .shape(self.library, text, &TextStyle::new(FONT, size))
    }

    /// Высота над и под базовой линией у текста размера `size`, пт.
    fn extent(&mut self, runs: &[ShapedRun], size: f32) -> (f32, f32) {
        runs.first()
            .and_then(|run| self.cache.metrics(self.library, run.font.id))
            .map_or((size * 0.75, size * 0.25), |m| {
                (m.ascent * size, m.descent * size)
            })
    }

    /// Глифы текста от точки `x` на базовой линии `baseline`.
    fn text(&mut self, runs: &[ShapedRun], x: f32, baseline: f32, color: Rgba) {
        let mut pen = x;
        for run in runs {
            let size_64 = (run.size * self.scale * 64.0).round().max(1.0) as u32;
            for glyph in &run.glyphs {
                let (gx, gy) = self.point((pen + glyph.x_offset, baseline - glyph.y_offset));
                let whole = gx.floor();
                let subpixel = (((gx - whole) * f32::from(SUBPIXEL_STEPS)).floor() as u8)
                    .min(SUBPIXEL_STEPS - 1);
                self.out.push(DrawItem::Glyph {
                    key: GlyphKey {
                        font: run.font.id,
                        glyph_id: glyph.glyph_id,
                        size_64,
                        subpixel,
                        bold: run.font.synthetic_bold,
                        italic: run.font.synthetic_italic,
                        width_percent: 100,
                        rotation: 0,
                    },
                    x: whole as i32,
                    y: gy.round() as i32,
                    color,
                });
                pen += glyph.x_advance;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::ChartSeries;

    #[test]
    fn nice_scales() {
        let s = Scale::nice(0.0, 4.5, 10);
        assert_eq!((s.min, s.max, s.step), (0.0, 5.0, 0.5));
        let s = Scale::nice(-8.0, 9.0, 5);
        assert_eq!((s.min, s.max, s.step), (-10.0, 10.0, 5.0));
        let s = Scale::nice(3.0, 3.0, 5);
        assert!(s.min <= 0.0 && s.max >= 3.0 && s.step > 0.0);
        let s = Scale::nice(0.0, 100.0, 5);
        assert_eq!((s.max, s.step, s.ticks().len()), (120.0, 20.0, 7));
    }

    #[test]
    fn numbers_on_axis() {
        assert_eq!(format_number(2.5, 0.5, false), "2,5");
        assert_eq!(format_number(20.0, 5.0, false), "20");
        assert_eq!(format_number(-0.0000001, 0.1, false), "0,0");
        assert_eq!(format_number(40.0, 20.0, true), "40%");
    }

    #[test]
    fn stacked_values_add_up() {
        let chart = Chart {
            grouping: ChartGrouping::PercentStacked,
            series: vec![
                ChartSeries {
                    values: vec![Some(1.0), Some(2.0)],
                    ..ChartSeries::default()
                },
                ChartSeries {
                    values: vec![Some(3.0), None],
                    ..ChartSeries::default()
                },
            ],
            ..Chart::default()
        };
        let values = stacked_values(&chart, 2);
        assert_eq!(values[0][0], Some((0.0, 25.0)));
        assert_eq!(values[1][0], Some((25.0, 100.0)));
        assert_eq!(values[0][1], Some((0.0, 100.0)));
        assert_eq!(values[1][1], None);
    }

    #[test]
    fn every_kind_draws_inside_its_box() {
        let library = FontLibrary::embedded_only();
        let mut cache = GlyphCache::new();
        for kind in [
            ChartKind::Column,
            ChartKind::Bar,
            ChartKind::Line,
            ChartKind::Area,
            ChartKind::Pie,
            ChartKind::Doughnut,
            ChartKind::Scatter,
        ] {
            let chart = Chart {
                kind,
                title: Some("Заголовок".into()),
                categories: vec!["А".into(), "Б".into(), "В".into()],
                series: vec![ChartSeries {
                    name: "Ряд".into(),
                    values: vec![Some(1.0), Some(-2.0), Some(3.0)],
                    ..ChartSeries::default()
                }],
                legend: Some(LegendPosition::Bottom),
                ..Chart::default()
            };
            let items = chart_items(&chart, 10.0, 20.0, 300.0, 200.0, 2.0, &library, &mut cache);
            assert!(
                items.iter().any(|i| matches!(i, DrawItem::Polygon { .. })) || items.len() > 10,
                "{kind:?}"
            );
            let inside =
                |x: f32, y: f32| (19.0..=621.0).contains(&x) && (39.0..=441.0).contains(&y);
            for item in &items {
                match item {
                    DrawItem::Rect {
                        x,
                        y,
                        width,
                        height,
                        ..
                    } => assert!(inside(*x, *y) && inside(x + width, y + height), "{kind:?}"),
                    DrawItem::Polygon { points, .. } => {
                        assert!(points.iter().all(|&(x, y)| inside(x, y)), "{kind:?}")
                    }
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn empty_chart_and_zero_size_draw_safely() {
        let library = FontLibrary::empty();
        let mut cache = GlyphCache::new();
        let chart = Chart::default();
        let items = chart_items(&chart, 0.0, 0.0, 100.0, 50.0, 1.0, &library, &mut cache);
        assert!(!items.is_empty());
        assert!(chart_items(&chart, 0.0, 0.0, 0.0, 0.0, 1.0, &library, &mut cache).is_empty());
    }
}
