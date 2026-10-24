#![no_main]

mod common;

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Ok(doc) = letter_odt::read(data) {
        common::use_document(&doc);
    }
});
