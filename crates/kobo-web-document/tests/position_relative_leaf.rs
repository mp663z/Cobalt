//! PRIVATE, unadopted candidate (ext entry only), B1: absolute LEAF children of a `position:relative` block
//! containing block (padding box), background painter only. Expected pixels are Chrome 156 samples taken from
//! the same page (evidence rel/chrome/b1.png, 960x100); engine-vs-Chrome whole-image diff was 0/96000 pixels.
use kobo_web_document::{
    box_tree::BoxTree, css_background::paint_backgrounds, display_list::Rgb,
    parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget,
};
const CSS: &str = "body{margin:0}.pre{height:13px;background:#ccc}.r{position:relative;margin-left:30px;width:300px;padding:6px 8px;background:#0f0}.in{height:40px;background:#00f}.l1{position:absolute;width:20px;height:10px;top:4px;left:5px;background:#f00}.l2{position:absolute;width:30px;height:12px;bottom:3px;right:7px;background:#ff0}.l3{position:absolute;width:20px;height:15px;top:48px;left:-5px;background:#0ff}.post{height:9px;background:#f0f}";
const BODY: &str = "<div class=pre></div><div class=r><div class=in></div><div class=l1></div><div class=l2></div><div class=l3></div></div><div class=post></div>";
fn tree(css: &str, body: &str) -> BoxTree {
    let h = format!("<!doctype html><style>{css}</style>{body}");
    BoxTree::from_style(&parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL))
}
fn px(css: &str, body: &str) -> Option<Vec<u8>> {
    let bt = tree(css, body);
    if bt.unsupported { return None; }
    paint_backgrounds(&bt, 960, 100).ok().map(|l| l.rasterize(960, 100, Rgb(255, 255, 255)).unwrap())
}
fn at(p: &[u8], x: usize, y: usize) -> (u8, u8, u8) { let i = (y * 960 + x) * 4; (p[i], p[i + 1], p[i + 2]) }
#[test]
fn leaves_resolve_against_the_relative_padding_box_and_paint_in_the_positioned_layer() {
    let p = px(CSS, BODY).expect("admitted");
    assert_eq!(at(&p, 40, 12), (204, 204, 204), "pre");
    assert_eq!(at(&p, 40, 15), (0, 255, 0), "relative padding area");
    assert_eq!(at(&p, 40, 30), (0, 0, 255), "in-flow child");
    assert_eq!(at(&p, 50, 25), (255, 0, 0), "l1 top/left against the padding box (Chrome sample)");
    assert_eq!(at(&p, 325, 50), (255, 255, 0), "l2 bottom/right against the padding box (Chrome sample)");
    assert_eq!(at(&p, 40, 69), (0, 255, 255), "l3 overflows the relative box and paints above the later static .post");
    assert_eq!(at(&p, 35, 70), (0, 255, 255), "negative left");
    assert_eq!(at(&p, 100, 70), (255, 0, 255), "post visible beside the leaf");
}
#[test]
fn static_container_uses_the_viewport_instead() {
    // Control: without relative the same leaves resolve against the initial containing block (Chrome differs too).
    let p = px(&CSS.replace("position:relative;", ""), BODY).expect("admitted");
    assert_eq!(at(&p, 50, 25), (0, 0, 255));
    assert_ne!(at(&p, 325, 50), (255, 255, 0));
}
#[test]
fn unsupported_shapes_refuse() {
    for (css, body) in [
        // leaf without explicit offsets on both axes
        (CSS.replace("top:4px;left:5px;", "left:5px;"), BODY.to_string()),
        (CSS.replace("top:4px;left:5px;", ""), BODY.to_string()),
        // leaf with a child, auto size, margin, rtl parent
        (CSS.to_string(), BODY.replace("<div class=l1></div>", "<div class=l1><div class=in></div></div>")),
        (CSS.replace("width:20px;height:10px;", "width:20px;"), BODY.to_string()),
        (CSS.replace(".l1{", ".l1{margin-top:2px;"), BODY.to_string()),
        (CSS.replace(".r{", ".r{direction:rtl;"), BODY.to_string()),
        // relative ancestor above the relative parent (depth 2), second positioned level
        (CSS.to_string(), format!("<div class=r>{BODY}</div>")),
        // an absolute box not under a relative block while relative is present
        (format!("{CSS}.z{{position:absolute;width:3px;height:3px;top:0;left:0}}"), format!("{BODY}<div class=z></div>")),
        // float/offset on the relative block itself
        (CSS.replace(".r{", ".r{top:3px;"), BODY.to_string()),
        (CSS.replace(".r{", ".r{float:left;"), BODY.to_string()),
        (CSS.replace(".r{", ".r{z-index:1;"), BODY.to_string()),
    ] {
        assert!(px(&css, &body).is_none(), "{css}");
    }
    assert!(px(CSS, BODY).is_some(), "control");
}

/// Broader parent shapes, each with a Chrome 156 fixture (evidence rel/chrome/b1-*.html/png): engine vs Chrome
/// RGBA was 0 differing pixels (960x100) for border, percent width and left+right; the expectations below are the
/// Chrome sampled pixels.
#[test]
fn border_percent_width_and_left_right_precedence_match_chrome_samples() {
    // Border on the relative parent: the leaf resolves against the PADDING box (inside the 3px border).
    let p = px(&CSS.replace(".r{", ".r{border:3px solid #123456;"), BODY).expect("border");
    assert_eq!(at(&p, 45, 25), (255, 0, 0), "l1 at padding box + (5,4)");
    for (x, y, c) in [(37, 25, (0, 255, 0)), (38, 25, (255, 0, 0)), (57, 25, (255, 0, 0)), (58, 25, (0, 0, 255)),
                      (45, 19, (0, 255, 0)), (45, 20, (255, 0, 0)), (45, 29, (255, 0, 0)), (45, 30, (0, 0, 255))] {
        assert_eq!(at(&p, x, y), c, "l1 edge at ({x},{y}) (Chrome)");
    }
    assert_eq!(at(&p, 31, 40), (18, 52, 86), "border color");
    assert_eq!(at(&p, 35, 40), (0, 255, 0));
    // Percent parent width (50% of the 960 viewport): right/bottom offsets use the resolved padding box.
    let p = px(&CSS.replace("width:300px;", "width:50%;"), BODY).expect("percent");
    assert_eq!(at(&p, 500, 55), (255, 255, 0), "l2 against the 480px parent");
    assert_eq!(at(&p, 600, 30), (255, 255, 255), "outside the parent");
    // left and right both set with a definite width: left wins in ltr, right is ignored (Chrome).
    let body = BODY.replace("</div><div class=post>", "<div class=l4></div></div><div class=post>");
    let css = format!("{CSS}.l4{{position:absolute;width:25px;height:8px;top:20px;left:10px;right:10px;background:#0ff}}");
    let p = px(&css, &body).expect("left+right");
    assert_eq!(at(&p, 50, 36), (0, 255, 255), "left wins");
    assert_eq!(at(&p, 320, 36), (0, 0, 255), "right offset ignored: in-flow child shows");
}
/// CORRECTION (reviewer, 11:32): the earlier version of this test refused through the unsupported `list-style:none`
/// SHORTHAND, which masks the marker/containing-block path. Recognised `list-style-type:none` normalises a DOM
/// list item to a Block box (LSN2), so a markerless <li> IS eligible as a relative parent; a marker-bearing <li> is not.
#[test]
fn markerless_li_is_an_eligible_parent_and_marker_bearing_li_is_refused() {
    let leaf = ".l1{position:absolute;width:20px;height:10px;top:4px;left:5px;background:#f00}.post{height:9px;background:#f0f}";
    let r = ".r{position:relative;margin-left:30px;width:300px;height:40px;background:#0f0}";
    let body = "<div style=\"height:13px;background:#ccc\"></div><ul><li class=r><div class=l1></div></li></ul><div class=post></div>";
    // Positive: recognised list-style-type:none (Chrome fixture b1-li-none: engine vs Chrome 0/96000 pixels).
    let p = px(&format!("body{{margin:0}}ul{{margin:0;padding:0;list-style-type:none}}{r}{leaf}"), body).expect("markerless li");
    for (x, y, c) in [(34, 20, (0, 255, 0)), (35, 20, (255, 0, 0)), (54, 20, (255, 0, 0)), (55, 20, (0, 255, 0)),
                      (40, 16, (0, 255, 0)), (40, 17, (255, 0, 0)), (40, 26, (255, 0, 0)), (40, 27, (0, 255, 0)), (100, 60, (255, 0, 255))] {
        assert_eq!(at(&p, x, y), c, "({x},{y}) (Chrome)");
    }
    // Same page with the default marker: refused by the bridge (ListItem is visible content). This is the unmasked
    // marker-bearing refusal: every other declaration here is the recognised, admitted set above.
    assert!(px(&format!("body{{margin:0}}ul{{margin:0;padding:0}}{r}{leaf}"), body).is_none());
    // EXPECTATION CHANGED (ls1): the `list-style:none` shorthand is now admitted under ext and must paint exactly like the
    // `list-style-type:none` longhand page above. display:list-item is still refused earlier (style layer).
    let short = px(&format!("body{{margin:0}}ul{{margin:0;padding:0;list-style:none}}{r}{leaf}"), body).expect("shorthand markerless li");
    assert!(short == p, "shorthand pixels differ from longhand pixels");
}
