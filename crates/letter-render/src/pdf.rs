//! Запись свёрстанных страниц в PDF.
//!
//! Страница рисуется той же сценой, что и на экране ([`page_scene`]),
//! только в очень мелком масштабе ([`SCALE`] точек на пункт), чтобы
//! округление положений было незаметно. Текст пишется настоящим
//! текстом: шрифты встраиваются подмножествами (только нужные глифы,
//! крейт `subsetter`), у каждого шрифта есть таблица «глиф → буква»,
//! поэтому текст из PDF можно копировать и искать. Ещё в файл пишутся
//! гиперссылки, переходы к закладкам и оглавление (закладки PDF) из
//! заголовков.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use letter_layout::{FontId, FontLibrary, LineItem, Page};
use letter_model::{Hyperlink, ImageData, Shape};
use pdf_writer::types::{
    ActionType, AnnotationType, CidFontType, FontFlags, SystemInfo, TextRenderingMode,
};
use pdf_writer::writers::Destination;
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr};
use subsetter::GlyphRemapper;

use crate::glyphs::{GlyphCache, SUBPIXEL_STEPS};
use crate::picture::decode;
use crate::scene::{DrawItem, Rgba, page_scene};
use crate::shape::{Segment, shape_paths};

/// Точек сцены на пункт: положения округляются до 1/100 пункта.
const SCALE: f32 = 100.0;
/// Наклон искусственного курсива: тангенс 12°, как на экране.
const ITALIC_SKEW: f32 = 0.2126;

/// Пункт оглавления PDF (закладки в боковой панели программы просмотра).
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineItem {
    /// Уровень, с 1.
    pub level: u8,
    /// Текст.
    pub title: String,
    /// Номер страницы, с нуля.
    pub page: usize,
    /// Верх строки, пт от верхнего края листа.
    pub top: f32,
}

/// Что записать в PDF кроме страниц.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PdfInfo {
    /// Название документа.
    pub title: Option<String>,
    /// Автор.
    pub author: Option<String>,
    /// Оглавление PDF по порядку документа.
    pub outline: Vec<OutlineItem>,
    /// Места закладок документа (для ссылок внутри документа): имя
    /// строчными буквами →
    /// страница (с нуля) и верх строки, пт от верхнего края листа.
    pub anchors: HashMap<String, (usize, f32)>,
}

/// Шрифт, встроенный в PDF.
struct EmbeddedFont {
    /// Имя в ресурсах страниц: `/F0`, `/F1`…
    name: String,
    /// Номер объекта шрифта (Type0).
    id: Ref,
    /// Старые номера глифов → новые (номера в подмножестве).
    remapper: GlyphRemapper,
}

/// Картинка, записанная в PDF.
struct EmbeddedImage {
    name: String,
    id: Ref,
}

/// Номера объектов PDF по порядку.
struct Refs(i32);

impl Refs {
    fn next(&mut self) -> Ref {
        self.0 += 1;
        Ref::new(self.0)
    }
}

/// Записывает страницы `pages` в PDF.
pub fn write_pdf(pages: &[Page], library: &FontLibrary, info: &PdfInfo) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let mut refs = Refs(0);
    let catalog_id = refs.next();
    let tree_id = refs.next();
    let info_id = refs.next();
    let page_ids: Vec<Ref> = pages.iter().map(|_| refs.next()).collect();

    let mut cache = GlyphCache::new();
    let mut fonts: HashMap<FontId, EmbeddedFont> = HashMap::new();
    let mut images: Vec<(Arc<ImageData>, EmbeddedImage)> = Vec::new();
    let mut alphas: BTreeMap<u8, (String, Ref)> = BTreeMap::new();
    let mut placeholder: Option<EmbeddedImage> = None;

    for (index, page) in pages.iter().enumerate() {
        let scene = page_scene(page, SCALE, library, &mut cache);
        let content = page_content(
            &scene,
            page.height,
            &mut fonts,
            &mut images,
            &mut alphas,
            &mut placeholder,
            &mut pdf,
            &mut refs,
        );
        let content_id = refs.next();
        let data = deflate(&content);
        pdf.stream(content_id, &data).filter(Filter::FlateDecode);

        let annotations = page_links(page, index, &page_ids, info, &mut pdf, &mut refs);
        let mut writer = pdf.page(page_ids[index]);
        writer
            .media_box(Rect::new(0.0, 0.0, page.width, page.height))
            .parent(tree_id)
            .contents(content_id);
        if !annotations.is_empty() {
            writer.annotations(annotations.iter().copied());
        }
        let mut resources = writer.resources();
        let mut font_dict = resources.fonts();
        for font in fonts.values() {
            font_dict.pair(Name(font.name.as_bytes()), font.id);
        }
        font_dict.finish();
        let mut objects = resources.x_objects();
        for image in images.iter().map(|(_, i)| i).chain(placeholder.as_ref()) {
            objects.pair(Name(image.name.as_bytes()), image.id);
        }
        objects.finish();
        let mut states = resources.ext_g_states();
        for (name, id) in alphas.values() {
            states.pair(Name(name.as_bytes()), *id);
        }
        states.finish();
        resources.finish();
        writer.finish();
    }

    // Прозрачности: по объекту на каждую.
    for (alpha, (_, id)) in &alphas {
        pdf.ext_graphics(*id)
            .non_stroking_alpha(f32::from(*alpha) / 255.0)
            .stroking_alpha(f32::from(*alpha) / 255.0);
    }

    // Шрифты.
    for (id, font) in &fonts {
        write_font(&mut pdf, &mut refs, library, *id, font);
    }

    pdf.pages(tree_id)
        .kids(page_ids.iter().copied())
        .count(page_ids.len() as i32);

    let outline_id = write_outline(&mut pdf, &mut refs, &info.outline, &page_ids, pages);
    let mut catalog = pdf.catalog(catalog_id);
    catalog.pages(tree_id);
    if let Some(outline) = outline_id {
        catalog.outlines(outline);
        catalog.page_mode(pdf_writer::types::PageMode::UseOutlines);
    }
    catalog.finish();

    let mut document = pdf.document_info(info_id);
    if let Some(title) = info.title.as_deref().filter(|t| !t.trim().is_empty()) {
        document.title(TextStr(title));
    }
    if let Some(author) = info.author.as_deref().filter(|a| !a.trim().is_empty()) {
        document.author(TextStr(author));
    }
    document.producer(TextStr("letter"));
    document.finish();
    pdf.finish()
}

/// Содержимое страницы из сцены.
#[allow(clippy::too_many_arguments)]
fn page_content(
    scene: &[DrawItem],
    height: f32,
    fonts: &mut HashMap<FontId, EmbeddedFont>,
    images: &mut Vec<(Arc<ImageData>, EmbeddedImage)>,
    alphas: &mut BTreeMap<u8, (String, Ref)>,
    placeholder: &mut Option<EmbeddedImage>,
    pdf: &mut Pdf,
    refs: &mut Refs,
) -> Vec<u8> {
    let mut content = Content::new();
    let mut in_text = false;
    let mut current_font: Option<(FontId, u32)> = None;
    let mut current_color: Option<Rgba> = None;
    let mut current_mode = TextRenderingMode::Fill;
    let mut alpha_state = |content: &mut Content, refs: &mut Refs, alpha: u8| {
        let count = alphas.len();
        let (name, _) = alphas
            .entry(alpha)
            .or_insert_with(|| (format!("A{count}"), refs.next()));
        content.set_parameters(Name(name.as_bytes()));
    };
    let mut current_alpha = 255u8;
    for item in scene {
        match item {
            DrawItem::Rect {
                x,
                y,
                width,
                height: h,
                color,
            } => {
                if in_text {
                    content.end_text();
                    in_text = false;
                    current_font = None;
                }
                if color[3] != current_alpha {
                    alpha_state(&mut content, refs, color[3]);
                    current_alpha = color[3];
                }
                set_fill(&mut content, *color, &mut current_color);
                let (x, y, w, h) = (x / SCALE, y / SCALE, width / SCALE, h / SCALE);
                content.rect(x, height - y - h, w, h).fill_nonzero();
            }
            DrawItem::Line {
                x1,
                y1,
                x2,
                y2,
                width,
                color,
            } => {
                if in_text {
                    content.end_text();
                    in_text = false;
                    current_font = None;
                }
                if color[3] != current_alpha {
                    alpha_state(&mut content, refs, color[3]);
                    current_alpha = color[3];
                }
                let [r, g, b, _] = *color;
                content
                    .save_state()
                    .set_stroke_rgb(unit(r), unit(g), unit(b))
                    .set_line_width(width / SCALE)
                    .set_line_cap(pdf_writer::types::LineCapStyle::RoundCap)
                    .move_to(x1 / SCALE, height - y1 / SCALE)
                    .line_to(x2 / SCALE, height - y2 / SCALE)
                    .stroke()
                    .restore_state();
            }
            DrawItem::Polygon { points, color } => {
                if in_text {
                    content.end_text();
                    in_text = false;
                    current_font = None;
                }
                if color[3] != current_alpha {
                    alpha_state(&mut content, refs, color[3]);
                    current_alpha = color[3];
                }
                set_fill(&mut content, *color, &mut current_color);
                let mut points = points.iter().map(|&(x, y)| (x / SCALE, height - y / SCALE));
                if let Some((x, y)) = points.next() {
                    content.move_to(x, y);
                    for (x, y) in points {
                        content.line_to(x, y);
                    }
                    content.close_path().fill_nonzero();
                }
            }
            DrawItem::Glyph { key, x, y, color } => {
                let font = embed_font(fonts, refs, key.font);
                let gid = font.remapper.remap(key.glyph_id);
                if !in_text {
                    content.begin_text();
                    in_text = true;
                }
                if color[3] != current_alpha {
                    alpha_state(&mut content, refs, color[3]);
                    current_alpha = color[3];
                }
                set_fill(&mut content, *color, &mut current_color);
                let size = key.size_64 as f32 / 64.0 / SCALE;
                let size_key = key.size_64;
                if current_font != Some((key.font, size_key)) {
                    content.set_font(Name(font.name.as_bytes()), size);
                    current_font = Some((key.font, size_key));
                }
                let mode = if key.bold {
                    TextRenderingMode::FillStroke
                } else {
                    TextRenderingMode::Fill
                };
                if mode != current_mode {
                    content.set_text_rendering_mode(mode);
                    if key.bold {
                        content.set_line_width(size * 0.03);
                    }
                    current_mode = mode;
                }
                if key.bold {
                    let [r, g, b, _] = *color;
                    content.set_stroke_rgb(unit(r), unit(g), unit(b));
                }
                let px = (*x as f32 + f32::from(key.subpixel) / f32::from(SUBPIXEL_STEPS)) / SCALE;
                let py = height - *y as f32 / SCALE;
                let sx = f32::from(key.width_percent) / 100.0;
                let skew = if key.italic { ITALIC_SKEW } else { 0.0 };
                let (sin, cos) = f32::from(key.rotation).to_radians().sin_cos();
                content.set_text_matrix([
                    sx * cos,
                    sx * sin,
                    skew * cos - sin,
                    skew * sin + cos,
                    px,
                    py,
                ]);
                content.show(Str(&gid.to_be_bytes()));
            }
            DrawItem::Picture {
                x,
                y,
                width,
                height: h,
                shape: Some(shape),
                ..
            } => {
                if in_text {
                    content.end_text();
                    in_text = false;
                    current_font = None;
                }
                write_shape(&mut content, shape, [*x, *y, *width, *h], height);
            }
            DrawItem::Picture {
                x,
                y,
                width,
                height: h,
                image,
                shape: None,
            } => {
                if in_text {
                    content.end_text();
                    in_text = false;
                    current_font = None;
                }
                let embedded = match image {
                    Some(data) => embed_image(images, data, pdf, refs),
                    None => None,
                };
                let embedded = match embedded {
                    Some(e) => Some(e),
                    None => {
                        if placeholder.is_none() {
                            *placeholder = Some(write_placeholder(pdf, refs));
                        }
                        placeholder.as_ref().map(|p| (p.name.clone(), p.id))
                    }
                };
                let Some((name, _)) = embedded else {
                    continue;
                };
                let (x, y, w, h) = (x / SCALE, y / SCALE, width / SCALE, h / SCALE);
                content
                    .save_state()
                    .transform([w, 0.0, 0.0, h, x, height - y - h])
                    .x_object(Name(name.as_bytes()))
                    .restore_state();
            }
            DrawItem::Path {
                points,
                closed,
                fill,
                stroke,
            } => {
                if in_text {
                    content.end_text();
                    in_text = false;
                    current_font = None;
                }
                let Some((&(x0, y0), rest)) = points.split_first() else {
                    continue;
                };
                let point = |x: f32, y: f32| (x / SCALE, height - y / SCALE);
                // Цвета и толщина линии — внутри q … Q, чтобы не сбить
                // то, что запомнено для текста.
                content.save_state();
                let path = |content: &mut Content| {
                    let (x, y) = point(x0, y0);
                    content.move_to(x, y);
                    for &(px, py) in rest {
                        let (x, y) = point(px, py);
                        content.line_to(x, y);
                    }
                    if *closed {
                        content.close_path();
                    }
                };
                if let Some(color) = fill {
                    if color[3] != current_alpha {
                        alpha_state(&mut content, refs, color[3]);
                    }
                    content.set_fill_rgb(unit(color[0]), unit(color[1]), unit(color[2]));
                    path(&mut content);
                    content.fill_nonzero();
                }
                if let Some((color, width)) = stroke {
                    if color[3] != current_alpha {
                        alpha_state(&mut content, refs, color[3]);
                    }
                    content.set_stroke_rgb(unit(color[0]), unit(color[1]), unit(color[2]));
                    content.set_line_width(width / SCALE);
                    content.set_line_join(pdf_writer::types::LineJoinStyle::RoundJoin);
                    path(&mut content);
                    content.stroke();
                }
                content.restore_state();
            }
        }
    }
    if in_text {
        content.end_text();
    }
    content.finish().to_vec()
}

/// Фигура линиями PDF: каждая линия — со своими заливкой и обводкой
/// внутри `q … Q`, чтобы не сбить цвета и толщину линий текста.
/// Прозрачность фигур пока не передаётся.
fn write_shape(content: &mut Content, shape: &Shape, [x, y, w, h]: [f32; 4], height: f32) {
    let point = |[px, py]: [f32; 2]| (px / SCALE, height - py / SCALE);
    for path in shape_paths(shape, x, y, w, h, SCALE) {
        if path.fill.is_none() && path.stroke.is_none() {
            continue;
        }
        content.save_state();
        if let Some([r, g, b, _]) = path.fill {
            content.set_fill_rgb(unit(r), unit(g), unit(b));
        }
        if let Some(stroke) = &path.stroke {
            let [r, g, b, _] = stroke.color;
            content.set_stroke_rgb(unit(r), unit(g), unit(b));
            content.set_line_width(stroke.width / SCALE);
            if !stroke.dash.is_empty() {
                content.set_dash_pattern(stroke.dash.iter().map(|d| d / SCALE), 0.0);
            }
        }
        for segment in &path.segments {
            match *segment {
                Segment::MoveTo(p) => {
                    let (x, y) = point(p);
                    content.move_to(x, y);
                }
                Segment::LineTo(p) => {
                    let (x, y) = point(p);
                    content.line_to(x, y);
                }
                Segment::CubicTo(a, b, c) => {
                    let ((x1, y1), (x2, y2), (x3, y3)) = (point(a), point(b), point(c));
                    content.cubic_to(x1, y1, x2, y2, x3, y3);
                }
                Segment::Close => {
                    content.close_path();
                }
            }
        }
        match (path.fill.is_some(), path.stroke.is_some()) {
            (true, true) => content.fill_nonzero_and_stroke(),
            (true, false) => content.fill_nonzero(),
            _ => content.stroke(),
        };
        content.restore_state();
    }
}

/// Цвет заливки, если он сменился.
fn set_fill(content: &mut Content, color: Rgba, current: &mut Option<Rgba>) {
    let rgb = [color[0], color[1], color[2], 255];
    if *current != Some(rgb) {
        content.set_fill_rgb(unit(color[0]), unit(color[1]), unit(color[2]));
        *current = Some(rgb);
    }
}

fn unit(v: u8) -> f32 {
    f32::from(v) / 255.0
}

/// Шрифт `id` среди встраиваемых (добавляется при первом глифе).
fn embed_font<'a>(
    fonts: &'a mut HashMap<FontId, EmbeddedFont>,
    refs: &mut Refs,
    id: FontId,
) -> &'a mut EmbeddedFont {
    let count = fonts.len();
    fonts.entry(id).or_insert_with(|| {
        let mut remapper = GlyphRemapper::new();
        // Глиф 0 («нет буквы») всегда остаётся нулевым.
        remapper.remap(0);
        EmbeddedFont {
            name: format!("F{count}"),
            id: refs.next(),
            remapper,
        }
    })
}

/// Картинка `data` среди записанных; `None`, если её не удалось
/// распаковать (тогда рисуется заглушка).
fn embed_image(
    images: &mut Vec<(Arc<ImageData>, EmbeddedImage)>,
    data: &Arc<ImageData>,
    pdf: &mut Pdf,
    refs: &mut Refs,
) -> Option<(String, Ref)> {
    if let Some((_, image)) = images
        .iter()
        .find(|(d, _)| Arc::ptr_eq(d, data) || **d == **data)
    {
        return Some((image.name.clone(), image.id));
    }
    let id = refs.next();
    let name = format!("I{}", images.len());
    if is_jpeg(&data.bytes)
        && let Some((width, height, components)) = jpeg_info(&data.bytes)
    {
        let mut image = pdf.image_xobject(id, &data.bytes);
        image.filter(Filter::DctDecode);
        image.width(width as i32).height(height as i32);
        image.bits_per_component(8);
        if components == 1 {
            image.color_space().device_gray();
        } else if components == 4 {
            image.color_space().device_cmyk();
            // Adobe пишет CMYK в JPEG наоборот.
            image.decode([1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0]);
        } else {
            image.color_space().device_rgb();
        }
        image.finish();
    } else {
        let decoded = decode(data)?;
        let (width, height) = (decoded.width as i32, decoded.height as i32);
        let mut rgb = Vec::with_capacity(decoded.pixels.len() / 4 * 3);
        let mut alpha = Vec::with_capacity(decoded.pixels.len() / 4);
        for pixel in decoded.pixels.as_chunks::<4>().0 {
            rgb.extend_from_slice(&pixel[..3]);
            alpha.push(pixel[3]);
        }
        let mask = alpha.iter().any(|&a| a != 255).then(|| {
            let mask_id = refs.next();
            let data = deflate(&alpha);
            let mut mask = pdf.image_xobject(mask_id, &data);
            mask.filter(Filter::FlateDecode);
            mask.width(width).height(height).bits_per_component(8);
            mask.color_space().device_gray();
            mask.finish();
            mask_id
        });
        let data = deflate(&rgb);
        let mut image = pdf.image_xobject(id, &data);
        image.filter(Filter::FlateDecode);
        image.width(width).height(height).bits_per_component(8);
        image.color_space().device_rgb();
        if let Some(mask) = mask {
            image.s_mask(mask);
        }
        image.finish();
    }
    images.push((
        data.clone(),
        EmbeddedImage {
            name: name.clone(),
            id,
        },
    ));
    Some((name, id))
}

/// Заглушка вместо картинки, которую не удалось распаковать: серый
/// прямоугольник с крестом (как на экране).
fn write_placeholder(pdf: &mut Pdf, refs: &mut Refs) -> EmbeddedImage {
    let id = refs.next();
    let picture = crate::picture::placeholder(64, 64);
    let rgb: Vec<u8> = picture
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    let data = deflate(&rgb);
    let mut image = pdf.image_xobject(id, &data);
    image.filter(Filter::FlateDecode);
    image.width(64).height(64).bits_per_component(8);
    image.color_space().device_rgb();
    image.finish();
    EmbeddedImage {
        name: "Placeholder".into(),
        id,
    }
}

fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8])
}

/// Ширина, высота и число каналов JPEG по заголовку кадра (SOFn).
fn jpeg_info(bytes: &[u8]) -> Option<(u32, u32, u8)> {
    let mut at = 2;
    while at + 4 <= bytes.len() {
        if bytes[at] != 0xFF {
            return None;
        }
        let marker = bytes[at + 1];
        let length = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        let is_frame = matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_frame {
            let frame = bytes.get(at + 4..at + 4 + 6)?;
            let height = u32::from(u16::from_be_bytes([frame[1], frame[2]]));
            let width = u32::from(u16::from_be_bytes([frame[3], frame[4]]));
            let components = frame[5];
            return (width > 0 && height > 0 && matches!(components, 1 | 3 | 4))
                .then_some((width, height, components));
        }
        at += 2 + length;
    }
    None
}

/// Ссылки страницы: прямоугольники кусков текста с гиперссылкой.
fn page_links(
    page: &Page,
    index: usize,
    page_ids: &[Ref],
    info: &PdfInfo,
    pdf: &mut Pdf,
    refs: &mut Refs,
) -> Vec<Ref> {
    let _ = index;
    // Куски строк со ссылками; соседние куски с одной ссылкой в строке —
    // один прямоугольник.
    let mut areas: Vec<(Hyperlink, [f32; 4])> = Vec::new();
    let notes = page.notes.iter().flat_map(|n| &n.story.lines);
    let lines = page
        .header
        .lines
        .iter()
        .chain(&page.footer.lines)
        .chain(notes)
        .chain(&page.lines);
    for line in lines {
        let top = line.baseline - line.line.ascent;
        let bottom = line.baseline + line.line.descent;
        let mut current: Option<(Hyperlink, [f32; 4])> = None;
        for item in &line.line.items {
            let LineItem::Text(text) = item else {
                continue;
            };
            let Some(link) = line.spans.get(text.span).and_then(|s| s.link.clone()) else {
                if let Some(done) = current.take() {
                    areas.push(done);
                }
                continue;
            };
            let left = line.x + text.x;
            let right = left + text.run.width();
            match &mut current {
                Some((l, rect)) if *l == link => {
                    rect[0] = rect[0].min(left);
                    rect[2] = rect[2].max(right);
                }
                _ => {
                    if let Some(done) = current.take() {
                        areas.push(done);
                    }
                    current = Some((link, [left, top, right, bottom]));
                }
            }
        }
        if let Some(done) = current {
            areas.push(done);
        }
    }
    let mut out = Vec::new();
    for (link, [left, top, right, bottom]) in areas {
        let target = match (&link.target, &link.anchor) {
            (Some(uri), _) if !uri.is_empty() => Some(Err(uri.clone())),
            (_, Some(anchor)) => info
                .anchors
                .get(&anchor.to_lowercase())
                .map(|&(p, y)| Ok((p, y))),
            _ => None,
        };
        let Some(target) = target else {
            continue;
        };
        let id = refs.next();
        let mut annotation = pdf.annotation(id);
        annotation
            .subtype(AnnotationType::Link)
            .rect(Rect::new(
                left,
                page.height - bottom,
                right,
                page.height - top,
            ))
            .border(0.0, 0.0, 0.0, None);
        match target {
            Err(uri) => {
                annotation
                    .action()
                    .action_type(ActionType::Uri)
                    .uri(Str(uri.as_bytes()));
            }
            Ok((target_page, y)) => {
                let Some(&target_ref) = page_ids.get(target_page) else {
                    continue;
                };
                let height = page.height;
                let dest: Destination = annotation.insert(Name(b"Dest")).start();
                dest.page(target_ref).xyz(0.0, height - y, None);
            }
        }
        annotation.finish();
        out.push(id);
    }
    out
}

/// Оглавление PDF; номер его корня, если пункты есть.
fn write_outline(
    pdf: &mut Pdf,
    refs: &mut Refs,
    items: &[OutlineItem],
    page_ids: &[Ref],
    pages: &[Page],
) -> Option<Ref> {
    let items: Vec<&OutlineItem> = items.iter().filter(|i| i.page < page_ids.len()).collect();
    if items.is_empty() {
        return None;
    }
    let root = refs.next();
    let ids: Vec<Ref> = items.iter().map(|_| refs.next()).collect();
    // Родитель каждого пункта: ближайший предыдущий пункт с меньшим
    // уровнем (или корень).
    let mut parents: Vec<Option<usize>> = Vec::with_capacity(items.len());
    let mut stack: Vec<usize> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        while stack.last().is_some_and(|&p| items[p].level >= item.level) {
            stack.pop();
        }
        parents.push(stack.last().copied());
        stack.push(i);
    }
    let children = |parent: Option<usize>| -> Vec<usize> {
        (0..items.len()).filter(|&i| parents[i] == parent).collect()
    };
    // Сколько всего потомков (все пункты показываются раскрытыми).
    let descendants = |index: usize| -> i32 {
        let mut count = 0;
        let mut i = index + 1;
        while i < items.len() && items[i].level > items[index].level {
            count += 1;
            i += 1;
        }
        count
    };
    let top = children(None);
    let mut outline = pdf.outline(root);
    if let (Some(&first), Some(&last)) = (top.first(), top.last()) {
        outline.first(ids[first]).last(ids[last]);
    }
    outline.count(items.len() as i32);
    outline.finish();
    for (i, item) in items.iter().enumerate() {
        let siblings = children(parents[i]);
        let position = siblings.iter().position(|&s| s == i).unwrap_or(0);
        let mut writer = pdf.outline_item(ids[i]);
        writer
            .title(TextStr(&item.title))
            .parent(parents[i].map_or(root, |p| ids[p]));
        if position > 0 {
            writer.prev(ids[siblings[position - 1]]);
        }
        if let Some(&next) = siblings.get(position + 1) {
            writer.next(ids[next]);
        }
        let own = children(Some(i));
        if let (Some(&first), Some(&last)) = (own.first(), own.last()) {
            writer.first(ids[first]).last(ids[last]);
            writer.count(descendants(i));
        }
        let height = pages.get(item.page).map_or(0.0, |p| p.height);
        writer
            .dest()
            .page(page_ids[item.page])
            .xyz(0.0, height - item.top, None);
        writer.finish();
    }
    Some(root)
}

/// Записывает шрифт: Type0 с подмножеством глифов, ширинами и
/// таблицей «глиф → буква».
fn write_font(
    pdf: &mut Pdf,
    refs: &mut Refs,
    library: &FontLibrary,
    id: FontId,
    font: &EmbeddedFont,
) {
    let type0 = font.id;
    let cid_ref = refs.next();
    let descriptor_ref = refs.next();
    let file_ref = refs.next();
    let cmap_ref = refs.next();
    let written = library.with_face_data(id, |bytes, index| {
        let face = ttf_parser::Face::parse(bytes, index).ok()?;
        let subset = subsetter::subset(bytes, index, &font.remapper).ok()?;
        Some(FontFacts::of(&face, &subset, &font.remapper))
    });
    let Some(Some(facts)) = written else {
        return;
    };
    let base = format!("{}+{}", subset_tag(&font.remapper), facts.postscript_name);
    let base = Name(base.as_bytes());

    pdf.type0_font(type0)
        .base_font(base)
        .encoding_predefined(Name(b"Identity-H"))
        .descendant_font(cid_ref)
        .to_unicode(cmap_ref);

    let mut cid = pdf.cid_font(cid_ref);
    cid.subtype(if facts.cff {
        CidFontType::Type0
    } else {
        CidFontType::Type2
    });
    cid.base_font(base);
    cid.system_info(SystemInfo {
        registry: Str(b"Adobe"),
        ordering: Str(b"Identity"),
        supplement: 0,
    });
    cid.font_descriptor(descriptor_ref);
    cid.default_width(0.0);
    if !facts.cff {
        cid.cid_to_gid_map_predefined(Name(b"Identity"));
    }
    let mut widths = cid.widths();
    for (gid, width) in &facts.widths {
        widths.consecutive(*gid, [*width]);
    }
    widths.finish();
    cid.finish();

    let mut descriptor = pdf.font_descriptor(descriptor_ref);
    let mut flags = FontFlags::SYMBOLIC;
    if facts.italic_angle != 0.0 {
        flags |= FontFlags::ITALIC;
    }
    if facts.serif {
        flags |= FontFlags::SERIF;
    }
    if facts.monospaced {
        flags |= FontFlags::FIXED_PITCH;
    }
    descriptor
        .name(base)
        .flags(flags)
        .bbox(facts.bbox)
        .italic_angle(facts.italic_angle)
        .ascent(facts.ascent)
        .descent(facts.descent)
        .cap_height(facts.cap_height)
        .stem_v(80.0);
    if facts.cff {
        descriptor.font_file3(file_ref);
    } else {
        descriptor.font_file2(file_ref);
    }
    descriptor.finish();

    let data = deflate(&facts.file);
    let mut file = pdf.stream(file_ref, &data);
    file.filter(Filter::FlateDecode);
    if facts.cff {
        file.pair(Name(b"Subtype"), Name(b"CIDFontType0C"));
    }
    file.finish();

    let mut cmap = pdf_writer::types::UnicodeCmap::new(
        Name(b"Custom"),
        SystemInfo {
            registry: Str(b"Adobe"),
            ordering: Str(b"UCS"),
            supplement: 0,
        },
    );
    for (gid, c) in &facts.unicode {
        cmap.pair(*gid, *c);
    }
    let cmap = cmap.finish();
    pdf.cmap(cmap_ref, &cmap);
}

/// Что нужно знать о шрифте для PDF.
struct FontFacts {
    postscript_name: String,
    /// Файл подмножества: целиком (TrueType) или только таблица CFF.
    file: Vec<u8>,
    cff: bool,
    /// Новые номера глифов и их ширины в тысячных кегля.
    widths: Vec<(u16, f32)>,
    /// Новые номера глифов и их буквы.
    unicode: Vec<(u16, char)>,
    bbox: Rect,
    italic_angle: f32,
    ascent: f32,
    descent: f32,
    cap_height: f32,
    serif: bool,
    monospaced: bool,
}

impl FontFacts {
    fn of(face: &ttf_parser::Face, subset: &[u8], remapper: &GlyphRemapper) -> FontFacts {
        let units = f32::from(face.units_per_em().max(1));
        let scale = |v: f32| v * 1000.0 / units;
        let postscript_name = face
            .names()
            .into_iter()
            .find(|n| n.name_id == ttf_parser::name_id::POST_SCRIPT_NAME)
            .and_then(|n| n.to_string())
            .unwrap_or_else(|| "Font".into())
            .chars()
            .filter(|c| c.is_ascii_graphic() && !"[](){}<>/%#".contains(*c))
            .collect::<String>();
        let cff = face.tables().cff.is_some();
        let file = if cff {
            ttf_parser::RawFace::parse(subset, 0)
                .ok()
                .and_then(|raw| raw.table(ttf_parser::Tag::from_bytes(b"CFF ")))
                .map(<[u8]>::to_vec)
                .unwrap_or_default()
        } else {
            subset.to_vec()
        };
        let mut widths: Vec<(u16, f32)> = remapper
            .remapped_gids()
            .filter_map(|old| {
                let new = remapper.get(old)?;
                let advance = face.glyph_hor_advance(ttf_parser::GlyphId(old))?;
                Some((new, scale(f32::from(advance))))
            })
            .collect();
        widths.sort_by_key(|(gid, _)| *gid);
        // Буквы глифов — обратным проходом по таблице cmap.
        let mut letters: HashMap<u16, char> = HashMap::new();
        if let Some(cmap) = face.tables().cmap {
            for table in cmap.subtables {
                if !table.is_unicode() {
                    continue;
                }
                table.codepoints(|code| {
                    let Some(c) = char::from_u32(code) else {
                        return;
                    };
                    if let Some(glyph) = table.glyph_index(code) {
                        letters
                            .entry(glyph.0)
                            .and_modify(|old| {
                                // Обычная буква лучше знака из личной области.
                                if is_private(*old) && !is_private(c) {
                                    *old = c;
                                }
                            })
                            .or_insert(c);
                    }
                });
            }
        }
        let mut unicode: Vec<(u16, char)> = remapper
            .remapped_gids()
            .filter_map(|old| Some((remapper.get(old)?, *letters.get(&old)?)))
            .filter(|(gid, _)| *gid != 0)
            .collect();
        unicode.sort_by_key(|(gid, _)| *gid);
        let bbox = face.global_bounding_box();
        let cap_height = face.capital_height().unwrap_or(face.ascender());
        FontFacts {
            postscript_name: if postscript_name.is_empty() {
                "Font".into()
            } else {
                postscript_name
            },
            file,
            cff,
            widths,
            unicode,
            bbox: Rect::new(
                scale(f32::from(bbox.x_min)),
                scale(f32::from(bbox.y_min)),
                scale(f32::from(bbox.x_max)),
                scale(f32::from(bbox.y_max)),
            ),
            italic_angle: face.italic_angle(),
            ascent: scale(f32::from(face.ascender())),
            descent: scale(f32::from(face.descender())),
            cap_height: scale(f32::from(cap_height)),
            serif: false,
            monospaced: face.is_monospaced(),
        }
    }
}

fn is_private(c: char) -> bool {
    matches!(c as u32, 0xE000..=0xF8FF)
}

/// Метка подмножества шрифта: шесть заглавных латинских букв,
/// зависящих от набора глифов.
fn subset_tag(remapper: &GlyphRemapper) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for gid in remapper.remapped_gids() {
        for byte in gid.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
    }
    (0..6)
        .map(|i| char::from(b'A' + ((hash >> (i * 5)) % 26) as u8))
        .collect()
}

/// Сжатие потока (FlateDecode).
fn deflate(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_size_from_frame_header() {
        // SOI, APP0 (длина 4), SOF0: точность 8, высота 2, ширина 3, 3 канала.
        let jpeg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00,
            0x02, 0x00, 0x03, 0x03,
        ];
        assert_eq!(jpeg_info(&jpeg), Some((3, 2, 3)));
        assert_eq!(jpeg_info(&[0xFF, 0xD8, 0x00]), None);
        assert!(is_jpeg(&jpeg) && !is_jpeg(b"\x89PNG"));
    }

    #[test]
    fn empty_document_is_still_a_pdf() {
        let bytes = write_pdf(&[], &FontLibrary::empty(), &PdfInfo::default());
        assert!(bytes.starts_with(b"%PDF-"));
    }
}
