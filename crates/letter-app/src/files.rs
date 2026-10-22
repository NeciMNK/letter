//! Файлы: чтение и запись документов во всех форматах, список недавних
//! файлов и автосохранение для восстановления после сбоя.
//!
//! Настройки и автосохранения лежат в папке программы:
//! Windows — `%APPDATA%\letter`, macOS — `~/Library/Application
//! Support/letter`, Linux — `$XDG_CONFIG_HOME/letter` (или
//! `~/.config/letter`).

use crate::i18n::tr;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use letter_model::Document;

/// Формат файла документа.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFormat {
    /// Документ Word (.docx).
    Docx,
    /// RTF.
    Rtf,
    /// OpenDocument (.odt, а для чтения ещё и плоский .fodt).
    Odt,
    /// Простой текст, HTML или Markdown.
    Text(letter_text::Format),
}

impl FileFormat {
    /// Формат по расширению файла.
    pub fn from_path(path: &Path) -> Option<FileFormat> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match extension.as_str() {
            "docx" | "docm" | "dotx" | "dotm" => FileFormat::Docx,
            "rtf" => FileFormat::Rtf,
            "odt" | "fodt" => FileFormat::Odt,
            _ => FileFormat::Text(letter_text::Format::from_path(path)?),
        })
    }
}

/// Фильтры окна «Открыть»: название и расширения.
pub const OPEN_FILTERS: &[(&str, &[&str])] = &[
    (
        "filter-all-documents",
        &[
            "docx", "docm", "dotx", "dotm", "rtf", "odt", "fodt", "txt", "html", "htm", "md",
        ],
    ),
    ("filter-docx", &["docx", "docm", "dotx", "dotm"]),
    ("filter-rtf", &["rtf"]),
    ("filter-odt", &["odt", "fodt"]),
    ("filter-txt", &["txt"]),
    ("filter-html", &["html", "htm"]),
    ("filter-md", &["md"]),
];

/// Фильтры окна «Сохранить как». Первый — формат по умолчанию.
pub const SAVE_FILTERS: &[(&str, &[&str])] = &[
    ("filter-docx", &["docx"]),
    ("filter-rtf", &["rtf"]),
    ("filter-odt", &["odt"]),
    ("filter-txt", &["txt"]),
    ("filter-html", &["html"]),
    ("filter-md", &["md"]),
];

/// Новый пустой документ: стили как у пустого документа Word.
pub fn blank() -> Document {
    letter_text::read_txt(&[])
}

/// Читает документ; формат — по расширению.
pub fn read(path: &Path) -> Result<Document, String> {
    let fail = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let format = FileFormat::from_path(path).ok_or_else(|| fail(&tr("error-unknown-format")))?;
    let data = fs::read(path).map_err(|e| fail(&e))?;
    match format {
        FileFormat::Docx => letter_docx::read_bytes(&data).map_err(|e| fail(&e)),
        FileFormat::Rtf => letter_rtf::read(&data).map_err(|e| fail(&e)),
        FileFormat::Odt => letter_odt::read(&data).map_err(|e| fail(&e)),
        FileFormat::Text(letter_text::Format::Text) => Ok(letter_text::read_txt(&data)),
        FileFormat::Text(letter_text::Format::Html) => Ok(letter_text::read_html(&data)),
        FileFormat::Text(letter_text::Format::Markdown) => Ok(letter_text::read_markdown(&data)),
    }
}

/// Записывает документ; формат — по расширению. Сначала пишется
/// временный файл рядом, потом он заменяет прежний: если программа
/// упадёт посреди записи, старый файл останется целым.
pub fn write(doc: &Document, path: &Path) -> Result<(), String> {
    let fail = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let format = FileFormat::from_path(path).ok_or_else(|| fail(&tr("error-unknown-format")))?;
    let data = match format {
        FileFormat::Docx => letter_docx::write_bytes(doc).map_err(|e| fail(&e))?,
        FileFormat::Rtf => letter_rtf::write(doc).into_bytes(),
        FileFormat::Odt => letter_odt::write(doc).map_err(|e| fail(&e))?,
        FileFormat::Text(letter_text::Format::Text) => letter_text::write_txt(doc),
        FileFormat::Text(letter_text::Format::Html) => letter_text::write_html(doc).into_bytes(),
        FileFormat::Text(letter_text::Format::Markdown) => {
            letter_text::write_markdown(doc).into_bytes()
        }
    };
    write_atomic(path, &data).map_err(|e| fail(&e))
}

pub(crate) fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = path.with_file_name(format!("~${name}.tmp"));
    let result = (|| {
        let mut file = File::create(&temp)?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Папка программы для настроек и автосохранений.
pub fn app_dir() -> Option<PathBuf> {
    let env = |name| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(windows) {
        env("APPDATA")?
    } else if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support")
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))?
    };
    Some(base.join("letter"))
}

/// Сколько недавних файлов помнить.
const RECENT_LIMIT: usize = 10;

/// Список недавних файлов (меню «Файл → Последние»). Хранится в
/// `recent.txt`, по пути на строку, новые сверху.
#[derive(Debug, Clone, Default)]
pub struct Recent {
    file: Option<PathBuf>,
    items: Vec<PathBuf>,
}

impl Recent {
    /// Читает список из папки программы `dir` (`None` — не хранить).
    pub fn load(dir: Option<&Path>) -> Recent {
        let file = dir.map(|d| d.join("recent.txt"));
        let items = file
            .as_ref()
            .and_then(|f| fs::read_to_string(f).ok())
            .map(|text| {
                text.lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(PathBuf::from)
                    .take(RECENT_LIMIT)
                    .collect()
            })
            .unwrap_or_default();
        Recent { file, items }
    }

    /// Файлы, новые сверху.
    pub fn items(&self) -> &[PathBuf] {
        &self.items
    }

    /// Ставит файл в начало списка и сохраняет список.
    pub fn add(&mut self, path: &Path) {
        let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        self.items.retain(|p| p != &path);
        self.items.insert(0, path);
        self.items.truncate(RECENT_LIMIT);
        self.save();
    }

    /// Убирает файл из списка (например, его больше нет).
    pub fn remove(&mut self, path: &Path) {
        self.items.retain(|p| p != path);
        self.save();
    }

    fn save(&self) {
        let Some(file) = &self.file else { return };
        let text: String = self
            .items
            .iter()
            .map(|p| format!("{}\n", p.display()))
            .collect();
        // Список недавних — не главное: если записать не вышло, молчим.
        if let Some(dir) = file.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = write_atomic(file, text.as_bytes());
    }
}

/// Автосохранение одного запущенного экземпляра программы. Пока
/// программа работает, она держит замок на файле `<номер>.lock`; если
/// программа упала, замок снимается, и при следующем запуске
/// автосохранения можно восстановить.
#[derive(Debug)]
pub struct Autosave {
    dir: PathBuf,
    id: String,
    /// Держит замок, пока программа работает.
    _lock: File,
}

/// Документ, оставшийся после сбоя.
#[derive(Debug, Clone, PartialEq)]
pub struct Recovered {
    /// Автосохранённый файл (.docx).
    pub autosave: PathBuf,
    /// Откуда документ был открыт (`None` — новый, не сохранённый).
    pub original: Option<PathBuf>,
    /// Когда сохранён.
    pub saved: Option<SystemTime>,
}

impl Autosave {
    /// Начинает автосохранение в папке `dir` (подпапка `autosave`
    /// папки программы). `None`, если папку не создать.
    pub fn start(dir: &Path) -> Option<Autosave> {
        fs::create_dir_all(dir).ok()?;
        let id = format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis())
        );
        let lock = File::create(dir.join(format!("{id}.lock"))).ok()?;
        lock.try_lock().ok()?;
        Some(Autosave {
            dir: dir.to_owned(),
            id,
            _lock: lock,
        })
    }

    fn files(&self) -> (PathBuf, PathBuf) {
        (
            self.dir.join(format!("{}.docx", self.id)),
            self.dir.join(format!("{}.path", self.id)),
        )
    }

    /// Сохраняет копию документа. `original` — откуда он открыт.
    pub fn save(&self, doc: &Document, original: Option<&Path>) -> Result<(), String> {
        let (copy, note) = self.files();
        let data = letter_docx::write_bytes(doc).map_err(|e| e.to_string())?;
        write_atomic(&copy, &data).map_err(|e| e.to_string())?;
        let original = original
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        write_atomic(&note, original.as_bytes()).map_err(|e| e.to_string())
    }

    /// Убирает копию: документ сохранён или закрыт без изменений.
    pub fn clear(&self) {
        let (copy, note) = self.files();
        let _ = fs::remove_file(copy);
        let _ = fs::remove_file(note);
    }

    /// Документы, оставшиеся от упавших экземпляров программы: их замки
    /// никто не держит.
    pub fn leftovers(dir: &Path) -> Vec<Recovered> {
        let Ok(entries) = fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("docx") {
                continue;
            }
            let lock = path.with_extension("lock");
            // Замок держит работающая программа — это не наше.
            if let Ok(file) = File::options().write(true).open(&lock)
                && file.try_lock().is_err()
            {
                continue;
            }
            let original = fs::read_to_string(path.with_extension("path"))
                .ok()
                .filter(|p| !p.trim().is_empty())
                .map(|p| PathBuf::from(p.trim()));
            let saved = entry.metadata().and_then(|m| m.modified()).ok();
            found.push(Recovered {
                autosave: path,
                original,
                saved,
            });
        }
        found.sort_by_key(|r| std::cmp::Reverse(r.saved));
        found
    }

    /// Удаляет оставшийся после сбоя документ (восстановлен или не нужен).
    pub fn discard(recovered: &Recovered) {
        let path = &recovered.autosave;
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(path.with_extension("path"));
        let _ = fs::remove_file(path.with_extension("lock"));
    }
}

impl Drop for Autosave {
    fn drop(&mut self) {
        // Программа закрылась как надо: копия не нужна.
        self.clear();
        let _ = fs::remove_file(self.dir.join(format!("{}.lock", self.id)));
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use letter_model::{Block, Paragraph, Run, RunProps};

    /// Пустая временная папка для теста.
    pub fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "letter-test-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample() -> Document {
        let mut doc = blank();
        doc.sections[0].blocks = vec![
            Paragraph::from_runs([
                Run::text("Привет, "),
                Run::text("мир").with_props(RunProps {
                    bold: Some(true),
                    ..RunProps::default()
                }),
            ])
            .into(),
            Paragraph::text("второй").into(),
        ];
        doc
    }

    #[test]
    fn formats_by_extension() {
        let format = |p: &str| FileFormat::from_path(Path::new(p));
        assert_eq!(format("a.DOCX"), Some(FileFormat::Docx));
        assert_eq!(format("a.rtf"), Some(FileFormat::Rtf));
        assert_eq!(format("a.fodt"), Some(FileFormat::Odt));
        assert_eq!(
            format("a.md"),
            Some(FileFormat::Text(letter_text::Format::Markdown))
        );
        assert_eq!(format("a.pdf"), None);
        assert_eq!(format("a"), None);
    }

    #[test]
    fn blank_document_has_one_empty_paragraph_and_styles() {
        let doc = blank();
        assert_eq!(doc.plain_text(), "");
        assert!(matches!(doc.blocks().next(), Some(Block::Paragraph(_))));
        assert!(doc.styles.get("Normal").is_some());
    }

    #[test]
    fn every_format_round_trips_its_text() {
        let dir = temp_dir("formats");
        for ext in ["docx", "rtf", "odt", "txt", "html", "md"] {
            let path = dir.join(format!("doc.{ext}"));
            write(&sample(), &path).unwrap();
            let back = read(&path).unwrap();
            assert_eq!(back.plain_text().trim(), "Привет, мир\nвторой", "{ext}");
            // Временный файл не остался.
            let left: Vec<_> = fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("~$"))
                .collect();
            assert!(left.is_empty(), "{ext}");
        }
        assert!(read(&dir.join("missing.docx")).is_err());
        assert!(write(&sample(), &dir.join("doc.pdf")).is_err());
        fs::write(dir.join("bad.docx"), b"not a zip").unwrap();
        assert!(read(&dir.join("bad.docx")).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recent_files_newest_first() {
        let dir = temp_dir("recent");
        let mut recent = Recent::load(Some(&dir));
        assert!(recent.items().is_empty());
        for i in 0..12 {
            recent.add(Path::new(&format!("/nowhere/{i}.docx")));
        }
        recent.add(Path::new("/nowhere/5.docx"));
        let again = Recent::load(Some(&dir));
        assert_eq!(again.items().len(), RECENT_LIMIT);
        assert_eq!(again.items()[0], Path::new("/nowhere/5.docx"));
        assert_eq!(again.items()[1], Path::new("/nowhere/11.docx"));
        recent.remove(Path::new("/nowhere/5.docx"));
        assert_eq!(
            Recent::load(Some(&dir)).items()[0],
            Path::new("/nowhere/11.docx")
        );
        // Без папки — просто в памяти.
        let mut memory = Recent::load(None);
        memory.add(Path::new("/x.docx"));
        assert_eq!(memory.items().len(), 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn autosave_survives_only_a_crash() {
        let dir = temp_dir("autosave");
        let running = Autosave::start(&dir).unwrap();
        running
            .save(&sample(), Some(Path::new("/docs/report.docx")))
            .unwrap();
        // Пока программа работает, её копия — не «оставшаяся».
        assert!(Autosave::leftovers(&dir).is_empty());

        // «Сбой»: копия и замок есть, но замок никто не держит.
        let crashed = dir.join("1-1.docx");
        fs::write(&crashed, letter_docx::write_bytes(&sample()).unwrap()).unwrap();
        fs::write(dir.join("1-1.path"), "").unwrap();
        fs::write(dir.join("1-1.lock"), "").unwrap();
        let found = Autosave::leftovers(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].original, None);
        let doc = read(&found[0].autosave).unwrap();
        assert_eq!(doc.plain_text(), "Привет, мир\nвторой");
        Autosave::discard(&found[0]);
        assert!(Autosave::leftovers(&dir).is_empty());

        // Закрылась как надо — копии нет.
        drop(running);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        let _ = fs::remove_dir_all(dir);
    }
}
