//! Единицы измерения, которые использует Word.

use std::fmt;

/// Твип — 1/20 типографского пункта, то есть 1/1440 дюйма.
///
/// Word хранит почти все расстояния (поля, отступы, интервалы) в твипах.
/// Значение может быть отрицательным: например, выступ абзаца влево за поле.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Twips(pub i32);

impl Twips {
    /// Сколько твипов в одном пункте.
    pub const PER_POINT: i32 = 20;
    /// Сколько твипов в одном дюйме.
    pub const PER_INCH: i32 = 1440;

    /// Нулевое расстояние.
    pub const ZERO: Twips = Twips(0);

    /// Расстояние в пунктах (1 пункт = 1/72 дюйма). Округляет до целого твипа.
    pub fn from_points(points: f64) -> Twips {
        Twips((points * f64::from(Self::PER_POINT)).round() as i32)
    }

    /// Расстояние в дюймах. Округляет до целого твипа.
    pub fn from_inches(inches: f64) -> Twips {
        Twips((inches * f64::from(Self::PER_INCH)).round() as i32)
    }

    /// Расстояние в миллиметрах. Округляет до целого твипа.
    pub fn from_mm(mm: f64) -> Twips {
        Self::from_inches(mm / 25.4)
    }

    /// Значение в пунктах.
    pub fn to_points(self) -> f64 {
        f64::from(self.0) / f64::from(Self::PER_POINT)
    }

    /// Значение в дюймах.
    pub fn to_inches(self) -> f64 {
        f64::from(self.0) / f64::from(Self::PER_INCH)
    }

    /// Значение в миллиметрах.
    pub fn to_mm(self) -> f64 {
        self.to_inches() * 25.4
    }
}

impl fmt::Display for Twips {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}pt", self.to_points())
    }
}

/// Полупункт — 1/2 пункта. В этих единицах Word хранит размер шрифта
/// (`w:sz`): шрифт 11 пт записан как `22`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct HalfPoints(pub u32);

impl HalfPoints {
    /// Размер в пунктах. Округляет до ближайшего полупункта.
    pub fn from_points(points: f64) -> HalfPoints {
        HalfPoints((points * 2.0).round().max(0.0) as u32)
    }

    /// Значение в пунктах.
    pub fn to_points(self) -> f64 {
        f64::from(self.0) / 2.0
    }
}

impl fmt::Display for HalfPoints {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}pt", self.to_points())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twips_conversions() {
        assert_eq!(Twips::from_points(12.0), Twips(240));
        assert_eq!(Twips::from_inches(1.0), Twips(1440));
        assert_eq!(Twips::from_mm(25.4), Twips(1440));
        assert_eq!(Twips::from_mm(210.0), Twips(11906)); // ширина A4
        assert_eq!(Twips(240).to_points(), 12.0);
        assert_eq!(Twips(-720).to_inches(), -0.5);
        assert_eq!(Twips(1440).to_string(), "72pt");
    }

    #[test]
    fn half_points_conversions() {
        assert_eq!(HalfPoints::from_points(11.0), HalfPoints(22));
        assert_eq!(HalfPoints::from_points(10.5), HalfPoints(21));
        assert_eq!(HalfPoints::from_points(-3.0), HalfPoints(0));
        assert_eq!(HalfPoints(23).to_points(), 11.5);
        assert_eq!(HalfPoints(24).to_string(), "12pt");
    }
}
