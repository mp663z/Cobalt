//! PRIVATE candidate FL19 (ext entry only): `overflow|overflow-x|overflow-y: visible` (the initial value) is accepted as a
//! no-op; FL22 DISCLOSED EXPECTATION CHANGES: `overflow:inherit` is now admitted as a typed keyword (box tree gates any resulting hidden); overflow-x/-y:visible now REFUSE (FL19 skipped them; they could pair with a hidden shorthand), and `overflow:hidden` is decided by the box tree (text children still refuse here); every other overflow value still refuses. Stub-font text painter plus block geometry; no real-glyph claim.
use kobo_web_document::box_tree::BoxTree;
use kobo_web_document::css_text_page::{paint_direct_text_blocks, FontProvider};
use kobo_web_document::display_list::Rgb;
use kobo_web_document::inline_lines::GlyphBitmap;
use kobo_web_document::{parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget};
struct Face;
impl FontProvider for Face {
    fn advance(&self, _: char, _: u32, _: bool) -> Option<u32> { Some(5) }
    fn line_height(&self, _: u32, _: bool) -> Option<u32> { Some(16) }
    fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> { Some(12) }
    fn raster(&self, _: char, _: u32, _: bool) -> Option<GlyphBitmap> { Some(GlyphBitmap { left: 0, top: -1, width: 1, height: 1, coverage: vec![255] }) }
}
fn pixels(decl: &str, ext: bool) -> Option<Vec<u8>> {
    let h = format!("<!doctype html><style>body{{margin:0}}.t{{font-size:16px;line-height:16px;background:#eee;width:60px;{decl}}}</style><div class=t>aa bb cc dd</div><div class=t style='margin-top:4px'>ee</div>");
    let s = if ext { parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL) } else { parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT) };
    if s.unsupported { return None; }
    let bt = BoxTree::from_style(&s);
    if bt.unsupported { return None; }
    paint_direct_text_blocks(&bt, 100, 80, &Face).ok()?.rasterize(100, 80, Rgb(255, 255, 255)).ok()
}
#[test]
fn visible_is_byte_equal_to_bare_under_ext_and_other_values_refuse() {
    let bare = pixels("", true);
    assert!(bare.is_some());
    for d in ["overflow:visible", "overflow:visible;overflow:visible"] {
        assert_eq!(pixels(d, true), bare, "{d}");
    }
    for d in ["overflow-x:visible", "overflow-y:visible", "overflow-x:visible;overflow-y:visible", "overflow:clip", "overflow:scroll", "overflow:auto", "overflow:visible hidden", "overflow-x:hidden", "overflow-y:auto", "overflow:", "overflow:Visible2", "overflow-x:clip;overflow-x:visible"] {
        assert_eq!(pixels(d, true), None, "{d}");
    }
}
#[test]
fn default_entry_unchanged_still_refuses_visible() {
    assert!(pixels("", false).is_some());
    assert_eq!(pixels("overflow:visible", false), None);
}
