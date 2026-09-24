//! Any bytes as a page: parsing must finish within its limits.
#![no_main]

use kobo_web_document::{parse_document, Limits, Url};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(base) = Url::parse("https://example.com/a/page.html") else {
        return;
    };
    let document = parse_document(data, &base, &Limits::DEFAULT);
    for link in &document.links {
        let _ = link.text.len();
    }
});
