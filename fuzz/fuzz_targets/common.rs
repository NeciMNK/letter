//! Общее для целей фаззинга: прочитанный документ должен ещё
//! свёрстываться и сохраняться без паники.

use letter_layout::{FontLibrary, Shaper, layout_document};
use letter_model::Document;

pub fn use_document(doc: &Document) {
    // Только встроенные шрифты: быстро и одинаково на всех машинах.
    let library = FontLibrary::embedded_only();
    let _ = layout_document(doc, &mut Shaper::new(), &library);
    let _ = letter_docx::write_bytes(doc);
    let _ = letter_odt::write(doc);
    let _ = letter_rtf::write(doc);
    let _ = letter_text::write_html(doc);
    let _ = letter_text::write_markdown(doc);
}
