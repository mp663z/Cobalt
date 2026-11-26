//! Cache invariants over actual Index, including arbitrary stored bytes.
use kobo_browser_core::cache::{blob_name, Index, MAX_BYTES, MAX_ENTRIES};
use proptest::prelude::*;
use std::collections::BTreeSet;

fn invariant(index: &Index) {
    assert!(index.entries().len() <= MAX_ENTRIES);
    assert!(index.bytes() <= MAX_BYTES);
    let names: BTreeSet<_> = index.entries().iter().map(|e| &e.name).collect();
    assert_eq!(names.len(), index.entries().len());
    for entry in index.entries() {
        assert_eq!(entry.name, blob_name(&entry.url));
    }
    assert_eq!(&Index::decode(&index.encode()), index);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn arbitrary_stored_bytes_decode_within_limits(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        invariant(&Index::decode(&bytes));
    }

    #[test]
    fn random_cache_actions_preserve_budget_and_roundtrip(
        actions in prop::collection::vec((0u8..4, 0u16..240, 0u64..(MAX_BYTES+2), any::<u64>()), 0..100)
    ) {
        let mut index=Index::default();
        for (action,key,bytes,time) in actions {
            let url=format!("https://cache.invalid/{key}");
            match action {
                0 | 1 => {
                    let before=index.clone();
                    match index.insert(&url,bytes,time) {
                        None => assert_eq!(index,before),
                        Some((name,evicted)) => {
                            assert_eq!(name,blob_name(&url));
                            assert!(index.find(&url).is_some());
                            for name in evicted { assert!(!index.entries().iter().any(|e| e.name==name)); }
                        }
                    }
                }
                2 => index.touch(&url),
                _ => index.remove(&blob_name(&url)),
            }
            invariant(&index);
        }
    }
}
