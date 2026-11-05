//! MEASUREMENT-ONLY: host-asserted light colour scheme for prefers-color-scheme. Unadopted scratch.

use kobo_web_document::{MediaContext, measure, parse_style_tree_with_supply, style_diagnostics::RefusalReason as R, Limits, SupplyBudget};
fn page(media: &str) -> Vec<u8> {
    format!("<!doctype html><html><head><style media=\"{media}\">p{{margin-top:8px}}</style></head><body><p>t</p></body></html>").into_bytes()
}
fn run(media: &str, ctx: Option<MediaContext>) -> (bool, String) {
    measure::SHEET_HREF_CAP.store(0, std::sync::atomic::Ordering::Relaxed);
    let t = parse_style_tree_with_supply(&page(media), &[], &Limits::DEFAULT, ctx, SupplyBudget::CANONICAL);
    let mt = t.nodes.iter().find(|n| n.tag == "p").map(|n| format!("{:?}", n.style.margin_top)).unwrap();
    (t.unsupported, mt)
}
#[test]
fn default_context_still_refuses_prefers_color_scheme() {
    let c = MediaContext::new(1072);
    assert!(run("(prefers-color-scheme: dark)", c).0);
    assert!(run("(prefers-color-scheme: light)", c).0);
}
#[test]
fn light_context_skips_dark_and_applies_light() {
    let c = MediaContext::new(1072).map(MediaContext::with_light_scheme);
    assert_eq!(run("(prefers-color-scheme: dark)", c), (false, "Px(16)".into()));
    assert_eq!(run("(prefers-color-scheme: light)", c), (false, "Px(8)".into()));
    assert!(run("(prefers-color-scheme: no-preference)", c).0, "unknown value refuses");
    assert!(run("(prefers-color-scheme: dark), print", c).0, "comma stays refused");
}
#[test]
fn width_features_unchanged_and_combine() {
    let c = MediaContext::new(1072).map(MediaContext::with_light_scheme);
    assert_eq!(run("(min-width: 1000px) and (prefers-color-scheme: light)", c).1, "Px(8)");
    assert_eq!(run("(min-width: 2000px) and (prefers-color-scheme: light)", c).1, "Px(16)");
}
