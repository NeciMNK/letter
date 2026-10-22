//! Подложка: надпись за текстом каждой страницы («ОБРАЗЕЦ»,
//! «ЧЕРНОВИК»). Word хранит её в верхнем колонтитуле фигурой VML
//! с текстом (`v:shape` с `v:textpath`, имя фигуры
//! `PowerPlusWaterMarkObject…`). В модели фигура остаётся как есть
//! (непонятым содержимым фрагмента), а здесь из неё читается то, что
//! нужно для показа.

use crate::{Block, RawXml, RunContent};

/// Текстовая подложка.
#[derive(Debug, Clone, PartialEq)]
pub struct Watermark {
    /// Надпись.
    pub text: String,
    /// Шрифт.
    pub font: String,
    /// Цвет.
    pub color: (u8, u8, u8),
    /// Непрозрачность от 0 до 1 («полупрозрачная» — 0,5).
    pub opacity: f32,
    /// Поворот по часовой стрелке, градусы (по диагонали — 315).
    pub rotation: f32,
    /// Ширина и высота надписи, пт: текст растягивается на этот
    /// прямоугольник.
    pub width: f32,
    /// Высота, пт.
    pub height: f32,
}

/// Размеры надписи подложки, как их выбирает Word при автоматическом
/// размере: высота около 137 пт, ширина по числу знаков, но не больше
/// 528 пт.
const HEIGHT: f32 = 137.45;
const MAX_WIDTH: f32 = 527.85;

impl Watermark {
    /// Подложка как у кнопки «Подложка» в Word: шрифт Calibri, серебристая,
    /// полупрозрачная, по диагонали, автоматический размер.
    pub fn new(text: &str) -> Watermark {
        let chars = text.chars().count().max(1) as f32;
        // Средняя ширина прописной буквы Calibri — около 0,6 кегля.
        let natural = HEIGHT * 0.6 * chars;
        let (width, height) = if natural > MAX_WIDTH {
            (MAX_WIDTH, (HEIGHT * MAX_WIDTH / natural).max(HEIGHT * 0.6))
        } else {
            (natural, HEIGHT)
        };
        Watermark {
            text: text.to_string(),
            font: "Calibri".to_string(),
            color: (0xC0, 0xC0, 0xC0),
            opacity: 0.5,
            rotation: 315.0,
            width,
            height,
        }
    }

    /// Фигура VML для записи в колонтитул (`w:pict`); `number` делает
    /// имя фигуры уникальным в документе.
    pub fn to_raw(&self, number: usize) -> RawXml {
        let (r, g, b) = self.color;
        let opacity = if self.opacity < 1.0 {
            format!("<v:fill opacity=\"{}\"/>", round(self.opacity))
        } else {
            String::new()
        };
        RawXml::new(format!(
            concat!(
                r##"<w:pict xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" "##,
                r##"xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office">"##,
                r##"<v:shapetype id="_x0000_t136" coordsize="21600,21600" o:spt="136" adj="10800" "##,
                r##"path="m@7,l@8,m@5,21600l@6,21600e"><v:formulas>"##,
                r##"<v:f eqn="sum #0 0 10800"/><v:f eqn="prod #0 2 1"/><v:f eqn="sum 21600 0 @1"/>"##,
                r##"<v:f eqn="sum 0 0 @2"/><v:f eqn="sum 21600 0 @3"/><v:f eqn="if @0 @3 0"/>"##,
                r##"<v:f eqn="if @0 21600 @1"/><v:f eqn="if @0 0 @2"/><v:f eqn="if @0 @4 21600"/>"##,
                r##"<v:f eqn="mid @5 @6"/><v:f eqn="mid @8 @5"/><v:f eqn="mid @7 @8"/>"##,
                r##"<v:f eqn="mid @6 @7"/><v:f eqn="sum @6 0 @5"/></v:formulas>"##,
                r##"<v:path textpathok="t" o:connecttype="custom" "##,
                r##"o:connectlocs="@9,0;@10,10800;@11,21600;@12,10800" "##,
                r##"o:connectangles="270,180,90,0"/><v:textpath on="t" fitshape="t"/>"##,
                r##"<v:handles><v:h position="#0,bottomRight" xrange="6629,14971"/></v:handles>"##,
                r##"<o:lock v:ext="edit" text="t" shapetype="t"/></v:shapetype>"##,
                r##"<v:shape id="PowerPlusWaterMarkObject{number}" o:spid="_x0000_s{spid}" type="#_x0000_t136" "##,
                r##"style="position:absolute;margin-left:0;margin-top:0;width:{width}pt;height:{height}pt;"##,
                r##"rotation:{rotation};z-index:-251657216;mso-position-horizontal:center;"##,
                r##"mso-position-horizontal-relative:margin;mso-position-vertical:center;"##,
                r##"mso-position-vertical-relative:margin" o:allowincell="f" "##,
                r##"fillcolor="#{r:02x}{g:02x}{b:02x}" stroked="f">{opacity}"##,
                r##"<v:textpath style="font-family:&quot;{font}&quot;;font-size:1pt" string="{text}"/>"##,
                r##"</v:shape></w:pict>"##,
            ),
            number = number,
            spid = 2049 + number,
            width = round(self.width),
            height = round(self.height),
            rotation = round(self.rotation),
            r = r,
            g = g,
            b = b,
            opacity = opacity,
            font = escape(&self.font),
            text = escape(&self.text),
        ))
    }

    /// Подложка из фигуры VML (`w:pict` целиком или `v:shape`); `None`,
    /// если это не текстовая подложка Word.
    pub fn from_raw(raw: &RawXml) -> Option<Watermark> {
        let xml = raw.as_str();
        let shape = tags(xml).find(|t| t.local_name() == "shape")?;
        if !shape.attribute("id")?.contains("WaterMark") {
            return None;
        }
        // Первый `v:textpath` — в описании типа фигуры, без текста.
        let path =
            tags(xml).find(|t| t.local_name() == "textpath" && t.attribute("string").is_some())?;
        let text = unescape(path.attribute("string")?);
        let style = shape.attribute("style").map(unescape).unwrap_or_default();
        let value = |name: &str| {
            style.split(';').find_map(|part| {
                let (key, value) = part.split_once(':')?;
                (key.trim() == name).then(|| value.trim().to_string())
            })
        };
        let font = path
            .attribute("style")
            .map(unescape)
            .and_then(|s| {
                s.split(';').find_map(|part| {
                    let (key, value) = part.split_once(':')?;
                    (key.trim() == "font-family")
                        .then(|| value.trim().trim_matches('"').to_string())
                })
            })
            .filter(|f| !f.is_empty())
            .unwrap_or_else(|| "Calibri".to_string());
        let opacity = tags(xml)
            .find(|t| t.local_name() == "fill")
            .and_then(|fill| fill.attribute("opacity").and_then(parse_fraction))
            .unwrap_or(1.0);
        Some(Watermark {
            text,
            font,
            color: shape
                .attribute("fillcolor")
                .and_then(parse_color)
                .unwrap_or((0xC0, 0xC0, 0xC0)),
            opacity,
            rotation: value("rotation")
                .and_then(|r| r.parse::<f32>().ok())
                .unwrap_or(0.0),
            width: value("width").and_then(|w| length(&w)).unwrap_or(0.0),
            height: value("height").and_then(|h| length(&h)).unwrap_or(0.0),
        })
    }
}

/// Число для VML: не больше двух знаков после точки, без лишних нулей.
fn round(value: f32) -> String {
    let text = format!("{:.2}", value);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    text.to_string()
}

/// Экранирует текст для значения атрибута XML.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// Фрагмент — текстовая подложка Word.
pub fn is_watermark(raw: &RawXml) -> bool {
    Watermark::from_raw(raw).is_some()
}

/// Первая подложка в блоках колонтитула (в абзацах и ячейках таблиц).
pub fn find_watermark(blocks: &[Block]) -> Option<Watermark> {
    blocks.iter().find_map(|block| match block {
        Block::Paragraph(p) => p.runs().find_map(|run| {
            run.content.iter().find_map(|item| match item {
                RunContent::Unknown(raw) => Watermark::from_raw(raw),
                _ => None,
            })
        }),
        Block::Table(t) => t
            .rows
            .iter()
            .flat_map(|r| &r.cells)
            .find_map(|c| find_watermark(&c.blocks)),
        Block::Unknown(_) => None,
    })
}

/// Открывающие теги фрагмента XML (без вложенности), каждый как
/// отдельный фрагмент — чтобы читать их атрибуты.
fn tags(xml: &str) -> impl Iterator<Item = RawXml> + '_ {
    xml.match_indices('<').filter_map(|(at, _)| {
        let rest = &xml[at..];
        if rest.starts_with("</") || rest.starts_with("<?") || rest.starts_with("<!") {
            return None;
        }
        let end = tag_end(rest)?;
        Some(RawXml::new(&rest[..=end]))
    })
}

/// Где кончается тег (`>` вне кавычек).
fn tag_end(tag: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in tag.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), _) if c == q => quote = None,
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

/// Раскрывает ссылки на сущности XML (`&quot;`, `&#39;`…).
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';') else {
            break;
        };
        let entity = &rest[1..end];
        let c = match entity {
            "quot" => Some('"'),
            "apos" => Some('\''),
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            _ => entity
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Длина VML в пунктах: `412.4pt`, `5in`, `10cm`, `20mm`; без единиц —
/// пиксели (3/4 пт).
fn length(value: &str) -> Option<f32> {
    let value = value.trim();
    let split = value
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .unwrap_or(value.len());
    let number: f32 = value[..split].parse().ok()?;
    let factor = match &value[split..] {
        "pt" => 1.0,
        "in" => 72.0,
        "cm" => 72.0 / 2.54,
        "mm" => 72.0 / 25.4,
        "pc" => 12.0,
        "" | "px" => 0.75,
        _ => return None,
    };
    Some(number * factor)
}

/// Доля: `.5`, `0.5`, `50%`, `32768f` (в 1/65536).
fn parse_fraction(value: &str) -> Option<f32> {
    let value = value.trim();
    let fraction = if let Some(p) = value.strip_suffix('%') {
        p.parse::<f32>().ok()? / 100.0
    } else if let Some(f) = value.strip_suffix('f') {
        f.parse::<f32>().ok()? / 65536.0
    } else {
        value.parse().ok()?
    };
    Some(fraction.clamp(0.0, 1.0))
}

/// Цвет VML: `#c0c0c0`, `silver`, `red [2]`.
fn parse_color(value: &str) -> Option<(u8, u8, u8)> {
    let value = value.split_whitespace().next()?.to_ascii_lowercase();
    if let Some(hex) = value.strip_prefix('#') {
        let hex = if hex.len() == 3 {
            hex.chars().flat_map(|c| [c, c]).collect()
        } else {
            hex.to_string()
        };
        let n = u32::from_str_radix(hex.get(..6)?, 16).ok()?;
        return Some(((n >> 16) as u8, (n >> 8) as u8, n as u8));
    }
    Some(match value.as_str() {
        "black" => (0, 0, 0),
        "silver" => (0xC0, 0xC0, 0xC0),
        "gray" | "grey" => (0x80, 0x80, 0x80),
        "white" => (0xFF, 0xFF, 0xFF),
        "maroon" => (0x80, 0, 0),
        "red" => (0xFF, 0, 0),
        "purple" => (0x80, 0, 0x80),
        "fuchsia" => (0xFF, 0, 0xFF),
        "green" => (0, 0x80, 0),
        "lime" => (0, 0xFF, 0),
        "olive" => (0x80, 0x80, 0),
        "yellow" => (0xFF, 0xFF, 0),
        "navy" => (0, 0, 0x80),
        "blue" => (0, 0, 0xFF),
        "teal" => (0, 0x80, 0x80),
        "aqua" => (0, 0xFF, 0xFF),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Подложка, как её пишет Word.
    pub(crate) const WORD: &str = r##"<w:pict xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office"><v:shapetype id="_x0000_t136" coordsize="21600,21600" o:spt="136" adj="10800" path="m@7,l@8,m@5,21600l@6,21600e"><v:path textpathok="t" o:connecttype="custom"/><v:textpath on="t" fitshape="t"/></v:shapetype><v:shape id="PowerPlusWaterMarkObject357922611" o:spid="_x0000_s2049" type="#_x0000_t136" style="position:absolute;margin-left:0;margin-top:0;width:412.4pt;height:137.45pt;rotation:315;z-index:-251657216;mso-position-horizontal:center;mso-position-horizontal-relative:margin;mso-position-vertical:center;mso-position-vertical-relative:margin" o:allowincell="f" fillcolor="silver" stroked="f"><v:fill opacity=".5"/><v:textpath style="font-family:&quot;Calibri&quot;;font-size:1pt" string="ОБРАЗЕЦ &amp; Co"/></v:shape></w:pict>"##;

    #[test]
    fn word_watermark_is_read() {
        let mark = Watermark::from_raw(&RawXml::new(WORD)).expect("подложка");
        assert_eq!(mark.text, "ОБРАЗЕЦ & Co");
        assert_eq!(mark.font, "Calibri");
        assert_eq!(mark.color, (0xC0, 0xC0, 0xC0));
        assert!((mark.opacity - 0.5).abs() < 1e-6);
        assert!((mark.rotation - 315.0).abs() < 1e-6);
        assert!((mark.width - 412.4).abs() < 1e-3 && (mark.height - 137.45).abs() < 1e-3);
        // Обычная фигура VML — не подложка.
        let other = WORD.replace("PowerPlusWaterMarkObject", "Shape");
        assert_eq!(Watermark::from_raw(&RawXml::new(other)), None);
    }

    #[test]
    fn new_watermark_survives_writing() {
        let mark = Watermark::new("ЧЕРНОВИК \"1\" & <2>");
        let raw = mark.to_raw(3);
        assert!(raw.as_str().contains("PowerPlusWaterMarkObject3"));
        let back = Watermark::from_raw(&raw).expect("подложка");
        assert_eq!(back.text, mark.text);
        assert_eq!(back.font, "Calibri");
        assert_eq!(back.color, (0xC0, 0xC0, 0xC0));
        assert!((back.opacity - 0.5).abs() < 1e-6);
        assert!((back.width - mark.width).abs() < 0.01);
        assert!((back.height - mark.height).abs() < 0.01);
        // Длинная надпись не шире 528 пт.
        let long = Watermark::new("КОНФИДЕНЦИАЛЬНО");
        assert!(long.width <= MAX_WIDTH && long.height < HEIGHT);
        assert_eq!(round(1.50), "1.5");
        assert_eq!(round(315.0), "315");
    }

    #[test]
    fn values_are_parsed() {
        assert_eq!(length("1in"), Some(72.0));
        assert_eq!(length("8px"), Some(6.0));
        assert_eq!(length("abc"), None);
        assert_eq!(parse_fraction("50%"), Some(0.5));
        assert_eq!(parse_fraction("32768f"), Some(0.5));
        assert_eq!(parse_color("#f00"), Some((255, 0, 0)));
        assert_eq!(parse_color("red [2]"), Some((255, 0, 0)));
        assert_eq!(unescape("a&#1040;&#x42;&bad;"), "aАB&bad;");
    }
}
