//! Разделы документа: размер страницы, поля, ориентация.

use crate::{Block, Twips};

/// Раздел документа. У каждого раздела свои размер страницы и поля;
/// документ из одного раздела — самый частый случай.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Section {
    /// Параметры страницы раздела.
    pub props: SectionProps,
    /// Абзацы и таблицы раздела по порядку.
    pub blocks: Vec<Block>,
}

impl Section {
    /// Текст раздела без оформления: блоки через `\n`.
    pub fn plain_text(&self) -> String {
        self.blocks
            .iter()
            .map(Block::plain_text)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Параметры страницы раздела. `None` — не задано в файле;
/// тогда используются [`PageSize::A4`] и [`PageMargins::default`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SectionProps {
    /// Размер и ориентация листа.
    pub page_size: Option<PageSize>,
    /// Поля страницы.
    pub margins: Option<PageMargins>,
}

/// Размер листа.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageSize {
    /// Ширина листа.
    pub width: Twips,
    /// Высота листа.
    pub height: Twips,
    /// Ориентация. Word хранит её отдельно от размеров, но при альбомной
    /// ориентации ширина обычно больше высоты.
    pub orientation: Orientation,
}

impl PageSize {
    /// A4, 210 × 297 мм, книжная.
    pub const A4: PageSize = PageSize {
        width: Twips(11906),
        height: Twips(16838),
        orientation: Orientation::Portrait,
    };

    /// US Letter, 8,5 × 11 дюймов, книжная.
    pub const LETTER: PageSize = PageSize {
        width: Twips(12240),
        height: Twips(15840),
        orientation: Orientation::Portrait,
    };

    /// Тот же лист, повёрнутый в другую ориентацию (ширина и высота меняются местами).
    pub fn rotated(self) -> PageSize {
        PageSize {
            width: self.height,
            height: self.width,
            orientation: match self.orientation {
                Orientation::Portrait => Orientation::Landscape,
                Orientation::Landscape => Orientation::Portrait,
            },
        }
    }
}

impl Default for PageSize {
    fn default() -> PageSize {
        PageSize::A4
    }
}

/// Ориентация листа.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Orientation {
    /// Книжная.
    #[default]
    Portrait,
    /// Альбомная.
    Landscape,
}

/// Поля страницы и расстояния до колонтитулов.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageMargins {
    /// Верхнее поле. Может быть отрицательным: тогда текст не отодвигается
    /// верхним колонтитулом.
    pub top: Twips,
    /// Правое поле.
    pub right: Twips,
    /// Нижнее поле (может быть отрицательным, как верхнее).
    pub bottom: Twips,
    /// Левое поле.
    pub left: Twips,
    /// Расстояние от края листа до верхнего колонтитула.
    pub header: Twips,
    /// Расстояние от края листа до нижнего колонтитула.
    pub footer: Twips,
    /// Поле под переплёт.
    pub gutter: Twips,
}

impl Default for PageMargins {
    /// Поля нового документа русского Word: сверху и снизу 2 см,
    /// слева 3 см, справа 1,5 см, колонтитулы 1,25 см.
    fn default() -> PageMargins {
        PageMargins {
            top: Twips(1134),
            right: Twips(850),
            bottom: Twips(1134),
            left: Twips(1701),
            header: Twips(708),
            footer: Twips(708),
            gutter: Twips::ZERO,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate_page() {
        let landscape = PageSize::A4.rotated();
        assert_eq!(landscape.width, Twips(16838));
        assert_eq!(landscape.orientation, Orientation::Landscape);
        assert_eq!(landscape.rotated(), PageSize::A4);
    }

    #[test]
    fn default_margins_in_cm() {
        let m = PageMargins::default();
        assert!((m.left.to_mm() - 30.0).abs() < 0.05);
        assert!((m.right.to_mm() - 15.0).abs() < 0.05);
    }
}
