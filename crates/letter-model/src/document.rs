//! Документ целиком.

use crate::{Block, Paragraph, Properties, RawXml, Section, SourcePackage, Styles, Theme, Twips};

/// Документ: разделы с содержимым, стили, нумерация списков и настройки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// Разделы по порядку. Word всегда записывает хотя бы один раздел,
    /// но в битом файле список может оказаться пустым: код, который читает
    /// модель, обязан это выдерживать.
    pub sections: Vec<Section>,
    /// Стили документа.
    pub styles: Styles,
    /// Определения списков.
    pub numbering: Numbering,
    /// Тема документа (шрифты).
    pub theme: Theme,
    /// Настройки документа.
    pub settings: Settings,
    /// Свойства документа: название, автор, даты, статистика.
    pub properties: Properties,
    /// Непонятые элементы уровня документа (например, фон страницы `w:background`).
    pub unknown: Vec<RawXml>,
    /// Части исходного файла, которые программа не разбирает и при
    /// сохранении возвращает без изменений. У нового документа — `None`.
    pub source: Option<SourcePackage>,
}

impl Document {
    /// Новый пустой документ, как после «Файл → Создать»:
    /// один раздел с одним пустым абзацем.
    pub fn new() -> Document {
        Document {
            sections: vec![Section {
                props: Default::default(),
                blocks: vec![Paragraph::default().into()],
            }],
            styles: Styles::default(),
            numbering: Numbering::default(),
            theme: Theme::default(),
            settings: Settings::default(),
            properties: Properties::default(),
            unknown: Vec::new(),
            source: None,
        }
    }

    /// Все блоки документа подряд, по всем разделам.
    pub fn blocks(&self) -> impl Iterator<Item = &Block> {
        self.sections.iter().flat_map(|s| s.blocks.iter())
    }

    /// Текст документа без оформления: абзацы через `\n`.
    pub fn plain_text(&self) -> String {
        self.blocks()
            .map(Block::plain_text)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Default for Document {
    fn default() -> Document {
        Document::new()
    }
}

/// Определения нумерованных и маркированных списков (`word/numbering.xml`).
/// Пока пусто: списки появятся в задаче F5.1.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Numbering {}

/// Настройки документа (`word/settings.xml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Шаг табуляции по умолчанию.
    pub default_tab_stop: Twips,
    /// Интервал после абзаца и интервал перед следующим складываются
    /// (настройка совместимости `w:doNotUseHTMLParagraphAutoSpacing`).
    /// Если нет — между абзацами берётся больший из двух, как в HTML;
    /// так Word делает по умолчанию.
    pub add_paragraph_spacing: bool,
}

impl Default for Settings {
    /// Как в Word: табуляция каждые 1,25 см (708 твипов).
    fn default() -> Settings {
        Settings {
            default_tab_stop: Twips(708),
            add_paragraph_spacing: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Run, RunProps, Table, TableCell, TableRow};

    #[test]
    fn new_document_has_one_empty_paragraph() {
        let doc = Document::new();
        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.blocks().count(), 1);
        assert_eq!(doc.plain_text(), "");
        assert_eq!(doc, Document::default());
    }

    #[test]
    fn build_document_by_hand() {
        let italic = RunProps {
            italic: Some(true),
            ..RunProps::default()
        };
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![
            Paragraph::from_runs([Run::text("Hello, "), Run::text("world").with_props(italic)])
                .into(),
            Table {
                raw_props: Vec::new(),
                rows: vec![TableRow {
                    raw_props: Vec::new(),
                    cells: vec![TableCell::text("1"), TableCell::text("2")],
                }],
            }
            .into(),
            Paragraph::text("Конец").into(),
        ];
        assert_eq!(doc.plain_text(), "Hello, world\n1\t2\nКонец");

        let copy = doc.clone();
        assert_eq!(doc, copy);
        doc.sections[0].blocks.pop();
        assert_ne!(doc, copy);
    }
}
