//! Небольшое дерево XML поверх `quick-xml`.
//!
//! Части ODT читаются целиком в дерево. Префиксы пространств имён
//! в файле могут быть любыми, поэтому имена элементов и атрибутов
//! переводятся в привычные префиксы ODF (`text:p`, `fo:margin-left`)
//! по адресу пространства имён.

use std::collections::HashMap;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::ReadError;

/// Привычные префиксы и адреса пространств имён ODF.
pub(crate) const NAMESPACES: &[(&str, &str)] = &[
    ("office", "urn:oasis:names:tc:opendocument:xmlns:office:1.0"),
    ("style", "urn:oasis:names:tc:opendocument:xmlns:style:1.0"),
    ("text", "urn:oasis:names:tc:opendocument:xmlns:text:1.0"),
    ("table", "urn:oasis:names:tc:opendocument:xmlns:table:1.0"),
    ("draw", "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"),
    (
        "fo",
        "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0",
    ),
    (
        "svg",
        "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0",
    ),
    ("meta", "urn:oasis:names:tc:opendocument:xmlns:meta:1.0"),
    ("config", "urn:oasis:names:tc:opendocument:xmlns:config:1.0"),
    (
        "manifest",
        "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0",
    ),
    ("dc", "http://purl.org/dc/elements/1.1/"),
    ("xlink", "http://www.w3.org/1999/xlink"),
    // Нужен в `settings.xml`: LibreOffice разбирает префикс в значении
    // `config:name="ooo:configuration-settings"`.
    ("ooo", "http://openoffice.org/2004/office"),
    (
        "loext",
        "urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0",
    ),
];

/// Элемент XML.
#[derive(Debug, Clone, Default)]
pub(crate) struct Element {
    /// Имя с привычным префиксом: `text:p`. Если пространство имён
    /// незнакомо, префикс — `?`.
    pub name: String,
    /// Атрибуты (имена тоже с привычными префиксами), кроме `xmlns`.
    pub attrs: Vec<(String, String)>,
    /// Дочерние узлы.
    pub children: Vec<Node>,
}

/// Узел дерева.
#[derive(Debug, Clone)]
pub(crate) enum Node {
    Element(Element),
    Text(String),
}

impl Element {
    /// Значение атрибута по имени с привычным префиксом.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Дочерние элементы (без текста).
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    /// Первый дочерний элемент с таким именем.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.name == name)
    }

    /// Весь текст внутри элемента, включая вложенные элементы.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for child in &self.children {
            match child {
                Node::Text(t) => out.push_str(t),
                Node::Element(e) => out.push_str(&e.text()),
            }
        }
        out
    }
}

/// Превращает байты части в строку (UTF-8, с меткой BOM или без).
fn decode(bytes: &[u8]) -> Result<&str, ReadError> {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    std::str::from_utf8(bytes).map_err(|e| ReadError::xml(e.to_string()))
}

/// Читает документ XML и возвращает корневой элемент.
pub(crate) fn parse(bytes: &[u8]) -> Result<Element, ReadError> {
    let text = decode(bytes)?;
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(false);

    // Видимые пространства имён: префикс → адрес, по уровням вложенности.
    let mut scopes: Vec<HashMap<String, String>> = vec![HashMap::from([(
        "xml".to_string(),
        "http://www.w3.org/XML/1998/namespace".to_string(),
    )])];
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;

    loop {
        let event = reader
            .read_event()
            .map_err(|e| ReadError::xml(e.to_string()))?;
        match event {
            Event::Start(start) => {
                let (element, scope) = start_element(&start, &scopes)?;
                scopes.push(scope);
                stack.push(element);
            }
            Event::Empty(start) => {
                let (element, _) = start_element(&start, &scopes)?;
                attach(&mut stack, &mut root, element)?;
            }
            Event::End(_) => {
                let element = stack
                    .pop()
                    .ok_or_else(|| ReadError::xml("лишний закрывающий тег"))?;
                scopes.pop();
                attach(&mut stack, &mut root, element)?;
            }
            Event::Text(t) => push_text(&mut stack, &t.xml10_content()),
            Event::CData(c) => push_text(&mut stack, &c.xml10_content()),
            Event::GeneralRef(r) => {
                let ch = match r.resolve_char_ref() {
                    Ok(Some(ch)) => ch,
                    Ok(None) => predefined_entity(&r.xml10_content())
                        .ok_or_else(|| ReadError::xml("неизвестная сущность"))?,
                    Err(e) => return Err(ReadError::xml(e.to_string())),
                };
                push_text(&mut stack, ch.encode_utf8(&mut [0; 4]));
            }
            Event::Eof => break,
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) | Event::DocType(_) => {}
        }
    }
    if !stack.is_empty() {
        return Err(ReadError::xml("незакрытый элемент"));
    }
    root.ok_or_else(|| ReadError::xml("нет корневого элемента"))
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
        None => return Err(ReadError::xml("больше одного корневого элемента")),
    }
    Ok(())
}

fn start_element(
    start: &BytesStart<'_>,
    scopes: &[HashMap<String, String>],
) -> Result<(Element, HashMap<String, String>), ReadError> {
    let mut raw_attrs = Vec::new();
    let mut scope = scopes.last().cloned().unwrap_or_default();
    for attr in start.attributes() {
        let attr = attr.map_err(|e| ReadError::xml(e.to_string()))?;
        let name = attr.key.0.to_string();
        let value = attr
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| ReadError::xml(e.to_string()))?
            .into_owned();
        if name == "xmlns" {
            scope.insert(String::new(), value);
        } else if let Some(prefix) = name.strip_prefix("xmlns:") {
            scope.insert(prefix.to_string(), value);
        } else {
            raw_attrs.push((name, value));
        }
    }
    let qname = start.name().0.to_string();
    let name = canonical(&qname, &scope, true);
    let attrs = raw_attrs
        .into_iter()
        .map(|(n, v)| (canonical(&n, &scope, false), v))
        .collect();
    let element = Element {
        name,
        attrs,
        children: Vec::new(),
    };
    Ok((element, scope))
}

/// Имя с привычным префиксом. У атрибутов без префикса пространства имён
/// нет — имя остаётся как есть.
fn canonical(qname: &str, scope: &HashMap<String, String>, is_element: bool) -> String {
    let (prefix, local) = qname.split_once(':').unwrap_or(("", qname));
    if prefix.is_empty() && !is_element {
        return local.to_string();
    }
    let uri = scope.get(prefix).map(String::as_str).unwrap_or("");
    if uri.is_empty() {
        return local.to_string();
    }
    match NAMESPACES.iter().find(|(_, u)| *u == uri) {
        Some((known, _)) => format!("{known}:{local}"),
        None if uri == "http://www.w3.org/XML/1998/namespace" => format!("xml:{local}"),
        None => format!("?:{local}"),
    }
}

/// Экранирует текст для XML (и для значений атрибутов в кавычках).
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // Управляющие символы XML 1.0 не допускает.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_are_normalised() {
        let xml = r#"<o:root xmlns:o="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
            xmlns:t="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
            xmlns:x="urn:example"><t:p t:style-name="a&amp;b" x:y="1" plain="2">Hi &#x41;</t:p></o:root>"#;
        let root = parse(xml.as_bytes()).unwrap_or_default();
        assert_eq!(root.name, "office:root");
        let p = root.child("text:p").cloned().unwrap_or_default();
        assert_eq!(p.attr("text:style-name"), Some("a&b"));
        assert_eq!(p.attr("?:y"), Some("1"));
        assert_eq!(p.attr("plain"), Some("2"));
        assert_eq!(p.text(), "Hi A");
    }

    #[test]
    fn broken_xml_is_an_error() {
        assert!(parse(b"<a><b></a>").is_err());
        assert!(parse(b"").is_err());
    }

    #[test]
    fn escaping() {
        assert_eq!(escape("a<b>&\"c\u{1}"), "a&lt;b&gt;&amp;&quot;c");
    }
}
