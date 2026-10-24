//! Отрисовка страницы в картинку в памяти (без видеокарты): для тестов,
//! сверки с эталонами и сохранения в файл.

use std::sync::Arc;

use letter_layout::{FontLibrary, Page};
use letter_model::ImageData;

use crate::glyphs::{GlyphCache, GlyphPixels};
use crate::picture::{decode, placeholder};
use crate::scene::{DrawItem, Rgba, page_scene};
use crate::shape::{paint, shape_paths};

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

    /// Рисует картинку `source`, растянутую на прямоугольник (берётся
    /// ближайшая точка картинки), с учётом прозрачности.
    fn draw_image(&mut self, x: f32, y: f32, width: f32, height: f32, source: &RgbaImage) {
        if width <= 0.0 || height <= 0.0 || source.width == 0 || source.height == 0 {
            return;
        }
        let (sw, sh) = (source.width as f32, source.height as f32);
        for py in y.round() as i64..(y + height).round() as i64 {
            let sy = ((py as f32 + 0.5 - y) / height * sh)
                .floor()
                .clamp(0.0, sh - 1.0) as u32;
            for px in x.round() as i64..(x + width).round() as i64 {
                let sx = ((px as f32 + 0.5 - x) / width * sw)
                    .floor()
                    .clamp(0.0, sw - 1.0) as u32;
                if let Some(p) = source.pixel(sx, sy) {
                    self.blend(px, py, [p[0], p[1], p[2], 255], p[3]);
                }
            }
        }
    }

    /// Заливает выпуклый многоугольник; покрытие пикселя на краях
    /// считается по 4×4 точкам внутри него.
    fn fill_polygon(&mut self, points: &[(f32, f32)], color: Rgba) {
        if points.len() < 3 {
            return;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &(x, y) in points {
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
        if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
            return;
        }
        let x0 = x0.floor().max(0.0) as i64;
        let y0 = y0.floor().max(0.0) as i64;
        let x1 = (x1.ceil() as i64).min(i64::from(self.width));
        let y1 = (y1.ceil() as i64).min(i64::from(self.height));
        for py in y0..y1 {
            for px in x0..x1 {
                let mut hits = 0u32;
                for sy in 0..4 {
                    for sx in 0..4 {
                        let x = px as f32 + (sx as f32 + 0.5) / 4.0;
                        let y = py as f32 + (sy as f32 + 0.5) / 4.0;
                        if inside_convex(points, x, y) {
                            hits += 1;
                        }
                    }
                }
                if hits > 0 {
                    self.blend(px, py, color, (hits * 255 / 16) as u8);
                }
            }
        }
    }

    /// Заливает любой многоугольник, и невыпуклый (правило «чёт-нечет»,
    /// по центрам пикселей).
    fn fill_shape(&mut self, points: &[(f32, f32)], color: Rgba) {
        if points.len() < 3 {
            return;
        }
        let top = points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
        let bottom = points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
        if !(top.is_finite() && bottom.is_finite()) {
            return;
        }
        let first = top.floor().max(0.0) as i64;
        let last = (bottom.ceil() as i64).min(i64::from(self.height));
        let mut crossings = Vec::new();
        for py in first..last {
            let y = py as f32 + 0.5;
            crossings.clear();
            for (i, &(x0, y0)) in points.iter().enumerate() {
                let (x1, y1) = points[(i + 1) % points.len()];
                if (y0 <= y) != (y1 <= y) {
                    crossings.push(x0 + (y - y0) / (y1 - y0) * (x1 - x0));
                }
            }
            crossings.sort_by(f32::total_cmp);
            for &[left, right] in crossings.as_chunks::<2>().0 {
                let from = (left - 0.5).ceil().max(0.0) as i64;
                let to = ((right - 0.5).floor() as i64).min(i64::from(self.width) - 1);
                for px in from..=to {
                    self.blend(px, py, color, 255);
                }
            }
        }
    }

    /// Линия по точкам толщиной `width`.
    fn stroke_polyline(&mut self, points: &[(f32, f32)], closed: bool, color: Rgba, width: f32) {
        let half = width.max(1.0) / 2.0;
        let count = if closed {
            points.len()
        } else {
            points.len().saturating_sub(1)
        };
        for i in 0..count {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let length = (dx * dx + dy * dy).sqrt();
            if length < 1e-3 {
                continue;
            }
            let (nx, ny) = (-dy / length * half, dx / length * half);
            // Отрезок чуть длиннее, чтобы углы не щербились.
            let (ex, ey) = (dx / length * half, dy / length * half);
            let quad = [
                (a.0 - ex + nx, a.1 - ey + ny),
                (b.0 + ex + nx, b.1 + ey + ny),
                (b.0 + ex - nx, b.1 + ey - ny),
                (a.0 - ex - nx, a.1 - ey - ny),
            ];
            self.fill_polygon(&quad, color);
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

impl RgbaImage {
    /// Рисует отрезок толщиной `width` со сглаженными краями.
    fn draw_line(&mut self, (x1, y1): (f32, f32), (x2, y2): (f32, f32), width: f32, color: Rgba) {
        let half = width.max(0.5) / 2.0;
        let (dx, dy) = (x2 - x1, y2 - y1);
        let length_sq = dx * dx + dy * dy;
        let (left, right) = (x1.min(x2) - half - 1.0, x1.max(x2) + half + 1.0);
        let (top, bottom) = (y1.min(y2) - half - 1.0, y1.max(y2) + half + 1.0);
        for py in top.floor() as i64..bottom.ceil() as i64 {
            for px in left.floor() as i64..right.ceil() as i64 {
                // Расстояние от середины пикселя до отрезка.
                let (cx, cy) = (px as f32 + 0.5, py as f32 + 0.5);
                let t = if length_sq > 0.0 {
                    (((cx - x1) * dx + (cy - y1) * dy) / length_sq).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let (nx, ny) = (x1 + t * dx - cx, y1 + t * dy - cy);
                let distance = (nx * nx + ny * ny).sqrt();
                let cover = (half + 0.5 - distance).clamp(0.0, 1.0);
                if cover > 0.0 {
                    self.blend(px, py, color, (cover * 255.0).round() as u8);
                }
            }
        }
    }
}

/// Точка внутри выпуклого многоугольника (вершины в любом направлении
/// обхода).
fn inside_convex(points: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut sign = 0.0f32;
    for (i, &(ax, ay)) in points.iter().enumerate() {
        let (bx, by) = points[(i + 1) % points.len()];
        let cross = (bx - ax) * (y - ay) - (by - ay) * (x - ax);
        if cross != 0.0 {
            if sign != 0.0 && cross.signum() != sign {
                return false;
            }
            sign = cross.signum();
        }
    }
    sign != 0.0
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
    // Одна картинка может стоять на странице много раз.
    let mut decoded: Vec<(Arc<ImageData>, Option<RgbaImage>)> = Vec::new();
    for item in page_scene(page, scale, library, cache) {
        match item {
            DrawItem::Rect {
                x,
                y,
                width,
                height,
                color,
            } => image.fill_rect(x, y, width, height, color),
            DrawItem::Line {
                x1,
                y1,
                x2,
                y2,
                width,
                color,
            } => image.draw_line((x1, y1), (x2, y2), width, color),
            DrawItem::Polygon { points, color } => image.fill_polygon(&points, color),
            DrawItem::Picture {
                x,
                y,
                width,
                height,
                shape: Some(shape),
                ..
            } => {
                let paths = shape_paths(&shape, x, y, width, height, scale);
                paint(&mut image, &paths, 0.0, 0.0);
            }
            DrawItem::Picture {
                x,
                y,
                width,
                height,
                image: data,
                shape: None,
            } => {
                let source = data.and_then(|data| {
                    let index = match decoded.iter().position(|(d, _)| Arc::ptr_eq(d, &data)) {
                        Some(i) => i,
                        None => {
                            let pixels = decode(&data);
                            decoded.push((data, pixels));
                            decoded.len() - 1
                        }
                    };
                    decoded[index].1.as_ref()
                });
                match source {
                    Some(source) => image.draw_image(x, y, width, height, source),
                    None => {
                        let stub = placeholder(width.round() as u32, height.round() as u32);
                        image.draw_image(x, y, width, height, &stub);
                    }
                }
            }
            DrawItem::Path {
                points,
                closed,
                fill,
                stroke,
            } => {
                if let Some(color) = fill {
                    image.fill_shape(&points, color);
                }
                if let Some((color, width)) = stroke {
                    image.stroke_polyline(&points, closed, color, width);
                }
            }
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
