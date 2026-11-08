//! Scratch flex-grow slice (art42; art41 tests are in flex_row_proto.rs). Every c* case was compared pixel-for-pixel with
//! Google Chrome 156 headless at 600x200 (0 differing pixels; synthetic cases only).
//! r* cases pin the fail-closed exclusions. Not adopted; no production gate.
use kobo_web_document::{box_tree::BoxTree, css_background::paint_backgrounds, display_list::Command, parse_style_tree, Limits};

fn fills(html: &str) -> Option<Vec<(i32, i32, u32, u32, u32)>> {
    let t = BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT));
    let list = paint_backgrounds(&t, 600, 200).ok()?;
    Some(list.commands().iter().filter_map(|c| if let Command::Fill { rect, color, .. } = c { Some((rect.x, rect.y, rect.width, rect.height, u32::from(color.0) << 16 | u32::from(color.1) << 8 | u32::from(color.2))) } else { None }).collect())
}

#[test]
fn g10_definite_height_equal_margin_box_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;height:33px;background:#eee'><div style='width:100px;height:25px;margin:5px 0 3px;background:#9cf'></div></div></body></html>"#), Some(vec![(0, 0, 600, 33, 0xeeeeee), (0, 5, 100, 25, 0x99ccff)]));
}

#[test]
fn g1_equal_grow_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;height:20px;flex-grow:1;background:#9cf'></div><div style='width:100px;height:20px;flex-grow:1;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 300, 20, 0x99ccff), (300, 0, 300, 20, 0xffcc99)]));
}

#[test]
fn g2_ratio_1_3_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;height:20px;flex-grow:1;background:#9cf'></div><div style='width:100px;height:20px;flex-grow:3;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 200, 20, 0x99ccff), (200, 0, 400, 20, 0xffcc99)]));
}

#[test]
fn g3_mixed_margin_padding_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;margin:3px 5px;padding:2px 4px;flex-grow:2;background:#9cf'><div style='height:20px'></div></div><div style='width:100px;height:20px;background:#fc9'></div><div style='width:50px;margin-left:10px;flex-grow:1;height:25px;background:#cf9'></div></div></body></html>"#), None);
}

#[test]
fn g4_single_fill_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;height:20px;flex-grow:1;background:#9cf'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 600, 20, 0x99ccff)]));
}

#[test]
fn g5_container_padding_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:500px;padding:6px 10px;background:#eee'><div style='width:100px;height:20px;flex-grow:1;background:#9cf'></div><div style='width:60px;height:30px;flex-grow:4;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 520, 42, 0xeeeeee), (10, 6, 168, 20, 0x99ccff), (178, 6, 332, 30, 0xffcc99)]));
}

#[test]
fn g6_nested_grow_inside_grown_item_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;flex-grow:1;background:#9cf'><div style='display:flex;background:#fc9'><div style='width:30px;height:20px;flex-grow:1;background:#c9f'></div><div style='width:20px;height:10px;background:#9fc'></div></div></div><div style='width:100px;height:15px;background:#fcc'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 500, 20, 0x99ccff), (0, 0, 500, 20, 0xffcc99), (0, 0, 480, 20, 0xcc99ff), (480, 0, 20, 10, 0x99ffcc), (500, 0, 100, 15, 0xffcccc)]));
}

#[test]
fn g7_grow_zero_and_full_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:300px;height:20px;flex-grow:0;background:#9cf'></div><div style='width:300px;height:20px;flex-grow:5;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 300, 20, 0x99ccff), (300, 0, 300, 20, 0xffcc99)]));
}

#[test]
fn g8_grow_inherit_keyword_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;height:20px;flex-grow:inherit;background:#9cf'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 100, 20, 0x99ccff)]));
}

#[test]
fn g9_mixed_margin_padding_divisible_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;margin:3px 5px;padding:2px 4px;flex-grow:2;background:#9cf'><div style='height:20px'></div></div><div style='width:100px;height:20px;background:#fc9'></div><div style='width:50px;margin-left:11px;flex-grow:1;height:25px;background:#cf9'></div></div></body></html>"#), Some(vec![(0, 0, 600, 30, 0xeeeeee), (5, 3, 322, 24, 0x99ccff), (332, 0, 100, 20, 0xffcc99), (443, 0, 157, 25, 0xccff99)]));
}

#[test]
fn h10_definite_height_below_tallest_item_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;height:30px;background:#eee'><div style='width:100px;height:50px;background:#9cf'></div></div></body></html>"#), None);
}

#[test]
fn h11_definite_height_below_item_margin_box_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;height:30px;background:#eee'><div style='width:100px;height:25px;margin:5px 0 3px;background:#9cf'></div></div></body></html>"#), None);
}

#[test]
fn h1_fractional_share_three_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;height:20px;flex-grow:1;background:#9cf'></div><div style='width:100px;height:20px;flex-grow:1;background:#fc9'></div><div style='width:100px;height:20px;flex-grow:1;background:#cf9'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 200, 20, 0x99ccff), (200, 0, 200, 20, 0xffcc99), (400, 0, 200, 20, 0xccff99)]));
}

#[test]
fn h2_fractional_grow_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;height:20px;flex-grow:0.5;background:#9cf'></div></div></body></html>"#), None);
}

#[test]
fn h3_negative_grow_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;height:20px;flex-grow:-1;background:#9cf'></div></div></body></html>"#), None);
}

// SUPERSEDED by art44: overflow now resolves by flex-shrink with automatic min size (Chrome 156: 0 diff px).
#[test]
fn h4_overflow_with_grow_superseded_now_shrinks() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:300px;background:#eee'><div style='width:200px;height:20px;flex-grow:1;background:#9cf'></div><div style='width:200px;height:20px;flex-grow:1;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 300, 20, 0xeeeeee), (0, 0, 150, 20, 0x99ccff), (150, 0, 150, 20, 0xffcc99)]));
}

#[test]
fn h5_exponent_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;height:20px;flex-grow:1e0;background:#9cf'></div></div></body></html>"#), None);
}

#[test]
fn h6_calc_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;height:20px;flex-grow:calc(1);background:#9cf'></div></div></body></html>"#), None);
}

// SUPERSEDED by art45: declared flex-shrink / flex-basis are supported; one item grows to 600 (Chrome 156: 0 diff px).
#[test]
fn h7_flex_shrink_declared_superseded_now_supported() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;height:20px;flex-grow:1;flex-shrink:0;background:#9cf'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 600, 20, 0x99ccff)]));
}

// SUPERSEDED by art45: declared flex-shrink / flex-basis are supported; one item grows to 600 (Chrome 156: 0 diff px).
#[test]
fn h8_flex_basis_declared_superseded_now_supported() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;height:20px;flex-grow:1;flex-basis:50px;background:#9cf'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 600, 20, 0x99ccff)]));
}

#[test]
fn h9_fractional_share_three_in_400_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:400px;background:#eee'><div style='width:100px;height:20px;flex-grow:1;background:#9cf'></div><div style='width:100px;height:20px;flex-grow:1;background:#fc9'></div><div style='width:100px;height:20px;flex-grow:1;background:#cf9'></div></div></body></html>"#), None);
}
