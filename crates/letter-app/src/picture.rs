//! Рисунки в окне: чтение файла картинки для «Вставка → Рисунки»,
//! рамка выделенного рисунка с маркерами и изменение размера мышью.

use std::path::Path;
use std::sync::Arc;

use eframe::egui::{
    Color32, CursorIcon, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2, pos2, vec2,
};
use letter_editor::PictureBox;
use letter_model::{EMU_PER_POINT, ImageData, Picture};

use crate::i18n::tr;

/// Расширения картинок, которые можно вставить.
pub const PICTURE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "jpe", "gif", "bmp"];

/// Самый большой файл картинки, который вставляем (100 МБ).
const MAX_FILE: u64 = 100 * 1024 * 1024;

/// EMU в пикселе картинки: Word считает картинку без своего разрешения
/// напечатанной с 96 точками на дюйм.
const EMU_PER_PIXEL: i64 = 9525;

/// Сторона квадратного маркера на рамке, точки экрана.
const HANDLE: f32 = 7.0;

/// Цвет рамки и маркеров выделенного рисунка.
const FRAME_COLOR: Color32 = Color32::from_rgb(0x2B, 0x57, 0x9A);

/// Рисунок из файла картинки: размер — по пикселям картинки (96 точек
/// на дюйм). Ошибка — текст для окна сообщения.
pub fn picture_from_file(path: &Path) -> Result<Picture, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_FILE {
        return Err(tr("picture-too-large").to_owned());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| PICTURE_EXTENSIONS.contains(&e.as_str()))
        .unwrap_or_else(|| "png".to_owned());
    picture_from_bytes(bytes, &extension)
}

/// Рисунок из байтов картинки с расширением `extension`.
pub fn picture_from_bytes(bytes: Vec<u8>, extension: &str) -> Result<Picture, String> {
    let data = ImageData {
        name: format!("media/image.{extension}"),
        bytes,
    };
    let (width, height) = letter_render::picture::pixel_size(&data)
        .ok_or_else(|| tr("picture-unsupported").to_owned())?;
    Ok(Picture {
        drawing: None,
        width: i64::from(width) * EMU_PER_PIXEL,
        height: i64::from(height) * EMU_PER_PIXEL,
        image: Some(Arc::new(data)),
        relationship: None,
        raw: None,
        anchor: None,
    })
}

/// Рисунок тянут мышью: меняют размер или переносят.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PictureDrag {
    /// Рисунок до изменения.
    pub picture: PictureBox,
    /// Какой маркер тянут ([`Handle::MOVE`] — переносят весь рисунок).
    pub handle: Handle,
    /// Где нажали кнопку мыши, точки экрана.
    pub start: Pos2,
    /// Новый размер, пт.
    pub size: Vec2,
    /// На сколько сдвинут перенесённый рисунок, пт.
    pub offset: Vec2,
}

/// Маркер на рамке рисунка: какой край или угол он двигает.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handle {
    /// −1 — левый край, 1 — правый, 0 — ширина не меняется.
    pub x: i8,
    /// −1 — верхний край, 1 — нижний, 0 — высота не меняется.
    pub y: i8,
}

impl Handle {
    /// Все восемь маркеров: углы и середины сторон.
    pub const ALL: [Handle; 8] = [
        Handle { x: -1, y: -1 },
        Handle { x: 0, y: -1 },
        Handle { x: 1, y: -1 },
        Handle { x: 1, y: 0 },
        Handle { x: 1, y: 1 },
        Handle { x: 0, y: 1 },
        Handle { x: -1, y: 1 },
        Handle { x: -1, y: 0 },
    ];

    /// Не маркер, а сам рисунок: его переносят.
    pub const MOVE: Handle = Handle { x: 0, y: 0 };

    /// Угловой маркер меняет размер с сохранением пропорций.
    pub fn is_corner(self) -> bool {
        self.x != 0 && self.y != 0
    }

    /// Середина маркера на рамке `rect`.
    pub fn center(self, rect: Rect) -> Pos2 {
        let at = |side: i8, min: f32, max: f32| match side {
            -1 => min,
            1 => max,
            _ => (min + max) / 2.0,
        };
        pos2(
            at(self.x, rect.left(), rect.right()),
            at(self.y, rect.top(), rect.bottom()),
        )
    }

    /// Указатель мыши над маркером.
    pub fn cursor(self) -> CursorIcon {
        match (self.x, self.y) {
            (0, 0) => CursorIcon::Move,
            (0, _) => CursorIcon::ResizeVertical,
            (_, 0) => CursorIcon::ResizeHorizontal,
            (x, y) if x == y => CursorIcon::ResizeNwSe,
            _ => CursorIcon::ResizeNeSw,
        }
    }
}

/// Маркер рамки `rect` под точкой `point` (с запасом в пару точек).
pub fn handle_at(rect: Rect, point: Pos2) -> Option<Handle> {
    Handle::ALL
        .into_iter()
        .find(|h| Rect::from_center_size(h.center(rect), Vec2::splat(HANDLE + 4.0)).contains(point))
}

/// Новый размер рисунка `size` (пт), если маркер `handle` сдвинули на
/// `delta` (пт). Угол сохраняет пропорции (по тому измерению, которое
/// изменилось сильнее),
/// середина стороны меняет одно измерение. Меньше 1 пт не бывает.
pub fn resized(size: Vec2, handle: Handle, delta: Vec2) -> Vec2 {
    let grow = vec2(f32::from(handle.x) * delta.x, f32::from(handle.y) * delta.y);
    let mut new = size + grow;
    if handle.is_corner() && size.x > 0.0 && size.y > 0.0 {
        let (rx, ry) = (new.x / size.x, new.y / size.y);
        let scale = if (rx - 1.0).abs() >= (ry - 1.0).abs() {
            rx
        } else {
            ry
        };
        new = size * scale;
    }
    let min = 1.0;
    if handle.is_corner() && (new.x < min || new.y < min) {
        let scale = (min / size.x).max(min / size.y);
        return size * scale;
    }
    vec2(new.x.max(min), new.y.max(min))
}

/// Размер в пунктах → EMU.
pub fn to_emu(points: f32) -> i64 {
    (points * EMU_PER_POINT as f32).round() as i64
}

/// Прямоугольник рисунка нового размера `size` (точки экрана): маркер
/// тянет свой край, противоположный стоит на месте.
pub fn preview(rect: Rect, handle: Handle, size: Vec2) -> Rect {
    let x = if handle.x < 0 {
        rect.right() - size.x
    } else {
        rect.left()
    };
    let y = if handle.y < 0 {
        rect.bottom() - size.y
    } else {
        rect.top()
    };
    Rect::from_min_size(pos2(x, y), size)
}

/// Рамка выделенного рисунка с восемью маркерами, как в Word.
pub fn paint_frame(painter: &Painter, rect: Rect) {
    let stroke = Stroke::new(1.0, FRAME_COLOR);
    painter.rect_stroke(rect, 0.0, stroke, StrokeKind::Outside);
    for handle in Handle::ALL {
        let square = Rect::from_center_size(handle.center(rect), Vec2::splat(HANDLE));
        painter.rect_filled(square, 0.0, Color32::WHITE);
        painter.rect_stroke(square, 0.0, stroke, StrokeKind::Inside);
    }
}

/// PNG 1 × 1 с прозрачной точкой (для тестов).
#[cfg(test)]
pub fn tests_png() -> Vec<u8> {
    [
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, 0x49, 0x48, 0x44, 0x52, 0,
        0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1F, 0x15, 0xC4, 0x89, 0, 0, 0, 0x0D, 0x49, 0x44,
        0x41, 0x54, 0x78, 0x9C, 0x63, 0, 1, 0, 0, 5, 0, 1, 0x0D, 0x0A, 0x2D, 0xB4, 0, 0, 0, 0,
        0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
    .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 0.01
    }

    #[test]
    fn corners_keep_proportions_and_sides_do_not() {
        let size = vec2(100.0, 50.0);
        let right = Handle { x: 1, y: 0 };
        assert!(close(
            resized(size, right, vec2(20.0, 99.0)),
            vec2(120.0, 50.0)
        ));
        // Левый край: тянем влево — шире.
        let left = Handle { x: -1, y: 0 };
        assert!(close(
            resized(size, left, vec2(-10.0, 0.0)),
            vec2(110.0, 50.0)
        ));
        let corner = Handle { x: 1, y: 1 };
        assert!(close(
            resized(size, corner, vec2(50.0, 0.0)),
            vec2(150.0, 75.0)
        ));
        let top_left = Handle { x: -1, y: -1 };
        assert!(close(
            resized(size, top_left, vec2(0.0, 25.0)),
            vec2(50.0, 25.0)
        ));
        // Меньше одного пункта не сжимается, пропорции угла сохраняются.
        assert!(close(
            resized(size, corner, vec2(-500.0, -500.0)),
            vec2(2.0, 1.0)
        ));
        assert!(close(
            resized(size, right, vec2(-500.0, 0.0)),
            vec2(1.0, 50.0)
        ));
    }

    #[test]
    fn handles_are_found_on_the_frame() {
        let rect = Rect::from_min_size(pos2(10.0, 10.0), vec2(100.0, 50.0));
        assert_eq!(
            handle_at(rect, pos2(110.0, 60.0)),
            Some(Handle { x: 1, y: 1 })
        );
        assert_eq!(
            handle_at(rect, pos2(60.0, 9.0)),
            Some(Handle { x: 0, y: -1 })
        );
        assert_eq!(handle_at(rect, pos2(60.0, 35.0)), None);
        assert_eq!(Handle { x: -1, y: -1 }.cursor(), CursorIcon::ResizeNwSe);
        assert_eq!(Handle { x: 1, y: -1 }.cursor(), CursorIcon::ResizeNeSw);
        // Тянем левый верхний угол: правый нижний стоит на месте.
        let moved = preview(rect, Handle { x: -1, y: -1 }, vec2(50.0, 25.0));
        assert_eq!(moved.max, rect.max);
        assert_eq!(to_emu(72.0), 914_400);
    }

    #[test]
    fn picture_file_gets_its_size_in_pixels() {
        let picture = picture_from_bytes(tests_png(), "png").unwrap();
        assert_eq!((picture.width, picture.height), (9525, 9525));
        assert_eq!(picture.image.unwrap().name, "media/image.png");
        assert!(picture_from_bytes(b"not a picture".to_vec(), "png").is_err());
    }
}
