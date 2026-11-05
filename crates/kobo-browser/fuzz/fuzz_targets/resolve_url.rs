//! Anything typed into the Go to field: it becomes an address or nothing.
#![no_main]

use kobo_browser_core::address::{resolve, DEFAULT_SEARCH};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(typed) = std::str::from_utf8(data) else {
        return;
    };
    if let Some(address) = resolve(typed, DEFAULT_SEARCH) {
        let text = address.url().to_string();
        assert!(
            text.starts_with("https://") || text.starts_with("http://"),
            "{text}"
        );
    }
});
