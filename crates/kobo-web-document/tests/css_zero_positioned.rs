//! Empty auto positioned containers: exact zero intrinsic size, not generic shrink-to-fit.
use kobo_web_document::{
    box_tree::{BoxTree, VerticalPass, WidthPass},
    css_background::paint_backgrounds,
    display_list::Rgb,
    parse_style_tree, Limits,
};
fn tree(css: &str, html: &str) -> BoxTree {
    BoxTree::from_style(&parse_style_tree(format!("<!doctype html><style>html,body{{margin:0;height:200px}}p{{height:20px;margin:0 0 7px}}main{{position:absolute}}div{{position:absolute;left:5px;top:7px;width:20px;height:10px;background:blue}}{css}</style>{html}").as_bytes(),&[],&Limits::DEFAULT))
}
#[test]
fn auto_container_is_zero_and_child_uses_true_static_flow_anchor() {
    let t = tree("", "<p></p><main><div></div></main>");
    let w = WidthPass::from_boxes(&t, 400);
    let v = VerticalPass::from_boxes(&t, 400, 200);
    assert_eq!(w.widths[3].unwrap().content, 0);
    assert_eq!((w.content_x[3], v.content_y[3]), (Some(0), Some(27)));
    assert_eq!((w.content_x[4], v.content_y[4]), (Some(5), Some(34)));
    assert!(paint_backgrounds(&t, 400, 200).is_ok());
}
#[test]
fn padding_content_origin_and_negative_right_bottom_use_zero_containing_block() {
    let t = tree(
        "body{padding-left:40px}div{left:auto;top:auto;right:3px;bottom:4px}",
        "<p></p><main><div></div></main>",
    );
    let w = WidthPass::from_boxes(&t, 400);
    let v = VerticalPass::from_boxes(&t, 400, 200);
    assert_eq!((w.content_x[3], v.content_y[3]), (Some(40), Some(27)));
    assert_eq!((w.content_x[4], v.content_y[4]), (Some(17), Some(13)));
    let p = paint_backgrounds(&t, 400, 200)
        .unwrap()
        .rasterize(400, 200, Rgb(255, 255, 255))
        .unwrap();
    assert_eq!(&p[(13 * 400 + 17) * 4..][..3], [0, 0, 255]);
}
#[test]
fn padded_static_ancestor_composes_child_offsets() {
    let t = tree(
        "section{padding:4px 6px;border:3px solid red;height:100px}div{left:5px;top:0px}",
        "<section><main><div></div></main></section>",
    );
    let w = WidthPass::from_boxes(&t, 400);
    let v = VerticalPass::from_boxes(&t, 400, 200);
    assert_eq!((w.content_x[4], v.content_y[4]), (Some(14), Some(7)));
    assert!(paint_backgrounds(&t, 400, 200).is_ok());
}
#[test]
fn unproven_auto_containers_refuse_whole() {
    for (css, html) in [
        ("main{left:0}", "<main><div></div></main>"),
        ("main{width:20px}", "<main><div></div></main>"),
        ("main{height:20px}", "<main><div></div></main>"),
        ("main{padding:1px}", "<main><div></div></main>"),
        ("main{border:1px solid blue}", "<main><div></div></main>"),
        ("main{background:red}", "<main><div></div></main>"),
        ("main{margin:1px}", "<main><div></div></main>"),
        ("main{direction:rtl}", "<main><div></div></main>"),
        ("div{position:static}", "<main><div></div></main>"),
        ("", "<main>text<div></div></main>"),
        ("", "<main><div><div></div></div></main>"),
        ("", "<main><img></main>"),
        ("div{left:auto}", "<main><div></div></main>"),
        ("", "<main></main>"),
        ("main{position:static;height:0;width:0}", "<main></main>"),
    ] {
        assert!(
            paint_backgrounds(&tree(css, html), 400, 200).is_err(),
            "{css} {html}"
        );
    }
}

#[test]
fn zero_container_visual_source_renders() {
    let t = BoxTree::from_style(&parse_style_tree(
        include_bytes!("fixtures/zero-positioned-lab.html"),
        &[],
        &Limits::DEFAULT,
    ));
    assert!(paint_backgrounds(&t, 400, 300).is_ok());
}
