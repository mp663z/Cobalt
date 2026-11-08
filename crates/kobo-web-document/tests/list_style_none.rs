//! PRIVATE, unadopted candidate: `list-style-type: none` (ext opt-in entry only). A list item with
//! no marker is a plain block box; every other list item keeps its unmodelled marker and stays
//! refused. No marker is painted or modelled; counters are not modelled.
use kobo_web_document::box_tree::{BoxKind, BoxTree};
use kobo_web_document::css_background::paint_backgrounds;
use kobo_web_document::{
    parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, parse_style_tree_with_media,
    parse_style_tree_with_pinned_font_family, Limits, MediaContext, SupplyBudget,
};

fn ext(css: &str, body: &str) -> kobo_web_document::style_tree::StyleTree {
    // Expectation change (disclosed, fl9): the ext entry now refuses a bare ul/ol because Chrome's UA gives them 16px
    // margins and 40px padding that the engine never modelled. Every page here authors them as 0 first so the tests keep
    // exercising list-style semantics only.
    let h = format!("<!doctype html><style>ul,ol{{margin:0;padding:0}}{css}</style>{body}");
    parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
}
fn kinds(t: &BoxTree, tags: &kobo_web_document::style_tree::StyleTree, tag: &str) -> Vec<BoxKind> {
    t.boxes.iter().filter(|b| b.source.is_some_and(|s| tags.nodes[s].tag == tag && b.text.is_none())).map(|b| b.kind).collect()
}
const LIST: &str = "<ul><li>a</li><li>b</li></ul>";

#[test]
fn explicit_none_makes_markerless_block_and_default_stays_list_item() {
    let s = ext("ul{list-style-type:none}", LIST);
    assert!(!s.unsupported);
    assert_eq!(kinds(&BoxTree::from_style(&s), &s, "li"), vec![BoxKind::Block, BoxKind::Block]);
    let d = ext("", LIST);
    assert!(!d.unsupported);
    assert_eq!(kinds(&BoxTree::from_style(&d), &d, "li"), vec![BoxKind::ListItem, BoxKind::ListItem]);
}

#[test]
fn inheritance_and_cascade() {
    // Chrome 156 computed listStyleType (private evidence lsn/p1-p3): `li{none}` applies to that li; an
    // author `none` on ul reaches its li (and nested ul, since ul inherits it), but NOT a nested ol
    // (UA decimal); an author `none` on body/div does NOT reach li under ul/ol (UA disc/circle/decimal)
    // and only reaches an li whose parent is not ul/ol.
    let nested = "<ul><li>a<ul><li>b</li></ul></li></ul>";
    let s = ext("li{list-style-type:none}", nested);
    assert!(kinds(&BoxTree::from_style(&s), &s, "li").iter().all(|k| *k == BoxKind::Block));
    let s = ext("ul{list-style-type:none}", nested);
    assert!(kinds(&BoxTree::from_style(&s), &s, "li").iter().all(|k| *k == BoxKind::Block));
    for css in ["body{list-style-type:none}", "div{list-style-type:none}"] {
        let s = ext(css, &format!("<div>{nested}<ol><li>c</li></ol></div>"));
        assert!(kinds(&BoxTree::from_style(&s), &s, "li").iter().all(|k| *k == BoxKind::ListItem), "{css}: UA ul/ol markers return");
    }
    let s = ext("body{list-style-type:none}", "<li>direct</li>");
    assert_eq!(kinds(&BoxTree::from_style(&s), &s, "li"), vec![BoxKind::Block], "li whose parent is not ul/ol inherits none (Chrome li[l4]=none)");
    let s = ext("div{list-style-type:none}", "<div><li>x</li></div>");
    assert_eq!(kinds(&BoxTree::from_style(&s), &s, "li"), vec![BoxKind::Block]);
    // ul none does not reach a nested ol (Chrome o2=decimal), but ol none + ul none reaches everything.
    let s = ext("ul{list-style-type:none}", "<ul><li>a<ol><li>c</li></ol></li></ul>");
    assert_eq!(kinds(&BoxTree::from_style(&s), &s, "li"), vec![BoxKind::Block, BoxKind::ListItem]);
    let s = ext("ul,ol{list-style-type:none}", "<ul><li>a<ol><li>c</li></ol></li></ul><ol><li><ul><li>d</li></ul></li></ol>");
    assert!(kinds(&BoxTree::from_style(&s), &s, "li").iter().all(|k| *k == BoxKind::Block));
    // initial resets to the unmodelled default marker: refused ListItem again.
    let s = ext("ul{list-style-type:none}li{list-style-type:initial}", LIST);
    assert!(kinds(&BoxTree::from_style(&s), &s, "li").iter().all(|k| *k == BoxKind::ListItem));
    // inherit and unset pick the parent's none; explicit later declaration wins by cascade.
    for css in ["ul{list-style-type:none}li{list-style-type:inherit}", "ul{list-style-type:none}li{list-style-type:unset}"] {
        let s = ext(css, LIST);
        assert!(kinds(&BoxTree::from_style(&s), &s, "li").iter().all(|k| *k == BoxKind::Block), "{css}");
    }
    // Specificity/order: a later same-specificity `initial` wins over an earlier none, and !important beats it.
    let s = ext("li{list-style-type:none}li{list-style-type:initial}", LIST);
    assert!(kinds(&BoxTree::from_style(&s), &s, "li").iter().all(|k| *k == BoxKind::ListItem));
    let s = ext("li{list-style-type:none !important}li{list-style-type:initial}", LIST);
    assert!(kinds(&BoxTree::from_style(&s), &s, "li").iter().all(|k| *k == BoxKind::Block));
    // Only list items change: a div under list-style none keeps its kind (no display change).
    let s = ext("body{list-style-type:none}", "<div>x</div>");
    assert_eq!(kinds(&BoxTree::from_style(&s), &s, "div"), vec![BoxKind::Block]);
    // display:list-item on a non-li follows the same rule.
    let s = ext("div{display:list-item;list-style-type:none}", "<div>x</div>");
    assert_eq!(kinds(&BoxTree::from_style(&s), &s, "div"), vec![BoxKind::Block]);
    let s = ext("div{display:list-item}", "<div>x</div>");
    assert_eq!(kinds(&BoxTree::from_style(&s), &s, "div"), vec![BoxKind::ListItem]);
}

#[test]
fn every_other_value_and_the_other_longhands_stay_refused() {
    for v in ["disc", "decimal", "circle", "square", "\"-\"", "lower-roman", "NONE x", "none none", "", "var(--x)", "symbols(cyclic \"*\")"] {
        assert!(ext(&format!("ul{{list-style-type:{v}}}"), LIST).unsupported, "{v}");
    }
    // EXPECTATION CHANGED (lh/ls1): `list-style:none` is now admitted under ext (see tests/list_style_shorthand_none.rs).
    for d in ["list-style-image:none", "list-style-position:inside", "counter-reset:list-item 0", "counter-increment:list-item 0"] {
        assert!(ext(&format!("ul{{{d}}}"), LIST).unsupported, "{d}");
    }
}

#[test]
fn other_entries_keep_refusing() {
    let h = format!("<!doctype html><style>ul{{list-style-type:none}}</style>{LIST}");
    assert!(parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported);
    assert!(parse_style_tree_with_pinned_font_family(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL).unsupported);
    assert!(parse_style_tree_with_media(h.as_bytes(), &[], &Limits::DEFAULT, MediaContext::new(785).unwrap()).unsupported);
}

#[test]
fn markerless_list_paints_exactly_like_the_block_equivalent_and_default_list_is_refused() {
    // Empty items: the rectangle-only painter refuses text, so this isolates the list-item gate.
    let empty = "<ul><li></li><li></li></ul>";
    let none = ext("ul{list-style-type:none;margin:0;padding:0}li{background-color:#cde;height:20px;margin:0}", empty);
    let blk = ext("div{margin:0;padding:0}.i{display:block;background-color:#cde;height:20px;margin:0}", "<div><div class=i></div><div class=i></div></div>");
    let a = paint_backgrounds(&BoxTree::from_style(&none), 100, 100).expect("markerless list paints");
    let b = paint_backgrounds(&BoxTree::from_style(&blk), 100, 100).expect("block paints");
    let bg = kobo_web_document::display_list::Rgb(255, 255, 255);
    let (ra, rb) = (a.rasterize(100, 100, bg).unwrap(), b.rasterize(100, 100, bg).unwrap());
    assert!(ra.iter().any(|&x| x != 255), "something painted");
    assert_eq!(ra, rb);
    let def = ext("ul{margin:0;padding:0}li{background-color:#cde;height:20px;margin:0}", empty);
    assert!(paint_backgrounds(&BoxTree::from_style(&def), 100, 100).is_err(), "default marker still refused");
}

#[test]
fn marker_box_and_counter_observers_stay_refused_with_markerless_items() {
    // These are the ways a markerless list item could still be observed; each must keep the page refused.
    for (css, body) in [
        ("ul{list-style-type:none}li::marker{color:red}", LIST),
        ("ul{list-style-type:none}li::marker{content:\"x\"}", LIST),
        ("ul{list-style-type:none;list-style-image:none}", LIST),
        ("ul{list-style-type:none;list-style-position:inside}", LIST),
        ("ul{list-style:none;list-style-image:none}", LIST),
        ("ul{list-style:none;list-style-position:inside}", LIST),
        ("ul{list-style-type:none;counter-reset:list-item 5}", LIST),
        ("ul{list-style-type:none}li{counter-increment:list-item 2}", LIST),
        ("ul{list-style-type:none}li{counter-set:list-item 2}", LIST),
        ("ul{list-style-type:none}li::before{content:counter(list-item)}", LIST),
        ("ul{list-style-type:none}li::before{content:counters(list-item,\".\")}", LIST),
    ] {
        assert!(ext(css, body).unsupported, "{css}");
    }
}

#[test]
fn revert_keywords_match_chrome_computed_values() {
    // Chrome 156 computed listStyleType (private evidence lsn/p4-p6): li revert under ul none -> none
    // (UA declares nothing for li, so it inherits); ul revert under body none -> disc (UA ul);
    // li revert-layer under ul none -> none (engine refuses revert-layer).
    let s = ext("ul{list-style-type:none}li{list-style-type:revert}", LIST);
    assert!(!s.unsupported);
    assert!(kinds(&BoxTree::from_style(&s), &s, "li").iter().all(|k| *k == BoxKind::Block));
    // revert-layer is not modelled by the engine at all: it fails closed (Chrome computes none here).
    assert!(ext("ul{list-style-type:none}li{list-style-type:revert-layer}", LIST).unsupported);
    let s = ext("body{list-style-type:none}ul{list-style-type:revert}", LIST);
    assert!(kinds(&BoxTree::from_style(&s), &s, "li").iter().all(|k| *k == BoxKind::ListItem));
}
