//! Свойства абзаца (`style:paragraph-properties`) и символов
//! (`style:text-properties`): чтение в модель и запись из модели.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;

use letter_model::{
    Alignment, Color, Document, FirstLineIndent, HalfPoints, Highlight, LineSpacing,
    ParagraphProps, RunFonts, RunProps, TabLeader, TabStop, TabStopKind, ThemeFont, Twips,
    Underline, VerticalAlign,
};

use crate::units::{length, number, parse_length, parse_percent};
use crate::xml::{Element, escape};

/// Объявленные шрифты: имя в `style:font-name` → название семейства.
pub(crate) type FontFaces = HashMap<String, String>;

/// Читает `style:paragraph-properties`.
pub(crate) fn read_paragraph(el: &Element) -> ParagraphProps {
    let mut p = ParagraphProps::default();
    let len = |name: &str| el.attr(name).and_then(parse_length);
    let flag = |name: &str| match el.attr(name) {
        Some("always") | Some("true") => Some(true),
        Some("auto") | Some("false") => Some(false),
        _ => None,
    };

    p.alignment = match el.attr("fo:text-align") {
        Some("start" | "left") => Some(Alignment::Left),
        Some("center") => Some(Alignment::Center),
        Some("end" | "right") => Some(Alignment::Right),
        Some("justify") if el.attr("fo:text-align-last") == Some("justify") => {
            Some(Alignment::Distribute)
        }
        Some("justify") => Some(Alignment::Justify),
        _ => None,
    };

    let all = len("fo:margin");
    p.indentation.left = len("fo:margin-left").or(all);
    p.indentation.right = len("fo:margin-right").or(all);
    p.spacing.before = len("fo:margin-top").or(all);
    p.spacing.after = len("fo:margin-bottom").or(all);
    p.indentation.first_line = len("fo:text-indent").map(|t| {
        if t.0 < 0 {
            FirstLineIndent::Hanging(Twips(-t.0))
        } else {
            FirstLineIndent::FirstLine(t)
        }
    });

    p.spacing.line = match el.attr("fo:line-height") {
        Some("normal") => Some(LineSpacing::SINGLE),
        Some(value) => match parse_percent(value) {
            Some(pct) => Some(LineSpacing::multiple(pct / 100.0)),
            None => parse_length(value).map(LineSpacing::Exact),
        },
        None => None,
    };
    if let Some(t) = len("style:line-height-at-least") {
        p.spacing.line = Some(LineSpacing::AtLeast(t));
    }

    p.keep_next = flag("fo:keep-with-next");
    p.keep_lines = flag("fo:keep-together");
    p.page_break_before = match el.attr("fo:break-before") {
        Some("page") => Some(true),
        Some("auto") => Some(false),
        _ => None,
    };
    p.widow_control = el
        .attr("fo:widows")
        .or_else(|| el.attr("fo:orphans"))
        .and_then(|v| v.trim().parse::<u32>().ok())
        .map(|n| n > 0);
    p.contextual_spacing =
        flag("style:contextual-spacing").or_else(|| flag("loext:contextual-spacing"));

    if let Some(stops) = el.child("style:tab-stops") {
        p.tabs = stops
            .elements()
            .filter(|e| e.name == "style:tab-stop")
            .filter_map(read_tab)
            .collect();
    }
    p
}

fn read_tab(el: &Element) -> Option<TabStop> {
    let position = el.attr("style:position").and_then(parse_length)?;
    let kind = match el.attr("style:type") {
        Some("center") => TabStopKind::Center,
        Some("right") => TabStopKind::Right,
        Some("char") => TabStopKind::Decimal,
        _ => TabStopKind::Left,
    };
    let bold = el.attr("style:leader-width") == Some("bold");
    let leader = match el.attr("style:leader-text").and_then(|t| t.chars().next()) {
        Some('.' | '…') => TabLeader::Dot,
        Some('-') => TabLeader::Hyphen,
        Some('_') if bold => TabLeader::Heavy,
        Some('_') => TabLeader::Underscore,
        Some('·') => TabLeader::MiddleDot,
        Some(' ') => TabLeader::None,
        _ => match el.attr("style:leader-style") {
            Some("dotted") => TabLeader::Dot,
            Some("dash") => TabLeader::Hyphen,
            Some("solid") if bold => TabLeader::Heavy,
            Some("solid") => TabLeader::Underscore,
            _ => TabLeader::None,
        },
    };
    Some(TabStop {
        position,
        kind,
        leader,
    })
}

/// Читает `style:text-properties`. Второе значение — размер шрифта
/// в процентах от родительского стиля, если он задан так.
pub(crate) fn read_text(el: &Element, fonts: &FontFaces) -> (RunProps, Option<f64>) {
    let mut r = RunProps::default();
    let font = |name_attr: &str, family_attr: &str| {
        el.attr(name_attr)
            .map(|n| fonts.get(n).cloned().unwrap_or_else(|| n.to_string()))
            .or_else(|| el.attr(family_attr).map(first_family))
            .filter(|f| !f.is_empty())
    };
    let western = font("style:font-name", "fo:font-family");
    r.fonts = RunFonts {
        ascii: western.clone(),
        h_ansi: western,
        east_asia: font("style:font-name-asian", "style:font-family-asian"),
        complex_script: font("style:font-name-complex", "style:font-family-complex"),
        ..RunFonts::default()
    };

    let mut percent = None;
    if let Some(size) = el.attr("fo:font-size") {
        match parse_percent(size) {
            Some(pct) => percent = Some(pct),
            None => {
                r.size = parse_length(size)
                    .filter(|t| t.0 > 0)
                    .map(|t| HalfPoints::from_points(t.to_points()));
            }
        }
    }
    r.bold = el.attr("fo:font-weight").and_then(|w| match w {
        "bold" | "bolder" => Some(true),
        "normal" | "lighter" => Some(false),
        n => n.parse::<u32>().ok().map(|n| n >= 600),
    });
    r.italic = el.attr("fo:font-style").and_then(|s| match s {
        "italic" | "oblique" => Some(true),
        "normal" => Some(false),
        _ => None,
    });
    r.underline = el
        .attr("style:text-underline-style")
        .map(|style| read_underline(el, style));
    match el.attr("style:text-line-through-style") {
        Some("none") => r.strike = Some(false),
        Some(_) if el.attr("style:text-line-through-type") == Some("double") => {
            r.double_strike = Some(true);
        }
        Some(_) => r.strike = Some(true),
        None => {}
    }
    r.color = if el.attr("style:use-window-font-color") == Some("true") {
        Some(Color::Auto)
    } else {
        el.attr("fo:color").and_then(parse_color)
    };
    r.highlight = el
        .attr("fo:background-color")
        .map(|c| match parse_color(c) {
            Some(Color::Rgb(red, green, blue)) => nearest_highlight(red, green, blue),
            _ => Highlight::None,
        });
    r.vertical_align = el.attr("style:text-position").and_then(read_position);
    (r, percent)
}

/// Первое семейство из списка `fo:font-family` без кавычек.
fn first_family(list: &str) -> String {
    list.split(',')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches(|c| c == '\'' || c == '"')
        .to_string()
}

fn read_underline(el: &Element, style: &str) -> Underline {
    let double = el.attr("style:text-underline-type") == Some("double");
    let bold = el.attr("style:text-underline-width") == Some("bold");
    let words = el.attr("style:text-underline-mode") == Some("skip-white-space");
    let pick = |normal, heavy| if bold { heavy } else { normal };
    match style {
        "none" => Underline::None,
        "dotted" => pick(Underline::Dotted, Underline::DottedHeavy),
        "dash" => pick(Underline::Dash, Underline::DashedHeavy),
        "long-dash" => pick(Underline::DashLong, Underline::DashLongHeavy),
        "dot-dash" => pick(Underline::DotDash, Underline::DashDotHeavy),
        "dot-dot-dash" => pick(Underline::DotDotDash, Underline::DashDotDotHeavy),
        "wave" if double => Underline::WavyDouble,
        "wave" => pick(Underline::Wave, Underline::WavyHeavy),
        _ if double => Underline::Double,
        _ if bold => Underline::Thick,
        _ if words => Underline::Words,
        _ => Underline::Single,
    }
}

fn read_position(value: &str) -> Option<VerticalAlign> {
    let first = value.split_whitespace().next()?;
    match first {
        "super" => Some(VerticalAlign::Superscript),
        "sub" => Some(VerticalAlign::Subscript),
        other => {
            let pct = parse_percent(other)?;
            Some(if pct > 0.0 {
                VerticalAlign::Superscript
            } else if pct < 0.0 {
                VerticalAlign::Subscript
            } else {
                VerticalAlign::Baseline
            })
        }
    }
}

/// Цвет `#rrggbb`.
pub(crate) fn parse_color(value: &str) -> Option<Color> {
    let hex = value.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let pair = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some(Color::Rgb(pair(0)?, pair(2)?, pair(4)?))
}

/// Ближайший из 16 цветов маркера Word.
fn nearest_highlight(r: u8, g: u8, b: u8) -> Highlight {
    const ALL: [Highlight; 16] = [
        Highlight::Black,
        Highlight::Blue,
        Highlight::Cyan,
        Highlight::Green,
        Highlight::Magenta,
        Highlight::Red,
        Highlight::Yellow,
        Highlight::White,
        Highlight::DarkBlue,
        Highlight::DarkCyan,
        Highlight::DarkGreen,
        Highlight::DarkMagenta,
        Highlight::DarkRed,
        Highlight::DarkYellow,
        Highlight::DarkGray,
        Highlight::LightGray,
    ];
    ALL.into_iter()
        .min_by_key(|h| {
            let (hr, hg, hb) = h.rgb().unwrap_or((0, 0, 0));
            let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
            d(r, hr) + d(g, hg) + d(b, hb)
        })
        .unwrap_or(Highlight::Yellow)
}

/// Атрибуты для записи: имя и значение.
pub(crate) type Attrs = Vec<(&'static str, String)>;

/// Строка атрибутов ` a="b" c="d"`.
pub(crate) fn attrs_string(attrs: &Attrs) -> String {
    let mut out = String::new();
    for (name, value) in attrs {
        let _ = write!(out, " {name}=\"{}\"", escape(value));
    }
    out
}

/// Свойства абзаца для записи: атрибуты `style:paragraph-properties`
/// и содержимое (позиции табуляции).
pub(crate) fn write_paragraph(p: &ParagraphProps) -> (Attrs, String) {
    let mut a: Attrs = Vec::new();
    if let Some(alignment) = p.alignment {
        let (align, last) = match alignment {
            Alignment::Left => ("start", None),
            Alignment::Center => ("center", None),
            Alignment::Right => ("end", None),
            Alignment::Justify => ("justify", Some("start")),
            Alignment::Distribute => ("justify", Some("justify")),
        };
        a.push(("fo:text-align", align.into()));
        if let Some(last) = last {
            a.push(("fo:text-align-last", last.into()));
        }
    }
    let mut len = |name, value: Option<Twips>| {
        if let Some(t) = value {
            a.push((name, length(t)));
        }
    };
    len("fo:margin-left", p.indentation.left);
    len("fo:margin-right", p.indentation.right);
    len("fo:margin-top", p.spacing.before);
    len("fo:margin-bottom", p.spacing.after);
    len(
        "fo:text-indent",
        p.indentation.first_line.map(|f| match f {
            FirstLineIndent::FirstLine(t) => t,
            FirstLineIndent::Hanging(t) => Twips(-t.0),
        }),
    );
    match p.spacing.line {
        Some(LineSpacing::Multiple(n)) => {
            a.push((
                "fo:line-height",
                format!("{}%", number(f64::from(n) * 100.0 / 240.0)),
            ));
        }
        Some(LineSpacing::Exact(t)) => a.push(("fo:line-height", length(t))),
        Some(LineSpacing::AtLeast(t)) => a.push(("style:line-height-at-least", length(t))),
        None => {}
    }
    let keep = |v: bool| if v { "always" } else { "auto" }.to_string();
    if let Some(v) = p.keep_next {
        a.push(("fo:keep-with-next", keep(v)));
    }
    if let Some(v) = p.keep_lines {
        a.push(("fo:keep-together", keep(v)));
    }
    if let Some(v) = p.page_break_before {
        a.push(("fo:break-before", if v { "page" } else { "auto" }.into()));
    }
    if let Some(v) = p.widow_control {
        let n = if v { "2" } else { "0" };
        a.push(("fo:widows", n.into()));
        a.push(("fo:orphans", n.into()));
    }
    if let Some(v) = p.contextual_spacing {
        a.push(("style:contextual-spacing", v.to_string()));
    }

    let mut inner = String::new();
    let tabs: Vec<&TabStop> = p
        .tabs
        .iter()
        .filter(|t| !matches!(t.kind, TabStopKind::Clear | TabStopKind::Bar))
        .collect();
    if !tabs.is_empty() {
        inner.push_str("<style:tab-stops>");
        for tab in tabs {
            let mut t: Attrs = vec![("style:position", length(tab.position))];
            let kind = match tab.kind {
                TabStopKind::Center => Some("center"),
                TabStopKind::Right => Some("right"),
                TabStopKind::Decimal => Some("char"),
                _ => None,
            };
            if let Some(kind) = kind {
                t.push(("style:type", kind.into()));
            }
            if tab.kind == TabStopKind::Decimal {
                t.push(("style:char", ",".into()));
            }
            let leader = match tab.leader {
                TabLeader::None => None,
                TabLeader::Dot => Some(("dotted", ".", false)),
                TabLeader::Hyphen => Some(("dash", "-", false)),
                TabLeader::Underscore => Some(("solid", "_", false)),
                TabLeader::Heavy => Some(("solid", "_", true)),
                TabLeader::MiddleDot => Some(("dotted", "·", false)),
            };
            if let Some((style, text, bold)) = leader {
                t.push(("style:leader-style", style.into()));
                t.push(("style:leader-text", text.into()));
                if bold {
                    t.push(("style:leader-width", "bold".into()));
                }
            }
            let _ = write!(inner, "<style:tab-stop{}/>", attrs_string(&t));
        }
        inner.push_str("</style:tab-stops>");
    }
    (a, inner)
}

/// Свойства символов для записи. Ссылки на шрифты темы заменяются
/// именами; использованные шрифты добавляются в `fonts`.
pub(crate) fn write_text(r: &RunProps, doc: &Document, fonts: &mut BTreeSet<String>) -> Attrs {
    let mut a: Attrs = Vec::new();
    let mut font = |name: &Option<String>, theme: Option<ThemeFont>| {
        let font = match theme {
            Some(theme) => doc.theme.font(theme).map(str::to_string),
            None => name.clone(),
        };
        font.filter(|f| !f.is_empty()).inspect(|f| {
            fonts.insert(f.clone());
        })
    };
    let f = &r.fonts;
    let western = font(&f.ascii, f.ascii_theme).or_else(|| font(&f.h_ansi, f.h_ansi_theme));
    if let Some(name) = western {
        a.push(("style:font-name", name));
    }
    if let Some(name) = font(&f.east_asia, f.east_asia_theme) {
        a.push(("style:font-name-asian", name));
    }
    if let Some(name) = font(&f.complex_script, f.complex_script_theme) {
        a.push(("style:font-name-complex", name));
    }
    if let Some(size) = r.size {
        let pt = format!("{}pt", number(size.to_points()));
        a.push(("fo:font-size", pt.clone()));
        a.push(("style:font-size-asian", pt.clone()));
        a.push(("style:font-size-complex", pt));
    }
    if let Some(bold) = r.bold {
        let w = if bold { "bold" } else { "normal" };
        a.push(("fo:font-weight", w.into()));
        a.push(("style:font-weight-asian", w.into()));
        a.push(("style:font-weight-complex", w.into()));
    }
    if let Some(italic) = r.italic {
        let s = if italic { "italic" } else { "normal" };
        a.push(("fo:font-style", s.into()));
        a.push(("style:font-style-asian", s.into()));
        a.push(("style:font-style-complex", s.into()));
    }
    if let Some(underline) = r.underline {
        write_underline(underline, &mut a);
    }
    if r.double_strike == Some(true) {
        a.push(("style:text-line-through-style", "solid".into()));
        a.push(("style:text-line-through-type", "double".into()));
    } else if r.strike == Some(true) {
        a.push(("style:text-line-through-style", "solid".into()));
        a.push(("style:text-line-through-type", "single".into()));
    } else if r.strike.is_some() || r.double_strike.is_some() {
        a.push(("style:text-line-through-style", "none".into()));
    }
    match r.color {
        Some(Color::Auto) => a.push(("style:use-window-font-color", "true".into())),
        Some(Color::Rgb(red, green, blue)) => {
            a.push(("fo:color", format!("#{red:02x}{green:02x}{blue:02x}")));
        }
        None => {}
    }
    if let Some(highlight) = r.highlight {
        let value = match highlight.rgb() {
            Some((red, green, blue)) => format!("#{red:02x}{green:02x}{blue:02x}"),
            None => "transparent".into(),
        };
        a.push(("fo:background-color", value));
    }
    if let Some(align) = r.vertical_align {
        let value = match align {
            VerticalAlign::Baseline => "0% 100%",
            VerticalAlign::Superscript => "super 58%",
            VerticalAlign::Subscript => "sub 58%",
        };
        a.push(("style:text-position", value.into()));
    }
    a
}

fn write_underline(underline: Underline, a: &mut Attrs) {
    // Вид линии, двойная, толстая.
    let (style, double, bold) = match underline {
        Underline::None => {
            a.push(("style:text-underline-style", "none".into()));
            return;
        }
        Underline::Single | Underline::Words => ("solid", false, false),
        Underline::Double => ("solid", true, false),
        Underline::Thick => ("solid", false, true),
        Underline::Dotted => ("dotted", false, false),
        Underline::DottedHeavy => ("dotted", false, true),
        Underline::Dash => ("dash", false, false),
        Underline::DashedHeavy => ("dash", false, true),
        Underline::DashLong => ("long-dash", false, false),
        Underline::DashLongHeavy => ("long-dash", false, true),
        Underline::DotDash => ("dot-dash", false, false),
        Underline::DashDotHeavy => ("dot-dash", false, true),
        Underline::DotDotDash => ("dot-dot-dash", false, false),
        Underline::DashDotDotHeavy => ("dot-dot-dash", false, true),
        Underline::Wave => ("wave", false, false),
        Underline::WavyHeavy => ("wave", false, true),
        Underline::WavyDouble => ("wave", true, false),
    };
    a.push(("style:text-underline-style", style.into()));
    if double {
        a.push(("style:text-underline-type", "double".into()));
    }
    a.push((
        "style:text-underline-width",
        if bold { "bold" } else { "auto" }.into(),
    ));
    a.push(("style:text-underline-color", "font-color".into()));
    if underline == Underline::Words {
        a.push(("style:text-underline-mode", "skip-white-space".into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::parse;

    const NS: &str = r#"xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0""#;

    fn element(tag: &str, attrs: &Attrs, inner: &str) -> Element {
        let xml = format!(
            "<style:{tag} {NS}{}>{inner}</style:{tag}>",
            attrs_string(attrs)
        );
        parse(xml.as_bytes()).unwrap_or_default()
    }

    #[test]
    fn paragraph_props_round_trip() {
        let p = ParagraphProps {
            alignment: Some(Alignment::Distribute),
            indentation: letter_model::Indentation {
                left: Some(Twips(720)),
                right: Some(Twips(-100)),
                first_line: Some(FirstLineIndent::Hanging(Twips(360))),
            },
            spacing: letter_model::Spacing {
                before: Some(Twips(0)),
                after: Some(Twips(160)),
                line: Some(LineSpacing::Multiple(259)),
            },
            contextual_spacing: Some(true),
            keep_next: Some(true),
            keep_lines: Some(false),
            page_break_before: Some(true),
            widow_control: Some(false),
            tabs: vec![
                TabStop {
                    position: Twips(4680),
                    kind: TabStopKind::Center,
                    leader: TabLeader::None,
                },
                TabStop {
                    position: Twips(9360),
                    kind: TabStopKind::Right,
                    leader: TabLeader::Dot,
                },
                TabStop {
                    position: Twips(100),
                    kind: TabStopKind::Decimal,
                    leader: TabLeader::Heavy,
                },
            ],
            ..ParagraphProps::default()
        };
        let (attrs, inner) = write_paragraph(&p);
        let back = read_paragraph(&element("paragraph-properties", &attrs, &inner));
        assert_eq!(back, p);

        for line in [
            LineSpacing::SINGLE,
            LineSpacing::Exact(Twips(300)),
            LineSpacing::AtLeast(Twips(280)),
        ] {
            let p = ParagraphProps {
                spacing: letter_model::Spacing {
                    line: Some(line),
                    ..Default::default()
                },
                ..ParagraphProps::default()
            };
            let (attrs, inner) = write_paragraph(&p);
            assert_eq!(
                read_paragraph(&element("paragraph-properties", &attrs, &inner)),
                p
            );
        }
    }

    #[test]
    fn text_props_round_trip() {
        let doc = Document::new();
        let underlines = [
            Underline::None,
            Underline::Single,
            Underline::Words,
            Underline::Double,
            Underline::Thick,
            Underline::Dotted,
            Underline::DottedHeavy,
            Underline::Dash,
            Underline::DashedHeavy,
            Underline::DashLong,
            Underline::DashLongHeavy,
            Underline::DotDash,
            Underline::DashDotHeavy,
            Underline::DotDotDash,
            Underline::DashDotDotHeavy,
            Underline::Wave,
            Underline::WavyHeavy,
            Underline::WavyDouble,
        ];
        for underline in underlines {
            let r = RunProps {
                fonts: RunFonts {
                    ascii: Some("Liberation Serif".into()),
                    h_ansi: Some("Liberation Serif".into()),
                    east_asia: Some("Noto Sans CJK".into()),
                    complex_script: Some("Amiri".into()),
                    ..RunFonts::default()
                },
                size: Some(HalfPoints(23)),
                bold: Some(true),
                italic: Some(false),
                underline: Some(underline),
                strike: Some(true),
                color: Some(Color::Rgb(0xC0, 0, 0x10)),
                highlight: Some(Highlight::DarkCyan),
                vertical_align: Some(VerticalAlign::Subscript),
                ..RunProps::default()
            };
            let mut fonts = BTreeSet::new();
            let attrs = write_text(&r, &doc, &mut fonts);
            let (back, pct) = read_text(&element("text-properties", &attrs, ""), &HashMap::new());
            assert_eq!(back, r, "{underline:?}");
            assert_eq!(pct, None);
            assert!(fonts.contains("Amiri"));
        }
    }

    #[test]
    fn libreoffice_text_props() {
        let fonts = HashMap::from([("Liberation Sans".to_string(), "Liberation Sans".to_string())]);
        let attrs: Attrs = vec![
            ("style:font-name", "Liberation Sans".into()),
            ("fo:font-size", "130%".into()),
            ("fo:font-weight", "700".into()),
            ("style:use-window-font-color", "true".into()),
            ("fo:background-color", "#fefe01".into()),
            ("style:text-position", "33% 58%".into()),
            ("style:text-line-through-style", "solid".into()),
            ("style:text-line-through-type", "double".into()),
        ];
        let (r, pct) = read_text(&element("text-properties", &attrs, ""), &fonts);
        assert_eq!(r.fonts.ascii.as_deref(), Some("Liberation Sans"));
        assert_eq!(pct, Some(130.0));
        assert_eq!(r.size, None);
        assert_eq!(r.bold, Some(true));
        assert_eq!(r.color, Some(Color::Auto));
        assert_eq!(r.highlight, Some(Highlight::Yellow));
        assert_eq!(r.vertical_align, Some(VerticalAlign::Superscript));
        assert_eq!((r.strike, r.double_strike), (None, Some(true)));

        let attrs: Attrs = vec![("fo:font-family", "'DejaVu Sans', sans-serif".into())];
        let (r, _) = read_text(&element("text-properties", &attrs, ""), &fonts);
        assert_eq!(r.fonts.h_ansi.as_deref(), Some("DejaVu Sans"));
    }
}
