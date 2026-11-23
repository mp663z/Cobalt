//! PRIVATE, unadopted candidate (ext entry only): `position:relative` with all-auto offsets, admitted only
//! where nothing can move or overlap (no float/clear/absolute/negative margin on the page). Groundwork only:
//! it provides no offsets; since B1 only direct absolute LEAF children of the relative block are admitted (see position_relative_leaf.rs), other absolute boxes stay refused.
use kobo_web_document::{
    box_tree::BoxTree, css_background::{paint_backgrounds, paint_backgrounds_float_kernel}, display_list::{Command, Rgb}, parse_style_tree,
    parse_style_tree_with_inline_sheets_and_selectors, parse_style_tree_with_media,
    parse_style_tree_with_pinned_font_family, Limits, MediaContext, SupplyBudget,
};
const BODY: &str = "<div class=a></div><div class=b><div class=c></div></div><div class=d></div>";
const BASE: &str = "body{margin:0}.a{height:10px;background:#f00}.b{height:20px;background:#0f0;padding:0}.c{height:5px;background:#00f}.d{height:7px;background:#ff0}";
fn ext(css: &str, body: &str) -> kobo_web_document::style_tree::StyleTree {
    let h = format!("<!doctype html><style>{css}</style>{body}");
    parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
}
fn list(css: &str, body: &str) -> Option<String> {
    let t = ext(css, body);
    if t.unsupported { return None; }
    let bt = BoxTree::from_style(&t);
    if bt.unsupported { return None; }
    paint_backgrounds(&bt, 785, 600).ok().map(|l| format!("{:?}", l.rasterize(785, 40, Rgb(255, 255, 255)).expect("raster")))
}
fn order(css: &str, body: &str) -> Vec<usize> {
    let bt = BoxTree::from_style(&ext(css, body));
    paint_backgrounds(&bt, 785, 600).unwrap().commands().iter().filter_map(|c| match c {
        Command::Fill { source, .. } => source.node,
        _ => None,
    }).collect()
}
#[test]
fn auto_offset_relative_paints_exactly_as_static_in_the_no_overlap_domain() {
    let base = list(BASE, BODY).expect("static base paints");
    for extra in [".b{position:relative}", ".a{position:relative}.d{position:relative}", ".c{position:relative}",
                  ".b{position:relative}.c{position:relative}", ".b{position:relative;top:auto;left:auto}"] {
        let got = list(&format!("{BASE}{extra}"), BODY);
        assert_eq!(got.as_deref(), Some(base.as_str()), "{extra}");
    }
}
#[test]
fn relative_box_paints_as_one_layer_after_static_content_children_after_the_box() {
    // CSS 2.2 App. E: static content first; the positioned box then its non-positioned descendants.
    // Static order is a(5) b(6) c(7) d(8); with .b relative: a d b c. Pixel equality above holds only
    // because the guard refuses the MODELLED overlap sources (not all possible overlap). The orders differ, so this is not a no-op paint path.
    assert_eq!(order(BASE, BODY), [5, 6, 7, 8]);
    assert_eq!(order(&format!("{BASE}.b{{position:relative}}"), BODY), [5, 8, 6, 7]);
    // Nested relative boxes: outer layer first (tree order), then its static children, then the inner box.
    assert_eq!(order(&format!("{BASE}.b{{position:relative}}.c{{position:relative}}"), BODY), [5, 8, 6, 7]);
}
#[test]
fn overlap_sources_offsets_and_unsupported_kinds_refuse() {
    for extra in [
        ".b{position:relative;top:5px}", ".b{position:relative;left:0}", ".b{position:relative;right:1px}", ".b{position:relative;bottom:-1px}",
        ".b{position:relative;margin-top:-3px}", ".d{margin-top:-3px}.b{position:relative}",
        ".a{float:left;width:5px}.b{position:relative}",
        // FL42 (disclosed edit): `.b{position:relative}.d{clear:both}` was pinned as refused here. With no float on the page clear is a
        // no-op and now admits (equal to the clear-free twin, asserted below); a float + clear page still refuses.
        ".a{float:left;width:5px}.b{position:relative}.d{clear:both}",
        // 1117/1122 expected refusal here; since B1 an abs LEAF directly under relative is admitted (see
        // position_relative_leaf.rs). An abs box NOT directly under the relative box stays refused:
        ".b{position:relative}.d{position:absolute;width:3px;height:3px;top:0;left:0}",
        ".b{position:relative;display:inline}", ".b{position:relative;direction:rtl}",
        ".b{position:relative;z-index:1}",
    ] {
        assert!(list(&format!("{BASE}{extra}"), BODY).is_none(), "{extra}");
    }
    let (c, n) = (list(&format!("{BASE}.b{{position:relative}}.d{{clear:both}}"), BODY), list(&format!("{BASE}.b{{position:relative}}"), BODY));
    assert!(n.is_some() && c == n, "FL42: float-free clear equals its clear-free twin");
    // Inline relative element is refused (kind guard).
    assert!(list(&format!("{BASE}span{{position:relative}}"), "<div class=a><span>x</span></div>").is_none());
}
#[test]
fn other_entries_keep_refusing_relative() {
    let h = format!("<!doctype html><style>{BASE}.b{{position:relative}}</style>{BODY}");
    assert!(parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported);
    assert!(parse_style_tree_with_pinned_font_family(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL).unsupported);
    assert!(parse_style_tree_with_media(h.as_bytes(), &[], &Limits::DEFAULT, MediaContext::new(785).unwrap()).unsupported);
}

/// The painter refuses some of these for other reasons, so check the tree guard itself. A confined clearfix
/// float page (the float kernel's own shape) is NOT refused by the tree, so it isolates the float/clear guard.
#[test]
fn tree_guard_alone_refuses_float_clear_and_inline_kind() {
    let bt = |css: &str, body: &str| BoxTree::from_style(&ext(css, body)).unsupported;
    let kernel = ".p::after{content:\"\";display:block;clear:both}.p{width:400px;background:#ccc}.f{float:left;width:100px;height:50px;background:#f00}.m{width:400px;height:20px;background:#880}";
    let kbody = "<div class=p><div class=f></div></div><div class=m></div>";
    let k = |css: &str| paint_backgrounds_float_kernel(&BoxTree::from_style(&ext(&format!("body{{margin:0}}{css}"), kbody)), 785, 600).is_ok();
    assert!(k(&format!("{kernel}")), "control: kernel float page paints through the float-kernel entry");
    assert!(!k(&format!("{kernel}.m{{position:relative}}")), "float/clear present with relative");
    assert!(!k(&format!("{kernel}.p{{position:relative}}")), "relative on the float container");
    assert!(!bt(&format!("{BASE}span{{color:#123456}}"), "<div class=a><span>x</span></div>"));
    assert!(!bt(&format!("{BASE}.b{{position:relative}}"), BODY));
    assert!(bt(&format!("{BASE}span{{position:relative}}"), "<div class=a><span>x</span></div>"));
}
