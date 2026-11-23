//! delta10 reviewer follow-up (CSS 2.1 16.3.1): a decoration takes the colour of the element
//! that ESTABLISHED it. The painter pins exact-or-refuse for every shape where the
//! establishing element's resolved colour could diverge from the underline fill colour:
//! those shapes REFUSE the whole page (never a block-colour underline under divergent ink).
//! The single admitted underline shape (decorated block, direct text children only, no other
//! decorated box, no decorated ancestor) has the establishing element == the block itself,
//! and this file pins that the fill command really carries the block's resolved colour.
use kobo_web_document::{
    box_tree::BoxTree,
    css_text_page::{paint_direct_text_blocks_underline, FontProvider, UnderlineProfile},
    display_list::{Command, DisplayList, Rgb},
    inline_lines::GlyphBitmap,
    parse_style_tree_with_inline_sheets_and_selectors, parse_style_tree_with_text_decoration,
    Limits, SupplyBudget,
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
const P: UnderlineProfile = UnderlineProfile::ChromeLiberationSerif156;
fn ul(html: &str) -> Result<DisplayList, ()> {
    let s = parse_style_tree_with_text_decoration(
        format!("<!doctype html><style>body{{margin:0}}p{{margin:0}}</style>{html}").as_bytes(), &[], &Limits::DEFAULT);
    paint_direct_text_blocks_underline(&BoxTree::from_style_with_text_decoration(&s), 100, 100, &Face, P).map_err(|_| ())
}
#[test]
fn decorated_block_with_divergent_inline_colour_refuses_the_page() {
    // Block establishes the decoration in #010203; the blue inline child is a colour-varying
    // run under that decoration. Neither a block-colour underline under blue ink nor a
    // silently dropped decoration is admissible: the whole page refuses.
    for h in [
        "<p style='font-size:16px;text-decoration:underline;color:#010203'>x <span style='color:#0000ee'>t</span></p>",
        "<p style='font-size:16px;text-decoration:underline;color:#010203'><span style='color:#0000ee'>t</span></p>",
        "<p style='font-size:16px;text-decoration:underline;color:#010203'>x <b style='color:#0000ee'>t</b></p>",
    ] {
        assert!(ul(h).is_err(), "{h}");
    }
}
#[test]
fn ua_underlined_blue_link_in_a_black_container_refuses_the_page() {
    // The reviewer's literal case: the ext UA sheet gives a[href] colour rgb(0,0,238) and an
    // underline, inside a black block - the dominant real-world colour-varying line. Exact
    // paint would need the <a>'s own underline in ITS colour; the painter refuses instead.
    for h in [
        "<div style='font-size:16px;line-height:16px;color:#010203'>x <a href=u>t</a></div>",
        "<div style='font-size:16px;line-height:16px;color:#010203'><a href=u>t</a></div>",
        "<div style='font-size:16px;line-height:16px;color:#010203'>x <u>t</u></div>",
    ] {
        let s = parse_style_tree_with_inline_sheets_and_selectors(
            format!("<!doctype html><style>body{{margin:0}}</style>{h}").as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
        assert!(!s.unsupported, "style tree admits {h}");
        let bt = BoxTree::from_style_with_text_decoration(&s);
        assert!(!bt.unsupported, "box tree admits {h}");
        assert!(paint_direct_text_blocks_underline(&bt, 100, 100, &Face, P).is_err(), "underline entry refuses {h}");
    }
}
#[test]
fn admitted_shape_fill_colour_is_the_establishing_elements_colour() {
    // Admitted shape: the decorating block has direct text only, so the establishing element
    // IS the block. Pin the actual fill colour, not just rect geometry.
    let d = ul("<p style='font-size:16px;text-decoration:underline;color:#010203'>AB</p>").unwrap();
    let underlines: Vec<Rgb> = d.commands().iter().filter_map(|c| match c {
        Command::Fill { rect, color, .. } if rect.height <= 3 => Some(*color),
        _ => None,
    }).collect();
    assert_eq!(underlines, vec![Rgb(1, 2, 3)], "underline fill carries the establishing element's resolved colour");
    let d = ul("<p style='font-size:16px;text-decoration:underline;color:#0000ee'>AB</p>").unwrap();
    let underlines: Vec<Rgb> = d.commands().iter().filter_map(|c| match c {
        Command::Fill { rect, color, .. } if rect.height <= 3 => Some(*color),
        _ => None,
    }).collect();
    assert_eq!(underlines, vec![Rgb(0, 0, 238)], "a blue establishing element paints a blue underline");
}
