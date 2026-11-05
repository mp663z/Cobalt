use kobo_web_document::{
    box_tree::BoxTree,
    computed_style::TextAlign,
    css_text_page::{paint_direct_text_blocks, FontProvider},
    display_list::Rgb,
    inline_lines::{place_runs_text, GlyphBitmap, LineError},
    parse_style_tree, Limits,
};
struct Face;
impl FontProvider for Face {
    fn advance(&self, _: char, _: u32, bold: bool) -> Option<u32> {
        Some(if bold { 4 } else { 2 })
    }
    fn line_height(&self, _: u32, _: bool) -> Option<u32> {
        Some(10)
    }
    fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> {
        Some(7)
    }
    fn raster(&self, ch: char, _: u32, _: bool) -> Option<GlyphBitmap> {
        Some(GlyphBitmap {
            left: 0,
            top: -3,
            width: u32::from(ch != ' '),
            height: if ch == ' ' { 0 } else { 2 },
            coverage: if ch == ' ' { vec![] } else { vec![255; 2] },
        })
    }
}
fn tree(html: &str) -> BoxTree {
    BoxTree::from_style(&parse_style_tree(
        format!("<!doctype html><style>body{{margin:0}}p{{margin:0}}</style>{html}").as_bytes(),
        &[],
        &Limits::DEFAULT,
    ))
}
fn positions(html: &str, width: u32) -> Result<Vec<(char, u32, u32, bool)>, LineError> {
    let t = tree(html);
    let p = t
        .boxes
        .iter()
        .position(|b| b.children.iter().any(|&c| t.boxes[c].text.is_some()))
        .unwrap();
    place_runs_text(
        &t,
        p,
        width,
        |_, _, bold| Some(if bold { 4 } else { 2 }),
        |_| Some(10),
    )
    .map(|lines| {
        lines
            .lines
            .glyphs
            .iter()
            .map(|g| (g.character, g.x, g.line, g.bold))
            .collect()
    })
}
#[test]
fn explicit_alignment_uses_advance_extent_not_ink() {
    for (align, x) in [("left", 0), ("right", 16), ("center", 8)] {
        let g = positions(&format!("<p style='text-align:{align}'>ab</p>"), 20).unwrap();
        assert_eq!((g[0].1, g[1].1), (x, x + 2));
    }
}
#[test]
fn wrapped_lines_align_independently_and_trim_boundary_spaces() {
    let g = positions("<p style='text-align:right'>  ab   cde  f  </p>", 10).unwrap();
    assert_eq!(
        g,
        vec![
            ('a', 6, 0, false),
            ('b', 8, 0, false),
            ('c', 0, 1, false),
            ('d', 2, 1, false),
            ('e', 4, 1, false),
            (' ', 6, 1, false),
            ('f', 8, 1, false)
        ]
    );
    let g = positions("<p style='text-align:center'>ab cde</p>", 10).unwrap();
    assert_eq!((g[0].1, g[2].1), (3, 2));
}
#[test]
fn inherited_alignment_and_inline_override_have_block_scope() {
    let g = positions("<div style='text-align:right'><p>ab</p></div>", 20).unwrap();
    assert_eq!(g[0].1, 16);
    let g = positions(
        "<p style='text-align:center'>a<span style='text-align:right'>b</span></p>",
        20,
    )
    .unwrap();
    assert_eq!((g[0].1, g[1].1), (8, 10));
}
#[test]
fn css_wide_keywords_and_importance_keep_existing_cascade() {
    for (value, expected) in [
        ("inherit", TextAlign::Right),
        ("unset", TextAlign::Right),
        ("initial", TextAlign::Initial),
        ("revert", TextAlign::Right),
    ] {
        let t = tree(&format!(
            "<div style='text-align:right'><p style='text-align:{value}'>ab</p></div>"
        ));
        let p = t
            .boxes
            .iter()
            .find(|b| b.children.iter().any(|&c| t.boxes[c].text.is_some()))
            .unwrap();
        assert_eq!(p.style.text_align, expected);
    }
    let g = positions(
        "<p style='text-align:right!important;text-align:left'>ab</p>",
        20,
    )
    .unwrap();
    assert_eq!(g[0].1, 16);
}
#[test]
fn fractional_center_refuses_instead_of_rounding() {
    assert_eq!(
        positions("<p style='text-align:center'>ab</p>", 19),
        Err(LineError::FractionalAlignment)
    );
    let t = tree("<p style='width:19px;text-align:center'>ab</p>");
    assert!(paint_direct_text_blocks(&t, 40, 40, &Face).is_err());
}
#[test]
fn mixed_bold_advances_control_offset() {
    let g = positions("<p style='text-align:center'>a<strong>b</strong>c</p>", 20).unwrap();
    assert_eq!(
        g,
        vec![('a', 6, 0, false), ('b', 8, 0, true), ('c', 12, 0, false)]
    );
}
#[test]
fn unsupported_values_and_rtl_remain_whole_page_refusals() {
    for value in [
        "justify",
        "start",
        "end",
        "match-parent",
        "'x'",
        "center right",
    ] {
        let t = tree(&format!("<p style='text-align:{value}'>ab</p>"));
        assert!(t.unsupported);
        assert!(paint_direct_text_blocks(&t, 40, 40, &Face).is_err());
    }
    let t = tree("<p style='direction:rtl;text-align:right'>ab</p>");
    assert!(paint_direct_text_blocks(&t, 40, 40, &Face).is_err());
}
#[test]
fn shifted_text_respects_content_origin_padding_and_border() {
    let t=tree("<p style='width:20px;padding-left:4px;padding-right:2px;border:2px solid red;text-align:right'>ab</p>");
    let list = paint_direct_text_blocks(&t, 40, 40, &Face).unwrap();
    let rgba = list.rasterize(40, 40, Rgb(255, 255, 255)).unwrap();
    let pixel = |x: usize, y: usize| &rgba[(y * 40 + x) * 4..(y * 40 + x) * 4 + 3];
    assert_eq!(pixel(22, 6), [0, 0, 0]);
    assert_eq!(pixel(24, 6), [0, 0, 0]);
    assert_eq!(pixel(21, 6), [255, 255, 255]);
    assert_eq!(pixel(0, 6), [255, 0, 0]);
}
#[test]
fn empty_text_and_unbreakable_words_do_not_gain_new_layout_paths() {
    assert!(positions("<p style='text-align:center'>   </p>", 20)
        .unwrap()
        .is_empty());
    assert_eq!(
        positions("<p style='text-align:right'>unbreakable</p>", 4),
        Err(LineError::UnbreakableWord)
    );
}
