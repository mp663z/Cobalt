//! Percent-guard coverage for the shaped and underline text entries (fake faces/shapers: this pins
//! admission dataflow only, not Chrome parity). Both entries run the shared inner painter, which
//! builds WidthPass itself; the 250px*65.2% float32 hole must be refused and 50% admitted.
use kobo_web_document::{
    box_tree::BoxTree,
    css_text_page::{paint_direct_text_blocks_shaped, paint_direct_text_blocks_underline, FontProvider, UnderlineProfile},
    inline_lines::{FitRule, GlyphBitmap, RunShaper, ShapedCluster, TextShaping},
    parse_style_tree, parse_style_tree_with_text_decoration, Limits,
};
struct Face;
impl FontProvider for Face {
    fn face_sha256_hex(&self) -> Option<String> { Some(UnderlineProfile::ChromeLiberationSerif156.face_sha256_hex().to_owned()) }
    fn advance(&self, _: char, _: u32, _: bool) -> Option<u32> { Some(2) }
    fn line_height(&self, _: u32, _: bool) -> Option<u32> { Some(10) }
    fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> { Some(7) }
    fn raster(&self, ch: char, _: u32, _: bool) -> Option<GlyphBitmap> {
        Some(GlyphBitmap { left: 0, top: -3, width: u32::from(ch != ' '), height: if ch == ' ' { 0 } else { 2 },
            coverage: if ch == ' ' { vec![] } else { vec![255; 2] } })
    }
}
struct Uniform;
impl RunShaper for Uniform {
    fn shape_run(&self, text: &str, _: u32, _: bool) -> Option<Vec<ShapedCluster>> {
        Some(text.chars().enumerate().map(|(i, _)| ShapedCluster { first_char: i as u32, char_len: 1, units: 2048 }).collect())
    }
}
fn html(w: &str, p_style: &str) -> String {
    format!("<!doctype html><style>html,body,p{{margin:0}}body{{width:250px}}p{{width:{w};{p_style}}}</style><p>ab</p>")
}
fn shaped_ok(w: &str) -> bool {
    let t = BoxTree::from_style(&parse_style_tree(html(w, "line-height:10px").as_bytes(), &[], &Limits::DEFAULT));
    let sh = TextShaping { shaper: &Uniform, fit: FitRule { unit: 1024, slack: 0 } };
    paint_direct_text_blocks_shaped(&t, 300, 100, &Face, &sh).is_ok()
}
fn underline_ok(w: &str) -> bool {
    let s = parse_style_tree_with_text_decoration(html(w, "font-size:16px;text-decoration:underline").as_bytes(), &[], &Limits::DEFAULT);
    let t = BoxTree::from_style_with_text_decoration(&s);
    paint_direct_text_blocks_underline(&t, 300, 100, &Face, UnderlineProfile::ChromeLiberationSerif156).is_ok()
}
#[test]
fn shaped_entry_refuses_float32_percent_hole_with_same_group_control() {
    assert!(shaped_ok("50%"), "control");
    assert!(shaped_ok("100px"), "px control");
    assert!(!shaped_ok("65.2%"));
}
#[test]
fn underline_entry_refuses_float32_percent_hole_with_same_group_control() {
    assert!(underline_ok("50%"), "control");
    assert!(underline_ok("100px"), "px control");
    assert!(!underline_ok("65.2%"));
}
