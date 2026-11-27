//! Generated URL normalization and relative-reference invariants.
use kobo_web_document::Url;
use proptest::prelude::*;

fn invariant(url: &Url) {
    assert!(url.path().starts_with('/'));
    assert_eq!(Url::parse(&url.to_string()).as_ref(), Ok(url));
    let bare = url.without_fragment();
    assert!(bare.fragment().is_none());
    assert!(bare.same_document(url));
    assert_eq!(bare.without_fragment(), bare);
    assert_eq!(url.upgraded().upgraded(), url.upgraded());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn structured_urls_keep_their_normalized_meaning(
        secure in any::<bool>(), host in "[a-z]{1,12}\\.[a-z]{2,6}",
        port in any::<u16>(), path in ".{0,60}",query in ".{0,40}",fragment in ".{0,40}",
        reference in ".{0,80}"
    ) {
        let scheme=if secure {"https"} else {"http"};
        let input=format!("{scheme}://{host}:{port}/{path}?{query}#{fragment}");
        if let Ok(url)=Url::parse(&input) {
            invariant(&url);
            if let Ok(next)=url.join(&reference) { invariant(&next); }
        }
    }
}
