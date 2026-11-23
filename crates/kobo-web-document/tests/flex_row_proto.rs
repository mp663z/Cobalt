//! Scratch flex-row slice (art41). Every c* case was compared pixel-for-pixel with
//! Google Chrome 156 headless at 600x200 (0 differing pixels; synthetic cases only).
//! r* cases pin the fail-closed exclusions. Not adopted; no production gate.
use kobo_web_document::{box_tree::BoxTree, css_background::paint_backgrounds, display_list::Command, parse_style_tree, Limits};

fn fills(html: &str) -> Option<Vec<(i32, i32, u32, u32, u32)>> {
    let t = BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT));
    let list = paint_backgrounds(&t, 600, 200).ok()?;
    Some(list.commands().iter().filter_map(|c| if let Command::Fill { rect, color, .. } = c { Some((rect.x, rect.y, rect.width, rect.height, u32::from(color.0) << 16 | u32::from(color.1) << 8 | u32::from(color.2))) } else { None }).collect())
}

#[test]
fn c10_two_flex_stacked_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;margin-bottom:8px;background:#eee'><div style='width:100px;height:20px;background:#9cf'></div></div><div style='display:flex;margin-top:15px;background:#ddd'><div style='width:100px;height:20px;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 100, 20, 0x99ccff), (0, 35, 600, 20, 0xdddddd), (0, 35, 100, 20, 0xffcc99)]));
}

#[test]
fn c1_explicit_heights_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:200px;height:50px;background:#9cf'></div><div style='width:150px;height:30px;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 600, 50, 0xeeeeee), (0, 0, 200, 50, 0x99ccff), (200, 0, 150, 30, 0xffcc99)]));
}

#[test]
fn c2_stretch_auto_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:200px;background:#9cf'><div style='height:40px'></div></div><div style='width:150px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 600, 40, 0xeeeeee), (0, 0, 200, 40, 0x99ccff), (200, 0, 150, 40, 0xffcc99)]));
}

#[test]
fn c3_margin_padding_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;margin:10px 5px 6px 7px;padding:4px 3px;background:#9cf'><div style='height:30px'></div></div><div style='width:120px;margin:2px;background:#fc9;height:20px'></div></div></body></html>"#), Some(vec![(0, 0, 600, 54, 0xeeeeee), (7, 10, 106, 38, 0x99ccff), (120, 2, 120, 20, 0xffcc99)]));
}

#[test]
fn c4_container_padding_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:400px;padding:8px 12px;background:#eee'><div style='width:100px;height:40px;background:#9cf'></div><div style='width:100px;height:25px;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 424, 56, 0xeeeeee), (12, 8, 100, 40, 0x99ccff), (112, 8, 100, 25, 0xffcc99)]));
}

#[test]
fn c5_sibling_margins_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='height:30px;margin-bottom:20px;background:#ccc'></div><div style='display:flex;margin-top:10px;margin-bottom:12px;background:#eee'><div style='width:80px;height:20px;background:#9cf'></div></div><div style='height:10px;background:#cfc'></div></body></html>"#), Some(vec![(0, 0, 600, 30, 0xcccccc), (0, 50, 600, 20, 0xeeeeee), (0, 50, 80, 20, 0x99ccff), (0, 82, 600, 10, 0xccffcc)]));
}

#[test]
fn c6_child_margin_no_collapse_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;background:#9cf'><div style='margin-top:15px;height:20px;background:#f99'></div></div></div></body></html>"#), Some(vec![(0, 0, 600, 35, 0xeeeeee), (0, 0, 100, 35, 0x99ccff), (0, 15, 100, 20, 0xff9999)]));
}

#[test]
fn c7_definite_height_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;height:90px;background:#eee'><div style='width:100px;background:#9cf'><div style='height:20px'></div></div><div style='width:60px;height:30px;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 600, 90, 0xeeeeee), (0, 0, 100, 90, 0x99ccff), (100, 0, 60, 30, 0xffcc99)]));
}

#[test]
fn c8_nested_in_block_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='margin-top:5px;padding:3px;background:#ddd'><div style='display:flex;margin:4px 0;background:#eee'><div style='width:50px;height:20px;background:#9cf'></div></div></div></body></html>"#), Some(vec![(0, 5, 600, 34, 0xdddddd), (3, 12, 594, 20, 0xeeeeee), (3, 12, 50, 20, 0x99ccff)]));
}

#[test]
fn c9_nested_flex_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:300px;background:#9cf'><div style='display:flex;margin:6px;background:#fc9'><div style='width:40px;height:20px;background:#c9f'></div><div style='width:50px;margin-left:4px;background:#9fc'><div style='height:30px'></div></div></div></div><div style='width:100px;height:10px;background:#fcc'></div></div></body></html>"#), Some(vec![(0, 0, 600, 42, 0xeeeeee), (0, 0, 300, 42, 0x99ccff), (6, 6, 288, 30, 0xffcc99), (6, 6, 40, 20, 0xcc99ff), (50, 6, 50, 30, 0x99ffcc), (300, 0, 100, 10, 0xffcccc)]));
}

#[test]
fn r10_wrap_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;flex-wrap:wrap;background:#eee'><div style='width:100px;height:20px;background:#9cf'></div></div></body></html>"#), None);
}

#[test]
fn r11_gap_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;gap:10px;background:#eee'><div style='width:100px;height:20px;background:#9cf'></div><div style='width:100px;height:20px;background:#fc9'></div></div></body></html>"#), None);
}

// SUPERSEDED by art46: the flex shorthand is supported for exact forms; flex:1 grows the item to 600 (Chrome 156: 0 diff px).
#[test]
fn r12_flex_shorthand_superseded_now_supported() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;flex:1;height:20px;background:#9cf'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 600, 20, 0x99ccff)]));
}

#[test]
fn r13_inline_flex_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:inline-flex;background:#eee'><div style='width:100px;height:20px;background:#9cf'></div></div></body></html>"#), None);
}

#[test]
fn r14_relative_item_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='position:relative;width:100px;height:20px;background:#9cf'></div></div></body></html>"#), None);
}

#[test]
fn r15_order_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='order:2;width:100px;height:20px;background:#9cf'></div></div></body></html>"#), None);
}

#[test]
fn r16_empty_item_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;background:#9cf'></div></div></body></html>"#), None);
}

// SUPERSEDED by art42: flex-grow is now supported (integer, exact shares). This case was a
// refusal pin in art41; it now asserts the Chrome-matching grown geometry (see g4 in flex_grow_proto.rs).
#[test]
fn r1_flex_grow_superseded_now_supported() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;flex-grow:1;height:20px;background:#9cf'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 600, 20, 0x99ccff)]));
}

// SUPERSEDED by art44: overflow now resolves by flex-shrink with automatic min size (Chrome 156: 0 diff px).
#[test]
fn r2_overflow_superseded_now_shrinks() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:300px;background:#eee'><div style='width:200px;height:20px;background:#9cf'></div><div style='width:200px;height:20px;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 300, 20, 0xeeeeee), (0, 0, 150, 20, 0x99ccff), (150, 0, 150, 20, 0xffcc99)]));
}

#[test]
fn r3_auto_width_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='height:20px;background:#9cf'></div></div></body></html>"#), None);
}

#[test]
fn r4_column_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;flex-direction:column;background:#eee'><div style='width:100px;height:20px;background:#9cf'></div></div></body></html>"#), None);
}

#[test]
fn r5_text_item_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'>hello<div style='width:100px;height:20px;background:#9cf'></div></div></body></html>"#), None);
}

#[test]
fn r6_auto_margin_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;margin-left:auto;height:20px;background:#9cf'></div></div></body></html>"#), None);
}

#[test]
fn r7_pct_width_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:50%;height:20px;background:#9cf'></div></div></body></html>"#), None);
}

// SUPERSEDED by art47: justify-content / align-items are supported for exact cases (Chrome 156: 0 diff px).
#[test]
fn r8_justify_superseded_now_supported() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;justify-content:center;background:#eee'><div style='width:100px;height:20px;background:#9cf'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (250, 0, 100, 20, 0x99ccff)]));
}

// SUPERSEDED by art47: justify-content / align-items are supported for exact cases (Chrome 156: 0 diff px).
#[test]
fn r9_align_items_superseded_now_supported() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;align-items:center;background:#eee'><div style='width:100px;height:20px;background:#9cf'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 100, 20, 0x99ccff)]));
}
