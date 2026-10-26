//! Элементы управления содержимым (`w:sdt`): поля формы, титульные
//! страницы, блоки с подсказкой «Введите текст» и т. п.

use std::sync::Arc;

use crate::RawXml;

/// Элемент управления содержимым (content control, `w:sdt`).
///
/// Содержимое элемента — обычные абзацы, таблицы или фрагменты текста,
/// которые лежат в документе на своих местах; у каждого из них в
/// оформлении есть ссылка ([`ContentControlRef`]) на элемент, в который
/// он входит. Соседние абзацы (фрагменты) с одной и той же ссылкой (тот же
/// [`Arc`], см. [`Arc::ptr_eq`]) при сохранении снова попадают в один
/// `w:sdt`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ContentControl {
    /// Свойства элемента (`w:sdtPr`) как есть: вид, подсказка, номер,
    /// привязка к данным.
    pub props: Option<RawXml>,
    /// Оформление знака конца элемента (`w:sdtEndPr`) как есть.
    pub end_props: Option<RawXml>,
}

impl ContentControl {
    /// Значение `w:val` элемента свойств с именем `local` (без префикса),
    /// например `"tag"` или `"alias"`.
    pub fn property(&self, local: &str) -> Option<String> {
        self.tags().find(|t| t.local == local)?.attr("val")
    }

    /// Пометка элемента (`w:tag`), по которой его находят программы.
    pub fn tag(&self) -> Option<String> {
        self.property("tag")
    }

    /// Название элемента (`w:alias`), которое Word показывает на рамке.
    pub fn title(&self) -> Option<String> {
        self.property("alias")
    }

    /// Вид элемента.
    pub fn kind(&self) -> ControlKind {
        let mut tags = self.tags().filter(|t| !t.closing);
        let mut kind = ControlKind::RichText;
        while let Some(tag) = tags.next() {
            kind = match tag.local {
                "text" => ControlKind::Text,
                "picture" => ControlKind::Picture,
                "docPartObj" | "docPartList" => ControlKind::Gallery,
                "checkbox" => {
                    let checked = self
                        .tags()
                        .find(|t| t.local == "checked")
                        .and_then(|t| t.attr("val"))
                        .is_some_and(|v| v == "1" || v == "true");
                    let mut symbols = CheckBoxSymbols::default();
                    for tag in self.tags() {
                        let symbol = tag
                            .attr("val")
                            .and_then(|v| u32::from_str_radix(&v, 16).ok())
                            .and_then(char::from_u32);
                        match (tag.local, symbol) {
                            ("checkedState", Some(c)) => symbols.checked = c,
                            ("uncheckedState", Some(c)) => symbols.unchecked = c,
                            _ => {}
                        }
                    }
                    return ControlKind::CheckBox { checked, symbols };
                }
                "dropDownList" | "comboBox" => {
                    let editable = tag.local == "comboBox";
                    let mut items = Vec::new();
                    if !tag.self_closing {
                        for item in tags.by_ref() {
                            if item.local == "listItem" {
                                let value = item.attr("value").unwrap_or_default();
                                let text =
                                    item.attr("displayText").unwrap_or_else(|| value.clone());
                                items.push(ListItem { text, value });
                            }
                        }
                    }
                    return ControlKind::List { items, editable };
                }
                "date" => {
                    let format = self.property("dateFormat");
                    return ControlKind::Date { format };
                }
                _ => continue,
            };
        }
        kind
    }

    /// Выбранная дата элемента выбора даты (`w:fullDate`): год, месяц,
    /// день.
    pub fn full_date(&self) -> Option<(i32, u32, u32)> {
        let full = self.tags().find(|t| t.local == "date")?.attr("fullDate")?;
        let mut parts = full.get(..10)?.split('-');
        let year = parts.next()?.parse().ok()?;
        let month = parts
            .next()?
            .parse()
            .ok()
            .filter(|m| (1..=12).contains(m))?;
        let day = parts
            .next()?
            .parse()
            .ok()
            .filter(|d| (1..=31).contains(d))?;
        Some((year, month, day))
    }

    /// Показывает ли элемент текст-подсказку (`w:showingPlcHdr`): тогда
    /// первый набранный текст заменяет её целиком.
    pub fn showing_placeholder(&self) -> bool {
        self.tags().any(|t| t.local == "showingPlcHdr")
    }

    /// Запрет правки (`w:lock`).
    pub fn lock(&self) -> ControlLock {
        match self.property("lock").as_deref() {
            Some("sdtLocked") => ControlLock::Control,
            Some("contentLocked") => ControlLock::Content,
            Some("sdtContentLocked") => ControlLock::Both,
            _ => ControlLock::None,
        }
    }

    /// Можно ли менять содержимое элемента.
    pub fn content_editable(&self) -> bool {
        !matches!(self.lock(), ControlLock::Content | ControlLock::Both)
    }

    /// Тот же элемент без пометки «показывает подсказку».
    pub fn without_placeholder(&self) -> ContentControl {
        let Some(props) = &self.props else {
            return self.clone();
        };
        let xml = props.as_str();
        let mut out = String::new();
        let mut last = 0;
        for tag in tags(xml) {
            if tag.local == "showingPlcHdr" && tag.self_closing {
                out.push_str(&xml[last..tag.range.start]);
                last = tag.range.end;
            }
        }
        out.push_str(&xml[last..]);
        ContentControl {
            props: Some(RawXml::new(out)),
            end_props: self.end_props.clone(),
        }
    }

    /// Флажок с отметкой `checked` (у других видов — без изменений).
    pub fn with_checked(&self, checked: bool) -> ContentControl {
        self.with_attr("checked", "val", if checked { "1" } else { "0" })
    }

    /// Дата с полной датой `full` (`w:fullDate`, например
    /// `2026-10-23T00:00:00Z`).
    pub fn with_full_date(&self, full: &str) -> ContentControl {
        self.with_attr("date", "fullDate", full)
    }

    /// Ставит атрибут `attr` первого элемента свойств `local` (добавляет,
    /// если его нет); без такого элемента возвращает копию.
    fn with_attr(&self, local: &str, attr: &str, value: &str) -> ContentControl {
        let Some(props) = &self.props else {
            return self.clone();
        };
        let xml = props.as_str();
        let Some(tag) = tags(xml).find(|t| t.local == local && !t.closing) else {
            return self.clone();
        };
        let escaped = escape(value);
        let text = &xml[tag.range.clone()];
        let replaced = match tag.attr_value_range(attr) {
            Some(range) => {
                let start = tag.range.start + range.start;
                let end = tag.range.start + range.end;
                format!("{}{escaped}{}", &xml[..start], &xml[end..])
            }
            None => {
                // Перед `>` или `/>`; префикс — как у самого элемента.
                let prefix = tag.name.rsplit_once(':').map(|(p, _)| p);
                let name = prefix.map_or(attr.to_string(), |p| format!("{p}:{attr}"));
                let insert = tag.range.start + text.len() - if tag.self_closing { 2 } else { 1 };
                format!(
                    "{} {name}=\"{escaped}\"{}",
                    xml[..insert].trim_end(),
                    &xml[insert..]
                )
            }
        };
        ContentControl {
            props: Some(RawXml::new(replaced)),
            end_props: self.end_props.clone(),
        }
    }

    /// Тот же элемент с названием `title` (`w:alias`), тегом `tag` и
    /// запретом `lock`; пустое название или тег убираются.
    pub fn with_settings(&self, title: &str, tag: &str, lock: ControlLock) -> ContentControl {
        let Some(props) = &self.props else {
            return self.clone();
        };
        let xml = props.as_str();
        let prefix = prefix_of(xml);
        let mut xml = remove_elements(xml, &["alias", "tag", "lock"]);
        let element = |local: &str, value: &str| {
            format!(r#"<{prefix}{local} {prefix}val="{}"/>"#, escape(value))
        };
        let mut named = String::new();
        if !title.is_empty() {
            named.push_str(&element("alias", title));
        }
        if !tag.is_empty() {
            named.push_str(&element("tag", tag));
        }
        // Порядок по схеме: rPr, alias, tag, id, lock…
        let at = element_end(&xml, "rPr").unwrap_or_else(|| open_end(&xml, "sdtPr"));
        xml.insert_str(at, &named);
        let lock = match lock {
            ControlLock::None => None,
            ControlLock::Control => Some("sdtLocked"),
            ControlLock::Content => Some("contentLocked"),
            ControlLock::Both => Some("sdtContentLocked"),
        };
        if let Some(lock) = lock {
            let at = element_end(&xml, "id").unwrap_or(at + named.len());
            xml.insert_str(at, &element("lock", lock));
        }
        self.with_xml(xml)
    }

    /// Тот же элемент с пометкой «показывает подсказку»
    /// (`w:showingPlcHdr`).
    pub fn with_placeholder(&self) -> ContentControl {
        let Some(props) = &self.props else {
            return self.clone();
        };
        if self.showing_placeholder() {
            return self.clone();
        }
        let mut xml = props.as_str().to_string();
        let prefix = prefix_of(&xml);
        // Порядок по схеме: …, id, lock, placeholder, temporary, showingPlcHdr.
        let at = [
            "temporary",
            "placeholder",
            "lock",
            "id",
            "tag",
            "alias",
            "rPr",
        ]
        .iter()
        .find_map(|local| element_end(&xml, local))
        .unwrap_or_else(|| open_end(&xml, "sdtPr"));
        xml.insert_str(at, &format!("<{prefix}showingPlcHdr/>"));
        self.with_xml(xml)
    }

    /// Тот же список (`w:dropDownList` или `w:comboBox`) с пунктами
    /// `items`.
    pub fn with_list_items(&self, items: &[ListItem]) -> ContentControl {
        let Some(props) = &self.props else {
            return self.clone();
        };
        let xml = props.as_str();
        let prefix = prefix_of(xml);
        let xml = remove_elements(xml, &["listItem"]);
        let list: String = items
            .iter()
            .map(|item| {
                format!(
                    r#"<{prefix}listItem {prefix}displayText="{}" {prefix}value="{}"/>"#,
                    escape(&item.text),
                    escape(&item.value)
                )
            })
            .collect();
        let xml = ["dropDownList", "comboBox"]
            .iter()
            .find_map(|local| insert_into(&xml, local, &list))
            .unwrap_or(xml);
        self.with_xml(xml)
    }

    /// Та же дата с образцом `format` (`w:dateFormat`).
    pub fn with_date_format(&self, format: &str) -> ContentControl {
        let Some(props) = &self.props else {
            return self.clone();
        };
        let xml = props.as_str();
        let prefix = prefix_of(xml);
        let xml = remove_elements(xml, &["dateFormat"]);
        let element = format!(r#"<{prefix}dateFormat {prefix}val="{}"/>"#, escape(format));
        let xml = insert_into(&xml, "date", &element).unwrap_or(xml);
        self.with_xml(xml)
    }

    fn with_xml(&self, xml: String) -> ContentControl {
        ContentControl {
            props: Some(RawXml::new(xml)),
            end_props: self.end_props.clone(),
        }
    }

    fn tags(&self) -> impl Iterator<Item = Tag<'_>> {
        tags(self.props.as_ref().map_or("", |p| p.as_str()))
    }
}

/// Вид элемента управления содержимым.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlKind {
    /// Форматированный текст (вид по умолчанию).
    RichText,
    /// Обычный текст.
    Text,
    /// Рисунок.
    Picture,
    /// Блок из коллекции (титульная страница, оглавление).
    Gallery,
    /// Флажок.
    CheckBox {
        /// Отмечен ли.
        checked: bool,
        /// Знаки отмеченного и пустого флажка.
        symbols: CheckBoxSymbols,
    },
    /// Раскрывающийся список или поле со списком.
    List {
        /// Пункты.
        items: Vec<ListItem>,
        /// Поле со списком: можно набрать свой текст.
        editable: bool,
    },
    /// Выбор даты.
    Date {
        /// Образец даты (`w:dateFormat`) в обозначениях Word, например
        /// `dd.MM.yyyy`.
        format: Option<String>,
    },
}

/// Знаки флажка.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckBoxSymbols {
    /// Отмеченный.
    pub checked: char,
    /// Пустой.
    pub unchecked: char,
}

impl Default for CheckBoxSymbols {
    /// Как у Word: ☒ и ☐.
    fn default() -> CheckBoxSymbols {
        CheckBoxSymbols {
            checked: '\u{2612}',
            unchecked: '\u{2610}',
        }
    }
}

/// Пункт списка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    /// Что показывается.
    pub text: String,
    /// Значение для программ.
    pub value: String,
}

/// Запрет правки элемента (`w:lock`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlLock {
    /// Без запретов.
    None,
    /// Элемент нельзя удалить (`sdtLocked`).
    Control,
    /// Содержимое нельзя менять (`contentLocked`).
    Content,
    /// И то и другое (`sdtContentLocked`).
    Both,
}

/// Префикс имён свойств (`w:`) по первому тегу; без префикса — пусто.
fn prefix_of(xml: &str) -> String {
    tags(xml)
        .next()
        .and_then(|t| t.name.rsplit_once(':'))
        .map_or(String::new(), |(p, _)| format!("{p}:"))
}

/// Где кончается открывающий тег первого элемента `local` (у пустого
/// тега — его начало, чтобы вставка шла перед ним); без элемента — 0.
fn open_end(xml: &str, local: &str) -> usize {
    tags(xml)
        .find(|t| t.local == local && !t.closing)
        .map_or(0, |t| t.range.end)
}

/// Где кончается первый элемент `local` целиком (после закрывающего
/// тега или пустого тега).
fn element_end(xml: &str, local: &str) -> Option<usize> {
    element_range(xml, local).map(|r| r.end)
}

/// Где первый элемент `local` целиком.
fn element_range(xml: &str, local: &str) -> Option<std::ops::Range<usize>> {
    let mut found = tags(xml).skip_while(|t| !(t.local == local && !t.closing));
    let open = found.next()?;
    if open.self_closing {
        return Some(open.range);
    }
    let mut depth = 0;
    for tag in found {
        if tag.local != local || tag.self_closing {
            continue;
        }
        if tag.closing {
            if depth == 0 {
                return Some(open.range.start..tag.range.end);
            }
            depth -= 1;
        } else {
            depth += 1;
        }
    }
    None
}

/// Убирает все элементы с этими именами.
fn remove_elements(xml: &str, locals: &[&str]) -> String {
    let mut xml = xml.to_string();
    for local in locals {
        while let Some(range) = element_range(&xml, local) {
            xml.replace_range(range, "");
        }
    }
    xml
}

/// Вставляет `inner` в начало элемента `local` (пустой тег становится
/// открывающим и закрывающим); `None`, если элемента нет.
fn insert_into(xml: &str, local: &str, inner: &str) -> Option<String> {
    let tag = tags(xml).find(|t| t.local == local && !t.closing)?;
    let mut out = xml.to_string();
    if tag.self_closing {
        let text = &xml[tag.range.clone()];
        let open = text.trim_end_matches("/>").trim_end();
        let replaced = format!("{open}>{inner}</{}>", tag.name);
        out.replace_range(tag.range, &replaced);
    } else {
        out.insert_str(tag.range.end, inner);
    }
    Some(out)
}

/// Открывающий, закрывающий или пустой тег в тексте XML.
struct Tag<'a> {
    /// Имя с префиксом.
    name: &'a str,
    /// Имя без префикса.
    local: &'a str,
    /// Текст атрибутов.
    attrs: &'a str,
    /// Смещение `attrs` от начала тега.
    attrs_at: usize,
    closing: bool,
    self_closing: bool,
    /// Где тег в тексте, от `<` до `>` включительно.
    range: std::ops::Range<usize>,
}

impl Tag<'_> {
    /// Значение атрибута с именем `local` (с любым префиксом или без),
    /// с раскрытыми ссылками на сущности.
    fn attr(&self, local: &str) -> Option<String> {
        let range = self.attr_value_range(local)?;
        let start = range.start - self.attrs_at;
        let end = range.end - self.attrs_at;
        Some(unescape(&self.attrs[start..end]))
    }

    /// Где значение атрибута `local`, от начала тега.
    fn attr_value_range(&self, local: &str) -> Option<std::ops::Range<usize>> {
        let mut rest = self.attrs;
        let mut offset = 0;
        loop {
            let eq = rest.find('=')?;
            let name = rest[..eq].trim();
            let after = rest[eq + 1..].trim_start();
            let skipped = rest.len() - (eq + 1) - after.len();
            let quote = after.chars().next()?;
            let end = after[1..].find(quote)?;
            let value_start = offset + eq + 1 + skipped + 1;
            if name.rsplit_once(':').map_or(name, |(_, l)| l) == local {
                let start = self.attrs_at + value_start;
                return Some(start..start + end);
            }
            let consumed = eq + 1 + skipped + 1 + end + 1;
            rest = &rest[consumed..];
            offset += consumed;
        }
    }
}

/// Теги текста XML по порядку (комментарии и объявления пропускаются).
fn tags(xml: &str) -> impl Iterator<Item = Tag<'_>> {
    let mut pos = 0;
    std::iter::from_fn(move || {
        loop {
            let start = pos + xml[pos..].find('<')?;
            let end = start + xml[start..].find('>')?;
            pos = end + 1;
            let inner = &xml[start + 1..end];
            if inner.starts_with('?') || inner.starts_with('!') {
                continue;
            }
            let closing = inner.starts_with('/');
            let self_closing = inner.ends_with('/');
            let body = inner.trim_start_matches('/').trim_end_matches('/');
            let lead = inner.len() - inner.trim_start_matches('/').len();
            let name_end = body.find(|c: char| c.is_whitespace()).unwrap_or(body.len());
            let name = &body[..name_end];
            return Some(Tag {
                name,
                local: name.rsplit_once(':').map_or(name, |(_, l)| l),
                attrs: &body[name_end..],
                attrs_at: 1 + lead + name_end,
                closing,
                self_closing,
                range: start..end + 1,
            });
        }
    })
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Ссылка на элемент управления содержимым из абзаца, таблицы или
/// фрагмента текста.
pub type ContentControlRef = Arc<ContentControl>;

/// Пишет содержимое с элементами управления: открывает и закрывает
/// `w:sdt` между соседними частями так, чтобы части с одной ссылкой
/// оказались внутри одного элемента. Вложенные элементы — по порядку от
/// внешнего к внутреннему.
#[derive(Debug, Default)]
pub struct ControlNesting {
    open: Vec<ContentControlRef>,
}

/// Что сделать перед очередной частью содержимого.
#[derive(Debug, PartialEq, Eq)]
pub enum ControlStep<'a> {
    /// Закрыть самый внутренний открытый элемент.
    Close(ContentControlRef),
    /// Открыть элемент.
    Open(&'a ContentControlRef),
}

impl ControlNesting {
    /// Шаги, которые переводят открытые элементы в `controls`
    /// (от внешнего к внутреннему).
    pub fn enter<'a>(&mut self, controls: &'a [ContentControlRef]) -> Vec<ControlStep<'a>> {
        let common = self
            .open
            .iter()
            .zip(controls)
            .take_while(|(a, b)| Arc::ptr_eq(a, b))
            .count();
        let mut steps = Vec::new();
        while self.open.len() > common {
            if let Some(control) = self.open.pop() {
                steps.push(ControlStep::Close(control));
            }
        }
        for control in &controls[common..] {
            self.open.push(control.clone());
            steps.push(ControlStep::Open(control));
        }
        steps
    }

    /// Шаги, которые закрывают все открытые элементы.
    pub fn finish(&mut self) -> Vec<ControlStep<'static>> {
        let mut steps = Vec::new();
        while let Some(control) = self.open.pop() {
            steps.push(ControlStep::Close(control));
        }
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control(tag: &str) -> ContentControlRef {
        Arc::new(ContentControl {
            props: Some(RawXml::new(format!(
                r#"<w:sdtPr xmlns:w="urn:w"><w:alias w:val="Имя"/><w:tag w:val='{tag}'/></w:sdtPr>"#
            ))),
            end_props: None,
        })
    }

    #[test]
    fn reads_tag_and_title() {
        let c = control("name");
        assert_eq!(c.tag().as_deref(), Some("name"));
        assert_eq!(c.title().as_deref(), Some("Имя"));
        assert_eq!(c.property("id"), None);
        assert_eq!(ContentControl::default().tag(), None);
    }

    #[test]
    fn nesting_opens_and_closes_in_order() {
        let a = control("a");
        let b = control("b");
        let same_value = control("a");
        let mut nesting = ControlNesting::default();
        assert!(nesting.enter(&[]).is_empty());
        assert_eq!(
            nesting.enter(&[a.clone(), b.clone()]),
            [ControlStep::Open(&a), ControlStep::Open(&b)]
        );
        assert!(nesting.enter(&[a.clone(), b.clone()]).is_empty());
        assert_eq!(
            nesting.enter(std::slice::from_ref(&a)),
            [ControlStep::Close(b.clone())]
        );
        // Равный по значению, но другой элемент — отдельный `w:sdt`.
        let steps = nesting.enter(std::slice::from_ref(&same_value));
        assert_eq!(steps.len(), 2);
        assert_eq!(nesting.finish(), [ControlStep::Close(same_value)]);
        assert!(nesting.finish().is_empty());
    }

    fn with_props(props: &str) -> ContentControl {
        ContentControl {
            props: Some(RawXml::new(format!(
                r#"<w:sdtPr xmlns:w="urn:w" xmlns:w14="urn:w14">{props}</w:sdtPr>"#
            ))),
            end_props: None,
        }
    }

    #[test]
    fn kinds() {
        assert_eq!(ContentControl::default().kind(), ControlKind::RichText);
        assert_eq!(
            with_props(r#"<w:rPr><w:b/></w:rPr><w:text w:multiLine="1"/>"#).kind(),
            ControlKind::Text
        );
        assert_eq!(
            with_props("<w:docPartObj><w:docPartGallery w:val=\"Cover Pages\"/></w:docPartObj>")
                .kind(),
            ControlKind::Gallery
        );
        let check = with_props(
            r#"<w14:checkbox><w14:checked w14:val="1"/><w14:checkedState w14:val="2611" w14:font="Segoe UI Symbol"/><w14:uncheckedState w14:val="2610" w14:font="MS Gothic"/></w14:checkbox>"#,
        );
        assert_eq!(
            check.kind(),
            ControlKind::CheckBox {
                checked: true,
                symbols: CheckBoxSymbols {
                    checked: '☑',
                    unchecked: '☐'
                }
            }
        );
        let unchecked = check.with_checked(false);
        assert!(matches!(
            unchecked.kind(),
            ControlKind::CheckBox { checked: false, .. }
        ));
        assert!(matches!(
            unchecked.with_checked(true).kind(),
            ControlKind::CheckBox { checked: true, .. }
        ));
        let list = with_props(
            r#"<w:dropDownList w:lastValue="b"><w:listItem w:displayText="Выберите" w:value=""/><w:listItem w:displayText="A &amp; B" w:value="b"/><w:listItem w:value="c"/></w:dropDownList>"#,
        );
        let ControlKind::List { items, editable } = list.kind() else {
            panic!("expected list")
        };
        assert!(!editable);
        assert_eq!(items.len(), 3);
        assert_eq!(items[1].text, "A & B");
        assert_eq!(items[2].text, "c");
        assert!(matches!(
            with_props("<w:comboBox/>").kind(),
            ControlKind::List { editable: true, .. }
        ));
        let date = with_props(
            r#"<w:date><w:dateFormat w:val="dd.MM.yyyy"/><w:lid w:val="ru-RU"/></w:date>"#,
        );
        assert_eq!(
            date.kind(),
            ControlKind::Date {
                format: Some("dd.MM.yyyy".into())
            }
        );
        let dated = date.with_full_date("2026-10-23T00:00:00Z");
        assert!(
            dated
                .props
                .as_ref()
                .unwrap()
                .as_str()
                .contains(r#"<w:date w:fullDate="2026-10-23T00:00:00Z">"#)
        );
        let redated = dated.with_full_date("2027-01-01T00:00:00Z");
        assert!(
            redated
                .props
                .as_ref()
                .unwrap()
                .as_str()
                .contains("2027-01-01")
        );
        assert!(!redated.props.as_ref().unwrap().as_str().contains("2026"));
        assert_eq!(date.full_date(), None);
        assert_eq!(dated.full_date(), Some((2026, 10, 23)));
        assert_eq!(redated.full_date(), Some((2027, 1, 1)));
        let reformatted = redated.with_date_format("d MMMM yyyy");
        assert_eq!(
            reformatted.kind(),
            ControlKind::Date {
                format: Some("d MMMM yyyy".into())
            }
        );
        assert_eq!(reformatted.full_date(), Some((2027, 1, 1)));
    }

    #[test]
    fn settings_and_items() {
        let control = with_props(
            r#"<w:rPr><w:b/></w:rPr><w:alias w:val="Старое"/><w:id w:val="7"/><w:lock w:val="contentLocked"/><w:dropDownList/>"#,
        );
        let named = control.with_settings("Отдел", "dept", ControlLock::Control);
        assert_eq!(named.title().as_deref(), Some("Отдел"));
        assert_eq!(named.tag().as_deref(), Some("dept"));
        assert_eq!(named.lock(), ControlLock::Control);
        let xml = named.props.as_ref().unwrap().as_str().to_string();
        // Порядок по схеме: rPr, alias, tag, id, lock.
        let place = |s: &str| xml.find(s).unwrap();
        assert!(place("rPr") < place("alias") && place("alias") < place("w:tag"));
        assert!(place("w:id") < place("w:lock"));
        let shown = named.with_placeholder();
        assert!(shown.showing_placeholder());
        assert_eq!(shown.with_placeholder(), shown);
        assert_eq!(shown.without_placeholder(), named);
        let cleared = named.with_settings("", "", ControlLock::None);
        assert_eq!(cleared.title(), None);
        assert_eq!(cleared.tag(), None);
        assert_eq!(cleared.lock(), ControlLock::None);
        let items = vec![
            ListItem {
                text: "Склад & <офис>".into(),
                value: "1".into(),
            },
            ListItem {
                text: "Бухгалтерия".into(),
                value: "2".into(),
            },
        ];
        let listed = cleared.with_list_items(&items);
        assert_eq!(
            listed.kind(),
            ControlKind::List {
                items: items.clone(),
                editable: false
            }
        );
        let fewer = listed.with_list_items(&items[1..]);
        let ControlKind::List { items: now, .. } = fewer.kind() else {
            panic!("ждали список");
        };
        assert_eq!(now, &items[1..]);
    }

    #[test]
    fn placeholder_and_lock() {
        let c = with_props(
            r#"<w:lock w:val="sdtContentLocked"/><w:placeholder><w:docPart w:val="X"/></w:placeholder><w:showingPlcHdr/>"#,
        );
        assert!(c.showing_placeholder());
        assert_eq!(c.lock(), ControlLock::Both);
        assert!(!c.content_editable());
        let shown = c.without_placeholder();
        assert!(!shown.showing_placeholder());
        assert!(
            shown
                .props
                .as_ref()
                .unwrap()
                .as_str()
                .contains("<w:placeholder>")
        );
        assert_eq!(
            with_props(r#"<w:lock w:val="sdtLocked"/>"#).lock(),
            ControlLock::Control
        );
        assert!(with_props(r#"<w:lock w:val="sdtLocked"/>"#).content_editable());
        assert_eq!(ContentControl::default().lock(), ControlLock::None);
    }
}
