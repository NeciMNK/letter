//! Картинки рисунков: распаковка PNG, JPEG, GIF и BMP в пиксели.
//!
//! Остальные виды (EMF, WMF, TIFF, битые файлы) не распаковываются:
//! на их месте рисуется заглушка ([`placeholder`]).

use std::io::Cursor;

use image::{ImageFormat, ImageReader, Limits};
use letter_model::ImageData;

use crate::image::RgbaImage;
use crate::scene::Rgba;

/// Самая большая сторона картинки, пиксели: больше не распаковываем.
const MAX_SIDE: u32 = 16_384;
/// Сколько памяти можно взять на распаковку одной картинки (256 МБ).
const MAX_ALLOC: u64 = 256 * 1024 * 1024;

/// Распаковывает картинку рисунка в пиксели RGBA. `None`, если вид
/// картинки не поддерживается, файл испорчен или слишком велик.
pub fn decode(data: &ImageData) -> Option<RgbaImage> {
    let format = ImageFormat::from_path(&data.name)
        .ok()
        .filter(|f| SUPPORTED.contains(f));
    let mut reader = ImageReader::new(Cursor::new(data.bytes.as_slice()));
    match format {
        Some(f) => reader.set_format(f),
        // Расширение врёт или его нет — узнаём вид по первым байтам.
        None => reader = reader.with_guessed_format().ok()?,
    }
    if !reader.format().is_some_and(|f| SUPPORTED.contains(&f)) {
        return None;
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    let decoded = reader.decode().ok()?.into_rgba8();
    let (width, height) = decoded.dimensions();
    Some(RgbaImage {
        width,
        height,
        pixels: decoded.into_raw(),
    })
}

/// Размер картинки в пикселях без распаковки (по заголовку файла).
/// `None`, если вид картинки не поддерживается или файл испорчен.
pub fn pixel_size(data: &ImageData) -> Option<(u32, u32)> {
    let reader = ImageReader::new(Cursor::new(data.bytes.as_slice()))
        .with_guessed_format()
        .ok()?;
    if !reader.format().is_some_and(|f| SUPPORTED.contains(&f)) {
        return None;
    }
    reader
        .into_dimensions()
        .ok()
        .filter(|&(w, h)| w > 0 && h > 0)
}

/// Уменьшает картинку так, чтобы ни одна сторона не была больше
/// `max_side` (берётся ближайшая точка). Меньшую картинку не трогает.
pub fn fit_within(image: RgbaImage, max_side: u32) -> RgbaImage {
    let side = image.width.max(image.height);
    if side <= max_side || max_side == 0 {
        return image;
    }
    let scale = f64::from(max_side) / f64::from(side);
    let width = ((f64::from(image.width) * scale).round() as u32).max(1);
    let height = ((f64::from(image.height) * scale).round() as u32).max(1);
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        let sy = (u64::from(y) * u64::from(image.height) / u64::from(height)) as u32;
        for x in 0..width {
            let sx = (u64::from(x) * u64::from(image.width) / u64::from(width)) as u32;
            pixels.extend_from_slice(&image.pixel(sx, sy).unwrap_or([0, 0, 0, 0]));
        }
    }
    RgbaImage {
        width,
        height,
        pixels,
    }
}

/// Виды картинок, которые программа умеет показывать.
const SUPPORTED: [ImageFormat; 4] = [
    ImageFormat::Png,
    ImageFormat::Jpeg,
    ImageFormat::Gif,
    ImageFormat::Bmp,
];

/// Цвет заливки заглушки на месте картинки, которую нельзя показать.
pub const PLACEHOLDER_FILL: Rgba = [242, 242, 242, 255];
/// Цвет рамки заглушки.
pub const PLACEHOLDER_BORDER: Rgba = [166, 166, 166, 255];

/// Заглушка `width × height` пикселей: светло-серый прямоугольник
/// с рамкой и косым крестом, как у Word на месте битой картинки.
pub fn placeholder(width: u32, height: u32) -> RgbaImage {
    let mut image = RgbaImage::filled(width, height, PLACEHOLDER_FILL);
    let (w, h) = (i64::from(width), i64::from(height));
    // Точка на диагонали, если до неё меньше полупикселя (в масштабе
    // большей стороны).
    let near = |x: i64, y: i64| (x * h - y * w).abs() * 2 <= w.max(h);
    for y in 0..height {
        for x in 0..width {
            let edge = x == 0 || y == 0 || x + 1 == width || y + 1 == height;
            let (px, py) = (i64::from(x), i64::from(y));
            let cross = near(px, py) || near(px, h - 1 - py);
            if edge || cross {
                let i = (y as usize * width as usize + x as usize) * 4;
                if let Some(p) = image.pixels.get_mut(i..i + 4) {
                    p.copy_from_slice(&PLACEHOLDER_BORDER);
                }
            }
        }
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PNG 2 × 1: красная и прозрачная точки.
    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        let image = image::RgbaImage::from_raw(2, 1, vec![255, 0, 0, 255, 0, 0, 0, 0]).unwrap();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn png_is_decoded_even_with_a_wrong_name() {
        for name in ["word/media/image1.png", "word/media/image1.bin"] {
            let data = ImageData {
                name: name.into(),
                bytes: png(),
            };
            let image = decode(&data).unwrap();
            assert_eq!((image.width, image.height), (2, 1));
            assert_eq!(pixel_size(&data), Some((2, 1)));
            assert_eq!(image.pixel(0, 0), Some([255, 0, 0, 255]));
            assert_eq!(image.pixel(1, 0), Some([0, 0, 0, 0]));
        }
    }

    #[test]
    fn unknown_and_broken_pictures_are_not_decoded() {
        let emf = ImageData {
            name: "word/media/image1.emf".into(),
            bytes: vec![1, 0, 0, 0, 108, 0, 0, 0],
        };
        assert!(decode(&emf).is_none());
        assert!(pixel_size(&emf).is_none());
        let broken = ImageData {
            name: "word/media/image1.png".into(),
            bytes: b"\x89PNG\r\n\x1a\nbroken".to_vec(),
        };
        assert!(decode(&broken).is_none());
    }

    #[test]
    fn big_pictures_are_shrunk() {
        let big = RgbaImage::filled(400, 100, [1, 2, 3, 255]);
        let small = fit_within(big.clone(), 100);
        assert_eq!((small.width, small.height), (100, 25));
        assert_eq!(small.pixel(99, 24), Some([1, 2, 3, 255]));
        assert_eq!(fit_within(big, 400).width, 400);
    }

    #[test]
    fn placeholder_has_a_frame_and_a_cross() {
        let image = placeholder(20, 10);
        assert_eq!(image.pixel(0, 5), Some(PLACEHOLDER_BORDER));
        assert_eq!(image.pixel(10, 5), Some(PLACEHOLDER_BORDER));
        assert_eq!(image.pixel(10, 2), Some(PLACEHOLDER_FILL));
    }
}
