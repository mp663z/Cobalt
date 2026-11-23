//! Two positioned levels, definite boxes, direct empty inner leaves only.
use kobo_web_document::{
    box_tree::{BoxTree, VerticalPass, WidthPass},
    css_background::paint_backgrounds,
    display_list::Rgb,
    parse_style_tree, Limits,
};
fn tree(css: &str, body: &str) -> BoxTree {
    BoxTree::from_style(&parse_style_tree(format!("<!doctype html><style>html,body{{margin:0;height:300px}}main{{position:absolute;left:30px;top:40px;width:160px;height:120px;padding:11px 13px;border:3px solid blue}}div{{position:absolute;width:20px;height:10px;padding:2px;border:2px solid green;left:5px;top:7px}}{css}</style>{body}").as_bytes(),&[],&Limits::DEFAULT))
}
#[test]
fn nested_offsets_use_padding_box_not_content_or_border_box() {
    let t = tree(
        "#b{left:auto;top:auto;right:9px;bottom:6px}",
        "<main><div></div><div id=b></div></main>",
    );
    assert!(!t.unsupported);
    let w = WidthPass::from_boxes(&t, 400);
    let v = VerticalPass::from_boxes(&t, 400, 300);
    assert!(!w.unsupported && !v.unsupported);
    // Content origins include the child's own 4px border+padding.
    assert_eq!((w.content_x[3], v.content_y[3]), (Some(42), Some(54)));
    assert_eq!((w.content_x[4], v.content_y[4]), (Some(186), Some(165)));
    assert!(paint_backgrounds(&t, 400, 300).is_ok());
}
#[test]
fn child_overflow_is_visible_and_negative_offsets_clip_only_at_viewport() {
    let t=tree("main{left:0px;top:0px;width:30px;height:30px;padding:0;border:0}div{left:-5px;top:-7px;border:0;padding:0;background:red}#b{left:25px;top:25px;background:blue}","<main><div></div><div id=b></div></main>");
    let p = paint_backgrounds(&t, 100, 100)
        .unwrap()
        .rasterize(100, 100, Rgb(255, 255, 255))
        .unwrap();
    let at = |x: usize, y: usize| &p[(y * 100 + x) * 4..][..3];
    assert_eq!(at(0, 0), [255, 0, 0]);
    assert_eq!(at(15, 0), [255, 255, 255]);
    assert_eq!(at(35, 30), [0, 0, 255]);
    assert_eq!(at(45, 30), [255, 255, 255]);
}
#[test]
fn child_border_box_sizing_and_opposite_offsets_preserve_precedence() {
    let t = tree(
        "div{box-sizing:border-box;width:20px;height:10px;left:5px;top:7px;right:99px;bottom:99px}",
        "<main><div></div></main>",
    );
    let w = WidthPass::from_boxes(&t, 400);
    let v = VerticalPass::from_boxes(&t, 400, 300);
    assert_eq!(w.widths[3].unwrap().content, 12);
    assert_eq!(w.content_x[3], Some(42));
    assert_eq!(v.content_y[3], Some(54));
    assert!(paint_backgrounds(&t, 400, 300).is_ok());
}
#[test]
fn unsupported_depth_flow_autosizes_and_offsets_refuse_whole() {
    for (css, body) in [
        ("", "<main><div><div></div></div></main>"),
        ("div{position:static}", "<main><div></div></main>"),
        ("main{width:auto}", "<main><div></div></main>"),
        ("main{height:auto}", "<main><div></div></main>"),
        ("div{left:auto}", "<main><div></div></main>"),
        ("div{top:auto}", "<main><div></div></main>"),
        ("div{left:1%}", "<main><div></div></main>"),
        ("div{left:1.5px}", "<main><div></div></main>"),
        ("div{width:auto}", "<main><div></div></main>"),
        ("div{margin:1px}", "<main><div></div></main>"),
        ("div{overflow:hidden}", "<main><div></div></main>"),
        ("div{z-index:1}", "<main><div></div></main>"),
        ("main{direction:rtl}", "<main><div></div></main>"),
    ] {
        let t = tree(css, body);
        assert!(paint_backgrounds(&t, 400, 300).is_err(), "{css} {body}");
    }
}

#[test]
fn overflowing_composed_coordinates_refuse_instead_of_wrapping() {
    for css in [
        "main{left:2147483647px}div{left:10px}",
        "main{top:2147483647px}div{top:10px}",
    ] {
        let t = tree(css, "<main><div></div></main>");
        assert!(paint_backgrounds(&t, 400, 300).is_err());
    }
}
#[test]
fn visual_nested_source_renders_with_uniform_ring_children() {
    let t = BoxTree::from_style(&parse_style_tree(
        include_bytes!("fixtures/nested-absolute-lab.html"),
        &[],
        &Limits::DEFAULT,
    ));
    assert!(paint_backgrounds(&t, 500, 500).is_ok());
}
