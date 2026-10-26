//! Шаблоны для «Файл → Создать»: свои встроенные (письмо, резюме, отчёт,
//! служебная записка, заявление — тексты на языке интерфейса, без
//! шаблонов Microsoft) и личные — файлы .dotx, .dotm, .dot в папке
//! `templates` рядом с параметрами программы.

use std::path::{Path, PathBuf};

use letter_editor::{ListKind, Position, toggle_list};
use letter_model::{Alignment, Block, Document, Inline, Paragraph, Run, RunProps};

use crate::i18n::tr;

/// Встроенный шаблон.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Builtin {
    /// Деловое письмо.
    Letter,
    /// Резюме.
    Resume,
    /// Отчёт.
    Report,
    /// Служебная записка.
    Memo,
    /// Заявление.
    Statement,
}

impl Builtin {
    /// Все встроенные шаблоны по порядку в галерее.
    pub const ALL: [Builtin; 5] = [
        Builtin::Letter,
        Builtin::Resume,
        Builtin::Report,
        Builtin::Memo,
        Builtin::Statement,
    ];

    /// Имя шаблона на языке интерфейса.
    pub fn name(self) -> &'static str {
        tr(&format!("template-{}", self.key()))
    }

    /// Часть имён строк в файлах перевода.
    fn key(self) -> &'static str {
        match self {
            Builtin::Letter => "letter",
            Builtin::Resume => "resume",
            Builtin::Report => "report",
            Builtin::Memo => "memo",
            Builtin::Statement => "statement",
        }
    }

    /// Строки шаблона: вид и номер строки текста (`tpl-<шаблон>-<номер>`).
    fn lines(self) -> &'static [(Line, u8)] {
        use Line::*;
        match self {
            Builtin::Letter => &[
                (Right, 1),
                (Right, 2),
                (Right, 3),
                (Empty, 0),
                (Text, 4),
                (Empty, 0),
                (Text, 5),
                (Text, 6),
                (Text, 7),
                (Empty, 0),
                (Bold, 8),
                (Text, 9),
                (Text, 10),
                (Text, 11),
                (Text, 12),
                (Empty, 0),
                (Text, 13),
            ],
            Builtin::Resume => &[
                (Title, 1),
                (Text, 2),
                (Heading, 3),
                (Text, 4),
                (Heading, 5),
                (Bold, 6),
                (Bullet, 7),
                (Bullet, 8),
                (Heading, 9),
                (Bold, 10),
                (Text, 11),
                (Heading, 12),
                (Bullet, 13),
                (Bullet, 14),
                (Bullet, 15),
            ],
            Builtin::Report => &[
                (Title, 1),
                (Text, 2),
                (Heading, 3),
                (Text, 4),
                (Heading, 5),
                (Text, 6),
                (Text, 7),
                (Heading, 8),
                (Bullet, 9),
                (Bullet, 10),
            ],
            Builtin::Memo => &[
                (Title, 1),
                (Empty, 0),
                (Text, 2),
                (Text, 3),
                (Text, 4),
                (Text, 5),
                (Empty, 0),
                (Text, 6),
                (Text, 7),
            ],
            Builtin::Statement => &[
                (Right, 1),
                (Right, 2),
                (Right, 3),
                (Empty, 0),
                (Empty, 0),
                (CenterBold, 4),
                (Empty, 0),
                (Text, 5),
                (Empty, 0),
                (Empty, 0),
                (Text, 6),
            ],
        }
    }

    /// Новый документ по шаблону.
    pub fn document(self) -> Document {
        let mut doc = crate::files::blank();
        let mut blocks = Vec::new();
        let mut bullets = Vec::new();
        for (index, (line, number)) in self.lines().iter().enumerate() {
            let text = if *number == 0 {
                ""
            } else {
                tr(&format!("tpl-{}-{number}", self.key()))
            };
            let mut paragraph = Paragraph::default();
            let mut run = RunProps::default();
            match line {
                Line::Title => paragraph.props.style = Some("Title".into()),
                Line::Heading => paragraph.props.style = Some("Heading1".into()),
                Line::Right => paragraph.props.alignment = Some(Alignment::Right),
                Line::Bold => run.bold = Some(true),
                Line::CenterBold => {
                    paragraph.props.alignment = Some(Alignment::Center);
                    run.bold = Some(true);
                }
                Line::Bullet => bullets.push(index),
                Line::Text | Line::Empty => {}
            }
            if !text.is_empty() {
                paragraph.content.push(Inline::Run(Run {
                    props: run,
                    ..Run::text(text)
                }));
            }
            blocks.push(Block::Paragraph(paragraph));
        }
        if let Some(section) = doc.sections.first_mut() {
            section.blocks = blocks;
        }
        // Подряд идущие пункты — один список.
        let mut start = 0;
        while start < bullets.len() {
            let mut end = start;
            while end + 1 < bullets.len() && bullets[end + 1] == bullets[end] + 1 {
                end += 1;
            }
            toggle_list(
                &mut doc,
                Position::new(bullets[start], 0),
                Position::new(bullets[end], 0),
                ListKind::Bullet,
            );
            start = end + 1;
        }
        doc
    }
}

/// Вид строки встроенного шаблона.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Line {
    /// Название (стиль «Title»).
    Title,
    /// Заголовок 1.
    Heading,
    /// Обычный абзац.
    Text,
    /// Абзац по правому краю.
    Right,
    /// Полужирный абзац.
    Bold,
    /// Полужирный по центру.
    CenterBold,
    /// Пункт маркированного списка.
    Bullet,
    /// Пустой абзац.
    Empty,
}

/// Шаблон в галерее «Создать».
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Template {
    /// Встроенный.
    Builtin(Builtin),
    /// Личный: файл шаблона.
    Personal(PathBuf),
}

impl Template {
    /// Имя на плитке.
    pub fn name(&self) -> String {
        match self {
            Template::Builtin(builtin) => builtin.name().to_string(),
            Template::Personal(path) => path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    }

    /// Документ по шаблону.
    pub fn document(&self) -> Result<Document, String> {
        match self {
            Template::Builtin(builtin) => Ok(builtin.document()),
            Template::Personal(path) => crate::files::read(path),
        }
    }
}

/// Папка личных шаблонов в папке программы `dir`.
pub fn personal_dir(dir: &Path) -> PathBuf {
    dir.join("templates")
}

/// Личные шаблоны из папки `dir` по алфавиту (нет папки — пусто).
pub fn personal(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && crate::files::is_template(p))
        .collect();
    paths.sort();
    paths
}

/// Копирует файл шаблона `file` в папку личных шаблонов `dir`.
/// Возвращает путь копии.
pub fn add_personal(dir: &Path, file: &Path) -> Result<PathBuf, String> {
    if !crate::files::is_template(file) {
        return Err(format!("{}: {}", file.display(), tr("error-not-template")));
    }
    let name = file
        .file_name()
        .ok_or_else(|| tr("error-not-template").to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let target = dir.join(name);
    std::fs::copy(file, &target).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_templates_have_text_and_styles() {
        for builtin in Builtin::ALL {
            let doc = builtin.document();
            let text = doc.plain_text();
            assert!(!text.trim().is_empty(), "{builtin:?}");
            assert!(!text.contains("tpl-"), "нет перевода: {text}");
            assert!(!builtin.name().starts_with("template-"));
            // Сохраняется и открывается как .docx.
            let bytes = letter_docx::write_bytes(&doc).unwrap();
            let again = letter_docx::read_bytes(&bytes).unwrap();
            assert_eq!(again.plain_text(), text);
        }
        let resume = Builtin::Resume.document();
        let styles: Vec<Option<&str>> = resume
            .blocks()
            .filter_map(|b| match b {
                Block::Paragraph(p) => Some(p.props.style.as_deref()),
                _ => None,
            })
            .collect();
        assert_eq!(styles[0], Some("Title"));
        assert!(styles.contains(&Some("Heading1")));
        let listed = resume
            .blocks()
            .filter(|b| matches!(b, Block::Paragraph(p) if p.props.numbering.is_some()))
            .count();
        assert_eq!(listed, 5);
    }

    #[test]
    fn personal_templates_are_found_and_added() {
        let dir = crate::files::tests::temp_dir("templates");
        let source = dir.join("Бланк.dotx");
        std::fs::write(
            &source,
            letter_docx::write_bytes(&Builtin::Memo.document()).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.join("заметки.txt"), "x").unwrap();
        let folder = personal_dir(&dir);
        assert!(personal(&folder).is_empty());
        let added = add_personal(&folder, &source).unwrap();
        assert_eq!(personal(&folder), std::slice::from_ref(&added));
        let template = Template::Personal(added);
        assert_eq!(template.name(), "Бланк");
        assert!(template.document().unwrap().plain_text().contains(':'));
        assert!(add_personal(&folder, &dir.join("заметки.txt")).is_err());
    }
}
