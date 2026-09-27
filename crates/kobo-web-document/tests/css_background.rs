use kobo_web_document::{
    box_tree::BoxTree,
    css_background::paint_backgrounds,
    display_list::{Command, Rect, Rgb},
    parse_style_tree, Limits,
};

#[test]
fn retained_color_paints_only_proven_content_rectangles() {
    let html = "<html style='height:100px'><body style='height:80px'><main style='width:50px;height:20px;background-color:#123456'></main></body></html>";
    let styled = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
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
fn unknown_css_and_text_are_not_passed_off_as_rendered() {
    for html in [
        "<html style='height:100px'><body style='height:80px'><main style='height:20px;background-color:red'></main></body></html>",
        "<html style='height:100px'><body style='height:80px'><main style='height:20px;background-color:#123456'>Text</main></body></html>",
    ] {
        let styled = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
        let tree = BoxTree::from_style(&styled);
        assert!(paint_backgrounds(&tree, 100, 100).is_err(), "{html}");
    }
}
