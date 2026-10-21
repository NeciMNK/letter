//! Буфер обмена: копирование и вставка между `letter` и другими
//! программами.
//!
//! При копировании в системный буфер кладутся простой текст и HTML
//! (Word, браузеры и почта вставят его с оформлением), а сам кусок
//! документа запоминается внутри программы: если вставляют то же самое,
//! берётся он — без потерь. При вставке из другой программы сначала
//! берётся HTML, затем RTF (если текст — это RTF), затем простой текст.

use letter_editor::Command;
use letter_model::Document;

/// Системный буфер обмена. Отдельный тип, чтобы тесты не трогали
/// настоящий буфер.
pub trait SystemClipboard {
    /// Кладёт в буфер текст и HTML.
    fn put(&mut self, text: &str, html: &str);
    /// HTML из буфера, если он там есть.
    fn html(&mut self) -> Option<String>;
    /// Простой текст из буфера.
    fn text(&mut self) -> Option<String>;
}

/// Настоящий системный буфер (библиотека `arboard`). Подключается при
/// первом обращении.
#[derive(Default)]
pub struct SystemBuffer(Option<arboard::Clipboard>);

impl SystemBuffer {
    fn get(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.0.is_none() {
            self.0 = arboard::Clipboard::new().ok();
        }
        self.0.as_mut()
    }
}

impl SystemClipboard for SystemBuffer {
    fn put(&mut self, text: &str, html: &str) {
        if let Some(clipboard) = self.get() {
            // Если не вышло, буфер просто останется прежним.
            let _ = clipboard.set_html(html, Some(text));
        }
    }

    fn html(&mut self) -> Option<String> {
        self.get()?.get().html().ok()
    }

    fn text(&mut self) -> Option<String> {
        self.get()?.get_text().ok()
    }
}

/// Буфер обмена программы.
pub struct Clipboard {
    system: Box<dyn SystemClipboard>,
    /// Последний скопированный кусок и его текст.
    own: Option<(String, Document)>,
}

impl Clipboard {
    /// Буфер поверх системного `system`.
    pub fn new(system: Box<dyn SystemClipboard>) -> Clipboard {
        Clipboard { system, own: None }
    }

    /// Копирует кусок документа (см. [`letter_editor::copy_range`]).
    pub fn copy(&mut self, fragment: Document) {
        let text = fragment.plain_text();
        let html = letter_text::write_html(&fragment);
        self.system.put(&text, &html);
        self.own = Some((text, fragment));
    }

    /// Текст из системного буфера (для кнопки «Вставить» на ленте:
    /// при Ctrl+V текст передаёт сама система).
    pub fn text(&mut self) -> Option<String> {
        self.system.text().filter(|t| !t.is_empty())
    }

    /// Что вставить, если система передала текст `text` (Ctrl+V).
    pub fn paste(&mut self, text: &str) -> Command {
        if let Some((own, fragment)) = &self.own
            && same_text(own, text)
        {
            return Command::Paste(Box::new(fragment.clone()));
        }
        if let Some(html) = self.system.html().filter(|h| !h.trim().is_empty()) {
            let fragment = letter_text::read_html(strip_header(&html).as_bytes());
            return Command::Paste(Box::new(fragment));
        }
        if text.trim_start().starts_with("{\\rtf")
            && let Ok(fragment) = letter_rtf::read(text.as_bytes())
        {
            return Command::Paste(Box::new(fragment));
        }
        Command::PasteText(text.to_owned())
    }
}

/// Тексты совпадают с точностью до переводов строк (Windows меняет
/// `\n` на `\r\n`).
fn same_text(a: &str, b: &str) -> bool {
    let normal = |s: &str| s.replace("\r\n", "\n");
    normal(a) == normal(b)
}

/// HTML из буфера Windows может начинаться заголовком «Version:0.9
/// StartHTML:…» — он отрезается.
fn strip_header(html: &str) -> &str {
    if html.starts_with("Version:")
        && let Some(start) = html.find('<')
    {
        &html[start..]
    } else {
        html
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    use letter_editor::{Position, copy_range};
    use letter_model::{Block, Paragraph, Run, RunProps};

    /// Буфер в памяти вместо системного.
    #[derive(Default, Clone)]
    pub struct FakeBuffer {
        /// Что лежит: текст и HTML.
        pub content: Rc<RefCell<(String, Option<String>)>>,
    }

    impl SystemClipboard for FakeBuffer {
        fn put(&mut self, text: &str, html: &str) {
            *self.content.borrow_mut() = (text.to_owned(), Some(html.to_owned()));
        }

        fn html(&mut self) -> Option<String> {
            self.content.borrow().1.clone()
        }

        fn text(&mut self) -> Option<String> {
            Some(self.content.borrow().0.clone())
        }
    }

    fn bold_doc() -> Document {
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![
            Paragraph::from_runs([Run::text("bold").with_props(RunProps {
                bold: Some(true),
                ..RunProps::default()
            })])
            .into(),
            Paragraph::text("second").into(),
        ];
        doc
    }

    #[test]
    fn own_copy_is_pasted_without_losses() {
        let fake = FakeBuffer::default();
        let mut clipboard = Clipboard::new(Box::new(fake.clone()));
        let doc = bold_doc();
        let part = copy_range(&doc, Position::new(0, 0), Position::new(1, 3));
        clipboard.copy(part.clone());
        let (text, html) = fake.content.borrow().clone();
        assert_eq!(text, "bold\nsec");
        assert!(html.is_some_and(|h| h.contains("bold")));
        // Windows вернёт текст с \r\n — всё равно наш кусок.
        assert_eq!(
            clipboard.paste("bold\r\nsec"),
            Command::Paste(Box::new(part))
        );
    }

    #[test]
    fn other_programs_give_html_rtf_or_text() {
        let fake = FakeBuffer::default();
        let mut clipboard = Clipboard::new(Box::new(fake.clone()));
        *fake.content.borrow_mut() = (
            "Hi".into(),
            Some("Version:0.9\r\nStartHTML:1\r\n<html><body><b>Hi</b></body></html>".into()),
        );
        let Command::Paste(fragment) = clipboard.paste("Hi") else {
            panic!("ждали HTML");
        };
        assert_eq!(fragment.plain_text(), "Hi");
        let Some(Block::Paragraph(p)) = fragment.sections[0].blocks.first() else {
            panic!()
        };
        assert!(p.runs().any(|r| r.props.bold == Some(true)));

        *fake.content.borrow_mut() = (String::new(), None);
        let rtf = r"{\rtf1\ansi {\b RTF}\par}";
        let Command::Paste(fragment) = clipboard.paste(rtf) else {
            panic!("ждали RTF");
        };
        assert_eq!(fragment.plain_text().trim(), "RTF");
        assert_eq!(clipboard.paste("plain"), Command::PasteText("plain".into()));
    }
}
