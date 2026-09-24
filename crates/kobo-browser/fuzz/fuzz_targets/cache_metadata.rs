//! Any bytes as the stored cache index: it must read as something, stay
//! within its bounds, and survive being written and read again.
#![no_main]

use kobo_browser_core::cache::{Index, MAX_BYTES, MAX_ENTRIES};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let index = Index::decode(data);
    assert!(index.entries().len() <= MAX_ENTRIES);
    assert!(index.entries().iter().map(|e| e.bytes).sum::<u64>() <= MAX_BYTES);
    let again = Index::decode(&index.encode());
    assert_eq!(again.entries().len(), index.entries().len());
});
