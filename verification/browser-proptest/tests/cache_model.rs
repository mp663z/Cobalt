//! Independent cache-policy model. It stores addresses directly and uses an
//! explicit sequence number, not production blob names, selection or encoding.
use kobo_browser_core::cache::{blob_name, Index, MAX_BYTES, MAX_ENTRIES};
use proptest::prelude::*;

#[derive(Clone, Debug, PartialEq)]
struct Page {
    key: u16,
    bytes: u64,
    fetched: u64,
    read: u64,
}

#[derive(Default)]
struct Shelf {
    pages: Vec<Page>,
    reads: u64,
}

impl Shelf {
    fn insert(&mut self, key: u16, bytes: u64, fetched: u64) -> Option<Vec<u16>> {
        if bytes > MAX_BYTES {
            return None;
        }
        self.pages.retain(|p| p.key != key);
        self.reads = self.reads.saturating_add(1);
        self.pages.push(Page {
            key,
            bytes,
            fetched,
            read: self.reads,
        });
        let mut removed = vec![];
        loop {
            let size: u64 = self.pages.iter().map(|p| p.bytes).sum();
            if self.pages.len() <= MAX_ENTRIES && size <= MAX_BYTES {
                break;
            }
            // Choose by sorting candidates, independently of Index's min scan.
            let mut candidates: Vec<_> = self
                .pages
                .iter()
                .filter(|p| p.key != key)
                .cloned()
                .collect();
            candidates.sort_by_key(|p| p.read);
            let victim = candidates.first().expect("new page fits on its own").key;
            self.pages.retain(|p| p.key != victim);
            removed.push(victim);
        }
        Some(removed)
    }
    fn touch(&mut self, key: u16) {
        self.reads = self.reads.saturating_add(1);
        for p in &mut self.pages {
            if p.key == key {
                p.read = self.reads;
            }
        }
    }
}

fn url(key: u16) -> String {
    format!("https://cache-model.invalid/{key}")
}

fn check(actions: &[(u8, u16, u64, u64)]) {
    let mut actual = Index::default();
    let mut expected = Shelf::default();
    for &(action, key, bytes, time) in actions {
        let address = url(key);
        match action {
            0 | 1 => {
                let result = actual.insert(&address, bytes, time);
                let modeled = expected.insert(key, bytes, time);
                assert_eq!(
                    result.map(|(name, evicted)| {
                        assert_eq!(name, blob_name(&address));
                        evicted
                    }),
                    modeled.map(|keys| keys.into_iter().map(|k| blob_name(&url(k))).collect())
                );
            }
            2 => {
                actual.touch(&address);
                expected.touch(key);
            }
            3 => {
                actual.remove(&blob_name(&address));
                expected.pages.retain(|p| p.key != key);
            }
            _ => {
                actual = Index::decode(&actual.encode());
            }
        }
        let rows: Vec<_> = actual
            .entries()
            .iter()
            .map(|e| {
                let key = e.url.rsplit('/').next().unwrap().parse().unwrap();
                assert_eq!(e.name, blob_name(&url(key)));
                Page {
                    key,
                    bytes: e.bytes,
                    fetched: e.fetched,
                    read: e.used,
                }
            })
            .collect();
        assert_eq!(rows, expected.pages);
        assert_eq!(actual.bytes(), expected.pages.iter().map(|p| p.bytes).sum());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn eviction_and_recency_match_independent_policy(
        actions in prop::collection::vec((0u8..5, 0u16..24,
            prop_oneof![Just(0u64), Just(MAX_BYTES), Just(MAX_BYTES+1), 0u64..(MAX_BYTES/4+1)], any::<u64>()), 0..400)
    ) { check(&actions); }

    #[test]
    fn entry_limit_and_recency_match_independent_policy(
        prefix in 201u16..260,
        suffix in prop::collection::vec((0u8..5, 0u16..280, 0u64..100, any::<u64>()), 0..100)
    ) {
        let mut actions: Vec<_> = (0..prefix).map(|key| (0, key, 1, u64::from(key))).collect();
        actions.extend(suffix);
        check(&actions);
    }
}
