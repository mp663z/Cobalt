//! PRIVATE candidate (ext entry; shared BoxTree geometry, so text consumers see it too - not tested with text here):
//! `margin:auto` centring of a direct, empty absolute leaf along an axis with a px size and BOTH opposite px offsets.
//! Chrome 156 fixture ct1 (960x260, constructed, engine vs Chrome RGBA 0 diffs). Odd free space (half-pixel margins),
//! negative free space, one-sided auto margins, auto size with auto margins, one-sided offsets all stay refused.
use kobo_web_document::{
    box_tree::BoxTree, css_background::paint_backgrounds, display_list::Rgb, parse_style_tree,
    parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget,
};
const CSS: &str = "body{margin:0}.g{height:5px}.r{position:relative;margin-left:30px;width:300px;height:40px;padding:4px;background:#0f0}.l1{position:absolute;width:100px;height:20px;left:10px;right:10px;top:0;bottom:0;margin:auto;background:#f00}.l2{position:absolute;width:50px;left:0;right:0;top:3px;height:10px;margin:0 auto;background:#00f}.l3{position:absolute;left:5px;width:20px;top:0;bottom:0;height:10px;margin:auto 0;background:#ff0}.l4{position:absolute;width:60px;height:12px;padding:3px 5px;left:7px;right:9px;top:1px;bottom:1px;margin:auto;background:#808}";
const BODY: &str = "<div class=g></div><div class=r><div class=l1></div></div><div class=g></div><div class=r><div class=l2></div></div><div class=g></div><div class=r><div class=l3></div></div><div class=g></div><div class=r><div class=l4></div></div><div class=g></div>";
fn paint(css: &str, body: &str, ext: bool) -> Option<Vec<u8>> {
    let h = format!("<!doctype html><style>{css}</style>{body}");
    let t = if ext {
        parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
    } else {
        parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT)
    };
    if t.unsupported { return None; }
    let bt = BoxTree::from_style(&t);
    if bt.unsupported { return None; }
    paint_backgrounds(&bt, 960, 260).ok().map(|l| l.rasterize(960, 260, Rgb(255, 255, 255)).unwrap())
}
#[test]
fn centred_leaves_match_chrome_bounding_boxes() {
    let p = paint(CSS, BODY, true).expect("admitted");
    let at = |x: usize, y: usize| { let i = (y * 960 + x) * 4; (p[i], p[i + 1], p[i + 2]) };
    // l1 both axes; l2 horizontal only; l3 vertical only; l4 both axes with padding (outer 70x18).
    for (c, [x0, y0, x1, y1]) in [
        ((255, 0, 0), [134, 19, 233, 38]),
        ((0, 0, 255), [159, 61, 208, 70]),
        ((255, 255, 0), [35, 130, 54, 139]),
        ((136, 0, 136), [148, 179, 217, 196]),
    ] {
        for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] { assert_eq!(at(x, y), c, "inside {c:?} ({x},{y})"); }
        for (x, y) in [(x0 - 1, y0 + 1), (x1 + 1, y0 + 1), (x0 + 1, y0 - 1), (x0 + 1, y1 + 1)] { assert_ne!(at(x, y), c, "outside {c:?} ({x},{y})"); }
    }
}
#[test]
fn unmodelled_auto_margin_stays_refused() {
    let body = "<div class=g></div><div class=r><div class=x></div></div>";
    let one = |l: &str| format!("{CSS}.x{{position:absolute;background:#f00;{l}}}");
    let ok = "width:100px;height:20px;left:10px;right:10px;top:0;bottom:0;margin:auto";
    assert!(paint(&one(ok), body, true).is_some(), "control: both axes centred");
    assert!(paint(&one("width:100px;height:20px;left:10px;right:10px;top:0;bottom:0;margin-left:auto"), body, true).is_none(), "one auto margin");
    assert!(paint(&one("width:100px;height:20px;left:10px;top:0;bottom:0;margin:auto"), body, true).is_none(), "horizontal auto with one offset");
    assert!(paint(&one("width:100px;height:20px;left:10px;right:10px;top:0;margin:auto"), body, true).is_none(), "vertical auto with one offset");
    assert!(paint(&one("width:99px;height:20px;left:10px;right:10px;top:0;bottom:0;margin:auto"), body, true).is_none(), "odd free width");
    assert!(paint(&one("width:100px;height:21px;left:10px;right:10px;top:0;bottom:0;margin:auto"), body, true).is_none(), "odd free height");
    assert!(paint(&one("width:300px;height:20px;left:10px;right:10px;top:0;bottom:0;margin:auto"), body, true).is_none(), "negative free width");
    assert!(paint(&one("width:100px;height:60px;left:10px;right:10px;top:0;bottom:0;margin:auto"), body, true).is_none(), "negative free height");
    assert!(paint(&one("inset:3px;margin:auto"), body, true).is_none(), "auto size with auto margins");
    assert!(paint(&one(ok), body, false).is_none(), "non-ext entry");
}
#[test]
fn default_entry_still_refuses_margin_auto_under_an_absolute_parent() {
    let pre = "html,body{margin:0;height:100px}.a{position:absolute;left:20px;top:10px;width:200px;height:50px;padding:4px;background:#0f0}.l{position:absolute;width:100px;height:20px;left:10px;right:10px;top:0;bottom:0;margin:auto;background:#f00}";
    let h = format!("<!doctype html><style>{pre}</style><div class=a><div class=l></div></div>");
    let run = |ext: bool| {
        let t = if ext { parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL) } else { parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT) };
        if t.unsupported { return false; }
        let bt = BoxTree::from_style(&t);
        !bt.unsupported && paint_backgrounds(&bt, 960, 100).is_ok()
    };
    assert!(!run(false), "default entry refuses margin:auto centring");
    assert!(run(true), "ext entry admits");
}
#[test]
fn padded_leaf_centres_by_outer_size_for_both_box_sizings_and_borders_are_admitted() {
    // Chrome ct3 (engine 0 diffs): padding 2px 3px; border-box outer = 100 (bbox x134..233), content-box outer = 106 (x131..236).
    let pre = "body{margin:0}.g{height:5px}.r{position:relative;margin-left:30px;width:300px;height:40px;padding:4px;background:#0f0}.l{position:absolute;width:100px;height:20px;padding:2px 3px;left:10px;right:10px;top:0;bottom:0;margin:auto;background:#00f}";
    let body = "<div class=g></div><div class=r><div class=l></div></div>";
    let cb = paint(pre, body, true).expect("content-box admitted");
    let bb = paint(&format!("{pre}.l{{box-sizing:border-box}}"), body, true).expect("border-box admitted");
    let at = |p: &[u8], x: usize, y: usize| { let i = (y * 960 + x) * 4; (p[i], p[i + 1], p[i + 2]) };
    // Chrome bbox rows 19..38 for both; x: content-box 131..236 (outer 106), border-box 134..233 (outer 100).
    for (p, x0, x1) in [(&cb, 131, 236), (&bb, 134, 233)] {
        assert_eq!(at(p, x0, 19), (0, 0, 255)); assert_eq!(at(p, x1, 38), (0, 0, 255));
        assert_ne!(at(p, x0 - 1, 25), (0, 0, 255)); assert_ne!(at(p, x1 + 1, 25), (0, 0, 255));
    }
    // Chrome ct2 (engine 0 diffs): leaf border 3px solid #333, padding 2, margin auto, content-box (red) and border-box (blue).
    let ct2 = "body{margin:0}.g{height:5px}.r{position:relative;margin-left:30px;width:300px;height:40px;padding:4px;background:#0f0}.l1{position:absolute;width:100px;height:20px;padding:2px;border:3px solid #333;left:10px;right:10px;top:0;bottom:0;margin:auto;background:#f00}.l2{position:absolute;width:100px;height:20px;padding:2px;border:3px solid #333;box-sizing:border-box;left:10px;right:10px;top:0;bottom:0;margin:auto;background:#00f}";
    let p = paint(ct2, "<div class=g></div><div class=r><div class=l1></div></div><div class=g></div><div class=r><div class=l2></div></div>", true).expect("bordered centred admitted");
    for (c, [x0, y0, x1, y1]) in [((51, 51, 51), [129, 14, 238, 43]), ((255, 0, 0), [132, 17, 235, 40]), ((51, 51, 51), [134, 72, 233, 91]), ((0, 0, 255), [137, 75, 230, 88])] {
        assert_eq!(at(&p, x0, y0), c, "corner {c:?}"); assert_eq!(at(&p, x1, y1), c, "corner {c:?}");
        assert_ne!(at(&p, x0 - 1, y0 + 1), c); assert_ne!(at(&p, x1 + 1, y0 + 1), c);
    }
}
