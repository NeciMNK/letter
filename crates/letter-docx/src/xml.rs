//! Небольшое дерево XML поверх `quick-xml` с поддержкой пространств имён.
//!
//! Части .docx невелики, поэтому удобнее сначала прочитать часть целиком
//! в дерево, а потом спокойно разбирать его. Дерево умеет записать любой
//! свой элемент обратно в строку — так сохраняется непонятое содержимое.

use std::collections::BTreeMap;
use std::rc::Rc;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::ReadError;

/// Пространства имён, видимые в точке документа: префикс → URI.
/// Пустой префикс — пространство имён по умолчанию.
pub(crate) type Scope = Rc<BTreeMap<String, String>>;

/// Элемент XML.
#[derive(Debug, Clone)]
pub(crate) struct Element {
    /// Имя как в файле, с префиксом: `w:p`.
    pub qname: String,
    /// URI пространства имён элемента.
    pub ns: Option<String>,
    /// Имя без префикса: `p`.
    pub local: String,
    /// Атрибуты, кроме объявлений `xmlns`.
    pub attrs: Vec<Attr>,
    /// Объявления `xmlns`, сделанные на самом элементе.
    pub decls: Vec<(String, String)>,
    /// Пространства имён, видимые в этом элементе.
    pub scope: Scope,
    /// Дочерние узлы.
    pub children: Vec<Node>,
}

/// Атрибут XML.
#[derive(Debug, Clone)]
pub(crate) struct Attr {
    /// Имя как в файле: `w:val`.
    pub qname: String,
    /// URI пространства имён (у атрибутов без префикса его нет).
    pub ns: Option<String>,
    /// Имя без префикса: `val`.
    pub local: String,
    /// Значение без экранирования.
    pub value: String,
}

/// Узел дерева.
#[derive(Debug, Clone)]
pub(crate) enum Node {
    Element(Element),
    Text(String),
}

fn split_qname(qname: &str) -> (&str, &str) {
    qname.split_once(':').unwrap_or(("", qname))
}

/// Превращает байты части в строку: UTF-8 (с BOM или без) или UTF-16 с BOM.
pub(crate) fn decode_bytes(bytes: &[u8]) -> Result<String, ReadError> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(rest.to_vec()).map_err(|e| ReadError::xml(e.to_string()));
    }
    let utf16 = |rest: &[u8], le: bool| {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| {
                if le {
                    u16::from_le_bytes(pair)
                } else {
                    u16::from_be_bytes(pair)
                }
            })
            .collect();
        String::from_utf16(&units).map_err(|e| ReadError::xml(e.to_string()))
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, false);
    }
    String::from_utf8(bytes.to_vec()).map_err(|e| ReadError::xml(e.to_string()))
}

/// Читает документ XML и возвращает корневой элемент.
pub(crate) fn parse(bytes: &[u8]) -> Result<Element, ReadError> {
    let text = decode_bytes(bytes)?;
    // XML-объявление может говорить о другой кодировке, но строка уже в UTF-8.
    let mut reader = Reader::from_str(&text);
    reader.config_mut().trim_text(false);

    let root_scope: Scope = Rc::new(BTreeMap::from([(
        "xml".to_string(),
        "http://www.w3.org/XML/1998/namespace".to_string(),
    )]));
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;

    loop {
        let event = reader
            .read_event()
            .map_err(|e| ReadError::xml(e.to_string()))?;
        match event {
            Event::Start(start) => {
                let scope = stack.last().map_or(&root_scope, |e| &e.scope).clone();
                stack.push(start_element(&start, scope)?);
            }
            Event::Empty(start) => {
                let scope = stack.last().map_or(&root_scope, |e| &e.scope).clone();
                let element = start_element(&start, scope)?;
                attach(&mut stack, &mut root, element)?;
            }
            Event::End(_) => {
                let element = stack
                    .pop()
                    .ok_or_else(|| ReadError::xml("unexpected closing tag"))?;
                attach(&mut stack, &mut root, element)?;
            }
            Event::Text(t) => push_text(&mut stack, &t.xml10_content()),
            Event::CData(c) => push_text(&mut stack, &c.xml10_content()),
            Event::GeneralRef(r) => {
                let ch = match r.resolve_char_ref() {
                    Ok(Some(ch)) => ch.to_string(),
                    Ok(None) => predefined_entity(&r.xml10_content())
                        .ok_or_else(|| ReadError::xml("unknown entity"))?
                        .to_string(),
                    Err(e) => return Err(ReadError::xml(e.to_string())),
                };
                push_text(&mut stack, &ch);
            }
            Event::Eof => break,
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) | Event::DocType(_) => {}
        }
    }
    if !stack.is_empty() {
        return Err(ReadError::xml("unclosed element"));
    }
    root.ok_or_else(|| ReadError::xml("no root element"))
}

fn predefined_entity(name: &str) -> Option<char> {
    match name {
        "lt" => Some('<'),
        "gt" => Some('>'),
        "amp" => Some('&'),
        "apos" => Some('\''),
        "quot" => Some('"'),
        _ => None,
    }
}

fn push_text(stack: &mut [Element], text: &str) {
    // Текст вне корневого элемента (пробелы, переводы строк) не нужен.
    if let Some(parent) = stack.last_mut() {
        if let Some(Node::Text(last)) = parent.children.last_mut() {
            last.push_str(text);
        } else {
            parent.children.push(Node::Text(text.to_string()));
        }
    }
}

fn attach(
    stack: &mut [Element],
    root: &mut Option<Element>,
    element: Element,
) -> Result<(), ReadError> {
    match stack.last_mut() {
        Some(parent) => parent.children.push(Node::Element(element)),
        None if root.is_none() => *root = Some(element),
        None => return Err(ReadError::xml("more than one root element")),
    }
    Ok(())
}

fn start_element(start: &BytesStart<'_>, parent_scope: Scope) -> Result<Element, ReadError> {
    let qname = start.name().0.to_string();
    let mut raw_attrs = Vec::new();
    let mut decls = Vec::new();
    for attr in start.attributes() {
        let attr = attr.map_err(|e| ReadError::xml(e.to_string()))?;
        let name = attr.key.0.to_string();
        let value = attr
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| ReadError::xml(e.to_string()))?
            .into_owned();
        if name == "xmlns" {
            decls.push((String::new(), value));
        } else if let Some(prefix) = name.strip_prefix("xmlns:") {
            decls.push((prefix.to_string(), value));
        } else {
            raw_attrs.push((name, value));
        }
    }
    let scope = if decls.is_empty() {
        parent_scope
    } else {
        let mut map = (*parent_scope).clone();
        for (prefix, uri) in &decls {
            map.insert(prefix.clone(), uri.clone());
        }
        Rc::new(map)
    };
    let (prefix, local) = split_qname(&qname);
    let ns = scope.get(prefix).filter(|uri| !uri.is_empty()).cloned();
    let local = local.to_string();
    let attrs = raw_attrs
        .into_iter()
        .map(|(qname, value)| {
            let (prefix, local) = split_qname(&qname);
            let ns = if prefix.is_empty() {
                None
            } else {
                scope.get(prefix).cloned()
            };
            let local = local.to_string();
            Attr {
                qname,
                ns,
                local,
                value,
            }
        })
        .collect();
    Ok(Element {
        qname,
        ns,
        local,
        attrs,
        decls,
        scope,
        children: Vec::new(),
    })
}

impl Element {
    /// Дочерние элементы (без текста).
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    /// Весь текст внутри элемента, включая вложенные элементы.
    pub fn text(&self) -> String {
        let mut out = String::new();
        self.collect_text(&mut out);
        out
    }

    fn collect_text(&self, out: &mut String) {
        for child in &self.children {
            match child {
                Node::Text(t) => out.push_str(t),
                Node::Element(e) => e.collect_text(out),
            }
        }
    }

    /// Атрибуты элемента для записи в том же виде: сначала объявления
    /// пространств имён (`xmlns:w`), потом обычные атрибуты.
    pub fn root_attributes(&self) -> Vec<(String, String)> {
        let decls = self.decls.iter().map(|(prefix, uri)| {
            let name = if prefix.is_empty() {
                "xmlns".to_string()
            } else {
                format!("xmlns:{prefix}")
            };
            (name, uri.clone())
        });
        let attrs = self
            .attrs
            .iter()
            .map(|a| (a.qname.clone(), a.value.clone()));
        decls.chain(attrs).collect()
    }

    /// Записывает элемент обратно в строку XML. В корень добавляются
    /// объявления тех пространств имён, которые нужны внутри фрагмента,
    /// но объявлены где-то выше.
    pub fn to_xml_fragment(&self) -> String {
        let mut used = std::collections::BTreeSet::new();
        self.collect_prefixes(&mut used);
        let mut extra = Vec::new();
        for prefix in used {
            if prefix == "xml" || self.decls.iter().any(|(p, _)| *p == prefix) {
                continue;
            }
            if let Some(uri) = self.scope.get(&prefix) {
                extra.push((prefix, uri.clone()));
            }
        }
        let mut out = String::new();
        self.write(&mut out, &extra);
        out
    }

    /// Собирает префиксы, которые встречаются в именах элементов и атрибутов,
    /// а также в значениях атрибутов (`mc:Ignorable="w14"`, `xsi:type="w:x"`).
    fn collect_prefixes(&self, used: &mut std::collections::BTreeSet<String>) {
        used.insert(split_qname(&self.qname).0.to_string());
        for attr in &self.attrs {
            let (prefix, _) = split_qname(&attr.qname);
            if !prefix.is_empty() {
                used.insert(prefix.to_string());
            }
            for token in attr.value.split_whitespace() {
                let candidate = token.split_once(':').map_or(token, |(p, _)| p);
                if self.scope.contains_key(candidate) {
                    used.insert(candidate.to_string());
                }
            }
        }
        for child in self.elements() {
            child.collect_prefixes(used);
        }
    }

    fn write(&self, out: &mut String, extra_decls: &[(String, String)]) {
        out.push('<');
        out.push_str(&self.qname);
        for (prefix, uri) in self.decls.iter().chain(extra_decls) {
            if prefix.is_empty() {
                out.push_str(" xmlns=\"");
            } else {
                out.push_str(" xmlns:");
                out.push_str(prefix);
                out.push_str("=\"");
            }
            out.push_str(&crate::serialize::escape_attr(uri));
            out.push('"');
        }
        for attr in &self.attrs {
            out.push(' ');
            out.push_str(&attr.qname);
            out.push_str("=\"");
            out.push_str(&crate::serialize::escape_attr(&attr.value));
            out.push('"');
        }
        if self.children.is_empty() {
            out.push_str("/>");
            return;
        }
        out.push('>');
        for child in &self.children {
            match child {
                Node::Text(t) => out.push_str(&quick_xml::escape::partial_escape(t.as_str())),
                Node::Element(e) => e.write(out, &[]),
            }
        }
        out.push_str("</");
        out.push_str(&self.qname);
        out.push('>');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_namespaces_and_entities() {
        let xml = br#"<?xml version="1.0"?>
<a:root xmlns:a="urn:a" xmlns:b="urn:b"><b:child a:x="1 &amp; 2">t&lt;&#1046;</b:child></a:root>"#;
        let root = parse(xml).unwrap();
        assert_eq!(root.local, "root");
        assert_eq!(root.ns.as_deref(), Some("urn:a"));
        let child = root.elements().next().unwrap();
        assert_eq!(child.ns.as_deref(), Some("urn:b"));
        assert_eq!(child.attrs[0].ns.as_deref(), Some("urn:a"));
        assert_eq!(child.attrs[0].value, "1 & 2");
        assert_eq!(child.text(), "t<Ж");
    }

    #[test]
    fn attribute_whitespace_survives() {
        let root = parse(b"<a v=\"x&#9;y&#10;z\"/>").unwrap();
        assert_eq!(root.attrs[0].value, "x\ty\nz");
        assert_eq!(root.to_xml_fragment(), "<a v=\"x&#9;y&#10;z\"/>");
    }

    #[test]
    fn fragment_is_self_contained() {
        let xml = br#"<w:r xmlns:w="urn:w" xmlns:mc="urn:mc" xmlns:w14="urn:w14" xmlns:unused="urn:u"><mc:AlternateContent><mc:Choice Requires="w14"><w:t xml:space="preserve"> a&amp;b </w:t></mc:Choice></mc:AlternateContent></w:r>"#;
        let root = parse(xml).unwrap();
        let fragment = root.elements().next().unwrap().to_xml_fragment();
        assert_eq!(
            fragment,
            r#"<mc:AlternateContent xmlns:mc="urn:mc" xmlns:w="urn:w" xmlns:w14="urn:w14"><mc:Choice Requires="w14"><w:t xml:space="preserve"> a&amp;b </w:t></mc:Choice></mc:AlternateContent>"#
        );
        // Фрагмент снова читается и даёт то же самое.
        let again = parse(fragment.as_bytes()).unwrap();
        assert_eq!(again.to_xml_fragment(), fragment);
    }

    #[test]
    fn utf16_and_bom() {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in "<a>Ж</a>".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(parse(&bytes).unwrap().text(), "Ж");
        assert_eq!(parse(b"\xEF\xBB\xBF<a>x</a>").unwrap().text(), "x");
    }

    #[test]
    fn broken_xml_is_an_error() {
        assert!(parse(b"<a><b></a>").is_err());
        assert!(parse(b"<a>").is_err());
        assert!(parse(b"").is_err());
        assert!(parse(b"<a/><b/>").is_err());
        assert!(parse(b"\xFF\xFF\xFF").is_err());
        assert!(parse(b"<a>&bogus;</a>").is_err());
    }
}
