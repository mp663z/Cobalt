//! MEASUREMENT-ONLY prototype tests (isolated copy, unadopted). The narrow candidate lets only
//! link[href^="data:text/css"] exceed the 2048-byte attribute cap, bounded per href and per document.
use kobo_web_document::{measure, parse_style_tree_with_supply, style_diagnostics::RefusalReason as R, Limits, SupplyBudget};
use std::sync::{atomic::Ordering, Mutex};
static SERIAL: Mutex<()> = Mutex::new(()); // the toggles are process-global statics

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
fn css_of(len: usize) -> Vec<u8> { // valid, supported CSS padded with comments to ~len bytes
    let mut s = String::from("p{margin-top:8px}");
    while s.len() < len { s.push_str("/* pad pad pad pad pad pad pad pad */"); }
    s.into_bytes()
}
fn page(links: &[(String, Option<&str>)], extra: &str) -> Vec<u8> {
    let mut h = String::from("<!doctype html><html><head>");
    for (href, media) in links {
        h.push_str(&format!("<link rel=stylesheet href=\"{href}\"{}>", media.map(|m| format!(" media=\"{m}\"")).unwrap_or_default()));
    }
    h.push_str(&format!("</head><body><p>text</p>{extra}</body></html>"));
    h.into_bytes()
}
fn run(html: &[u8], href_cap: usize, agg: usize, sheet_max: usize, sheets: usize) -> (bool, Vec<(R, usize)>) {
    measure::EXEMPT_CAP.store(0, Ordering::Relaxed);
    measure::SHEET_HREF_CAP.store(href_cap, Ordering::Relaxed);
    measure::SHEET_AGG_CAP.store(agg, Ordering::Relaxed);
    let b = SupplyBudget { max_linked_sheets: sheets, max_sheet_bytes: sheet_max, max_aggregate_bytes: sheets * sheet_max };
    let t = parse_style_tree_with_supply(html, &[], &Limits::DEFAULT, None, b);
    measure::SHEET_HREF_CAP.store(0, Ordering::Relaxed);
    measure::SHEET_AGG_CAP.store(0, Ordering::Relaxed);
    (t.unsupported, R::ALL.iter().filter(|r| t.diagnostics.count(**r) > 0).map(|r| (*r, t.diagnostics.count(*r))).collect())
}
fn data(css: &[u8]) -> String { format!("data:text/css;base64,{}", b64(css)) }

#[test]
fn off_by_default_a_large_sheet_href_is_dropped_loudly() {
    let _g = SERIAL.lock().unwrap();
    let h = page(&[(data(&css_of(6000)), None)], "");
    let (unsupported, reasons) = run(&h, 0, 0, 262_144, 2);
    assert!(unsupported);
    assert!(reasons.contains(&(R::DroppedAttributes, 1)), "{reasons:?}");
}
#[test]
fn narrow_on_a_large_valid_sheet_is_retained_decoded_and_clean() {
    let _g = SERIAL.lock().unwrap();
    let h = page(&[(data(&css_of(6000)), None)], "");
    let (unsupported, reasons) = run(&h, 1 << 20, 1 << 21, 262_144, 2);
    assert!(!unsupported, "{reasons:?}");
    assert!(reasons.is_empty(), "{reasons:?}");
}
#[test]
fn narrow_does_not_exempt_other_data_urls() {
    let _g = SERIAL.lock().unwrap();
    let big = "x".repeat(3000);
    // a non-CSS data: link href and an image data: src stay over-cap drops
    let h = page(&[(format!("data:text/html;base64,{big}"), None)], &format!("<img src=\"data:image/png;base64,{big}\">"));
    let (_, reasons) = run(&h, 1 << 20, 1 << 21, 262_144, 2);
    assert!(reasons.contains(&(R::DroppedAttributes, 1)), "both are dropped as one loss record: {reasons:?}");
    let h2 = page(&[(data(&css_of(6000)), None)], &format!("<img src=\"data:image/png;base64,{big}\">"));
    let (_, r2) = run(&h2, 1 << 20, 1 << 21, 262_144, 2);
    assert!(r2.contains(&(R::DroppedAttributes, 1)), "image src still dropped even though the sheet is kept: {r2:?}");
}
#[test]
fn aggregate_cap_drops_the_excess_href_not_silently() {
    let _g = SERIAL.lock().unwrap();
    let one = data(&css_of(6000));
    let h = page(&[(one.clone(), None), (one.clone(), None)], "");
    let (unsupported, reasons) = run(&h, 1 << 20, one.len() + 10, 262_144, 8); // room for exactly one
    assert!(unsupported);
    assert!(reasons.contains(&(R::DroppedAttributes, 1)) && reasons.contains(&(R::MissingStylesheet, 1)), "{reasons:?}");
}
#[test]
fn per_href_cap_and_per_sheet_budget_refuse() {
    let _g = SERIAL.lock().unwrap();
    let h = page(&[(data(&css_of(6000)), None)], "");
    // href over the per-href cap: dropped
    let (u, r) = run(&h, 4000, 1 << 21, 262_144, 2);
    assert!(u && r.contains(&(R::DroppedAttributes, 1)), "{r:?}");
    // retained but decoded size over the per-sheet budget: scan-budget refusal
    let (u, r) = run(&h, 1 << 20, 1 << 21, 4000, 2);
    assert!(u && r.iter().any(|(k, _)| *k == R::CssScanBudget), "{r:?}");
}
#[test]
fn sheet_count_ceiling_refuses_the_extra_sheet() {
    let _g = SERIAL.lock().unwrap();
    let s = data(&css_of(3000));
    let h = page(&[(s.clone(), None), (s.clone(), None), (s.clone(), None)], "");
    let (u, r) = run(&h, 1 << 20, 1 << 22, 262_144, 2);
    assert!(u && r.contains(&(R::StylesheetCount, 1)), "{r:?}");
}
#[test]
fn bad_payloads_and_dark_media_refuse() {
    let _g = SERIAL.lock().unwrap();
    // invalid UTF-8 payload (0xFF) padded over the cap
    let mut bad = css_of(3000);
    bad.push(0xFF);
    let h = page(&[(data(&bad), None)], "");
    let (u, r) = run(&h, 1 << 20, 1 << 21, 262_144, 2);
    assert!(u && !r.is_empty(), "{r:?}");
    // invalid base64 length
    let h = page(&[(format!("data:text/css;base64,{}A", b64(&css_of(3000))), None)], "");
    let (u, r) = run(&h, 1 << 20, 1 << 21, 262_144, 2);
    assert!(u && !r.is_empty(), "{r:?}");
    // a dark-scheme sheet is refused (not applied, not silently skipped)
    let h = page(&[(data(&css_of(3000)), Some("(prefers-color-scheme: dark)"))], "");
    let (u, r) = run(&h, 1 << 20, 1 << 21, 262_144, 2);
    assert!(u && r.contains(&(R::StylesheetMedia, 1)), "{r:?}");
}

fn mt(html: &[u8]) -> (bool, String) {
    measure::EXEMPT_CAP.store(0, Ordering::Relaxed);
    measure::SHEET_HREF_CAP.store(1 << 20, Ordering::Relaxed);
    measure::SHEET_AGG_CAP.store(1 << 21, Ordering::Relaxed);
    let b = SupplyBudget { max_linked_sheets: 2, max_sheet_bytes: 262_144, max_aggregate_bytes: 2 * 262_144 };
    let t = parse_style_tree_with_supply(html, &[], &Limits::DEFAULT, None, b);
    measure::SHEET_HREF_CAP.store(0, Ordering::Relaxed);
    measure::SHEET_AGG_CAP.store(0, Ordering::Relaxed);
    let n = t.nodes.iter().find(|n| n.tag == "p");
    (t.unsupported, n.map(|n| format!("{:?}", n.style.margin_top)).unwrap_or_else(|| "NO_P".into()))
}
fn big(decl: &str) -> Vec<u8> { // >2048 B encoded so the cap exemption is exercised
    let mut s = format!("p{{{decl}}}");
    while s.len() < 4000 { s.push_str("/* pad pad pad pad pad pad pad pad */"); }
    s.into_bytes()
}
fn page2(order: &str, link: &str, style: &str) -> Vec<u8> {
    let l = format!("<link rel=stylesheet href=\"{link}\">");
    let st = format!("<style>{style}</style>");
    let head = if order == "ls" { format!("{l}{st}") } else { format!("{st}{l}") };
    format!("<!doctype html><html><head>{head}</head><body><p>text</p></body></html>").into_bytes()
}
#[test]
fn cascade_link_then_style_later_wins() {
    let _g = SERIAL.lock().unwrap();
    let r = mt(&page2("ls", &data(&big("margin-top:8px")), "p{margin-top:16px}"));
    assert_eq!(r, (false, "Px(16)".into()), "{r:?}");
}
#[test]
fn cascade_style_then_link_later_wins() {
    let _g = SERIAL.lock().unwrap();
    let r = mt(&page2("sl", &data(&big("margin-top:8px")), "p{margin-top:16px}"));
    assert_eq!(r, (false, "Px(8)".into()), "{r:?}");
}
#[test]
fn cascade_specificity_beats_order_and_important_beats_both() {
    let _g = SERIAL.lock().unwrap();
    let r = mt(&page2("sl", &data(&big("margin-top:8px")), "html p{margin-top:16px}"));
    assert_eq!(r, (false, "Px(16)".into()), "specificity {r:?}");
    let r = mt(&page2("ls", &data(&big("margin-top:8px!important")), "html p{margin-top:16px}"));
    assert_eq!(r, (false, "Px(8)".into()), "important {r:?}");
}
#[test]
fn link_then_style_document_order_both_retained_clean() {
    let _g = SERIAL.lock().unwrap();
    let mut h = page(&[(data(&css_of(6000)), None)], "");
    let s = String::from_utf8(h.clone()).unwrap().replace("</head>", "<style>p{margin-top:16px}</style></head>");
    h = s.into_bytes();
    let (unsupported, reasons) = run(&h, 1 << 20, 1 << 21, 262_144, 2);
    assert!(!unsupported, "{reasons:?}");
}
