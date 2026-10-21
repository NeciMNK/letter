//! Печатает текст документа: `cargo run -p letter-docx --example dump_text -- файл.docx`.

fn main() {
    for path in std::env::args().skip(1) {
        match letter_docx::read_file(&path) {
            Ok(doc) => println!("== {path}\n{}", doc.plain_text()),
            Err(e) => println!("== {path}: ошибка: {e}"),
        }
    }
}
