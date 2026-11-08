//! PRIVATE candidate FL18 (ext entry only): `column-gap` / single-value `gap` (px or `normal`) in the admitted single-line LTR
//! row flex slice. Expected x/width come from Chrome 156 (private-evidence ul1/gp1.json, 800 wide, items 10px high); the
//! engine side is rasterised-background geometry (paint_backgrounds), no Chrome/stored-pixel comparison. Fractional flex
//! results (gp1 g10) refuse. The default entry is unchanged (asserted refusing).
use kobo_web_document::{box_tree::BoxTree, css_background::paint_backgrounds, display_list::Command, parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget};
fn page(container: &str, items: &[&str]) -> String {
    let its: String = items.iter().map(|s| format!("<div style='height:10px;background:#123456;{s}'></div>")).collect();
    format!("<!doctype html><body style='margin:0'><div style='display:flex;{container}'>{its}</div>")
}
fn xs(html: &str) -> Option<Vec<(i32, u32)>> {
    let t = parse_style_tree_with_inline_sheets_and_selectors(html.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    if t.unsupported { return None; }
    let bt = BoxTree::from_style(&t);
    if bt.unsupported { return None; }
    let l = paint_backgrounds(&bt, 800, 200).ok()?;
    Some(l.commands().iter().filter_map(|c| if let Command::Fill { rect, .. } = c { Some((rect.x, rect.width)) } else { None }).collect())
}
const W100: &str = "width:100px";
#[test]
fn gap_matches_chrome_fixture_numbers() {
    assert_eq!(xs(&page("gap:10px", &[W100; 3])), Some(vec![(0, 100), (110, 100), (220, 100)])); // g1
    assert_eq!(xs(&page("column-gap:10px", &["flex:1 1 0px"; 3])), Some(vec![(0, 260), (270, 260), (540, 260)])); // g2 grow
    assert_eq!(xs(&page("gap:10px", &["width:300px"; 3])), Some(vec![(0, 260), (270, 260), (540, 260)])); // g3 shrink
    assert_eq!(xs(&page("gap:20px;justify-content:space-between", &[W100; 3])), Some(vec![(0, 100), (350, 100), (700, 100)])); // g4
    assert_eq!(xs(&page("column-gap:10px", &["width:100px;margin:0 5px"; 3])), Some(vec![(5, 100), (125, 100), (245, 100)])); // g5
    assert_eq!(xs(&page("gap:10px", &[W100])), Some(vec![(0, 100)])); // g6 single item
    assert_eq!(xs(&page("column-gap:10px;justify-content:center", &[W100; 2])), Some(vec![(295, 100), (405, 100)])); // g7
    assert_eq!(xs(&page("column-gap:10px", &["width:100px;padding:0 7px"; 2])), Some(vec![(0, 114), (124, 114)])); // g8
    assert_eq!(xs(&page("column-gap:10px;justify-content:flex-end", &[W100; 2])), Some(vec![(590, 100), (700, 100)])); // g9
}
#[test]
fn gap_normal_zero_and_later_declaration_cascade() {
    let base = xs(&page("", &[W100; 2]));
    assert_eq!(base, Some(vec![(0, 100), (100, 100)]));
    for z in ["normal", "0", "0px"] {
        assert_eq!(xs(&page(&format!("gap:10px;gap:{z}"), &[W100; 2])), base, "{z}");
    }
    assert_eq!(xs(&page("gap:0;gap:10px", &[W100; 2])), Some(vec![(0, 100), (110, 100)]));
    assert_eq!(xs(&page("column-gap:10px;gap:20px", &[W100; 2])), Some(vec![(0, 100), (120, 100)])); // later shorthand wins
}
#[test]
fn unmodelled_gap_shapes_refuse() {
    for c in ["gap:10px 20px", "row-gap:10px", "gap:5%", "column-gap:1em", "gap:calc(1px+1px)", "gap:-1px", "gap:2000px"] {
        assert_eq!(xs(&page(c, &[W100; 2])), None, "{c}");
    }
    // fractional flex result (Chrome g10: 453.328125 / 226.671875) refuses
    assert_eq!(xs(&page("column-gap:10px", &["flex:2 1 0px", "flex:1 1 0px", W100])), None);
}
#[test]
fn default_entry_still_refuses_gap() {
    let t = parse_style_tree(page("gap:10px", &[W100; 2]).as_bytes(), &[], &Limits::DEFAULT);
    assert!(t.unsupported);
}
