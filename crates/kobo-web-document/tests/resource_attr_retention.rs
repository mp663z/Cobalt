//! Barrier-B (bounded approval 2026-11-09, pins P1-P7): per-parse retention budget for
//! oversize resource-reference attribute values in the ext parse entries:
//! img[src], and link[href] NOT starting with data:text/css. Deferred classes
//! (a[href], meta[content], source[srcset], path[d], anything else) still drop
//! and taint. Default entry (0,0) is byte-identical to before. Values are
//! retained as plain strings only: never fetched, decoded, or painted.
use kobo_web_document::{
    parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors as ext,
    style_diagnostics::RefusalReason as R, style_tree::StyleTree, Limits, SupplyBudget,
};

fn parse(html: &str) -> StyleTree {
    ext(
        html.as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    )
}
fn dropped(t: &StyleTree) -> usize {
    t.diagnostics.dropped_attributes
}
fn retained(t: &StyleTree) -> usize {
    t.diagnostics.retained_resource_attr_bytes
}
fn reasons(t: &StyleTree) -> Vec<(R, usize)> {
    R::ALL
        .iter()
        .filter(|r| t.diagnostics.count(**r) > 0)
        .map(|r| (*r, t.diagnostics.count(*r)))
        .collect()
}
fn img_margin_top(t: &StyleTree) -> u32 {
    let n = t.nodes.iter().find(|n| n.tag == "img").unwrap();
    format!("{:?}", n.style.margin_top)
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}
fn big(ch: char, len: usize) -> String {
    ch.to_string().repeat(len)
}

#[test]
fn img_src_retained_in_full_under_budget() {
    let v = big('A', 3000);
    let t = parse(&format!(
        "<!doctype html><p>x</p><img src=\"data:image/png;base64,{v}\">"
    ));
    assert_eq!((dropped(&t), retained(&t)), (0, 3022), "{:?}", reasons(&t));
    // A value within the ordinary 2048 cap needs no budget and uses none.
    let v = big('A', 2000);
    let t = parse(&format!(
        "<!doctype html><p>x</p><img src=\"data:image/png;base64,{v}\">"
    ));
    assert_eq!((dropped(&t), retained(&t)), (0, 0), "{:?}", reasons(&t));
}

#[test]
fn per_value_cap_is_exact() {
    // "data:image/png;base64," is 22 bytes; total value length hits the cap exactly.
    let v = big('A', 262_144 - 22);
    let t = parse(&format!(
        "<!doctype html><p>x</p><img src=\"data:image/png;base64,{v}\">"
    ));
    assert_eq!(
        (dropped(&t), retained(&t)),
        (0, 262_144),
        "{:?}",
        reasons(&t)
    );
    let v = big('A', 262_145 - 22);
    let t = parse(&format!(
        "<!doctype html><p>x</p><img src=\"data:image/png;base64,{v}\">"
    ));
    assert_eq!((dropped(&t), retained(&t)), (1, 0), "{:?}", reasons(&t));
    assert_eq!(t.diagnostics.count(R::DroppedAttributes), 1);
}

#[test]
fn aggregate_cap_is_saturating_and_first_come() {
    let v = big('A', 200_000);
    let html = format!(
        "<!doctype html><p>x</p><img src=\"data:image/png;base64,{v}\"><img src=\"data:image/gif;base64,{v}\"><img src=\"data:image/webp;base64,{v}\">"
    );
    let t = parse(&html);
    assert_eq!(
        (dropped(&t), retained(&t)),
        (1, 400_044),
        "{:?}",
        reasons(&t)
    );
}

#[test]
fn link_href_non_stylesheet_data_retained() {
    let v = big('A', 2766);
    let t = parse(&format!(
        "<!doctype html><link rel=icon href=\"data:image/svg+xml;base64,{v}\"><p>x</p>"
    ));
    assert_eq!((dropped(&t), retained(&t)), (0, 2792), "{:?}", reasons(&t));
}

#[test]
fn data_text_css_href_never_uses_the_resource_budget() {
    // In-sheet-cap data:text/css: routed to the narrow stylesheet path only.
    let v = big('A', 3000);
    let t = parse(&format!(
        "<!doctype html><link rel=stylesheet href=\"data:text/css;base64,{v}\"><p>x</p>"
    ));
    assert_eq!((dropped(&t), retained(&t)), (0, 0), "{:?}", reasons(&t));
    assert_eq!(t.diagnostics.retained_inline_sheet_href_bytes, 3000 + 21);
    // Same with shouting case: the prefix test is ASCII case-insensitive.
    let t = parse(&format!(
        "<!doctype html><link rel=stylesheet href=\"DATA:TEXT/CSS;base64,{v}\"><p>x</p>"
    ));
    assert_eq!((dropped(&t), retained(&t)), (0, 0), "{:?}", reasons(&t));
    // Over the sheet cap: still drops and taints; the resource budget never admits it.
    let v = big('A', 400_000);
    let t = parse(&format!(
        "<!doctype html><link rel=stylesheet href=\"data:text/css;base64,{v}\"><p>x</p>"
    ));
    assert_eq!((dropped(&t), retained(&t)), (1, 0), "{:?}", reasons(&t));
    assert_eq!(t.diagnostics.retained_inline_sheet_href_bytes, 0);
}

#[test]
fn deferred_classes_still_drop_and_taint() {
    let v = big('A', 3000);
    for html in [
        format!("<!doctype html><p>x</p><a href=\"data:text/html;base64,{v}\">l</a>"),
        format!("<!doctype html><meta content=\"{v}\"><p>x</p>"),
        format!("<!doctype html><p>x</p><source srcset=\"data:image/png;base64,{v}\">"),
        format!("<!doctype html><p>x</p><svg><path d=\"{v}\"></path></svg>"),
        format!("<!doctype html><p>x</p><div data-x=\"{v}\"></div>"),
    ] {
        let t = parse(&html);
        assert_eq!(
            (dropped(&t), retained(&t)),
            (1, 0),
            "{}: {:?}",
            &html[..60],
            reasons(&t)
        );
    }
}

#[test]
fn count_cap_still_applies_to_a_budget_eligible_value() {
    // 32 filler attributes first; img[src] is the 33rd and must drop even though
    // the value itself is within the resource budget (pin P1: count cap always).
    let attrs: String = (0..32).map(|i| format!(" data-{i}=1")).collect();
    let v = big('A', 3000);
    let t = parse(&format!(
        "<!doctype html><p>x</p><img{attrs} src=\"data:image/png;base64,{v}\">"
    ));
    assert_eq!((dropped(&t), retained(&t)), (1, 0), "{:?}", reasons(&t));
}

#[test]
fn duplicate_attribute_keeps_the_first_without_dropping() {
    let v = big('A', 3000);
    let t = parse(&format!(
        "<!doctype html><p>x</p><img src=\"data:,first\" src=\"data:image/png;base64,{v}\">"
    ));
    assert_eq!((dropped(&t), retained(&t)), (0, 0), "{:?}", reasons(&t));
}

#[test]
fn default_entry_still_drops_everything_oversize() {
    let v = big('A', 3000);
    for html in [
        format!("<!doctype html><p>x</p><img src=\"data:image/png;base64,{v}\">"),
        format!("<!doctype html><link rel=icon href=\"data:image/svg+xml;base64,{v}\"><p>x</p>"),
    ] {
        let t = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
        assert_eq!(
            (dropped(&t), retained(&t)),
            (1, 0),
            "default entry unchanged: {:?}",
            reasons(&t)
        );
    }
}

#[test]
fn retained_value_is_visible_to_attribute_selectors() {
    // Pin P6 unit half: a retained >2048B value matches [src^=...] exactly like a
    // short value; the negative control does not match. Computed-style level only.
    let v = big('A', 3000);
    let t = parse(&format!(
        "<!doctype html><style>img[src^=\"data:image\"]{{margin-top:9px}}</style><p>x</p><img src=\"data:image/png;base64,{v}\">"
    ));
    assert_eq!(
        (dropped(&t), img_margin_top(&t)),
        (0, 9),
        "{:?}",
        reasons(&t)
    );
    let t = parse(&format!(
        "<!doctype html><style>img[src$=\"zzz\"]{{margin-top:9px}}</style><p>x</p><img src=\"data:image/png;base64,{v}\">"
    ));
    assert_eq!(img_margin_top(&t), 0, "negative control must not match");
}
