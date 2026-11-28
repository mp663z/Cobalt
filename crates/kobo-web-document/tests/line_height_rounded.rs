//! A unitless line-height whose product with the font size is not a whole pixel is rounded to
//! the nearest pixel and counted, instead of silently falling back to the natural height.
use kobo_web_document::{
    basic_page::{paint_basic_page, PartialStats},
    box_tree::BoxTree,
    css_text_page::FontProvider,
    inline_lines::GlyphBitmap,
    Limits, SupplyBudget,
};
struct Face;
impl FontProvider for Face {
    fn has_glyph(&self, _: char, _: bool) -> Option<bool> { Some(true) }
    fn advance(&self, _: char, s: u32, _: bool) -> Option<u32> { Some(s / 2) }
    fn line_height(&self, s: u32, _: bool) -> Option<u32> { Some(s + 4) }
    fn baseline_offset(&self, s: u32, _: bool) -> Option<i32> { i32::try_from(s).ok() }
    fn raster(&self, _: char, s: u32, _: bool) -> Option<GlyphBitmap> {
        Some(GlyphBitmap { left: 0, top: -(s as i32), width: s / 2, height: s, coverage: vec![255; (s * s / 2) as usize] })
    }
}
/// Page height and stats for `n` single-line paragraphs at 20px (natural line height 24).
fn paint(line_height: &str, n: usize) -> (i64, PartialStats) {
    let body = "<p>x</p>".repeat(n);
    let html = format!("<!doctype html><style>body{{margin:0;font-size:20px}}p{{margin:0;line-height:{line_height}}}</style>{body}");
    let styled = kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(
        html.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL,
    );
    let (_, stats, height) = paint_basic_page(&BoxTree::from_style(&styled), 300, 500, &Face).unwrap();
    (height, stats)
}
fn rounded(stats: &PartialStats) -> usize {
    stats.approximations.get("line-height-rounded").copied().unwrap_or(0)
}
#[test]
fn whole_pixel_products_are_exact_and_uncounted() {
    let (h, s) = paint("1.5", 4);
    assert_eq!((h, rounded(&s)), (120, 0)); // 30px lines
    let (h, s) = paint("2", 4);
    assert_eq!((h, rounded(&s)), (160, 0));
}
#[test]
fn fractional_products_round_to_the_nearest_pixel_and_count_once_per_text_block() {
    let (h, s) = paint("1.57", 4); // 31.4px
    assert_eq!((h, rounded(&s)), (124, 4));
    let (h, s) = paint("1.58", 4); // 31.6px
    assert_eq!((h, rounded(&s)), (128, 4));
    let (h, s) = paint("1.6", 1); // 32px exactly: 1.6 x 20
    assert_eq!((h, rounded(&s)), (32, 0));
}
#[test]
fn normal_and_zero_adjacent_forms_keep_the_natural_height() {
    let (h, s) = paint("normal", 4);
    assert_eq!((h, rounded(&s)), (96, 0));
}
fn length_rounded(stats: &PartialStats) -> usize {
    stats.approximations.get("line-height-length-rounded").copied().unwrap_or(0)
}
#[test]
fn whole_pixel_percent_and_em_lengths_are_exact_and_uncounted() {
    for lh in ["150%", "1.5em", "30px"] {
        let (h, s) = paint(lh, 4);
        assert_eq!((h, length_rounded(&s), rounded(&s)), (120, 0, 0), "{lh}");
    }
}
#[test]
fn fractional_percent_and_em_lengths_round_to_the_nearest_pixel_and_count_per_block() {
    let (h, s) = paint("157%", 4); // 31.4px
    assert_eq!((h, length_rounded(&s), rounded(&s)), (124, 4, 0));
    let (h, s) = paint("1.58em", 4); // 31.6px
    assert_eq!((h, length_rounded(&s)), (128, 4));
    let (h, s) = paint("1.62em", 2); // 32.4px
    assert_eq!((h, length_rounded(&s)), (64, 2));
    let (h, s) = paint("1.12em", 2); // 22.4px rounds to 22, then the 24px natural floor applies
    assert_eq!((h, length_rounded(&s)), (48, 2));
}
#[test]
fn an_inherited_fractional_length_keeps_its_computed_size() {
    // The child inherits the parent's computed 31.4px, not 157% of its own 10px size.
    let html = "<!doctype html><style>body{margin:0;font-size:20px}div{line-height:157%}p{margin:0;font-size:10px}</style><div><p>x</p><p>x</p></div>";
    let styled = kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(
        html.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL,
    );
    let (_, stats, height) = paint_basic_page(&BoxTree::from_style(&styled), 300, 500, &Face).unwrap();
    assert_eq!((height, length_rounded(&stats)), (62, 2));
}
