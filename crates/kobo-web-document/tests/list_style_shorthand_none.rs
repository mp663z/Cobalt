//! PRIVATE candidate (ext entry only): `list-style: none` shorthand equals `list-style-type: none` here.
//! Chrome 156 (private evidence ls1/out.json): `ul{list-style:none}` gives li none/none/outside; nested ol li stays decimal;
//! `ul.c li{list-style-type:inherit}` keeps none; `body>li{list-style:none}` gives none. Same as the type longhand tests.
use kobo_web_document::box_tree::{BoxKind, BoxTree};
use kobo_web_document::{parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget};
fn ext(css: &str, body: &str) -> kobo_web_document::style_tree::StyleTree {
    // Expectation change (disclosed, fl10): bare ul/ol now refuse under ext (UA margins/padding), so every page authors them
    // as 0 first; otherwise the a-vs-b comparison below compared two unsupported trees.
    let h = format!("<!doctype html><style>ul,ol{{margin:0;padding:0}}{css}</style>{body}");
    parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
}
fn kinds(s: &kobo_web_document::style_tree::StyleTree) -> Vec<BoxKind> {
    BoxTree::from_style(s).boxes.iter().filter(|b| b.source.is_some_and(|i| s.nodes[i].tag == "li" && b.text.is_none())).map(|b| b.kind).collect()
}
#[test]
fn shorthand_none_equals_type_none_across_cascade_cases() {
    let nested = "<ul><li>a<ol><li>c</li></ol></li></ul><ol><li><ul><li>d</li></ul></li></ol><li>direct</li>";
    for sel in ["ul", "ol", "ul,ol", "li", "body", "div", "ul li", "ul>li"] {
        let a = ext(&format!("{sel}{{list-style:none}}"), nested);
        let b = ext(&format!("{sel}{{list-style-type:none}}"), nested);
        assert!(!a.unsupported && !b.unsupported, "anchor: both admitted, not both-unsupported: {sel}");
        assert_eq!((a.unsupported, kinds(&a)), (b.unsupported, kinds(&b)), "{sel}");
    }
    let ul = ext("ul{list-style:none}", "<ul><li>a<ol><li>c</li></ol></li></ul>");
    assert_eq!(kinds(&ul), vec![BoxKind::Block, BoxKind::ListItem]); // Chrome: li none, nested ol li decimal
    let inh = ext("ul{list-style:none}li{list-style-type:inherit}", "<ul><li>c</li></ul>");
    assert_eq!(kinds(&inh), vec![BoxKind::Block]);
    let init = ext("ul{list-style:none}li{list-style-type:initial}", "<ul><li>c</li></ul>");
    assert_eq!(kinds(&init), vec![BoxKind::ListItem]);
    // later shorthand wins over earlier longhand none? both none: stays Block; shorthand initial refuses.
    let later = ext("ul{list-style-type:none}ul{list-style:none}", "<ul><li>c</li></ul>");
    assert_eq!(kinds(&later), vec![BoxKind::Block]);
}
#[test]
fn default_entry_and_other_shorthand_values_refuse() {
    let h = "<!doctype html><style>ul{list-style:none}</style><ul><li>a</li></ul>";
    assert!(parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported);
    for v in ["none inside", "none outside", "disc", "none url(a.png)", "inherit", "initial", "unset", "square inside", "url(a.png)", "none none", "NONE x", ""] {
        assert!(ext(&format!("ul{{list-style:{v}}}"), "<ul><li>a</li></ul>").unsupported, "{v}");
    }
}
