//! Uniform non-solid rings: complete endpoints make same-colour joins exact.
use kobo_web_document::{
    box_tree::BoxTree, css_background::paint_backgrounds, display_list::Rgb, parse_style_tree,
    Limits,
};
fn paint(css: &str) -> Result<Vec<u8>, kobo_web_document::css_paint_bridge::BridgeError> {
    let h=format!("<!doctype html><style>html,body{{margin:0;height:180px}}div{{position:absolute;left:3px;top:5px;background:#ffa500;background-clip:padding-box;{css}}}</style><div></div>");
    let t = BoxTree::from_style(&parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT));
    Ok(paint_backgrounds(&t, 180, 180)?
        .rasterize(180, 180, Rgb(255, 255, 255))
        .unwrap())
}
#[test]
fn uniform_rectangular_rings_have_exact_corners_gaps_and_interiors() {
    for kind in ["dashed", "dotted"] {
        for w in [1_usize, 2, 3, 4, 6, 10] {
            if kind == "dotted" && w > 3 {
                continue;
            }
            let (d, g) = if kind == "dotted" {
                (w, w)
            } else if w < 3 {
                (3 * w, 2 * w)
            } else {
                (2 * w, w)
            };
            for (nx, ny) in [(2, 3), (3, 5), (5, 2), (4, 4)] {
                let width = nx * d + (nx - 1) * g;
                let height = ny * d + (ny - 1) * g;
                let p = paint(&format!(
                    "width:{}px;height:{}px;border:{w}px {kind} blue",
                    width - 2 * w,
                    height - 2 * w
                ))
                .unwrap();
                for y in 0..180 {
                    for x in 0..180 {
                        let inside = x >= 3 && x < 3 + width && y >= 5 && y < 5 + height;
                        let (rx, ry) = (x.saturating_sub(3), y.saturating_sub(5));
                        let ink = inside
                            && (((ry < w || ry >= height - w) && rx % (d + g) < d)
                                || ((rx < w || rx >= width - w) && ry % (d + g) < d));
                        let expected = if ink {
                            [0, 0, 255]
                        } else if inside && rx >= w && rx < width - w && ry >= w && ry < height - w
                        {
                            [255, 165, 0]
                        } else {
                            [255, 255, 255]
                        };
                        assert_eq!(
                            &p[(y * 180 + x) * 4..][..3],
                            expected,
                            "{kind}/{w}/{nx}/{ny}@{x},{y}"
                        );
                    }
                }
            }
        }
    }
}
#[test]
fn rings_resolve_currentcolor_and_style_reset_without_colour_guess() {
    let expected = paint("width:18px;height:18px;border:3px dashed blue").unwrap();
    for css in ["width:18px;height:18px;color:blue;border:3px dashed", "width:18px;height:18px;border:3px solid blue;border-style:dashed", "width:18px;height:18px;border:3px dashed blue;border-style:initial;border-style:dashed!important"] {
  assert_eq!(paint(css).unwrap(),expected);
 }
}
#[test]
fn unproven_ring_joins_and_fit_refuse_whole() {
    for css in [
        "border:3px dashed blue;border-left-width:0",
        "border:3px dashed blue;border-left-width:2px",
        "border:3px dashed blue;border-left-style:solid",
        "border:3px dashed blue;border-left-color:red",
        "border:3px dashed blue;border-left-color:transparent",
        "border:4px dotted blue",
        "border:3px dashed rgba(0,0,255,.5)",
        "border:3px dashed blue;width:19px",
        "border:3px dashed blue;height:19px",
        "border:1.5px dashed blue",
        "border:3px dashed blue;border-radius:1px",
    ] {
        assert!(
            paint(&format!("width:18px;height:18px;{css}")).is_err(),
            "{css}"
        );
    }
}
#[test]
fn ring_command_exhaustion_refuses_whole() {
    assert!(paint("width:2999px;height:2999px;border:1px dotted blue").is_err());
}

#[test]
fn visual_ring_and_rectangle_sources_render() {
    for (html, height) in [
        (
            include_bytes!("fixtures/uniform-pattern-ring-lab.html").as_slice(),
            600,
        ),
        (
            include_bytes!("fixtures/uniform-pattern-rect-lab.html").as_slice(),
            400,
        ),
    ] {
        let t = BoxTree::from_style(&parse_style_tree(html, &[], &Limits::DEFAULT));
        assert!(paint_backgrounds(&t, 800, height).is_ok());
    }
}
