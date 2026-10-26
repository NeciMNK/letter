//! F9.6: лист наклеек и конверт переживают сохранение в .docx и
//! помещаются на свои страницы, а слияние даёт по листу на каждые N записей.

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

#[test]
fn envelope_survives_round_trip_and_takes_one_page() {
    let library = FontLibrary::embedded_only();
    for size in letter_editor::ENVELOPES {
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![letter_model::Paragraph::text("Письмо").into()];
        let envelope = letter_editor::Envelope {
            size,
            delivery: "Петрову П. П.\nул. Мира, 1\nОмск".into(),
            return_address: "Иванов И. И.\nМосква".into(),
            address_block: false,
        };
        letter_editor::add_envelope(&mut doc, &envelope);
        let bytes = letter_docx::write_bytes(&doc).unwrap();
        let again = letter_docx::read_bytes(&bytes).unwrap();
        assert!(letter_editor::has_envelope(&again), "{}", size.name);
        assert_eq!(again.sections[0].blocks, doc.sections[0].blocks);
        assert_eq!(
            again.sections[0].props.page_size,
            doc.sections[0].props.page_size
        );
        let layout = layout_document(&again, &mut Shaper::new(), &library);
        // Конверт и письмо.
        assert_eq!(layout.pages.len(), 2, "{}", size.name);
    }
}
