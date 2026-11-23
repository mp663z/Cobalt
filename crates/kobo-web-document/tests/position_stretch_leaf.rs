//! PRIVATE candidate (ext entry only, bg bridge only): a direct, empty absolute leaf of a relative Block parent whose width
//! and/or height is `auto` and fixed by BOTH opposite px offsets (stretch). Chrome 156 fixture st3 (960x260, engine vs
//! Chrome RGBA 0 diffs, constructed synthetic page, 960 wide): bounding boxes below come from the Chrome png.
use kobo_web_document::{
    box_tree::BoxTree, css_background::paint_backgrounds, css_text_page::{paint_direct_text_blocks, FontProvider}, display_list::Rgb, inline_lines::GlyphBitmap, parse_style_tree,
    parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget,
};
const CSS: &str = "body{margin:0}.g{height:5px}.r{position:relative;margin-left:30px;width:300px;padding:6px 8px;background:#0f0}.in{height:30px;background:#00f}.l1{position:absolute;inset:5px 7px 3px 9px;background:#f00}.l2{position:absolute;width:20px;top:2px;bottom:4px;left:150px;background:#ff0}.l3{position:absolute;height:6px;left:3px;right:200px;top:30px;background:#0ff}.l4{position:absolute;inset:-4px -5px -6px -3px;padding:2px 3px;background:#808}";
const BODY: &str = "<div class=g></div><div class=r><div class=in></div><div class=l1></div></div><div class=g></div><div class=r><div class=in></div><div class=l2></div></div><div class=g></div><div class=r><div class=in></div><div class=l3></div></div><div class=g></div><div class=r><div class=in></div><div class=l4></div></div><div class=g></div>";
fn paint(css: &str, body: &str, ext: bool) -> Option<Vec<u8>> { paint_h(css, body, ext, 260) }
fn paint_h(css: &str, body: &str, ext: bool, hh: u32) -> Option<Vec<u8>> {
    let h = format!("<!doctype html><style>{css}</style>{body}");
    let t = if ext {
        parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
    } else {
        parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT)
    };
    if t.unsupported { return None; }
    let bt = BoxTree::from_style(&t);
    if bt.unsupported { return None; }
    paint_backgrounds(&bt, 960, hh).ok().map(|l| l.rasterize(960, hh, Rgb(255, 255, 255)).unwrap())
}
#[test]
fn stretched_leaves_match_chrome_bounding_boxes() {
    let p = paint(CSS, BODY, true).expect("admitted");
    let at = |x: usize, y: usize| { let i = (y * 960 + x) * 4; (p[i], p[i + 1], p[i + 2]) };
    // l1: both axes auto; l2: width px, height auto; l3: width auto, height px; l4: negative offsets + padding.
    for (c, [x0, y0, x1, y1]) in [
        ((255, 0, 0), [39, 10, 338, 43]),
        ((255, 255, 0), [180, 54, 199, 89]),
        ((0, 255, 255), [33, 129, 145, 134]),
        ((136, 0, 136), [27, 142, 350, 193]),
    ] {
        for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] { assert_eq!(at(x, y), c, "inside corner {c:?} ({x},{y})"); }
        for (x, y) in [(x0 - 1, y0 + 1), (x1 + 1, y0 + 1), (x0 + 1, y0 - 1), (x0 + 1, y1 + 1)] { assert_ne!(at(x, y), c, "outside {c:?} ({x},{y})"); }
    }
}
#[test]
fn stretch_equals_the_equivalent_px_leaf() {
    // l1 on the first parent: parent padding box 316x(30+12)=42 high; offsets 5/7/3/9 -> 300x34 at (9,5).
    let a = paint(CSS, BODY, true).unwrap();
    let px = CSS.replace(".l1{position:absolute;inset:5px 7px 3px 9px;", ".l1{position:absolute;width:300px;height:34px;inset:5px 7px 3px 9px;");
    let b = paint(&px, BODY, true).expect("px twin admitted");
    assert!(a == b, "stretch leaf paints the same as its explicit px twin");
}
#[test]
fn incomplete_or_unmodelled_stretch_stays_refused() {
    let one = |l: &str| format!("{CSS}.x{{position:absolute;background:#f00;{l}}}");
    let body = "<div class=g></div><div class=r><div class=in></div><div class=x></div></div>";
    assert!(paint(&one("left:3px;top:3px;bottom:3px"), body, true).is_none(), "auto width with only left");
    assert!(paint(&one("left:3px;right:3px;top:3px"), body, true).is_none(), "auto height with only top");
    assert!(paint(&one("inset:3px;margin:1px"), body, true).is_none(), "leaf margin");
    assert!(paint(&one("inset:3px;border:1px solid #000"), body, true).is_none(), "leaf border");
    assert!(paint(&one("inset:3px 400px"), body, true).is_none(), "negative used width");
    assert!(paint(&one("inset:90px 3px"), body, true).is_none(), "negative used height");
    assert!(paint(&one("inset:3px"), "<div class=g></div><div class=r><div class=in></div><div class=x>t</div></div>", true).is_none(), "leaf with content");
    assert!(paint(&one("inset:3px"), "<div class=g></div><div><div class=in></div><div class=x></div></div>", true).is_none(), "static parent");
    assert!(paint(&one("inset:3px"), body, false).is_none(), "non-ext entry");
    assert!(paint(&one("inset:3px"), body, true).is_some(), "control: complete stretch admitted");
}
#[test]
fn stretch_inside_a_bordered_definite_parent_matches_chrome() {
    // Chrome st2: .r 200x50 + padding 4 + border 3 at x20 y10; leaf inset:6px 8px resolves against the PADDING box
    // (border excluded). Chrome bboxes: border box 20..233 x 10..73, green 23..230 x 13..70, red 31..222 x 19..64.
    let css = "body{margin:0}.pre{height:10px}.r{position:relative;margin-left:20px;width:200px;height:50px;padding:4px;border:3px solid #333;background:#0f0}.l{position:absolute;inset:6px 8px;background:#f00}";
    let p = paint(css, "<div class=pre></div><div class=r><div class=l></div></div>", true).expect("admitted");
    let at = |x: usize, y: usize| { let i = (y * 960 + x) * 4; (p[i], p[i + 1], p[i + 2]) };
    for (c, [x0, y0, x1, y1]) in [((255, 0, 0), [31, 19, 222, 64]), ((51, 51, 51), [20, 10, 233, 73])] {
        for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] { assert_eq!(at(x, y), c, "inside {c:?} ({x},{y})"); }
        for (x, y) in [(x0 - 1, y0 + 1), (x1 + 1, y0 + 1), (x0 + 1, y0 - 1), (x0 + 1, y1 + 1)] { assert_ne!(at(x, y), c, "outside {c:?} ({x},{y})"); }
    }
    assert_eq!(at(25, 30), (0, 255, 0), "padding area left of the leaf is green");
}

fn corners(p: &[u8], c: (u8, u8, u8), [x0, y0, x1, y1]: [usize; 4]) {
    let at = |x: usize, y: usize| { let i = (y * 960 + x) * 4; (p[i], p[i + 1], p[i + 2]) };
    for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] { assert_eq!(at(x, y), c, "inside {c:?} ({x},{y})"); }
    for (x, y) in [(x0 - 1, y0 + 1), (x1 + 1, y0 + 1), (x0 + 1, y0 - 1), (x0 + 1, y1 + 1)] { assert_ne!(at(x, y), c, "outside {c:?} ({x},{y})"); }
}
#[test]
fn absolute_parent_stretch_matches_chrome() {
    // Chrome st4a (engine vs Chrome full-frame 0 diffs): absolute 200x50 px parent with padding 4, leaf inset:6px 8px.
    let css = "html,body{margin:0;height:100px}.a{position:absolute;left:20px;top:10px;width:200px;height:50px;padding:4px;background:#0f0}.l{position:absolute;inset:6px 8px;background:#f00}";
    let p = paint_h(css, "<div class=a><div class=l></div></div>", true, 100).expect("abs parent admitted");
    corners(&p, (0, 255, 0), [20, 10, 227, 67]);
    corners(&p, (255, 0, 0), [28, 16, 219, 61]);
}
#[test]
fn border_box_leaf_with_padding_matches_chrome_and_content_box() {
    // Chrome st4b/st5 (0 diffs): with both opposite offsets the auto axes fill the same margin box either way, padding
    // does not shrink a border-box leaf and does shrink a content-box leaf's content only.
    let css = "html,body{margin:0;height:100px}.b{position:absolute;left:30px;top:10px;width:150px;height:60px;background:#ccc}.l3{position:absolute;left:5px;right:5px;top:5px;bottom:5px;box-sizing:border-box;padding:7px;background:#ff0}";
    let body = "<div class=b><div class=l3></div></div>";
    let bb = paint_h(css, body, true, 100).expect("border-box admitted");
    corners(&bb, (255, 255, 0), [35, 15, 174, 64]);
    let cb = paint_h(&css.replace("box-sizing:border-box;", ""), body, true, 100).expect("content-box admitted");
    assert!(bb == cb, "auto axes: border-box and content-box leaves paint identically");
}
struct Face;
impl FontProvider for Face {
    fn advance(&self, _: char, _: u32, _: bool) -> Option<u32> { Some(5) }
    fn line_height(&self, _: u32, _: bool) -> Option<u32> { Some(16) }
    fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> { Some(12) }
    fn raster(&self, _: char, _: u32, _: bool) -> Option<GlyphBitmap> { Some(GlyphBitmap { left: 0, top: -1, width: 1, height: 1, coverage: vec![255] }) }
}
fn text_page(css: &str, body: &str) -> Option<Vec<u8>> {
    let h = format!("<!doctype html><style>body{{margin:0}}.r{{position:relative;width:150px;padding:2px}}.t{{font-size:16px;line-height:16px;height:20px}}{css}</style>{body}");
    let t = parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    let bt = BoxTree::from_style(&t);
    if bt.unsupported { return None; }
    paint_direct_text_blocks(&bt, 200, 60, &Face).ok().map(|l| l.rasterize(200, 60, Rgb(255, 255, 255)).unwrap())
}
#[test]
fn text_consumer_stretch_leaf_covers_earlier_text_dots_stub_font() {
    // Stub font, layer order only (no real-font or Chrome glyph parity). The stretch leaf is a later abs leaf in the
    // relative parent, so it paints above the earlier text dots, as the px leaf does.
    let without = text_page("", "<div class=r><div class=t>ab</div></div>").expect("no leaf");
    let leaf = ".l{position:absolute;inset:0 30px 0 0;background:#00f}";
    let with = text_page(leaf, "<div class=r><div class=t>ab</div><div class=l></div></div>");
    // Actual outcome recorded by this assertion: admitted with the leaf covering the dots.
    let with = with.expect("stretch leaf admitted by the text painter");
    let at = |p: &[u8], x: usize, y: usize| { let i = (y * 200 + x) * 4; (p[i], p[i + 1], p[i + 2]) };
    let dots: Vec<_> = (0..60).flat_map(|y| (0..120).map(move |x| (x, y))).filter(|&(x, y)| at(&without, x, y) == (0, 0, 0)).collect();
    assert!(!dots.is_empty(), "control: dots exist");
    for (x, y) in dots { assert_eq!(at(&with, x, y), (0, 0, 255), "leaf above dot ({x},{y})"); }
}
#[test]
fn default_entry_still_refuses_stretch_under_an_absolute_parent() {
    // Unmasked boundary (the relative/inset refusals of the default entry would otherwise hide a bypass): absolute parent,
    // longhands only, zero margins. Default entry must keep refusing; ext admits.
    let pre = "html,body{margin:0;height:100px}.a{position:absolute;left:20px;top:10px;width:200px;height:50px;padding:4px;background:#0f0}.l{position:absolute;left:8px;right:8px;top:6px;bottom:6px;background:#f00}";
    let body = "<div class=a><div class=l></div></div>";
    assert!(paint_h(pre, body, false, 100).is_none(), "default entry refuses auto-size abs leaf");
    assert!(paint_h(pre, body, true, 100).is_some(), "ext entry admits");
}
#[test]
fn text_consumer_actual_outcomes_per_leaf_kind_stub_font() {
    // Actual owner-run outcomes of paint_direct_text_blocks (stub font, layer order only) for each leaf kind under a
    // relative parent with text "ab" (padding 2, width 150, padding box 154x24): ADMITTED stretch both, stretch width only,
    // stretch height only, centre horizontal only; REFUSED centre both axes and centre vertical only. Refused is a
    // text-painter limit, not a claim about Chrome.
    let page = |leaf: &str| text_page(&format!(".l{{position:absolute;background:#00f;{leaf}}}"), "<div class=r><div class=t>ab</div><div class=l></div></div>");
    for leaf in ["inset:0 30px 0 0", "left:0;right:30px;top:0;height:12px", "left:0;width:60px;top:0;bottom:0", "width:40px;height:20px;left:0;right:100px;top:0;margin:0 auto"] {
        assert!(page(leaf).is_some(), "admitted: {leaf}");
    }
    for leaf in ["width:60px;height:12px;left:0;right:30px;top:0;bottom:0;margin:auto", "width:60px;height:12px;left:0;top:0;bottom:0;margin:auto 0"] {
        assert!(page(leaf).is_none(), "refused: {leaf}");
    }
    // Centred horizontal-only leaf: free space 154-0-100-40 = 14 -> x 7..46, y 0..19. It covers the second dot (x 7) only.
    let without = text_page("", "<div class=r><div class=t>ab</div></div>").unwrap();
    let with = page("width:40px;height:20px;left:0;right:100px;top:0;margin:0 auto").unwrap();
    let at = |p: &[u8], x: usize, y: usize| { let i = (y * 200 + x) * 4; (p[i], p[i + 1], p[i + 2]) };
    let dots: Vec<_> = (0..60).flat_map(|y| (0..120).map(move |x| (x, y))).filter(|&(x, y)| at(&without, x, y) == (0, 0, 0)).collect();
    assert_eq!(dots.len(), 2, "control: two stub dots");
    let covered = |x: usize| (7..=46).contains(&x);
    for (x, y) in dots { assert_eq!(at(&with, x, y), if covered(x) { (0, 0, 255) } else { (0, 0, 0) }, "dot ({x},{y})"); }
    assert_eq!(at(&with, 7, 0), (0, 0, 255)); assert_eq!(at(&with, 46, 19), (0, 0, 255));
    assert_ne!(at(&with, 6, 5), (0, 0, 255)); assert_ne!(at(&with, 47, 5), (0, 0, 255));
}
