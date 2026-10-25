//! Примечания (комментарии рецензентов, «Рецензирование → Создать
//! примечание»). Примечание относится к куску текста: начало и конец
//! куска — [`crate::Inline::CommentStart`] и [`crate::Inline::CommentEnd`]
//! в содержимом абзацев, а место значка примечания —
//! [`crate::RunContent::CommentReference`]. Все три связаны номером
//! примечания. Сам текст примечания лежит в [`crate::Document::comments`].
//!
//! Ответ на примечание — тоже примечание, со ссылкой на исходное
//! ([`Comment::parent`]); у ответа свой кусок текста (обычно тот же).

use crate::{Block, Inline, Paragraph, RunContent};

/// Примечание.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Comment {
    /// Номер (`w:id`), по нему примечание связано с текстом документа.
    pub id: String,
    /// Автор.
    pub author: String,
    /// Инициалы автора (`w:initials`).
    pub initials: Option<String>,
    /// Когда написано, как в файле: `2026-10-25T12:00:00Z`.
    pub date: Option<String>,
    /// Текст примечания.
    pub blocks: Vec<Block>,
    /// Номер примечания, ответом на которое служит это (`None` — не
    /// ответ).
    pub parent: Option<String>,
    /// Обсуждение закрыто («Решить», `w15:done`).
    pub done: bool,
    /// Номер последнего абзаца примечания в файле (`w14:paraId`, восемь
    /// шестнадцатеричных цифр): по нему связаны ответы и отметки
    /// «решено». У нового примечания — `None`, номер даётся при записи.
    pub para_id: Option<String>,
}

impl Comment {
    /// Текст примечания без оформления: абзацы через `\n`.
    pub fn plain_text(&self) -> String {
        self.blocks
            .iter()
            .map(Block::plain_text)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Номера примечаний, на которые есть значок в абзацах `blocks` (и
/// в их таблицах), по порядку.
pub fn comment_references(blocks: &[Block], out: &mut Vec<String>) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => paragraph_references(p, out),
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        comment_references(&cell.blocks, out);
                    }
                }
            }
            Block::Unknown(_) => {}
        }
    }
}

fn paragraph_references(p: &Paragraph, out: &mut Vec<String>) {
    for item in &p.content {
        if let Inline::Run(run) = item {
            for content in &run.content {
                if let RunContent::CommentReference(id) = content {
                    out.push(id.clone());
                }
            }
        }
    }
}
