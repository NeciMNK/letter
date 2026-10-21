//! Замер скорости вёрстки большого документа (F3.9): сначала целиком,
//! потом после правки одного абзаца в середине.
//!
//! Запуск: `cargo run --release -p letter-layout --example relayout`.

use std::time::Instant;

use letter_layout::{FontLibrary, LayoutCache, Shaper, layout_document_cached};
use letter_model::{Block, Document, Paragraph};

fn main() {
    let text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod \
                tempor incididunt ut labore et dolore magna aliqua. "
        .repeat(4);
    let mut doc = Document::new();
    doc.sections[0].blocks = (0..3000)
        .map(|_| Block::from(Paragraph::text(text.clone())))
        .collect();
    let library = FontLibrary::embedded_only();
    let mut shaper = Shaper::new();
    let mut cache = LayoutCache::default();

    let start = Instant::now();
    let layout = layout_document_cached(&doc, &mut shaper, &library, &mut cache);
    println!(
        "целиком: {} стр., {:?}",
        layout.pages.len(),
        start.elapsed()
    );
    for i in 0..5 {
        doc.sections[0].blocks[1500] = Block::from(Paragraph::text(format!("{text}{i}")));
        let start = Instant::now();
        let layout = layout_document_cached(&doc, &mut shaper, &library, &mut cache);
        println!(
            "после правки: {} стр., заново абзацев: {}, {:?}",
            layout.pages.len(),
            cache.rebuilt(),
            start.elapsed()
        );
    }
}
