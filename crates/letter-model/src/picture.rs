//! Рисунки в тексте ([`Picture`]) и их содержимое ([`ImageData`]).

use std::sync::Arc;

use crate::RawXml;

/// Сколько EMU (английских метрических единиц OOXML) в пункте.
/// В дюйме 914 400 EMU, в сантиметре — 360 000.
pub const EMU_PER_POINT: i64 = 12_700;

/// Рисунок, стоящий в строке как буква (`wp:inline` в .docx).
///
/// В тексте абзаца рисунок занимает один знак U+FFFC («объект»).
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
