//! Примечания (комментарии рецензентов). Кусок текста, к которому
//! относится примечание, отмечен в абзацах началом
//! ([`crate::Inline::CommentStart`]) и концом ([`crate::Inline::CommentEnd`]),
//! а сразу за концом стоит ссылка ([`crate::RunContent::CommentReference`]).
//! Сам текст примечания лежит в [`crate::Document::comments`].

use crate::Block;

/// Примечание: кто, когда и что написал.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Comment {
    /// Номер (`w:id`), по нему примечание связано с текстом.
    pub id: String,
    /// Автор (`w:author`).
    pub author: String,
    /// Инициалы автора (`w:initials`).
    pub initials: Option<String>,
    /// Когда написано (`w:date`), как в файле: `2026-10-23T12:00:00Z`.
    pub date: Option<String>,
    /// Текст примечания.
    pub blocks: Vec<Block>,
    /// Номер примечания, на которое это — ответ (`w15:paraIdParent`
    /// в `commentsExtended.xml`); у обычного примечания — `None`.
    pub parent: Option<String>,
    /// Отмечено «Решено» (`w15:done`).
    pub done: bool,
    /// Метка последнего абзаца текста (`w14:paraId`), по ней Word
    /// связывает примечание с ответами и отметкой «решено».
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
