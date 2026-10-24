//! Растеризация глифов (`swash`) с кешем.
//!
//! Глиф рисуется в точном размере в пикселях; дробная часть положения
//! по горизонтали округляется до четверти пикселя, и для каждой четверти
//! растровое изображение своё — так буквы стоят ровно там, где их
//! поставила вёрстка, а не прыгают к целым пикселям.

use std::collections::HashMap;
use std::sync::Arc;

use letter_layout::{FontId, FontLibrary, FontMetrics, ShapedRun, Shaper, TextStyle};
use swash::FontRef;
use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::{Angle, Format, Transform, Vector};

/// На сколько частей делится пиксель по горизонтали.
pub const SUBPIXEL_STEPS: u8 = 4;

/// Наклон искусственного курсива (если у шрифта нет курсивного
/// начертания), градусы.
const SYNTHETIC_ITALIC_DEGREES: f32 = 12.0;

/// Какой глиф нарисовать.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    /// Шрифт.
    pub font: FontId,
    /// Номер глифа в шрифте.
    pub glyph_id: u16,
    /// Размер в 1/64 пикселя.
    pub size_64: u32,
    /// Сдвиг вправо в долях пикселя: `0..SUBPIXEL_STEPS`.
    pub subpixel: u8,
    /// Искусственный полужирный.
    pub bold: bool,
    /// Искусственный курсив.
    pub italic: bool,
    /// Масштаб по ширине, проценты (`100` — как есть).
    pub width_percent: u16,
    /// Поворот против часовой стрелки, градусы (`0` — без поворота;
    /// подложка).
    pub rotation: u16,
}

impl GlyphKey {
    /// Размер в пикселях.
    pub fn size(&self) -> f32 {
        self.size_64 as f32 / 64.0
    }
}

/// Растровое изображение глифа.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphImage {
    /// Сдвиг левого края изображения от точки глифа, пиксели.
    pub left: i32,
    /// Сдвиг верхнего края вверх от базовой линии, пиксели.
    pub top: i32,
    /// Ширина, пиксели.
    pub width: u32,
    /// Высота, пиксели.
    pub height: u32,
    /// Пиксели.
    pub pixels: GlyphPixels,
}

/// Пиксели глифа.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlyphPixels {
    /// Обычная буква: только прозрачность (0–255) на пиксель,
    /// цвет — цвет текста.
    Mask(Vec<u8>),
    /// Цветной глиф (эмодзи): RGBA без предумножения.
    Color(Vec<u8>),
}

/// Байты файла шрифта и номер начертания в нём.
type FontData = Arc<(Vec<u8>, u32)>;

/// Шрифты, размеры и готовые изображения глифов. Один на окно.
#[derive(Default)]
pub struct GlyphCache {
    context: ScaleContext,
    shaper: Shaper,
    fonts: HashMap<FontId, Option<FontData>>,
    glyphs: HashMap<GlyphKey, Option<Arc<GlyphImage>>>,
}

impl std::fmt::Debug for GlyphCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlyphCache")
            .field("fonts", &self.fonts.len())
            .field("glyphs", &self.glyphs.len())
            .finish()
    }
}

impl GlyphCache {
    /// Пустой кеш.
    pub fn new() -> GlyphCache {
        GlyphCache::default()
    }

    /// Сколько изображений глифов в кеше.
    pub fn len(&self) -> usize {
        self.glyphs.len()
    }

    /// Кеш пуст.
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// Шейпер для надписей, которые свёрстаны не заранее (текст фигур
    /// SmartArt).
    pub(crate) fn shaper(&mut self) -> &mut Shaper {
        &mut self.shaper
    }

    /// Размеры шрифта (подчёркивание, зачёркивание).
    pub fn metrics(&mut self, library: &FontLibrary, font: FontId) -> Option<FontMetrics> {
        self.shaper.metrics(library, font)
    }

    /// Превращает текст в глифы (для подписей, которых нет в вёрстке:
    /// диаграммы).
    pub fn shape(
        &mut self,
        library: &FontLibrary,
        text: &str,
        style: &TextStyle,
    ) -> Vec<ShapedRun> {
        self.shaper.shape(library, text, style)
    }

    /// Изображение глифа. `None`, если рисовать нечего (пробел) или
    /// шрифт не читается.
    pub fn glyph(&mut self, library: &FontLibrary, key: GlyphKey) -> Option<Arc<GlyphImage>> {
        if let Some(found) = self.glyphs.get(&key) {
            return found.clone();
        }
        let image = self.rasterize(library, key).map(Arc::new);
        self.glyphs.insert(key, image.clone());
        image
    }

    fn font_data(&mut self, library: &FontLibrary, font: FontId) -> Option<FontData> {
        self.fonts
            .entry(font)
            .or_insert_with(|| {
                library
                    .with_face_data(font, |bytes, index| (bytes.to_vec(), index))
                    .map(Arc::new)
            })
            .clone()
    }

    fn rasterize(&mut self, library: &FontLibrary, key: GlyphKey) -> Option<GlyphImage> {
        let data = self.font_data(library, key.font)?;
        let font = FontRef::from_index(&data.0, usize::try_from(data.1).ok()?)?;
        let size = key.size();
        if !(size > 0.0 && size < 4096.0) {
            return None;
        }
        let mut scaler = self.context.builder(font).size(size).hint(false).build();
        let mut render = Render::new(&[
            Source::ColorOutline(0),
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::Outline,
        ]);
        render.format(Format::Alpha).offset(Vector::new(
            f32::from(key.subpixel) / f32::from(SUBPIXEL_STEPS),
            0.0,
        ));
        if key.bold {
            render.embolden(size / 32.0);
        }
        let skew = key
            .italic
            .then(|| Transform::skew(Angle::from_degrees(SYNTHETIC_ITALIC_DEGREES), Angle::ZERO));
        let stretch = (key.width_percent != 100 && key.width_percent > 0)
            .then(|| Transform::scale(f32::from(key.width_percent) / 100.0, 1.0));
        let rotate = (!key.rotation.is_multiple_of(360))
            .then(|| Transform::rotation(Angle::from_degrees(f32::from(key.rotation))));
        let transform = [stretch, skew, rotate]
            .into_iter()
            .flatten()
            .reduce(|acc, next| acc.then(&next));
        render.transform(transform);
        let image = render.render(&mut scaler, key.glyph_id)?;
        let placement = image.placement;
        if placement.width == 0 || placement.height == 0 {
            return None;
        }
        let pixels = match image.content {
            Content::Mask => GlyphPixels::Mask(image.data),
            Content::Color => GlyphPixels::Color(image.data),
            // Цветовые маски для ЖК-экранов не заказываются; если
            // пришла такая, берём зелёный канал как прозрачность.
            Content::SubpixelMask => {
                GlyphPixels::Mask(image.data.as_chunks::<4>().0.iter().map(|p| p[1]).collect())
            }
        };
        Some(GlyphImage {
            left: placement.left,
            top: placement.top,
            width: placement.width,
            height: placement.height,
            pixels,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(library: &FontLibrary, c: char, size: f32) -> GlyphKey {
        let font = library.find("Liberation Serif", false, false).unwrap();
        let glyph_id = library
            .with_face_data(font.id, |bytes, index| {
                swash::FontRef::from_index(bytes, index as usize)
                    .unwrap()
                    .charmap()
                    .map(c)
            })
            .unwrap();
        GlyphKey {
            font: font.id,
            glyph_id,
            size_64: (size * 64.0) as u32,
            subpixel: 0,
            bold: false,
            italic: false,
            width_percent: 100,
            rotation: 0,
        }
    }

    #[test]
    fn letters_are_rasterized_and_cached() {
        let library = FontLibrary::embedded_only();
        let mut cache = GlyphCache::new();
        let h = key(&library, 'H', 32.0);
        let image = cache.glyph(&library, h).unwrap();
        // «H» в Liberation Serif — 0,66 кегля высотой, стоит на базовой линии.
        assert!((20..=23).contains(&image.top), "{}", image.top);
        assert!(image.height >= 20 && image.width >= 20);
        let GlyphPixels::Mask(alpha) = &image.pixels else {
            panic!("ожидалась маска");
        };
        assert_eq!(alpha.len(), (image.width * image.height) as usize);
        assert!(alpha.contains(&255));
        assert_eq!(cache.len(), 1);
        assert!(Arc::ptr_eq(&image, &cache.glyph(&library, h).unwrap()));

        // Пробел — рисовать нечего.
        assert!(cache.glyph(&library, key(&library, ' ', 32.0)).is_none());
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn synthetic_styles_change_the_image() {
        let library = FontLibrary::embedded_only();
        let mut cache = GlyphCache::new();
        let plain = key(&library, 'l', 40.0);
        let bold = GlyphKey {
            bold: true,
            ..plain
        };
        let italic = GlyphKey {
            italic: true,
            ..plain
        };
        let shifted = GlyphKey {
            subpixel: 2,
            ..plain
        };
        let wide = GlyphKey {
            width_percent: 200,
            ..plain
        };
        let wide_italic = GlyphKey {
            italic: true,
            ..wide
        };
        let plain_width = cache_width(&mut cache, &library, plain);
        assert!(cache_width(&mut cache, &library, bold) > plain_width);
        assert!(cache_width(&mut cache, &library, italic) > plain_width);
        assert!(cache_width(&mut cache, &library, wide) > plain_width);
        assert!(
            cache_width(&mut cache, &library, wide_italic)
                > cache_width(&mut cache, &library, wide)
        );
        assert_ne!(cache.glyph(&library, shifted), cache.glyph(&library, plain));
        assert!(cache.metrics(&library, plain.font).is_some());
        // Повёрнутая на 90° буква «l» лежит: шире, чем выше.
        let lying = cache
            .glyph(
                &library,
                GlyphKey {
                    rotation: 90,
                    ..plain
                },
            )
            .unwrap();
        assert!(
            lying.width > lying.height * 2,
            "{}×{}",
            lying.width,
            lying.height
        );
        // Против часовой стрелки: «l» стоит на базовой линии и уходит
        // влево от своей точки.
        assert!(lying.left < -10, "{}", lying.left);
    }

    fn cache_width(cache: &mut GlyphCache, library: &FontLibrary, key: GlyphKey) -> u32 {
        cache.glyph(library, key).unwrap().width
    }
}
