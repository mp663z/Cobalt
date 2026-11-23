//! Scratch flex-shrink slice (art44; earlier slices in flex_row_proto.rs / flex_grow_proto.rs). Every c* case was compared pixel-for-pixel with
//! Google Chrome 156 headless at 600x200 (0 differing pixels; synthetic cases only).
//! r* cases pin the fail-closed exclusions. Not adopted; no production gate.
use kobo_web_document::{box_tree::BoxTree, css_background::paint_backgrounds, display_list::Command, parse_style_tree, Limits};

fn fills(html: &str) -> Option<Vec<(i32, i32, u32, u32, u32)>> {
    let t = BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT));
    let list = paint_backgrounds(&t, 600, 200).ok()?;
    Some(list.commands().iter().filter_map(|c| if let Command::Fill { rect, color, .. } = c { Some((rect.x, rect.y, rect.width, rect.height, u32::from(color.0) << 16 | u32::from(color.1) << 8 | u32::from(color.2))) } else { None }).collect())
}

#[test]
fn s10_freeze_then_scale_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:600px;background:#eee'><div style='width:400px;background:#9cf'><div style='width:380px;height:20px'></div></div><div style='width:300px;background:#fc9'><div style='height:20px'></div></div><div style='width:300px;background:#cf9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 380, 20, 0x99ccff), (380, 0, 110, 20, 0xffcc99), (490, 0, 110, 20, 0xccff99)]));
}

#[test]
fn s1_equal_shrink_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:400px;background:#9cf'><div style='height:20px'></div></div><div style='width:400px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 300, 20, 0x99ccff), (300, 0, 300, 20, 0xffcc99)]));
}

#[test]
fn s2_scaled_unequal_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:400px;background:#eee'><div style='width:300px;background:#9cf'><div style='height:20px'></div></div><div style='width:100px;background:#fc9'><div style='height:20px'></div></div><div style='width:200px;background:#cf9'><div style='height:20px'></div></div></div></body></html>"#), None);
}

#[test]
fn s3_min_content_freeze_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:400px;background:#eee'><div style='width:300px;background:#9cf'><div style='width:280px;height:20px'></div></div><div style='width:300px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 400, 20, 0xeeeeee), (0, 0, 280, 20, 0x99ccff), (280, 0, 120, 20, 0xffcc99)]));
}

#[test]
fn s4_margins_padding_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:400px;background:#eee'><div style='width:300px;margin:0 10px;padding:0 5px;background:#9cf'><div style='height:20px'></div></div><div style='width:200px;margin-left:5px;background:#fc9'><div style='height:30px'></div></div></div></body></html>"#), Some(vec![(0, 0, 400, 30, 0xeeeeee), (10, 0, 229, 30, 0x99ccff), (254, 0, 146, 30, 0xffcc99)]));
}

#[test]
fn s5_grow_ignored_when_overflow_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:400px;background:#eee'><div style='width:300px;flex-grow:3;background:#9cf'><div style='height:20px'></div></div><div style='width:300px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 400, 20, 0xeeeeee), (0, 0, 200, 20, 0x99ccff), (200, 0, 200, 20, 0xffcc99)]));
}

#[test]
fn s6_min_content_from_nested_padding_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:300px;background:#eee'><div style='width:400px;background:#9cf'><div style='padding:0 10px'><div style='width:150px;height:20px'></div></div></div><div style='width:200px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 300, 20, 0xeeeeee), (0, 0, 200, 20, 0x99ccff), (200, 0, 100, 20, 0xffcc99)]));
}

#[test]
fn s7_all_frozen_at_min_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:100px;background:#eee'><div style='width:200px;background:#9cf'><div style='width:80px;height:20px'></div></div><div style='width:200px;background:#fc9'><div style='width:70px;height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 100, 20, 0xeeeeee), (0, 0, 80, 20, 0x99ccff), (80, 0, 70, 20, 0xffcc99)]));
}

#[test]
fn s8_zero_min_item_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:500px;background:#eee'><div style='width:600px;background:#9cf'><div style='height:20px'></div></div><div style='width:400px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 500, 20, 0xeeeeee), (0, 0, 300, 20, 0x99ccff), (300, 0, 200, 20, 0xffcc99)]));
}

#[test]
fn s9_three_exact_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:400px;background:#eee'><div style='width:300px;background:#9cf'><div style='height:20px'></div></div><div style='width:150px;background:#fc9'><div style='height:20px'></div></div><div style='width:150px;background:#cf9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 400, 20, 0xeeeeee), (0, 0, 200, 20, 0x99ccff), (200, 0, 100, 20, 0xffcc99), (300, 0, 100, 20, 0xccff99)]));
}

#[test]
fn t1_fractional_shrink_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:250px;background:#eee'><div style='width:100px;background:#9cf'><div style='height:20px'></div></div><div style='width:200px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), None);
}

#[test]
fn t2_percent_child_min_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:200px;background:#eee'><div style='width:300px;background:#9cf'><div style='width:50%;height:20px'></div></div><div style='width:300px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), None);
}

#[test]
fn t3_flex_child_min_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:200px;background:#eee'><div style='width:300px;background:#9cf'><div style='display:flex'><div style='width:50px;height:20px'></div></div></div><div style='width:300px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), None);
}

#[test]
fn t4_auto_margin_child_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:200px;background:#eee'><div style='width:300px;background:#9cf'><div style='margin-left:auto;width:50px;height:20px'></div></div><div style='width:300px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), None);
}

#[test]
fn t5_zero_base_all_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:50px;background:#eee'><div style='width:0px;margin-left:60px;height:20px;background:#9cf'></div></div></body></html>"#), None);
}
