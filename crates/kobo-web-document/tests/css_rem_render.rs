//! Synthetic renderer-path proof for `font-size:<n>rem` (not real-page evidence).
//! A size-dependent font makes the rasterized pixels depend on the resolved px size.
use kobo_web_document::{
    box_tree::BoxTree,
    css_text_page::{paint_single_text_page, FontProvider},
    display_list::Rgb,
    inline_lines::GlyphBitmap,
    parse_style_tree, Limits,
};
struct SizeFace;
impl FontProvider for SizeFace {
    fn advance(&self, _: char, s: u32, _: bool) -> Option<u32> { Some(s / 2) }
    fn line_height(&self, s: u32, _: bool) -> Option<u32> { Some(s + s / 2) }
    fn baseline_offset(&self, s: u32, _: bool) -> Option<i32> { Some(s as i32) }
    fn raster(&self, c: char, s: u32, _: bool) -> Option<GlyphBitmap> {
        if c == ' ' {
            return Some(GlyphBitmap { left: 0, top: 0, width: 0, height: 0, coverage: vec![] });
        }
        Some(GlyphBitmap { left: 0, top: -(s as i32), width: s / 2, height: s, coverage: vec![255; (s / 2 * s) as usize] })
    }
}
fn page(html_attr: &str, p_attr: &str) -> Vec<u8> {
    format!("<!doctype html><style>body{{margin:0}}p{{margin:0}}</style><html style='height:100px;{html_attr}'><body><p style='{p_attr}'>abc</p></body></html>").into_bytes()
}
fn render(bytes: &[u8]) -> Option<Vec<u8>> {
    let style = parse_style_tree(bytes, &[], &Limits::DEFAULT);
    let boxes = BoxTree::from_style(&style);
    let list = paint_single_text_page(&boxes, 100, 100, &SizeFace).ok()?;
    Some(list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap())
}
fn ink(p: &[u8]) -> usize { p.chunks(4).filter(|c| c[0] != 255).count() }
#[test]
fn rem_renders_pixel_identical_to_resolved_px() {
    for (root, rem, px) in [("", "1.5rem", "24px"), ("font-size:20px", "1.5rem", "30px"), ("font-size:62.5%", "2rem", "20px"), ("", ".5rem", "8px")] {
        let a = render(&page(root, &format!("font-size:{rem}"))).expect("rem page renders");
        let b = render(&page(root, &format!("font-size:{px}"))).expect("px page renders");
        assert_eq!(a, b, "{root} {rem} vs {px}");
        assert!(ink(&a) > 0, "{rem}");
    }
    // distinct sizes really differ (the face is size-dependent), so identity is meaningful
    let big = render(&page("", "font-size:2rem")).unwrap();
    let small = render(&page("", "font-size:1rem")).unwrap();
    assert_ne!(big, small);
    assert!(ink(&big) > ink(&small));
}
#[test]
fn fractional_rem_is_not_rounded_and_paint_path_refuses() {
    // 1.4rem = 22.4px: must not equal 22px or 23px output, and the paint path refuses.
    for bad in ["font-size:1.4rem", "font-size:1.rem"] {
        let bytes = page("", bad);
        assert!(parse_style_tree(&bytes, &[], &Limits::DEFAULT).unsupported, "{bad}");
        assert!(render(&bytes).is_none(), "{bad}");
    }
}
#[test]
fn dump_ppm_when_requested() {
    let Ok(dir) = std::env::var("REM_DUMP") else { return };
    for (name, root, size) in [("rem24", "", "1.5rem"), ("px24", "", "24px"), ("rem30_root20", "font-size:20px", "1.5rem"), ("px30", "", "30px")] {
        if let Some(p) = render(&page(root, &format!("font-size:{size}"))) {
            let mut out = b"P6\n100 100\n255\n".to_vec();
            for c in p.chunks(4) { out.extend_from_slice(&c[..3]); }
            std::fs::write(format!("{dir}/{name}.ppm"), out).unwrap();
        }
    }
}
