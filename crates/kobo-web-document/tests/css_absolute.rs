use kobo_web_document::{
    box_tree::{BoxTree, UsedHeightPass, VerticalPass, WidthPass},
    css_background::paint_backgrounds,
    display_list::Rgb,
    parse_style_tree, Limits,
};
fn tree(css: &str, body: &str) -> BoxTree {
    BoxTree::from_style(&parse_style_tree(format!("<!doctype html><style>html{{height:80px}}body{{margin:0;height:80px}}{css}</style><html><body>{body}</body></html>").as_bytes(),&[],&Limits::DEFAULT))
}
#[test]
fn positioned_leaf_removed_from_sibling_flow_and_painted_last() {
    let tree=tree("main{width:10px;height:10px;background:red}div{position:absolute;top:0px;left:0px;width:20px;height:20px;background:blue}section{width:10px;height:10px;background:yellow}","<main></main><div></div><section></section>");
    let list = paint_backgrounds(&tree, 80, 80).unwrap();
    let p = list.rasterize(80, 80, Rgb(255, 255, 255)).unwrap();
    assert_eq!(&p[..3], &[0, 0, 255]);
    assert_eq!(&p[(15 * 80 + 1) * 4..][..3], &[0, 0, 255]);
    assert_eq!(&p[(25 * 80 + 1) * 4..][..3], &[255, 255, 255]);
    let vertical = VerticalPass::from_boxes(&tree, 80, 80);
    assert_eq!(vertical.content_y[4], Some(10));
}
#[test]
fn opposite_offsets_use_initial_containing_block_and_box_edges() {
    let tree=tree("main{width:30px;height:30px;padding:5px}div{position:absolute;right:7px;bottom:9px;width:20px;height:10px;border:2px solid blue;padding:3px;background:red}","<main><div></div></main>");
    let widths = WidthPass::from_boxes(&tree, 80);
    let v = VerticalPass::from_boxes(&tree, 80, 80);
    assert_eq!(widths.content_x[3], Some(48));
    assert_eq!(v.content_y[3], Some(56));
    assert!(paint_backgrounds(&tree, 80, 80).is_ok());
}
#[test]
fn auto_offsets_share_static_anchor_without_advancing_flow() {
    let tree=tree("main{height:10px;margin-bottom:6px}div{position:absolute;width:20px;height:20px}section{height:10px}","<main></main><div></div><div></div><section></section>");
    let v = VerticalPass::from_boxes(&tree, 80, 80);
    assert!(!v.unsupported);
    assert_eq!(v.content_y[3], Some(16));
    assert_eq!(v.content_y[4], Some(16));
    assert_eq!(v.content_y[5], Some(16));
    let heights = UsedHeightPass::from_boxes(&tree, 80, 80);
    assert_eq!(heights.heights[1], Some(80));
}
#[test]
fn unsupported_position_shapes_do_not_turn_into_normal_flow() {
    for extra in [
        "width:auto",
        "height:auto",
        "top:1%",
        "left:1.5px",
        "direction:rtl",
        "position:fixed",
        "position:relative",
        "margin:1px",
        "z-index:2",
    ] {
        let t = tree(
            &format!("div{{position:absolute;width:20px;height:20px;{extra}}}"),
            "<div></div>",
        );
        assert!(paint_backgrounds(&t, 80, 80).is_err(), "{extra}");
    }
    for html in ["<div><section></section></div>", "<span></span>"] {
        let t = tree(
            "div,span{position:absolute;width:20px;height:20px}section{height:10px}",
            html,
        );
        assert!(paint_backgrounds(&t, 80, 80).is_err());
    }
}

#[test]
fn cascade_initial_unset_and_inherit_keep_position_noninherited() {
    use kobo_web_document::computed_style::{Margin, Position};
    for (css, absolute) in [
        ("div{position:absolute;position:initial}", false),
        ("div{position:absolute;position:unset}", false),
        (
            "div{position:static;position:absolute!important;position:static}",
            true,
        ),
        (
            "body{position:static;top:7px}div{position:absolute;top:inherit}",
            true,
        ),
    ] {
        let t = tree(
            &format!("div{{width:20px;height:20px}}{css}"),
            "<div></div>",
        );
        assert_eq!(
            t.boxes[2].style.positioned.position == Position::Absolute,
            absolute
        );
        if css.contains("top:inherit") {
            assert_eq!(t.boxes[2].style.positioned.offsets[0], Margin::Px(7));
        }
        assert!(paint_backgrounds(&t, 80, 80).is_ok());
    }
}

#[test]
fn negative_explicit_offsets_clip_in_primitive_not_wrap_unsigned() {
    let t = tree(
        "div{position:absolute;left:-5px;top:-7px;width:20px;height:20px;background:blue}",
        "<div></div>",
    );
    let list = paint_backgrounds(&t, 80, 80).unwrap();
    let p = list.rasterize(80, 80, Rgb(255, 255, 255)).unwrap();
    assert_eq!(&p[..3], [0, 0, 255]);
    assert_eq!(&p[(13 * 80) * 4..][..3], [255, 255, 255]);
    assert_eq!(&p[15 * 4..][..3], [255, 255, 255]);
}

#[test]
fn opaque_rgba_is_rgb_but_partial_or_malformed_alpha_refuses() {
    for color in [
        "rgba(255,165,0,1)",
        "rgba(255, 165, 0, 1.0)",
        "rgba(255,165,0,1.00)",
    ] {
        let t=tree(&format!("div{{position:absolute;left:0px;top:0px;width:20px;height:20px;background:{color}}}"),"<div></div>");
        let p = paint_backgrounds(&t, 80, 80)
            .unwrap()
            .rasterize(80, 80, Rgb(255, 255, 255))
            .unwrap();
        assert_eq!(&p[..3], [255, 165, 0]);
    }
    for color in [
        "rgba(255,165,0,.5)",
        "rgba(255,165,0,0)",
        "rgba(255,165,0,1,2)",
        "rgba(256,165,0,1)",
        "rgba(255,165,0)",
    ] {
        let t = tree(
            &format!("div{{width:20px;height:20px;background:{color}}}"),
            "<div></div>",
        );
        assert!(paint_backgrounds(&t, 80, 80).is_err(), "{color}");
    }
}
#[test]
fn visual_absolute_lab_sources_render() {
    for html in [
        include_bytes!("fixtures/absolute-explicit-lab.html").as_slice(),
        include_bytes!("fixtures/absolute-static-lab.html").as_slice(),
    ] {
        let arena = parse_style_tree(html, &[], &Limits::DEFAULT);
        assert!(paint_backgrounds(&BoxTree::from_style(&arena), 400, 320).is_ok());
    }
}
