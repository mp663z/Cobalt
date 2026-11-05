//! Opt-in candidate (UNREVIEWED): oversize `link[href^=data:text/css]` retained per parse so the
//! engine's own strict decoder is the authority; default entries unchanged; all other drops unchanged.
use kobo_web_document::{
    parse_style_tree_with_inline_sheets as optin, parse_style_tree_with_supply as plain, style_diagnostics::RefusalReason as R,
    style_tree::StyleTree, Limits, SupplyBudget,
};

fn b64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}
/// Valid CSS padded with comments to about `len` decoded bytes.
fn css(len: usize, rule: &str) -> Vec<u8> {
    let mut s = String::from(rule);
    while s.len() < len {
        s.push(' ');
    }
    s.into_bytes()
}
fn href(len: usize, rule: &str) -> String {
    format!("data:text/css;base64,{}", b64(&css(len, rule)))
}
fn budget(max: usize, sheets: usize) -> SupplyBudget {
    SupplyBudget { max_linked_sheets: sheets, max_sheet_bytes: max, max_aggregate_bytes: sheets * max }
}
fn count(t: &StyleTree, r: R) -> usize {
    t.diagnostics.count(r)
}
fn page(head: &str) -> Vec<u8> {
    format!("<!doctype html><html><head>{head}</head><body><p>text</p></body></html>").into_bytes()
}
fn margin_top(t: &StyleTree) -> u32 {
    let n = t.nodes.iter().find(|n| n.tag == "p").unwrap();
    format!("{:?}", n.style.margin_top).chars().filter(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0)
}
fn reasons(t: &StyleTree) -> Vec<(R, usize)> {
    R::ALL.iter().filter(|r| count(t, **r) > 0).map(|r| (*r, count(t, *r))).collect()
}

#[test]
fn oversize_sheet_is_decoded_by_the_engine_only_when_opted_in() {
    let h = href(6000, "p{margin-top:7px}");
    assert!(h.len() > 2048);
    let html = page(&format!("<link rel=stylesheet href=\"{h}\">"));
    let b = budget(8192, 2);
    let off = plain(&html, &[], &Limits::DEFAULT, None, b);
    assert_eq!(count(&off, R::DroppedAttributes), 1, "default path unchanged: href dropped and counted");
    assert!(off.unsupported);
    let on = optin(&html, &[], &Limits::DEFAULT, None, b);
    assert_eq!(count(&on, R::DroppedAttributes), 0);
    assert!(!on.unsupported, "{:?}", reasons(&on));
    assert_eq!(margin_top(&on), 7, "engine decoded and applied the sheet");
}

#[test]
fn encoded_bound_is_exactly_four_ceil_third_plus_header_and_decoder_stays_authority() {
    let max = 3000usize;
    let cap = 4 * max.div_ceil(3) + 5 + 64 + 1;
    let b = budget(max, 2);
    // Exactly at the cap: retained (not dropped); invalid base64 length is refused by the DECODER.
    let at = format!("data:text/css;base64,{}", "A".repeat(cap - 21));
    assert_eq!(at.len(), cap);
    let t = optin(&page(&format!("<link rel=stylesheet href=\"{at}\">")), &[], &Limits::DEFAULT, None, b);
    assert_eq!(count(&t, R::DroppedAttributes), 0);
    assert!(t.unsupported, "decoder must still refuse invalid base64");
    // One byte over: dropped and counted, exactly as the default path.
    let over = format!("{at}A");
    let t = optin(&page(&format!("<link rel=stylesheet href=\"{over}\">")), &[], &Limits::DEFAULT, None, b);
    assert_eq!(count(&t, R::DroppedAttributes), 1);
    // Decoded size above the sheet budget but within the encoded cap: DECODER refuses it.
    let big = href(max + 30, "p{margin-top:3px}");
    assert!(big.len() <= cap);
    let t = optin(&page(&format!("<link rel=stylesheet href=\"{big}\">")), &[], &Limits::DEFAULT, None, b);
    assert_eq!(count(&t, R::DroppedAttributes), 0);
    assert!(t.unsupported);
    assert!(!reasons(&t).is_empty());
    eprintln!("decoded-over-budget reasons: {:?}", reasons(&t));
}

#[test]
fn other_attributes_and_elements_still_drop_and_count() {
    let h = href(6000, "p{margin-top:7px}");
    let b = budget(8192, 2);
    for head in [
        format!("<link rel=stylesheet src=\"{h}\">"),
        format!("<link rel=stylesheet data-x=\"{h}\">"),
        format!("<meta content=\"{h}\">"),
        format!("<link rel=stylesheet href=\"data:image/png;base64,{}\">", "A".repeat(6000)),
        format!("<link rel=stylesheet href=\"https://example.test/{}\">", "a".repeat(6000)),
    ] {
        let t = optin(&page(&head), &[], &Limits::DEFAULT, None, b);
        assert!(count(&t, R::DroppedAttributes) >= 1, "{}", &head[..40]);
    }
    let body = format!("<!doctype html><p>x</p><a href=\"{h}\">l</a><img src=\"{h}\">").into_bytes();
    let tb = optin(&body, &[], &Limits::DEFAULT, None, b);
    assert_eq!(count(&tb, R::DroppedAttributes), 1, "a[href] dropped; img[src] over cap also dropped but counted once per diagnostics record: {:?} attrs={}", reasons(&tb), tb.diagnostics.dropped_attributes);
    assert_eq!(tb.diagnostics.dropped_attributes, 2);
    // 33rd attribute on an otherwise exempt link is still dropped.
    let attrs: String = (0..32).map(|i| format!(" data-{i}=1")).collect();
    let t = optin(&page(&format!("<link{attrs} rel=stylesheet href=\"{h}\">")), &[], &Limits::DEFAULT, None, b);
    assert_eq!(t.diagnostics.dropped_attributes, 2, "rel and href are attributes 33 and 34: {:?}", reasons(&t));
}

#[test]
fn aggregate_is_per_parse_and_third_sheet_still_refuses() {
    let max = 3000;
    let b = budget(max, 2);
    let hs: Vec<String> = (0..3).map(|i| href(2500, &format!("p{{margin-top:{}px}}", i + 1))).collect();
    let head: String = hs.iter().map(|h| format!("<link rel=stylesheet href=\"{h}\">")).collect();
    let t = optin(&page(&head), &[], &Limits::DEFAULT, None, b);
    assert!(t.unsupported);
    assert!(count(&t, R::DroppedAttributes) >= 1, "third href exceeds the per-parse aggregate: {:?}", reasons(&t));
    // A second parse starts from zero (no process-global state).
    let t2 = optin(&page(&format!("<link rel=stylesheet href=\"{}\">", hs[0])), &[], &Limits::DEFAULT, None, b);
    assert_eq!(count(&t2, R::DroppedAttributes), 0);
    assert!(!t2.unsupported, "{:?}", reasons(&t2));
}

#[test]
fn html_entities_in_the_href_are_decoded_before_length_and_decoding() {
    let h = href(6000, "p{margin-top:9px}");
    let (first, rest) = h.split_at(h.len() / 2);
    let c = rest.chars().next().unwrap();
    let ent = format!("{first}&#x{:x};{}", c as u32, &rest[c.len_utf8()..]);
    let t = optin(&page(&format!("<link rel=stylesheet href=\"{ent}\">")), &[], &Limits::DEFAULT, None, budget(8192, 2));
    assert_eq!(count(&t, R::DroppedAttributes), 0);
    assert_eq!(margin_top(&t), 9);
}

#[test]
fn inactive_link_keeps_default_semantics() {
    let h = href(6000, "p{margin-top:7px}");
    let t = optin(&page(&format!("<link rel=icon href=\"{h}\">")), &[], &Limits::DEFAULT, None, budget(8192, 2));
    assert_eq!(count(&t, R::DroppedAttributes), 0, "retained, not lost");
    assert_eq!(margin_top(&t), 16, "not a stylesheet: the sheet is not applied");
    let small = page("<link rel=icon href=\"data:text/css;base64,cHt9\">");
    let a = plain(&small, &[], &Limits::DEFAULT, None, budget(8192, 2));
    let b = optin(&small, &[], &Limits::DEFAULT, None, budget(8192, 2));
    assert_eq!(format!("{:?}", reasons(&a)), format!("{:?}", reasons(&b)));
}

#[test]
fn inputs_without_oversize_data_hrefs_are_identical_to_the_default_entry() {
    let big_other = "x".repeat(5000);
    let pages: Vec<Vec<u8>> = vec![
        page("<style>p{margin-top:5px}</style>"),
        page("<link rel=stylesheet href=\"data:text/css;base64,cHttYXJnaW4tdG9wOjVweH0=\">"),
        page(&format!("<meta content=\"{big_other}\">")),
        page("<link rel=stylesheet href=\"https://example.test/a.css\">"),
        b"<!doctype html><p style='margin:3px'>a</p><div class=a></div>".to_vec(),
        page(""),
    ];
    for (i, p) in pages.iter().enumerate() {
        let a = plain(p, &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
        let b = optin(p, &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
        assert_eq!(a.unsupported, b.unsupported, "page {i}");
        assert_eq!(reasons(&a), reasons(&b), "page {i}");
        assert_eq!(format!("{:?}", a.nodes.iter().map(|n| (&n.tag, n.style.margin_top)).collect::<Vec<_>>()), format!("{:?}", b.nodes.iter().map(|n| (&n.tag, n.style.margin_top)).collect::<Vec<_>>()), "page {i}");
    }
}

const CANON_CAP: usize = 349_528 + 5 + 64 + 1; // 4*ceil(262144/3) + "data:" + header window + comma

#[test]
fn canonical_budget_derives_the_documented_cap_and_cap_plus_one_drops() {
    let b = SupplyBudget::CANONICAL;
    assert_eq!(CANON_CAP, 349_598);
    let at = format!("data:text/css;base64,{}", "A".repeat(CANON_CAP - 21));
    let t = optin(&page(&format!("<link rel=stylesheet href=\"{at}\">")), &[], &Limits::DEFAULT, None, b);
    assert_eq!(t.diagnostics.dropped_attributes, 0);
    assert_eq!(t.diagnostics.retained_inline_sheet_href_bytes, CANON_CAP);
    let t = optin(&page(&format!("<link rel=stylesheet href=\"{at}A\">")), &[], &Limits::DEFAULT, None, b);
    assert_eq!(t.diagnostics.dropped_attributes, 1);
    assert_eq!(t.diagnostics.retained_inline_sheet_href_bytes, 0);
    // Default entry never retains and reports 0.
    let d = plain(&page(&format!("<link rel=stylesheet href=\"{at}\">")), &[], &Limits::DEFAULT, None, b);
    assert_eq!((d.diagnostics.dropped_attributes, d.diagnostics.retained_inline_sheet_href_bytes), (1, 0));
}

#[test]
fn header_case_charset_media_and_decoder_refusals_are_the_decoders() {
    let b = budget(8192, 2);
    let body = b64(&css(6000, "p{margin-top:7px}"));
    let ok = |href: String, extra: &str| optin(&page(&format!("<link rel=stylesheet{extra} href=\"{href}\">")), &[], &Limits::DEFAULT, None, b);
    let t = ok(format!("DATA:TEXT/CSS;BASE64,{body}"), "");
    assert_eq!((t.diagnostics.dropped_attributes, t.unsupported, margin_top(&t)), (0, false, 7), "{:?}", reasons(&t));
    let t = ok(format!("data:text/css;charset=utf-8;base64,{body}"), "");
    assert_eq!((t.diagnostics.dropped_attributes, t.unsupported, margin_top(&t)), (0, false, 7), "{:?}", reasons(&t));
    // Non-css media type with the css prefix absent: not retained.
    let t = ok(format!("data:text/plain;base64,{body}"), "");
    assert_eq!(t.diagnostics.dropped_attributes, 1);
    // Retained, then refused by the decoder for other charset / URL-escaped / non-base64 forms.
    for href in [format!("data:text/css;charset=latin1;base64,{body}"), format!("data:text/css,{}", "p%7Bmargin-top:1px%7D".repeat(300))] {
        let t = ok(href, "");
        assert_eq!(t.diagnostics.dropped_attributes, 0);
        assert!(t.unsupported, "decoder refuses");
    }
    // Invalid UTF-8 inside valid base64 and invalid base64 characters: decoder refusals.
    let bad_utf8 = b64(&[0xff; 6000]);
    let t = ok(format!("data:text/css;base64,{bad_utf8}"), "");
    assert_eq!(t.diagnostics.dropped_attributes, 0);
    assert!(t.unsupported);
    let t = ok(format!("data:text/css;base64,{}!{}", &body[..3000], &body[3000..]), "");
    assert_eq!(t.diagnostics.dropped_attributes, 0);
    assert!(t.unsupported);
    // Media attribute unchanged semantics: non-matching media is not an applied sheet but is retained.
    let t = ok(format!("data:text/css;base64,{body}"), " media=\"print\"");
    assert_eq!(t.diagnostics.dropped_attributes, 0);
    assert_eq!(margin_top(&t), 16, "print media sheet not applied; {:?}", reasons(&t));
}

#[test]
fn optin_caps_win_over_the_measurement_globals_and_globals_stay_inert_by_default() {
    use kobo_web_document::measure;
    use std::sync::{atomic::Ordering, Mutex};
    static SERIAL: Mutex<()> = Mutex::new(());
    let _g = SERIAL.lock().unwrap();
    let h = href(6000, "p{margin-top:7px}");
    let html = page(&format!("<link rel=stylesheet href=\"{h}\">"));
    measure::SHEET_HREF_CAP.store(100, Ordering::Relaxed); // global cap too small for the href
    measure::SHEET_AGG_CAP.store(100, Ordering::Relaxed);
    let on = optin(&html, &[], &Limits::DEFAULT, None, budget(8192, 2));
    let off = plain(&html, &[], &Limits::DEFAULT, None, budget(8192, 2));
    measure::SHEET_HREF_CAP.store(0, Ordering::Relaxed);
    measure::SHEET_AGG_CAP.store(0, Ordering::Relaxed);
    assert_eq!(on.diagnostics.dropped_attributes, 0, "opt-in cap takes precedence");
    assert_eq!(off.diagnostics.dropped_attributes, 1, "global too small: dropped as before");
    // Globals alone (measurement path) still work exactly as before and report retained bytes.
    measure::SHEET_HREF_CAP.store(10_000, Ordering::Relaxed);
    measure::SHEET_AGG_CAP.store(20_000, Ordering::Relaxed);
    let g = plain(&html, &[], &Limits::DEFAULT, None, budget(8192, 2));
    measure::SHEET_HREF_CAP.store(0, Ordering::Relaxed);
    measure::SHEET_AGG_CAP.store(0, Ordering::Relaxed);
    assert_eq!(g.diagnostics.dropped_attributes, 0);
    assert_eq!(g.diagnostics.retained_inline_sheet_href_bytes, h.len());
}
