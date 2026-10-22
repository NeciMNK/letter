//! Рисунки в тексте ([`Picture`]) и их содержимое ([`ImageData`]).

use std::sync::Arc;

use crate::RawXml;

/// Сколько EMU (английских метрических единиц OOXML) в пункте.
/// В дюйме 914 400 EMU, в сантиметре — 360 000.
pub const EMU_PER_POINT: i64 = 12_700;

/// Рисунок: в строке как буква (`wp:inline` в .docx) или плавающий,
/// со своим местом на странице (`wp:anchor`, см. [`Picture::anchor`]).
///
/// В тексте абзаца рисунок занимает один знак U+FFFC («объект»).
/// У плавающего рисунка этот знак — только привязка к абзацу: места
/// в строке он не занимает.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    /// Ширина на странице, EMU.
    pub width: i64,
    /// Высота на странице, EMU.
    pub height: i64,
    /// Байты картинки. `None`, если файл картинки не нашёлся (битая
    /// ссылка): тогда вместо картинки рисуется заглушка.
    pub image: Option<Arc<ImageData>>,
    /// Имя связи из исходного файла (`r:embed`, например `"rId5"`).
    /// `None` у нового рисунка: связь появится при сохранении.
    pub relationship: Option<String>,
    /// Исходный элемент `w:drawing` целиком. При сохранении в нём
    /// меняется только размер, а всё остальное (обрезка, эффекты,
    /// замещающий текст) остаётся как было. `None` у нового рисунка.
    pub raw: Option<RawXml>,
    /// Положение и обтекание плавающего рисунка; `None` — рисунок
    /// в строке.
    pub anchor: Option<Box<Anchor>>,
}

/// Где стоит плавающий рисунок и как его обтекает текст.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    /// Положение по горизонтали.
    pub horizontal: AnchorPosition<HorizontalFrom>,
    /// Положение по вертикали.
    pub vertical: AnchorPosition<VerticalFrom>,
    /// Обтекание текстом.
    pub wrap: Wrap,
    /// Для обтекания «нет»: рисунок за текстом (иначе — перед ним).
    pub behind_text: bool,
    /// Расстояние от текста сверху, снизу, слева и справа, EMU.
    pub distance: Distance,
}

/// Положение рисунка по одной оси: от чего отсчитывать и сколько.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnchorPosition<R> {
    /// От чего отсчитывается положение.
    pub relative: R,
    /// Сдвиг или выравнивание.
    pub placement: Placement,
}

/// Сдвиг или выравнивание рисунка.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Сдвиг от начала области, EMU (может быть отрицательным).
    Offset(i64),
    /// Выравнивание в области.
    Align(AnchorAlign),
}

/// Выравнивание плавающего рисунка в области.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorAlign {
    /// По левому (верхнему) краю.
    Start,
    /// По центру.
    Center,
    /// По правому (нижнему) краю.
    End,
    /// Внутри (у переплёта; пока как по левому или верхнему краю).
    Inside,
    /// Снаружи (пока как по правому или нижнему краю).
    Outside,
}

/// От чего отсчитывается положение по горизонтали (`relativeFrom`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HorizontalFrom {
    /// От левого поля (область — между полями).
    Margin,
    /// От края листа.
    Page,
    /// От колонки (без колонок — как от поля).
    Column,
    /// От знака привязки.
    Character,
    /// Область — левое поле.
    LeftMargin,
    /// Область — правое поле.
    RightMargin,
    /// Область — внутреннее поле.
    InsideMargin,
    /// Область — внешнее поле.
    OutsideMargin,
}

/// От чего отсчитывается положение по вертикали (`relativeFrom`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalFrom {
    /// От верхнего поля (область — между полями).
    Margin,
    /// От края листа.
    Page,
    /// От верха абзаца привязки.
    Paragraph,
    /// От верха строки привязки.
    Line,
    /// Область — верхнее поле.
    TopMargin,
    /// Область — нижнее поле.
    BottomMargin,
    /// Область — внутреннее поле.
    InsideMargin,
    /// Область — внешнее поле.
    OutsideMargin,
}

/// Обтекание плавающего рисунка текстом.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Wrap {
    /// Без обтекания: рисунок за текстом или перед ним.
    #[default]
    None,
    /// Вокруг рамки.
    Square,
    /// По контуру (пока как вокруг рамки).
    Tight,
    /// Сквозное (пока как вокруг рамки).
    Through,
    /// Сверху и снизу: текст только над и под рисунком.
    TopAndBottom,
}

/// Расстояния от рисунка до текста, EMU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Distance {
    /// Сверху.
    pub top: i64,
    /// Снизу.
    pub bottom: i64,
    /// Слева.
    pub left: i64,
    /// Справа.
    pub right: i64,
}

impl Picture {
    /// Ширина, пт.
    pub fn width_points(&self) -> f32 {
        self.width as f32 / EMU_PER_POINT as f32
    }

    /// Высота, пт.
    pub fn height_points(&self) -> f32 {
        self.height as f32 / EMU_PER_POINT as f32
    }
}

/// Файл картинки: имя части пакета и байты (PNG, JPEG и т. д. как есть).
#[derive(Clone, PartialEq, Eq)]
pub struct ImageData {
    /// Имя части внутри файла документа, например `word/media/image1.png`.
    /// По расширению видно, что это за картинка.
    pub name: String,
    /// Содержимое файла картинки.
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for ImageData {
    /// Байты не печатаем: картинки бывают по несколько мегабайт.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageData")
            .field("name", &self.name)
            .field("len", &self.bytes.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_in_points() {
        let picture = Picture {
            width: 914_400,
            height: 127_000,
            image: None,
            relationship: None,
            raw: None,
            anchor: None,
        };
        assert_eq!(picture.width_points(), 72.0);
        assert_eq!(picture.height_points(), 10.0);
        let data = ImageData {
            name: "word/media/a.png".into(),
            bytes: vec![0; 5],
        };
        assert_eq!(
            format!("{data:?}"),
            r#"ImageData { name: "word/media/a.png", len: 5 }"#
        );
    }
}
