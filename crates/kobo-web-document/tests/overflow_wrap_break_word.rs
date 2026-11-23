//! PRIVATE candidate (ext entry only): `overflow-wrap|word-wrap: break-word|normal` is a no-op for every page the
//! engine admits (the placer refuses a word wider than the line, which is the only case break-word changes).
//! Stub-font text painter; no real-glyph claim.
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
fn ext(css: &str, body: &str) -> kobo_web_document::style_tree::StyleTree {
    let h = format!("<!doctype html><style>body{{margin:0}}.t{{font-size:16px;line-height:16px;background:#eee;width:60px}}{css}</style>{body}");
    parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
}
fn pixels(css: &str, body: &str) -> Option<Vec<u8>> {
    let s = ext(css, body);
    if s.unsupported { return None; }
    let bt = BoxTree::from_style(&s);
    if bt.unsupported { return None; }
    paint_direct_text_blocks(&bt, 100, 60, &Face).ok()?.rasterize(100, 60, Rgb(255, 255, 255)).ok()
}
#[test]
fn admitted_pages_paint_exactly_like_bare_and_too_wide_words_refuse_either_way() {
    // wraps at spaces (60px box, 5px glyphs): "aa bb cc dd" -> multiple lines, admitted and byte-equal.
    let wrapping = "<div class=t>aa bb cc dd ee ff</div>";
    let bare = pixels("", wrapping).expect("bare wraps");
    for css in [".t{overflow-wrap:break-word}", ".t{word-wrap:break-word}", ".t{overflow-wrap:normal}", "body{overflow-wrap:break-word}"] {
        assert_eq!(pixels(css, wrapping).as_ref(), Some(&bare), "{css}");
    }
    // a word fitting on a fresh line moves down exactly as in normal (break-word does not split it)
    let move_down = "<div class=t>aaaa bbbbbbbbbb</div>";
    let bare2 = pixels("", move_down).expect("bare move-down");
    assert_eq!(pixels(".t{overflow-wrap:break-word}", move_down).as_ref(), Some(&bare2));
    // a word wider than the box is the one case break-word would change: the placer refuses in both entries here.
    let too_wide = "<div class=t>aaaaaaaaaaaaaaaaaaaa</div>";
    assert!(pixels("", too_wide).is_none(), "anchor: normal refuses too-wide word");
    assert!(pixels(".t{overflow-wrap:break-word}", too_wide).is_none(), "break-word must not manufacture admission");
}
#[test]
fn other_values_and_default_entry_refuse() {
    for d in ["overflow-wrap:anywhere", "overflow-wrap:inherit", "overflow-wrap:initial", "overflow-wrap:break-spaces", "overflow-wrap:", "word-wrap:anywhere", "word-wrap:break-word x", "word-break:break-word", "word-break:break-all", "hyphens:auto"] {
        assert!(ext(&format!(".t{{{d}}}"), "<div class=t>a</div>").unsupported, "{d}");
    }
    for d in ["overflow-wrap:break-word", "word-wrap:break-word", "overflow-wrap:normal"] {
        let h = format!("<!doctype html><style>.t{{{d}}}</style><div class=t>a</div>");
        assert!(parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported, "default {d}");
    }
}
