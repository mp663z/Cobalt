use kobo_web_document::css_paint_bridge;
pub use kobo_web_document::{box_tree, computed_style, display_list};

use box_tree::{BoxTree, VerticalPass, WidthPass};
use css_paint_bridge::{paint_content_rectangles, BridgeError, FillSpec, MAX_BRIDGE_BOXES};
use display_list::{Command, Rect, Rgb, Source, MAX_COMMANDS};
use kobo_web_document::{parse_style_tree, Limits};

fn setup(html: &str) -> (BoxTree, WidthPass, VerticalPass) {
    let styled = parse_style_tree(
        format!("<!doctype html>{html}").as_bytes(),
        &[],
        &Limits::DEFAULT,
    );
    let tree = BoxTree::from_style(&styled);
    let widths = WidthPass::from_boxes(&tree, 400);
    let vertical = VerticalPass::from_boxes(&tree, 400, 600);
    (tree, widths, vertical)
}

fn paint(
    tree: &BoxTree,
    widths: &WidthPass,
    vertical: &VerticalPass,
    fills: &[FillSpec],
) -> Result<display_list::DisplayList, BridgeError> {
    paint_content_rectangles(tree, widths, vertical, 400, 600, fills)
}

fn fill(box_index: usize, color: Rgb) -> FillSpec {
    FillSpec { box_index, color }
}

#[test]
fn content_paint_refuses_quirks_even_with_proven_geometry() {
    let styled = parse_style_tree(
        b"<html style='height:600px'><body style='height:300px;background:red'></body></html>",
        &[],
        &Limits::DEFAULT,
    );
    assert!(styled.quirks);
    let tree = BoxTree::from_style(&styled);
    assert_eq!(
        paint(
            &tree,
            &WidthPass::from_boxes(&tree, 400),
            &VerticalPass::from_boxes(&tree, 400, 600),
            &[]
        )
        .err(),
        Some(BridgeError::Unsupported)
    );
}

#[test]
fn exact_order_content_boxes_and_source_ids() {
    let (tree, widths, vertical) = setup("<style>html{height:600px}body{height:300px}main{width:100px;height:100px;margin-left:20px}section{width:25px;height:10px;margin-left:5px;margin-bottom:10px}article{width:40px;height:20px;margin-top:5px}</style><body><main><section></section><article></article></main></body>");
    let section = tree
        .boxes
        .iter()
        .position(|n| n.style.height == computed_style::Length::Px(10))
        .unwrap();
    let article = tree
        .boxes
        .iter()
        .position(|n| n.style.height == computed_style::Length::Px(20))
        .unwrap();
    let specs = [fill(article, Rgb(2, 3, 4)), fill(section, Rgb(7, 8, 9))];
    let list = paint(&tree, &widths, &vertical, &specs).unwrap();
    assert_eq!(
        list.commands(),
        [
            Command::Fill {
                rect: Rect {
                    x: 20,
                    y: 20,
                    width: 40,
                    height: 20
                },
                color: Rgb(2, 3, 4),
                source: Source {
                    node: tree.boxes[article].source,
                    action: None
                }
            },
            Command::Fill {
                rect: Rect {
                    x: 25,
                    y: 0,
                    width: 25,
                    height: 10
                },
                color: Rgb(7, 8, 9),
                source: Source {
                    node: tree.boxes[section].source,
                    action: None
                }
            },
        ]
    );
    assert_eq!(list.glyph_bytes(), 0);
}

#[test]
fn auto_block_background_uses_child_flow_height() {
    let (tree, widths, vertical) = setup("<style>html{height:600px}body{padding-top:7px;padding-bottom:3px;background-color:#123456}main{height:20px;padding-top:2px;padding-bottom:3px;margin-bottom:5px}section{height:10px;margin-top:8px}</style><body><main></main><section></section></body>");
    let body = tree
        .boxes
        .iter()
        .position(|b| b.style.background_color == Some(0x12_34_56))
        .unwrap();
    let list = css_paint_bridge::paint_padding_rectangles(
        &tree,
        &widths,
        &vertical,
        400,
        600,
        &[fill(body, Rgb(18, 52, 86))],
    )
    .unwrap();
    assert_eq!(
        list.commands(),
        [Command::Fill {
            rect: Rect {
                x: 0,
                y: 0,
                width: 400,
                height: 53
            },
            color: Rgb(18, 52, 86),
            source: Source {
                node: tree.boxes[body].source,
                action: None
            },
        }]
    );
}

#[test]
fn rejects_unproven_coordinates_and_selected_unknown_box() {
    let (tree, mut widths, mut vertical) =
        setup("<style>html{height:600px}body{height:300px}</style><body></body>");
    let specs = [fill(0, Rgb(0, 0, 0))];
    assert_eq!(
        paint(&tree, &widths, &vertical, &[fill(999, Rgb(0, 0, 0))]).err(),
        Some(BridgeError::InvalidGeometry)
    );
    widths.content_x[0] = Some(1);
    assert_eq!(
        paint(&tree, &widths, &vertical, &specs).err(),
        Some(BridgeError::Unsupported)
    );
    widths = WidthPass::from_boxes(&tree, 400);
    vertical.content_y[0] = Some(3);
    assert_eq!(
        paint(&tree, &widths, &vertical, &specs).err(),
        Some(BridgeError::Unsupported)
    );
    vertical = VerticalPass::from_boxes(&tree, 400, 600);
    assert_eq!(
        paint_content_rectangles(&tree, &widths, &vertical, 401, 600, &specs).err(),
        Some(BridgeError::Unsupported)
    );
    assert_eq!(
        paint_content_rectangles(&tree, &widths, &vertical, 0, 600, &specs).err(),
        Some(BridgeError::InvalidViewport)
    );
    assert_eq!(
        paint_content_rectangles(&tree, &widths, &vertical, 2000, 600, &specs).err(),
        Some(BridgeError::InvalidViewport)
    );
}

#[test]
fn unsupported_css_never_yields_partial_paint_even_empty_selection() {
    for html in [
        "<style>html{height:600px}body{height:300px}main{height:auto}</style><body><main></main></body>",
        "<style>html{height:600px}body{height:300px}main{height:20px;padding-top:2px}</style><body><main></main></body>",
        "<style>html{height:600px}body{height:300px}main{height:20px;border-top:2px solid red}</style><body><main></main></body>",
        "<style>html{height:600px}body{height:300px}main{height:20px;clear:both}</style><body><main></main></body>",
        "<style>html{height:600px}body{height:300px}main{height:20px}</style><body><main>text</main></body>",
        "<style>html{height:600px}body{height:300px}main{height:20px}</style><body><main><img src=x></main></body>",
    ] {
        let (tree, widths, vertical) = setup(html);
        assert_eq!(paint(&tree, &widths, &vertical, &[]).err(), Some(BridgeError::Unsupported), "{html}");
    }
    let (mut tree, widths, vertical) =
        setup("<style>html{height:600px}body{height:300px}</style><body></body>");
    tree.boxes[0].style.box_sizing = computed_style::BoxSizing::BorderBox;
    assert_eq!(
        paint(&tree, &widths, &vertical, &[]).err(),
        Some(BridgeError::Unsupported)
    );
}

#[test]
fn command_and_box_limits_reject_without_partial_list() {
    let (tree, widths, vertical) =
        setup("<style>html{height:600px}body{height:300px}</style><body></body>");
    let specs = vec![fill(0, Rgb(1, 2, 3)); MAX_COMMANDS];
    assert_eq!(
        paint(&tree, &widths, &vertical, &specs)
            .unwrap()
            .commands()
            .len(),
        MAX_COMMANDS
    );
    assert_eq!(
        paint(
            &tree,
            &widths,
            &vertical,
            &[specs, vec![fill(0, Rgb(1, 2, 3))]].concat()
        )
        .err(),
        Some(BridgeError::TooManyCommands)
    );
    let mut huge = tree;
    huge.boxes
        .resize(MAX_BRIDGE_BOXES + 1, huge.boxes[0].clone());
    assert_eq!(
        paint(&huge, &widths, &vertical, &[]).err(),
        Some(BridgeError::TooManyBoxes)
    );
}

#[test]
fn rejects_zero_width_and_truncated_input() {
    let (tree, widths, vertical) = setup("<style>html{height:600px}body{height:300px}main{width:0;height:20px}</style><body><main></main></body>");
    let main = tree
        .boxes
        .iter()
        .position(|node| node.style.height == computed_style::Length::Px(20))
        .unwrap();
    assert_eq!(
        paint(&tree, &widths, &vertical, &[fill(main, Rgb(5, 6, 7))]).err(),
        Some(BridgeError::InvalidGeometry)
    );
    let mut truncated = tree;
    truncated.truncated = true;
    assert_eq!(
        paint(&truncated, &widths, &vertical, &[]).err(),
        Some(BridgeError::Unsupported)
    );
}

#[test]
fn rejects_invalid_tree_structure() {
    let (mut tree, widths, vertical) =
        setup("<style>html{height:600px}body{height:300px}</style><body></body>");
    tree.boxes[0].children.push(9999);
    assert_eq!(
        paint(&tree, &widths, &vertical, &[]).err(),
        Some(BridgeError::Unsupported)
    );
}
