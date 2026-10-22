//! Документ целиком.

use crate::{
    Block, Color, Inline, Note, Numbering, Paragraph, Picture, Properties, RawXml, RunContent,
    Section, SourcePackage, Styles, Theme, Twips,
};

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
    /// Сноски (с служебными: чертой над сносками и т. п.) в порядке из
    /// файла.
    pub footnotes: Vec<Note>,
    /// Концевые сноски.
    pub endnotes: Vec<Note>,
    /// Цвет страницы (`w:background`); `None` — белая.
    pub background: Option<Color>,
    /// Непонятые элементы уровня документа (например, фон-рисунок).
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
            footnotes: Vec::new(),
            endnotes: Vec::new(),
            background: None,
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

    /// Вызывает `f` для каждого рисунка документа, включая рисунки
    /// в таблицах (и во вложенных таблицах).
    pub fn for_each_picture_mut(&mut self, mut f: impl FnMut(&mut Picture)) {
        for section in &mut self.sections {
            pictures_in(&mut section.blocks, &mut f);
        }
    }
}

/// Вызывает `f` для каждого рисунка в блоках `blocks`, включая рисунки
/// в таблицах (например, в колонтитуле).
pub fn for_each_picture_in(blocks: &mut [Block], mut f: impl FnMut(&mut Picture)) {
    pictures_in(blocks, &mut f);
}

fn pictures_in(blocks: &mut [Block], f: &mut impl FnMut(&mut Picture)) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                for inline in &mut paragraph.content {
                    let Inline::Run(run) = inline else { continue };
                    for item in &mut run.content {
                        if let RunContent::Picture(picture) = item {
                            f(picture);
                        }
                    }
                }
            }
            Block::Table(table) => {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        pictures_in(&mut cell.blocks, f);
                    }
                }
            }
            Block::Unknown(_) => {}
        }
    }
}

impl Default for Document {
    fn default() -> Document {
        Document::new()
    }
}

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
    /// Режим совместимости с версией Word (`compatibilityMode`): 15 —
    /// Word 2013 и новее, 14 — Word 2010, 12 — Word 2007 (так считается,
    /// если режим не указан). От него зависит, например, где стоит левая
    /// граница таблицы.
    pub compatibility_mode: u32,
    /// «Разные колонтитулы для чётных и нечётных страниц»
    /// (`w:evenAndOddHeaders`).
    pub even_and_odd_headers: bool,
}

impl Default for Settings {
    /// Как в Word: табуляция каждые 1,25 см (708 твипов).
    fn default() -> Settings {
        Settings {
            default_tab_stop: Twips(708),
            add_paragraph_spacing: false,
            compatibility_mode: 12,
            even_and_odd_headers: false,
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
                grid: Vec::new(),
                raw_props: Vec::new(),
                props: Default::default(),
                rows: vec![TableRow {
                    raw_props: Vec::new(),
                    props: Default::default(),
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
