//! Сверка нашей вёрстки с эталонными PDF из `tests/fixtures/reference`
//! (задача F2.8 плана) — постоянная метрика качества вёрстки.
//!
//! Для каждого `reference/<источник>/<имя>.pdf` берётся
//! `tests/fixtures/docx/<имя>.docx`, раскладывается по страницам (только
//! встроенные шрифты, чтобы результат не зависел от компьютера)
//! и сравнивается с эталоном. Полный отчёт пишется в
//! `target/tmp/layout-compare.md` и печатается (`cargo test -p letter-cli
//! --test compare -- --nocapture`).
//!
//! Тест падает, только если стало хуже, чем записано в
//! `reference/baseline.txt`. Стало лучше — обновить файл:
//! `LETTER_UPDATE_BASELINE=1 cargo test -p letter-cli --test compare`.
//! Другой допуск (в пунктах): `LETTER_COMPARE_TOLERANCE=0.5`; тогда
//! сравнения с `baseline.txt` нет, только отчёт.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use letter_cli::compare::{Comparison, DEFAULT_TOLERANCE};
use letter_cli::compare_files;
use letter_layout::FontLibrary;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

/// Эталоны: (`источник/имя`, путь к PDF), по порядку.
fn references(root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut sources: Vec<PathBuf> = std::fs::read_dir(root)
        .unwrap_or_else(|e| panic!("{}: {e}", root.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.is_dir())
        .collect();
    sources.sort();
    for source in sources {
        let source_name = source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut pdfs: Vec<PathBuf> = std::fs::read_dir(&source)
            .unwrap_or_else(|e| panic!("{}: {e}", source.display()))
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
            })
            .collect();
        pdfs.sort();
        for pdf in pdfs {
            let stem = pdf
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.push((format!("{source_name}/{stem}"), pdf));
        }
    }
    out
}

/// Записанные значения: имя → (перенос и страница верны, совпало
/// полностью).
fn read_baseline(path: &Path) -> BTreeMap<String, (usize, usize)> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?.to_string();
            let paired = parts.next()?.parse().ok()?;
            let matched = parts.next()?.parse().ok()?;
            Some((name, (paired, matched)))
        })
        .collect()
}

fn write_baseline(path: &Path, results: &[(String, Comparison)]) {
    let mut text = format!(
        "# Сверка вёрстки с эталонами (crates/letter-cli/tests/compare.rs).\n\
         # Для каждого эталона: сколько строк стоит на своём месте по тексту\n\
         # (перенос и страница как в эталоне) и сколько из них совпало ещё и\n\
         # по положению (допуск {DEFAULT_TOLERANCE} пт). Тест падает, если число стало\n\
         # меньше. Обновить: LETTER_UPDATE_BASELINE=1 cargo test -p letter-cli --test compare\n"
    );
    for (name, result) in results {
        let _ = writeln!(text, "{name} {} {}", result.paired, result.matched);
    }
    std::fs::write(path, text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}

#[test]
fn layout_is_not_worse_than_baseline() {
    let root = fixtures().join("reference");
    let tolerance = std::env::var("LETTER_COMPARE_TOLERANCE")
        .ok()
        .and_then(|v| v.replace(',', ".").parse::<f32>().ok());
    let library = FontLibrary::embedded_only();
    let mut results = Vec::new();
    for (name, pdf) in references(&root) {
        let stem = name.rsplit('/').next().unwrap_or_default();
        let docx = fixtures().join("docx").join(format!("{stem}.docx"));
        let result = compare_files(
            &docx,
            &pdf,
            &library,
            tolerance.unwrap_or(DEFAULT_TOLERANCE),
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        results.push((name, result));
    }
    assert!(!results.is_empty(), "нет эталонов в {}", root.display());

    let mut summary = String::from(
        "| Эталон | Страниц (эталон / мы) | Строк (эталон / мы) | Перенос верен | Совпало | % | Сдвиг, пт (средний / самый большой) |\n\
         |---|---|---|---|---|---|---|\n",
    );
    let mut details = String::new();
    for (name, r) in &results {
        let _ = writeln!(
            summary,
            "| {name} | {} / {} | {} / {} | {} | {} | {:.1} | {:+.2} / {:+.2} |",
            r.reference_pages,
            r.our_pages,
            r.reference_lines,
            r.our_lines,
            r.paired,
            r.matched,
            r.score(),
            r.mean_shift,
            r.max_shift
        );
        let _ = write!(details, "\n## {name}\n\n```\n{}```\n", r.report());
    }
    let report = format!("# Сверка вёрстки с эталонами\n\n{summary}{details}");
    let report_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("layout-compare.md");
    if let Err(e) = std::fs::write(&report_path, &report) {
        println!("не удалось записать {}: {e}", report_path.display());
    }
    println!("{summary}\nПолный отчёт: {}", report_path.display());

    let baseline_path = root.join("baseline.txt");
    if std::env::var_os("LETTER_UPDATE_BASELINE").is_some() {
        write_baseline(&baseline_path, &results);
        println!("Обновлён {}", baseline_path.display());
        return;
    }
    if tolerance.is_some() {
        println!("Задан свой допуск — с baseline.txt не сравниваю.");
        return;
    }
    let baseline = read_baseline(&baseline_path);
    let mut worse = Vec::new();
    let mut better = Vec::new();
    for (name, r) in &results {
        match baseline.get(name) {
            // Новый эталон (например, владелец добавил PDF из Word) не
            // должен ронять проверку: он попадёт в файл при обновлении.
            None => better.push(name.as_str()),
            Some(&(paired, matched)) => {
                if r.paired < paired || r.matched < matched {
                    worse.push(format!(
                        "{name}: перенос верен {} (было {paired}), совпало {} (было {matched})",
                        r.paired, r.matched
                    ));
                } else if r.paired > paired || r.matched > matched {
                    better.push(name.as_str());
                }
            }
        }
    }
    if !better.is_empty() {
        println!(
            "Стало лучше или эталон новый: {} — обнови baseline.txt (LETTER_UPDATE_BASELINE=1).",
            better.join(", ")
        );
    }
    assert!(
        worse.is_empty(),
        "Вёрстка стала хуже эталона:\n{}\n\n{report}",
        worse.join("\n")
    );
}
