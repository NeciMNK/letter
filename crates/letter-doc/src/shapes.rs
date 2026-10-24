//! Фигуры OfficeArt (MS-ODRAW): вид (готовая фигура по номеру или свой
//! контур по точкам и формулам), заливка, линия, поворот, группы и
//! текст надписей. Из записей фигуры строится [`Shape`] модели.

use letter_model::{
    Block, ColorMod, DEFAULT_LINE_WIDTH, DEFAULT_TEXT_INSETS, Frame, Geometry, Group, GroupChild,
    LineDash, LineEnd, PathCommand, Shape, ShapeColor, ShapeLine, ShapePath, TextAnchor,
};

use crate::fib::{u16_at, u32_at};
use crate::pictures::{Record, records};

/// EMU в одном twip (1/20 пункта).
const EMU_PER_TWIP: i64 = 635;

/// Свойства-массивы (`IMsoArray`): точки контура, части контура,
/// точки соединения, ручки, формулы, область текста.
const ARRAYS: [u16; 7] = [0x0145, 0x0146, 0x0151, 0x0152, 0x0155, 0x0156, 0x0157];

/// Фигура из записей `OfficeArtSpContainer` (и группы
/// `OfficeArtSpgrContainer`).
#[derive(Debug, Clone, Default)]
pub(crate) struct ShapeRecord {
    /// Номер фигуры (`spid`).
    pub spid: u32,
    /// Вид фигуры (`MSOSPT`, вариант записи `FSP`): 1 — прямоугольник,
    /// 3 — овал… 0 и 4095 — свой контур.
    pub kind: u16,
    /// Признаки `FSP`: группа, отражения и т. д.
    pub flags: u32,
    /// Простые свойства: номер и значение.
    pub props: Vec<(u16, u32)>,
    /// Сложные свойства (строки, массивы): номер и байты.
    pub complex: Vec<(u16, Vec<u8>)>,
    /// Система координат группы (`FSPGR`): левый, верхний, правый,
    /// нижний края.
    pub group_frame: Option<[i32; 4]>,
    /// Место фигуры внутри группы (`OfficeArtChildAnchor`).
    pub child_anchor: Option<[i32; 4]>,
    /// Фигуры группы; `None` — не группа.
    pub children: Option<Vec<ShapeRecord>>,
}

impl ShapeRecord {
    /// Значение простого свойства.
    pub fn prop(&self, id: u16) -> Option<u32> {
        self.props.iter().find(|(p, _)| *p == id).map(|&(_, v)| v)
    }

    fn complex(&self, id: u16) -> Option<&[u8]> {
        self.complex
            .iter()
            .find(|(p, _)| *p == id)
            .map(|(_, v)| v.as_slice())
    }

    /// Номер текста надписи (`lTxid`), если у фигуры есть текст.
    fn has_text(&self) -> bool {
        self.prop(0x0080).is_some()
    }
}

/// Фигуры среди записей `bytes[from..to]`: фигуры и группы верхнего
/// уровня (служебная фигура-«основа» рисунка тоже попадает в список).
pub(crate) fn shapes_in(bytes: &[u8], from: usize, to: usize) -> Vec<ShapeRecord> {
    let mut out = Vec::new();
    for record in records(bytes, from, to) {
        match record.kind {
            0xF004 => out.extend(shape_record(bytes, &record)),
            0xF003 => out.extend(group_record(bytes, &record)),
            _ if record.version == 0xF => out.extend(shapes_in(bytes, record.start, record.end)),
            _ => {}
        }
    }
    out
}

/// Группа: первая фигура описывает саму группу, остальные — её состав.
/// У основы рисунка (`fPatriarch`) состав — просто фигуры верхнего уровня.
fn group_record(bytes: &[u8], record: &Record) -> Vec<ShapeRecord> {
    let mut items = Vec::new();
    for inner in records(bytes, record.start, record.end) {
        match inner.kind {
            0xF004 => items.extend(shape_record(bytes, &inner)),
            0xF003 => items.extend(group_record(bytes, &inner)),
            _ => {}
        }
    }
    match items.first() {
        Some(head) if head.flags & 0x1 != 0 && head.flags & 0x4 == 0 => {
            let mut head = items.remove(0);
            head.children = Some(items);
            vec![head]
        }
        _ => items,
    }
}

/// Одна фигура `OfficeArtSpContainer`.
fn shape_record(bytes: &[u8], record: &Record) -> Option<ShapeRecord> {
    let mut shape = ShapeRecord::default();
    let mut found = false;
    let rect = |r: &Record| -> Option<[i32; 4]> {
        let side = |i: usize| u32_at(bytes, r.start + i * 4).map(|v| v as i32);
        Some([side(0)?, side(1)?, side(2)?, side(3)?])
    };
    for inner in records(bytes, record.start, record.end) {
        match inner.kind {
            0xF00A => {
                shape.kind = inner.instance;
                shape.spid = u32_at(bytes, inner.start)?;
                shape.flags = u32_at(bytes, inner.start + 4).unwrap_or(0);
                found = true;
            }
            0xF009 => shape.group_frame = rect(&inner),
            0xF00F => shape.child_anchor = rect(&inner),
            0xF00B | 0xF121 | 0xF122 => options(bytes, &inner, &mut shape),
            _ => {}
        }
    }
    found.then_some(shape)
}

/// Таблица свойств (`OfficeArtFOPT`): записи по 6 байт (номер с флагами
/// и значение), за ними — данные сложных свойств по порядку.
fn options(bytes: &[u8], record: &Record, shape: &mut ShapeRecord) {
    let count = usize::from(record.instance);
    let mut data = record.start + count * 6;
    for i in 0..count {
        let at = record.start + i * 6;
        if at + 6 > record.end {
            break;
        }
        let (Some(raw), Some(value)) = (u16_at(bytes, at), u32_at(bytes, at + 2)) else {
            break;
        };
        let id = raw & 0x3FFF;
        shape.props.push((id, value));
        if raw & 0x8000 == 0 {
            continue;
        }
        let mut len = value as usize;
        // У массива в начале заголовок из 6 байт: число элементов,
        // запас, размер элемента. Некоторые программы (LibreOffice)
        // считают длину без заголовка.
        if ARRAYS.contains(&id)
            && let (Some(n), Some(size)) = (u16_at(bytes, data), u16_at(bytes, data + 4))
        {
            let full = 6 + usize::from(n) * element_size(size);
            if len + 6 == full {
                len = full;
            }
        }
        let Some(end) = data.checked_add(len).filter(|&e| e <= record.end) else {
            break;
        };
        if let Some(chunk) = bytes.get(data..end) {
            shape.complex.push((id, chunk.to_vec()));
        }
        data = end;
    }
}

/// Размер элемента массива: 0xFFF0 — точки по 2 байта на координату.
fn element_size(size: u16) -> usize {
    if size == 0xFFF0 { 4 } else { usize::from(size) }
}

/// Элементы массива: размер элемента и байты элементов.
fn array(data: &[u8]) -> Option<(usize, Vec<&[u8]>)> {
    let n = usize::from(u16_at(data, 0)?);
    let size = element_size(u16_at(data, 4)?);
    if size == 0 {
        return None;
    }
    let items: Vec<&[u8]> = data.get(6..)?.chunks_exact(size).take(n).collect();
    (items.len() == n).then_some((size, items))
}

/// Фигура модели. `text` — текст надписи по номеру фигуры.
pub(crate) fn shape(record: &ShapeRecord, text: &dyn Fn(u32) -> Vec<Block>) -> Shape {
    if let Some(children) = &record.children {
        let frame = record.group_frame.map(frame_of).unwrap_or_default();
        let children = children
            .iter()
            .map(|child| {
                let frame = child.child_anchor.map(frame_of).unwrap_or_default();
                GroupChild {
                    frame: rotated_frame(frame, rotation(child)),
                    shape: shape(child, text),
                }
            })
            .collect();
        return Shape {
            geometry: Geometry::Group(Group { frame, children }),
            ..Shape::default()
        };
    }
    let flags = record.flags;
    let insets = [0x0081, 0x0082, 0x0083, 0x0084].map(|id| {
        let i = (id - 0x0081) as usize;
        record
            .prop(id)
            .map_or(DEFAULT_TEXT_INSETS[i], |v| i64::from(v as i32))
    });
    let mut blocks = if record.has_text() {
        text(record.spid)
    } else {
        Vec::new()
    };
    // Пустой текст надписи — просто фигура.
    if blocks.iter().all(|b| b.plain_text().trim().is_empty()) && blocks.len() <= 1 {
        blocks.clear();
    }
    Shape {
        geometry: geometry(record),
        fill: fill(record),
        line: line(record),
        rotation: rotation(record),
        flip_horizontal: flags & 0x40 != 0,
        flip_vertical: flags & 0x80 != 0,
        text: blocks,
        text_insets: insets,
        text_anchor: match record.prop(0x0087) {
            Some(1 | 4) => TextAnchor::Center,
            Some(2 | 5) => TextAnchor::Bottom,
            _ => TextAnchor::Top,
        },
        text_color: None,
    }
}

/// Прямоугольник группы `[левый, верхний, правый, нижний]` в EMU.
fn frame_of([left, top, right, bottom]: [i32; 4]) -> Frame {
    let emu = |v: i32| i64::from(v) * EMU_PER_TWIP;
    Frame {
        x: emu(left),
        y: emu(top),
        width: emu(right.saturating_sub(left).max(0)),
        height: emu(bottom.saturating_sub(top).max(0)),
    }
}

/// Поворот фигуры (`rotation`, градусы 16.16) в 1/60 000 градуса.
pub(crate) fn rotation(record: &ShapeRecord) -> i32 {
    record.prop(0x0004).map_or(0, |v| {
        ((i64::from(v as i32) * 60_000) >> 16).rem_euclid(21_600_000) as i32
    })
}

/// У фигуры, повёрнутой почти поперёк (45–135° и 225–315°), в файле
/// записан повёрнутый прямоугольник: ширина и высота меняются местами
/// вокруг середины.
pub(crate) fn rotated_frame(frame: Frame, rotation: i32) -> Frame {
    let degrees = rotation / 60_000;
    if !((45..135).contains(&degrees) || (225..315).contains(&degrees)) {
        return frame;
    }
    let (cx, cy) = (frame.x + frame.width / 2, frame.y + frame.height / 2);
    Frame {
        x: cx - frame.height / 2,
        y: cy - frame.width / 2,
        width: frame.height,
        height: frame.width,
    }
}

/// Готовая фигура по номеру вида (`MSOSPT`) — имя, как в .docx.
fn preset(kind: u16) -> Option<&'static str> {
    Some(match kind {
        1 | 75 | 202 => "rect",
        2 => "roundRect",
        3 => "ellipse",
        4 => "diamond",
        5 => "triangle",
        6 => "rtTriangle",
        7 => "parallelogram",
        8 => "trapezoid",
        9 => "hexagon",
        10 => "octagon",
        11 => "plus",
        12 => "star5",
        13 | 14 => "rightArrow",
        15 => "homePlate",
        16 => "cube",
        18 | 59 => "star16",
        19 => "arc",
        20 => "line",
        21 => "plaque",
        22 => "can",
        23 => "donut",
        32 => "straightConnector1",
        33 => "bentConnector2",
        34 => "bentConnector3",
        35 => "bentConnector4",
        36 => "bentConnector5",
        37 => "curvedConnector2",
        38 => "curvedConnector3",
        39 => "curvedConnector4",
        40 => "curvedConnector5",
        53 => "ribbon",
        54 => "ribbon2",
        55 => "chevron",
        56 => "pentagon",
        57 => "noSmoking",
        58 => "star8",
        60 => "star32",
        61 => "wedgeRectCallout",
        62 => "wedgeRoundRectCallout",
        63 => "wedgeEllipseCallout",
        64 => "wave",
        65 => "foldedCorner",
        66 => "leftArrow",
        67 => "downArrow",
        68 => "upArrow",
        69 => "leftRightArrow",
        70 => "upDownArrow",
        71 => "irregularSeal1",
        72 => "irregularSeal2",
        73 => "lightningBolt",
        74 => "heart",
        76 => "quadArrow",
        84 => "bevel",
        85 => "leftBracket",
        86 => "rightBracket",
        87 => "leftBrace",
        88 => "rightBrace",
        92 => "star24",
        93 => "stripedRightArrow",
        94 => "notchedRightArrow",
        95 => "blockArc",
        96 => "smileyFace",
        183 => "sun",
        184 => "moon",
        185 => "bracketPair",
        186 => "bracePair",
        187 => "star4",
        188 => "doubleWave",
        _ => return None,
    })
}

/// Контур: готовая фигура по виду, иначе свой контур по точкам; если
/// его не понять — прямоугольник.
fn geometry(record: &ShapeRecord) -> Geometry {
    if let Some(name) = preset(record.kind) {
        return Geometry::preset(name);
    }
    custom(record).map_or_else(Geometry::default, Geometry::Custom)
}

/// Свой контур (`pVertices`, `pSegmentInfo`, формулы `pGuides`).
fn custom(record: &ShapeRecord) -> Option<Vec<ShapePath>> {
    let geo = |id: u16, default: i32| record.prop(id).map_or(default, |v| v as i32);
    let (left, top) = (geo(0x0140, 0), geo(0x0141, 0));
    let (right, bottom) = (geo(0x0142, 21_600), geo(0x0143, 21_600));
    let adjust: Vec<f64> = (0..10).map(|i| f64::from(geo(0x0147 + i, 0))).collect();
    let guides = match record.complex(0x0156) {
        Some(data) => guides(data, [left, top, right, bottom], &adjust)?,
        None => Vec::new(),
    };
    let (size, items) = array(record.complex(0x0145)?)?;
    let coordinate = |v: i32| -> Option<f64> {
        let raw = v as u32;
        if (0x8000_0000..=0x8000_007F).contains(&raw) {
            guides.get((raw - 0x8000_0000) as usize).copied()
        } else {
            Some(f64::from(v))
        }
    };
    let points = items
        .iter()
        .map(|item| {
            if size >= 8 {
                let x = u32_at(item, 0)? as i32;
                let y = u32_at(item, 4)? as i32;
                Some((coordinate(x)?, coordinate(y)?))
            } else {
                let x = u16_at(item, 0)? as i16;
                let y = u16_at(item, 2)? as i16;
                Some((f64::from(x), f64::from(y)))
            }
        })
        .collect::<Option<Vec<_>>>()?;
    if points.is_empty() {
        return None;
    }
    // Система координат: из свойств фигуры, а если там пусто (так пишет
    // LibreOffice) — по крайним точкам.
    let (mut x0, mut y0) = (f64::from(left), f64::from(top));
    let (mut width, mut height) = (f64::from(right - left), f64::from(bottom - top));
    if width <= 0.0 || height <= 0.0 {
        let min = |f: fn(&(f64, f64)) -> f64| points.iter().map(f).fold(f64::MAX, f64::min);
        let max = |f: fn(&(f64, f64)) -> f64| points.iter().map(f).fold(f64::MIN, f64::max);
        (x0, y0) = (min(|p| p.0), min(|p| p.1));
        width = (max(|p| p.0) - x0).max(1.0);
        height = (max(|p| p.1) - y0).max(1.0);
    }
    let point = |i: usize| -> Option<(i64, i64)> {
        let (x, y) = points.get(i)?;
        Some(((x - x0).round() as i64, (y - y0).round() as i64))
    };
    let segments: Vec<u16> = match record.complex(0x0146) {
        Some(data) => array(data)?.1.iter().filter_map(|s| u16_at(s, 0)).collect(),
        // Без частей — точки по порядку прямыми.
        None => vec![0x4000, (points.len() as u16).saturating_sub(1), 0x8000],
    };

    let new_path = || ShapePath {
        width: width.round() as i64,
        height: height.round() as i64,
        fill: true,
        stroke: true,
        commands: Vec::new(),
    };
    let mut paths = Vec::new();
    let mut path = new_path();
    let mut next = 0;
    for segment in segments {
        let count = usize::from(segment & 0x1FFF).max(1);
        match segment >> 13 {
            0 => {
                for _ in 0..count {
                    let (x, y) = point(next)?;
                    path.commands.push(PathCommand::LineTo(x, y));
                    next += 1;
                }
            }
            1 => {
                for _ in 0..count {
                    let curve = [point(next)?, point(next + 1)?, point(next + 2)?];
                    path.commands.push(PathCommand::CubicTo(curve));
                    next += 3;
                }
            }
            2 => {
                let (x, y) = point(next)?;
                path.commands.push(PathCommand::MoveTo(x, y));
                next += 1;
            }
            3 => path.commands.push(PathCommand::Close),
            4 => {
                if !path.commands.is_empty() {
                    paths.push(std::mem::replace(&mut path, new_path()));
                }
            }
            5 => {
                // Особые части: только «без заливки» и «без линии».
                if segment & 0xFF != 0 {
                    return None;
                }
                match (segment >> 8) & 0x1F {
                    0x0A => path.fill = false,
                    0x0B => path.stroke = false,
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
    if !path.commands.is_empty() {
        paths.push(path);
    }
    // Не все точки ушли в контур — значит, части записаны не так, как
    // мы понимаем.
    (next == points.len() && !paths.is_empty()).then_some(paths)
}

/// Формулы контура (`pGuides`, записи `MSOSG` по 8 байт): действие
/// и три числа; число — постоянное или ссылка (на край фигуры,
/// настройку формы или прежнюю формулу).
fn guides(data: &[u8], geo: [i32; 4], adjust: &[f64]) -> Option<Vec<f64>> {
    let (_, items) = array(data)?;
    let mut values: Vec<f64> = Vec::with_capacity(items.len());
    for item in items {
        let flags = u16_at(item, 0)?;
        let param = |i: usize| -> Option<f64> {
            let raw = u16_at(item, 2 + i * 2)?;
            if flags & (0x2000 << i) == 0 {
                return Some(f64::from(raw as i16));
            }
            Some(match raw {
                0x0140..=0x0143 => f64::from(geo[usize::from(raw - 0x0140)]),
                0x0147..=0x0150 => adjust[usize::from(raw - 0x0147)],
                0x0400..=0x047F => *values.get(usize::from(raw - 0x0400))?,
                other => f64::from(other),
            })
        };
        let (a, b, c) = (param(0)?, param(1)?, param(2)?);
        let degrees = |v: f64| (v / 65_536.0).to_radians();
        let value = match flags & 0x1FFF {
            0 => a + b - c,
            1 if c == 0.0 => a * b,
            1 => a * b / c,
            2 => (a + b) / 2.0,
            3 => a.abs(),
            4 => a.min(b),
            5 => a.max(b),
            6 => {
                if a > 0.0 {
                    b
                } else {
                    c
                }
            }
            7 => (a * a + b * b + c * c).sqrt(),
            8 => b.atan2(a).to_degrees() * 65_536.0,
            9 => a * degrees(b).sin(),
            10 => a * degrees(b).cos(),
            11 => a * c.atan2(b).cos(),
            12 => a * c.atan2(b).sin(),
            13 => a.max(0.0).sqrt(),
            14 => a + b * 65_536.0 - c * 65_536.0,
            15 if b == 0.0 => 0.0,
            15 => c * (1.0 - (a / b).powi(2)).max(0.0).sqrt(),
            16 => a * degrees(b).tan(),
            _ => return None,
        };
        values.push(if value.is_finite() { value } else { 0.0 });
    }
    Some(values)
}

/// Цвет `OfficeArtCOLORREF` (красный, зелёный, синий, флаги); цвет из
/// палитры или схемы заменяется `default`.
fn color(value: Option<u32>, default: [u8; 3]) -> ShapeColor {
    let rgb = match value {
        Some(v) if (v >> 24) & 0x19 == 0 => [v as u8, (v >> 8) as u8, (v >> 16) as u8],
        _ => default,
    };
    ShapeColor::rgb(rgb)
}

/// Прозрачность (16.16, 0x10000 — непрозрачный).
fn with_opacity(mut color: ShapeColor, opacity: Option<u32>) -> ShapeColor {
    if let Some(v) = opacity.filter(|&v| v < 0x10000) {
        color
            .mods
            .push(ColorMod::Alpha((i64::from(v) * 100_000 / 65_536) as i32));
    }
    color
}

/// Признак из набора `*Bool`: `bit` — значение, `use_bit` — «задано».
fn flag(record: &ShapeRecord, id: u16, bit: u32, use_bit: u32, default: bool) -> bool {
    match record.prop(id) {
        Some(v) if v & (1 << use_bit) != 0 => v & (1 << bit) != 0,
        _ => default,
    }
}

/// Заливка: по умолчанию белая.
fn fill(record: &ShapeRecord) -> Option<ShapeColor> {
    if !flag(record, 0x01BF, 4, 20, true) {
        return None;
    }
    let color = color(record.prop(0x0181), [0xFF, 0xFF, 0xFF]);
    Some(with_opacity(color, record.prop(0x0182)))
}

/// Линия: по умолчанию чёрная, 0,75 пт.
fn line(record: &ShapeRecord) -> Option<ShapeLine> {
    if !flag(record, 0x01FF, 3, 19, true) {
        return None;
    }
    let end = |id: u16| match record.prop(id) {
        Some(1) => LineEnd::Triangle,
        Some(2) => LineEnd::Stealth,
        Some(3) => LineEnd::Diamond,
        Some(4) => LineEnd::Oval,
        Some(5) => LineEnd::Arrow,
        _ => LineEnd::None,
    };
    Some(ShapeLine {
        color: with_opacity(color(record.prop(0x01C0), [0, 0, 0]), record.prop(0x01C1)),
        width: record
            .prop(0x01CB)
            .map_or(DEFAULT_LINE_WIDTH, |v| i64::from(v as i32).max(0)),
        dash: match record.prop(0x01CE) {
            Some(1 | 6 | 7) => LineDash::Dash,
            Some(2 | 5) => LineDash::Dot,
            Some(3 | 4 | 8..=10) => LineDash::DashDot,
            _ => LineDash::Solid,
        },
        head: end(0x01D0),
        tail: end(0x01D1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(version: u16, instance: u16, kind: u16, data: &[u8]) -> Vec<u8> {
        let mut out = (version | instance << 4).to_le_bytes().to_vec();
        out.extend(kind.to_le_bytes());
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        out
    }

    fn fsp(kind: u16, spid: u32, flags: u32) -> Vec<u8> {
        record(
            2,
            kind,
            0xF00A,
            &[spid.to_le_bytes(), flags.to_le_bytes()].concat(),
        )
    }

    /// Таблица свойств; у сложных — данные после записей.
    fn opt(props: &[(u16, u32)], complex: &[u8]) -> Vec<u8> {
        let mut data: Vec<u8> = props
            .iter()
            .flat_map(|(id, v)| [id.to_le_bytes().to_vec(), v.to_le_bytes().to_vec()].concat())
            .collect();
        data.extend(complex);
        record(3, props.len() as u16, 0xF00B, &data)
    }

    fn array_bytes(size: u16, items: &[&[u8]]) -> Vec<u8> {
        let n = items.len() as u16;
        let mut out = [n.to_le_bytes(), n.to_le_bytes(), size.to_le_bytes()].concat();
        for item in items {
            out.extend(*item);
        }
        out
    }

    fn parse(sp: &[u8]) -> ShapeRecord {
        shapes_in(sp, 0, sp.len()).remove(0)
    }

    fn no_text(_: u32) -> Vec<Block> {
        Vec::new()
    }

    #[test]
    fn preset_fill_line_and_rotation() {
        let props = [
            (0x0004, 90 << 16),
            (0x0181, 0x0000_80FF), // оранжевый
            (0x0182, 0x8000),      // полупрозрачный
            (0x01C0, 0x00FF_0000), // синий
            (0x01CB, 25_400),
            (0x01CE, 2),
            (0x01D1, 1),
        ];
        let sp = record(
            0xF,
            0,
            0xF004,
            &[fsp(3, 5, 0x40), opt(&props, &[])].concat(),
        );
        let shape = shape(&parse(&sp), &no_text);
        assert_eq!(shape.geometry, Geometry::preset("ellipse"));
        let fill = shape.fill.unwrap();
        assert_eq!(fill.base, letter_model::ColorBase::Rgb([0xFF, 0x80, 0]));
        assert_eq!(fill.mods, [ColorMod::Alpha(50_000)]);
        let line = shape.line.unwrap();
        assert_eq!(line.color, ShapeColor::rgb([0, 0, 0xFF]));
        assert_eq!(
            (line.width, line.dash, line.tail),
            (25_400, LineDash::Dot, LineEnd::Triangle)
        );
        assert_eq!(shape.rotation, 90 * 60_000);
        assert!(shape.flip_horizontal && !shape.flip_vertical);
        let frame = Frame {
            x: 0,
            y: 0,
            width: 200,
            height: 100,
        };
        assert_eq!(
            rotated_frame(frame, shape.rotation),
            Frame {
                x: 50,
                y: -50,
                width: 100,
                height: 200
            }
        );
    }

    #[test]
    fn no_fill_no_line_and_text() {
        let props = [
            (0x0080, 0x10000),
            (0x0087, 1),
            (0x01BF, 0x10_0000),
            (0x01FF, 0x8_0000),
        ];
        let sp = record(0xF, 0, 0xF004, &[fsp(202, 9, 0), opt(&props, &[])].concat());
        let text = |spid: u32| -> Vec<Block> {
            vec![
                letter_model::Paragraph {
                    content: vec![letter_model::Inline::Run(letter_model::Run::text(format!(
                        "надпись {spid}"
                    )))],
                    ..letter_model::Paragraph::default()
                }
                .into(),
            ]
        };
        let shape = shape(&parse(&sp), &text);
        assert_eq!(shape.fill, None);
        assert_eq!(shape.line, None);
        assert_eq!(shape.text_anchor, TextAnchor::Center);
        assert_eq!(shape.text[0].plain_text(), "надпись 9");
        assert_eq!(shape.text_insets, DEFAULT_TEXT_INSETS);
    }

    #[test]
    fn custom_outline_with_guides() {
        // Треугольник: вершина — формула «половина ширины», длина
        // массива без заголовка (как пишет LibreOffice).
        let point = |x: i32, y: i32| [x.to_le_bytes(), y.to_le_bytes()].concat();
        let vertices = array_bytes(8, &[&point(i32::MIN, 0), &point(100, 100), &point(0, 100)]);
        let segments = array_bytes(
            2,
            &[
                &0x4000u16.to_le_bytes(),
                &2u16.to_le_bytes(),
                &0x6001u16.to_le_bytes(),
                &0x8000u16.to_le_bytes(),
            ],
        );
        // Формула 0: произведение (ширина · 1 / 2), ширина — ссылка.
        let guide = [0x2001u16, 0x0142, 1, 2]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>();
        let guides = array_bytes(8, &[&guide]);
        let props = [
            (0x0142, 100),
            (0x0143, 100),
            (0x8145, (vertices.len() - 6) as u32),
            (0x8146, segments.len() as u32),
            (0x8156, guides.len() as u32),
        ];
        let sp = record(
            0xF,
            0,
            0xF004,
            &[
                fsp(0, 3, 0),
                opt(&props, &[vertices, segments, guides].concat()),
            ]
            .concat(),
        );
        let shape = shape(&parse(&sp), &no_text);
        let Geometry::Custom(paths) = shape.geometry else {
            panic!("не свой контур: {:?}", shape.geometry);
        };
        assert_eq!((paths[0].width, paths[0].height), (100, 100));
        assert_eq!(
            paths[0].commands,
            [
                PathCommand::MoveTo(50, 0),
                PathCommand::LineTo(100, 100),
                PathCommand::LineTo(0, 100),
                PathCommand::Close
            ]
        );
    }

    #[test]
    fn group_with_children() {
        let child = |spid: u32, anchor: [i32; 4]| {
            let anchor: Vec<u8> = anchor.iter().flat_map(|v| v.to_le_bytes()).collect();
            record(
                0xF,
                0,
                0xF004,
                &[fsp(1, spid, 0x2), record(0, 0, 0xF00F, &anchor)].concat(),
            )
        };
        let frame: Vec<u8> = [0i32, 0, 200, 100]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let head = record(
            0xF,
            0,
            0xF004,
            &[record(1, 0, 0xF009, &frame), fsp(0, 10, 0x201)].concat(),
        );
        let group = record(
            0xF,
            0,
            0xF003,
            &[
                head,
                child(11, [0, 0, 100, 100]),
                child(12, [100, 0, 200, 100]),
            ]
            .concat(),
        );
        // Основа рисунка с группой внутри.
        let patriarch = record(0xF, 0, 0xF004, &fsp(0, 1, 0x5));
        let all = record(0xF, 0, 0xF003, &[patriarch, group].concat());
        let shapes = shapes_in(&all, 0, all.len());
        let group = shapes.iter().find(|s| s.spid == 10).unwrap();
        let shape = shape(group, &no_text);
        let Geometry::Group(group) = shape.geometry else {
            panic!("не группа");
        };
        assert_eq!(group.frame.width, 200 * EMU_PER_TWIP);
        assert_eq!(group.children.len(), 2);
        assert_eq!(group.children[1].frame.x, 100 * EMU_PER_TWIP);
        assert_eq!(group.children[1].shape.geometry, Geometry::preset("rect"));
    }

    #[test]
    fn broken_records_are_not_crashes() {
        let props = [(0x8145, 1000), (0x8146, 4)];
        let sp = record(
            0xF,
            0,
            0xF004,
            &[fsp(0, 3, 0), opt(&props, &[1, 2, 3])].concat(),
        );
        for len in 0..sp.len() {
            for s in shapes_in(&sp[..len], 0, len) {
                let _ = shape(&s, &no_text);
            }
        }
    }
}
