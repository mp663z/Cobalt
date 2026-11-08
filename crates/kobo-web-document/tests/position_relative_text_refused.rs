//! PRIVATE candidate. History: 1117/1122 refused every text entry on a page with `position:relative` (reviewer
//! counterexample: earlier .a height 1px, 16px direct text overflowing into a later relative .b background; Chrome
//! paints .b OVER the earlier text). Since the text-layer change the text painter has a positioned-layer phase, so
//! that page is admitted and the relative block covers the overflowing text, as in Chrome (stored control
//! rel/chrome/txt-static.png vs txt-rel.png differ at bbox 0,1,18,13). The font here is a stub; engine-vs-Chrome
//! glyph parity is NOT claimed, only the layer order.
use kobo_web_document::{
    box_tree::BoxTree,
    css_text_page::{paint_direct_text_blocks, FontProvider},
    display_list::Rgb,
    inline_lines::GlyphBitmap,
    parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget,
};
struct Face;
impl FontProvider for Face {
    fn advance(&self, _: char, _: u32, _: bool) -> Option<u32> { Some(5) }
    fn line_height(&self, _: u32, _: bool) -> Option<u32> { Some(16) }
    fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> { Some(12) }
    fn raster(&self, _: char, _: u32, _: bool) -> Option<GlyphBitmap> {
        Some(GlyphBitmap { left: 0, top: -1, width: 1, height: 1, coverage: vec![255] })
    }
}
fn render(extra_b: &str) -> Option<Vec<u8>> {
    let css = format!("body{{margin:0}}.a{{height:1px;font-size:16px;line-height:16px}}.b{{height:20px;background:#f00{extra_b}}}");
    let h = format!("<!doctype html><style>{css}</style><div class=a>ab</div><div class=b></div>");
    let t = parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    let bt = BoxTree::from_style(&t);
    if bt.unsupported { return None; }
    paint_direct_text_blocks(&bt, 100, 40, &Face).ok().map(|l| l.rasterize(100, 40, Rgb(255, 255, 255)).unwrap())
}
fn at(p: &[u8], x: usize, y: usize) -> (u8, u8, u8) { let i = (y * 100 + x) * 4; (p[i], p[i + 1], p[i + 2]) }
#[test]
fn relative_block_covers_earlier_overflowing_text_like_chrome() {
    let s = render("").expect("static baseline");
    let r = render(";position:relative").expect("relative now admitted by the text painter");
    // Glyph pixels of the earlier text that lie on the later block (y>=1) are black when static, covered when relative.
    let on_b: Vec<(usize, usize)> = (1..16).flat_map(|y| (0..20).map(move |x| (x, y))).filter(|&(x, y)| at(&s, x, y) == (0, 0, 0)).collect();
    assert!(!on_b.is_empty(), "static: text paints above the later normal-flow block");
    for (x, y) in on_b { assert_eq!(at(&r, x, y), (255, 0, 0), "covered at ({x},{y})"); }
}

#[test]
fn text_inside_the_relative_block_paints_above_its_own_background() {
    // The relative block's own text is part of its layer: above its background. (Absolute-leaf order is tested
    // separately in absolute_leaf_in_a_relative_parent_paints_above_earlier_text.)
    let h = |extra: &str| format!("<!doctype html><style>body{{margin:0}}.r{{position:relative;height:20px;line-height:16px;font-size:16px;background:#f00}}.l{{position:absolute;width:6px;height:6px;top:0;left:0;background:#00f}}</style><div class=r>ab{extra}</div>");
    let px = |html: String| {
        let t = parse_style_tree_with_inline_sheets_and_selectors(html.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
        let bt = BoxTree::from_style(&t);
        assert!(!bt.unsupported);
        paint_direct_text_blocks(&bt, 100, 40, &Face).ok().map(|l| l.rasterize(100, 40, Rgb(255, 255, 255)).unwrap())
    };
    // Text and block children cannot mix directly (the painter needs an all-inline block), so use text only.
    let p = px(h("")).expect("relative text block");
    let black = (0..20).flat_map(|y| (0..30).map(move |x| (x, y))).filter(|&(x, y)| at(&p, x, y) == (0, 0, 0)).count();
    assert!(black > 0, "own text visible above the relative block's background");
}

#[test]
fn each_background_is_painted_exactly_once_across_layers() {
    use kobo_web_document::display_list::Command;
    let css = "body{margin:0}.a{height:1px;font-size:16px;line-height:16px}.b{height:20px;background:#f00;position:relative}";
    let h = format!("<!doctype html><style>{css}</style><div class=a>ab</div><div class=b></div>");
    let t = parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    let l = paint_direct_text_blocks(&BoxTree::from_style(&t), 100, 40, &Face).unwrap();
    let reds = l.commands().iter().filter(|c| matches!(c, Command::Fill { color: Rgb(255, 0, 0), .. })).count();
    assert_eq!(reds, 1, "relative background painted once");
}

// ---- Regressions requested at review (stub font: layer order only) ----
fn page(css: &str, body: &str) -> Option<Vec<u8>> {
    let h = format!("<!doctype html><style>body{{margin:0}}.t{{font-size:16px;line-height:16px;height:20px}}{css}</style>{body}");
    let t = parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    let bt = BoxTree::from_style(&t);
    if bt.unsupported { return None; }
    paint_direct_text_blocks(&bt, 100, 60, &Face).ok().map(|l| l.rasterize(100, 60, Rgb(255, 255, 255)).unwrap())
}
fn blacks(p: &[u8], ys: std::ops::Range<usize>, xs: std::ops::Range<usize>) -> Vec<(usize, usize)> {
    ys.flat_map(|y| xs.clone().map(move |x| (x, y))).filter(|&(x, y)| at(p, x, y) == (0, 0, 0)).collect()
}
#[test]
fn nested_relative_layers_order() {
    // r1 (red) contains r2 (blue) with text. r2 paints after r1; r2's text above r2's background.
    let body = "<div class=r1><div class=r2><div class=t>ab</div></div></div>";
    let css = ".r1{position:relative;background:#f00;padding:2px}.r2{position:relative;background:#00f}";
    let p = page(css, body).expect("nested relative admitted");
    assert_eq!(at(&p, 1, 1), (255, 0, 0), "r1 padding area");
    assert_eq!(at(&p, 60, 10), (0, 0, 255), "r2 background above r1");
    assert!(!blacks(&p, 2..22, 2..30).is_empty(), "text above its blue background");
}
#[test]
fn non_positioned_descendant_text_is_covered_by_a_later_relative_sibling() {
    // r1's child .c (height 1px, 16px text overflowing down) is a NON-positioned descendant of relative r1.
    // The later relative sibling r2 paints after r1's whole layer, so r2's background covers the overflowing text.
    let body = "<div class=r1><div class=c>ab</div></div><div class=r2></div>";
    let css = ".r1{position:relative}.c{height:1px;font-size:16px;line-height:16px}.r2{position:relative;height:20px;background:#f00}";
    let p = page(css, body).expect("admitted");
    let without = page(".r1{position:relative}.c{height:1px;font-size:16px;line-height:16px}.r2{height:20px;background:#f00}", body).expect("r2 static");
    // With a static r2 the text overflow is above the later block (control); with relative r2 it is covered.
    let on_r2 = blacks(&without, 1..16, 0..20);
    assert!(!on_r2.is_empty(), "control: static later block leaves overflowing text visible");
    for (x, y) in on_r2 { assert_eq!(at(&p, x, y), (255, 0, 0), "covered at ({x},{y})"); }
}
#[test]
fn absolute_leaf_in_a_relative_parent_paints_above_earlier_text() {
    let body = "<div class=r><div class=t>ab</div><div class=l></div></div>";
    let base = ".r{position:relative}";
    let without = page(base, "<div class=r><div class=t>ab</div></div>").expect("no leaf");
    let leaf = format!("{base}.l{{position:absolute;width:12px;height:12px;top:0;left:0;background:#00f}}");
    let with = page(&leaf, body).expect("leaf admitted");
    let glyphs_under = blacks(&without, 0..12, 0..12);
    assert!(!glyphs_under.is_empty(), "control: text pixels exist where the leaf will sit");
    for (x, y) in glyphs_under { assert_eq!(at(&with, x, y), (0, 0, 255), "leaf above text at ({x},{y})"); }
    let outside = blacks(&without, 0..20, 12..40);
    for (x, y) in outside { assert_eq!(at(&with, x, y), (0, 0, 0), "text outside the leaf stays ({x},{y})"); }
}
#[test]
fn non_relative_absolute_leaf_is_above_text_as_before() {
    // No relative box on the page: abs leaves paint last (unchanged behaviour). Command identity against the
    // pre-change core is in evidence txtlayer/cmd-identity-*.txt (probe run on both trees).
    let body = "<div class=t>ab</div><div class=l></div>";
    let without = page("", "<div class=t>ab</div>").expect("no leaf");
    let with = page(".l{position:absolute;width:12px;height:12px;top:0;left:0;background:#00f}", body).expect("leaf");
    let under = blacks(&without, 0..12, 0..12);
    assert!(!under.is_empty());
    for (x, y) in under { assert_eq!(at(&with, x, y), (0, 0, 255)); }
}
#[test]
fn mixed_block_and_raw_text_inline_relative_and_inline_runs_are_refused_or_unchanged() {
    // Raw text next to a block child normalises to an anonymous block and fails closed in the text painter.
    assert!(page(".r{position:relative}", "<div class=r>ab<div class=t></div></div>").is_none());
    // An inline relative element is refused by the tree guard.
    assert!(page("span{position:relative}", "<div class=t>a<span>b</span></div>").is_none());
    // Ordinary inline run inside a relative block parent: record the actual outcome (admitted or refused) and require
    // it to equal the same page without relative, so relative itself changes nothing here.
    let inline = |rel: &str| page(&format!(".r{{{rel}}}"), "<div class=r><div class=t>a<b>b</b></div></div>");
    // Actual outcome (probe txtpng, stub font): BOTH are admitted with identical RGBA, so assert admission and equal pixels.
    let (rel, stat) = (inline("position:relative"), inline(""));
    assert!(rel.is_some() && stat.is_some(), "inline run in a block parent is admitted with and without relative");
    assert!(rel == stat, "relative on the block parent changes no pixel of the inline-run page");
}
