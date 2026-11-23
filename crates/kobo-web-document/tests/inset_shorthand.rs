//! PRIVATE, unadopted candidate (ext entry only): `inset` shorthand of `auto`/integer-px values expands to
//! top/right/bottom/left with the margin-style rule. Offsets only; positioned validation unchanged.
use kobo_web_document::{
    box_tree::BoxTree, css_background::paint_backgrounds, display_list::Rgb, parse_style_tree,
    parse_style_tree_with_inline_sheets_and_selectors, parse_style_tree_with_media,
    parse_style_tree_with_pinned_font_family, Limits, MediaContext, SupplyBudget,
};
fn ext(css: &str) -> kobo_web_document::style_tree::StyleTree {
    let h = format!("<!doctype html><style>{css}</style><div class=a><p class=b>x</p></div>");
    parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
}
fn dump(t: &kobo_web_document::style_tree::StyleTree) -> String {
    t.nodes.iter().map(|n| format!("{}:{:?}", n.tag, n.style)).collect::<Vec<_>>().join("|")
}
#[test]
fn expansion_equals_the_longhands_for_every_arity() {
    for (short, long) in [
        ("5px", "top:5px;right:5px;bottom:5px;left:5px"),
        ("auto", "top:auto;right:auto;bottom:auto;left:auto"),
        ("1px 2px", "top:1px;right:2px;bottom:1px;left:2px"),
        ("auto 10px 1px", "top:auto;right:10px;bottom:1px;left:10px"),
        ("1px 2px 3px 4px", "top:1px;right:2px;bottom:3px;left:4px"),
        ("-1px 0 auto -4px", "top:-1px;right:0;bottom:auto;left:-4px"),
        ("  3px   auto  ", "top:3px;right:auto;bottom:3px;left:auto"),
    ] {
        let a = ext(&format!(".b{{position:absolute;inset:{short}}}"));
        let b = ext(&format!(".b{{position:absolute;{long}}}"));
        assert!(!a.unsupported, "{short}");
        assert_eq!(dump(&a), dump(&b), "{short}");
    }
    // Later longhand overrides, earlier longhand is overridden (source order within the cascade).
    assert_eq!(dump(&ext(".b{position:absolute;inset:1px;left:9px}")), dump(&ext(".b{position:absolute;top:1px;right:1px;bottom:1px;left:9px}")));
    assert_eq!(dump(&ext(".b{position:absolute;left:9px;inset:1px}")), dump(&ext(".b{position:absolute;top:1px;right:1px;bottom:1px;left:1px}")));
    // !important carries to all four longhands: it beats a later normal longhand.
    assert_eq!(dump(&ext(".b{position:absolute;inset:1px !important;left:9px}")), dump(&ext(".b{position:absolute;top:1px;right:1px;bottom:1px;left:1px}")));
}
#[test]
fn everything_else_refuses_and_other_entries_refuse() {
    for v in ["", "1px 2px 3px 4px 5px", "50%", "1em", "calc(1px + 2px)", "inherit", "initial", "1px,2px", "auto auto auto auto auto", "1px 2% 3px"] {
        assert!(ext(&format!(".b{{inset:{v}}}")).unsupported, "{v:?}");
    }
    for p in ["inset-inline", "inset-block", "inset-inline-start", "inset-block-end"] {
        assert!(ext(&format!(".b{{{p}:1px}}")).unsupported, "{p}");
    }
    let h = "<!doctype html><style>.b{position:absolute;inset:1px}</style><div class=b></div>";
    assert!(parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported);
    assert!(parse_style_tree_with_pinned_font_family(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL).unsupported);
    assert!(parse_style_tree_with_media(h.as_bytes(), &[], &Limits::DEFAULT, MediaContext::new(785).unwrap()).unsupported);
}
const CSS: &str = "body{margin:0}.pre{height:13px;background:#ccc}.r{position:relative;margin-left:30px;width:300px;padding:6px 8px;background:#0f0}.in{height:40px;background:#00f}.l1{position:absolute;width:20px;height:10px;inset:auto 10px 1px;background:#f00}.l2{position:absolute;width:30px;height:12px;inset:4px 7px 3px 5px;background:#ff0}.l3{position:absolute;width:6px;height:5px;inset:20px 9px;background:#0ff}.post{height:9px;background:#f0f}";
const BODY: &str = "<div class=pre></div><div class=r><div class=in></div><div class=l1></div><div class=l2></div><div class=l3></div></div><div class=post></div>";
#[test]
fn leaves_placed_by_inset_match_chrome_bounding_boxes() {
    // Chrome 156 b1-inset (960x100): engine vs Chrome RGBA 0 diffs; leaf bboxes from the Chrome png:
    // red x40..59 y54..63 (top auto, bottom 1px, left=right 10px: left wins), yellow x35..64 y17..28, cyan x39..44 y33..37.
    let h = format!("<!doctype html><style>{CSS}</style>{BODY}");
    let t = parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    let bt = BoxTree::from_style(&t);
    assert!(!t.unsupported && !bt.unsupported);
    let p = paint_backgrounds(&bt, 960, 100).unwrap().rasterize(960, 100, Rgb(255, 255, 255)).unwrap();
    let at = |x: usize, y: usize| { let i = (y * 960 + x) * 4; (p[i], p[i + 1], p[i + 2]) };
    for (c, [x0, y0, x1, y1]) in [((255, 0, 0), [40, 54, 59, 63]), ((255, 255, 0), [35, 17, 64, 28]), ((0, 255, 255), [39, 33, 44, 37])] {
        for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] { assert_eq!(at(x, y), c, "inside corner {c:?} ({x},{y})"); }
        for (x, y) in [(x0 - 1, y0 + 1), (x1 + 1, y0 + 1), (x0 + 1, y0 - 1), (x0 + 1, y1 + 1)] { assert_ne!(at(x, y), c, "outside {c:?} ({x},{y})"); }
    }
}
#[test]
fn var_in_inset_expands_before_the_shorthand_like_the_longhands() {
    // Resolved custom property is substituted first, then the shorthand expands (Chrome order).
    for (def, short, long) in [
        ("--x:5px", "var(--x)", "top:5px;right:5px;bottom:5px;left:5px"),
        ("--x:1px", "var(--x) 2px", "top:1px;right:2px;bottom:1px;left:2px"),
        ("--x:auto", "var(--x) 3px 4px", "top:auto;right:3px;bottom:4px;left:3px"),
    ] {
        let a = ext(&format!(":root{{{def}}}.b{{position:absolute;inset:{short}}}"));
        let b = ext(&format!(":root{{{def}}}.b{{position:absolute;{long}}}"));
        assert!(!a.unsupported, "{short}");
        assert_eq!(dump(&a), dump(&b), "{short}");
    }
}
#[test]
fn undefined_var_in_inset_is_not_silently_expanded() {
    let a = ext(".b{position:absolute;inset:var(--nope)}");

    // Actual outcome (owner-run): the page is refused (unsupported); strict refusal, no guessed expansion.
    assert!(a.unsupported);
}
