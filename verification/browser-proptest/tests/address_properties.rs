//! Typed-address and query transport properties over actual browser core.
use kobo_browser_core::address::{encode_query, resolve, Address, DEFAULT_SEARCH};
use kobo_web_document::Url;
use proptest::prelude::*;

// Independent form decoder checks the exact original UTF-8 byte stream.
fn decode_form(encoded: &str) -> Vec<u8> {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'+' => {
                decoded.push(b' ');
                at += 1;
            }
            b'%' => {
                let pair = std::str::from_utf8(&bytes[at + 1..at + 3]).unwrap();
                decoded.push(u8::from_str_radix(pair, 16).unwrap());
                at += 3;
            }
            byte => {
                assert!(byte.is_ascii_alphanumeric() || b"-._~".contains(&byte));
                decoded.push(byte);
                at += 1;
            }
        }
    }
    decoded
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn form_query_preserves_arbitrary_unicode(query in ".{0,300}") {
        assert_eq!(decode_form(&encode_query(&query)),query.as_bytes());
    }
    #[test]
    fn resolved_addresses_are_stable_and_search_keeps_query(typed in ".{0,200}") {
        match resolve(&typed,DEFAULT_SEARCH) {
            None => assert!(typed.trim().is_empty()),
            Some(address) => {
                let rendered=address.url().to_string();
                assert_eq!(&Url::parse(&rendered).unwrap(),address.url());
                if let Address::Search { query,url }=address {
                    assert_eq!(query,typed.trim());
                    let rendered=url.to_string();
                    let encoded=rendered.strip_prefix(DEFAULT_SEARCH).unwrap();
                    assert_eq!(decode_form(encoded),query.as_bytes());
                }
            }
        }
    }
}
