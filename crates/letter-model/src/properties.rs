//! Свойства документа («Файл → Сведения»): название, автор, даты,
//! статистика.

use crate::{Block, Document, RawXml};

/// Свойства документа.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Properties {
    /// Основные свойства (`docProps/core.xml`).
    pub core: CoreProperties,
    /// Сведения о программе и статистика (`docProps/app.xml`).
    pub app: AppProperties,
}

/// Основные свойства: название, автор, даты. Пустая строка и `None`
/// различаются: в файле может быть пустой элемент.
///
/// Даты хранятся строкой в формате файла (W3CDTF, например
/// `2024-05-01T10:00:00Z`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoreProperties {
    /// Название.
    pub title: Option<String>,
    /// Тема.
    pub subject: Option<String>,
    /// Автор.
    pub creator: Option<String>,
    /// Ключевые слова.
    pub keywords: Option<String>,
    /// Примечания (описание).
    pub description: Option<String>,
    /// Кто изменял последним.
    pub last_modified_by: Option<String>,
    /// Номер редакции (сколько раз сохраняли).
    pub revision: Option<String>,
    /// Когда создан.
    pub created: Option<String>,
    /// Когда изменён.
    pub modified: Option<String>,
    /// Когда последний раз напечатан.
    pub last_printed: Option<String>,
    /// Категория.
    pub category: Option<String>,
    /// Состояние («Черновик», «Окончательный» и т. п.).
    pub content_status: Option<String>,
    /// Язык.
    pub language: Option<String>,
    /// Непонятые элементы, сохранённые как есть.
    pub unknown: Vec<RawXml>,
}

/// Сведения о программе и статистика документа. Числа — то, что записала
/// программа при последнем сохранении; посчитать заново можно через
/// [`Document::statistics`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppProperties {
    /// Программа, сохранившая документ.
    pub application: Option<String>,
    /// Версия программы.
    pub app_version: Option<String>,
    /// Шаблон, на котором основан документ.
    pub template: Option<String>,
    /// Организация.
    pub company: Option<String>,
    /// Руководитель.
    pub manager: Option<String>,
    /// Общее время правки в минутах.
    pub total_time: Option<u32>,
    /// Число страниц.
    pub pages: Option<u32>,
    /// Число слов.
    pub words: Option<u32>,
    /// Число знаков без пробелов.
    pub characters: Option<u32>,
    /// Число знаков с пробелами.
    pub characters_with_spaces: Option<u32>,
    /// Число строк.
    pub lines: Option<u32>,
    /// Число абзацев.
    pub paragraphs: Option<u32>,
    /// Непонятые элементы, сохранённые как есть.
    pub unknown: Vec<RawXml>,
}

/// Статистика текста, как в окне «Статистика» Word. Страницы и строки
/// зависят от вёрстки и здесь не считаются.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Statistics {
    /// Слова: куски текста между пробелами.
    pub words: u32,
    /// Знаки без пробелов.
    pub characters: u32,
    /// Знаки с пробелами (и табуляциями).
    pub characters_with_spaces: u32,
    /// Непустые абзацы.
    pub paragraphs: u32,
}

impl Document {
    /// Считает статистику по всему тексту документа, включая таблицы.
    pub fn statistics(&self) -> Statistics {
        let mut stats = Statistics::default();
        for block in self.blocks() {
            count_block(block, &mut stats);
        }
        stats
    }
}

fn count_block(block: &Block, stats: &mut Statistics) {
    match block {
        Block::Paragraph(p) => {
            let text = p.plain_text();
            let words = text.split_whitespace().count();
            let characters = text.chars().filter(|c| !c.is_whitespace()).count();
            // Разрывы строк и страниц — не знаки, пробелы и табуляции — знаки.
            let with_spaces = text
                .chars()
                .filter(|c| !matches!(c, '\n' | '\r' | '\u{b}' | '\u{c}'))
                .count();
            stats.words += saturate(words);
            stats.characters += saturate(characters);
            stats.characters_with_spaces += saturate(with_spaces);
            if characters > 0 {
                stats.paragraphs += 1;
            }
        }
        Block::Table(table) => {
            for cell in table.rows.iter().flat_map(|r| &r.cells) {
                for block in &cell.blocks {
                    count_block(block, stats);
                }
            }
        }
        Block::Unknown(_) => {}
    }
}

fn saturate(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Paragraph, Run, RunContent, Table, TableCell, TableRow};

    #[test]
    fn counts_words_and_characters() {
        let mut doc = Document::new();
        let mut with_tab = Run::text("один");
        with_tab.content.push(RunContent::Tab);
        with_tab.content.push(RunContent::Text("два  три".into()));
        doc.sections[0].blocks = vec![
            Paragraph::from_runs([with_tab]).into(),
            Paragraph::default().into(),
            Table {
                raw_props: Vec::new(),
                rows: vec![TableRow {
                    raw_props: Vec::new(),
                    cells: vec![TableCell::text("a b"), TableCell::text("")],
                }],
            }
            .into(),
        ];
        let stats = doc.statistics();
        assert_eq!(stats.words, 5);
        assert_eq!(stats.characters, 4 + 3 + 3 + 2);
        // «один\tдва  три» — 13 знаков, «a b» — 3.
        assert_eq!(stats.characters_with_spaces, 13 + 3);
        assert_eq!(stats.paragraphs, 2);
        assert_eq!(Document::new().statistics(), Statistics::default());
    }
}
