//! `host_stylesheet_links` census cross-checked against the parse's own host-supply handshake
//! (`host_sheets_claimed`). Synthetic fixtures only. An optional env-gated check reads a private page
//! the caller names; it is skipped when unset.
use kobo_web_document::{
    host_stylesheet_links, parse_style_tree_with_media, style_diagnostics::RefusalReason, Limits,
    MediaContext, Url,
};

fn page() -> Url {
    Url::parse("https://example.test/a/b/index.html").unwrap()
}
fn media() -> MediaContext {
    MediaContext::new(1072).unwrap()
}
fn census(html: &str) -> kobo_web_document::style_tree::HostSheetLinks {
    host_stylesheet_links(html.as_bytes(), &page(), &Limits::DEFAULT, Some(media()))
}
fn urls(html: &str) -> Vec<Option<String>> {
    census(html).links.iter().map(|u| u.as_ref().map(|u| u.to_string())).collect()
}
/// Supplying exactly the census count satisfies the handshake; one fewer is a missing-sheet refusal.
fn handshake(html: &str) {
    let c = census(html);
    let n = c.links.len();
    let full = vec![b"p{margin:0}".to_vec(); n];
    let t = parse_style_tree_with_media(html.as_bytes(), &full, &Limits::DEFAULT, media());
    assert_eq!(t.diagnostics.count(RefusalReason::MissingStylesheet), 0);
    // Over the two-sheet ceiling the parse refuses (claim withheld); this census reports the count, nothing more.
    if n + c.data_links > 2 {
        assert!(t.diagnostics.count(RefusalReason::StylesheetCount) > 0, "{html}");
        return;
    }
    assert_eq!(t.host_sheets_claimed, Some(n), "claimed vs census for {html}");
    if n > 0 {
        let short = vec![b"p{margin:0}".to_vec(); n - 1];
        let t = parse_style_tree_with_media(html.as_bytes(), &short, &Limits::DEFAULT, media());
        assert!(t.diagnostics.count(RefusalReason::MissingStylesheet) > 0);
    }
}

#[test]
fn order_resolution_and_base() {
    let h = "<!doctype html><link rel=stylesheet href=one.css><p>x</p><link rel='ICON stylesheet' href='/two.css'><link rel=stylesheet href=https://cdn.test/three.css>";
    assert_eq!(
        urls(h),
        vec![
            Some("https://example.test/a/b/one.css".into()),
            Some("https://example.test/two.css".into()),
            Some("https://cdn.test/three.css".into()),
        ]
    );
    handshake(h);
    let b = "<!doctype html><base href=\"https://other.test/x/\"><link rel=stylesheet href=s.css>";
    assert_eq!(urls(b), vec![Some("https://other.test/x/s.css".into())]);
    handshake(b);
}

#[test]
fn skipped_and_special_links() {
    for h in [
        "<!doctype html><link rel=preload href=a.css><link rel=stylesheet disabled href=a.css>",
        "<!doctype html><link rel=stylesheet media=print href=a.css>",
        "<!doctype html><style>p{}</style><p>x</p>",
        "<!doctype html><link rel=stylesheet href=\"data:text/css;base64,cHttYXJnaW46MH0=\"><link rel=stylesheet href=a.css>",
    ] {
        handshake(h);
    }
    let d = census("<!doctype html><link rel=stylesheet href=\"data:text/css;base64,cHttYXJnaW46MH0=\"><link rel=stylesheet href=a.css>");
    assert_eq!((d.links.len(), d.data_links), (1, 1));
    assert_eq!(census("<!doctype html><link rel=stylesheet media=print href=a.css>").links.len(), 0);
}

#[test]
fn hrefless_and_non_http_links_take_a_slot_but_are_unfetchable() {
    let h = "<!doctype html><link rel=stylesheet><link rel=stylesheet href=\"ftp://x.test/a.css\"><link rel=stylesheet href=b.css>";
    let u = urls(h);
    assert_eq!(u.len(), 3);
    assert_eq!(u[0], None);
    assert_eq!(u[1], None);
    assert_eq!(u[2], Some("https://example.test/a/b/b.css".into()));
    handshake(h);
}

#[test]
fn unknown_media_is_reported_and_incomplete_is_flagged() {
    let c = census("<!doctype html><link rel=stylesheet media=\"(prefers-color-scheme: dark)\" href=d.css><link rel=stylesheet href=a.css>");
    assert_eq!(c.unknown_media, 1);
    assert_eq!(c.links.len(), 1);
    assert!(c.complete);
    let tiny = Limits { max_input_bytes: 20, ..Limits::DEFAULT };
    let cut = host_stylesheet_links(b"<!doctype html><link rel=stylesheet href=a.css>", &page(), &tiny, Some(media()));
    assert!(!cut.complete);
}

#[test]
fn optional_private_page_cross_check() {
    let Ok(path) = std::env::var("HOST_LINKS_PAGE") else { return };
    let bytes = std::fs::read(path).unwrap();
    let c = host_stylesheet_links(&bytes, &page(), &Limits::DEFAULT, Some(media()));
    let sheets = vec![b"p{margin:0}".to_vec(); c.links.len()];
    let t = parse_style_tree_with_media(&bytes, &sheets, &Limits::DEFAULT, media());
    eprintln!("census links={} data={} unknown={} complete={} claimed={:?}", c.links.len(), c.data_links, c.unknown_media, c.complete, t.host_sheets_claimed);
    if c.links.len() + c.data_links > 2 {
        assert!(t.diagnostics.count(RefusalReason::StylesheetCount) > 0);
    } else {
        assert_eq!(t.host_sheets_claimed, Some(c.links.len()));
    }
}
