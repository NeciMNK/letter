//! Отрисовка страниц на экране через egui: серый фон, белые листы
//! с тенью, текст и линии.
//!
//! Изображения глифов собираются в одну текстуру-атлас; каждая страница
//! рисуется одной сеткой треугольников с этой текстурой, а линии
//! и выделения — прямоугольниками.

use std::collections::HashMap;
use std::sync::Arc;

use egui::epaint::{Mesh, Shadow};
use egui::{
    Color32, ColorImage, Context, Painter, Pos2, Rect, TextureHandle, TextureId, TextureOptions,
};
use letter_layout::{FontLibrary, Page};
use letter_model::ImageData;

use crate::glyphs::{GlyphCache, GlyphImage, GlyphKey, GlyphPixels};
use crate::picture::{PLACEHOLDER_BORDER, PLACEHOLDER_FILL, decode, fit_within};
use crate::scene::{DrawItem, Rgba, page_scene};

/// Цвет фона вокруг листов (как в Word в режиме разметки).
pub const DESK_COLOR: Color32 = Color32::from_rgb(0xE6, 0xE6, 0xE6);

/// Сторона текстуры-атласа, пиксели.
const ATLAS_SIZE: usize = 2048;

/// Отступ между глифами в атласе, чтобы соседи не просвечивали.
const PADDING: usize = 1;

/// Место глифа в атласе.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Slot {
    /// Левый верхний угол в атласе, пиксели.
    x: usize,
    y: usize,
}

/// Раскладка прямоугольников в атласе «полками»: слева направо, пока
/// влезает, потом новая полка ниже.
#[derive(Debug, Clone, Default, PartialEq)]
struct Shelves {
    x: usize,
    y: usize,
    shelf_height: usize,
}

impl Shelves {
    /// Место под прямоугольник `width × height`. `None`, если атлас полон.
    fn allocate(&mut self, width: usize, height: usize) -> Option<Slot> {
        let (w, h) = (width + PADDING, height + PADDING);
        if w > ATLAS_SIZE || h > ATLAS_SIZE {
            return None;
        }
        if self.x + w > ATLAS_SIZE {
            self.x = 0;
            self.y += self.shelf_height;
            self.shelf_height = 0;
        }
        if self.y + h > ATLAS_SIZE {
            return None;
        }
        let slot = Slot {
            x: self.x,
            y: self.y,
        };
        self.x += w;
        self.shelf_height = self.shelf_height.max(h);
        Some(slot)
    }
}

/// Рисует страницы на экране. Один на окно: хранит кеш глифов и атлас.
#[derive(Default)]
pub struct PagePainter {
    cache: GlyphCache,
    texture: Option<TextureHandle>,
    shelves: Shelves,
    slots: HashMap<GlyphKey, Option<Slot>>,
    /// Текстуры картинок рисунков (`None` — картинку не удалось
    /// распаковать, рисуется заглушка).
    pictures: Vec<(Arc<ImageData>, Option<TextureHandle>)>,
}

impl std::fmt::Debug for PagePainter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PagePainter")
            .field("cache", &self.cache)
            .field("slots", &self.slots.len())
            .field("pictures", &self.pictures.len())
            .finish()
    }
}

/// Тень листа, как у страницы в Word.
fn sheet_shadow() -> Shadow {
    Shadow {
        offset: [2, 3],
        blur: 8,
        spread: 0,
        color: Color32::from_black_alpha(60),
    }
}

impl PagePainter {
    /// Новый, с пустыми кешами.
    pub fn new() -> PagePainter {
        PagePainter::default()
    }

    /// Рисует лист `page` в прямоугольнике `rect` (в точках egui):
    /// тень, белый лист, текст. Масштаб определяется шириной `rect`.
    pub fn paint_page(
        &mut self,
        painter: &Painter,
        library: &FontLibrary,
        page: &Page,
        rect: Rect,
    ) {
        painter.add(sheet_shadow().as_shape(rect, 0.0));
        painter.rect_filled(rect, 0.0, Color32::WHITE);
        if page.width <= 0.0 || !painter.clip_rect().intersects(rect) {
            return;
        }
        let ctx = painter.ctx().clone();
        let pixels_per_point = ctx.pixels_per_point();
        // Пикселей на пункт документа.
        let scale = rect.width() / page.width * pixels_per_point;
        let to_screen = |x: f32, y: f32| {
            Pos2::new(
                rect.min.x + x / pixels_per_point,
                rect.min.y + y / pixels_per_point,
            )
        };

        let items = page_scene(page, scale, library, &mut self.cache);
        let mut mesh = None;
        for item in items {
            match item {
                DrawItem::Rect {
                    x,
                    y,
                    width,
                    height,
                    color,
                } => {
                    let r = Rect::from_min_max(to_screen(x, y), to_screen(x + width, y + height));
                    painter.rect_filled(r, 0.0, color32(color));
                }
                DrawItem::Line {
                    x1,
                    y1,
                    x2,
                    y2,
                    width,
                    color,
                } => {
                    let stroke = egui::Stroke::new(width / pixels_per_point, color32(color));
                    painter.line_segment([to_screen(x1, y1), to_screen(x2, y2)], stroke);
                }
                DrawItem::Glyph { key, x, y, color } => {
                    let Some((slot, glyph)) = self.slot(&ctx, library, key) else {
                        continue;
                    };
                    let texture = match &self.texture {
                        Some(t) => t.id(),
                        None => continue,
                    };
                    let mesh = mesh.get_or_insert_with(|| Mesh::with_texture(texture));
                    let left = (x + glyph.left) as f32;
                    let top = (y - glyph.top) as f32;
                    let (w, h) = (glyph.width as f32, glyph.height as f32);
                    let screen =
                        Rect::from_min_max(to_screen(left, top), to_screen(left + w, top + h));
                    let size = ATLAS_SIZE as f32;
                    let uv = Rect::from_min_size(
                        Pos2::new(slot.x as f32 / size, slot.y as f32 / size),
                        egui::vec2(w / size, h / size),
                    );
                    let tint = if matches!(glyph.pixels, GlyphPixels::Color(_)) {
                        Color32::WHITE
                    } else {
                        color32(color)
                    };
                    mesh.add_rect_with_uv(screen, uv, tint);
                }
                DrawItem::Picture {
                    x,
                    y,
                    width,
                    height,
                    image,
                } => {
                    let r = Rect::from_min_max(to_screen(x, y), to_screen(x + width, y + height));
                    match image.and_then(|data| self.picture(&ctx, data)) {
                        Some(texture) => {
                            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                            painter.image(texture, r, uv, Color32::WHITE);
                        }
                        None => paint_placeholder(painter, r),
                    }
                }
            }
        }
        if let Some(mesh) = mesh {
            painter.add(mesh);
        }
    }

    /// Текстура картинки (распаковывает и загружает при первом показе).
    fn picture(&mut self, ctx: &Context, data: Arc<ImageData>) -> Option<TextureId> {
        if let Some((_, texture)) = self.pictures.iter().find(|(d, _)| Arc::ptr_eq(d, &data)) {
            return texture.as_ref().map(TextureHandle::id);
        }
        let max_side = ctx.input(|i| i.max_texture_side).min(u32::MAX as usize) as u32;
        let texture = decode(&data).map(|image| {
            let image = fit_within(image, max_side);
            let size = [image.width as usize, image.height as usize];
            let pixels = ColorImage::from_rgba_unmultiplied(size, &image.pixels);
            ctx.load_texture(
                format!("letter-picture-{}", data.name),
                pixels,
                TextureOptions::LINEAR,
            )
        });
        let id = texture.as_ref().map(TextureHandle::id);
        self.pictures.push((data, texture));
        id
    }

    /// Место глифа в атласе (загружает, если ещё нет) и сам глиф.
    fn slot(
        &mut self,
        ctx: &Context,
        library: &FontLibrary,
        key: GlyphKey,
    ) -> Option<(Slot, Arc<GlyphImage>)> {
        let glyph = self.cache.glyph(library, key)?;
        if let Some(slot) = self.slots.get(&key) {
            return slot.map(|s| (s, glyph));
        }
        let texture = self.texture.get_or_insert_with(|| {
            ctx.load_texture(
                "letter-glyphs",
                ColorImage::filled([ATLAS_SIZE, ATLAS_SIZE], Color32::TRANSPARENT),
                TextureOptions::LINEAR,
            )
        });
        let (w, h) = (glyph.width as usize, glyph.height as usize);
        let mut slot = self.shelves.allocate(w, h);
        if slot.is_none() {
            // Атлас полон: начинаем заново. Глифы, нужные дальше,
            // загрузятся снова.
            self.shelves = Shelves::default();
            self.slots.clear();
            texture.set(
                ColorImage::filled([ATLAS_SIZE, ATLAS_SIZE], Color32::TRANSPARENT),
                TextureOptions::LINEAR,
            );
            slot = self.shelves.allocate(w, h);
        }
        if let Some(s) = slot {
            let pixels: Vec<Color32> = match &glyph.pixels {
                GlyphPixels::Mask(alpha) => alpha
                    .iter()
                    .map(|&a| Color32::from_rgba_premultiplied(a, a, a, a))
                    .collect(),
                GlyphPixels::Color(rgba) => rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
                    .collect(),
            };
            texture.set_partial(
                [s.x, s.y],
                ColorImage::new([w, h], pixels),
                TextureOptions::LINEAR,
            );
        }
        self.slots.insert(key, slot);
        slot.map(|s| (s, glyph))
    }
}

/// Заглушка на месте картинки, которую нельзя показать: серый
/// прямоугольник с рамкой и косым крестом.
fn paint_placeholder(painter: &Painter, r: Rect) {
    let border = egui::Stroke::new(1.0, color32(PLACEHOLDER_BORDER));
    painter.rect_filled(r, 0.0, color32(PLACEHOLDER_FILL));
    painter.rect_stroke(r, 0.0, border, egui::StrokeKind::Inside);
    painter.line_segment([r.left_top(), r.right_bottom()], border);
    painter.line_segment([r.right_top(), r.left_bottom()], border);
}

fn color32(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelves_fill_rows_then_go_down() {
        let mut shelves = Shelves::default();
        let a = shelves.allocate(1000, 10).unwrap();
        let b = shelves.allocate(1000, 20).unwrap();
        let c = shelves.allocate(100, 5).unwrap();
        assert_eq!((a.x, a.y), (0, 0));
        assert_eq!((b.x, b.y), (1001, 0));
        // Третий не влез в первую полку: новая полка ниже самого высокого.
        assert_eq!((c.x, c.y), (0, 21));
        assert!(shelves.allocate(ATLAS_SIZE + 1, 1).is_none());
        // Заполняем до конца.
        let mut count = 0;
        while shelves.allocate(500, 500).is_some() {
            count += 1;
        }
        // Три — рядом с «c» на её полке, и ещё три полки по четыре.
        assert_eq!(count, 15);
        assert!(shelves.allocate(500, 500).is_none());
    }

    #[test]
    fn page_is_painted_with_glyphs_from_the_atlas() {
        let library = FontLibrary::embedded_only();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/docx/headings.docx");
        let doc = letter_docx::read_file(&path).unwrap();
        let layout =
            letter_layout::layout_document(&doc, &mut letter_layout::Shaper::new(), &library);
        let page = &layout.pages[0];

        let ctx = Context::default();
        let mut pages = PagePainter::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let rect = Rect::from_min_size(Pos2::new(20.0, 20.0), egui::vec2(612.0, 792.0));
            pages.paint_page(ui.painter(), &library, page, rect);
        });
        // Текстуры отдаются видеокарте; в тесте её нет.
        assert!(!output.textures_delta.set.is_empty());
        output.textures_delta.clear();
        // Буквы загружены в атлас, текстура создана.
        assert!(pages.slots.len() > 20, "{}", pages.slots.len());
        assert!(pages.slots.values().all(Option::is_some));
        assert!(pages.texture.is_some());
        // Тень, лист и сетка с буквами.
        assert!(output.shapes.len() >= 3, "{}", output.shapes.len());
    }
}
