//! Typed addresses must resolve to stable web URLs or be refused.
#![no_main]

use kobo_browser_core::address::{resolve, Address, DEFAULT_SEARCH};
use kobo_web_document::Url;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(typed) = std::str::from_utf8(data) else {
        return;
    };
    if let Some(address) = resolve(typed, DEFAULT_SEARCH) {
        let text = address.url().to_string();
        assert!(text.starts_with("https://") || text.starts_with("http://"));
        assert_eq!(Url::parse(&text).as_ref(), Ok(address.url()));
        if let Address::Search { query, .. } = address {
            assert_eq!(query, typed.trim());
        }
    }
});
