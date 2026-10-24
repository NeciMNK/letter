//! Разделы документа: размер страницы, поля, ориентация.

use crate::{Block, Border, NumberFormat, RawXml, Twips};

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

/// Нумерация страниц раздела, окно «Формат номера страницы».
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PageNumbering {
    /// «Начать с» (`w:start`); `None` — «продолжить» нумерацию
    /// предыдущего раздела.
    pub start: Option<u32>,
    /// Вид номеров (`w:fmt`); `None` — 1, 2, 3.
    pub format: Option<NumberFormat>,
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
    /// Верхние колонтитулы раздела.
    pub headers: HeaderFooters,
    /// Нижние колонтитулы раздела.
    pub footers: HeaderFooters,
    /// «Особый колонтитул для первой страницы» (`w:titlePg`).
    pub title_page: bool,
    /// Границы страниц (`w:pgBorders`).
    pub page_borders: Option<PageBorders>,
    /// Нумерация страниц (`w:pgNumType`); `None` — как у предыдущего
    /// раздела, арабскими цифрами.
    pub page_numbering: Option<PageNumbering>,
    /// Свойства, которые программа пока не понимает (колонтитулы, колонки,
    /// сетка и т. п.), в порядке из файла.
    pub unknown: Vec<RawXml>,
}

/// Границы страниц раздела (`w:pgBorders`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PageBorders {
    /// Сверху.
    pub top: Option<Border>,
    /// Слева.
    pub left: Option<Border>,
    /// Снизу.
    pub bottom: Option<Border>,
    /// Справа.
    pub right: Option<Border>,
    /// Отступ линий (`w:space`) считается от края листа
    /// (`w:offsetFrom="page"`), иначе — от текста.
    pub from_page_edge: bool,
    /// На каких страницах раздела линии видны (`w:display`).
    pub display: PageBorderDisplay,
    /// Линии под текстом (`w:zOrder="back"`), иначе — над ним.
    pub behind_text: bool,
}

/// На каких страницах раздела видны границы.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PageBorderDisplay {
    /// На всех (`allPages`).
    #[default]
    AllPages,
    /// Только на первой (`firstPage`).
    FirstPage,
    /// На всех, кроме первой (`notFirstPage`).
    NotFirstPage,
}

/// Колонтитул: абзацы и таблицы над или под текстом страницы.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HeaderFooter {
    /// Содержимое.
    pub blocks: Vec<Block>,
    /// Связь главной части .docx с частью колонтитула (`r:id`), если
    /// колонтитул прочитан из файла: при сохранении он записывается в ту же
    /// часть, и её связи (картинки) остаются верными. У нового — `None`.
    /// Колонтитул, общий для нескольких разделов, записывается по первому
    /// из них.
    pub relationship: Option<String>,
}

/// Колонтитулы одного вида (верхние или нижние) раздела. `None` — свой
/// колонтитул не задан: берётся колонтитул того же типа у предыдущего
/// раздела (у первого раздела — пусто).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HeaderFooters {
    /// Обычный (на нечётных страницах, если чётные отдельно).
    pub default: Option<HeaderFooter>,
    /// Первой страницы раздела (если [`SectionProps::title_page`]).
    pub first: Option<HeaderFooter>,
    /// Чётных страниц (если в настройках «чётные и нечётные отдельно»).
    pub even: Option<HeaderFooter>,
}

impl HeaderFooters {
    /// Колонтитул вида `kind`.
    pub fn get(&self, kind: HeaderFooterKind) -> Option<&HeaderFooter> {
        match kind {
            HeaderFooterKind::Default => self.default.as_ref(),
            HeaderFooterKind::First => self.first.as_ref(),
            HeaderFooterKind::Even => self.even.as_ref(),
        }
    }

    /// Колонтитул вида `kind` для правки.
    pub fn get_mut(&mut self, kind: HeaderFooterKind) -> &mut Option<HeaderFooter> {
        match kind {
            HeaderFooterKind::Default => &mut self.default,
            HeaderFooterKind::First => &mut self.first,
            HeaderFooterKind::Even => &mut self.even,
        }
    }

    /// Все заданные колонтитулы с их видом.
    pub fn iter(&self) -> impl Iterator<Item = (HeaderFooterKind, &HeaderFooter)> {
        HeaderFooterKind::ALL
            .into_iter()
            .filter_map(|kind| Some((kind, self.get(kind)?)))
    }

    /// Нет ни одного колонтитула.
    pub fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }
}

/// Вид колонтитула: для каких страниц.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HeaderFooterKind {
    /// Обычный.
    Default,
    /// Первой страницы раздела.
    First,
    /// Чётных страниц.
    Even,
}

impl HeaderFooterKind {
    /// Все виды.
    pub const ALL: [HeaderFooterKind; 3] = [
        HeaderFooterKind::Default,
        HeaderFooterKind::First,
        HeaderFooterKind::Even,
    ];
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
