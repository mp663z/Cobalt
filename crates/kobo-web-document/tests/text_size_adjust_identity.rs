//! PRIVATE, unadopted candidate: `text-size-adjust: 100%|none` accepted as an identity declaration
//! ONLY by the extended-CSS opt-in entry. Whole computed style must equal the page without it.
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
fn identity_values_change_nothing_in_the_opt_in_entry() {
    let base = ext(".a{margin:3px}");
    assert!(!base.unsupported);
    for css in [
        ".a{margin:3px;text-size-adjust:100%}",
        ".a{text-size-adjust:none;margin:3px}",
        "html{text-size-adjust:100%}.a{margin:3px}",
        ".a{margin:3px;text-size-adjust:100% !important}",
        ".a{margin:3px;TEXT-SIZE-ADJUST: 100%}",
    ] {
        let t = ext(css);
        assert!(!t.unsupported, "{css}");
        assert_eq!(dump(&t), dump(&base), "{css}");
    }
}

#[test]
fn other_values_and_spellings_stay_refused() {
    for css in [
        ".a{text-size-adjust:auto}",
        ".a{text-size-adjust:150%}",
        ".a{text-size-adjust:100.0%}",
        ".a{text-size-adjust:99%}",
        ".a{text-size-adjust:100}",
        ".a{text-size-adjust:inherit}",
        ".a{text-size-adjust:}",
        ".a{-webkit-text-size-adjust:100%}",
        ".a{text-size-adjust:100% 100%}",
    ] {
        assert!(ext(css).unsupported, "{css}");
    }
}

#[test]
fn other_entries_keep_refusing() {
    let h = page(".a{text-size-adjust:100%}");
    assert!(parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported);
    assert!(parse_style_tree_with_pinned_font_family(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL).unsupported);
    assert!(parse_style_tree_with_media(h.as_bytes(), &[], &Limits::DEFAULT, MediaContext::new(785).unwrap()).unsupported);
}

#[test]
fn cascade_paths_conflict_inheritance_var() {
    // Inheritance/ancestor placement: identity on an ancestor changes nothing for descendants.
    let base = ext(".a{margin:3px}");
    assert_eq!(dump(&ext("body{text-size-adjust:none}.a{margin:3px}")), dump(&base));
    // Conflict: an identity value followed (or preceded) by a refused value still refuses; the
    // identity does not mask the later/earlier `auto` in the same rule or across rules.
    assert!(ext(".a{text-size-adjust:100%;text-size-adjust:auto}").unsupported);
    assert!(ext(".a{text-size-adjust:auto;text-size-adjust:100%}").unsupported);
    assert!(ext("body{text-size-adjust:100%}.a{text-size-adjust:auto}").unsupported);
    // Positive control: with only the identity declaration the same pages are accepted.
    assert!(!ext(".a{text-size-adjust:100%;text-size-adjust:none}").unsupported);
    // var() resolved: the ext entry substitutes custom properties before declarations are
    // interpreted, so a var() that resolves to an identity literal is accepted (positive,
    // resolved control) and equals the base; a var() resolving to `auto`, or undefined, refuses.
    let r = ext("html{--t:100%}.a{margin:3px;text-size-adjust:var(--t)}");
    assert!(!r.unsupported);
    assert_eq!(dump(&r), dump(&base));
    assert!(ext("html{--t:auto}.a{text-size-adjust:var(--t)}").unsupported);
    assert!(ext(".a{text-size-adjust:var(--missing)}").unsupported);
    // Inline style attribute path.
    let inline = |v: &str| parse_style_tree_with_inline_sheets_and_selectors(
        format!("<!doctype html><p style='text-size-adjust:{v}'>x</p>").as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    assert!(!inline("100%").unsupported);
    assert!(inline("auto").unsupported);
}
