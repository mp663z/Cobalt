//! Scratch flex-basis/shrink-declaration slice (art45; earlier slices in flex_row/grow/shrink_proto.rs). Every c* case was compared pixel-for-pixel with
//! Google Chrome 156 headless at 600x200 (0 differing pixels; synthetic cases only).
//! r* cases pin the fail-closed exclusions. Not adopted; no production gate.
use kobo_web_document::{box_tree::BoxTree, css_background::paint_backgrounds, display_list::Command, parse_style_tree, Limits};

fn fills(html: &str) -> Option<Vec<(i32, i32, u32, u32, u32)>> {
    let t = BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT));
    let list = paint_backgrounds(&t, 600, 200).ok()?;
    Some(list.commands().iter().filter_map(|c| if let Command::Fill { rect, color, .. } = c { Some((rect.x, rect.y, rect.width, rect.height, u32::from(color.0) << 16 | u32::from(color.1) << 8 | u32::from(color.2))) } else { None }).collect())
}

#[test]
fn b1_basis_auto_width_grow_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex-basis:100px;flex-grow:1;background:#9cf'><div style='height:20px'></div></div><div style='flex-basis:200px;flex-grow:1;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 250, 20, 0x99ccff), (250, 0, 350, 20, 0xffcc99)]));
}

#[test]
fn b2_basis_overrides_width_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:50px;flex-basis:200px;background:#9cf'><div style='height:20px'></div></div><div style='width:60px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 200, 20, 0x99ccff), (200, 0, 60, 20, 0xffcc99)]));
}

#[test]
fn b3_shrink_zero_item_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:600px;background:#eee'><div style='width:400px;flex-shrink:0;background:#9cf'><div style='height:20px'></div></div><div style='width:400px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 400, 20, 0x99ccff), (400, 0, 200, 20, 0xffcc99)]));
}

#[test]
fn b4_shrink_2_vs_1_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:450px;background:#eee'><div style='width:300px;flex-shrink:2;background:#9cf'><div style='height:20px'></div></div><div style='width:300px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 450, 20, 0xeeeeee), (0, 0, 200, 20, 0x99ccff), (200, 0, 250, 20, 0xffcc99)]));
}

#[test]
fn b5_basis_below_min_content_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex-basis:10px;background:#9cf'><div style='width:80px;height:20px'></div></div><div style='flex-basis:100px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 80, 20, 0x99ccff), (80, 0, 100, 20, 0xffcc99)]));
}

#[test]
fn b6_shrink_basis_px_width_auto_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:300px;background:#eee'><div style='flex-basis:200px;background:#9cf'><div style='height:20px'></div></div><div style='flex-basis:200px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 300, 20, 0xeeeeee), (0, 0, 150, 20, 0x99ccff), (150, 0, 150, 20, 0xffcc99)]));
}

#[test]
fn b7_grow_and_shrink_declared_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:400px;background:#eee'><div style='width:300px;flex-grow:2;flex-shrink:3;background:#9cf'><div style='height:20px'></div></div><div style='width:300px;flex-grow:1;flex-shrink:1;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 400, 20, 0xeeeeee), (0, 0, 150, 20, 0x99ccff), (150, 0, 250, 20, 0xffcc99)]));
}

#[test]
fn b8_frozen_at_hyp_then_shrink_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:150px;background:#eee'><div style='flex-basis:10px;background:#9cf'><div style='width:80px;height:20px'></div></div><div style='flex-basis:100px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 150, 20, 0xeeeeee), (0, 0, 80, 20, 0x99ccff), (80, 0, 70, 20, 0xffcc99)]));
}

#[test]
fn b9_shrink_zero_all_overflow_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:300px;background:#eee'><div style='width:200px;flex-shrink:0;background:#9cf'><div style='height:20px'></div></div><div style='width:200px;flex-shrink:0;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 300, 20, 0xeeeeee), (0, 0, 200, 20, 0x99ccff), (200, 0, 200, 20, 0xffcc99)]));
}

#[test]
fn o1_old_r2_overflow_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:300px;background:#eee'><div style='width:200px;height:20px;background:#9cf'></div><div style='width:200px;height:20px;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 300, 20, 0xeeeeee), (0, 0, 150, 20, 0x99ccff), (150, 0, 150, 20, 0xffcc99)]));
}

#[test]
fn o2_old_h4_overflow_grow_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:300px;background:#eee'><div style='width:200px;height:20px;flex-grow:1;background:#9cf'></div><div style='width:200px;height:20px;flex-grow:1;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 300, 20, 0xeeeeee), (0, 0, 150, 20, 0x99ccff), (150, 0, 150, 20, 0xffcc99)]));
}

#[test]
fn x1_basis_percent_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex-basis:50%;background:#9cf'><div style='height:20px'></div></div></div></body></html>"#), None);
}

#[test]
fn x2_basis_content_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex-basis:content;background:#9cf'><div style='height:20px'></div></div></div></body></html>"#), None);
}

#[test]
fn x3_shrink_fraction_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;flex-shrink:0.5;background:#9cf'><div style='height:20px'></div></div></div></body></html>"#), None);
}

#[test]
fn x4_auto_basis_auto_width_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex-grow:1;background:#9cf'><div style='height:20px'></div></div></div></body></html>"#), None);
}

#[test]
fn x5_negative_basis_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex-basis:-5px;background:#9cf'><div style='height:20px'></div></div></div></body></html>"#), None);
}

#[test]
fn x6_negative_item_margin_left_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;margin-left:-10px;background:#9cf'><div style='height:20px'></div></div></div></body></html>"#), None);
}

#[test]
fn x7_negative_item_margin_top_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='width:100px;margin-top:-5px;background:#9cf'><div style='height:30px'></div></div></div></body></html>"#), None);
}

#[test]
fn x8_negative_child_margin_shrink_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:200px;background:#eee'><div style='width:300px;background:#9cf'><div style='margin-left:-10px;width:50px;height:20px'></div></div><div style='width:300px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), None);
}

#[test]
fn x9_fraction_after_freeze_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:155px;background:#eee'><div style='flex-basis:10px;background:#9cf'><div style='width:80px;height:20px'></div></div><div style='flex-basis:100px;background:#fc9'><div style='height:20px'></div></div><div style='flex-basis:40px;height:20px;background:#cf9'></div></div></body></html>"#), None);
}
