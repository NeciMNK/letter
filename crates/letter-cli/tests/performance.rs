//! F10.3: документ на 500 страниц открывается быстрее 2 секунд и
//! занимает меньше 500 МБ памяти.
//!
//! Тест сам строит большой .docx (абзацы, заголовки, таблицы), затем
//! замеряет «открытие»: чтение файла и вёрстку всех страниц. В отладочной
//! сборке (обычный `cargo test`) программа в разы медленнее, поэтому
//! время там только печатается; проверка срабатывает в сборке
//! `--release` (так её запускает CI):
//!
//! ```text
//! cargo test --release -p letter-cli --test performance -- --nocapture
//! ```

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};

use letter_layout::{FontLibrary, Shaper, layout_document};
use letter_model::{Block, Document, Paragraph, Table, TableCell, TableRow};

/// Сколько страниц должно получиться.
const PAGES: usize = 500;
/// Предел времени открытия.
const TIME_LIMIT: Duration = Duration::from_secs(2);
/// Предел памяти (пиковой, всего процесса), байт.
const MEMORY_LIMIT: u64 = 500 * 1024 * 1024;

const TEXT: &str = "Съешь же ещё этих мягких французских булок, да выпей чаю. \
The quick brown fox jumps over the lazy dog. Широкая электрификация южных \
губерний даст мощный толчок подъёму сельского хозяйства. ";

/// Документ примерно на `PAGES` страниц: на страницу — заголовок, пятнадцать
/// абзацев по 4–5 строк, каждая десятая страница — с таблицей.
fn big_document() -> Document {
    let mut doc = Document::new();
    let blocks = &mut doc.sections[0].blocks;
    blocks.clear();
    for page in 0..PAGES + PAGES / 20 {
        let mut heading = Paragraph::text(format!("Глава {}", page + 1));
        heading.props.style = Some("Heading1".into());
        blocks.push(heading.into());
        for i in 0..15 {
            blocks.push(Paragraph::text(format!("{page}.{i}. {}", TEXT.repeat(2))).into());
        }
        if page % 10 == 0 {
            let rows = (0..4)
                .map(|r| TableRow {
                    cells: (0..3)
                        .map(|c| TableCell::text(format!("Ячейка {r}.{c}")))
                        .collect(),
                    ..Default::default()
                })
                .collect();
            blocks.push(Block::Table(Table {
                rows,
                ..Default::default()
            }));
        }
    }
    doc
}

/// Пиковая память процесса, байт (только Linux: строка `VmHWM` в
/// `/proc/self/status`). На других системах — `None`.
fn peak_memory() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

#[test]
fn opening_500_pages_is_fast() {
    let bytes = letter_docx::write_bytes(&big_document()).unwrap();
    // Шрифты ищутся в системе один раз при запуске программы, это не
    // часть открытия документа.
    let library = FontLibrary::embedded_only();

    let start = Instant::now();
    let doc = letter_docx::read_bytes(&bytes).unwrap();
    let read = start.elapsed();
    let layout = layout_document(&doc, &mut Shaper::new(), &library);
    let total = start.elapsed();
    let memory = peak_memory();

    println!(
        "Файл {:.1} МБ, страниц {}: чтение {read:.2?}, всего {total:.2?}, память {}",
        bytes.len() as f64 / 1e6,
        layout.pages.len(),
        memory.map_or("неизвестно".to_string(), |m| format!(
            "{} МБ",
            m / 1024 / 1024
        )),
    );
    assert!(
        layout.pages.len() >= PAGES,
        "страниц {}, нужно не меньше {PAGES}",
        layout.pages.len()
    );
    if cfg!(debug_assertions) {
        return;
    }
    assert!(
        total < TIME_LIMIT,
        "открытие заняло {total:.2?}, предел {TIME_LIMIT:?}"
    );
    if let Some(memory) = memory {
        assert!(
            memory < MEMORY_LIMIT,
            "память {} МБ, предел {} МБ",
            memory / 1024 / 1024,
            MEMORY_LIMIT / 1024 / 1024
        );
    }
}
