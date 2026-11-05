//! Experimental, measured-subset underline. Geometry numbers come from Chrome 156 pixels
//! (see packet); these tests pin the rule and the refusals, not general text-decoration.
use kobo_web_document::{
    box_tree::BoxTree,
    css_text_page::{paint_direct_text_blocks, paint_direct_text_blocks_underline, FontProvider, UnderlineProfile},
    display_list::{Command, DisplayList},
    inline_lines::GlyphBitmap,
    parse_style_tree, Limits,
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
fn tree(html: &str, allow: bool) -> BoxTree {
    let s = parse_style_tree(
        format!("<!doctype html><style>body{{margin:0}}p{{margin:0}}</style>{html}").as_bytes(), &[], &Limits::DEFAULT);
    if allow { BoxTree::from_style_with_text_decoration(&s) } else { BoxTree::from_style(&s) }
}
fn ul(html: &str) -> Result<DisplayList, ()> {
    paint_direct_text_blocks_underline(&tree(html, true), 100, 100, &Face, UnderlineProfile::ChromeLiberationSerif156).map_err(|_| ())
}
fn fills(d: &DisplayList) -> Vec<(i32, i32, u32, u32)> {
    d.commands().iter().filter_map(|c| match c { Command::Fill { rect, .. } => Some((rect.x, rect.y, rect.width, rect.height)), _ => None }).collect()
}
#[test]
fn rule_matches_recorded_chrome_rows() {
    // (size, offset below baseline row, thickness) from the Chrome 156 pixel table.
    for (s, off, th) in [(12u32, 1u32, 1u32), (14, 1, 1), (16, 1, 1), (20, 1, 2), (24, 2, 2), (32, 2, 3)] {
        let d = ul(&format!("<p style='font-size:{s}px;text-decoration:underline'>AB</p>")).unwrap();
        let f = fills(&d);
        let line = f.iter().find(|r| r.3 == th && r.2 == 4).expect("underline fill");
        assert_eq!(line.0, 0);
        assert_eq!(line.1, 7 + off as i32, "size {s}");
    }
}
#[test]
fn none_is_noop_and_default_painter_refuses_decorated() {
    let t = tree("<p style='text-decoration:none'>AB</p>", false);
    assert!(paint_direct_text_blocks(&t, 100, 100, &Face).is_ok());
    let t = tree("<p style='text-decoration:underline'>AB</p>", true);
    assert!(paint_direct_text_blocks(&t, 100, 100, &Face).is_err());
    assert!(tree("<p style='text-decoration:underline'>AB</p>", false).unsupported);
}
#[test]
fn refusals() {
    for h in [
        "<p style='font-size:16px;text-decoration:underline'><span>AB</span></p>",
        "<div style='text-decoration:underline'><p style='font-size:16px'>AB</p></div>",
        "<p style='font-size:16px;text-decoration:underline;font-weight:700'>AB</p>",
        "<p style='font-size:16px;text-decoration:underline;font-style:italic'>AB</p>",
        "<p style='font-size:16px;text-decoration:underline'>A~B</p>",
        "<p style='font-size:16px;text-decoration:underline'>AB<br>CD</p>",
        "<p style='font-size:11px;text-decoration:underline'>AB</p>",
        "<p style='font-size:40px;text-decoration:underline'>AB</p>",
        "<p style='font-size:16px;text-decoration:underline;text-align:center'>AB</p>",
        "<p style='font-size:16px;width:6px;text-decoration:underline'>AB CD EF</p>",
    ] {
        assert!(ul(h).is_err(), "{h}");
    }
}
#[test]
fn other_forms_stay_refused() {
    for v in ["overline", "line-through", "underline overline", "underline red", "underline dotted", "none underline", "inherit x"] {
        let s = parse_style_tree(format!("<p style='text-decoration:{v}'>A</p>").as_bytes(), &[], &Limits::DEFAULT);
        assert!(s.unsupported, "{v}");
    }
}
#[test]
fn skip_ink_chars_and_unmeasured_chars_refuse_but_plain_letters_pass() {
    for ch in ["g", "j", "p", "q", "y", "Q", ",", ";", "(", ")", "[", "]", "@", "é", "~", "$"] {
        assert!(ul(&format!("<p style='font-size:16px;text-decoration:underline'>A{ch}B</p>")).is_err(), "{ch}");
    }
    assert!(ul("<p style='font-size:16px;text-decoration:underline'>AZaz09 .:!?-</p>").is_ok());
}
#[test]
fn overflow_beyond_viewport_refuses_or_stays_inside() {
    // 100px-wide viewport, 60 glyphs x 2px = 120px on one line: must not paint a line past the page silently.
    let long = "A".repeat(60);
    if let Ok(d) = ul(&format!("<p style='font-size:16px;text-decoration:underline'>{long}</p>")) {
        assert!(fills(&d).iter().all(|r| r.0 >= 0 && r.0 + r.2 as i32 <= 100), "{:?}", fills(&d));
    }
}
struct NoId;
impl FontProvider for NoId {
    fn advance(&self, _: char, _: u32, _: bool) -> Option<u32> { Some(2) }
    fn line_height(&self, _: u32, _: bool) -> Option<u32> { Some(10) }
    fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> { Some(7) }
    fn raster(&self, ch: char, s: u32, b: bool) -> Option<GlyphBitmap> { Face.raster(ch, s, b) }
}
struct WrongId;
impl FontProvider for WrongId {
    fn face_sha256_hex(&self) -> Option<String> { Some("0".repeat(64)) }
    fn advance(&self, _: char, _: u32, _: bool) -> Option<u32> { Some(2) }
    fn line_height(&self, _: u32, _: bool) -> Option<u32> { Some(10) }
    fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> { Some(7) }
    fn raster(&self, ch: char, s: u32, b: bool) -> Option<GlyphBitmap> { Face.raster(ch, s, b) }
}
#[test]
fn unbound_or_wrong_face_refuses_at_library_boundary() {
    let t = tree("<p style='font-size:16px;text-decoration:underline'>AB</p>", true);
    let p = UnderlineProfile::ChromeLiberationSerif156;
    assert!(paint_direct_text_blocks_underline(&t, 100, 100, &Face, p).is_ok());
    assert!(paint_direct_text_blocks_underline(&t, 100, 100, &NoId, p).is_err());
    assert!(paint_direct_text_blocks_underline(&t, 100, 100, &WrongId, p).is_err());
}
#[test]
fn underscore_refuses_as_skip_ink_unproven() {
    assert!(ul("<p style='font-size:16px;text-decoration:underline'>A_B</p>").is_err());
}
