//! `link_takes_host_slot` lines up one entry per enabled stylesheet link with the links the host must supply,
//! and the linked-sheet prefilter folds an off-canvas fixed drawer to display:none.
use kobo_web_document::basic_page::prefilter_linked_sheet_for_basic;
use kobo_web_document::style_tree::HostSheetLinks;
use kobo_web_document::{host_stylesheet_links, Limits, MediaContext, Url};

fn census(html: &str) -> HostSheetLinks {
    let page = Url::parse("https://example.test/p.html").unwrap();
    let media = MediaContext::new(800).map(MediaContext::with_light_scheme);
    host_stylesheet_links(html.as_bytes(), &page, &Limits::DEFAULT, media)
}

#[test]
fn dark_scheme_link_takes_no_host_slot_under_light_profile() {
    let c = census(
        "<link rel=stylesheet href=a.css><link rel=stylesheet media='(prefers-color-scheme: dark)' href=b.css>\
         <link rel=stylesheet href=c.css><link rel=icon href=x.png>",
    );
    assert_eq!(c.link_takes_host_slot, vec![true, false, true]);
    assert_eq!(c.links.len(), 2);
    assert_eq!(c.unknown_media, 0);
}

#[test]
fn dark_scheme_link_is_unknown_without_light_profile() {
    let page = Url::parse("https://example.test/p.html").unwrap();
    let c = host_stylesheet_links(
        b"<link rel=stylesheet media='(prefers-color-scheme: dark)' href=b.css>",
        &page,
        &Limits::DEFAULT,
        MediaContext::new(800),
    );
    assert_eq!(c.unknown_media, 1);
    assert_eq!(c.link_takes_host_slot, vec![false]);
}

#[test]
fn linked_sheet_prefilter_folds_offcanvas_drawer() {
    let html = b"<html><body><div class=drawer>x</div><p>y</p></body></html>";
    let css = b".drawer{position:fixed;transform:translateX(calc(-100% - 10px))} p{color:red}";
    let (out, stats) = prefilter_linked_sheet_for_basic(html, css);
    assert_eq!(stats.offcanvas_static, 1);
    assert!(String::from_utf8(out).unwrap().contains("display:none"));
}

#[test]
fn prefilter_keeps_rules_for_unquoted_spaced_and_uppercase_attributes() {
    let html = b"<html><body><div class=a1 ID = b2><span CLASS = 'c3 d4'>x</span></div></body></html>";
    for sel in [".a1", "#b2", ".c3", ".d4"] {
        let css = format!("{sel}{{color:red}}");
        let (_, stats) = prefilter_linked_sheet_for_basic(html, css.as_bytes());
        assert_eq!((sel, stats.rules_dropped), (sel, 0));
    }
    let (_, stats) = prefilter_linked_sheet_for_basic(html, b".absent{color:red}");
    assert_eq!(stats.rules_dropped, 1);
}
