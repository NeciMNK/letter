//! SmartArt: готовый вид диаграммы.
//!
//! В `w:drawing` SmartArt лежит не картинка, а ссылки на четыре части
//! диаграммы (`dgm:relIds`: данные, макет, стиль, цвета). Вместе с ними
//! Word сохраняет готовый вид диаграммы — фигуры с заливкой, контуром
//! и текстом (`word/diagrams/drawing1.xml`, корень `dsp:drawing`). На
//! эту часть указывает `dsp:dataModelExt relId` в части данных. Здесь
//! из неё строится [`Drawing`]: контуры фигур, цвета (с цветами темы)
//! и текст. Сама диаграмма не разбирается и при сохранении остаётся
//! как была.

use std::f64::consts::PI;

use letter_model::drawing::{
    Contour, DrawingShape, Rgba, ShapeLine, ShapeParagraph, ShapeRun, ShapeText, TextAlign,
    TextAnchor,
};
use letter_model::{Drawing, Theme};

use crate::xml::Element;

/// Связь части данных диаграммы с её готовым видом.
pub(crate) const DRAWING_RELATIONSHIP: &str = "/diagramDrawing";

/// Сколько отрезков на полный оборот дуги.
const ARC_STEPS: f64 = 96.0;

/// Самая большая координата, EMU: защищает от бессмысленных чисел.
const MAX_EMU: f64 = 201_168_000.0;

/// Цвета темы (`a:clrScheme`): имя (`dk1`, `accent1`…) и цвет.
#[derive(Debug, Clone, Default)]
pub(crate) struct Colors(Vec<(String, [u8; 3])>);

/// Читает цвета из `a:theme/a:themeElements/a:clrScheme`.
pub(crate) fn read_colors(theme: &Element) -> Colors {
    let scheme = theme
        .elements()
        .filter(|e| e.local == "themeElements")
        .flat_map(Element::elements)
        .find(|e| e.local == "clrScheme");
    let mut colors = Vec::new();
    for slot in scheme.into_iter().flat_map(Element::elements) {
        let value = slot.elements().find_map(|c| match c.local.as_str() {
            "srgbClr" => attr(c, "val").and_then(hex),
            "sysClr" => attr(c, "lastClr").and_then(hex),
            _ => None,
        });
        if let Some(rgb) = value {
            colors.push((slot.local.clone(), rgb));
        }
    }
    Colors(colors)
}

impl Colors {
    fn get(&self, name: &str) -> Option<[u8; 3]> {
        let name = match name {
            "bg1" => "lt1",
            "tx1" => "dk1",
            "bg2" => "lt2",
            "tx2" => "dk2",
            other => other,
        };
        self.0
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, c)| *c)
            // Цвета темы Office по умолчанию, если в теме их нет.
            .or_else(|| {
                Some(match name {
                    "dk1" => [0, 0, 0],
                    "lt1" => [255, 255, 255],
                    "dk2" => [0x44, 0x54, 0x6A],
                    "lt2" => [0xE7, 0xE6, 0xE6],
                    "accent1" => [0x44, 0x72, 0xC4],
                    "accent2" => [0xED, 0x7D, 0x31],
                    "accent3" => [0xA5, 0xA5, 0xA5],
                    "accent4" => [0xFF, 0xC0, 0x00],
                    "accent5" => [0x5B, 0x9B, 0xD5],
                    "accent6" => [0x70, 0xAD, 0x47],
                    "hlink" => [0x05, 0x63, 0xC1],
                    "folHlink" => [0x95, 0x4F, 0x72],
                    _ => return None,
                })
            })
    }
}

/// Имя связи с частью данных диаграммы (`dgm:relIds r:dm`), если
/// `w:drawing` — SmartArt.
pub(crate) fn data_relationship(drawing: &Element) -> Option<&str> {
    let frame = drawing
        .elements()
        .find(|e| e.local == "inline" || e.local == "anchor")?;
    let graphic = frame.elements().find(|e| e.local == "graphic")?;
    let data = graphic.elements().find(|e| e.local == "graphicData")?;
    let ids = data.elements().find(|e| e.local == "relIds")?;
    attr(ids, "dm")
}

/// Имя связи с готовым видом из части данных диаграммы
/// (`dsp:dataModelExt relId`).
pub(crate) fn drawing_relationship(data: &Element) -> Option<&str> {
    find(data, "dataModelExt").and_then(|e| attr(e, "relId"))
}

/// Готовый вид диаграммы из `dsp:drawing`. `width` и `height` —
/// размер рамки диаграммы в документе, EMU: координаты фигур отсчитаны
/// от её угла.
pub(crate) fn read_drawing(
    root: &Element,
    width: i64,
    height: i64,
    colors: &Colors,
    theme: &Theme,
) -> Option<Drawing> {
    if root.local != "drawing" {
        return None;
    }
    let tree = root.elements().find(|e| e.local == "spTree")?;
    let mut shapes = Vec::new();
    let context = Context { colors, theme };
    collect_shapes(tree, &context, &mut shapes, 0);
    // Координаты готового вида отсчитаны от рамки того размера, при
    // котором Word его построил. Если рамку потом уменьшили (у нас — без
    // нового готового вида), фигуры вылезают за неё: тогда оси
    // увеличиваются так, чтобы всё влезло.
    let (right, bottom) = shapes
        .iter()
        .flat_map(|s| &s.contours)
        .flat_map(|c| &c.points)
        .fold((0, 0), |(r, b), &(x, y)| (r.max(x), b.max(y)));
    let (width, height) = (width.max(1), height.max(1));
    let factor = (right as f64 / width as f64).max(bottom as f64 / height as f64);
    let (width, height) = if factor > 1.01 {
        (
            (width as f64 * factor).round() as i64,
            (height as f64 * factor).round() as i64,
        )
    } else {
        (width, height)
    };
    Some(Drawing {
        width,
        height,
        shapes,
    })
}

struct Context<'a> {
    colors: &'a Colors,
    theme: &'a Theme,
}

/// Фигуры группы по порядку (вложенные группы — тоже; их координаты
/// в готовом виде уже в осях рисунка).
fn collect_shapes(group: &Element, context: &Context, out: &mut Vec<DrawingShape>, depth: u32) {
    if depth > 16 {
        return;
    }
    for child in group.elements() {
        match child.local.as_str() {
            "sp" => out.extend(read_shape(child, context)),
            "grpSp" => collect_shapes(child, context, out, depth + 1),
            _ => {}
        }
    }
}

/// Цвета из `dsp:style`: ссылки на заливку, линию и шрифт темы.
#[derive(Default)]
struct StyleRefs {
    fill: Option<(u32, Option<Rgba>)>,
    line: Option<(u32, Option<Rgba>)>,
    font: Option<(String, Option<Rgba>)>,
}

fn read_style(style: Option<&Element>, colors: &Colors) -> StyleRefs {
    let Some(style) = style else {
        return StyleRefs::default();
    };
    let reference = |local: &str| style.elements().find(|e| e.local == local);
    let indexed = |local: &str| {
        let r = reference(local)?;
        let idx = attr(r, "idx").and_then(|v| v.parse().ok()).unwrap_or(0);
        Some((idx, read_color(r, colors, None)))
    };
    StyleRefs {
        fill: indexed("fillRef"),
        line: indexed("lnRef"),
        font: reference("fontRef").map(|r| {
            (
                attr(r, "idx").unwrap_or("minor").to_string(),
                read_color(r, colors, None),
            )
        }),
    }
}

fn read_shape(shape: &Element, context: &Context) -> Option<DrawingShape> {
    let props = shape.elements().find(|e| e.local == "spPr")?;
    let xfrm = props.elements().find(|e| e.local == "xfrm")?;
    let frame = read_xfrm(xfrm)?;
    let style = read_style(
        shape.elements().find(|e| e.local == "style"),
        context.colors,
    );
    let colors = context.colors;

    let contours = geometry(props, &frame);

    let style_fill = style
        .fill
        .and_then(|(idx, color)| if idx == 0 { None } else { color });
    let fill = match read_fill(props, colors, style_fill) {
        Paint::Set(color) => color,
        Paint::Unset => style_fill,
    };

    let style_line = style
        .line
        .and_then(|(idx, color)| Some((idx, color?)))
        .filter(|(idx, _)| *idx > 0);
    let default_width = |idx: u32| match idx {
        1 => 6_350,
        2 => 12_700,
        _ => 19_050,
    };
    let line = match props.elements().find(|e| e.local == "ln") {
        Some(ln) => {
            let width = attr(ln, "w")
                .and_then(|w| w.parse::<i64>().ok())
                .filter(|w| (0..=MAX_EMU as i64).contains(w));
            let color = match read_fill(ln, colors, style_line.map(|(_, c)| c)) {
                Paint::Set(color) => color,
                Paint::Unset => style_line.map(|(_, c)| c),
            };
            color.map(|color| ShapeLine {
                color,
                width: width.unwrap_or_else(|| style_line.map_or(9_525, |(i, _)| default_width(i))),
            })
        }
        None => style_line.map(|(idx, color)| ShapeLine {
            color,
            width: default_width(idx),
        }),
    };

    let text = shape
        .elements()
        .find(|e| e.local == "txBody")
        .and_then(|body| {
            let box_ = shape
                .elements()
                .find(|e| e.local == "txXfrm")
                .and_then(read_xfrm)
                .unwrap_or(frame);
            read_text(body, &box_, &style, context)
        });

    Some(DrawingShape {
        contours,
        fill,
        line,
        text,
    })
}

/// Рамка фигуры (`a:xfrm`): положение, размер, поворот, отражения.
#[derive(Debug, Clone, Copy)]
struct Frame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    /// Поворот по часовой стрелке, радианы.
    rotation: f64,
    flip_h: bool,
    flip_v: bool,
}

fn read_xfrm(xfrm: &Element) -> Option<Frame> {
    let number = |e: &Element, name: &str| -> Option<f64> {
        let value: f64 = attr(e, name)?.parse().ok()?;
        (value.abs() <= MAX_EMU).then_some(value)
    };
    let off = xfrm.elements().find(|e| e.local == "off");
    let ext = xfrm.elements().find(|e| e.local == "ext")?;
    let flag = |name: &str| matches!(attr(xfrm, name), Some("1" | "true"));
    Some(Frame {
        x: off.and_then(|o| number(o, "x")).unwrap_or(0.0),
        y: off.and_then(|o| number(o, "y")).unwrap_or(0.0),
        width: number(ext, "cx")?.max(0.0),
        height: number(ext, "cy")?.max(0.0),
        rotation: attr(xfrm, "rot")
            .and_then(|r| r.parse::<f64>().ok())
            .map_or(0.0, |r| (r / 60_000.0).to_radians()),
        flip_h: flag("flipH"),
        flip_v: flag("flipV"),
    })
}

impl Frame {
    /// Точка в осях фигуры (от её угла) → точка рисунка: отражение,
    /// поворот вокруг середины, сдвиг.
    fn place(&self, (x, y): (f64, f64)) -> (i64, i64) {
        let (cx, cy) = (self.width / 2.0, self.height / 2.0);
        let mut dx = x - cx;
        let mut dy = y - cy;
        if self.flip_h {
            dx = -dx;
        }
        if self.flip_v {
            dy = -dy;
        }
        let (sin, cos) = self.rotation.sin_cos();
        let rx = dx * cos - dy * sin;
        let ry = dx * sin + dy * cos;
        let clamp = |v: f64| v.clamp(-MAX_EMU, MAX_EMU).round() as i64;
        (clamp(self.x + cx + rx), clamp(self.y + cy + ry))
    }
}

/// Контур в осях фигуры, до поворота.
struct Path {
    points: Vec<(f64, f64)>,
    closed: bool,
}

impl Path {
    fn closed(points: Vec<(f64, f64)>) -> Path {
        Path {
            points,
            closed: true,
        }
    }
}

/// Контуры фигуры по `a:prstGeom` или `a:custGeom`; без них —
/// прямоугольник.
fn geometry(props: &Element, frame: &Frame) -> Vec<Contour> {
    let (w, h) = (frame.width, frame.height);
    let paths = if let Some(custom) = props.elements().find(|e| e.local == "custGeom") {
        custom_geometry(custom, w, h)
    } else if let Some(preset) = props.elements().find(|e| e.local == "prstGeom") {
        let adjust = adjustments(preset);
        preset_geometry(attr(preset, "prst").unwrap_or("rect"), w, h, &adjust)
    } else {
        vec![rect(w, h)]
    };
    paths
        .into_iter()
        .filter(|p| p.points.len() >= 2)
        .map(|p| Contour {
            points: p.points.iter().map(|&pt| frame.place(pt)).collect(),
            closed: p.closed,
        })
        .collect()
}

/// Значения `a:avLst/a:gd fmla="val N"` по именам.
fn adjustments(geometry: &Element) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    for list in geometry.elements().filter(|e| e.local == "avLst") {
        for gd in list.elements().filter(|e| e.local == "gd") {
            let value = attr(gd, "fmla")
                .and_then(|f| f.strip_prefix("val "))
                .and_then(|v| v.trim().parse::<f64>().ok());
            if let (Some(name), Some(value)) = (attr(gd, "name"), value) {
                out.push((name.to_string(), value));
            }
        }
    }
    out
}

fn rect(w: f64, h: f64) -> Path {
    Path::closed(vec![(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)])
}

/// Точки дуги эллипса с серединой `(cx, cy)` и полуосями `rx`, `ry`:
/// от угла `start` на `sweep` (радианы, по часовой стрелке).
fn arc(
    out: &mut Vec<(f64, f64)>,
    (cx, cy): (f64, f64),
    (rx, ry): (f64, f64),
    start: f64,
    sweep: f64,
) {
    let steps = ((sweep.abs() / (2.0 * PI) * ARC_STEPS).ceil() as usize).clamp(1, 512);
    for i in 0..=steps {
        let a = start + sweep * i as f64 / steps as f64;
        out.push((cx + rx * a.cos(), cy + ry * a.sin()));
    }
}

/// Прямоугольник со скруглёнными углами; `radii` — радиусы углов по
/// часовой стрелке от левого верхнего.
fn rounded(w: f64, h: f64, radii: [f64; 4]) -> Path {
    let limit = w.min(h) / 2.0;
    let [tl, tr, br, bl] = radii.map(|r| r.clamp(0.0, limit));
    let mut p = Vec::new();
    let corner = |p: &mut Vec<(f64, f64)>, c: (f64, f64), r: f64, start: f64, at: (f64, f64)| {
        if r > 0.0 {
            arc(p, c, (r, r), start, PI / 2.0);
        } else {
            p.push(at);
        }
    };
    corner(&mut p, (tl, tl), tl, PI, (0.0, 0.0));
    corner(&mut p, (w - tr, tr), tr, -PI / 2.0, (w, 0.0));
    corner(&mut p, (w - br, h - br), br, 0.0, (w, h));
    corner(&mut p, (bl, h - bl), bl, PI / 2.0, (0.0, h));
    Path::closed(p)
}

/// Угол OOXML (60 000-е доли градуса) в радианах.
fn angle(value: f64) -> f64 {
    (value / 60_000.0).to_radians()
}

/// Кольцевой сектор: от угла `start` на `sweep`, внешний эллипс по
/// рамке, толщина `thickness`.
fn ring(w: f64, h: f64, start: f64, sweep: f64, thickness: f64) -> Path {
    let (cx, cy) = (w / 2.0, h / 2.0);
    let thickness = thickness.clamp(0.0, w.min(h) / 2.0);
    let mut p = Vec::new();
    arc(&mut p, (cx, cy), (cx, cy), start, sweep);
    arc(
        &mut p,
        (cx, cy),
        (cx - thickness, cy - thickness),
        start + sweep,
        -sweep,
    );
    Path::closed(p)
}

/// Положительный угол развёртки от `start` до `end` по часовой стрелке.
fn sweep_between(start: f64, end: f64) -> f64 {
    let sweep = (end - start).rem_euclid(2.0 * PI);
    if sweep == 0.0 { 2.0 * PI } else { sweep }
}

fn preset_geometry(name: &str, w: f64, h: f64, adjust: &[(String, f64)]) -> Vec<Path> {
    let ss = w.min(h);
    let adj = |name: &str, default: f64| {
        adjust
            .iter()
            .find(|(n, _)| n == name)
            .map_or(default, |(_, v)| *v)
    };
    let part = |value: f64| ss * value / 100_000.0;
    let (cx, cy) = (w / 2.0, h / 2.0);
    let path = match name {
        "roundRect" | "flowChartAlternateProcess" => {
            let r = part(adj("adj", 16_667.0));
            rounded(w, h, [r; 4])
        }
        "round2SameRect" => {
            let r = part(adj("adj1", 16_667.0));
            let r2 = part(adj("adj2", 0.0));
            rounded(w, h, [r, r, r2, r2])
        }
        "round1Rect" => rounded(w, h, [0.0, part(adj("adj", 16_667.0)), 0.0, 0.0]),
        "round2DiagRect" => {
            let r = part(adj("adj1", 16_667.0));
            let r2 = part(adj("adj2", 0.0));
            rounded(w, h, [r, r2, r, r2])
        }
        "snip1Rect" => {
            let x = part(adj("adj", 16_667.0)).min(ss);
            Path::closed(vec![(0.0, 0.0), (w - x, 0.0), (w, x), (w, h), (0.0, h)])
        }
        "ellipse" | "flowChartConnector" => {
            let mut p = Vec::new();
            arc(&mut p, (cx, cy), (cx, cy), 0.0, 2.0 * PI);
            p.pop();
            Path::closed(p)
        }
        "triangle" | "flowChartExtract" => {
            let x = w * adj("adj", 50_000.0) / 100_000.0;
            Path::closed(vec![(x, 0.0), (w, h), (0.0, h)])
        }
        "rtTriangle" => Path::closed(vec![(0.0, 0.0), (w, h), (0.0, h)]),
        "diamond" | "flowChartDecision" => {
            Path::closed(vec![(cx, 0.0), (w, cy), (cx, h), (0.0, cy)])
        }
        "parallelogram" | "flowChartInputOutput" => {
            let x = part(adj("adj", 25_000.0)).min(w);
            Path::closed(vec![(x, 0.0), (w, 0.0), (w - x, h), (0.0, h)])
        }
        "trapezoid" => {
            let x = part(adj("adj", 25_000.0)).min(cx);
            Path::closed(vec![(0.0, h), (x, 0.0), (w - x, 0.0), (w, h)])
        }
        "hexagon" => {
            let x = part(adj("adj", 25_000.0)).min(cx);
            Path::closed(vec![
                (0.0, cy),
                (x, 0.0),
                (w - x, 0.0),
                (w, cy),
                (w - x, h),
                (x, h),
            ])
        }
        "octagon" => {
            let x = part(adj("adj", 29_289.0)).min(ss / 2.0);
            Path::closed(vec![
                (x, 0.0),
                (w - x, 0.0),
                (w, x),
                (w, h - x),
                (w - x, h),
                (x, h),
                (0.0, h - x),
                (0.0, x),
            ])
        }
        "pentagon" => Path::closed(vec![
            (cx, 0.0),
            (w, h * 0.382),
            (w * 0.809, h),
            (w * 0.191, h),
            (0.0, h * 0.382),
        ]),
        "homePlate" => {
            let x = w - part(adj("adj", 50_000.0)).min(w);
            Path::closed(vec![(0.0, 0.0), (x, 0.0), (w, cy), (x, h), (0.0, h)])
        }
        "chevron" => {
            let x = part(adj("adj", 50_000.0)).min(w);
            Path::closed(vec![
                (0.0, 0.0),
                (w - x, 0.0),
                (w, cy),
                (w - x, h),
                (0.0, h),
                (x, cy),
            ])
        }
        "rightArrow" | "leftArrow" | "upArrow" | "downArrow" => {
            // Стрелка вправо в осях, где «длина» идёт по стрелке.
            let vertical = name == "upArrow" || name == "downArrow";
            let (len, wid) = if vertical { (h, w) } else { (w, h) };
            let t = wid * adj("adj1", 50_000.0) / 200_000.0;
            let head = part(adj("adj2", 50_000.0)).min(len);
            let mid = wid / 2.0;
            let arrow = [
                (0.0, mid - t),
                (len - head, mid - t),
                (len - head, 0.0),
                (len, mid),
                (len - head, wid),
                (len - head, mid + t),
                (0.0, mid + t),
            ];
            let points = arrow
                .iter()
                .map(|&(a, b)| match name {
                    "leftArrow" => (len - a, b),
                    "downArrow" => (b, a),
                    "upArrow" => (b, len - a),
                    _ => (a, b),
                })
                .collect();
            Path::closed(points)
        }
        "leftRightArrow" => {
            let t = h * adj("adj1", 50_000.0) / 200_000.0;
            let head = part(adj("adj2", 50_000.0)).min(cx);
            Path::closed(vec![
                (0.0, cy),
                (head, 0.0),
                (head, cy - t),
                (w - head, cy - t),
                (w - head, 0.0),
                (w, cy),
                (w - head, h),
                (w - head, cy + t),
                (head, cy + t),
                (head, h),
            ])
        }
        "donut" => {
            let thickness = part(adj("adj", 25_000.0));
            // Кольцо одним контуром: внешний круг и внутренний обратно.
            ring(w, h, 0.0, 2.0 * PI, thickness)
        }
        "blockArc" => {
            let start = angle(adj("adj1", 10_800_000.0));
            let end = angle(adj("adj2", 0.0));
            ring(
                w,
                h,
                start,
                sweep_between(start, end),
                part(adj("adj3", 25_000.0)),
            )
        }
        "circularArrow" | "leftCircularArrow" => {
            let thickness = part(adj("adj1", 12_500.0));
            let start = angle(adj("adj4", 10_800_000.0));
            let end = angle(adj("adj3", 20_457_681.0));
            let mut sweep = sweep_between(start, end);
            if name == "leftCircularArrow" {
                sweep -= 2.0 * PI;
            }
            circular_arrow(w, h, start, sweep, thickness)
        }
        "pie" => {
            let start = angle(adj("adj1", 0.0));
            let end = angle(adj("adj2", 16_200_000.0));
            let mut p = vec![(cx, cy)];
            arc(&mut p, (cx, cy), (cx, cy), start, sweep_between(start, end));
            Path::closed(p)
        }
        "arc" => {
            let start = angle(adj("adj1", 16_200_000.0));
            let end = angle(adj("adj2", 0.0));
            let mut p = Vec::new();
            arc(&mut p, (cx, cy), (cx, cy), start, sweep_between(start, end));
            Path {
                points: p,
                closed: false,
            }
        }
        "line" | "straightConnector1" => Path {
            points: vec![(0.0, 0.0), (w, h)],
            closed: false,
        },
        "plus" | "mathPlus" => {
            let x = part(adj("adj", 25_000.0)).min(ss / 2.0);
            Path::closed(vec![
                (x, 0.0),
                (w - x, 0.0),
                (w - x, x),
                (w, x),
                (w, h - x),
                (w - x, h - x),
                (w - x, h),
                (x, h),
                (x, h - x),
                (0.0, h - x),
                (0.0, x),
                (x, x),
            ])
        }
        // Остальные фигуры пока рисуются прямоугольником.
        _ => rect(w, h),
    };
    vec![path]
}

/// Круговая стрелка: кольцевой сектор с наконечником на конце.
fn circular_arrow(w: f64, h: f64, start: f64, sweep: f64, thickness: f64) -> Path {
    let (cx, cy) = (w / 2.0, h / 2.0);
    let thickness = thickness.clamp(0.0, w.min(h) / 4.0);
    let (rx, ry) = (cx - thickness, cy - thickness);
    let (ix, iy) = (rx - thickness, ry - thickness);
    let mid = |r: f64| r - thickness / 2.0;
    // Наконечник занимает около двух толщин по дуге.
    let head = (2.5 * thickness / cx.max(1.0)).min(sweep.abs() / 2.0) * sweep.signum();
    let body = sweep - head;
    let mut p = Vec::new();
    arc(&mut p, (cx, cy), (rx, ry), start, body);
    let end = start + body;
    let tip = start + sweep;
    p.push((
        cx + (rx + thickness) * end.cos(),
        cy + (ry + thickness) * end.sin(),
    ));
    p.push((cx + mid(rx) * tip.cos(), cy + mid(ry) * tip.sin()));
    p.push((
        cx + (ix - thickness) * end.cos(),
        cy + (iy - thickness) * end.sin(),
    ));
    arc(&mut p, (cx, cy), (ix, iy), end, -body);
    Path::closed(p)
}

/// Контуры `a:custGeom/a:pathLst/a:path`. Координаты пути — в его
/// осях `w` × `h`, растягиваются на рамку фигуры.
fn custom_geometry(custom: &Element, width: f64, height: f64) -> Vec<Path> {
    let mut out = Vec::new();
    let Some(list) = custom.elements().find(|e| e.local == "pathLst") else {
        return vec![rect(width, height)];
    };
    for path in list.elements().filter(|e| e.local == "path") {
        let number = |e: &Element, name: &str| -> Option<f64> {
            let v: f64 = attr(e, name)?.parse().ok()?;
            (v.abs() <= MAX_EMU).then_some(v)
        };
        let pw = number(path, "w").filter(|v| *v > 0.0).unwrap_or(width);
        let ph = number(path, "h").filter(|v| *v > 0.0).unwrap_or(height);
        let (sx, sy) = (
            if pw > 0.0 { width / pw } else { 1.0 },
            if ph > 0.0 { height / ph } else { 1.0 },
        );
        let no_fill = attr(path, "fill") == Some("none");
        let point = |e: &Element| -> Option<(f64, f64)> {
            let pt = e.elements().find(|c| c.local == "pt")?;
            Some((number(pt, "x")? * sx, number(pt, "y")? * sy))
        };
        let mut current: Vec<(f64, f64)> = Vec::new();
        let mut finish = |current: &mut Vec<(f64, f64)>, closed: bool| {
            if current.len() >= 2 {
                out.push(Path {
                    points: std::mem::take(current),
                    closed: closed && !no_fill,
                });
            } else {
                current.clear();
            }
        };
        for command in path.elements() {
            match command.local.as_str() {
                "moveTo" => {
                    finish(&mut current, false);
                    current.extend(point(command));
                }
                "lnTo" => current.extend(point(command)),
                "cubicBezTo" | "quadBezTo" => {
                    let points: Vec<(f64, f64)> = command
                        .elements()
                        .filter(|c| c.local == "pt")
                        .filter_map(|pt| Some((number(pt, "x")? * sx, number(pt, "y")? * sy)))
                        .collect();
                    let start = current.last().copied().unwrap_or((0.0, 0.0));
                    bezier(&mut current, start, &points);
                }
                "arcTo" => {
                    let (Some(wr), Some(hr), Some(st), Some(sw)) = (
                        number(command, "wR"),
                        number(command, "hR"),
                        number(command, "stAng"),
                        number(command, "swAng"),
                    ) else {
                        continue;
                    };
                    let (wr, hr) = (wr * sx, hr * sy);
                    let (st, sw) = (angle(st), angle(sw));
                    let (px, py) = current.last().copied().unwrap_or((0.0, 0.0));
                    let center = (px - wr * st.cos(), py - hr * st.sin());
                    arc(&mut current, center, (wr, hr), st, sw);
                }
                "close" => finish(&mut current, true),
                _ => {}
            }
        }
        finish(&mut current, false);
    }
    out
}

/// Кривая Безье от `start` через точки `control` (2 — квадратичная,
/// 3 — кубическая), разбитая на отрезки.
fn bezier(out: &mut Vec<(f64, f64)>, start: (f64, f64), control: &[(f64, f64)]) {
    const STEPS: usize = 16;
    let lerp =
        |a: (f64, f64), b: (f64, f64), t: f64| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
    let mut points = vec![start];
    points.extend_from_slice(control);
    if points.len() < 2 {
        return;
    }
    for i in 1..=STEPS {
        let t = i as f64 / STEPS as f64;
        // Алгоритм де Кастельжо.
        let mut level = points.clone();
        while level.len() > 1 {
            level = level.windows(2).map(|p| lerp(p[0], p[1], t)).collect();
        }
        out.extend(level.first().copied());
    }
}

/// Заливка из `spPr` или `a:ln`.
enum Paint {
    /// Задана: цвет или «без заливки».
    Set(Option<Rgba>),
    /// Не задана — берётся из стиля фигуры.
    Unset,
}

fn read_fill(parent: &Element, colors: &Colors, placeholder: Option<Rgba>) -> Paint {
    for child in parent.elements() {
        match child.local.as_str() {
            "noFill" => return Paint::Set(None),
            "solidFill" => return Paint::Set(read_color(child, colors, placeholder)),
            "gradFill" => {
                // Градиент — пока средним цветом первой и последней точки.
                let stops: Vec<Rgba> = child
                    .elements()
                    .filter(|e| e.local == "gsLst")
                    .flat_map(Element::elements)
                    .filter_map(|gs| read_color(gs, colors, placeholder))
                    .collect();
                let color = match (stops.first(), stops.last()) {
                    (Some(a), Some(b)) => Some(std::array::from_fn(|i| {
                        ((u16::from(a[i]) + u16::from(b[i])) / 2) as u8
                    })),
                    _ => placeholder,
                };
                return Paint::Set(color);
            }
            "pattFill" => {
                let fg = child.elements().find(|e| e.local == "fgClr");
                return Paint::Set(fg.and_then(|fg| read_color(fg, colors, placeholder)));
            }
            "blipFill" | "grpFill" => return Paint::Set(placeholder),
            _ => {}
        }
    }
    Paint::Unset
}

/// Цвет из первого элемента цвета внутри `parent` (`a:srgbClr`,
/// `a:schemeClr` и т. д.) с его изменениями (`a:lumMod`, `a:alpha`…).
/// `placeholder` — цвет для `schemeClr val="phClr"`.
fn read_color(parent: &Element, colors: &Colors, placeholder: Option<Rgba>) -> Option<Rgba> {
    let element = parent.elements().find(|e| {
        matches!(
            e.local.as_str(),
            "srgbClr" | "schemeClr" | "sysClr" | "prstClr" | "scrgbClr" | "hslClr"
        )
    })?;
    let base: Rgba = match element.local.as_str() {
        "srgbClr" => with_alpha(hex(attr(element, "val")?)?),
        "sysClr" => with_alpha(hex(attr(element, "lastClr").unwrap_or("000000"))?),
        "schemeClr" => match attr(element, "val")? {
            "phClr" => placeholder?,
            name => with_alpha(colors.get(name)?),
        },
        "prstClr" => with_alpha(preset_color(attr(element, "val")?)?),
        "scrgbClr" => {
            let part = |name: &str| -> Option<u8> {
                let linear = attr(element, name)?.parse::<f64>().ok()? / 100_000.0;
                Some(to_byte(linear_to_srgb(linear)))
            };
            [part("r")?, part("g")?, part("b")?, 255]
        }
        "hslClr" => {
            let value = |name: &str| attr(element, name).and_then(|v| v.parse::<f64>().ok());
            let (r, g, b) = hsl_to_rgb(
                (value("hue")? / 60_000.0) / 360.0,
                value("sat")? / 100_000.0,
                value("lum")? / 100_000.0,
            );
            [to_byte(r), to_byte(g), to_byte(b), 255]
        }
        _ => return None,
    };
    Some(transform(base, element))
}

fn with_alpha([r, g, b]: [u8; 3]) -> Rgba {
    [r, g, b, 255]
}

/// Изменения цвета: яркость, насыщенность, оттенок, осветление,
/// затемнение, прозрачность.
fn transform(color: Rgba, element: &Element) -> Rgba {
    let [r, g, b, a] = color;
    let mut rgb = [r, g, b].map(|c| f64::from(c) / 255.0);
    let mut alpha = f64::from(a) / 255.0;
    for change in element.elements() {
        let Some(value) = attr(change, "val").and_then(|v| v.parse::<f64>().ok()) else {
            continue;
        };
        let k = value / 100_000.0;
        match change.local.as_str() {
            "lumMod" | "lumOff" | "satMod" | "satOff" | "hueOff" | "hueMod" | "lum" | "sat"
            | "hue" => {
                let (mut h, mut s, mut l) = rgb_to_hsl(rgb[0], rgb[1], rgb[2]);
                match change.local.as_str() {
                    "lumMod" => l *= k,
                    "lumOff" => l += k,
                    "lum" => l = k,
                    "satMod" => s *= k,
                    "satOff" => s += k,
                    "sat" => s = k,
                    "hueOff" => h += value / 60_000.0 / 360.0,
                    "hueMod" => h *= k,
                    _ => h = value / 60_000.0 / 360.0,
                }
                let (r, g, b) = hsl_to_rgb(h.rem_euclid(1.0), s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
                rgb = [r, g, b];
            }
            "tint" => {
                rgb = rgb.map(|c| linear_to_srgb(1.0 - (1.0 - srgb_to_linear(c)) * k));
            }
            "shade" => rgb = rgb.map(|c| linear_to_srgb(srgb_to_linear(c) * k)),
            "alpha" => alpha = k,
            "alphaMod" => alpha *= k,
            "alphaOff" => alpha += k,
            _ => {}
        }
    }
    let [r, g, b] = rgb.map(to_byte);
    [r, g, b, to_byte(alpha)]
}

fn to_byte(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f64) -> f64 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn rgb_to_hsl(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0.0, 0.0, l);
    }
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h, s, l)
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    if s == 0.0 {
        return (l, l, l);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let channel = |t: f64| {
        let t = t.rem_euclid(1.0);
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
    (channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0))
}

fn preset_color(name: &str) -> Option<[u8; 3]> {
    Some(match name {
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" => [0, 128, 0],
        "blue" => [0, 0, 255],
        "yellow" => [255, 255, 0],
        "gray" | "grey" => [128, 128, 128],
        "orange" => [255, 165, 0],
        _ => return None,
    })
}

/// Цвет `RRGGBB`.
fn hex(value: &str) -> Option<[u8; 3]> {
    if value.len() != 6 || !value.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&value[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// Текст фигуры (`dsp:txBody`) в области `frame` (`dsp:txXfrm`).
fn read_text(
    body: &Element,
    frame: &Frame,
    style: &StyleRefs,
    context: &Context,
) -> Option<ShapeText> {
    let props = body.elements().find(|e| e.local == "bodyPr");
    let inset = |name: &str, default: f64| {
        props
            .and_then(|p| attr(p, name))
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| (0.0..=MAX_EMU).contains(v))
            .unwrap_or(default)
    };
    let (left, top) = (inset("lIns", 91_440.0), inset("tIns", 45_720.0));
    let (right, bottom) = (inset("rIns", 91_440.0), inset("bIns", 45_720.0));
    let anchor = match props.and_then(|p| attr(p, "anchor")) {
        Some("t") => TextAnchor::Top,
        Some("b") => TextAnchor::Bottom,
        _ => TextAnchor::Middle,
    };
    let default_color = style
        .font
        .as_ref()
        .and_then(|(_, c)| *c)
        .unwrap_or([0, 0, 0, 255]);
    let default_font = style.font.as_ref().and_then(|(idx, _)| {
        let set = if idx == "major" {
            &context.theme.major
        } else {
            &context.theme.minor
        };
        set.latin.clone()
    });
    let mut paragraphs = Vec::new();
    for p in body.elements().filter(|e| e.local == "p") {
        let ppr = p.elements().find(|e| e.local == "pPr");
        let align = match ppr.and_then(|e| attr(e, "algn")) {
            Some("l" | "just" | "justLow" | "thaiDist") => TextAlign::Left,
            Some("r") => TextAlign::Right,
            Some(_) => TextAlign::Center,
            None => TextAlign::Left,
        };
        let bullet = ppr
            .and_then(|e| e.elements().find(|c| c.local == "buChar"))
            .and_then(|b| attr(b, "char"))
            .map(|c| format!("{c} "));
        let mut runs: Vec<ShapeRun> = Vec::new();
        let run_props = |rpr: Option<&Element>| -> ShapeRun {
            let size = rpr
                .and_then(|r| attr(r, "sz"))
                .and_then(|v| v.parse::<u32>().ok())
                .filter(|v| (100..=400_000).contains(v))
                .unwrap_or(1_800);
            let flag = |name: &str| matches!(rpr.and_then(|r| attr(r, name)), Some("1" | "true"));
            let color = rpr
                .and_then(
                    |r| match read_fill(r, context.colors, Some(default_color)) {
                        Paint::Set(c) => c,
                        Paint::Unset => None,
                    },
                )
                .unwrap_or(default_color);
            let font = rpr
                .and_then(|r| r.elements().find(|e| e.local == "latin"))
                .and_then(|l| attr(l, "typeface"))
                .and_then(|face| match face {
                    f if f.starts_with("+mj") => context.theme.major.latin.clone(),
                    f if f.starts_with("+mn") => context.theme.minor.latin.clone(),
                    "" => None,
                    f => Some(f.to_string()),
                })
                .or_else(|| default_font.clone());
            ShapeRun {
                text: String::new(),
                font,
                size,
                color,
                bold: flag("b"),
                italic: flag("i"),
            }
        };
        for item in p.elements() {
            match item.local.as_str() {
                "r" | "fld" => {
                    let mut run = run_props(item.elements().find(|e| e.local == "rPr"));
                    run.text = item
                        .elements()
                        .filter(|e| e.local == "t")
                        .map(Element::text)
                        .collect();
                    if !run.text.is_empty() {
                        runs.push(run);
                    }
                }
                "br" => {
                    let mut run = run_props(item.elements().find(|e| e.local == "rPr"));
                    run.text = "\n".into();
                    runs.push(run);
                }
                _ => {}
            }
        }
        let end = run_props(p.elements().find(|e| e.local == "endParaRPr"));
        if let (Some(bullet), Some(first)) = (bullet, runs.first()) {
            let mut mark = first.clone();
            mark.text = bullet;
            runs.insert(0, mark);
        }
        paragraphs.push(ShapeParagraph {
            align,
            size: runs.first().map_or(end.size, |r| r.size),
            runs,
        });
    }
    if paragraphs.iter().all(|p| p.runs.is_empty()) {
        return None;
    }
    // Поворот области текста не учитывается: текст пишется
    // горизонтально в её рамке.
    let x = frame.x + left;
    let y = frame.y + top;
    Some(ShapeText {
        x: x.round() as i64,
        y: y.round() as i64,
        width: (frame.width - left - right).max(0.0).round() as i64,
        height: (frame.height - top - bottom).max(0.0).round() as i64,
        anchor,
        paragraphs,
    })
}

/// Атрибут по имени без префикса.
fn attr<'a>(element: &'a Element, local: &str) -> Option<&'a str> {
    element
        .attrs
        .iter()
        .find(|a| a.local == local)
        .map(|a| a.value.as_str())
}

/// Первый элемент с именем `local` внутри (в глубину).
fn find<'a>(element: &'a Element, local: &str) -> Option<&'a Element> {
    for child in element.elements() {
        if child.local == local {
            return Some(child);
        }
        if let Some(found) = find(child, local) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(xml: &str) -> Element {
        crate::xml::parse(xml.as_bytes()).expect("xml")
    }

    #[test]
    fn theme_colors_with_changes() {
        let colors = Colors(vec![("accent1".into(), [0x44, 0x72, 0xC4])]);
        let fill = element(
            r#"<a:solidFill xmlns:a="a"><a:schemeClr val="accent1"><a:lumMod val="50000"/><a:alpha val="50000"/></a:schemeClr></a:solidFill>"#,
        );
        let color = read_color(&fill, &colors, None).expect("color");
        // Вдвое темнее и полупрозрачный.
        assert!(color[2] < 0xC4 && color[2] > 0x40, "{color:?}");
        assert_eq!(color[3], 128);
        let tx = element(r#"<a:x xmlns:a="a"><a:schemeClr val="tx1"/></a:x>"#);
        assert_eq!(read_color(&tx, &colors, None), Some([0, 0, 0, 255]));
        let ph = element(r#"<a:x xmlns:a="a"><a:schemeClr val="phClr"/></a:x>"#);
        assert_eq!(
            read_color(&ph, &colors, Some([1, 2, 3, 4])),
            Some([1, 2, 3, 4])
        );
    }

    #[test]
    fn shapes_are_turned_and_placed() {
        let frame = Frame {
            x: 100.0,
            y: 0.0,
            width: 200.0,
            height: 100.0,
            rotation: PI / 2.0,
            flip_h: false,
            flip_v: false,
        };
        // Поворот на 90° по часовой стрелке вокруг середины (200, 50):
        // левый верхний угол уходит вправо вверх.
        assert_eq!(frame.place((0.0, 0.0)), (250, -50));
        let chevron = preset_geometry("chevron", 200.0, 100.0, &[]);
        assert_eq!(chevron[0].points.len(), 6);
        assert_eq!(chevron[0].points[5], (50.0, 50.0));
        let unknown = preset_geometry("cloud", 10.0, 10.0, &[]);
        assert_eq!(unknown[0].points.len(), 4);
    }
}
