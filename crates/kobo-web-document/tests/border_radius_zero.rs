//! PRIVATE, unadopted candidate: `border-radius: 0|0px` accepted only by the ext opt-in entry.
use kobo_web_document::{
    parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, parse_style_tree_with_media,
    parse_style_tree_with_pinned_font_family, Limits, MediaContext, SupplyBudget,
};
fn page(css: &str) -> String {
    format!("<!doctype html><style>{css}</style><div class=a><p>x</p></div>")
}
fn ext(css: &str) -> kobo_web_document::style_tree::StyleTree {
    parse_style_tree_with_inline_sheets_and_selectors(page(css).as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
}
fn dump(t: &kobo_web_document::style_tree::StyleTree) -> String {
    t.nodes.iter().map(|n| format!("{}:{:?}", n.tag, n.style)).collect::<Vec<_>>().join("|")
}
#[test]
fn zero_radius_changes_nothing_in_opt_in_entry() {
    let base = ext(".a{margin:3px}");
    assert!(!base.unsupported);
    for css in [".a{margin:3px;border-radius:0}", ".a{margin:3px;border-radius:0px}", "p{border-radius:0px}.a{margin:3px}",
        ".a{margin:3px;border-radius:0px !important}", "body{border-radius:0}.a{margin:3px}"] {
        let t = ext(css);
        assert!(!t.unsupported, "{css}");
        assert_eq!(dump(&t), dump(&base), "{css}");
    }
}
#[test]
fn nonzero_and_other_forms_stay_refused() {
    for css in [".a{border-radius:1px}", ".a{border-radius:0.5px}", ".a{border-radius:0 0 0 1px}", ".a{border-radius:0 / 0}",
        ".a{border-radius:50%}", ".a{border-radius:0%}", ".a{border-radius:inherit}", ".a{border-radius:}",
        ".a{border-radius:0em}", ".a{border-top-left-radius:0}", ".a{border-radius:0px 0px}",
        ".a{border-radius:0px;border-radius:4px}", ".a{border-radius:4px;border-radius:0px}",
        "html{--r:4px}.a{border-radius:var(--r)}", "html{--r:0px}.a{border-radius:var(--r);border-radius:2px}"] {
        assert!(ext(css).unsupported, "{css}");
    }
    let r = ext("html{--r:0px}.a{margin:3px;border-radius:var(--r)}");
    assert!(!r.unsupported);
    assert_eq!(dump(&r), dump(&ext(".a{margin:3px}")));
}
#[test]
fn other_entries_keep_refusing() {
    let h = page(".a{border-radius:0px}");
    assert!(parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported);
    assert!(parse_style_tree_with_pinned_font_family(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL).unsupported);
    assert!(parse_style_tree_with_media(h.as_bytes(), &[], &Limits::DEFAULT, MediaContext::new(785).unwrap()).unsupported);
}
