//! Списки (`word/numbering.xml`): чтение и запись.

use letter_model::{
    AbstractNumbering, LevelOverride, LevelSuffix, ListLevel, NumberFormat, Numbering,
    NumberingInstance,
};

use crate::document::{read_paragraph_props, read_run_props, toggle};
use crate::names::{ALIGNMENTS, lookup, name_of};
use crate::serialize::{
    Children, element_with, paragraph_props, root_attributes, run_props, val, xml_part,
};
use crate::xml::Element;

/// Виды номеров (`w:numFmt/@w:val`).
pub(crate) const FORMATS: &[(&str, NumberFormat)] = &[
    ("decimal", NumberFormat::Decimal),
    ("decimalZero", NumberFormat::DecimalZero),
    ("upperRoman", NumberFormat::UpperRoman),
    ("lowerRoman", NumberFormat::LowerRoman),
    ("upperLetter", NumberFormat::UpperLetter),
    ("lowerLetter", NumberFormat::LowerLetter),
    ("russianUpper", NumberFormat::RussianUpper),
    ("russianLower", NumberFormat::RussianLower),
    ("ordinal", NumberFormat::Ordinal),
    ("bullet", NumberFormat::Bullet),
    ("none", NumberFormat::None),
];

/// Что после номера (`w:suff/@w:val`).
const SUFFIXES: &[(&str, LevelSuffix)] = &[
    ("tab", LevelSuffix::Tab),
    ("space", LevelSuffix::Space),
    ("nothing", LevelSuffix::Nothing),
];

/// Порядок дочерних элементов по схеме.
const NUMBERING_ORDER: &[&str] = &["numPicBullet", "abstractNum", "num", "numIdMacAtCleanup"];
const ABSTRACT_ORDER: &[&str] = &[
    "nsid",
    "multiLevelType",
    "tmpl",
    "name",
    "styleLink",
    "numStyleLink",
    "lvl",
];
const LEVEL_ORDER: &[&str] = &[
    "start",
    "numFmt",
    "lvlRestart",
    "pStyle",
    "isLgl",
    "suff",
    "lvlText",
    "lvlPicBulletId",
    "legacy",
    "lvlJc",
    "pPr",
    "rPr",
];
const NUM_ORDER: &[&str] = &["abstractNumId", "lvlOverride"];
const OVERRIDE_ORDER: &[&str] = &["startOverride", "lvl"];

/// Превращает `w:numbering` в модель. Всё непонятое сохраняется.
pub(crate) fn read_numbering(root: &Element) -> Numbering {
    let mut numbering = Numbering::default();
    if !root.is_w("numbering") {
        return numbering;
    }
    for child in root.elements() {
        if child.is_w("abstractNum") {
            match read_abstract(child) {
                Some(a) => numbering.abstracts.push(a),
                None => numbering.unknown.push(child.raw()),
            }
        } else if child.is_w("num") {
            match read_instance(child) {
                Some(n) => numbering.instances.push(n),
                None => numbering.unknown.push(child.raw()),
            }
        } else {
            numbering.unknown.push(child.raw());
        }
    }
    numbering
}

/// Атрибуты, кроме `w:<main>`: имя как в файле и значение.
fn other_attributes(element: &Element, main: &str) -> Vec<(String, String)> {
    element
        .attrs
        .iter()
        .filter(|a| !(a.local == main && crate::document::is_w_ns(a.ns.as_deref())))
        .map(|a| (a.qname.clone(), a.value.clone()))
        .collect()
}

/// Число из `w:val` элемента, у которого других атрибутов нет.
fn number_val(element: &Element) -> Option<u32> {
    element
        .only_attrs(&["val"])
        .then(|| element.w_attr("val"))
        .flatten()
        .and_then(|v| v.trim().parse().ok())
}

/// Строка из `w:val` элемента, у которого других атрибутов нет.
fn string_val(element: &Element) -> Option<String> {
    element
        .only_attrs(&["val"])
        .then(|| element.w_attr("val"))
        .flatten()
        .map(str::to_owned)
}

fn read_abstract(element: &Element) -> Option<AbstractNumbering> {
    let id = element.w_attr("abstractNumId")?.trim().parse().ok()?;
    let mut abstract_numbering = AbstractNumbering {
        id,
        attributes: other_attributes(element, "abstractNumId"),
        ..Default::default()
    };
    for child in element.elements() {
        match child.is_w("lvl").then(|| read_level(child)).flatten() {
            Some(level) => abstract_numbering.levels.push(level),
            None => abstract_numbering.unknown.push(child.raw()),
        }
    }
    Some(abstract_numbering)
}

fn read_level(element: &Element) -> Option<ListLevel> {
    let level = element.w_attr("ilvl")?.trim().parse().ok()?;
    let mut lvl = ListLevel {
        level,
        attributes: other_attributes(element, "ilvl"),
        ..Default::default()
    };
    for child in element.elements() {
        let understood = crate::document::is_w_ns(child.ns.as_deref())
            && match child.local.as_str() {
                "start" => number_val(child).map(|v| lvl.start = Some(v)).is_some(),
                "numFmt" => string_val(child)
                    .map(|v| {
                        let known = FORMATS.iter().find(|(name, _)| *name == v);
                        lvl.format = Some(known.map_or(NumberFormat::Other(v), |(_, f)| f.clone()));
                    })
                    .is_some(),
                "lvlRestart" => number_val(child)
                    .and_then(|v| u8::try_from(v).ok())
                    .map(|v| lvl.restart = Some(v))
                    .is_some(),
                "pStyle" => string_val(child).map(|v| lvl.style = Some(v)).is_some(),
                "isLgl" => toggle(child).map(|v| lvl.legal = Some(v)).is_some(),
                "suff" => string_val(child)
                    .and_then(|v| lookup(SUFFIXES, &v))
                    .map(|v| lvl.suffix = Some(v))
                    .is_some(),
                "lvlText" => string_val(child).map(|v| lvl.text = Some(v)).is_some(),
                "lvlJc" => string_val(child)
                    .and_then(|v| lookup(ALIGNMENTS, &v))
                    .map(|v| lvl.alignment = Some(v))
                    .is_some(),
                "pPr" => {
                    lvl.paragraph = read_paragraph_props(child, false).0;
                    true
                }
                "rPr" => {
                    lvl.run = read_run_props(child);
                    true
                }
                _ => false,
            };
        if !understood {
            lvl.unknown.push(child.raw());
        }
    }
    Some(lvl)
}

fn read_instance(element: &Element) -> Option<NumberingInstance> {
    let id = element.w_attr("numId")?.trim().parse().ok()?;
    let mut instance = NumberingInstance {
        id,
        attributes: other_attributes(element, "numId"),
        ..Default::default()
    };
    let mut has_abstract = false;
    for child in element.elements() {
        if child.is_w("abstractNumId") && !has_abstract {
            if let Some(v) = number_val(child) {
                instance.abstract_id = v;
                has_abstract = true;
                continue;
            }
        } else if child.is_w("lvlOverride")
            && let Some(over) = read_override(child)
        {
            instance.overrides.push(over);
            continue;
        }
        instance.unknown.push(child.raw());
    }
    has_abstract.then_some(instance)
}

fn read_override(element: &Element) -> Option<LevelOverride> {
    if !element.only_attrs(&["ilvl"]) {
        return None;
    }
    let mut over = LevelOverride {
        level: element.w_attr("ilvl")?.trim().parse().ok()?,
        ..Default::default()
    };
    for child in element.elements() {
        if child.is_w("startOverride") && over.start.is_none() {
            if let Some(v) = number_val(child) {
                over.start = Some(v);
                continue;
            }
        } else if child.is_w("lvl")
            && over.definition.is_none()
            && let Some(level) = read_level(child)
        {
            over.definition = Some(level);
            continue;
        }
        over.unknown.push(child.raw());
    }
    Some(over)
}

/// Часть `word/numbering.xml` из модели.
pub(crate) fn numbering_xml(
    numbering: &Numbering,
    original_root: Option<&[(String, String)]>,
) -> String {
    let mut children = Children::new(NUMBERING_ORDER);
    for a in &numbering.abstracts {
        children.push("abstractNum", abstract_xml(a));
    }
    for n in &numbering.instances {
        children.push("num", instance_xml(n));
    }
    children.raw(&numbering.unknown);
    let attrs = root_attributes(original_root);
    xml_part(&element_with("w:numbering", &attrs, &children.finish()))
}

/// Атрибуты элемента: главный (`w:abstractNumId`) и остальные как были.
fn attributes(main: (&str, String), others: &[(String, String)]) -> Vec<(String, String)> {
    std::iter::once((main.0.to_owned(), main.1))
        .chain(others.iter().cloned())
        .collect()
}

fn abstract_xml(a: &AbstractNumbering) -> String {
    let mut children = Children::new(ABSTRACT_ORDER);
    for level in &a.levels {
        children.push("lvl", level_xml(level));
    }
    children.raw(&a.unknown);
    let attrs = attributes(("w:abstractNumId", a.id.to_string()), &a.attributes);
    element_with("w:abstractNum", &attrs, &children.finish())
}

fn level_xml(level: &ListLevel) -> String {
    let mut children = Children::new(LEVEL_ORDER);
    if let Some(start) = level.start {
        children.push("start", val("w:start", &start.to_string()));
    }
    if let Some(format) = &level.format {
        let name = match format {
            NumberFormat::Other(name) => name.as_str(),
            known => FORMATS
                .iter()
                .find(|(_, f)| f == known)
                .map_or("decimal", |(n, _)| n),
        };
        children.push("numFmt", val("w:numFmt", name));
    }
    if let Some(restart) = level.restart {
        children.push("lvlRestart", val("w:lvlRestart", &restart.to_string()));
    }
    if let Some(style) = &level.style {
        children.push("pStyle", val("w:pStyle", style));
    }
    if let Some(legal) = level.legal {
        children.push("isLgl", crate::serialize::toggle("w:isLgl", legal));
    }
    if let Some(name) = level.suffix.and_then(|s| name_of(SUFFIXES, s)) {
        children.push("suff", val("w:suff", name));
    }
    if let Some(text) = &level.text {
        children.push("lvlText", val("w:lvlText", text));
    }
    if let Some(name) = level.alignment.and_then(|a| name_of(ALIGNMENTS, a)) {
        children.push("lvlJc", val("w:lvlJc", name));
    }
    let paragraph = paragraph_props(&level.paragraph, None);
    if !paragraph.is_empty() {
        children.push("pPr", paragraph);
    }
    let run = run_props(&level.run);
    if !run.is_empty() {
        children.push("rPr", run);
    }
    children.raw(&level.unknown);
    let attrs = attributes(("w:ilvl", level.level.to_string()), &level.attributes);
    element_with("w:lvl", &attrs, &children.finish())
}

fn instance_xml(n: &NumberingInstance) -> String {
    let mut children = Children::new(NUM_ORDER);
    children.push(
        "abstractNumId",
        val("w:abstractNumId", &n.abstract_id.to_string()),
    );
    for over in &n.overrides {
        let mut inner = Children::new(OVERRIDE_ORDER);
        if let Some(start) = over.start {
            inner.push("startOverride", val("w:startOverride", &start.to_string()));
        }
        if let Some(level) = &over.definition {
            inner.push("lvl", level_xml(level));
        }
        inner.raw(&over.unknown);
        let attrs = vec![("w:ilvl".to_owned(), over.level.to_string())];
        children.push(
            "lvlOverride",
            element_with("w:lvlOverride", &attrs, &inner.finish()),
        );
    }
    children.raw(&n.unknown);
    let attrs = attributes(("w:numId", n.id.to_string()), &n.attributes);
    element_with("w:num", &attrs, &children.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::{Alignment, FirstLineIndent, Twips};

    const SAMPLE: &str = r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w15="http://schemas.microsoft.com/office/word/2012/wordml">
<w:abstractNum w:abstractNumId="0" w15:restartNumberingAfterBreak="0">
<w:nsid w:val="1A2B3C4D"/><w:multiLevelType w:val="hybridMultilevel"/>
<w:lvl w:ilvl="0" w:tplc="0419000F"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl>
<w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:suff w:val="space"/><w:lvlText w:val="%2)"/><w:lvlJc w:val="left"/><w:rPr><w:b/></w:rPr></w:lvl>
<w:lvl w:ilvl="2"><w:numFmt w:val="chineseCounting"/><w:lvlText w:val="%3"/><w:legacy w:legacy="1"/></w:lvl>
</w:abstractNum>
<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
<w:num w:numId="2"><w:abstractNumId w:val="0"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="3"/></w:lvlOverride></w:num>
</w:numbering>"#;

    #[test]
    fn read_and_write_numbering() {
        let root = crate::xml::parse(SAMPLE.as_bytes()).unwrap();
        let numbering = read_numbering(&root);
        assert_eq!(numbering.abstracts.len(), 1);
        let a = &numbering.abstracts[0];
        assert_eq!(
            a.attributes,
            [("w15:restartNumberingAfterBreak".to_owned(), "0".to_owned())]
        );
        assert_eq!(a.unknown.len(), 2);
        let first = &a.levels[0];
        assert_eq!(first.format, Some(NumberFormat::Decimal));
        assert_eq!(first.text.as_deref(), Some("%1."));
        assert_eq!(first.alignment, Some(Alignment::Left));
        assert_eq!(first.paragraph.indentation.left, Some(Twips(720)));
        assert_eq!(
            first.paragraph.indentation.first_line,
            Some(FirstLineIndent::Hanging(Twips(360)))
        );
        assert_eq!(a.levels[1].suffix, Some(LevelSuffix::Space));
        assert_eq!(a.levels[1].run.bold, Some(true));
        assert_eq!(
            a.levels[2].format,
            Some(NumberFormat::Other("chineseCounting".into()))
        );
        assert_eq!(a.levels[2].unknown.len(), 1);
        assert_eq!(numbering.level(2, 0).and_then(|l| l.start), Some(3));

        let xml = numbering_xml(&numbering, Some(&root.root_attributes()));
        let again = read_numbering(&crate::xml::parse(xml.as_bytes()).unwrap());
        assert_eq!(again, numbering);
        assert!(xml.contains("w15:restartNumberingAfterBreak=\"0\""));
        assert!(xml.contains("<w:lvl w:ilvl=\"0\" w:tplc=\"0419000F\">"));
    }

    #[test]
    fn broken_parts_are_kept() {
        let xml = r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum/><w:num w:numId="x"/><w:numPicBullet w:numPicBulletId="0"/></w:numbering>"#;
        let numbering = read_numbering(&crate::xml::parse(xml.as_bytes()).unwrap());
        assert!(numbering.abstracts.is_empty() && numbering.instances.is_empty());
        assert_eq!(numbering.unknown.len(), 3);
        let out = numbering_xml(&numbering, None);
        assert!(out.find("numPicBullet") < out.find("abstractNum"));
    }
}
