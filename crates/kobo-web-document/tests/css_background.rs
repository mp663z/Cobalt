use kobo_web_document::{
    box_tree::BoxTree,
    css_background::paint_backgrounds,
    display_list::{Command, Rect, Rgb},
    parse_style_tree, Limits,
};

fn standards_tree(html: &[u8]) -> kobo_web_document::style_tree::StyleTree {
    let mut bytes = b"<!doctype html>".to_vec();
    bytes.extend_from_slice(html);
    parse_style_tree(&bytes, &[], &Limits::DEFAULT)
}

#[test]
fn visual_paint_refuses_quirks_and_limited_quirks() {
    for html in [
        "<html style='height:100px;background:red'><body style='height:80px'></body></html>",
        "<!doctype html public '-//W3C//DTD HTML 4.01 Transitional//EN'><html style='height:100px;background:red'><body style='height:80px'></body></html>",
    ] {
        let styled = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
        assert!(styled.quirks, "{html}");
        let boxes = BoxTree::from_style(&styled);
        assert!(boxes.quirks);
        assert!(paint_backgrounds(&boxes, 100, 100).is_err(), "{html}");
    }
    let styled = standards_tree(
        b"<html style='height:100px;background:red'><body style='height:80px'></body></html>",
    );
    assert!(!styled.quirks);
    assert!(paint_backgrounds(&BoxTree::from_style(&styled), 100, 100).is_ok());
}

#[test]
fn visual_paint_refuses_legacy_decoding_and_truncated_utf8() {
    let prefix = b"<html style='height:100px'><body style='height:80px'><main style='height:20px;background-color:red'></main>";
    let mut legacy = prefix.to_vec();
    legacy.push(0xe9);
    let styled = parse_style_tree(&legacy, &[], &Limits::DEFAULT);
    assert!(styled.unsupported);
    let mut partial = prefix.to_vec();
    partial.push(0xe2);
    let styled = parse_style_tree(&partial, &[], &Limits::DEFAULT);
    assert!(styled.unsupported);
    let exact = parse_style_tree(prefix, &[], &Limits::DEFAULT);
    assert!(!exact.unsupported);
}

#[test]
fn canvas_backgrounds_cover_viewport_before_local_boxes() {
    for html in [
        "<html style='height:100px;background:red'><body style='height:80px'><main style='height:20px;background:blue'></main></body></html>",
        "<html style='height:100px'><body style='height:80px;background:red'><main style='height:20px;background:blue'></main></body></html>",
    ] {
        let styled = standards_tree(html.as_bytes());
        let tree = BoxTree::from_style(&styled);
        let list = paint_backgrounds(&tree, 100, 100).unwrap();
        assert_eq!(list.commands().len(), 2, "{html}");
        assert!(matches!(list.commands()[0], Command::Fill { rect: Rect { x:0, y:0, width:100, height:100 }, color: Rgb(255,0,0), .. }));
        let pixels = list.rasterize(100, 100, Rgb(255,255,255)).unwrap();
        assert_eq!(&pixels[0..4], &[0,0,255,255]);
        assert_eq!(&pixels[(90*100*4)..(90*100*4+4)], &[255,0,0,255]);
    }
    // When root has its own canvas color, the body background stays local.
    let html = "<html style='height:100px;background:green'><body style='height:40px;background:red'></body></html>";
    let styled = standards_tree(html.as_bytes());
    let list = paint_backgrounds(&BoxTree::from_style(&styled), 100, 100).unwrap();
    assert_eq!(list.commands().len(), 2);
    let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
    assert_eq!(&pixels[0..4], &[255, 0, 0, 255]);
    assert_eq!(&pixels[90 * 100 * 4..90 * 100 * 4 + 4], &[0, 128, 0, 255]);
}

#[test]
fn retained_color_paints_only_proven_content_rectangles() {
    let html = "<html style='height:100px'><body style='height:80px'><main style='width:50px;height:20px;background-color:#123456'></main></body></html>";
    let styled = standards_tree(html.as_bytes());
    let tree = BoxTree::from_style(&styled);
    let list = paint_backgrounds(&tree, 100, 100).unwrap();
    assert_eq!(list.commands().len(), 1);
    assert!(matches!(
        list.commands()[0],
        Command::Fill {
            rect: Rect {
                x: 0,
                y: 0,
                width: 50,
                height: 20
            },
            color: Rgb(0x12, 0x34, 0x56),
            ..
        }
    ));
    let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
    assert_eq!(&pixels[0..4], &[0x12, 0x34, 0x56, 255]);
    assert_eq!(
        &pixels[(21 * 100 * 4)..(21 * 100 * 4 + 4)],
        &[255, 255, 255, 255]
    );
}

#[test]
fn single_color_background_shorthand_paints_only_supported_rectangles() {
    let html = "<html style='height:100px'><body style='height:80px'><main style='height:20px;background:rgb(5,6,7)'></main></body></html>";
    let styled = standards_tree(html.as_bytes());
    let tree = BoxTree::from_style(&styled);
    let list = paint_backgrounds(&tree, 100, 100).unwrap();
    assert!(matches!(
        list.commands(),
        [Command::Fill {
            color: Rgb(5, 6, 7),
            ..
        }]
    ));
}

#[test]
fn opaque_named_and_integer_rgb_colors_paint_exact_channels() {
    for (css, expected) in [
        ("green", Rgb(0, 128, 0)),
        ("lime", Rgb(0, 255, 0)),
        ("rgb(10,20,255)", Rgb(10, 20, 255)),
    ] {
        let html = format!("<html style='height:100px'><body style='height:80px'><main style='height:20px;background-color:{css}'></main></body></html>");
        let styled = standards_tree(html.as_bytes());
        let tree = BoxTree::from_style(&styled);
        let list = paint_backgrounds(&tree, 100, 100).unwrap();
        assert!(matches!(list.commands(), [Command::Fill { color, .. }] if *color == expected));
    }
}

#[test]
fn unknown_css_and_text_are_not_passed_off_as_rendered() {
    for html in [
        "<html style='height:100px'><body style='height:80px'><main style='height:20px;background-color:rgba(255,0,0,.5)'></main></body></html>",
        "<html style='height:100px'><body style='height:80px'><main style='height:20px;background-color:#123456'>Text</main></body></html>",
    ] {
        let styled = standards_tree(html.as_bytes());
        let tree = BoxTree::from_style(&styled);
        assert!(paint_backgrounds(&tree, 100, 100).is_err(), "{html}");
    }
}

#[test]
fn padded_background_covers_padding_box_and_stacks_after_bottom_padding() {
    let html = "<html style='height:100px'><body style='height:80px;padding-top:3px'><main style='width:50px;height:20px;padding-left:5px;padding-right:7px;padding-top:4px;padding-bottom:6px;background-color:#123456'></main><section style='height:10px;width:20px;margin-top:8px;background-color:#abcdef'></section></body></html>";
    let styled = standards_tree(html.as_bytes());
    let tree = BoxTree::from_style(&styled);
    let list = paint_backgrounds(&tree, 100, 100).unwrap();
    let rects: Vec<_> = list
        .commands()
        .iter()
        .filter_map(|command| match command {
            Command::Fill { rect, .. } => Some(*rect),
            _ => None,
        })
        .collect();
    assert_eq!(
        rects,
        [
            Rect {
                x: 0,
                y: 3,
                width: 62,
                height: 30
            },
            Rect {
                x: 0,
                y: 41,
                width: 20,
                height: 10
            }
        ]
    );
    let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
    let at = |x: usize, y: usize| &pixels[((y * 100 + x) * 4)..((y * 100 + x) * 4 + 4)];
    assert_eq!(at(61, 32), &[0x12, 0x34, 0x56, 255]);
    assert_eq!(at(62, 32), &[255, 255, 255, 255]);
    assert_eq!(at(0, 41), &[0xab, 0xcd, 0xef, 255]);
}

#[test]
fn malformed_arena_is_refused_before_geometry_passes() {
    for defect in 0..7 {
        let mut boxes = BoxTree::from_style(&standards_tree(
            b"<html style='height:100px'><body style='height:80px;background:red'></body></html>",
        ));
        match defect {
            0 => boxes.boxes[0].children = vec![usize::MAX],
            1 => boxes.boxes[1].parent = Some(usize::MAX),
            2 => boxes.boxes[1].children.push(0),
            3 => {
                let child = boxes.boxes[0].children[0];
                boxes.boxes[0].children.push(child);
            }
            4 => boxes.boxes[0].children.clear(),
            5 => boxes.boxes[0].source = None,
            _ => boxes.roots[0] = usize::MAX,
        }
        assert!(
            paint_backgrounds(&boxes, 100, 100).is_err(),
            "defect {defect}"
        );
    }
}

#[test]
fn empty_list_item_still_requires_marker_paint() {
    let tree = BoxTree::from_style(&standards_tree(b"<html style='height:100px'><body style='height:80px'><ul style='height:40px'><li style='height:20px;background:red'></li></ul></body></html>"));
    assert!(paint_backgrounds(&tree, 100, 100).is_err());
}

#[test]
fn a_display_none_root_neither_paints_nor_propagates_its_background() {
    let styled = standards_tree(b"<html style='background:red;display:none'></html>");
    let list = paint_backgrounds(&BoxTree::from_style(&styled), 100, 100).expect("blank canvas");
    let rgba = list
        .rasterize(100, 100, Rgb(255, 255, 255))
        .expect("raster");
    assert!(rgba.chunks_exact(4).all(|px| px[..3] == [255, 255, 255]));
}
