//! Synthetic provider: explicit odd line-height is admitted only when the provider proves the
//! normal strut's line gap, and then follows Chrome's floor(line-height - (ascent + descent)) / 2.
//! Chrome-measured agreement lives in the private lh78 evidence; this fixes the arithmetic.
use kobo_web_document::{
    box_tree::BoxTree,
    css_text_page::{paint_direct_text_blocks, FontProvider, NormalLineMetrics},
    display_list::Rgb,
    inline_lines::GlyphBitmap,
    parse_style_tree, Limits,
};
// ascent A=13, descent D=4, gap g: natural = 17 + g, paired baseline = A + g/2.
struct Face { gap: Option<u32>, strut_height_off: bool }
impl FontProvider for Face {
    fn normal_line_metrics(&self, _: u32, _: bool) -> Option<NormalLineMetrics> {
        let g = self.gap?;
        Some(NormalLineMetrics { height: 17 + g + u32::from(self.strut_height_off), baseline: i32::try_from(13 + g / 2).unwrap() })
    }
    fn normal_line_gap(&self, _: u32, _: bool) -> Option<u32> { self.gap }
    fn advance(&self, _: char, _: u32, _: bool) -> Option<u32> { Some(3) }
    fn line_height(&self, _: u32, _: bool) -> Option<u32> { Some(17 + self.gap.unwrap_or(1)) }
    fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> { Some(13) }
    fn raster(&self, c: char, _: u32, _: bool) -> Option<GlyphBitmap> {
        Some(if c == ' ' { GlyphBitmap { left: 0, top: 0, width: 0, height: 0, coverage: vec![] } }
        else { GlyphBitmap { left: 0, top: -1, width: 1, height: 1, coverage: vec![255] } })
    }
}
fn ink_row(lh: u32, face: &Face) -> Option<usize> {
    let t = BoxTree::from_style(&parse_style_tree(format!("<!doctype html><style>html,body{{margin:0}}p{{margin:0;line-height:{lh}px}}</style><p>ab</p>").as_bytes(), &[], &Limits::DEFAULT));
    let p = paint_direct_text_blocks(&t, 100, 100, face).ok()?.rasterize(100, 100, Rgb(255, 255, 255)).ok()?;
    (0..100).find(|&y| p[(y * 100) * 4..][..12].chunks(4).any(|px| px[0] < 128) || (0..100).any(|x| p[(y * 100 + x) * 4] < 128))
}
#[test]
fn odd_leading_floors_with_gap_proof_and_even_is_unchanged() {
    for g in [0u32, 1, 2, 3] {
        let f = Face { gap: Some(g), strut_height_off: false };
        let natural = 17 + g;
        for lh in (natural.saturating_sub(5))..=(natural + 6) {
            let baseline = 13 + (i64::from(lh) - 17).div_euclid(2);
            let row = ink_row(lh, &f).unwrap_or_else(|| panic!("refused g {g} lh {lh}"));
            assert_eq!(row as i64, baseline - 1, "g {g} lh {lh}");
        }
    }
}
#[test]
fn odd_leading_refuses_without_gap_proof_or_with_disagreeing_strut() {
    let legacy = Face { gap: None, strut_height_off: false };
    let off = Face { gap: Some(1), strut_height_off: true };
    for lh in [19u32, 21, 23] { // natural 18 for legacy (gap 1 default): odd leading
        assert!(ink_row(lh, &legacy).is_none(), "legacy odd lh {lh}");
        assert!(ink_row(lh, &off).is_none(), "disagreeing strut odd lh {lh}");
    }
    for lh in [18u32, 20, 22] {
        assert!(ink_row(lh, &legacy).is_some(), "legacy even lh {lh}");
    }
}
