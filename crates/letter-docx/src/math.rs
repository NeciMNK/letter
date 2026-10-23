//! Формулы OMML (`m:oMath`, `m:oMathPara`): чтение в дерево
//! [`Math`] и запись обратно.
//!
//! Формула из файла хранит и исходный XML ([`Math::raw`]): пока её не
//! меняли, она записывается ровно так, как была, со всеми свойствами,
//! которые дерево не описывает (шрифты, размеры скобок, выравнивание
//! строк системы и т. п.).

use letter_model::{FractionKind, Math, MathAlign, MathList, MathNode};

use crate::serialize::{escape_attr, escape_text};
use crate::xml::Element;

/// Пространство имён OMML.
pub(crate) const M_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
/// Пространство имён OMML в строгом (Strict) варианте OOXML.
const M_NS_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/math";

impl Element {
    /// Элемент `m:<local>`.
    fn is_m(&self, local: &str) -> bool {
        self.local == local && matches!(self.ns.as_deref(), Some(M_NS | M_NS_STRICT))
    }

    /// Дочерний элемент `m:<local>`.
    fn m_child(&self, local: &str) -> Option<&Element> {
        self.elements().find(|e| e.is_m(local))
    }

    /// Атрибут `m:val` дочернего элемента `m:<local>`.
    fn m_val(&self, local: &str) -> Option<&str> {
        self.m_child(local)?
            .attrs
            .iter()
            .find(|a| a.local == "val")
            .map(|a| a.value.as_str())
    }

    /// Переключатель вида `<m:degHide m:val="1"/>` (без `m:val` — включён).
    fn m_flag(&self, local: &str) -> bool {
        match self.m_child(local) {
            None => false,
            Some(_) => !matches!(self.m_val(local), Some("0" | "false" | "off")),
        }
    }
}

/// Формула, если элемент — `m:oMath` или `m:oMathPara`.
pub(crate) fn read_math(element: &Element) -> Option<Math> {
    let mut math = Math {
        raw: Some(element.raw()),
        ..Math::default()
    };
    if element.is_m("oMath") {
        math.lines.push(read_list(element));
    } else if element.is_m("oMathPara") {
        math.display = true;
        if let Some(props) = element.m_child("oMathParaPr") {
            math.align = match props.m_val("jc") {
                Some("left") => MathAlign::Left,
                Some("right") => MathAlign::Right,
                _ => MathAlign::Center,
            };
        }
        math.lines = element
            .elements()
            .filter(|e| e.is_m("oMath"))
            .map(read_list)
            .collect();
    } else {
        return None;
    }
    Some(math)
}

/// Узлы внутри элемента-ячейки (`m:e`, `m:num`, `m:sub`…).
fn read_list(element: &Element) -> MathList {
    element.elements().filter_map(read_node).collect()
}

/// Ячейка `m:<local>` элемента (пустая, если её нет).
fn cell(element: &Element, local: &str) -> MathList {
    element.m_child(local).map(read_list).unwrap_or_default()
}

/// Свойство-знак (`m:chr`, `m:begChr`…) из свойств `props`; `default` —
/// если не задан. Пустое значение — знака нет.
fn chr(props: Option<&Element>, local: &str, default: &str) -> String {
    match props.and_then(|p| p.m_child(local)) {
        Some(e) => e
            .attrs
            .iter()
            .find(|a| a.local == "val")
            .map_or(String::new(), |a| a.value.clone()),
        None => default.to_string(),
    }
}

fn read_node(element: &Element) -> Option<MathNode> {
    let ns_ok = matches!(element.ns.as_deref(), Some(M_NS | M_NS_STRICT));
    if !ns_ok {
        // Закладки, исправления и т. п. внутри формулы не показываются.
        return None;
    }
    let props = |name: &str| element.m_child(name);
    let node = match element.local.as_str() {
        "r" => {
            let text: String = element
                .elements()
                .filter(|e| e.is_m("t"))
                .map(Element::text)
                .collect();
            let plain = element
                .m_child("rPr")
                .is_some_and(|p| p.m_flag("nor") || matches!(p.m_val("sty"), Some("p" | "b")));
            MathNode::Text { text, plain }
        }
        "f" => MathNode::Fraction {
            kind: match props("fPr").and_then(|p| p.m_val("type")) {
                Some("noBar") => FractionKind::NoBar,
                Some("skw") => FractionKind::Skewed,
                Some("lin") => FractionKind::Linear,
                _ => FractionKind::Bar,
            },
            num: cell(element, "num"),
            den: cell(element, "den"),
        },
        "sSub" | "sSup" | "sSubSup" | "sPre" => {
            let local = element.local.as_str();
            MathNode::Scripts {
                base: cell(element, "e"),
                sub: (local != "sSup").then(|| cell(element, "sub")),
                sup: (local != "sSub").then(|| cell(element, "sup")),
                pre: local == "sPre",
            }
        }
        "rad" => {
            let hidden = props("radPr").is_some_and(|p| p.m_flag("degHide"));
            let degree = cell(element, "deg");
            MathNode::Radical {
                degree: (!hidden && !degree.is_empty()).then_some(degree),
                base: cell(element, "e"),
            }
        }
        "d" => {
            let p = props("dPr");
            MathNode::Delimiter {
                open: chr(p, "begChr", "("),
                close: chr(p, "endChr", ")"),
                separator: chr(p, "sepChr", "|"),
                items: element
                    .elements()
                    .filter(|e| e.is_m("e"))
                    .map(read_list)
                    .collect(),
            }
        }
        "nary" => {
            let p = props("naryPr");
            let hide = |name| p.is_some_and(|p| p.m_flag(name));
            MathNode::Nary {
                op: chr(p, "chr", "∫"),
                limits_over: match p.and_then(|p| p.m_val("limLoc")) {
                    Some("undOvr") => Some(true),
                    Some("subSup") => Some(false),
                    _ => None,
                },
                sub: (!hide("subHide")).then(|| cell(element, "sub")),
                sup: (!hide("supHide")).then(|| cell(element, "sup")),
                base: cell(element, "e"),
            }
        }
        "func" => MathNode::Function {
            name: cell(element, "fName"),
            base: cell(element, "e"),
        },
        "acc" => MathNode::Accent {
            mark: chr(props("accPr"), "chr", "\u{302}"),
            base: cell(element, "e"),
        },
        "bar" => MathNode::Bar {
            top: props("barPr").and_then(|p| p.m_val("pos")) == Some("top"),
            base: cell(element, "e"),
        },
        "limLow" | "limUpp" => MathNode::Limit {
            upper: element.local == "limUpp",
            base: cell(element, "e"),
            limit: cell(element, "lim"),
        },
        "groupChr" => {
            let p = props("groupChrPr");
            MathNode::GroupChar {
                mark: chr(p, "chr", "\u{23df}"),
                top: p.and_then(|p| p.m_val("pos")) == Some("top"),
                base: cell(element, "e"),
            }
        }
        "m" => MathNode::Matrix(
            element
                .elements()
                .filter(|e| e.is_m("mr"))
                .map(|row| {
                    row.elements()
                        .filter(|e| e.is_m("e"))
                        .map(read_list)
                        .collect()
                })
                .collect(),
        ),
        "eqArr" => MathNode::EqArray(
            element
                .elements()
                .filter(|e| e.is_m("e"))
                .map(read_list)
                .collect(),
        ),
        // Свойства (`m:ctrlPr` и др.) не видны.
        local if local.ends_with("Pr") => return None,
        // Рамка, «фантом», вложенная формула и непонятое: содержимое.
        _ => {
            let inner = element
                .m_child("e")
                .map_or_else(|| read_list(element), read_list);
            MathNode::Group(inner)
        }
    };
    Some(node)
}

/// XML формулы: исходный, если формулу не меняли, иначе по дереву.
pub(crate) fn math_xml(math: &Math) -> String {
    if let Some(raw) = &math.raw {
        return raw.as_str().to_string();
    }
    let ns = format!(r#" xmlns:m="{M_NS}""#);
    if math.display {
        let jc = match math.align {
            MathAlign::Center => String::new(),
            MathAlign::Left => r#"<m:oMathParaPr><m:jc m:val="left"/></m:oMathParaPr>"#.into(),
            MathAlign::Right => r#"<m:oMathParaPr><m:jc m:val="right"/></m:oMathParaPr>"#.into(),
        };
        let lines: String = math
            .lines
            .iter()
            .map(|l| format!("<m:oMath>{}</m:oMath>", list_xml(l)))
            .collect();
        format!("<m:oMathPara{ns}>{jc}{lines}</m:oMathPara>")
    } else {
        let list: String = math.lines.iter().map(|l| list_xml(l)).collect();
        format!("<m:oMath{ns}>{list}</m:oMath>")
    }
}

fn list_xml(list: &[MathNode]) -> String {
    list.iter().map(node_xml).collect()
}

/// Ячейка `<m:name>…</m:name>`.
fn cell_xml(name: &str, list: &[MathNode]) -> String {
    format!("<m:{name}>{}</m:{name}>", list_xml(list))
}

/// Свойство со значением: `<m:name m:val="…"/>`.
fn val(name: &str, value: &str) -> String {
    format!(r#"<m:{name} m:val="{}"/>"#, escape_attr(value))
}

fn node_xml(node: &MathNode) -> String {
    match node {
        MathNode::Text { text, plain } => {
            let props = if *plain {
                format!("<m:rPr>{}</m:rPr>", val("sty", "p"))
            } else {
                String::new()
            };
            format!(
                r#"<m:r>{props}<w:rPr><w:rFonts w:ascii="Cambria Math" w:hAnsi="Cambria Math"/></w:rPr><m:t xml:space="preserve">{}</m:t></m:r>"#,
                escape_text(text)
            )
        }
        MathNode::Fraction { kind, num, den } => {
            let kind = match kind {
                FractionKind::Bar => String::new(),
                FractionKind::NoBar => format!("<m:fPr>{}</m:fPr>", val("type", "noBar")),
                FractionKind::Skewed => format!("<m:fPr>{}</m:fPr>", val("type", "skw")),
                FractionKind::Linear => format!("<m:fPr>{}</m:fPr>", val("type", "lin")),
            };
            format!(
                "<m:f>{kind}{}{}</m:f>",
                cell_xml("num", num),
                cell_xml("den", den)
            )
        }
        MathNode::Scripts {
            base,
            sub,
            sup,
            pre,
        } => {
            let name = match (pre, sub, sup) {
                (true, _, _) => "sPre",
                (false, Some(_), Some(_)) => "sSubSup",
                (false, Some(_), None) => "sSub",
                (false, None, _) => "sSup",
            };
            let sub = sub.as_deref().map_or(String::new(), |s| cell_xml("sub", s));
            let sup = sup.as_deref().map_or(String::new(), |s| cell_xml("sup", s));
            let base = cell_xml("e", base);
            if *pre {
                format!("<m:{name}>{sub}{sup}{base}</m:{name}>")
            } else {
                format!("<m:{name}>{base}{sub}{sup}</m:{name}>")
            }
        }
        MathNode::Radical { degree, base } => {
            let props = if degree.is_none() {
                format!("<m:radPr>{}</m:radPr>", val("degHide", "1"))
            } else {
                String::new()
            };
            format!(
                "<m:rad>{props}{}{}</m:rad>",
                cell_xml("deg", degree.as_deref().unwrap_or_default()),
                cell_xml("e", base)
            )
        }
        MathNode::Delimiter {
            open,
            close,
            separator,
            items,
        } => {
            let mut props = String::new();
            if open != "(" {
                props.push_str(&val("begChr", open));
            }
            if separator != "|" {
                props.push_str(&val("sepChr", separator));
            }
            if close != ")" {
                props.push_str(&val("endChr", close));
            }
            let items: String = items.iter().map(|i| cell_xml("e", i)).collect();
            format!("<m:d><m:dPr>{props}</m:dPr>{items}</m:d>")
        }
        MathNode::Nary {
            op,
            limits_over,
            sub,
            sup,
            base,
        } => {
            let mut props = String::new();
            if op != "∫" {
                props.push_str(&val("chr", op));
            }
            match limits_over {
                Some(true) => props.push_str(&val("limLoc", "undOvr")),
                Some(false) => props.push_str(&val("limLoc", "subSup")),
                None => {}
            }
            if sub.is_none() {
                props.push_str(&val("subHide", "1"));
            }
            if sup.is_none() {
                props.push_str(&val("supHide", "1"));
            }
            format!(
                "<m:nary><m:naryPr>{props}</m:naryPr>{}{}{}</m:nary>",
                cell_xml("sub", sub.as_deref().unwrap_or_default()),
                cell_xml("sup", sup.as_deref().unwrap_or_default()),
                cell_xml("e", base)
            )
        }
        MathNode::Function { name, base } => format!(
            "<m:func>{}{}</m:func>",
            cell_xml("fName", name),
            cell_xml("e", base)
        ),
        MathNode::Accent { mark, base } => format!(
            "<m:acc><m:accPr>{}</m:accPr>{}</m:acc>",
            val("chr", mark),
            cell_xml("e", base)
        ),
        MathNode::Bar { top, base } => format!(
            "<m:bar><m:barPr>{}</m:barPr>{}</m:bar>",
            val("pos", if *top { "top" } else { "bot" }),
            cell_xml("e", base)
        ),
        MathNode::Limit { upper, base, limit } => {
            let name = if *upper { "limUpp" } else { "limLow" };
            format!(
                "<m:{name}>{}{}</m:{name}>",
                cell_xml("e", base),
                cell_xml("lim", limit)
            )
        }
        MathNode::GroupChar { mark, top, base } => format!(
            "<m:groupChr><m:groupChrPr>{}{}</m:groupChrPr>{}</m:groupChr>",
            val("chr", mark),
            val("pos", if *top { "top" } else { "bot" }),
            cell_xml("e", base)
        ),
        MathNode::Matrix(rows) => {
            let rows: String = rows
                .iter()
                .map(|row| {
                    let cells: String = row.iter().map(|c| cell_xml("e", c)).collect();
                    format!("<m:mr>{cells}</m:mr>")
                })
                .collect();
            format!("<m:m>{rows}</m:m>")
        }
        MathNode::EqArray(rows) => {
            let rows: String = rows.iter().map(|r| cell_xml("e", r)).collect();
            format!("<m:eqArr>{rows}</m:eqArr>")
        }
        MathNode::Group(list) => format!("<m:box>{}</m:box>", cell_xml("e", list)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml;

    const QUADRATIC: &str = r#"<m:oMathPara xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><m:oMath><m:r><m:t>x=</m:t></m:r><m:f><m:num><m:r><m:t>-b±</m:t></m:r><m:rad><m:radPr><m:degHide m:val="1"/></m:radPr><m:deg/><m:e><m:sSup><m:e><m:r><m:t>b</m:t></m:r></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup><m:r><m:t>-4ac</m:t></m:r></m:e></m:rad></m:num><m:den><m:r><m:t>2a</m:t></m:r></m:den></m:f></m:oMath></m:oMathPara>"#;

    fn parse(text: &str) -> Math {
        let root = xml::parse(text.as_bytes()).expect("XML");
        read_math(&root).expect("формула")
    }

    #[test]
    fn reads_quadratic_formula() {
        let math = parse(QUADRATIC);
        assert!(math.display);
        assert_eq!(math.lines.len(), 1);
        assert_eq!(math.linear_text(), "x=(-b±√(b^2-4ac))/(2a)");
    }

    #[test]
    fn reads_nary_function_and_delimiters() {
        let text = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:nary><m:naryPr><m:chr m:val="∑"/><m:limLoc m:val="undOvr"/></m:naryPr><m:sub><m:r><m:t>i=1</m:t></m:r></m:sub><m:sup><m:r><m:t>n</m:t></m:r></m:sup><m:e><m:func><m:fName><m:r><m:rPr><m:sty m:val="p"/></m:rPr><m:t>sin</m:t></m:r></m:fName><m:e><m:d><m:dPr><m:begChr m:val="["/><m:endChr m:val="]"/></m:dPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:d></m:e></m:func></m:e></m:nary></m:oMath>"#;
        let math = parse(text);
        assert!(!math.display);
        let [
            MathNode::Nary {
                op,
                limits_over,
                base,
                ..
            },
        ] = math.lines[0].as_slice()
        else {
            panic!("ждали оператор: {:?}", math.lines);
        };
        assert_eq!(op, "∑");
        assert_eq!(*limits_over, Some(true));
        let [MathNode::Function { name, .. }] = base.as_slice() else {
            panic!("ждали функцию");
        };
        assert_eq!(
            name[0],
            MathNode::Text {
                text: "sin".into(),
                plain: true
            }
        );
        assert_eq!(math.linear_text(), "∑_(i=1)^n▒(sin⁡[x])");
    }

    #[test]
    fn tree_written_without_raw_reads_back_the_same() {
        let mut math = parse(QUADRATIC);
        math.raw = None;
        let written = math_xml(&math);
        let root = xml::parse(
            written
                .replacen(
                    "<m:oMathPara",
                    r#"<m:oMathPara xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#,
                    1,
                )
                .as_bytes(),
        )
        .expect("XML");
        let mut again = read_math(&root).expect("формула");
        again.raw = None;
        assert_eq!(again, math);
    }

    #[test]
    fn other_elements_are_not_math() {
        let root = xml::parse(br#"<w:r xmlns:w="urn:w"/>"#).expect("XML");
        assert!(read_math(&root).is_none());
    }
}
