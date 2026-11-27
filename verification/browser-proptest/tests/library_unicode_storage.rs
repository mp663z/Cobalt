//! Stored URL text may expand when parsed, but must persist once accepted.
use kobo_browser_core::library::Library;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn stored_unicode_address_roundtrips(count in 1usize..330) {
        let url=format!("https://library-storage.invalid/{}","é".repeat(count));
        let mut bytes=b"kobo-browse-library 1\nB".to_vec();
        bytes.extend_from_slice(&(url.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(url.as_bytes());
        let library=Library::decode(&bytes);
        assert_eq!(Library::decode(&library.encode()),library);
    }
}
