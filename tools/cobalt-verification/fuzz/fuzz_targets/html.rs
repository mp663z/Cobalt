#![no_main]
libfuzzer_sys::fuzz_target!(|bytes: &[u8]| {
    let url = kobo_web_document::Url::parse("https://example.invalid/").unwrap();
    let _ = kobo_web_document::parse_document(bytes, &url, &kobo_web_document::Limits::default());
});
