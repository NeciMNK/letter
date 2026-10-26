//! Фигуры в редакторе: новая фигура («Вставка → Фигуры») и смена её
//! вида, заливки и контура (вкладка «Формат фигуры»).
//!
//! Фигура в тексте — это рисунок ([`Picture`]) с [`Picture::shape`]:
//! выделение, размер мышью и обтекание у неё те же, что у картинки.

use std::sync::Arc;

use letter_model::{
    Anchor, AnchorPosition, DEFAULT_LINE_WIDTH, DEFAULT_TEXT_INSETS, Distance, Document,
    EMU_PER_POINT, Geometry, HorizontalFrom, LineEnd, Paragraph, Picture, Placement, Shape,
    ShapeColor, ShapeLine, TextAnchor, VerticalFrom, Wrap,
};

use crate::picture::{picture_mut, picture_ref};
use crate::position::Position;

/// Имя «фигуры» надписи в меню: прямоугольник с текстом.
pub const TEXT_BOX: &str = "textBox";

/// Готовые фигуры меню «Фигуры» по группам: линии, прямоугольники,
/// основные фигуры, фигурные стрелки, звёзды, надпись.
pub const SHAPE_GROUPS: [&[&str]; 6] = [
    &["line", "straightConnector1"],
    &["rect", "roundRect"],
    &[
        "ellipse",
        "triangle",
        "rtTriangle",
        "diamond",
        "parallelogram",
        "trapezoid",
        "pentagon",
        "hexagon",
        "octagon",
        "plus",
        "heart",
    ],
    &[
        "rightArrow",
        "leftArrow",
        "upArrow",
        "downArrow",
        "leftRightArrow",
        "upDownArrow",
        "chevron",
    ],
    &["star4", "star5", "star6"],
    &[TEXT_BOX],
];

/// Линия ли это (у линии нет заливки, высота может быть нулевой).
pub fn is_line(preset: &str) -> bool {
    matches!(preset, "line" | "straightConnector1")
}

fn scheme(name: &str) -> ShapeColor {
    ShapeColor {
        base: letter_model::ColorBase::Scheme(name.into()),
        mods: Vec::new(),
    }
}

/// Новая фигура `preset` (имя из [`SHAPE_GROUPS`]) перед текстом, в
/// левом верхнем углу абзаца у курсора — как фигура, нарисованная в
/// Word щелчком без протягивания: квадрат в дюйм, синяя заливка цвета
/// темы «Акцент 1» с более тёмным контуром 1 пт. Надпись — белая с
/// чёрным контуром 0,75 пт и пустым абзацем для текста, 2 × 1 дюйма.
pub fn new_shape(preset: &str) -> Picture {
    let inch = 72 * EMU_PER_POINT;
    let accent_line = ShapeColor {
        mods: vec![letter_model::ColorMod::Shade(50_000)],
        ..scheme("accent1")
    };
    let (shape, width, height) = if preset == TEXT_BOX {
        let shape = Shape {
            geometry: Geometry::preset("rect"),
            fill: Some(scheme("lt1")),
            line: Some(ShapeLine {
                color: scheme("tx1"),
                width: DEFAULT_LINE_WIDTH,
                dash: Default::default(),
                head: LineEnd::None,
                tail: LineEnd::None,
            }),
            text: vec![Paragraph::default().into()],
            text_insets: DEFAULT_TEXT_INSETS,
            text_color: Some(scheme("tx1")),
            ..Shape::default()
        };
        (shape, 2 * inch, inch)
    } else if is_line(preset) {
        let shape = Shape {
            geometry: Geometry::preset(preset),
            fill: None,
            line: Some(ShapeLine {
                color: scheme("accent1"),
                width: 6_350,
                dash: Default::default(),
                head: LineEnd::None,
                tail: if preset == "line" {
                    LineEnd::None
                } else {
                    LineEnd::Triangle
                },
            }),
            ..Shape::default()
        };
        (shape, inch, 0)
    } else {
        let shape = Shape {
            geometry: Geometry::preset(preset),
            fill: Some(scheme("accent1")),
            line: Some(ShapeLine {
                color: accent_line,
                width: EMU_PER_POINT,
                dash: Default::default(),
                head: LineEnd::None,
                tail: LineEnd::None,
            }),
            text_insets: DEFAULT_TEXT_INSETS,
            text_anchor: TextAnchor::Center,
            text_color: Some(scheme("lt1")),
            ..Shape::default()
        };
        (shape, inch, inch)
    };
    Picture {
        width,
        height,
        image: None,
        relationship: None,
        raw: None,
        anchor: Some(Box::new(Anchor {
            horizontal: AnchorPosition {
                relative: HorizontalFrom::Column,
                placement: Placement::Offset(0),
            },
            vertical: AnchorPosition {
                relative: VerticalFrom::Paragraph,
                placement: Placement::Offset(0),
            },
            wrap: Wrap::None,
            behind_text: false,
            distance: Distance {
                left: 114_300,
                right: 114_300,
                ..Distance::default()
            },
        })),
        shape: Some(Arc::new(shape)),
        drawing: None,
    }
}

/// Фигура рисунка в позиции `at`, если там фигура (не группа).
pub fn shape_ref(doc: &Document, at: Position) -> Option<&Shape> {
    let shape = picture_ref(doc, at)?.shape.as_deref()?;
    (!matches!(shape.geometry, Geometry::Group(_))).then_some(shape)
}

/// Ставит рисунку в позиции `at` фигуру `shape`. Исходный XML рисунка
/// больше не годится и забывается: фигура запишется из модели. `false`,
/// если там нет фигуры.
pub fn set_picture_shape(doc: &mut Document, at: Position, shape: Shape) -> bool {
    let Some(picture) = picture_mut(doc, at) else {
        return false;
    };
    if picture.shape.is_none() {
        return false;
    }
    picture.shape = Some(Arc::new(shape));
    picture.raw = None;
    true
}

/// Фигура с другим контуром (`preset`), остальное как было. Для линии —
/// без заливки.
pub fn with_geometry(shape: &Shape, preset: &str) -> Shape {
    let mut shape = shape.clone();
    shape.geometry = Geometry::preset(preset);
    if is_line(preset) {
        shape.fill = None;
    }
    shape
}

/// Фигура с заливкой `fill` (`None` — «Нет заливки»).
pub fn with_fill(shape: &Shape, fill: Option<[u8; 3]>) -> Shape {
    let mut shape = shape.clone();
    shape.fill = fill.map(ShapeColor::rgb);
    shape
}

/// Фигура с цветом контура `color` (`None` — «Нет контура»). У фигуры
/// без контура появляется контур 0,75 пт.
pub fn with_line_color(shape: &Shape, color: Option<[u8; 3]>) -> Shape {
    let mut shape = shape.clone();
    shape.line = color.map(|rgb| {
        let mut line = shape.line.clone().unwrap_or(ShapeLine {
            color: ShapeColor::rgb(rgb),
            width: DEFAULT_LINE_WIDTH,
            dash: Default::default(),
            head: LineEnd::None,
            tail: LineEnd::None,
        });
        line.color = ShapeColor::rgb(rgb);
        line
    });
    shape
}

/// Фигура с толщиной контура `width` (EMU). У фигуры без контура
/// появляется чёрный контур.
pub fn with_line_width(shape: &Shape, width: i64) -> Shape {
    let mut shape = shape.clone();
    let mut line = shape.line.clone().unwrap_or(ShapeLine {
        color: ShapeColor::rgb([0, 0, 0]),
        width,
        dash: Default::default(),
        head: LineEnd::None,
        tail: LineEnd::None,
    });
    line.width = width;
    shape.line = Some(line);
    shape
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picture::insert_picture;

    #[test]
    fn new_shapes() {
        let rect = new_shape("rect");
        let shape = rect.shape.as_deref().unwrap();
        assert_eq!(shape.geometry, Geometry::preset("rect"));
        assert!(shape.fill.is_some());
        assert_eq!(rect.width, rect.height);
        assert!(rect.anchor.is_some());
        let line = new_shape("straightConnector1");
        let shape = line.shape.as_deref().unwrap();
        assert!(shape.fill.is_none());
        assert_eq!(shape.line.as_ref().unwrap().tail, LineEnd::Triangle);
        let text = new_shape(TEXT_BOX);
        assert_eq!(text.shape.as_deref().unwrap().text.len(), 1);
        assert!(text.width > text.height);
    }

    #[test]
    fn style_changes() {
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![Paragraph::default().into()];
        let at = Position::new(0, 0);
        insert_picture(&mut doc, at, new_shape("rect"));
        let shape = shape_ref(&doc, at).unwrap().clone();
        let red = with_fill(&shape, Some([255, 0, 0]));
        assert!(set_picture_shape(&mut doc, at, red));
        let shape = shape_ref(&doc, at).unwrap().clone();
        assert_eq!(shape.fill, Some(ShapeColor::rgb([255, 0, 0])));
        let none = with_line_color(&shape, None);
        assert!(none.line.is_none());
        let thick = with_line_width(&none, 3 * EMU_PER_POINT);
        assert_eq!(thick.line.as_ref().unwrap().width, 3 * EMU_PER_POINT);
        let oval = with_geometry(&thick, "ellipse");
        assert_eq!(oval.geometry, Geometry::preset("ellipse"));
        assert!(oval.fill.is_some());
        assert!(with_geometry(&oval, "line").fill.is_none());
        assert!(!set_picture_shape(&mut doc, Position::new(0, 9), oval));
    }
}
