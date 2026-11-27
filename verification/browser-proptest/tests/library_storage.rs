//! Corrupt library storage must remain bounded and canonical once decoded.
use kobo_browser_core::library::{Library, MAX_BOOKMARKS, MAX_RECENT};
use proptest::prelude::*;

fn invariant(bytes: &[u8]) {
    let library = Library::decode(bytes);
    assert!(library.bookmarks.len() <= MAX_BOOKMARKS);
    assert!(library.recent.len() <= MAX_RECENT);
    for place in library.bookmarks.iter().chain(&library.recent) {
        assert!(place.title.len() <= 200);
        assert!(place.url.to_string().len() <= 1000);
    }
    assert_eq!(Library::decode(&library.encode()), library);
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn arbitrary_library_bytes_are_bounded(bytes in prop::collection::vec(any::<u8>(),0..4096)) { invariant(&bytes); }
    #[test]
    fn structured_library_rows_remain_bounded(
        rows in prop::collection::vec((prop_oneof![Just(b'B'),Just(b'R')],0u16..140,".{0,50}"),0..240),
        truncate in any::<bool>(),
    ) {
        let mut bytes=b"kobo-browse-library 1\n".to_vec();
        for key in 0..110 {
            let url=format!("https://library-storage.invalid/prefix/{key}");
            bytes.push(b'B');
            bytes.extend_from_slice(&(url.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.extend_from_slice(url.as_bytes());
        }
        for (kind,key,title) in rows {
            let url=format!("https://library-storage.invalid/{key}");
            bytes.push(kind);
            bytes.extend_from_slice(&(url.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&(title.len() as u32).to_le_bytes());
            bytes.extend_from_slice(url.as_bytes());
            bytes.extend_from_slice(title.as_bytes());
        }
        if truncate { bytes.pop(); }
        invariant(&bytes);
    }
}
