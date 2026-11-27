//! Any stored bytes must decode within bounds and roundtrip exactly.
#![no_main]

use kobo_browser_core::cache::{blob_name, Index, MAX_BYTES, MAX_ENTRIES};
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeSet;

fuzz_target!(|data: &[u8]| {
    let index = Index::decode(data);
    assert!(index.entries().len() <= MAX_ENTRIES);
    assert!(index.bytes() <= MAX_BYTES);
    let mut names = BTreeSet::new();
    for entry in index.entries() {
        assert!(names.insert(&entry.name));
        assert_eq!(entry.name, blob_name(&entry.url));
    }
    assert_eq!(Index::decode(&index.encode()), index);
});
