//! F10.4: битые файлы не роняют программу.
//!
//! Тест портит образцы из `tests/fixtures` предсказуемым генератором
//! случайных чисел (один и тот же набор порчи при каждом запуске) и
//! проверяет, что чтение, вёрстка и сохранение не паникуют. Ошибка
//! чтения — правильный ответ на битый файл, паника — нет.
//!
//! Это «быстрый фаззинг» для обычного `cargo test`. Настоящий фаззер
//! (`cargo fuzz`, ищет новые входы часами) — в каталоге `fuzz/`.

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Cursor, Read, Write};
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;

use letter_layout::{FontLibrary, Shaper, layout_document};
use letter_model::Document;

/// Сколько испорченных копий делать из каждого образца. Отладочная
/// сборка в разы медленнее, поэтому в обычном `cargo test` их меньше;
/// CI гоняет полный набор в `--release`.
const MUTATIONS: usize = if cfg!(debug_assertions) { 6 } else { 40 };

/// Простой генератор случайных чисел (xorshift): без зависимостей и
/// одинаковый на всех машинах.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

/// Куски, которые чаще всего ломают разборщики.
const TOKENS: &[&[u8]] = &[
    b"<",
    b">",
    b"</w:p>",
    b"<w:p>",
    b"<w:tbl>",
    b"<w:tc>",
    b"&",
    b"&#xFFFF;",
    b"&#0;",
    b"\"",
    b"=\"-99999999999\"",
    b"=\"4294967296\"",
    b"=\"NaN\"",
    b"{",
    b"}",
    b"\\",
    b"\\u-32768?",
    b"\\bin999999 ",
    b"\0",
    b"\xff\xfe",
    b"\xd0",
    b"\n\n",
    b"# ",
    b"| a |",
    b"<table><tr><td>",
];

/// Одна случайная порча байтов.
fn mutate(data: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut out = data.to_vec();
    for _ in 0..1 + rng.below(4) {
        let at = rng.below(out.len() + 1);
        match rng.below(6) {
            // Заменить байт.
            0 if !out.is_empty() => {
                let i = rng.below(out.len());
                out[i] = rng.next() as u8;
            }
            // Вырезать кусок.
            1 => {
                let len = rng.below(64).min(out.len() - at);
                out.drain(at..at + len);
            }
            // Повторить кусок.
            2 => {
                let len = rng.below(256).min(out.len() - at);
                let piece = out[at..at + len].to_vec();
                out.splice(at..at, piece);
            }
            // Обрезать файл.
            3 => out.truncate(at),
            // Вставить опасный кусок.
            _ => {
                let token = TOKENS[rng.below(TOKENS.len())];
                out.splice(at..at, token.iter().copied());
            }
        }
    }
    out
}

/// Портит XML внутри ZIP-архива (.docx, .odt): если портить сам архив,
/// почти всё отсекает проверка контрольной суммы и до разбора XML дело
/// не доходит. Архив собирается заново без сжатия.
fn mutate_zip(data: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(data)).unwrap();
    let mut parts = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        parts.push((file.name().unwrap().to_string(), bytes));
    }
    // Чаще портим одну часть, иногда несколько.
    for _ in 0..1 + rng.below(2) {
        let i = rng.below(parts.len());
        if parts[i].0.ends_with(".xml") || parts[i].0.ends_with(".rels") {
            parts[i].1 = mutate(&parts[i].1, rng);
        }
    }
    // Иногда выбрасываем часть целиком.
    if rng.below(8) == 0 {
        let i = rng.below(parts.len());
        parts.remove(i);
    }
    let mut buffer = Cursor::new(Vec::new());
    let mut out = zip::ZipWriter::new(&mut buffer);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in parts {
        out.start_file(name, options).unwrap();
        out.write_all(&bytes).unwrap();
    }
    out.finish().unwrap();
    buffer.into_inner()
}

/// Прочитанный документ должен свёрстываться и сохраняться без паники.
fn use_document(doc: &Document, library: &FontLibrary) {
    let layout = layout_document(doc, &mut Shaper::new(), library);
    assert!(!layout.pages.is_empty());
    let _ = letter_docx::write_bytes(doc);
    let _ = letter_odt::write(doc);
    let _ = letter_rtf::write(doc);
    let _ = letter_text::write_html(doc);
    let _ = letter_text::write_markdown(doc);
    let _ = letter_text::write_txt(doc);
}

fn fixtures(dir: &str, ext: &str) -> Vec<(String, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(dir);
    let mut out: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read(&p).unwrap())
        })
        .collect();
    out.sort();
    assert!(!out.is_empty(), "нет образцов в {}", dir.display());
    out
}

/// Портит каждый образец `MUTATIONS` раз и читает `read`.
fn check(
    samples: &[(String, Vec<u8>)],
    seed: u64,
    zip: bool,
    read: impl Fn(&[u8]) -> Option<Document>,
) {
    let library = FontLibrary::embedded_only();
    for (n, (name, data)) in samples.iter().enumerate() {
        for i in 0..MUTATIONS {
            let mut rng = Rng(seed ^ ((n as u64) << 32) ^ ((i as u64 + 1) * 0x9E37_79B9));
            let broken = if zip {
                mutate_zip(data, &mut rng)
            } else {
                mutate(data, &mut rng)
            };
            // Подпись, чтобы по сообщению о панике найти, какая порча её вызвала.
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                if let Some(doc) = read(&broken) {
                    use_document(&doc, &library);
                }
            }));
            if result.is_err() {
                let path = std::env::temp_dir().join(format!("letter-broken-{i}-{name}"));
                let _ = std::fs::write(&path, &broken);
                panic!(
                    "паника на испорченном {name} (порча {i}), файл: {}",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn broken_docx_does_not_panic() {
    let samples = fixtures("docx", "docx");
    check(&samples, 1, true, |b| letter_docx::read_bytes(b).ok());
    // Испорченный сам архив тоже не должен ронять программу.
    check(&samples, 2, false, |b| letter_docx::read_bytes(b).ok());
}

#[test]
fn broken_odt_does_not_panic() {
    let samples = fixtures("odt", "odt");
    check(&samples, 3, true, |b| letter_odt::read(b).ok());
    check(&samples, 4, false, |b| letter_odt::read(b).ok());
}

#[test]
fn broken_rtf_does_not_panic() {
    check(&fixtures("rtf", "rtf"), 5, false, |b| {
        letter_rtf::read(b).ok()
    });
}

#[test]
fn broken_text_formats_do_not_panic() {
    // Текстовые форматы читают что угодно, поэтому берём как исходник
    // и RTF, и то, что letter сам пишет в HTML и Markdown.
    let rtf = fixtures("rtf", "rtf");
    let mut samples = rtf.clone();
    for (name, data) in &rtf {
        let doc = letter_rtf::read(data).unwrap();
        samples.push((
            format!("{name}.html"),
            letter_text::write_html(&doc).into_bytes(),
        ));
        samples.push((
            format!("{name}.md"),
            letter_text::write_markdown(&doc).into_bytes(),
        ));
    }
    check(&samples, 6, false, |b| Some(letter_text::read_txt(b)));
    check(&samples, 7, false, |b| Some(letter_text::read_html(b)));
    check(&samples, 8, false, |b| Some(letter_text::read_markdown(b)));
}

#[test]
fn garbage_does_not_panic() {
    let mut rng = Rng(42);
    let library = FontLibrary::embedded_only();
    for len in [0, 1, 2, 4, 22, 100, 1000, 10_000] {
        let data: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        for doc in [
            letter_docx::read_bytes(&data).ok(),
            letter_odt::read(&data).ok(),
            letter_rtf::read(&data).ok(),
            Some(letter_text::read_txt(&data)),
            Some(letter_text::read_html(&data)),
            Some(letter_text::read_markdown(&data)),
        ]
        .into_iter()
        .flatten()
        {
            use_document(&doc, &library);
        }
    }
}
