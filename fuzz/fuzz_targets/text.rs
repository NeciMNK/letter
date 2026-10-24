#![no_main]

mod common;

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    common::use_document(&letter_text::read_txt(data));
    common::use_document(&letter_text::read_html(data));
    common::use_document(&letter_text::read_markdown(data));
});
