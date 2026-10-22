//! Границы и заливка: у таблиц и ячеек (позже — у абзацев и страниц).

use crate::Color;

/// Одна линия границы (`w:top`, `w:insideH` и т. п.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Border {
    /// Вид линии, как в файле: `single`, `double`, `dotted`, `nil`,
    /// `none`… (видов больше двухсот, поэтому имя хранится строкой).
    pub style: String,
    /// Толщина в восьмых долях пункта (`w:sz`).
    pub size: Option<u32>,
    /// Отступ от текста в пунктах (`w:space`).
    pub space: Option<u32>,
    /// Цвет; `None` — «Авто».
    pub color: Option<Color>,
}

impl Border {
    /// Сплошная линия толщиной `eighths` / 8 пт цвета «Авто».
    pub fn single(eighths: u32) -> Border {
        Border {
            style: "single".into(),
            size: Some(eighths),
            space: Some(0),
            color: Some(Color::Auto),
        }
    }

    /// Линии нет (`nil` или `none`).
    pub fn is_none(&self) -> bool {
        matches!(self.style.as_str(), "nil" | "none")
    }

    /// Толщина в пунктах; у линии без вида — 0.
    pub fn width_points(&self) -> f32 {
        if self.is_none() {
            return 0.0;
        }
        // Word рисует линии не тоньше 1/4 пт и не толще 6 пт.
        let eighths = self.size.unwrap_or(4).clamp(2, 96);
        eighths as f32 / 8.0
    }
}

/// Границы со всех сторон и внутренние линии. `None` — не задана здесь
/// (берётся из стиля), `Some` с видом `nil` — убрана.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Borders {
    /// Сверху.
    pub top: Option<Border>,
    /// Слева.
    pub left: Option<Border>,
    /// Снизу.
    pub bottom: Option<Border>,
    /// Справа.
    pub right: Option<Border>,
    /// Между строками (`w:insideH`).
    pub inside_h: Option<Border>,
    /// Между столбцами (`w:insideV`).
    pub inside_v: Option<Border>,
}

impl Borders {
    /// Все шесть линий одинаковые.
    pub fn all(border: Border) -> Borders {
        Borders {
            top: Some(border.clone()),
            left: Some(border.clone()),
            bottom: Some(border.clone()),
            right: Some(border.clone()),
            inside_h: Some(border.clone()),
            inside_v: Some(border),
        }
    }

    /// Ничего не задано.
    pub fn is_empty(&self) -> bool {
        *self == Borders::default()
    }
}

/// Заливка (`w:shd`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shading {
    /// Узор, как в файле: `clear` (без узора), `solid`, `pct10`…
    pub pattern: String,
    /// Цвет узора; `None` — «Авто».
    pub color: Option<Color>,
    /// Цвет фона; `None` — «Авто» (нет заливки).
    pub fill: Option<Color>,
}

impl Shading {
    /// Сплошная заливка цветом `fill`.
    pub fn fill(fill: Color) -> Shading {
        Shading {
            pattern: "clear".into(),
            color: Some(Color::Auto),
            fill: Some(fill),
        }
    }

    /// Цвет, которым закрасить фон, если он есть.
    pub fn background(&self) -> Option<(u8, u8, u8)> {
        let rgb = |c: Option<Color>| match c {
            Some(Color::Rgb(r, g, b)) => Some((r, g, b)),
            _ => None,
        };
        match self.pattern.as_str() {
            "solid" => rgb(self.color).or(Some((0, 0, 0))),
            "nil" => None,
            _ => rgb(self.fill),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn border_widths() {
        assert_eq!(Border::single(4).width_points(), 0.5);
        assert_eq!(Border::single(0).width_points(), 0.25);
        let none = Border {
            style: "nil".into(),
            size: None,
            space: None,
            color: None,
        };
        assert_eq!(none.width_points(), 0.0);
    }

    #[test]
    fn shading_background() {
        assert_eq!(
            Shading::fill(Color::Rgb(1, 2, 3)).background(),
            Some((1, 2, 3))
        );
        let solid = Shading {
            pattern: "solid".into(),
            color: Some(Color::Auto),
            fill: None,
        };
        assert_eq!(solid.background(), Some((0, 0, 0)));
        assert!(Borders::all(Border::single(4)).top.is_some());
    }
}
