//! Номера сносок. Сноски нумеруются по порядку ссылок в тексте
//! документа: обычные — 1, 2, 3…, концевые — i, ii, iii… (как в Word по
//! умолчанию). Ссылка со своим знаком номера не получает.

use std::collections::HashMap;

use letter_model::{Block, Document, NoteKind, NoteReference, Paragraph, RunContent, roman};

/// Видимые номера сносок документа.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct NoteNumbers {
    numbers: HashMap<(NoteKind, String), String>,
    /// Сноски, на которые есть ссылки, в порядке первых ссылок.
    referenced: Vec<(NoteKind, String)>,
}

impl NoteNumbers {
    /// Номера по ссылкам в тексте документа.
    pub fn of(doc: &Document) -> NoteNumbers {
        let mut numbers = NoteNumbers::default();
        let mut counters = [0usize; 2];
        for section in &doc.sections {
            numbers.scan(&section.blocks, &mut counters);
        }
        numbers
    }

    fn scan(&mut self, blocks: &[Block], counters: &mut [usize; 2]) {
        for block in blocks {
            match block {
                Block::Paragraph(p) => {
                    for item in p.runs().flat_map(|r| &r.content) {
                        let RunContent::NoteReference(r) = item else {
                            continue;
                        };
                        let key = (r.kind, r.id.clone());
                        if !self.referenced.contains(&key) {
                            self.referenced.push(key.clone());
                        }
                        if r.custom_mark || self.numbers.contains_key(&key) {
                            continue;
                        }
                        let counter = &mut counters[r.kind as usize];
                        *counter += 1;
                        self.numbers.insert(key, format_number(r.kind, *counter));
                    }
                }
                Block::Table(t) => {
                    for cell in t.rows.iter().flat_map(|r| &r.cells) {
                        self.scan(&cell.blocks, counters);
                    }
                }
                Block::Unknown(_) => {}
            }
        }
    }

    /// Номер сноски `id` вида `kind`.
    pub fn number(&self, kind: NoteKind, id: &str) -> Option<&str> {
        self.numbers
            .get(&(kind, id.to_string()))
            .map(String::as_str)
    }

    /// Сноски вида `kind`, на которые есть ссылки, в порядке ссылок.
    pub fn referenced(&self, kind: NoteKind) -> impl Iterator<Item = &str> {
        self.referenced
            .iter()
            .filter(move |(k, _)| *k == kind)
            .map(|(_, id)| id.as_str())
    }

    /// Надписи ссылок на сноски и номеров сносок в абзаце по порядку.
    /// `own` — номер сноски, текст которой этот абзац (для её номера
    /// в начале текста).
    pub fn marks(&self, paragraph: &Paragraph, own: Option<&str>) -> Vec<String> {
        paragraph
            .runs()
            .flat_map(|r| &r.content)
            .filter_map(|item| match item {
                RunContent::NoteReference(NoteReference {
                    kind,
                    id,
                    custom_mark: false,
                }) => Some(self.number(*kind, id).unwrap_or("?").to_string()),
                RunContent::NoteMark(_) => Some(own.unwrap_or("?").to_string()),
                _ => None,
            })
            .collect()
    }
}

/// Номер в виде, принятом для вида сносок.
fn format_number(kind: NoteKind, n: usize) -> String {
    match kind {
        NoteKind::Footnote => n.to_string(),
        NoteKind::Endnote => roman(n).to_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::Run;

    fn reference(kind: NoteKind, id: &str, custom_mark: bool) -> Run {
        Run {
            content: vec![RunContent::NoteReference(NoteReference {
                kind,
                id: id.into(),
                custom_mark,
            })],
            ..Run::default()
        }
    }

    #[test]
    fn notes_are_numbered_in_order() {
        let mut doc = Document::new();
        let p = Paragraph::from_runs([
            reference(NoteKind::Footnote, "5", false),
            reference(NoteKind::Endnote, "1", false),
            reference(NoteKind::Footnote, "2", true),
            reference(NoteKind::Footnote, "3", false),
            reference(NoteKind::Endnote, "2", false),
        ]);
        doc.sections[0].blocks = vec![p.clone().into()];
        let numbers = NoteNumbers::of(&doc);
        assert_eq!(numbers.number(NoteKind::Footnote, "5"), Some("1"));
        assert_eq!(numbers.number(NoteKind::Footnote, "3"), Some("2"));
        assert_eq!(numbers.number(NoteKind::Footnote, "2"), None);
        assert_eq!(numbers.number(NoteKind::Endnote, "2"), Some("ii"));
        assert_eq!(numbers.marks(&p, None), ["1", "i", "2", "ii"]);
        let footnotes: Vec<&str> = numbers.referenced(NoteKind::Footnote).collect();
        assert_eq!(footnotes, ["5", "2", "3"]);
    }
}
