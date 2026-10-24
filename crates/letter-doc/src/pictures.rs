//! Картинки (MS-DOC 2.9.192 `PICFAndOfficeArtData`, MS-ODRAW). Встроенная
//! в строку картинка — знак 0x01 со ссылкой в поток `Data`: там заголовок
//! `PICF` (размер и масштаб) и записи OfficeArt с самой картинкой.
//! Плавающая — знак 0x08 и запись `FSPA` (место и обтекание), а картинка
//! лежит в общем хранилище рисунков документа (`OfficeArtContent`).

use std::collections::HashMap;
use std::sync::Arc;

use letter_model::{
    Anchor, AnchorAlign, AnchorPosition, Block, Distance, Frame, HorizontalFrom, ImageData,
    Picture, Placement, VerticalFrom, Wrap,
};

use crate::fib::{i16_at, u16_at, u32_at};
use crate::shapes::{self, ShapeRecord};

/// EMU в одном twip (1/20 пункта).
const EMU_PER_TWIP: i64 = 635;

/// Размер заголовка `PICF`.
const PICF_SIZE: usize = 68;

/// Заголовок записи OfficeArt.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Record {
    /// Версия: 0xF — контейнер с записями внутри.
    pub version: u16,
    /// Номер варианта (у картинок — вид и число меток).
    pub instance: u16,
    pub kind: u16,
    /// Начало данных записи.
    pub start: usize,
    /// Конец данных записи.
    pub end: usize,
}

/// Записи подряд в `bytes[from..to]`; испорченная запись обрывает список.
pub(crate) fn records(bytes: &[u8], from: usize, to: usize) -> Vec<Record> {
    let to = to.min(bytes.len());
    let mut out = Vec::new();
    let mut at = from;
    while at + 8 <= to {
        let (Some(head), Some(kind), Some(len)) = (
            u16_at(bytes, at),
            u16_at(bytes, at + 2),
            u32_at(bytes, at + 4),
        ) else {
            break;
        };
        let start = at + 8;
        let Some(end) = start.checked_add(len as usize).filter(|&e| e <= to) else {
            break;
        };
        out.push(Record {
            version: head & 0x0F,
            instance: head >> 4,
            kind,
            start,
            end,
        });
        at = end;
    }
    out
}

/// Файл картинки: байты и расширение.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Blip {
    pub bytes: Vec<u8>,
    pub extension: &'static str,
}

/// Картинка из записи `OfficeArtBlip*`.
fn blip(bytes: &[u8], record: &Record) -> Option<Blip> {
    let data = bytes.get(record.start..record.end)?;
    // Одна или две метки по 16 байт (вторая — у нечётного варианта).
    let uids = |single: u16| if record.instance == single { 16 } else { 32 };
    let raster = |single: u16, extension| {
        Some(Blip {
            bytes: data.get(uids(single) + 1..)?.to_vec(),
            extension,
        })
    };
    match record.kind {
        0xF01D | 0xF02A => raster(
            if record.instance >= 0x6E2 {
                0x6E2
            } else {
                0x46A
            },
            "jpeg",
        ),
        0xF01E => raster(0x6E0, "png"),
        0xF029 => raster(0x6E4, "tiff"),
        0xF01F => {
            // Растр без заголовка файла BMP: дописываем его.
            let dib = data.get(uids(0x7A8) + 1..)?;
            let header = u32_at(dib, 0)?;
            let bits = u16_at(dib, 14).unwrap_or(24);
            let colors = match u32_at(dib, 32).unwrap_or(0) {
                0 if bits <= 8 => 1u32 << bits,
                n => n,
            };
            let offset = 14 + header + colors.min(256) * 4;
            let mut bmp = b"BM".to_vec();
            bmp.extend((14 + dib.len() as u32).to_le_bytes());
            bmp.extend([0; 4]);
            bmp.extend(offset.to_le_bytes());
            bmp.extend(dib);
            Some(Blip {
                bytes: bmp,
                extension: "bmp",
            })
        }
        0xF01A | 0xF01B => {
            let header = uids(if record.kind == 0xF01A { 0x3D4 } else { 0x216 });
            let meta = data.get(header..header + 34)?;
            let saved = u32_at(meta, 28)? as usize;
            let packed = data.get(header + 34..header + 34 + saved)?;
            let mut bytes = match meta[32] {
                0 => miniz_oxide::inflate::decompress_to_vec_zlib(packed).ok()?,
                _ => packed.to_vec(),
            };
            if record.kind == 0xF01B {
                bytes = placeable_wmf(meta, bytes);
            }
            Some(Blip {
                bytes,
                extension: if record.kind == 0xF01A { "emf" } else { "wmf" },
            })
        }
        _ => None,
    }
}

/// WMF с заголовком «размещаемого» метафайла, как в файлах .wmf: рамка
/// из заголовка записи, 1440 единиц на дюйм.
fn placeable_wmf(meta: &[u8], body: Vec<u8>) -> Vec<u8> {
    let side =
        |i: usize| i32::from_le_bytes(meta[4 + i * 4..8 + i * 4].try_into().unwrap_or_default());
    let mut out = 0x9AC6_CDD7u32.to_le_bytes().to_vec();
    out.extend([0, 0]);
    for i in 0..4 {
        out.extend((side(i).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16).to_le_bytes());
    }
    out.extend(1440u16.to_le_bytes());
    out.extend([0; 4]);
    let checksum = out
        .chunks(2)
        .fold(0u16, |sum, w| sum ^ u16::from_le_bytes([w[0], w[1]]));
    out.extend(checksum.to_le_bytes());
    out.extend(body);
    out
}

/// Картинка из записи `OfficeArtFBSE` (описание в хранилище): своя внутри
/// записи или по смещению в потоке `WordDocument` (`delay`).
fn bse_blip(bytes: &[u8], record: &Record, delay: &[u8]) -> Option<Blip> {
    let data = bytes.get(record.start..record.end)?;
    let name_len = usize::from(*data.get(33)?);
    let inside = record.start + 36 + name_len;
    if let Some(inner) = records(bytes, inside, record.end).first() {
        return blip(bytes, inner);
    }
    let offset = u32_at(data, 28)? as usize;
    let inner = records(delay, offset, delay.len()).into_iter().next()?;
    blip(delay, &inner)
}

/// Первая картинка среди записей (с заходом в контейнеры).
fn find_blip(bytes: &[u8], from: usize, to: usize, delay: &[u8]) -> Option<Blip> {
    for record in records(bytes, from, to) {
        let found = match record.kind {
            0xF007 => bse_blip(bytes, &record, delay),
            0xF018..=0xF117 => blip(bytes, &record),
            _ if record.version == 0xF => find_blip(bytes, record.start, record.end, delay),
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

/// Нумерует файлы картинок: `word/media/image1.png` и т. д.
#[derive(Debug, Default)]
pub(crate) struct Media {
    count: std::cell::Cell<usize>,
}

impl Media {
    fn image(&self, blip: Blip) -> Arc<ImageData> {
        let n = self.count.get() + 1;
        self.count.set(n);
        Arc::new(ImageData {
            name: format!("word/media/image{n}.{}", blip.extension),
            bytes: blip.bytes,
            chart: None,
        })
    }
}

/// Картинка или фигура в строке по смещению `offset` в потоке `Data`;
/// `text` — текст надписи по номеру фигуры.
pub(crate) fn inline(
    data: &[u8],
    offset: usize,
    media: &Media,
    text: &dyn Fn(u32) -> Vec<Block>,
) -> Option<Picture> {
    let picf = data.get(offset..offset + PICF_SIZE)?;
    let size = u32_at(picf, 0)? as usize;
    let header = usize::from(u16_at(picf, 4)?).max(PICF_SIZE);
    let mm = u16_at(picf, 6)?;
    let end = offset.checked_add(size)?.min(data.len());
    let mut at = offset + header;
    // MM_SHAPEFILE: после заголовка — имя картинки.
    if mm == 0x66 {
        at += 1 + usize::from(*data.get(at)?);
    }
    let goal = |i: usize| i64::from(i16_at(picf, i).unwrap_or(0));
    let scale = |i: usize| i64::from(u16_at(picf, i).unwrap_or(1000));
    let crop = |a: usize, b: usize| goal(a) + goal(b);
    let width = (goal(28) - crop(36, 40)).max(0) * scale(32) / 1000 * EMU_PER_TWIP;
    let height = (goal(30) - crop(38, 42)).max(0) * scale(34) / 1000 * EMU_PER_TWIP;
    if let Some(blip) = find_blip(data, at, end, &[]) {
        return Some(picture(width, height, Some(media.image(blip)), None));
    }
    // Фигура без картинки; пустая рамка картинки (вид 75) — не фигура.
    let record = shapes::shapes_in(data, at, end)
        .into_iter()
        .next()
        .filter(|r| r.kind != 75)?;
    let mut picture = picture(width, height, None, None);
    picture.shape = Some(Arc::new(shapes::shape(&record, text)));
    Some(picture)
}

fn picture(
    width: i64,
    height: i64,
    image: Option<Arc<ImageData>>,
    anchor: Option<Anchor>,
) -> Picture {
    Picture {
        width,
        height,
        image,
        relationship: None,
        raw: None,
        anchor: anchor.map(Box::new),
        shape: None,
        drawing: None,
    }
}

/// Рисунки документа (`OfficeArtContent` в табличном потоке): картинки
/// хранилища по порядку и номер картинки у каждой фигуры.
#[derive(Debug, Default)]
pub(crate) struct Drawings {
    /// Картинки хранилища (`pib` — номер с 1).
    blips: Vec<Option<Blip>>,
    /// Фигуры по номеру (`spid`).
    shapes: HashMap<u32, ShapeRecord>,
}

impl Drawings {
    /// Разбирает `OfficeArtContent`. `word` — поток `WordDocument`, где
    /// лежат картинки хранилища.
    pub fn read(content: &[u8], word: &[u8]) -> Drawings {
        let mut drawings = Drawings::default();
        for record in records(content, 0, content.len()) {
            if record.kind == 0xF000 {
                for store in records(content, record.start, record.end) {
                    if store.kind == 0xF001 {
                        drawings.blips = records(content, store.start, store.end)
                            .iter()
                            .map(|bse| bse_blip(content, bse, word))
                            .collect();
                    }
                }
            }
        }
        // За контейнером хранилища — рисунки основного текста и
        // колонтитулов: байт «чей» и контейнер `OfficeArtDgContainer`.
        let mut at = records(content, 0, content.len())
            .first()
            .map_or(content.len(), |r| r.end);
        while at < content.len() {
            let rest = records(content, at + 1, content.len());
            let Some(dg) = rest.first() else {
                break;
            };
            drawings.collect_shapes(content, dg.start, dg.end);
            at = dg.end;
        }
        drawings
    }

    /// Фигуры среди записей; у группы её фигуры лежат внутри неё.
    fn collect_shapes(&mut self, bytes: &[u8], from: usize, to: usize) {
        for shape in shapes::shapes_in(bytes, from, to) {
            self.shapes.insert(shape.spid, shape);
        }
    }

    /// Плавающая картинка или фигура по записи `FSPA` (26 байт); `text`
    /// — текст надписи по номеру фигуры.
    pub fn floating(
        &self,
        fspa: &[u8],
        media: &Media,
        text: &dyn Fn(u32) -> Vec<Block>,
    ) -> Option<Picture> {
        let spid = u32_at(fspa, 0)?;
        let side = |i: usize| i64::from(u32_at(fspa, 4 + i * 4).map_or(0, |v| v as i32));
        let (left, top, right, bottom) = (side(0), side(1), side(2), side(3));
        let flags = u16_at(fspa, 20)?;
        let horizontal = match (flags >> 1) & 3 {
            0 => HorizontalFrom::Margin,
            1 => HorizontalFrom::Page,
            _ => HorizontalFrom::Column,
        };
        let vertical = match (flags >> 3) & 3 {
            0 => VerticalFrom::Margin,
            1 => VerticalFrom::Page,
            _ => VerticalFrom::Paragraph,
        };
        let wrap = match (flags >> 5) & 0x0F {
            1 => Wrap::TopAndBottom,
            2 => Wrap::Square,
            4 => Wrap::Tight,
            5 => Wrap::Through,
            _ => Wrap::None,
        };
        let record = self.shapes.get(&spid)?;
        let prop = |id: u16| record.prop(id);
        // Word 2000 и новее пишет точнее: от чего отсчитывать
        // (`posrelh`, `posrelv`) и выравнивание (`posh`, `posv`).
        let horizontal = match prop(0x0390) {
            Some(0) => HorizontalFrom::Margin,
            Some(1) => HorizontalFrom::Page,
            Some(2) => HorizontalFrom::Column,
            Some(3) => HorizontalFrom::Character,
            _ => horizontal,
        };
        let vertical = match prop(0x0392) {
            Some(0) => VerticalFrom::Margin,
            Some(1) => VerticalFrom::Page,
            Some(2) => VerticalFrom::Paragraph,
            Some(3) => VerticalFrom::Line,
            _ => vertical,
        };
        let placement = |align: Option<u32>, offset: i64| match align {
            Some(1) => Placement::Align(AnchorAlign::Start),
            Some(2) => Placement::Align(AnchorAlign::Center),
            Some(3) => Placement::Align(AnchorAlign::End),
            Some(4) => Placement::Align(AnchorAlign::Inside),
            Some(5) => Placement::Align(AnchorAlign::Outside),
            _ => Placement::Offset(offset * EMU_PER_TWIP),
        };
        // Расстояния до текста, EMU; по умолчанию 1/8 дюйма слева и справа.
        let distance = |id: u16, default: i64| prop(id).map_or(default, |v| i64::from(v as i32));
        let anchor = Anchor {
            horizontal: AnchorPosition {
                relative: horizontal,
                placement: placement(prop(0x038F), left),
            },
            vertical: AnchorPosition {
                relative: vertical,
                placement: placement(prop(0x0391), top),
            },
            wrap,
            behind_text: flags & 0x4000 != 0,
            distance: Distance {
                left: distance(0x0384, 114_300),
                top: distance(0x0385, 0),
                right: distance(0x0386, 114_300),
                bottom: distance(0x0387, 0),
            },
        };
        // Повёрнутая почти поперёк фигура записана повёрнутым
        // прямоугольником.
        let rotation = shapes::rotation(record);
        let frame = shapes::rotated_frame(
            Frame {
                x: left * EMU_PER_TWIP,
                y: top * EMU_PER_TWIP,
                width: (right - left).max(0) * EMU_PER_TWIP,
                height: (bottom - top).max(0) * EMU_PER_TWIP,
            },
            rotation,
        );
        let shift = |placement: &mut Placement, by: i64| {
            if let Placement::Offset(offset) = placement {
                *offset += by;
            }
        };
        let mut anchor = anchor;
        shift(
            &mut anchor.horizontal.placement,
            frame.x - left * EMU_PER_TWIP,
        );
        shift(&mut anchor.vertical.placement, frame.y - top * EMU_PER_TWIP);
        // Фигура «как знак» (так LibreOffice пишет фигуры в строке):
        // от знака и строки без сдвига — значит, в строке.
        let as_character = prop(0x0390) == Some(3)
            && prop(0x0392) == Some(3)
            && anchor.horizontal.placement == Placement::Offset(0)
            && anchor.vertical.placement == Placement::Offset(0);
        let blip =
            prop(0x0104).and_then(|pib| self.blips.get((pib as usize).checked_sub(1)?)?.clone());
        let mut picture = picture(
            frame.width,
            frame.height,
            blip.map(|b| media.image(b)),
            (!as_character).then_some(anchor),
        );
        if picture.image.is_none() {
            picture.shape = Some(Arc::new(shapes::shape(record, text)));
        }
        Some(picture)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(version: u16, instance: u16, kind: u16, data: &[u8]) -> Vec<u8> {
        let mut out = (version | instance << 4).to_le_bytes().to_vec();
        out.extend(kind.to_le_bytes());
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        out
    }

    fn png_blip() -> Vec<u8> {
        let mut data = vec![0; 17];
        data.extend(b"\x89PNG-data");
        record(0, 0x6E0, 0xF01E, &data)
    }

    #[test]
    fn inline_picture_size_and_bytes() {
        let mut picf = vec![0; PICF_SIZE];
        let shape = record(0xF, 0, 0xF004, &[]);
        let total = PICF_SIZE + shape.len() + png_blip().len();
        picf[..4].copy_from_slice(&(total as u32).to_le_bytes());
        picf[4..6].copy_from_slice(&(PICF_SIZE as u16).to_le_bytes());
        picf[6..8].copy_from_slice(&0x64u16.to_le_bytes());
        picf[28..30].copy_from_slice(&1440i16.to_le_bytes()); // дюйм
        picf[30..32].copy_from_slice(&720i16.to_le_bytes());
        picf[32..34].copy_from_slice(&500u16.to_le_bytes()); // 50 %
        picf[34..36].copy_from_slice(&1000u16.to_le_bytes());
        let mut data = vec![0xAA; 3]; // картинка не с начала потока
        data.extend(picf);
        data.extend(shape);
        data.extend(png_blip());
        let media = Media::default();
        let picture = inline(&data, 3, &media, &|_| Vec::new()).unwrap();
        assert_eq!(picture.width, 720 * EMU_PER_TWIP);
        assert_eq!(picture.height, 720 * EMU_PER_TWIP);
        let image = picture.image.unwrap();
        assert_eq!(image.name, "word/media/image1.png");
        assert_eq!(image.bytes, b"\x89PNG-data");
        // Обрезанный поток — не падение.
        for len in 0..data.len() {
            let _ = inline(&data[..len], 3, &media, &|_| Vec::new());
        }
    }

    #[test]
    fn floating_picture_from_store() {
        // Хранилище с одной картинкой внутри описания, рисунок с фигурой 7.
        let mut bse = vec![0; 36];
        bse.extend(png_blip());
        let store = record(0xF, 1, 0xF001, &record(2, 6, 0xF007, &bse));
        let mut content = record(0xF, 0, 0xF000, &store);
        let fsp = record(2, 0, 0xF00A, &[7, 0, 0, 0, 0, 0, 0, 0]);
        let mut opt = 0x4104u16.to_le_bytes().to_vec();
        opt.extend(1u32.to_le_bytes());
        let opt = record(3, 1, 0xF00B, &opt);
        let sp = record(0xF, 0, 0xF004, &[fsp, opt].concat());
        let dg = record(0xF, 0, 0xF002, &record(0xF, 0, 0xF003, &sp));
        content.push(0);
        content.extend(dg);
        let drawings = Drawings::read(&content, &[]);

        let mut fspa = 7u32.to_le_bytes().to_vec();
        for v in [1440i32, 0, 2880, 720] {
            fspa.extend(v.to_le_bytes());
        }
        fspa.extend((1u16 << 1 | 2 << 5 | 0x4000).to_le_bytes());
        fspa.extend([0; 4]);
        let media = Media::default();
        let picture = drawings.floating(&fspa, &media, &|_| Vec::new()).unwrap();
        assert_eq!(picture.width, 1440 * EMU_PER_TWIP);
        assert_eq!(picture.height, 720 * EMU_PER_TWIP);
        assert_eq!(picture.image.unwrap().bytes, b"\x89PNG-data");
        let anchor = picture.anchor.unwrap();
        assert_eq!(anchor.horizontal.relative, HorizontalFrom::Page);
        assert_eq!(
            anchor.horizontal.placement,
            Placement::Offset(1440 * EMU_PER_TWIP)
        );
        assert_eq!(anchor.wrap, Wrap::Square);
        assert!(anchor.behind_text);
    }
}
