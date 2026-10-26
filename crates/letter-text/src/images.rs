//! Рисунки в текстовых форматах: картинка прямо в тексте файла как
//! `data:`-адрес (`data:image/png;base64,…`), размер картинки по её
//! заголовку.

use std::sync::Arc;

use letter_model::{ImageData, Picture};

/// EMU в пикселе: картинка без своего разрешения считается напечатанной
/// с 96 точками на дюйм, как в Word и браузерах.
pub const EMU_PER_PIXEL: i64 = 9525;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Байты в Base64.
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Base64 в байты; пробелы и переводы строк пропускаются. `None` — не
/// Base64.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in text.bytes() {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            c if c.is_ascii_whitespace() => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Вид картинки по первым байтам: расширение и MIME-тип.
pub fn image_kind(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"\x89PNG") {
        Some(("png", "image/png"))
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        Some(("jpeg", "image/jpeg"))
    } else if bytes.starts_with(b"GIF8") {
        Some(("gif", "image/gif"))
    } else if bytes.starts_with(b"BM") {
        Some(("bmp", "image/bmp"))
    } else {
        None
    }
}

/// Размер картинки в пикселях по её заголовку (PNG, JPEG, GIF, BMP).
pub fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let be32 = |at: usize| Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    let le16 = |at: usize| Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?));
    let be16 = |at: usize| Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?));
    let size = match image_kind(bytes)?.0 {
        "png" => (be32(16)?, be32(20)?),
        "gif" => (u32::from(le16(6)?), u32::from(le16(8)?)),
        "bmp" => {
            let w = i32::from_le_bytes(bytes.get(18..22)?.try_into().ok()?);
            let h = i32::from_le_bytes(bytes.get(22..26)?.try_into().ok()?);
            (w.unsigned_abs(), h.unsigned_abs())
        }
        _ => {
            // JPEG: ищем заголовок кадра SOF0…SOF15 (кроме DHT, JPG, DAC).
            let mut at = 2;
            loop {
                if *bytes.get(at)? != 0xFF {
                    return None;
                }
                let marker = *bytes.get(at + 1)?;
                let length = usize::from(be16(at + 2)?);
                if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                    break (u32::from(be16(at + 7)?), u32::from(be16(at + 5)?));
                }
                at += 2 + length;
            }
        }
    };
    (size.0 > 0 && size.1 > 0).then_some(size)
}

/// Рисунок из `data:`-адреса. Размер — `width` × `height` пикселей, если
/// заданы (одна сторона — с сохранением пропорций), иначе — по картинке.
pub fn picture_from_data_uri(
    src: &str,
    width: Option<f32>,
    height: Option<f32>,
) -> Option<Picture> {
    let rest = src.trim().strip_prefix("data:")?;
    let (head, data) = rest.split_once(',')?;
    if !head.ends_with(";base64") {
        return None;
    }
    let bytes = base64_decode(data)?;
    picture_from_bytes(bytes, width, height)
}

/// Рисунок из байтов картинки (см. [`picture_from_data_uri`]).
pub fn picture_from_bytes(
    bytes: Vec<u8>,
    width: Option<f32>,
    height: Option<f32>,
) -> Option<Picture> {
    let (extension, _) = image_kind(&bytes)?;
    let (w, h) = image_size(&bytes)?;
    let (w, h) = (w as f32, h as f32);
    let (width, height) = match (width.filter(|v| *v > 0.0), height.filter(|v| *v > 0.0)) {
        (Some(x), Some(y)) => (x, y),
        (Some(x), None) => (x, x * h / w),
        (None, Some(y)) => (y * w / h, y),
        (None, None) => (w, h),
    };
    Some(Picture {
        drawing: None,
        width: (width * EMU_PER_PIXEL as f32).round() as i64,
        height: (height * EMU_PER_PIXEL as f32).round() as i64,
        image: Some(Arc::new(ImageData {
            name: format!("media/image.{extension}"),
            bytes,
            chart: None,
        })),
        relationship: None,
        raw: None,
        anchor: None,
        shape: None,
    })
}

/// Картинка рисунка как `data:`-адрес и размер в пикселях; `None` — у
/// рисунка нет картинки (фигура, диаграмма) или вид картинки незнаком.
pub fn data_uri(picture: &Picture) -> Option<(String, f32, f32)> {
    let image = picture.image.as_ref().filter(|i| i.chart.is_none())?;
    let (_, mime) = image_kind(&image.bytes)?;
    let px = |emu: i64| (emu as f32 / EMU_PER_PIXEL as f32).round();
    Some((
        format!("data:{mime};base64,{}", base64_encode(&image.bytes)),
        px(picture.width),
        px(picture.height),
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn base64_both_ways() {
        for text in ["", "f", "fo", "foo", "foob", "fooba", "foobar"] {
            let encoded = base64_encode(text.as_bytes());
            assert_eq!(base64_decode(&encoded).as_deref(), Some(text.as_bytes()));
        }
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_decode("Zm9v\nYmE=").as_deref(), Some(&b"fooba"[..]));
        assert_eq!(base64_decode("Zm!v"), None);
    }

    #[test]
    fn sizes_from_headers() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(7u32.to_be_bytes());
        png.extend(3u32.to_be_bytes());
        assert_eq!(image_size(&png), Some((7, 3)));
        let gif = b"GIF89a\x05\x00\x02\x00";
        assert_eq!(image_size(gif), Some((5, 2)));
        // JPEG: APP0, потом SOF0 с высотой 2 и шириной 4.
        let jpeg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xC0, 0, 11, 8, 0, 2, 0, 4,
        ];
        assert_eq!(image_size(&jpeg), Some((4, 2)));
        let picture = picture_from_bytes(png, Some(14.0), None).unwrap();
        assert_eq!(picture.width, 14 * EMU_PER_PIXEL);
        assert_eq!(picture.height, 6 * EMU_PER_PIXEL);
        let (uri, w, h) = data_uri(&picture).unwrap();
        assert!(uri.starts_with("data:image/png;base64,iVBORw0KGgo"));
        assert_eq!((w, h), (14.0, 6.0));
        let back = picture_from_data_uri(&uri, Some(w), Some(h)).unwrap();
        assert_eq!(back.image, picture.image);
    }
}
