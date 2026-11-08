//! PRIVATE candidate (ext entry only): positive min-height through the DIRECT TEXT page path with a STUB font. Stub-font
//! geometry proves layer order and the used-height seam only: the text block is 10px tall (stub line height), the min-height
//! dominates (50), so the next block must start at y=50 (Chrome fixture number for min-dominant case: ul1/mh2.json c1).
use kobo_web_document::{
    box_tree::BoxTree,
    css_text_page::{paint_direct_text_blocks, FontProvider},
    display_list::Rgb,
    inline_lines::GlyphBitmap,
    parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget,
};
struct Face;
impl FontProvider for Face {
    fn advance(&self, _: char, _: u32, _: bool) -> Option<u32> { Some(2) }
    fn line_height(&self, _: u32, _: bool) -> Option<u32> { Some(10) }
    fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> { Some(7) }
    fn raster(&self, ch: char, _: u32, _: bool) -> Option<GlyphBitmap> {
        Some(GlyphBitmap { left: 0, top: -3, width: u32::from(ch != ' '), height: if ch == ' ' { 0 } else { 2 }, coverage: if ch == ' ' { vec![] } else { vec![255; 2] } })
    }
}
fn first_aside_row(css: &str) -> Option<usize> {
    let h = format!("<!doctype html><style>body{{margin:0}}div,section,aside{{margin:0;padding:0}}{css}</style><div><section>ab</section><aside style='height:7px;background-color:#f00'></aside></div>");
    let t = parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    if t.unsupported {
        return None;
    }
    let bt = BoxTree::from_style(&t);
    let list = paint_direct_text_blocks(&bt, 60, 100, &Face).ok()?;
    let rgba = list.rasterize(60, 100, Rgb(255, 255, 255)).ok()?;
    (0..100).find(|&y| rgba[(y * 60) * 4..(y * 60) * 4 + 3] == [255, 0, 0])
}
#[test]
fn min_height_dominates_a_stub_text_block_and_pushes_the_next_block() {
    assert_eq!(first_aside_row(""), Some(10), "anchor: without min-height the 10px stub line ends at 10");
    assert_eq!(first_aside_row("section{min-height:50px}"), Some(50));
    assert_eq!(first_aside_row("section{min-height:6px}"), Some(10), "content (10) > min (6)");
}
