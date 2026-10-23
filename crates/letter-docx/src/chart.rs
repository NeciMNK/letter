//! Диаграммы: часть `c:chartSpace` (`word/charts/chart1.xml`).
//!
//! Из диаграммы берётся только то, что нужно для показа: вид,
//! заголовок, легенда, ряды и их значения из кеша (`c:strCache`,
//! `c:numCache` — последние значения, которые сохранил Word), цвета
//! рядов. Сама часть при сохранении не меняется.

use letter_model::{Chart, ChartGrouping, ChartKind, ChartSeries, LegendPosition};

use crate::xml::Element;

/// Цвета `accent1`–`accent6` из схемы цветов темы
/// (`a:themeElements/a:clrScheme`). Пусто, если их нет.
pub(crate) fn theme_palette(theme: &Element) -> Vec<(u8, u8, u8)> {
    let Some(scheme) = child(theme, "themeElements").and_then(|e| child(e, "clrScheme")) else {
        return Vec::new();
    };
    let mut palette = Vec::new();
    for n in 1..=6 {
        let Some(color) = child(scheme, &format!("accent{n}")).and_then(|accent| {
            let value = accent.elements().next()?;
            match value.local.as_str() {
                "srgbClr" => hex(attr(value, "val")?),
                "sysClr" => hex(attr(value, "lastClr")?),
                _ => None,
            }
        }) else {
            return Vec::new();
        };
        palette.push(color);
    }
    palette
}

/// Разбирает `c:chartSpace`. `None`, если диаграмма непонятного вида
/// (лепестковая, поверхность, новые диаграммы Office 2016) или без рядов:
/// тогда вместо неё рисуется заглушка.
pub(crate) fn read_chart(root: &Element, palette: &[(u8, u8, u8)]) -> Option<Chart> {
    if root.local != "chartSpace" {
        return None;
    }
    let chart = child(root, "chart")?;
    let plot = child(chart, "plotArea")?;
    let mut kind = None;
    let mut grouping = ChartGrouping::Standard;
    let mut vary_colors = false;
    let mut gap_width = 150;
    let mut overlap = 0;
    let mut series = Vec::new();
    let mut categories = Vec::new();
    for group in plot.elements() {
        let Some(group_kind) = group_kind(group) else {
            continue;
        };
        if kind.is_none() {
            kind = Some(group_kind);
            grouping = match child(group, "grouping").and_then(|g| attr(g, "val")) {
                Some("stacked") => ChartGrouping::Stacked,
                Some("percentStacked") => ChartGrouping::PercentStacked,
                _ => ChartGrouping::Standard,
            };
            vary_colors = child(group, "varyColors").is_some_and(boolean);
            let number = |name: &str| {
                child(group, name)
                    .and_then(|e| attr(e, "val"))
                    .and_then(|v| v.trim_end_matches('%').parse::<i32>().ok())
            };
            gap_width = number("gapWidth").map_or(150, |v| v.clamp(0, 500) as u16);
            overlap = match number("overlap") {
                Some(v) => v.clamp(-100, 100) as i16,
                None if grouping != ChartGrouping::Standard => 100,
                None => 0,
            };
        }
        for ser in group.elements().filter(|e| e.local == "ser") {
            let (read, cats) = read_series(ser, group_kind, palette);
            if categories.is_empty() {
                categories = cats;
            }
            series.push(read);
        }
    }
    let kind = kind?;
    if series.is_empty() {
        return None;
    }
    let legend = child(chart, "legend").map(|legend| {
        match child(legend, "legendPos").and_then(|p| attr(p, "val")) {
            Some("b") => LegendPosition::Bottom,
            Some("t") => LegendPosition::Top,
            Some("l") => LegendPosition::Left,
            _ => LegendPosition::Right,
        }
    });
    let title = match child(chart, "title") {
        Some(title) => Some(title_text(title).unwrap_or_else(|| auto_title(&series))),
        None => {
            let deleted = child(chart, "autoTitleDeleted").is_some_and(boolean);
            (!deleted && series.len() == 1).then(|| series[0].name.clone())
        }
    }
    .filter(|t| !t.trim().is_empty());
    Some(Chart {
        kind,
        grouping,
        title,
        categories,
        series,
        legend,
        vary_colors,
        gap_width,
        overlap,
        palette: palette.to_vec(),
    })
}

/// Вид диаграммы по имени группы рядов (`c:barChart` и т. п.).
fn group_kind(group: &Element) -> Option<ChartKind> {
    Some(match group.local.as_str() {
        "barChart" | "bar3DChart" => match child(group, "barDir").and_then(|d| attr(d, "val")) {
            Some("bar") => ChartKind::Bar,
            _ => ChartKind::Column,
        },
        "lineChart" | "line3DChart" | "stockChart" => ChartKind::Line,
        "areaChart" | "area3DChart" => ChartKind::Area,
        "pieChart" | "pie3DChart" | "ofPieChart" => ChartKind::Pie,
        "doughnutChart" => ChartKind::Doughnut,
        "scatterChart" | "bubbleChart" => ChartKind::Scatter,
        _ => return None,
    })
}

/// Ряд `c:ser` и подписи его категорий.
fn read_series(
    ser: &Element,
    kind: ChartKind,
    palette: &[(u8, u8, u8)],
) -> (ChartSeries, Vec<String>) {
    let name = child(ser, "tx")
        .map(|tx| match child(tx, "v") {
            Some(v) => v.text(),
            None => strings(tx)
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" "),
        })
        .unwrap_or_default();
    let (cats, values, x_values) = if kind == ChartKind::Scatter {
        let x = child(ser, "xVal");
        let y = child(ser, "yVal").map(numbers).unwrap_or_default();
        // X бывают и подписями — тогда точки идут по порядку.
        let xs = x.map(numbers).unwrap_or_default();
        (Vec::new(), y, xs)
    } else {
        let cats = child(ser, "cat")
            .map(|c| {
                strings(c)
                    .into_iter()
                    .map(Option::unwrap_or_default)
                    .collect()
            })
            .unwrap_or_default();
        let values = child(ser, "val").map(numbers).unwrap_or_default();
        (cats, values, Vec::new())
    };
    // У линии цвет — у линии, у остальных — у заливки.
    let line = matches!(kind, ChartKind::Line | ChartKind::Scatter);
    let color = child(ser, "spPr").and_then(|sp| shape_color(sp, line, palette));
    let point_colors = ser
        .elements()
        .filter(|e| e.local == "dPt")
        .filter_map(|pt| {
            let idx = child(pt, "idx")
                .and_then(|i| attr(i, "val"))?
                .parse()
                .ok()?;
            let color = child(pt, "spPr").and_then(|sp| shape_color(sp, line, palette))?;
            Some((idx, color))
        })
        .collect();
    let series = ChartSeries {
        name,
        values,
        x_values,
        color,
        point_colors,
    };
    (series, cats)
}

/// Текст заголовка: `c:tx/c:rich` (абзацы через пробел) или ссылка
/// на ячейку с кешем.
fn title_text(title: &Element) -> Option<String> {
    let tx = child(title, "tx")?;
    if let Some(rich) = child(tx, "rich") {
        let paragraphs: Vec<String> = rich
            .elements()
            .filter(|e| e.local == "p")
            .map(|p| {
                let mut text = String::new();
                for r in p.elements().filter(|e| e.local == "r" || e.local == "fld") {
                    if let Some(t) = child(r, "t") {
                        text.push_str(&t.text());
                    }
                }
                text
            })
            .collect();
        return Some(paragraphs.join(" "));
    }
    Some(
        strings(tx)
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// Заголовок, который Word ставит сам: имя единственного ряда
/// или «Название диаграммы».
fn auto_title(series: &[ChartSeries]) -> String {
    match series {
        [one] if !one.name.is_empty() => one.name.clone(),
        _ => "Название диаграммы".to_string(),
    }
}

/// Значения кеша строк (`c:strRef/c:strCache`, `c:strLit`, многоуровневые
/// подписи — нижний уровень; числа как текст) по номерам точек.
fn strings(holder: &Element) -> Vec<Option<String>> {
    let Some(source) = holder.elements().next() else {
        return Vec::new();
    };
    let cache = match source.local.as_str() {
        "strRef" => child(source, "strCache"),
        "numRef" => child(source, "numCache"),
        "multiLvlStrRef" => {
            child(source, "multiLvlStrCache").and_then(|c| c.elements().find(|e| e.local == "lvl"))
        }
        "strLit" | "numLit" => Some(source),
        _ => None,
    };
    cache.map(points).unwrap_or_default()
}

/// Числа кеша (`c:numRef/c:numCache` или `c:numLit`); не число — `None`.
fn numbers(holder: &Element) -> Vec<Option<f64>> {
    strings(holder)
        .into_iter()
        .map(|s| {
            s.and_then(|s| s.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite())
        })
        .collect()
}

/// Самое большое число точек ряда: защита от `ptCount` в миллиард.
const MAX_POINTS: usize = 10_000;

/// Точки `c:pt idx=… / c:v` по номерам; `c:ptCount` задаёт длину.
fn points(cache: &Element) -> Vec<Option<String>> {
    let count: usize = child(cache, "ptCount")
        .and_then(|c| attr(c, "val"))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut out: Vec<Option<String>> = vec![None; count.min(MAX_POINTS)];
    for pt in cache.elements().filter(|e| e.local == "pt") {
        let Some(idx) = attr(pt, "idx").and_then(|i| i.parse::<usize>().ok()) else {
            continue;
        };
        if idx >= MAX_POINTS {
            continue;
        }
        if idx >= out.len() {
            out.resize(idx + 1, None);
        }
        out[idx] = child(pt, "v").map(|v| v.text());
    }
    out
}

/// Цвет заливки (или линии, если `line`) из `c:spPr`.
fn shape_color(sp: &Element, line: bool, palette: &[(u8, u8, u8)]) -> Option<(u8, u8, u8)> {
    let fill = || child(sp, "solidFill");
    let outline = || child(sp, "ln").and_then(|ln| child(ln, "solidFill"));
    let solid = if line {
        outline().or_else(fill)
    } else {
        fill().or_else(outline)
    }?;
    let value = solid.elements().next()?;
    let base = match value.local.as_str() {
        "srgbClr" => hex(attr(value, "val")?)?,
        "sysClr" => hex(attr(value, "lastClr")?)?,
        "schemeClr" => scheme_color(attr(value, "val")?, palette)?,
        "prstClr" => match attr(value, "val")? {
            "black" => (0, 0, 0),
            "white" => (255, 255, 255),
            "red" => (255, 0, 0),
            "green" => (0, 128, 0),
            "blue" => (0, 0, 255),
            "yellow" => (255, 255, 0),
            _ => return None,
        },
        _ => return None,
    };
    Some(modified(base, value))
}

/// Цвет схемы темы по имени (`accent1`, `tx1` и т. п.).
fn scheme_color(name: &str, palette: &[(u8, u8, u8)]) -> Option<(u8, u8, u8)> {
    let palette = if palette.len() == 6 {
        palette
    } else {
        &letter_model::DEFAULT_PALETTE[..]
    };
    match name {
        "tx1" | "dk1" => Some((0, 0, 0)),
        "bg1" | "lt1" => Some((255, 255, 255)),
        "tx2" | "dk2" => Some((0x0E, 0x28, 0x41)),
        "bg2" | "lt2" => Some((0xE8, 0xE8, 0xE8)),
        _ => {
            let n: usize = name.strip_prefix("accent")?.parse().ok()?;
            palette.get(n.checked_sub(1)?).copied()
        }
    }
}

/// Цвет с поправками `a:lumMod`, `a:lumOff` (яркость в HSL), `a:shade`
/// (темнее) и `a:tint` (светлее); значения — в тысячных долях процента.
fn modified(base: (u8, u8, u8), value: &Element) -> (u8, u8, u8) {
    let (mut h, mut s, mut l) = to_hsl(base);
    let mut rgb = None;
    for m in value.elements() {
        let Some(v) = attr(m, "val").and_then(|v| v.parse::<f32>().ok()) else {
            continue;
        };
        let v = v / 100_000.0;
        if let Some(c) = rgb.take() {
            (h, s, l) = to_hsl(c);
        }
        match m.local.as_str() {
            "lumMod" => l = (l * v).clamp(0.0, 1.0),
            "lumOff" => l = (l + v).clamp(0.0, 1.0),
            "shade" => {
                let (r, g, b) = from_hsl(h, s, l);
                let f = |c: u8| (f32::from(c) * v).round().clamp(0.0, 255.0) as u8;
                rgb = Some((f(r), f(g), f(b)));
            }
            "tint" => {
                let (r, g, b) = from_hsl(h, s, l);
                let f = |c: u8| {
                    (f32::from(c) + (255.0 - f32::from(c)) * (1.0 - v))
                        .round()
                        .clamp(0.0, 255.0) as u8
                };
                rgb = Some((f(r), f(g), f(b)));
            }
            _ => {}
        }
    }
    rgb.unwrap_or_else(|| from_hsl(h, s, l))
}

fn to_hsl((r, g, b): (u8, u8, u8)) -> (f32, f32, f32) {
    let (r, g, b) = (
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h / 6.0, s, l)
}

fn from_hsl(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let to = |c: f32| (c * 255.0).round().clamp(0.0, 255.0) as u8;
    if s == 0.0 {
        return (to(l), to(l), to(l));
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let hue = |mut t: f32| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    (to(hue(h + 1.0 / 3.0)), to(hue(h)), to(hue(h - 1.0 / 3.0)))
}

/// `RRGGBB` → цвет.
fn hex(value: &str) -> Option<(u8, u8, u8)> {
    if value.len() != 6 || !value.is_ascii() {
        return None;
    }
    let part = |i: usize| u8::from_str_radix(value.get(i..i + 2)?, 16).ok();
    Some((part(0)?, part(2)?, part(4)?))
}

/// Флажок `val`: без атрибута — «да».
fn boolean(element: &Element) -> bool {
    !matches!(attr(element, "val"), Some("0" | "false"))
}

fn child<'a>(element: &'a Element, local: &str) -> Option<&'a Element> {
    element.elements().find(|e| e.local == local)
}

fn attr<'a>(element: &'a Element, local: &str) -> Option<&'a str> {
    element
        .attrs
        .iter()
        .find(|a| a.local == local)
        .map(|a| a.value.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml;

    const C: &str = "xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" \
                     xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"";

    fn parse(body: &str) -> Option<Chart> {
        let text = format!("<c:chartSpace {C}><c:chart>{body}</c:chart></c:chartSpace>");
        let root = xml::parse(text.as_bytes()).ok()?;
        read_chart(&root, &[])
    }

    #[test]
    fn reads_column_chart_from_cache() {
        let chart = parse(
            "<c:title><c:tx><c:rich><a:p><a:r><a:t>Продажи</a:t></a:r></a:p></c:rich></c:tx></c:title>\
             <c:plotArea><c:barChart><c:barDir val=\"col\"/><c:grouping val=\"stacked\"/>\
             <c:varyColors val=\"0\"/>\
             <c:ser><c:idx val=\"0\"/><c:tx><c:strRef><c:f>Лист1!$B$1</c:f><c:strCache>\
             <c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>Ряд 1</c:v></c:pt></c:strCache></c:strRef></c:tx>\
             <c:spPr><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></c:spPr>\
             <c:cat><c:strRef><c:strCache><c:ptCount val=\"3\"/>\
             <c:pt idx=\"0\"><c:v>Январь</c:v></c:pt><c:pt idx=\"2\"><c:v>Март</c:v></c:pt>\
             </c:strCache></c:strRef></c:cat>\
             <c:val><c:numRef><c:numCache><c:ptCount val=\"3\"/>\
             <c:pt idx=\"0\"><c:v>4.3</c:v></c:pt><c:pt idx=\"1\"><c:v>2.5E1</c:v></c:pt>\
             </c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea>\
             <c:legend><c:legendPos val=\"b\"/></c:legend>",
        )
        .unwrap();
        assert_eq!(chart.kind, ChartKind::Column);
        assert_eq!(chart.grouping, ChartGrouping::Stacked);
        assert_eq!(chart.title.as_deref(), Some("Продажи"));
        assert_eq!(chart.categories, ["Январь", "", "Март"]);
        assert_eq!(chart.legend, Some(LegendPosition::Bottom));
        assert!(!chart.vary_colors);
        assert_eq!((chart.gap_width, chart.overlap), (150, 100));
        let series = &chart.series[0];
        assert_eq!(series.name, "Ряд 1");
        assert_eq!(series.values, [Some(4.3), Some(25.0), None]);
        assert_eq!(series.color, Some((255, 0, 0)));
    }

    #[test]
    fn reads_pie_with_scheme_colors_and_auto_title() {
        let chart = parse(
            "<c:autoTitleDeleted val=\"0\"/><c:plotArea><c:pieChart><c:varyColors val=\"1\"/>\
             <c:ser><c:tx><c:v>Доля</c:v></c:tx>\
             <c:dPt><c:idx val=\"1\"/><c:spPr><a:solidFill><a:schemeClr val=\"accent2\">\
             <a:lumMod val=\"50000\"/></a:schemeClr></a:solidFill></c:spPr></c:dPt>\
             <c:val><c:numLit><c:ptCount val=\"2\"/><c:pt idx=\"0\"><c:v>1</c:v></c:pt>\
             <c:pt idx=\"1\"><c:v>3</c:v></c:pt></c:numLit></c:val></c:ser></c:pieChart></c:plotArea>",
        )
        .unwrap();
        assert_eq!(chart.kind, ChartKind::Pie);
        assert!(chart.vary_colors);
        assert_eq!(chart.title.as_deref(), Some("Доля"));
        assert_eq!(chart.legend, None);
        let (idx, (r, g, b)) = chart.series[0].point_colors[0];
        assert_eq!(idx, 1);
        // Тёмно-оранжевый: accent2 по умолчанию (E97132) в два раза темнее.
        assert!(r > g && g > b && r < 0xE9, "{r} {g} {b}");
    }

    #[test]
    fn unknown_kinds_and_empty_charts_are_not_read() {
        assert!(parse("<c:plotArea><c:radarChart><c:ser/></c:radarChart></c:plotArea>").is_none());
        assert!(parse("<c:plotArea><c:lineChart/></c:plotArea>").is_none());
        let huge = "<c:plotArea><c:lineChart><c:ser><c:val><c:numLit>\
                    <c:ptCount val=\"999999999\"/><c:pt idx=\"99999999\"><c:v>1</c:v></c:pt>\
                    </c:numLit></c:val></c:ser></c:lineChart></c:plotArea>";
        let chart = parse(huge).unwrap();
        assert_eq!(chart.series[0].values.len(), MAX_POINTS);
    }

    #[test]
    fn reads_theme_palette() {
        let theme = "<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">\
                     <a:themeElements><a:clrScheme name=\"x\">\
                     <a:dk1><a:sysClr val=\"windowText\" lastClr=\"000000\"/></a:dk1>\
                     <a:accent1><a:srgbClr val=\"4472C4\"/></a:accent1>\
                     <a:accent2><a:srgbClr val=\"ED7D31\"/></a:accent2>\
                     <a:accent3><a:srgbClr val=\"A5A5A5\"/></a:accent3>\
                     <a:accent4><a:srgbClr val=\"FFC000\"/></a:accent4>\
                     <a:accent5><a:srgbClr val=\"5B9BD5\"/></a:accent5>\
                     <a:accent6><a:sysClr val=\"x\" lastClr=\"70AD47\"/></a:accent6>\
                     </a:clrScheme></a:themeElements></a:theme>";
        let root = xml::parse(theme.as_bytes()).unwrap();
        let palette = theme_palette(&root);
        assert_eq!(palette.len(), 6);
        assert_eq!(palette[0], (0x44, 0x72, 0xC4));
        assert_eq!(palette[5], (0x70, 0xAD, 0x47));
    }

    #[test]
    fn hsl_round_trip() {
        for c in [
            (0x15, 0x60, 0x82),
            (255, 255, 255),
            (0, 0, 0),
            (0xE9, 0x71, 0x32),
        ] {
            let (h, s, l) = to_hsl(c);
            assert_eq!(from_hsl(h, s, l), c);
        }
    }
}
