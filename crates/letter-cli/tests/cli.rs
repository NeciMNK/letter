//! Команды `letter-cli` на образцах из `tests/fixtures/docx`.

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use letter_cli::{USAGE, run};

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/docx")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn dump_prints_the_tree() {
    let out = run(&args(&["dump", &fixture("headings.docx")])).unwrap();
    assert!(out.starts_with("Документ: разделов 1"), "{out}");
    assert!(out.contains("Название: \"Заголовки\""));
    assert!(out.contains("  Абзац [стиль Heading1]\n    Фрагмент: \"Heading 1\""));

    let table = run(&args(&["dump", &fixture("table.docx")])).unwrap();
    assert!(table.contains("Таблица 3×3"));
    assert!(table.contains("Ячейка 1.1"));

    let formatted = run(&args(&["dump", &fixture("bold-italic.docx")])).unwrap();
    assert!(formatted.contains("[ж]"), "{formatted}");
}

#[test]
fn dump_debug_prints_all_fields() {
    let out = run(&args(&["dump", "--debug", &fixture("plain-text.docx")])).unwrap();
    assert!(out.contains("Document {"));
    assert!(out.contains("sections:"));
}

#[test]
fn roundtrip_reports_equal_models() {
    let dir = std::env::temp_dir().join(format!("letter-cli-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let output = dir.join("копия.docx");
    let out = run(&args(&[
        "roundtrip",
        &fixture("image.docx"),
        &output.to_string_lossy(),
    ]))
    .unwrap();
    assert!(out.contains("совпадают"), "{out}");
    assert!(letter_docx::read_file(&output).is_ok());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn errors_are_reported() {
    assert_eq!(run(&args(&[])), Err(USAGE.to_string()));
    assert_eq!(run(&args(&["bogus", "x"])), Err(USAGE.to_string()));
    let missing = run(&args(&["dump", "нет-такого-файла.docx"])).unwrap_err();
    assert!(missing.starts_with("нет-такого-файла.docx: "), "{missing}");
}

#[test]
fn pages_prints_lines_of_each_page() {
    let out = run(&args(&[
        "pages",
        "--embedded-fonts",
        &fixture("headings.docx"),
    ]))
    .unwrap();
    assert!(out.starts_with("Страница 1: "), "{out}");
    assert!(out.contains("абзац 2   \"Heading 1\""), "{out}");
    assert!(run(&args(&["pages"])).is_err());
}

#[test]
fn render_writes_png_pages() {
    let dir = std::env::temp_dir().join(format!("letter-cli-render-{}", std::process::id()));
    let out = run(&args(&[
        "render",
        "--embedded-fonts",
        &fixture("headings.docx"),
        &dir.to_string_lossy(),
    ]))
    .unwrap();
    // Letter 8,5×11 дюймов при 96 точках на дюйм.
    assert!(out.contains("page-1.png (816×1056)"), "{out}");
    let bytes = std::fs::read(dir.join("page-1.png")).unwrap();
    assert!(bytes.starts_with(b"\x89PNG"));
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(run(&args(&["render", &fixture("headings.docx")])).is_err());
}

fn reference(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/reference/libreoffice")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

#[test]
fn pdf_lines_prints_text_with_positions() {
    let out = run(&args(&["pdf-lines", &reference("ru-en.pdf")])).unwrap();
    assert!(
        out.starts_with("Страница 1: 215.9×279.4 мм, строк 3"),
        "{out}"
    );
    assert!(
        out.contains("\"The quick brown fox jumps over the lazy dog.\""),
        "{out}"
    );
    assert!(out.contains("«кавычки»"), "{out}");
}

#[test]
fn compare_reports_differences() {
    let out = run(&args(&[
        "compare",
        "--embedded-fonts",
        "--tolerance",
        "0,5",
        &fixture("table.docx"),
        &reference("table.pdf"),
    ]))
    .unwrap();
    assert!(out.starts_with("Страниц: эталон 1, у нас 1\n"), "{out}");
    assert!(out.contains("допуск 0.50 пт"), "{out}");
    // Строки таблицы на своих местах; с допуском 0,5 пт расходится
    // только первая строка.
    assert!(!out.contains("только"), "{out}");
    assert!(out.contains("«A 3×3 table with a merged cell:»: сдвиг"), "{out}");

    assert!(run(&args(&["compare", &fixture("table.docx")])).is_err());
    assert!(run(&args(&["compare", "--tolerance", "x", "a.docx", "b.pdf"])).is_err());
    let missing = run(&args(&["compare", &fixture("table.docx"), "missing.pdf"]));
    assert!(missing.unwrap_err().starts_with("missing.pdf:"));
}
