//! PRIVATE, unadopted candidate: `pointer-events:none` and `user-select:none` accepted as inert
//! declarations ONLY by the extended-CSS opt-in entry. Whole computed style must equal the page without them.
use kobo_web_document::{
    parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, parse_style_tree_with_media,
    parse_style_tree_with_pinned_font_family, Limits, MediaContext, SupplyBudget,
};
fn page(css: &str) -> String { format!("<!doctype html><style>{css}</style><div class=a><p>x</p></div>") }
fn ext(css: &str) -> kobo_web_document::style_tree::StyleTree {
    parse_style_tree_with_inline_sheets_and_selectors(page(css).as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
}
fn dump(t: &kobo_web_document::style_tree::StyleTree) -> String {
    t.nodes.iter().map(|n| format!("{}:{:?}", n.tag, n.style)).collect::<Vec<_>>().join("|")
}
#[test]
fn none_values_change_nothing_in_the_opt_in_entry() {
    let base = ext(".a{margin:3px}");
    assert!(!base.unsupported);
    for css in [
        ".a{margin:3px;pointer-events:none}", ".a{user-select:none;margin:3px}", "body{pointer-events:none;user-select:none}.a{margin:3px}",
        ".a{margin:3px;POINTER-EVENTS: NONE}", ".a{margin:3px;user-select:none !important}",
    ] {
        let t = ext(css);
        assert!(!t.unsupported, "{css}");
        assert_eq!(dump(&t), dump(&base), "{css}");
    }
}
#[test]
fn every_other_value_and_alias_stays_refused() {
    for css in [
        ".a{pointer-events:auto}", ".a{pointer-events:all}", ".a{pointer-events:inherit}", ".a{pointer-events:}", ".a{pointer-events:none none}",
        ".a{user-select:auto}", ".a{user-select:text}", ".a{user-select:all}", ".a{user-select:contain}", ".a{user-select:inherit}",
        ".a{-webkit-user-select:none}", ".a{-moz-user-select:none}", ".a{pointer-events:none;pointer-events:auto}", ".a{user-select:none;user-select:text}",
        ".a{pointer-events:var(--x)}", // undefined var, no fallback: refused (masks nothing about the skip gate)
        ":root{--x:auto}.a{pointer-events:var(--x)}", ":root{--x:text}.a{user-select:var(--x)}",
        ":root{--x:auto}.a{user-select:var(--x)}", ":root{--x:none}.a{pointer-events:var(--x);pointer-events:auto}",
    ] {
        assert!(ext(css).unsupported, "{css}");
    }
}
#[test]
fn other_entries_keep_refusing() {
    for css in [".a{pointer-events:none}", ".a{user-select:none}"] {
        let h = page(css);
        assert!(parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported);
        assert!(parse_style_tree_with_pinned_font_family(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL).unsupported);
        assert!(parse_style_tree_with_media(h.as_bytes(), &[], &Limits::DEFAULT, MediaContext::new(785).unwrap()).unsupported);
    }
}

#[test]
fn resolved_none_through_var_is_admitted_on_ordinary_elements() {
    // Ordinary-element var() substitution runs before the property skip, so a DEFINED --x:none is admitted
    // for both properties (same as the literal). Resolved auto/text are refused (negatives above).
    let base = ext(".a{margin:3px}");
    for css in [
        ":root{--x:none}.a{margin:3px;pointer-events:var(--x)}",
        ":root{--x:none}.a{margin:3px;user-select:var(--x)}",
        ".a{--x:none;margin:3px;pointer-events:var(--x)}",
        ".a{margin:3px;pointer-events:var(--x,none)}",
    ] {
        let t = ext(css);
        assert!(!t.unsupported, "{css}");
        assert_eq!(dump(&t), dump(&base), "{css}");
    }
}
