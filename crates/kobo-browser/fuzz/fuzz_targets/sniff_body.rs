//! Any response body: it is named as a page, text, or something else.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = kobo_browser_core::fetch::sniff(data);
});
