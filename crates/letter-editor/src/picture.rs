//! Рисунки в редакторе: вставка, выделение щелчком и изменение размера.
//!
//! Рисунок в тексте — один знак ([`OBJECT_CHAR`]), поэтому выделенный
//! рисунок — это выделение ровно этого знака. Как в Word, щелчок по
//! рисунку выделяет его, а за маркеры на рамке можно менять размер.

use letter_layout::{DocumentLayout, Float, LineItem};
use letter_model::{Document, Inline, OBJECT_CHAR, Picture, RunContent};

use crate::edit::{clamp_offset, insert_items, paragraph_mut, paragraph_ref, plain_text};
use crate::position::Position;
use crate::selection::Selection;

/// EMU в твипе.
const EMU_PER_TWIP: i64 = 635;
/// Самый маленький размер рисунка при изменении мышью, EMU (1 пт).
pub const MIN_PICTURE_SIZE: i64 = 12_700;

/// Рисунок на странице: где он и где его знак в тексте.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PictureBox {
    /// Номер страницы, с нуля.
    pub page: usize,
    /// Знак рисунка в тексте.
    pub at: Position,
    /// Левый край, пт от левого края листа.
    pub left: f32,
    /// Верх, пт от верхнего края листа.
    pub top: f32,
    /// Ширина, пт.
    pub width: f32,
    /// Высота, пт.
    pub height: f32,
}

impl PictureBox {
    /// Лежит ли точка (пт от угла листа) на рисунке.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        (self.left..=self.left + self.width).contains(&x)
            && (self.top..=self.top + self.height).contains(&y)
    }

    /// Выделение, в котором выделен только этот рисунок.
    pub fn selection(&self) -> Selection {
        let end = Position::new(self.at.paragraph, self.at.offset + OBJECT_CHAR.len_utf8());
        Selection::new(self.at, end)
    }
}

/// Все рисунки на странице `page` в порядке «кто сверху»: сначала
/// плавающие перед текстом, потом рисунки в строке, потом плавающие
/// за текстом.
pub fn pictures_on_page(layout: &DocumentLayout, page: usize) -> Vec<PictureBox> {
    let Some(sheet) = layout.pages.get(page) else {
        return Vec::new();
    };
    let float = |f: &Float| PictureBox {
        page,
        at: Position::new(f.paragraph, f.at),
        left: f.x,
        top: f.y,
        width: f.width,
        height: f.height,
    };
    let mut out: Vec<PictureBox> = sheet
        .floats
        .iter()
        .filter(|f| !f.behind)
        .map(float)
        .collect();
    for line in &sheet.lines {
        for item in &line.line.items {
            if let LineItem::Object(object) = item
                && (object.width > 0.0 || object.height > 0.0)
            {
                out.push(PictureBox {
                    page,
                    at: Position::new(line.paragraph, object.at),
                    left: line.x + object.x,
                    top: line.baseline - object.height,
                    width: object.width,
                    height: object.height,
                });
            }
        }
    }
    out.extend(sheet.floats.iter().filter(|f| f.behind).map(float));
    out
}

/// Рисунок под точкой `x`, `y` (пт от угла листа) на странице `page`.
pub fn picture_at_point(
    layout: &DocumentLayout,
    page: usize,
    x: f32,
    y: f32,
) -> Option<PictureBox> {
    pictures_on_page(layout, page)
        .into_iter()
        .find(|p| p.contains(x, y))
}

/// Рисунок, если выделен ровно он один.
pub fn selected_picture(layout: &DocumentLayout, selection: Selection) -> Option<PictureBox> {
    let (start, end) = (selection.start(), selection.end());
    if start.paragraph != end.paragraph || end.offset != start.offset + OBJECT_CHAR.len_utf8() {
        return None;
    }
    (0..layout.pages.len())
        .flat_map(|page| pictures_on_page(layout, page))
        .find(|p| p.at.paragraph == start.paragraph && p.at.offset == start.offset)
}

/// Рисунок, знак которого стоит в позиции `at`.
pub fn picture_ref(doc: &Document, at: Position) -> Option<&Picture> {
    let paragraph = paragraph_ref(doc, at.paragraph)?;
    let mut pos = 0;
    for run in paragraph.runs() {
        for item in &run.content {
            if pos == at.offset
                && let RunContent::Picture(picture) = item
            {
                return Some(picture);
            }
            pos += item.text_len();
        }
    }
    None
}

fn picture_mut(doc: &mut Document, at: Position) -> Option<&mut Picture> {
    let paragraph = paragraph_mut(doc, at.paragraph)?;
    let mut pos = 0;
    for inline in &mut paragraph.content {
        let Inline::Run(run) = inline else { continue };
        for item in &mut run.content {
            let len = item.text_len();
            if pos == at.offset
                && let RunContent::Picture(picture) = item
            {
                return Some(picture);
            }
            pos += len;
        }
    }
    None
}

/// Вставляет рисунок в позицию `at`. Как в Word, рисунок шире текста
/// уменьшается до ширины текста (раздела или ячейки) с сохранением
/// пропорций. Возвращает позицию за рисунком.
pub fn insert_picture(doc: &mut Document, at: Position, mut picture: Picture) -> Position {
    let limit = i64::from(crate::table::text_width(doc, at.paragraph).max(1)) * EMU_PER_TWIP;
    if picture.width > limit && picture.width > 0 {
        picture.height = (picture.height as i128 * limit as i128 / picture.width as i128) as i64;
        picture.width = limit;
    }
    picture.width = picture.width.max(MIN_PICTURE_SIZE);
    picture.height = picture.height.max(MIN_PICTURE_SIZE);
    let Some(paragraph) = paragraph_mut(doc, at.paragraph) else {
        return at;
    };
    let offset = clamp_offset(paragraph, at.offset);
    insert_items(paragraph, offset, vec![RunContent::Picture(picture)]);
    Position::new(at.paragraph, offset + OBJECT_CHAR.len_utf8())
}

/// Меняет размер рисунка в позиции `at` (EMU). `false`, если там нет
/// рисунка.
pub fn resize_picture(doc: &mut Document, at: Position, width: i64, height: i64) -> bool {
    let Some(picture) = picture_mut(doc, at) else {
        return false;
    };
    picture.width = width.max(MIN_PICTURE_SIZE);
    picture.height = height.max(MIN_PICTURE_SIZE);
    true
}

/// Есть ли в абзаце `paragraph` рисунок в позиции `offset`.
pub fn is_picture(doc: &Document, at: Position) -> bool {
    paragraph_ref(doc, at.paragraph)
        .map(plain_text)
        .is_some_and(|t| {
            t.get(at.offset..)
                .is_some_and(|t| t.starts_with(OBJECT_CHAR))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::{Block, Paragraph, Run};

    fn picture(width: i64, height: i64) -> Picture {
        Picture {
            width,
            height,
            image: None,
            relationship: None,
            raw: None,
            anchor: None,
        }
    }

    #[test]
    fn wide_picture_shrinks_to_the_text_width() {
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![Block::from(Paragraph::from_runs([Run::text("ab")]))];
        let limit = i64::from(crate::table::text_width(&doc, 0)) * EMU_PER_TWIP;
        let at = insert_picture(&mut doc, Position::new(0, 1), picture(limit * 2, 1_000_000));
        assert_eq!(at, Position::new(0, 4));
        assert_eq!(doc.plain_text(), "a\u{fffc}b");
        let placed = picture_ref(&doc, Position::new(0, 1)).unwrap();
        assert_eq!((placed.width, placed.height), (limit, 500_000));
        assert!(limit > 5_000_000);
        assert!(is_picture(&doc, Position::new(0, 1)));
        assert!(!is_picture(&doc, Position::new(0, 0)));

        assert!(resize_picture(&mut doc, Position::new(0, 1), 100, 200_000));
        let placed = picture_ref(&doc, Position::new(0, 1)).unwrap();
        assert_eq!((placed.width, placed.height), (MIN_PICTURE_SIZE, 200_000));
        assert!(!resize_picture(&mut doc, Position::new(0, 0), 1, 1));
    }

    #[test]
    fn picture_is_found_on_the_page() {
        let mut doc = Document::new();
        insert_picture(&mut doc, Position::new(0, 0), picture(914_400, 457_200));
        let library = letter_layout::FontLibrary::embedded_only();
        let layout =
            letter_layout::layout_document(&doc, &mut letter_layout::Shaper::new(), &library);
        let boxes = pictures_on_page(&layout, 0);
        let [found] = boxes[..] else {
            panic!("{boxes:?}");
        };
        assert!((found.width - 72.0).abs() < 0.01 && (found.height - 36.0).abs() < 0.01);
        let x = found.left + 10.0;
        let y = found.top + 10.0;
        assert_eq!(picture_at_point(&layout, 0, x, y), Some(found));
        assert_eq!(picture_at_point(&layout, 0, found.left - 5.0, y), None);
        let selection = found.selection();
        assert_eq!(selected_picture(&layout, selection), Some(found));
        assert_eq!(selected_picture(&layout, Selection::caret(found.at)), None);
    }

    #[test]
    fn floating_picture_is_found_where_it_is_placed() {
        use letter_model::{
            Anchor, AnchorPosition, Distance, HorizontalFrom, Placement, VerticalFrom, Wrap,
        };
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![Block::from(Paragraph::from_runs([Run::text("ab")]))];
        let mut floating = picture(914_400, 457_200);
        floating.anchor = Some(Box::new(Anchor {
            horizontal: AnchorPosition {
                relative: HorizontalFrom::Page,
                placement: Placement::Offset(300 * 12_700),
            },
            vertical: AnchorPosition {
                relative: VerticalFrom::Page,
                placement: Placement::Offset(400 * 12_700),
            },
            wrap: Wrap::None,
            behind_text: false,
            distance: Distance::default(),
        }));
        insert_picture(&mut doc, Position::new(0, 1), floating);
        let library = letter_layout::FontLibrary::embedded_only();
        let layout =
            letter_layout::layout_document(&doc, &mut letter_layout::Shaper::new(), &library);
        let boxes = pictures_on_page(&layout, 0);
        let [found] = boxes[..] else {
            panic!("{boxes:?}");
        };
        assert_eq!(found.at, Position::new(0, 1));
        assert!((found.left - 300.0).abs() < 0.01 && (found.top - 400.0).abs() < 0.01);
        assert_eq!(picture_at_point(&layout, 0, 310.0, 410.0), Some(found));
        assert_eq!(selected_picture(&layout, found.selection()), Some(found));
    }
}
