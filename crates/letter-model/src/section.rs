//! Разделы документа: размер страницы, поля, ориентация.

use crate::{Block, RawXml, Twips};

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
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SectionProps {
    /// Размер и ориентация листа.
    pub page_size: Option<PageSize>,
    /// Поля страницы.
    pub margins: Option<PageMargins>,
    /// Где начинается раздел (`w:type`); `None` — с новой страницы.
    pub start: Option<SectionStart>,
    /// Колонки текста (`w:cols`); `None` — одна колонка.
    pub columns: Option<Columns>,
    /// Свойства, которые программа пока не понимает (колонтитулы, колонки,
    /// сетка и т. п.), в порядке из файла.
    pub unknown: Vec<RawXml>,
}

/// Где начинается раздел.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SectionStart {
    /// Со следующей страницы.
    #[default]
    NextPage,
    /// На текущей странице.
    Continuous,
    /// С чётной страницы.
    EvenPage,
    /// С нечётной страницы.
    OddPage,
    /// Со следующей колонки.
    NextColumn,
}

/// Колонки текста раздела.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Columns {
    /// Сколько колонок (не меньше одной).
    pub count: u16,
    /// Промежуток между колонками одинаковой ширины.
    pub space: Twips,
    /// Линия между колонками.
    pub separator: bool,
    /// Колонки разной ширины: ширина каждой и промежуток после неё.
    /// Пусто — колонки одинаковые.
    pub widths: Vec<(Twips, Twips)>,
}

impl Default for Columns {
    /// Одна колонка, промежуток 1,25 см (как в Word).
    fn default() -> Columns {
        Columns {
            count: 1,
            space: Twips(708),
            separator: false,
            widths: Vec::new(),
        }
    }
}

impl Columns {
    /// Колонки внутри ширины текста `width`: левый край от поля и ширина
    /// каждой. Колонки разной ширины, не влезающие в `width`, сжимаются
    /// в той же пропорции.
    pub fn layout(&self, width: Twips) -> Vec<(Twips, Twips)> {
        let width = width.0.max(1);
        if !self.widths.is_empty() {
            let total: i64 = self
                .widths
                .iter()
                .enumerate()
                .map(|(i, (w, s))| {
                    let space = if i + 1 < self.widths.len() { s.0 } else { 0 };
                    i64::from(w.0.max(0)) + i64::from(space.max(0))
                })
                .sum();
            let scale = |v: i32| -> i32 {
                if total > i64::from(width) && total > 0 {
                    (i64::from(v) * i64::from(width) / total) as i32
                } else {
                    v
                }
            };
            let mut x = 0;
            return self
                .widths
                .iter()
                .map(|(w, s)| {
                    let column = (Twips(x), Twips(scale(w.0.max(0)).max(1)));
                    x += scale(w.0.max(0)) + scale(s.0.max(0));
                    column
                })
                .collect();
        }
        let count = i32::from(self.count.max(1));
        let space = self.space.0.max(0).min(width / count);
        let column = ((width - space * (count - 1)) / count).max(1);
        (0..count)
            .map(|i| (Twips(i * (column + space)), Twips(column)))
            .collect()
    }
}

/// Размер листа.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageSize {
    /// Ширина листа. В битом файле может быть нулевой или отрицательной:
    /// вёрстка обязана заменять такие значения разумными.
    pub width: Twips,
    /// Высота листа (те же оговорки, что у ширины).
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

    #[test]
    fn columns_share_the_width() {
        let two = Columns {
            count: 2,
            space: Twips(100),
            ..Columns::default()
        };
        assert_eq!(
            two.layout(Twips(1100)),
            [(Twips(0), Twips(500)), (Twips(600), Twips(500))]
        );
        assert_eq!(
            Columns::default().layout(Twips(900)),
            [(Twips(0), Twips(900))]
        );
        // Разной ширины, шире текста — сжимаются.
        let uneven = Columns {
            count: 2,
            widths: vec![(Twips(1000), Twips(200)), (Twips(800), Twips(0))],
            ..Columns::default()
        };
        assert_eq!(
            uneven.layout(Twips(1000)),
            [(Twips(0), Twips(500)), (Twips(600), Twips(400))]
        );
    }
}
