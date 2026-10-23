//! Фигуры DrawingML ([`Shape`]): прямоугольники, овалы, стрелки, линии,
//! надписи и группы фигур.
//!
//! Фигура живёт внутри рисунка ([`crate::Picture::shape`]): размер,
//! положение на странице и обтекание у неё те же, что у картинки.
//! Сама фигура описывает только, что нарисовать в этом прямоугольнике:
//! контур, заливку, линию и текст надписи.

use crate::Block;

/// Фигура (`wps:wsp` в .docx) или группа фигур (`wpg:wgp`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Shape {
    /// Контур фигуры или состав группы.
    pub geometry: Geometry,
    /// Заливка; `None` — без заливки (прозрачная).
    pub fill: Option<ShapeColor>,
    /// Линия контура; `None` — без линии.
    pub line: Option<ShapeLine>,
    /// Поворот по часовой стрелке, 1/60 000 градуса (как `rot` в файле).
    pub rotation: i32,
    /// Отражение слева направо.
    pub flip_horizontal: bool,
    /// Отражение сверху вниз.
    pub flip_vertical: bool,
    /// Текст надписи (`wps:txbx`); пусто — фигура без текста.
    pub text: Vec<Block>,
    /// Поля текста надписи слева, сверху, справа и снизу, EMU.
    pub text_insets: [i64; 4],
}

/// Поля текста надписи по умолчанию: 0,25 см слева и справа,
/// 0,13 см сверху и снизу (`lIns`, `tIns`, `rIns`, `bIns`).
pub const DEFAULT_TEXT_INSETS: [i64; 4] = [91_440, 45_720, 91_440, 45_720];

/// Контур фигуры.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Geometry {
    /// Готовая фигура Word (`a:prstGeom`): имя (`"rect"`, `"ellipse"`,
    /// `"rightArrow"`…) и изменённые настройки формы (`a:avLst`), например
    /// `("adj", 16667)` — радиус скругления.
    Preset {
        /// Имя фигуры, как в файле.
        name: String,
        /// Настройки формы: имя и значение.
        adjust: Vec<(String, i64)>,
    },
    /// Свой контур (`a:custGeom`): список линий.
    Custom(Vec<ShapePath>),
    /// Группа: фигуры внутри со своими местами.
    Group(Group),
}

impl Default for Geometry {
    fn default() -> Geometry {
        Geometry::preset("rect")
    }
}

impl Geometry {
    /// Готовая фигура без изменённых настроек.
    pub fn preset(name: &str) -> Geometry {
        Geometry::Preset {
            name: name.to_string(),
            adjust: Vec::new(),
        }
    }
}

/// Группа фигур.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Group {
    /// Система координат внутри группы (`a:chOff`, `a:chExt`): левый
    /// верхний угол и размер, которые растягиваются на всю группу.
    pub frame: Frame,
    /// Фигуры группы по порядку (следующая рисуется поверх предыдущей).
    pub children: Vec<GroupChild>,
}

/// Прямоугольник: левый верхний угол и размер, EMU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Frame {
    /// Левый край.
    pub x: i64,
    /// Верхний край.
    pub y: i64,
    /// Ширина.
    pub width: i64,
    /// Высота.
    pub height: i64,
}

/// Фигура внутри группы.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupChild {
    /// Место в системе координат группы ([`Group::frame`]).
    pub frame: Frame,
    /// Сама фигура (может быть вложенной группой).
    pub shape: Shape,
}

/// Одна линия своего контура (`a:path`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShapePath {
    /// Размер системы координат линии (`w`, `h`); 0 — как у фигуры, EMU.
    pub width: i64,
    /// Высота системы координат.
    pub height: i64,
    /// Заливать ли то, что внутри линии.
    pub fill: bool,
    /// Рисовать ли саму линию.
    pub stroke: bool,
    /// Команды по порядку.
    pub commands: Vec<PathCommand>,
}

/// Команда линии своего контура. Координаты — в системе
/// [`ShapePath::width`] × [`ShapePath::height`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathCommand {
    /// Перейти в точку, не рисуя.
    MoveTo(i64, i64),
    /// Прямая до точки.
    LineTo(i64, i64),
    /// Кривая Безье с двумя опорными точками.
    CubicTo([(i64, i64); 3]),
    /// Кривая Безье с одной опорной точкой.
    QuadTo([(i64, i64); 2]),
    /// Дуга эллипса от текущей точки (`a:arcTo`): радиусы, начальный
    /// угол и размах в 1/60 000 градуса.
    ArcTo {
        /// Радиус по горизонтали.
        width_radius: i64,
        /// Радиус по вертикали.
        height_radius: i64,
        /// Угол начала дуги.
        start: i64,
        /// Размах дуги (по часовой стрелке, если больше нуля).
        swing: i64,
    },
    /// Замкнуть линию.
    Close,
}

/// Линия контура фигуры (`a:ln`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeLine {
    /// Цвет.
    pub color: ShapeColor,
    /// Толщина, EMU (12 700 — 1 пт).
    pub width: i64,
    /// Пунктир.
    pub dash: LineDash,
    /// Конец в начале линии (стрелка).
    pub head: LineEnd,
    /// Конец в конце линии.
    pub tail: LineEnd,
}

/// Толщина линии фигуры по умолчанию — 0,75 пт.
pub const DEFAULT_LINE_WIDTH: i64 = 9_525;

/// Вид линии.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineDash {
    /// Сплошная.
    #[default]
    Solid,
    /// Точки.
    Dot,
    /// Штрихи.
    Dash,
    /// Штрих и точка.
    DashDot,
}

/// Конец линии (`a:headEnd`, `a:tailEnd`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEnd {
    /// Без стрелки.
    #[default]
    None,
    /// Закрашенный треугольник.
    Triangle,
    /// Острая стрелка.
    Stealth,
    /// Открытая стрелка (две линии).
    Arrow,
    /// Ромб.
    Diamond,
    /// Круг.
    Oval,
}

/// Цвет фигуры: свой или из темы, с поправками (светлее, темнее,
/// прозрачность).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeColor {
    /// Основа цвета.
    pub base: ColorBase,
    /// Поправки по порядку из файла.
    pub mods: Vec<ColorMod>,
}

/// Основа цвета фигуры.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColorBase {
    /// Цвет RGB.
    Rgb([u8; 3]),
    /// Цвет темы по имени (`accent1`, `tx1`, `bg1`…).
    Scheme(String),
}

/// Поправка цвета; значения — тысячные доли процента (100 000 — 100 %).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMod {
    /// Умножить яркость (`a:lumMod`).
    LumMod(i32),
    /// Прибавить яркость (`a:lumOff`).
    LumOff(i32),
    /// Затемнить: смешать с чёрным (`a:shade`).
    Shade(i32),
    /// Осветлить: смешать с белым (`a:tint`).
    Tint(i32),
    /// Непрозрачность (`a:alpha`).
    Alpha(i32),
}

impl ShapeColor {
    /// Цвет RGB без поправок.
    pub fn rgb(rgb: [u8; 3]) -> ShapeColor {
        ShapeColor {
            base: ColorBase::Rgb(rgb),
            mods: Vec::new(),
        }
    }

    /// Цвет RGBA для показа: цвет темы берётся из `scheme`, поправки
    /// применяются по порядку.
    pub fn rgba(&self, scheme: &ColorScheme) -> [u8; 4] {
        let [r, g, b] = match &self.base {
            ColorBase::Rgb(rgb) => *rgb,
            ColorBase::Scheme(name) => scheme.color(name),
        };
        let mut rgb = [r, g, b].map(|c| f64::from(c) / 255.0);
        let mut alpha = 1.0;
        for m in &self.mods {
            match *m {
                ColorMod::Shade(v) => rgb = rgb.map(|c| c * fraction(v)),
                ColorMod::Tint(v) => rgb = rgb.map(|c| 1.0 - (1.0 - c) * fraction(v)),
                ColorMod::Alpha(v) => alpha = fraction(v),
                ColorMod::LumMod(v) => rgb = with_lum(rgb, |l| l * fraction(v)),
                ColorMod::LumOff(v) => rgb = with_lum(rgb, |l| l + f64::from(v) / 100_000.0),
            }
        }
        let byte = |c: f64| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        [byte(rgb[0]), byte(rgb[1]), byte(rgb[2]), byte(alpha)]
    }

    /// Тот же цвет в виде RGB (с прозрачностью, если она была): цвет
    /// темы заменяется настоящим цветом из `scheme`.
    pub fn resolved(&self, scheme: &ColorScheme) -> ShapeColor {
        let [r, g, b, a] = self.rgba(scheme);
        let mut color = ShapeColor::rgb([r, g, b]);
        if a != 255 {
            color
                .mods
                .push(ColorMod::Alpha(i32::from(a) * 100_000 / 255));
        }
        color
    }
}

fn fraction(value: i32) -> f64 {
    f64::from(value.clamp(0, 100_000)) / 100_000.0
}

/// Меняет яркость цвета в модели HSL.
fn with_lum(rgb: [f64; 3], f: impl Fn(f64) -> f64) -> [f64; 3] {
    let (h, s, l) = to_hsl(rgb);
    from_hsl(h, s, f(l).clamp(0.0, 1.0))
}

fn to_hsl([r, g, b]: [f64; 3]) -> (f64, f64, f64) {
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

fn from_hsl(h: f64, s: f64, l: f64) -> [f64; 3] {
    if s == 0.0 {
        return [l; 3];
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let channel = |mut t: f64| {
        t = t.rem_euclid(1.0);
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
    [channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0)]
}

/// Цвета темы (`a:clrScheme`): тёмный и светлый цвета текста и фона,
/// шесть цветов акцента и цвета ссылок.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorScheme {
    /// Цвета по именам из файла: `dk1`, `lt1`, `dk2`, `lt2`,
    /// `accent1`…`accent6`, `hlink`, `folHlink`.
    pub colors: Vec<(String, [u8; 3])>,
}

impl Default for ColorScheme {
    /// Цвета темы Office (Word 2013–2021).
    fn default() -> ColorScheme {
        let colors = [
            ("dk1", [0x00, 0x00, 0x00]),
            ("lt1", [0xFF, 0xFF, 0xFF]),
            ("dk2", [0x44, 0x54, 0x6A]),
            ("lt2", [0xE7, 0xE6, 0xE6]),
            ("accent1", [0x44, 0x72, 0xC4]),
            ("accent2", [0xED, 0x7D, 0x31]),
            ("accent3", [0xA5, 0xA5, 0xA5]),
            ("accent4", [0xFF, 0xC0, 0x00]),
            ("accent5", [0x5B, 0x9B, 0xD5]),
            ("accent6", [0x70, 0xAD, 0x47]),
            ("hlink", [0x05, 0x63, 0xC1]),
            ("folHlink", [0x95, 0x4F, 0x72]),
        ];
        ColorScheme {
            colors: colors
                .into_iter()
                .map(|(n, c)| (n.to_string(), c))
                .collect(),
        }
    }
}

impl ColorScheme {
    /// Цвет по имени. Понимает и синонимы из фигур: `tx1` — `dk1`,
    /// `bg1` — `lt1`, `tx2` — `dk2`, `bg2` — `lt2`. Неизвестное имя —
    /// чёрный.
    pub fn color(&self, name: &str) -> [u8; 3] {
        let name = match name {
            "tx1" => "dk1",
            "bg1" => "lt1",
            "tx2" => "dk2",
            "bg2" => "lt2",
            other => other,
        };
        self.colors
            .iter()
            .find(|(n, _)| n == name)
            .map_or([0, 0, 0], |(_, c)| *c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_colors_with_modifications() {
        let scheme = ColorScheme::default();
        let accent = ShapeColor {
            base: ColorBase::Scheme("accent1".into()),
            mods: Vec::new(),
        };
        assert_eq!(accent.rgba(&scheme), [0x44, 0x72, 0xC4, 255]);
        // Линия фигуры Word по умолчанию: accent1 с затемнением 15 %.
        let dark = ShapeColor {
            mods: vec![ColorMod::Shade(15_000)],
            ..accent.clone()
        };
        assert_eq!(dark.rgba(&scheme), [10, 17, 29, 255]);
        // «Белый, фон 1, темнее 50 %».
        let gray = ShapeColor {
            base: ColorBase::Scheme("bg1".into()),
            mods: vec![ColorMod::LumMod(50_000)],
        };
        assert_eq!(gray.rgba(&scheme), [128, 128, 128, 255]);
        let light = ShapeColor {
            base: ColorBase::Rgb([0, 0, 255]),
            mods: vec![ColorMod::LumMod(20_000), ColorMod::LumOff(80_000)],
        };
        assert_eq!(light.rgba(&scheme), [204, 204, 255, 255]);
        let clear = ShapeColor {
            base: ColorBase::Rgb([255, 0, 0]),
            mods: vec![ColorMod::Alpha(50_000)],
        };
        assert_eq!(clear.rgba(&scheme)[3], 128);
        assert_eq!(clear.resolved(&scheme).rgba(&scheme), [255, 0, 0, 128]);
        assert_eq!(
            accent.resolved(&scheme),
            ShapeColor::rgb([0x44, 0x72, 0xC4])
        );
    }
}
