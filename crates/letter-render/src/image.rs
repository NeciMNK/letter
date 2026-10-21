//! Отрисовка страницы в картинку в памяти (без видеокарты): для тестов,
//! сверки с эталонами и сохранения в файл.

use letter_layout::{FontLibrary, Page};

use crate::glyphs::{GlyphCache, GlyphPixels};
use crate::scene::{DrawItem, Rgba, page_scene};

/// Картинка RGBA, 8 бит на канал, без предумножения, строки сверху вниз.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaImage {
    /// Ширина, пиксели.
    pub width: u32,
    /// Высота, пиксели.
    pub height: u32,
    /// Пиксели: по 4 байта, строка за строкой.
    pub pixels: Vec<u8>,
}

impl RgbaImage {
    /// Картинка, залитая цветом.
    pub fn filled(width: u32, height: u32, color: Rgba) -> RgbaImage {
        let count = width as usize * height as usize;
        RgbaImage {
            width,
            height,
            pixels: color.repeat(count),
        }
    }

    /// Цвет пикселя. `None` за пределами картинки.
    pub fn pixel(&self, x: u32, y: u32) -> Option<Rgba> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let p = self.pixels.get(i..i + 4)?;
        Some([p[0], p[1], p[2], p[3]])
    }

    /// Смешивает цвет с пикселем: `alpha` — непрозрачность 0–255.
    fn blend(&mut self, x: i64, y: i64, color: Rgba, alpha: u8) {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return;
        }
        let a = u32::from(alpha) * u32::from(color[3]) / 255;
        if a == 0 {
            return;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        if let Some(p) = self.pixels.get_mut(i..i + 4) {
            for c in 0..3 {
                let mixed = (u32::from(color[c]) * a + u32::from(p[c]) * (255 - a) + 127) / 255;
                p[c] = mixed as u8;
            }
            p[3] = 255;
        }
    }

    fn fill_rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: Rgba) {
        // Покрытие края пикселя учитывается, чтобы тонкие линии не
        // пропадали и не толстели.
        let (x0, y0, x1, y1) = (x, y, x + width, y + height);
        for py in y0.floor() as i64..y1.ceil() as i64 {
            let cover_y = coverage(py as f32, y0, y1);
            for px in x0.floor() as i64..x1.ceil() as i64 {
                let cover = cover_y * coverage(px as f32, x0, x1);
                self.blend(px, py, color, (cover * 255.0).round() as u8);
            }
        }
    }
}

/// Какая доля пикселя `[p, p + 1)` попадает в отрезок `[a, b)`.
fn coverage(p: f32, a: f32, b: f32) -> f32 {
    ((p + 1.0).min(b) - p.max(a)).clamp(0.0, 1.0)
}

/// Рисует страницу на белом листе при масштабе `scale` пикселей на пункт.
pub fn render_page(
    page: &Page,
    scale: f32,
    library: &FontLibrary,
    cache: &mut GlyphCache,
) -> RgbaImage {
    let width = (page.width * scale).round().max(1.0) as u32;
    let height = (page.height * scale).round().max(1.0) as u32;
    let mut image = RgbaImage::filled(width, height, [255, 255, 255, 255]);
    for item in page_scene(page, scale, library, cache) {
        match item {
            DrawItem::Rect {
                x,
                y,
                width,
                height,
                color,
            } => image.fill_rect(x, y, width, height, color),
            DrawItem::Glyph { key, x, y, color } => {
                let Some(glyph) = cache.glyph(library, key) else {
                    continue;
                };
                let left = i64::from(x) + i64::from(glyph.left);
                let top = i64::from(y) - i64::from(glyph.top);
                let w = glyph.width as usize;
                for row in 0..glyph.height as usize {
                    for col in 0..w {
                        let (px, py) = (left + col as i64, top + row as i64);
                        match &glyph.pixels {
                            GlyphPixels::Mask(alpha) => {
                                if let Some(&a) = alpha.get(row * w + col) {
                                    image.blend(px, py, color, a);
                                }
                            }
                            GlyphPixels::Color(rgba) => {
                                let i = (row * w + col) * 4;
                                if let Some(p) = rgba.get(i..i + 4) {
                                    image.blend(px, py, [p[0], p[1], p[2], 255], p[3]);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    image
}
