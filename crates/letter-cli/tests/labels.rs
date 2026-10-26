//! F9.6: лист наклеек переживает сохранение в .docx и помещается на
//! одну страницу, а слияние даёт по листу на каждые N записей.

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use letter_editor::{
    LABELS, MergeData, Position, has_next_records, insert_merge_field, make_labels, merge_all,
    update_labels,
};
use letter_layout::{FontLibrary, Shaper, layout_document};
use letter_model::Document;

#[test]
fn label_sheet_survives_round_trip_and_fits_a_page() {
    let library = FontLibrary::embedded_only();
    for spec in &LABELS {
        let mut doc = Document::new();
        make_labels(&mut doc, spec);
        insert_merge_field(&mut doc, Position::new(0, 0), "Имя");
        update_labels(&mut doc);
        let bytes = letter_docx::write_bytes(&doc).unwrap();
        let again = letter_docx::read_bytes(&bytes).unwrap();
        assert!(has_next_records(&again), "{}", spec.name);
        assert_eq!(
            again.sections[0].blocks, doc.sections[0].blocks,
            "{}",
            spec.name
        );
        assert_eq!(
            again.sections[0].props.margins,
            doc.sections[0].props.margins
        );
        let pages = layout_document(&again, &mut Shaper::new(), &library)
            .pages
            .len();
        assert_eq!(pages, 1, "{}", spec.name);
        // Записей на полтора листа — два листа.
        let count = (spec.columns * spec.rows * 3 / 2) as usize;
        let rows: String = (0..count).map(|n| format!("{n}\n")).collect();
        let data = MergeData::from_csv(&format!("Имя\n{rows}"));
        let merged = merge_all(&again, &data);
        let pages = layout_document(&merged, &mut Shaper::new(), &library)
            .pages
            .len();
        assert_eq!(pages, 2, "{}", spec.name);
    }
}
