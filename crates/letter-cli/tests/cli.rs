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
