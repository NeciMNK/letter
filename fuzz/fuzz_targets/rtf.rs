#![no_main]

mod common;

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Ok(doc) = letter_rtf::read(data) {
        common::use_document(&doc);
    }
});
