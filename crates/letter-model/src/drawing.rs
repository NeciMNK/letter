//! Векторный рисунок из фигур ([`Drawing`]). Так показывается SmartArt:
//! вместе с диаграммой Word сохраняет её готовый вид — фигуры с
//! заливкой, контуром и текстом (часть `word/diagrams/drawing1.xml`).
//! Программа рисует этот вид, а саму диаграмму (данные, макет, стиль)
//! не разбирает: при сохранении она остаётся как была.

/// Рисунок из фигур. Координаты фигур — EMU в осях рисунка: от его
/// левого верхнего угла, `width` × `height` — весь рисунок. Если рамку
/// рисунка растянули, фигуры растягиваются вместе с ней.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Drawing {
    /// Ширина осей рисунка, EMU.
    pub width: i64,
    /// Высота осей рисунка, EMU.
    pub height: i64,
    /// Фигуры снизу вверх: каждая следующая рисуется поверх предыдущих.
    pub shapes: Vec<DrawingShape>,
}

/// Фигура: контуры, заливка, линия и текст.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DrawingShape {
    /// Контуры фигуры (уже повёрнутые), EMU в осях рисунка.
    pub contours: Vec<Contour>,
    /// Заливка замкнутых контуров; `None` — без заливки.
    pub fill: Option<Rgba>,
    /// Линия контура; `None` — без линии.
    pub line: Option<ShapeLine>,
    /// Текст фигуры.
    pub text: Option<ShapeText>,
}

/// Цвет RGBA (последнее — непрозрачность, 255 — непрозрачный).
pub type Rgba = [u8; 4];

/// Ломаная. Кривые уже разбиты на короткие отрезки.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Contour {
    /// Точки, EMU в осях рисунка.
    pub points: Vec<(i64, i64)>,
    /// Замкнутый контур: заливается, линия идёт и от последней точки
    /// к первой. Незамкнутый (дуга) — только линия.
    pub closed: bool,
}

/// Линия контура фигуры.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapeLine {
    /// Цвет.
    pub color: Rgba,
    /// Толщина, EMU.
    pub width: i64,
}

/// Текст в прямоугольнике фигуры.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShapeText {
    /// Левый край области текста (уже без внутренних полей), EMU.
    pub x: i64,
    /// Верхний край.
    pub y: i64,
    /// Ширина.
    pub width: i64,
    /// Высота.
    pub height: i64,
    /// Где текст по высоте области.
    pub anchor: TextAnchor,
    /// Абзацы.
    pub paragraphs: Vec<ShapeParagraph>,
}

/// Положение текста по высоте области фигуры.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAnchor {
    /// Сверху.
    Top,
    /// По середине.
    #[default]
    Middle,
    /// Снизу.
    Bottom,
}

/// Абзац текста фигуры.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShapeParagraph {
    /// Выравнивание строк.
    pub align: TextAlign,
    /// Куски текста одного оформления.
    pub runs: Vec<ShapeRun>,
    /// Размер шрифта пустого абзаца, сотые пункта (по нему — высота
    /// строки, если кусков нет).
    pub size: u32,
}

/// Выравнивание строк текста фигуры.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    /// По левому краю.
    Left,
    /// По центру.
    #[default]
    Center,
    /// По правому краю.
    Right,
}

/// Кусок текста фигуры.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShapeRun {
    /// Текст.
    pub text: String,
    /// Шрифт; `None` — шрифт текста документа.
    pub font: Option<String>,
    /// Размер, сотые пункта.
    pub size: u32,
    /// Цвет.
    pub color: Rgba,
    /// Жирный.
    pub bold: bool,
    /// Курсив.
    pub italic: bool,
}
