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
    /// например `"tag"` или `"alias"`. Ссылки на сущности (`&amp;`) не
    /// раскрываются.
    pub fn property(&self, local: &str) -> Option<&str> {
        let mut rest = self.props.as_ref()?.as_str();
        while let Some(start) = rest.find('<') {
            rest = &rest[start + 1..];
            let tag = &rest[..rest.find('>')?];
            let name_end = tag
                .find(|c: char| c.is_whitespace() || c == '/')
                .unwrap_or(tag.len());
            let name = &tag[..name_end];
            if name.rsplit_once(':').map_or(name, |(_, l)| l) == local {
                return val_attribute(&tag[name_end..]);
            }
        }
        None
    }

    /// Пометка элемента (`w:tag`), по которой его находят программы.
    pub fn tag(&self) -> Option<&str> {
        self.property("tag")
    }

    /// Название элемента (`w:alias`), которое Word показывает на рамке.
    pub fn title(&self) -> Option<&str> {
        self.property("alias")
    }
}

/// Значение атрибута `val` (с любым префиксом) в тексте атрибутов тега.
fn val_attribute(attrs: &str) -> Option<&str> {
    let mut rest = attrs;
    while let Some(pos) = rest.find("val=") {
        let before = rest[..pos].chars().next_back();
        let after = &rest[pos + 4..];
        let quote = after.chars().next()?;
        let value = &after[1..];
        let end = value.find(quote)?;
        if matches!(
            before,
            Some(':') | Some(' ') | Some('\t') | Some('\n') | Some('\r')
        ) {
            return Some(&value[..end]);
        }
        rest = &value[end..];
    }
    None
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
        assert_eq!(c.tag(), Some("name"));
        assert_eq!(c.title(), Some("Имя"));
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
}
