//! PRIVATE candidate FL20 (ext entry only): INITIAL-value declarations accepted as no-ops (INITIAL_NOOPS). Chrome 156 measured
//! each pair computed-style-identical to the undeclared div (private-evidence ul1/ini19.json, plain div only). Stub-font text
//! painter pixels, byte-equal to the bare page; no real-glyph claim; no Chrome/stored-pixel comparison.
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
const PAIRS: &[(&str, &str)] = &[
    ("opacity", "1"),
    ("text-transform", "none"),
    ("overflow-anchor", "auto"),
    ("text-shadow", "none"),
    ("transform-origin", "50% 50%"),
    ("visibility", "visible"),
    ("word-break", "normal"),
    ("hyphens", "manual"),
    ("z-index", "auto"),
    ("will-change", "auto"),
    ("isolation", "auto"),
    ("mix-blend-mode", "normal"),
    ("backface-visibility", "visible"),
    ("content-visibility", "visible"),
    ("contain", "none"),
    ("clip-path", "none"),
    ("filter", "none"),
    ("transform", "none"),
    ("perspective", "none"),
    ("object-fit", "fill"),
    ("scroll-behavior", "auto"),
    ("overscroll-behavior", "auto"),
    ("caret-color", "auto"),
    ("accent-color", "auto"),
    ("touch-action", "auto"),
    ("image-rendering", "auto"),
    ("forced-color-adjust", "auto"),
    ("text-indent", "0"),
    ("text-indent", "0px"),
    ("word-spacing", "normal"),
    ("letter-spacing", "normal"),
    ("vertical-align", "baseline"),
    ("table-layout", "auto"),
    ("border-collapse", "separate"),
    ("empty-cells", "show"),
    ("orphans", "2"),
    ("widows", "2"),
    ("writing-mode", "horizontal-tb"),
    ("order", "0"),
    ("align-self", "auto"),
    ("align-content", "normal"),
    ("aspect-ratio", "auto"),
    ("text-overflow", "clip"),
    ("resize", "none"),
    ("quotes", "auto"),
];
#[test]
fn every_initial_pair_is_byte_equal_to_bare_under_ext_and_refused_by_default() {
    let bare = pixels("", true);
    assert!(bare.is_some());
    assert_eq!(bare, pixels("", false));
    for (p, v) in PAIRS {
        let d = format!("{p}:{v}");
        assert_eq!(pixels(&d, true), bare, "{d}");
        assert_eq!(pixels(&d, false), None, "default entry unchanged: {d}");
        assert_eq!(pixels(&format!("{d} !important"), true), bare, "{d} important");
    }
}
#[test]
fn non_initial_values_of_the_same_properties_still_refuse() {
    for d in ["visibility:hidden", "opacity:0.5", "opacity:0", "text-transform:uppercase", "text-transform:capitalize", "overflow-anchor:none", "text-shadow:1px 1px red", "transform-origin:0 0", "transform-origin:left top", "visibility:collapse", "z-index:1", "z-index:-1", "transform:rotate(1deg)", "filter:blur(1px)", "order:1", "order:-1", "text-indent:10px",
        "letter-spacing:1px", "word-spacing:3px", "vertical-align:top", "vertical-align:middle", "word-break:break-all", "writing-mode:vertical-rl", "outline:1px solid red",
        "aspect-ratio:1", "aspect-ratio:1/1", "resize:both", "text-overflow:ellipsis", "border-collapse:collapse", "table-layout:fixed", "align-self:center",
        "align-content:center", "flex-flow:column", "flex-flow:row wrap", "flex-flow:row nowrap", "pointer-events:auto", "user-select:auto", "isolation:isolate", "mix-blend-mode:multiply", "will-change:transform", "contain:paint",
        "clip-path:circle(1px)", "object-fit:cover", "content-visibility:hidden", "hyphens:auto", "empty-cells:hide", "orphans:3", "widows:3", "z-index:0",
        "filter:none blur(1px)", "perspective:10px", "accent-color:red", "backface-visibility:hidden", "caret-color:red", "forced-color-adjust:none", "image-rendering:pixelated", "overscroll-behavior:contain", "quotes:none", "scroll-behavior:smooth", "touch-action:none"] {
        assert_eq!(pixels(d, true), None, "{d}");
    }
}
