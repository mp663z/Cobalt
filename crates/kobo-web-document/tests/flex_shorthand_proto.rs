//! Scratch flex shorthand slice (art46; earlier slices in flex_*_proto.rs). Every c* case was compared pixel-for-pixel with
//! Google Chrome 156 headless at 600x200 (0 differing pixels; synthetic cases only).
//! r* cases pin the fail-closed exclusions. Not adopted; no production gate.
use kobo_web_document::{box_tree::BoxTree, css_background::paint_backgrounds, display_list::Command, parse_style_tree, Limits};

fn fills(html: &str) -> Option<Vec<(i32, i32, u32, u32, u32)>> {
    let t = BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT));
    let list = paint_backgrounds(&t, 600, 200).ok()?;
    Some(list.commands().iter().filter_map(|c| if let Command::Fill { rect, color, .. } = c { Some((rect.x, rect.y, rect.width, rect.height, u32::from(color.0) << 16 | u32::from(color.1) << 8 | u32::from(color.2))) } else { None }).collect())
}

#[test]
fn f10_shrink_zero_form_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:500px;background:#eee'><div style='flex:0 0 400px;background:#9cf'><div style='height:20px'></div></div><div style='flex:0 1 400px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 500, 20, 0xeeeeee), (0, 0, 400, 20, 0x99ccff), (400, 0, 100, 20, 0xffcc99)]));
}

#[test]
fn f11_zero_basis_three_items_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex:1 1 0;background:#9cf'><div style='height:20px'></div></div><div style='flex:2 1 0px;background:#fc9'><div style='height:20px'></div></div><div style='flex:3 1 0%;background:#cf9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 100, 20, 0x99ccff), (100, 0, 200, 20, 0xffcc99), (300, 0, 300, 20, 0xccff99)]));
}

#[test]
fn f12_basis_first_exact_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex:100px 3 1;background:#9cf'><div style='height:20px'></div></div><div style='flex:100px 1 1;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 400, 20, 0x99ccff), (400, 0, 200, 20, 0xffcc99)]));
}

#[test]
fn f1_flex1_two_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex:1;background:#9cf'><div style='height:20px'></div></div><div style='flex:1;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 300, 20, 0x99ccff), (300, 0, 300, 20, 0xffcc99)]));
}

#[test]
fn f2_flex_2_1_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex:2 1;background:#9cf'><div style='height:20px'></div></div><div style='flex:1 1;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 400, 20, 0x99ccff), (400, 0, 200, 20, 0xffcc99)]));
}

#[test]
fn f3_none_width_px_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;width:300px;background:#eee'><div style='flex:none;width:200px;background:#9cf'><div style='height:20px'></div></div><div style='flex:none;width:200px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 300, 20, 0xeeeeee), (0, 0, 200, 20, 0x99ccff), (200, 0, 200, 20, 0xffcc99)]));
}

#[test]
fn f4_auto_width_px_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex:auto;width:100px;background:#9cf'><div style='height:20px'></div></div><div style='flex:auto;width:200px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 250, 20, 0x99ccff), (250, 0, 350, 20, 0xffcc99)]));
}

#[test]
fn f5_triple_basis_last_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex:1 1 100px;background:#9cf'><div style='height:20px'></div></div><div style='flex:1 1 200px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 250, 20, 0x99ccff), (250, 0, 350, 20, 0xffcc99)]));
}

#[test]
fn f6_basis_first_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex:100px 2 1;background:#9cf'><div style='height:20px'></div></div><div style='flex:100px 1 1;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), None);
}

#[test]
fn f7_auto_then_grow_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex:auto 1;width:100px;background:#9cf'><div style='height:20px'></div></div><div style='flex:auto 3;width:100px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 200, 20, 0x99ccff), (200, 0, 400, 20, 0xffcc99)]));
}

#[test]
fn f8_single_basis_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex:150px;background:#9cf'><div style='height:20px'></div></div><div style='flex:250px;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 250, 20, 0x99ccff), (250, 0, 350, 20, 0xffcc99)]));
}

#[test]
fn f9_flex_then_longhand_override_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex:1;flex-grow:3;background:#9cf'><div style='height:20px'></div></div><div style='flex:1;background:#fc9'><div style='height:20px'></div></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 450, 20, 0x99ccff), (450, 0, 150, 20, 0xffcc99)]));
}

#[test]
fn y1_fraction_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex:1.5;background:#9cf'><div style='height:20px'></div></div></div></body></html>"#), None);
}

#[test]
fn y2_pct_basis_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex:1 1 50%;background:#9cf'><div style='height:20px'></div></div></div></body></html>"#), None);
}

#[test]
fn y3_content_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex:1 1 content;background:#9cf'><div style='height:20px'></div></div></div></body></html>"#), None);
}

#[test]
fn y4_four_tokens_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex:1 1 1 1;background:#9cf'><div style='height:20px'></div></div></div></body></html>"#), None);
}

#[test]
fn y5_negative_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex:-1;background:#9cf'><div style='height:20px'></div></div></div></body></html>"#), None);
}

#[test]
fn y6_basis_middle_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex:1 100px 1;background:#9cf'><div style='height:20px'></div></div></div></body></html>"#), None);
}

#[test]
fn y7_calc_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex:calc(1);background:#9cf'><div style='height:20px'></div></div></div></body></html>"#), None);
}

#[test]
fn y8_flex_auto_no_width_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee'><div style='flex:auto;background:#9cf'><div style='height:20px'></div></div></div></body></html>"#), None);
}
