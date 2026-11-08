//! Regression evidence for the implicit 1 CSS px = 1 device px contract.
use kobo_web_document::{
    box_tree::BoxTree, css_background::paint_backgrounds, display_list::Rgb, parse_style_tree,
    Limits, DEVICE_PX_PER_CSS_PX,
};

fn raster(html: &str, w: u32, h: u32) -> Vec<u8> {
    let html = format!("<!doctype html><style>html,body{{margin:0}}</style>{html}");
    let styled = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
    let tree = BoxTree::from_style(&styled);
    paint_backgrounds(&tree, w, h).unwrap().rasterize(w, h, Rgb(255, 255, 255)).unwrap()
}

#[test]
fn constant_is_one_and_documented_assumption_holds() {
    assert_eq!(DEVICE_PX_PER_CSS_PX, 1);
}

#[test]
fn a_20_by_10_css_box_covers_exactly_20_by_10_raster_pixels() {
    let page = "<body><div style=\"width:20px;height:10px;background:#00f\"></div></body>";
    let (w, h) = (40_u32, 30_u32);
    let px = raster(page, w, h);
    let blue = |x: u32, y: u32| px[((y * w + x) * 4) as usize..][..3] == [0, 0, 255];
    let count = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|&(x, y)| blue(x, y)).count();
    assert_eq!(count as u32, 20 * 10 * DEVICE_PX_PER_CSS_PX * DEVICE_PX_PER_CSS_PX);
    assert!(blue(19, 9) && !blue(20, 9) && !blue(19, 10));
}

#[test]
fn fractional_border_floor_is_the_1x_rule_with_min_one_device_px() {
    // 2.99 css px -> 2 device px at 1x; 0.5 -> 1 (min one device px).
    for (decl, device_px) in [("2.99px", 2_u32), ("0.5px", 1)] {
        let page = format!("<body><div style=\"width:10px;height:10px;border:{decl} solid #00f\"></div></body>");
        let (w, h) = (30_u32, 30_u32);
        let px = raster(&page, w, h);
        let blue = |x: u32, y: u32| px[((y * w + x) * 4) as usize..][..3] == [0, 0, 255];
        let want = device_px * DEVICE_PX_PER_CSS_PX;
        assert!(blue(0, 0) && blue(want - 1, 5), "{decl}");
        assert!(!blue(want, 5) || want >= 10, "{decl} ring thicker than {want}px");
    }
}
