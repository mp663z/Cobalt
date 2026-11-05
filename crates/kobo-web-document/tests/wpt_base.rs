//! `<base href>`: which one counts and what it does to the page's links.
//!
//! The cases follow the HTML standard's rules for the document base URL
//! (the first `base` element with an `href` counts, resolved against the
//! page's own address) and the behaviour WPT checks in
//! `html/semantics/document-metadata/the-base-element`.

use kobo_web_document::{parse_document, Limits, Url};

const PAGE: &str = "https://site.example/dir/page.html";

/// The address the page's one link resolves to.
fn link_target(head: &str) -> (Option<String>, Option<String>) {
    let html =
        format!("<html><head>{head}</head><body><a href=\"next.html\">next</a></body></html>");
    let url = Url::parse(PAGE).expect("url");
    let document = parse_document(html.as_bytes(), &url, &Limits::DEFAULT);
    (
        document.base.as_ref().map(ToString::to_string),
        document.links.first().map(|link| link.target.to_string()),
    )
}

#[test]
fn without_a_base_links_resolve_against_the_page() {
    assert_eq!(
        link_target("").1.as_deref(),
        Some("https://site.example/dir/next.html")
    );
}

#[test]
fn an_absolute_base_moves_every_link() {
    let (_, link) = link_target(r#"<base href="https://other.example/a/b/">"#);
    assert_eq!(link.as_deref(), Some("https://other.example/a/b/next.html"));
}

#[test]
fn a_relative_base_is_resolved_against_the_page_first() {
    let (_, link) = link_target(r#"<base href="../up/">"#);
    assert_eq!(link.as_deref(), Some("https://site.example/up/next.html"));
}

#[test]
fn only_the_first_base_with_an_href_counts() {
    let (_, link) =
        link_target(r#"<base target="_blank"><base href="/first/"><base href="/second/">"#);
    assert_eq!(
        link.as_deref(),
        Some("https://site.example/first/next.html")
    );
}

#[test]
fn a_base_the_browser_cannot_use_leaves_the_page_address() {
    for href in [
        "javascript:alert(1)",
        "data:text/html,x",
        "ftp://files.example/",
    ] {
        let (_, link) = link_target(&format!(r#"<base href="{href}">"#));
        assert_eq!(
            link.as_deref(),
            Some("https://site.example/dir/next.html"),
            "{href}"
        );
    }
}

#[test]
fn entities_in_a_base_href_are_decoded() {
    let (_, link) = link_target(r#"<base href="https://other.example/x?a=1&amp;b=2">"#);
    assert_eq!(link.as_deref(), Some("https://other.example/next.html"));
    let html = r#"<a href="/q?a=1&amp;b=2&copy=3">q</a>"#;
    let url = Url::parse(PAGE).expect("url");
    let document = parse_document(html.as_bytes(), &url, &Limits::DEFAULT);
    // `&copy` followed by `=` in an attribute is left alone, as the standard
    // says, so old query strings keep working.
    assert_eq!(
        document.links[0].target.to_string(),
        "https://site.example/q?a=1&b=2&copy=3"
    );
}

#[test]
fn a_base_in_the_body_still_counts() {
    let html = r#"<p><a href="next.html">next</a></p><base href="https://late.example/">"#;
    let url = Url::parse(PAGE).expect("url");
    let document = parse_document(html.as_bytes(), &url, &Limits::DEFAULT);
    assert_eq!(
        document.links[0].target.to_string(),
        "https://late.example/next.html"
    );
}
