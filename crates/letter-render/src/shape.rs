//! Фигуры: из описания фигуры ([`Shape`]) получаются линии ([`Path`])
//! в пикселях — с заливкой, обводкой и стрелками. Их рисуют все виды
//! вывода: картинка в памяти и экран — через растеризатор `zeno`
//! ([`paint`]), PDF — своими командами линий.

use std::f32::consts::PI;

use letter_model::{
    ColorScheme, EMU_PER_POINT, Geometry, LineDash, LineEnd, PathCommand, Shape, ShapeLine,
};
use swash::zeno::{self, Command, Mask, Vector};

use crate::image::RgbaImage;
use crate::scene::Rgba;

/// Кусок линии. Координаты — пиксели.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    /// Начать новую линию в точке.
    MoveTo([f32; 2]),
    /// Прямая до точки.
    LineTo([f32; 2]),
    /// Кривая Безье: две опорные точки и конец.
    CubicTo([f32; 2], [f32; 2], [f32; 2]),
    /// Замкнуть линию.
    Close,
}

/// Обводка линии.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    /// Цвет.
    pub color: Rgba,
    /// Толщина, пиксели.
    pub width: f32,
    /// Пунктир: длины штрихов и промежутков по очереди; пусто — сплошная.
    pub dash: Vec<f32>,
}

/// Линия фигуры с заливкой и (или) обводкой.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    /// Куски линии.
    pub segments: Vec<Segment>,
    /// Заливка; `None` — без заливки.
    pub fill: Option<Rgba>,
    /// Обводка; `None` — без обводки.
    pub stroke: Option<Stroke>,
}

/// Аффинное преобразование: `x' = a·x + c·y + e`, `y' = b·x + d·y + f`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Affine([f32; 6]);

impl Affine {
    const IDENTITY: Affine = Affine([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    fn translate(x: f32, y: f32) -> Affine {
        Affine([1.0, 0.0, 0.0, 1.0, x, y])
    }

    fn apply(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        let [a, b, c, d, e, f] = self.0;
        [a * x + c * y + e, b * x + d * y + f]
    }

    /// Сначала `inner`, потом `self`.
    fn then(&self, inner: &Affine) -> Affine {
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = inner.0;
        Affine([
            a * a2 + c * b2,
            b * a2 + d * b2,
            a * c2 + c * d2,
            b * c2 + d * d2,
            a * e2 + c * f2 + e,
            b * e2 + d * f2 + f,
        ])
    }
}

/// Линии фигуры `shape` в прямоугольнике `x`, `y`, `width` × `height`
/// пикселей при масштабе `scale` пикселей на пункт. Цвета темы берутся
/// из темы Office (настоящие цвета документа подставляются при чтении).
pub fn shape_paths(
    shape: &Shape,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    scale: f32,
) -> Vec<Path> {
    let mut out = Vec::new();
    let scheme = ColorScheme::default();
    let emu = |px: f32| px / scale.max(f32::EPSILON) * EMU_PER_POINT as f32;
    let size = (emu(width), emu(height));
    add_shape(
        &mut out,
        shape,
        [x, y, width, height],
        size,
        &Affine::IDENTITY,
        scale,
        &scheme,
        0,
    );
    out
}

/// Самая глубокая вложенность групп.
const MAX_DEPTH: usize = 16;

#[allow(clippy::too_many_arguments)]
fn add_shape(
    out: &mut Vec<Path>,
    shape: &Shape,
    [x, y, w, h]: [f32; 4],
    emu_size: (f32, f32),
    parent: &Affine,
    scale: f32,
    scheme: &ColorScheme,
    depth: usize,
) {
    if !(w.is_finite() && h.is_finite() && w >= 0.0 && h >= 0.0) || depth > MAX_DEPTH {
        return;
    }
    // Отражение и поворот — вокруг середины фигуры.
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (sin, cos) = (shape.rotation as f32 / 60_000.0 * PI / 180.0).sin_cos();
    let fx = if shape.flip_horizontal { -1.0 } else { 1.0 };
    let fy = if shape.flip_vertical { -1.0 } else { 1.0 };
    let local = Affine::translate(x + cx, y + cy)
        .then(&Affine([cos, sin, -sin, cos, 0.0, 0.0]))
        .then(&Affine([fx, 0.0, 0.0, fy, 0.0, 0.0]))
        .then(&Affine::translate(-cx, -cy));
    let transform = parent.then(&local);
    match &shape.geometry {
        Geometry::Group(group) => {
            let frame = group.frame;
            let sx = if frame.width > 0 {
                w / frame.width as f32
            } else {
                0.0
            };
            let sy = if frame.height > 0 {
                h / frame.height as f32
            } else {
                0.0
            };
            for child in &group.children {
                let f = child.frame;
                let rect = [
                    (f.x - frame.x) as f32 * sx,
                    (f.y - frame.y) as f32 * sy,
                    f.width as f32 * sx,
                    f.height as f32 * sy,
                ];
                let size = (f.width as f32, f.height as f32);
                add_shape(
                    out,
                    &child.shape,
                    rect,
                    size,
                    &transform,
                    scale,
                    scheme,
                    depth + 1,
                );
            }
        }
        geometry => {
            let fill = shape.fill.as_ref().map(|c| c.rgba(scheme));
            let line = shape.line.as_ref();
            let stroke = line.map(|l| stroke(l, scale, scheme));
            for outline in outlines(geometry, w, h, emu_size) {
                let open = !outline.segments.contains(&Segment::Close);
                let mut segments = outline.segments;
                let mut heads = Vec::new();
                if let (Some(line), Some(s), true, true) = (line, &stroke, open, outline.stroke) {
                    heads = arrowheads(&mut segments, line, s);
                }
                let segments: Vec<Segment> = segments.iter().map(|s| map(s, &transform)).collect();
                out.push(Path {
                    segments,
                    fill: fill.filter(|_| outline.fill),
                    stroke: stroke.clone().filter(|_| outline.stroke),
                });
                for (segments, filled) in heads {
                    let Some(s) = &stroke else { continue };
                    let segments = segments.iter().map(|p| map(p, &transform)).collect();
                    out.push(Path {
                        segments,
                        fill: filled.then_some(s.color),
                        stroke: (!filled).then(|| Stroke {
                            dash: Vec::new(),
                            ..s.clone()
                        }),
                    });
                }
            }
        }
    }
}

fn map(segment: &Segment, t: &Affine) -> Segment {
    match *segment {
        Segment::MoveTo(p) => Segment::MoveTo(t.apply(p)),
        Segment::LineTo(p) => Segment::LineTo(t.apply(p)),
        Segment::CubicTo(a, b, c) => Segment::CubicTo(t.apply(a), t.apply(b), t.apply(c)),
        Segment::Close => Segment::Close,
    }
}

fn stroke(line: &ShapeLine, scale: f32, scheme: &ColorScheme) -> Stroke {
    // Тоньше пикселя линия не рисуется: иначе она пропадает.
    let width = (line.width as f32 / EMU_PER_POINT as f32 * scale).max(1.0);
    let dash = match line.dash {
        LineDash::Solid => Vec::new(),
        LineDash::Dot => vec![width, width],
        LineDash::Dash => vec![4.0 * width, 3.0 * width],
        LineDash::DashDot => vec![4.0 * width, 3.0 * width, width, 3.0 * width],
    };
    Stroke {
        color: line.color.rgba(scheme),
        width,
        dash,
    }
}

/// Контур без заливки и обводки (что из них рисовать, решает фигура).
struct Outline {
    segments: Vec<Segment>,
    fill: bool,
    stroke: bool,
}

impl Outline {
    fn closed(segments: Vec<Segment>) -> Outline {
        Outline {
            segments,
            fill: true,
            stroke: true,
        }
    }

    fn open(segments: Vec<Segment>) -> Outline {
        Outline {
            segments,
            fill: false,
            stroke: true,
        }
    }
}

/// Многоугольник по точкам.
fn polygon(points: &[[f32; 2]]) -> Vec<Segment> {
    let mut out = Vec::with_capacity(points.len() + 1);
    for (i, p) in points.iter().enumerate() {
        out.push(if i == 0 {
            Segment::MoveTo(*p)
        } else {
            Segment::LineTo(*p)
        });
    }
    out.push(Segment::Close);
    out
}

/// Ломаная по точкам (не замкнутая).
fn polyline(points: &[[f32; 2]]) -> Vec<Segment> {
    let mut out = polygon(points);
    out.pop();
    out
}

/// Дуга эллипса с центром `c` и радиусами `r` от угла `start` с размахом
/// `sweep` (радианы, по часовой стрелке при оси y вниз) — кривыми Безье
/// не больше четверти круга.
fn arc(out: &mut Vec<Segment>, c: [f32; 2], r: [f32; 2], start: f32, sweep: f32) {
    let pieces = (sweep.abs() / (PI / 2.0)).ceil().max(1.0) as usize;
    let step = sweep / pieces as f32;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let point = |a: f32| [c[0] + r[0] * a.cos(), c[1] + r[1] * a.sin()];
    let tangent = |a: f32| [-r[0] * a.sin(), r[1] * a.cos()];
    for i in 0..pieces {
        let a0 = start + step * i as f32;
        let a1 = a0 + step;
        let (p0, p1) = (point(a0), point(a1));
        let (t0, t1) = (tangent(a0), tangent(a1));
        out.push(Segment::CubicTo(
            [p0[0] + k * t0[0], p0[1] + k * t0[1]],
            [p1[0] - k * t1[0], p1[1] - k * t1[1]],
            p1,
        ));
    }
}

fn ellipse(x: f32, y: f32, w: f32, h: f32) -> Vec<Segment> {
    let (rx, ry) = (w / 2.0, h / 2.0);
    let c = [x + rx, y + ry];
    let mut out = vec![Segment::MoveTo([c[0] + rx, c[1]])];
    arc(&mut out, c, [rx, ry], 0.0, 2.0 * PI);
    out.push(Segment::Close);
    out
}

fn round_rect(w: f32, h: f32, r: f32) -> Vec<Segment> {
    let r = r.clamp(0.0, w.min(h) / 2.0);
    let mut out = vec![Segment::MoveTo([r, 0.0]), Segment::LineTo([w - r, 0.0])];
    arc(&mut out, [w - r, r], [r, r], -PI / 2.0, PI / 2.0);
    out.push(Segment::LineTo([w, h - r]));
    arc(&mut out, [w - r, h - r], [r, r], 0.0, PI / 2.0);
    out.push(Segment::LineTo([r, h]));
    arc(&mut out, [r, h - r], [r, r], PI / 2.0, PI / 2.0);
    out.push(Segment::LineTo([0.0, r]));
    arc(&mut out, [r, r], [r, r], PI, PI / 2.0);
    out.push(Segment::Close);
    out
}

/// Звезда с `n` лучами: внутренний радиус — доля `inner` от внешнего.
fn star(w: f32, h: f32, n: usize, inner: f32) -> Vec<Segment> {
    let points: Vec<[f32; 2]> = (0..2 * n)
        .map(|i| {
            let a = -PI / 2.0 + PI * i as f32 / n as f32;
            let k = if i % 2 == 0 { 1.0 } else { inner };
            [
                w / 2.0 + w / 2.0 * k * a.cos(),
                h / 2.0 + h / 2.0 * k * a.sin(),
            ]
        })
        .collect();
    polygon(&points)
}

/// Контуры фигуры в прямоугольнике `w` × `h` пикселей от точки (0, 0).
/// `emu_size` — размер фигуры в EMU: в нём заданы точки своего контура
/// без собственной системы координат.
fn outlines(geometry: &Geometry, w: f32, h: f32, emu_size: (f32, f32)) -> Vec<Outline> {
    match geometry {
        Geometry::Preset { name, adjust } => {
            let adj = |key: &str, default: f32| {
                adjust
                    .iter()
                    .find(|(n, _)| n == key || (key == "adj1" && n == "adj"))
                    .map_or(default, |(_, v)| *v as f32 / 100_000.0)
            };
            vec![preset(name, w, h, &adj)]
        }
        Geometry::Custom(paths) => paths
            .iter()
            .map(|path| {
                let pw = if path.width > 0 {
                    path.width as f32
                } else {
                    emu_size.0
                };
                let ph = if path.height > 0 {
                    path.height as f32
                } else {
                    emu_size.1
                };
                let sx = if pw > 0.0 { w / pw } else { 0.0 };
                let sy = if ph > 0.0 { h / ph } else { 0.0 };
                Outline {
                    segments: custom(&path.commands, sx, sy),
                    fill: path.fill,
                    stroke: path.stroke,
                }
            })
            .collect(),
        Geometry::Group(_) => Vec::new(),
    }
}

/// Свой контур: команды файла в пикселях (`sx`, `sy` — пикселей на
/// единицу координат линии).
fn custom(commands: &[PathCommand], sx: f32, sy: f32) -> Vec<Segment> {
    let p = |(x, y): (i64, i64)| [x as f32 * sx, y as f32 * sy];
    let mut out = Vec::new();
    let mut current = [0.0, 0.0];
    let mut start = current;
    for command in commands {
        match *command {
            PathCommand::MoveTo(x, y) => {
                current = p((x, y));
                start = current;
                out.push(Segment::MoveTo(current));
            }
            PathCommand::LineTo(x, y) => {
                current = p((x, y));
                out.push(Segment::LineTo(current));
            }
            PathCommand::CubicTo([a, b, c]) => {
                current = p(c);
                out.push(Segment::CubicTo(p(a), p(b), current));
            }
            PathCommand::QuadTo([a, b]) => {
                let (q, end) = (p(a), p(b));
                let c1 = [
                    current[0] + 2.0 / 3.0 * (q[0] - current[0]),
                    current[1] + 2.0 / 3.0 * (q[1] - current[1]),
                ];
                let c2 = [
                    end[0] + 2.0 / 3.0 * (q[0] - end[0]),
                    end[1] + 2.0 / 3.0 * (q[1] - end[1]),
                ];
                out.push(Segment::CubicTo(c1, c2, end));
                current = end;
            }
            PathCommand::ArcTo {
                width_radius,
                height_radius,
                start: angle,
                swing,
            } => {
                let (rx, ry) = (width_radius as f32 * sx, height_radius as f32 * sy);
                let a = angle as f32 / 60_000.0 * PI / 180.0;
                let sweep = swing as f32 / 60_000.0 * PI / 180.0;
                let c = [current[0] - rx * a.cos(), current[1] - ry * a.sin()];
                if out.is_empty() {
                    out.push(Segment::MoveTo(current));
                }
                arc(&mut out, [c[0], c[1]], [rx, ry], a, sweep);
                let end = a + sweep;
                current = [c[0] + rx * end.cos(), c[1] + ry * end.sin()];
            }
            PathCommand::Close => {
                out.push(Segment::Close);
                current = start;
            }
        }
    }
    out
}

/// Готовая фигура Word. Неизвестная фигура рисуется прямоугольником.
fn preset(name: &str, w: f32, h: f32, adj: &dyn Fn(&str, f32) -> f32) -> Outline {
    let ss = w.min(h);
    let closed = |points: &[[f32; 2]]| Outline::closed(polygon(points));
    match name {
        "line" | "straightConnector1" => Outline::open(polyline(&[[0.0, 0.0], [w, h]])),
        "bentConnector2" => Outline::open(polyline(&[[0.0, 0.0], [w, 0.0], [w, h]])),
        "bentConnector3" => {
            let x = w * adj("adj1", 0.5);
            Outline::open(polyline(&[[0.0, 0.0], [x, 0.0], [x, h], [w, h]]))
        }
        "curvedConnector3" => Outline::open(vec![
            Segment::MoveTo([0.0, 0.0]),
            Segment::CubicTo([w / 2.0, 0.0], [w / 2.0, h], [w, h]),
        ]),
        "ellipse" | "flowChartConnector" => Outline::closed(ellipse(0.0, 0.0, w, h)),
        "roundRect" | "flowChartAlternateProcess" => {
            Outline::closed(round_rect(w, h, ss * adj("adj1", 0.166_67)))
        }
        "flowChartTerminator" => Outline::closed(round_rect(w, h, ss / 2.0)),
        "triangle" | "flowChartExtract" => closed(&[[w * adj("adj1", 0.5), 0.0], [w, h], [0.0, h]]),
        "rtTriangle" => closed(&[[0.0, 0.0], [w, h], [0.0, h]]),
        "diamond" | "flowChartDecision" => {
            closed(&[[w / 2.0, 0.0], [w, h / 2.0], [w / 2.0, h], [0.0, h / 2.0]])
        }
        "parallelogram" | "flowChartInputOutput" => {
            let x = if name == "parallelogram" {
                ss * adj("adj1", 0.25)
            } else {
                w / 5.0
            };
            closed(&[[x, 0.0], [w, 0.0], [w - x, h], [0.0, h]])
        }
        "trapezoid" => {
            let x = ss * adj("adj1", 0.25);
            closed(&[[x, 0.0], [w - x, 0.0], [w, h], [0.0, h]])
        }
        "pentagon" | "homePlate" if name == "homePlate" => {
            let x = ss * adj("adj1", 0.5);
            closed(&[[0.0, 0.0], [w - x, 0.0], [w, h / 2.0], [w - x, h], [0.0, h]])
        }
        "pentagon" => closed(&[
            [w / 2.0, 0.0],
            [w, h * 0.382],
            [w * 0.809, h],
            [w * 0.191, h],
            [0.0, h * 0.382],
        ]),
        "hexagon" => {
            let x = ss * adj("adj1", 0.25);
            closed(&[
                [x, 0.0],
                [w - x, 0.0],
                [w, h / 2.0],
                [w - x, h],
                [x, h],
                [0.0, h / 2.0],
            ])
        }
        "octagon" => {
            let x = ss * adj("adj1", 0.292_89);
            closed(&[
                [x, 0.0],
                [w - x, 0.0],
                [w, x],
                [w, h - x],
                [w - x, h],
                [x, h],
                [0.0, h - x],
                [0.0, x],
            ])
        }
        "chevron" => {
            let x = ss * adj("adj1", 0.5);
            closed(&[
                [0.0, 0.0],
                [w - x, 0.0],
                [w, h / 2.0],
                [w - x, h],
                [0.0, h],
                [x, h / 2.0],
            ])
        }
        "plus" => {
            let x = ss * adj("adj1", 0.25);
            closed(&[
                [x, 0.0],
                [w - x, 0.0],
                [w - x, x],
                [w, x],
                [w, h - x],
                [w - x, h - x],
                [w - x, h],
                [x, h],
                [x, h - x],
                [0.0, h - x],
                [0.0, x],
                [x, x],
            ])
        }
        "star4" => Outline::closed(star(w, h, 4, adj("adj1", 0.125) * 2.0)),
        "star5" => Outline::closed(star(w, h, 5, adj("adj1", 0.190_98) * 2.0)),
        "star6" => Outline::closed(star(w, h, 6, adj("adj1", 0.288_68) * 2.0)),
        "rightArrow" | "leftArrow" | "leftRightArrow" => {
            let shaft = h * adj("adj1", 0.5);
            let head = (ss * adj("adj2", 0.5)).min(w / 2.0);
            let (y1, y2) = ((h - shaft) / 2.0, (h + shaft) / 2.0);
            let right = name != "leftArrow";
            let left = name != "rightArrow";
            let (x1, x2) = (
                if left { head } else { 0.0 },
                if right { w - head } else { w },
            );
            let mut points = Vec::new();
            if left {
                points.extend([[0.0, h / 2.0], [x1, 0.0], [x1, y1]]);
            } else {
                points.push([0.0, y1]);
            }
            if right {
                points.extend([[x2, y1], [x2, 0.0], [w, h / 2.0], [x2, h], [x2, y2]]);
            } else {
                points.extend([[w, y1], [w, y2]]);
            }
            if left {
                points.extend([[x1, y2], [x1, h]]);
            } else {
                points.push([0.0, y2]);
            }
            closed(&points)
        }
        "upArrow" | "downArrow" | "upDownArrow" => {
            // Та же стрелка, повёрнутая набок.
            let horizontal = match name {
                "upArrow" => "leftArrow",
                "downArrow" => "rightArrow",
                _ => "leftRightArrow",
            };
            let mut outline = preset(horizontal, h, w, adj);
            for s in &mut outline.segments {
                let swap = |[x, y]: [f32; 2]| [y, x];
                *s = match *s {
                    Segment::MoveTo(p) => Segment::MoveTo(swap(p)),
                    Segment::LineTo(p) => Segment::LineTo(swap(p)),
                    Segment::CubicTo(a, b, c) => Segment::CubicTo(swap(a), swap(b), swap(c)),
                    Segment::Close => Segment::Close,
                };
            }
            outline
        }
        "heart" => Outline::closed(vec![
            Segment::MoveTo([w / 2.0, h * 0.3]),
            Segment::CubicTo([w * 0.5, 0.0], [0.0, 0.0], [0.0, h * 0.3]),
            Segment::CubicTo([0.0, h * 0.6], [w * 0.5, h * 0.8], [w / 2.0, h]),
            Segment::CubicTo([w * 0.5, h * 0.8], [w, h * 0.6], [w, h * 0.3]),
            Segment::CubicTo([w, 0.0], [w * 0.5, 0.0], [w / 2.0, h * 0.3]),
            Segment::Close,
        ]),
        _ => closed(&[[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]]),
    }
}

/// Стрелки на концах открытой линии. Конец линии под закрашенной
/// стрелкой укорачивается, чтобы не торчать из её острия. Возвращает
/// контуры стрелок и признак «закрасить» (иначе — обвести).
fn arrowheads(
    segments: &mut [Segment],
    line: &ShapeLine,
    stroke: &Stroke,
) -> Vec<(Vec<Segment>, bool)> {
    let mut out = Vec::new();
    let size = 3.0 * stroke.width;
    let point = |s: &Segment| match *s {
        Segment::MoveTo(p) | Segment::LineTo(p) | Segment::CubicTo(_, _, p) => Some(p),
        Segment::Close => None,
    };
    let len = segments.len();
    if len < 2 {
        return out;
    }
    // Начало: точка и откуда к ней идёт линия.
    let head = (|| {
        let tip = point(&segments[0])?;
        let from = match segments[1] {
            Segment::CubicTo(a, _, _) => a,
            ref s => point(s)?,
        };
        Some((tip, from))
    })();
    let tail = (|| {
        let tip = point(&segments[len - 1])?;
        let from = match segments[len - 1] {
            Segment::CubicTo(_, b, _) => b,
            _ => point(&segments[len - 2])?,
        };
        Some((tip, from))
    })();
    for (end, place, index) in [(line.head, head, 0), (line.tail, tail, len - 1)] {
        let (Some((tip, from)), false) = (place, end == LineEnd::None) else {
            continue;
        };
        let (dx, dy) = (tip[0] - from[0], tip[1] - from[1]);
        let length = (dx * dx + dy * dy).sqrt();
        if length <= f32::EPSILON {
            continue;
        }
        let (ux, uy) = (dx / length, dy / length);
        let (nx, ny) = (-uy, ux);
        let at = |along: f32, across: f32| {
            [
                tip[0] - ux * along + nx * across,
                tip[1] - uy * along + ny * across,
            ]
        };
        let half = size / 2.0;
        let (shape, filled) = match end {
            LineEnd::None => continue,
            LineEnd::Triangle => (polygon(&[tip, at(size, half), at(size, -half)]), true),
            LineEnd::Stealth => (
                polygon(&[tip, at(size, half), at(size * 0.7, 0.0), at(size, -half)]),
                true,
            ),
            LineEnd::Arrow => (polyline(&[at(size, half), tip, at(size, -half)]), false),
            LineEnd::Diamond => (
                polygon(&[tip, at(half, half), at(size, 0.0), at(half, -half)]),
                true,
            ),
            LineEnd::Oval => {
                let c = at(half, 0.0);
                (ellipse(c[0] - half, c[1] - half, size, size), true)
            }
        };
        // Линия кончается в середине стрелки.
        if filled && length > half {
            let short = at(half, 0.0);
            segments[index] = match segments[index] {
                Segment::MoveTo(_) => Segment::MoveTo(short),
                Segment::LineTo(_) => Segment::LineTo(short),
                Segment::CubicTo(a, b, _) => Segment::CubicTo(a, b, short),
                Segment::Close => Segment::Close,
            };
        }
        out.push((shape, filled));
    }
    out
}

/// Самая большая сторона маски одной линии, пиксели.
const MAX_MASK: f32 = 16_384.0;

/// Рисует линии в картинку `target`, сдвинув их на `dx`, `dy` пикселей.
pub fn paint(target: &mut RgbaImage, paths: &[Path], dx: f32, dy: f32) {
    for path in paths {
        let commands: Vec<Command> = path
            .segments
            .iter()
            .map(|s| {
                let v = |[x, y]: [f32; 2]| Vector::new(x + dx, y + dy);
                match *s {
                    Segment::MoveTo(p) => Command::MoveTo(v(p)),
                    Segment::LineTo(p) => Command::LineTo(v(p)),
                    Segment::CubicTo(a, b, c) => Command::CurveTo(v(a), v(b), v(c)),
                    Segment::Close => Command::Close,
                }
            })
            .collect();
        if let Some(color) = path.fill {
            fill_mask(
                target,
                &commands,
                zeno::Style::Fill(zeno::Fill::NonZero),
                color,
            );
        }
        if let Some(stroke) = &path.stroke {
            let mut style = zeno::Stroke::new(stroke.width);
            if !stroke.dash.is_empty() {
                style.dash(&stroke.dash, 0.0);
            }
            fill_mask(target, &commands, zeno::Style::Stroke(style), stroke.color);
        }
    }
}

/// Растеризует линию стилем `style` в пределах картинки и смешивает
/// цвет с картинкой по полученной маске.
fn fill_mask(target: &mut RgbaImage, commands: &[Command], style: zeno::Style, color: Rgba) {
    let bounds = zeno::bounds(commands, style, None);
    let x0 = bounds.min.x.floor().max(0.0);
    let y0 = bounds.min.y.floor().max(0.0);
    let x1 = bounds.max.x.ceil().min(target.width as f32);
    let y1 = bounds.max.y.ceil().min(target.height as f32);
    if !(x1 > x0 && y1 > y0) || x1 - x0 > MAX_MASK || y1 - y0 > MAX_MASK {
        return;
    }
    let (w, h) = ((x1 - x0) as u32, (y1 - y0) as u32);
    let (mask, _) = Mask::new(commands)
        .style(style)
        .offset(Vector::new(-x0, -y0))
        .size(w, h)
        .render();
    for row in 0..h {
        for col in 0..w {
            let alpha = mask
                .get(row as usize * w as usize + col as usize)
                .copied()
                .unwrap_or(0);
            if alpha > 0 {
                over(target, col + x0 as u32, row + y0 as u32, color, alpha);
            }
        }
    }
}

/// Растровая картинка фигуры для экрана: сама картинка и где её левый
/// верхний угол относительно угла прямоугольника фигуры, пиксели.
/// `None`, если рисовать нечего.
pub fn rasterize(
    shape: &Shape,
    width: f32,
    height: f32,
    scale: f32,
) -> Option<(RgbaImage, [f32; 2])> {
    let mut paths = shape_paths(shape, 0.0, 0.0, width, height, scale);
    paths.retain(|p| p.fill.is_some() || p.stroke.is_some());
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for path in &paths {
        let pad = path.stroke.as_ref().map_or(0.0, |s| s.width) + 1.0;
        for s in &path.segments {
            let points: &[[f32; 2]] = match s {
                Segment::MoveTo(p) | Segment::LineTo(p) => std::slice::from_ref(p),
                Segment::CubicTo(a, b, c) => &[*a, *b, *c],
                Segment::Close => &[],
            };
            for p in points {
                for i in 0..2 {
                    min[i] = min[i].min(p[i] - pad);
                    max[i] = max[i].max(p[i] + pad);
                }
            }
        }
    }
    let (left, top) = (min[0].floor(), min[1].floor());
    let (w, h) = ((max[0] - left).ceil(), (max[1] - top).ceil());
    if !(w > 0.0 && h > 0.0) || w > MAX_MASK || h > MAX_MASK {
        return None;
    }
    let mut image = RgbaImage::filled(w as u32, h as u32, [0, 0, 0, 0]);
    paint(&mut image, &paths, -left, -top);
    Some((image, [left, top]))
}

/// Кладёт цвет с непрозрачностью `alpha` (0–255, вдобавок к своей)
/// поверх пикселя: и на белом листе, и на прозрачной картинке.
fn over(target: &mut RgbaImage, x: u32, y: u32, color: Rgba, alpha: u8) {
    let i = (y as usize * target.width as usize + x as usize) * 4;
    let Some(dst) = target.pixels.get_mut(i..i + 4) else {
        return;
    };
    let sa = f32::from(alpha) / 255.0 * f32::from(color[3]) / 255.0;
    if sa <= 0.0 {
        return;
    }
    let da = f32::from(dst[3]) / 255.0;
    let out = sa + da * (1.0 - sa);
    for c in 0..3 {
        let v = (f32::from(color[c]) * sa + f32::from(dst[c]) * da * (1.0 - sa)) / out;
        dst[c] = v.round() as u8;
    }
    dst[3] = (out * 255.0).round() as u8;
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::{Frame, Group, GroupChild, ShapeColor};

    fn filled(name: &str) -> Shape {
        Shape {
            geometry: Geometry::preset(name),
            fill: Some(ShapeColor::rgb([255, 192, 0])),
            ..Shape::default()
        }
    }

    fn draw(shape: &Shape) -> RgbaImage {
        let mut image = RgbaImage::filled(120, 60, [255; 4]);
        paint(
            &mut image,
            &shape_paths(shape, 10.0, 10.0, 100.0, 40.0, 1.0),
            0.0,
            0.0,
        );
        image
    }

    const YELLOW: Option<[u8; 4]> = Some([255, 192, 0, 255]);
    const WHITE: Option<[u8; 4]> = Some([255; 4]);

    #[test]
    fn arrow_has_a_shaft_and_a_head() {
        let image = draw(&filled("rightArrow"));
        // Древко в середине, над ним пусто; острие справа во всю высоту.
        assert_eq!(image.pixel(30, 30), YELLOW);
        assert_eq!(image.pixel(30, 14), WHITE);
        assert_eq!(image.pixel(92, 14), YELLOW);
        assert_eq!(image.pixel(107, 14), WHITE);
        // Повёрнутая на 180° стрелка смотрит влево.
        let left = draw(&Shape {
            rotation: 180 * 60_000,
            ..filled("rightArrow")
        });
        assert_eq!(left.pixel(28, 14), YELLOW);
        assert_eq!(left.pixel(80, 14), WHITE);
    }

    #[test]
    fn ellipse_and_rounded_corners() {
        let image = draw(&filled("ellipse"));
        assert_eq!(image.pixel(60, 30), YELLOW);
        assert_eq!(image.pixel(11, 11), WHITE);
        let image = draw(&filled("roundRect"));
        assert_eq!(image.pixel(10, 10), WHITE);
        assert_eq!(image.pixel(10, 11), WHITE);
        assert_eq!(image.pixel(11, 30), YELLOW);
    }

    #[test]
    fn group_children_are_scaled_into_the_group() {
        let group = Shape {
            geometry: Geometry::Group(Group {
                frame: Frame {
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 4,
                },
                children: vec![GroupChild {
                    frame: Frame {
                        x: 5,
                        y: 0,
                        width: 5,
                        height: 4,
                    },
                    shape: filled("rect"),
                }],
            }),
            ..Shape::default()
        };
        let image = draw(&group);
        assert_eq!(image.pixel(40, 30), WHITE);
        assert_eq!(image.pixel(80, 30), YELLOW);
    }

    #[test]
    fn line_with_arrowhead_and_screen_raster() {
        let line = Shape {
            geometry: Geometry::preset("straightConnector1"),
            line: Some(ShapeLine {
                color: ShapeColor::rgb([0, 0, 0]),
                width: 2 * EMU_PER_POINT,
                dash: LineDash::Solid,
                head: LineEnd::None,
                tail: LineEnd::Triangle,
            }),
            ..Shape::default()
        };
        let paths = shape_paths(&line, 0.0, 0.0, 100.0, 0.0, 1.0);
        // Линия и закрашенная стрелка.
        assert_eq!(paths.len(), 2);
        assert!(paths[1].fill.is_some());
        let (image, [left, top]) = rasterize(&line, 100.0, 0.0, 1.0).unwrap();
        assert!(left < 0.0 && top < 0.0);
        // Острие стрелки толще линии.
        let column = |x: u32| {
            (0..image.height)
                .filter(|&y| image.pixel(x, y).unwrap()[3] > 128)
                .count()
        };
        let x = |at: f32| (at - left) as u32;
        assert!(column(x(95.0)) > column(x(40.0)) + 1);
        assert!(rasterize(&Shape::default(), 0.0, 0.0, 1.0).is_none());
    }
}
