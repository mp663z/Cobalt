//! PRIVATE candidate (ext entry only): `transition` family as no-ops under the first-computed-style-only contract, with a
//! bounded TOKEN-FILTER grammar (not CSS validation: some invalid combinations are also accepted as inert). Chrome 156
//! initial computed colour fixture matches; that is a fixture check, not computed-style equality.
use kobo_web_document::box_tree::BoxTree;
use kobo_web_document::{parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget};
fn ext(css: &str) -> kobo_web_document::style_tree::StyleTree {
    let h = format!("<!doctype html><style>a{{text-decoration:none}}{css}</style><div><a href='/x'>t</a></div>");
    parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
}
fn ink(t: &kobo_web_document::style_tree::StyleTree) -> Vec<u32> {
    t.nodes.iter().filter(|n| n.tag == "a").map(|n| n.style.color).collect()
}
#[test]
fn bounded_transition_values_are_noops_and_change_no_style() {
    let base = ext("a{color:#1a65a6}");
    assert!(!base.unsupported);
    let want = ink(&base);
    assert_eq!(want.len(), 1);
    assert_ne!(want[0], ink(&ext("a{color:#000000}"))[0], "anchor: colour is a real value, not a default");
    for v in [
        "transition:none", "transition:color .2s", "transition:color 200ms ease-in-out", "transition:all 0s", "transition:color .2s linear 1s",
        "transition:color .2s,background-color .3s", "transition-property:color", "transition-duration:.3s", "transition-delay:0s",
        "transition-timing-function:ease",
    ] {
        let t = ext(&format!("a{{color:#1a65a6;{v}}}"));
        assert!(!t.unsupported, "{v}");
        assert_eq!(ink(&t), want, "{v}: same computed colour as without it");
        assert!(!BoxTree::from_style(&t).unsupported, "{v}");
    }
}
#[test]
fn unbounded_or_other_transition_values_refuse() {
    for v in [
        "transition:color cubic-bezier(0,0,1,1) .2s", "transition:color steps(2) .2s", "transition:inherit", "transition:initial", "transition:unset",
        "transition:color .2s allow-discrete", "transition:a 1s,b 1s,c 1s,d 1s,e 1s", "transition:color 1s 2s 3s 4s", "transition:", "transition:color 1x",
        "transition:color ..2s", "transition:var(--x)", "transition-property:-webkit-foo", "transition-duration:1", "transition-behavior:allow-discrete",
    ] {
        let t = ext(&format!("a{{{v}}}"));
        assert!(t.unsupported, "{v}");
    }
}
#[test]
fn default_entry_still_refuses_transition() {
    let h = "<!doctype html><style>a{transition:color .2s}</style><div><a href='/x'>t</a></div>";
    assert!(parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported);
}
