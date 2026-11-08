//! Scratch justify-content/align-items slice (art47; earlier slices in flex_*_proto.rs). Every c* case was compared pixel-for-pixel with
//! Google Chrome 156 headless at 600x200 (0 differing pixels; synthetic cases only).
//! r* cases pin the fail-closed exclusions. Not adopted; no production gate.
use kobo_web_document::{box_tree::BoxTree, css_background::paint_backgrounds, display_list::Command, parse_style_tree, Limits};

fn fills(html: &str) -> Option<Vec<(i32, i32, u32, u32, u32)>> {
    let t = BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT));
    let list = paint_backgrounds(&t, 600, 200).ok()?;
    Some(list.commands().iter().filter_map(|c| if let Command::Fill { rect, color, .. } = c { Some((rect.x, rect.y, rect.width, rect.height, u32::from(color.0) << 16 | u32::from(color.1) << 8 | u32::from(color.2))) } else { None }).collect())
}

#[test]
fn a1_flex_start_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;align-items:flex-start'><div style='width:100px;height:40px;background:#9cf'></div><div style='width:100px;height:20px;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 600, 40, 0xeeeeee), (0, 0, 100, 40, 0x99ccff), (100, 0, 100, 20, 0xffcc99)]));
}

#[test]
fn a2_flex_end_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;align-items:flex-end'><div style='width:100px;height:40px;background:#9cf'></div><div style='width:100px;height:20px;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 600, 40, 0xeeeeee), (0, 0, 100, 40, 0x99ccff), (100, 20, 100, 20, 0xffcc99)]));
}

#[test]
fn a3_center_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;align-items:center'><div style='width:100px;height:40px;background:#9cf'></div><div style='width:100px;height:20px;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 600, 40, 0xeeeeee), (0, 0, 100, 40, 0x99ccff), (100, 10, 100, 20, 0xffcc99)]));
}

#[test]
fn a4_stretch_explicit_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;align-items:stretch'><div style='width:100px;height:40px;background:#9cf'></div><div style='width:100px;background:#fc9'><div style='height:10px'></div></div></div></body></html>"#), Some(vec![(0, 0, 600, 40, 0xeeeeee), (0, 0, 100, 40, 0x99ccff), (100, 0, 100, 40, 0xffcc99)]));
}

#[test]
fn a5_flex_start_auto_height_nostretch_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;align-items:flex-start'><div style='width:100px;background:#9cf'><div style='height:40px'></div></div><div style='width:100px;background:#fc9'><div style='height:10px'></div></div></div></body></html>"#), Some(vec![(0, 0, 600, 40, 0xeeeeee), (0, 0, 100, 40, 0x99ccff), (100, 0, 100, 10, 0xffcc99)]));
}

#[test]
fn a6_center_margins_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;align-items:center;height:60px'><div style='width:100px;height:20px;margin:4px 0 6px;background:#9cf'></div><div style='width:100px;height:10px;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 600, 60, 0xeeeeee), (0, 19, 100, 20, 0x99ccff), (100, 25, 100, 10, 0xffcc99)]));
}

#[test]
fn a7_flex_end_padding_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;align-items:flex-end'><div style='width:100px;padding:3px 0;background:#9cf'><div style='height:30px'></div></div><div style='width:100px;height:10px;margin-top:2px;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 600, 36, 0xeeeeee), (0, 0, 100, 36, 0x99ccff), (100, 26, 100, 10, 0xffcc99)]));
}

#[test]
fn a8_both_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;justify-content:space-between;align-items:center'><div style='width:100px;height:40px;background:#9cf'></div><div style='width:100px;height:20px;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 600, 40, 0xeeeeee), (0, 0, 100, 40, 0x99ccff), (500, 10, 100, 20, 0xffcc99)]));
}

#[test]
fn j10_margins_center_even_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;justify-content:center'><div style='width:100px;height:20px;margin:0 10px 0 4px;background:#9cf'></div><div style='width:100px;height:20px;padding:0 3px;margin-left:6px;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (191, 0, 100, 20, 0x99ccff), (307, 0, 106, 20, 0xffcc99)]));
}

#[test]
fn j11_overflow_end_shrinks_to_fit_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;width:150px;justify-content:flex-end'><div style='width:100px;height:20px;background:#9cf'></div><div style='width:100px;height:20px;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 150, 20, 0xeeeeee), (0, 0, 75, 20, 0x99ccff), (75, 0, 75, 20, 0xffcc99)]));
}

#[test]
fn j1_flex_end_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;justify-content:flex-end'><div style='width:100px;height:20px;background:#9cf'></div><div style='width:150px;height:20px;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (350, 0, 100, 20, 0x99ccff), (450, 0, 150, 20, 0xffcc99)]));
}

#[test]
fn j2_center_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;justify-content:center'><div style='width:100px;height:20px;background:#9cf'></div><div style='width:150px;height:20px;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (175, 0, 100, 20, 0x99ccff), (275, 0, 150, 20, 0xffcc99)]));
}

#[test]
fn j3_space_between_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;justify-content:space-between'><div style='width:100px;height:20px;background:#9cf'></div><div style='width:100px;height:20px;background:#fc9'></div><div style='width:100px;height:20px;background:#cf9'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 100, 20, 0x99ccff), (250, 0, 100, 20, 0xffcc99), (500, 0, 100, 20, 0xccff99)]));
}

#[test]
fn j4_space_around_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;justify-content:space-around'><div style='width:100px;height:20px;background:#9cf'></div><div style='width:100px;height:20px;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (100, 0, 100, 20, 0x99ccff), (400, 0, 100, 20, 0xffcc99)]));
}

#[test]
fn j5_space_evenly_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;justify-content:space-evenly'><div style='width:100px;height:20px;background:#9cf'></div><div style='width:100px;height:20px;background:#fc9'></div><div style='width:100px;height:20px;background:#cf9'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (75, 0, 100, 20, 0x99ccff), (250, 0, 100, 20, 0xffcc99), (425, 0, 100, 20, 0xccff99)]));
}

#[test]
fn j6_margins_center_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;justify-content:center'><div style='width:100px;height:20px;margin:0 10px 0 4px;background:#9cf'></div><div style='width:100px;height:20px;padding:0 3px;margin-left:5px;background:#fc9'></div></div></body></html>"#), None);
}

#[test]
fn j7_single_between_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;justify-content:space-between'><div style='width:100px;height:20px;background:#9cf'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 100, 20, 0x99ccff)]));
}

#[test]
fn j8_grow_consumes_free_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;justify-content:center'><div style='width:100px;height:20px;flex-grow:1;background:#9cf'></div></div></body></html>"#), Some(vec![(0, 0, 600, 20, 0xeeeeee), (0, 0, 600, 20, 0x99ccff)]));
}

#[test]
fn j9_flex_start_overflow_shrink_matches_chrome() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;width:300px;justify-content:flex-start'><div style='width:200px;height:20px;background:#9cf'></div><div style='width:200px;height:20px;background:#fc9'></div></div></body></html>"#), Some(vec![(0, 0, 300, 20, 0xeeeeee), (0, 0, 150, 20, 0x99ccff), (150, 0, 150, 20, 0xffcc99)]));
}

#[test]
fn z1_odd_center_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;justify-content:center'><div style='width:101px;height:20px;background:#9cf'></div></div></body></html>"#), None);
}

#[test]
fn z2_fractional_between_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;justify-content:space-between'><div style='width:100px;height:20px;background:#9cf'></div><div style='width:100px;height:20px;background:#fc9'></div><div style='width:101px;height:20px;background:#cf9'></div></div></body></html>"#), None);
}

#[test]
fn z3_fractional_around_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;justify-content:space-around'><div style='width:100px;height:20px;background:#9cf'></div><div style='width:101px;height:20px;background:#fc9'></div></div></body></html>"#), None);
}

#[test]
fn z4_overflow_center_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;width:200px;justify-content:center'><div style='width:100px;height:20px;background:#9cf'></div><div style='width:100px;height:20px;background:#fc9'></div><div style='width:0;height:0'></div></div></body></html>"#), None);
}

#[test]
fn z5_odd_cross_center_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;align-items:center'><div style='width:100px;height:41px;background:#9cf'></div><div style='width:100px;height:20px;background:#fc9'></div></div></body></html>"#), None);
}

#[test]
fn z6_baseline_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;align-items:baseline'><div style='width:100px;height:20px;background:#9cf'></div></div></body></html>"#), None);
}

#[test]
fn z7_space_left_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;justify-content:left'><div style='width:100px;height:20px;background:#9cf'></div></div></body></html>"#), None);
}

#[test]
fn z8_align_self_refused() {
    assert_eq!(fills(r#"<!doctype html><html><body style='margin:0'><div style='display:flex;background:#eee;align-items:center'><div style='width:100px;height:20px;align-self:flex-end;background:#9cf'></div></div></body></html>"#), None);
}
