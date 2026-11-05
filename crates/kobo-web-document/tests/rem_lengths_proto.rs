//! MEASUREMENT-ONLY (unadopted): rem margin/padding against the ROOT computed font size, exact whole px or refuse.
use kobo_web_document::{box_tree::BoxTree, css_background::paint_backgrounds, display_list::{Command, Rect}, parse_style_tree, style_diagnostics::RefusalReason as R, Limits};
fn tree(root: &str, body: &str) -> kobo_web_document::style_tree::StyleTree {
    parse_style_tree(format!("<!doctype html><style>html,body{{margin:0}}{root}</style><html style='height:200px'>{body}</html>").as_bytes(), &[], &Limits::DEFAULT)
}
fn main_style(decl: &str) -> String { format!("<body style='height:150px'><main style='width:50px;height:20px;background-color:#123456;{decl}'></main></body>") }
fn m(root: &str, decl: &str) -> (bool, [i32; 4], [u32; 4]) {
    let t = tree(root, &main_style(decl));
    let n = t.nodes.iter().find(|n| n.tag == "main").unwrap();
    let px = |x: kobo_web_document::computed_style::Margin| match x { kobo_web_document::computed_style::Margin::Px(v) => v, _ => i32::MIN };
    let s = &n.style;
    (t.unsupported, [px(s.margin_top), px(s.margin_right), px(s.margin_bottom), px(s.margin_left)], [s.padding_top, s.padding_right, s.padding_bottom, s.padding_left])
}
fn rects(root: &str, decl: &str) -> Vec<Rect> {
    let t = tree(root, &main_style(decl));
    assert!(!t.unsupported, "{decl}");
    paint_backgrounds(&BoxTree::from_style(&t), 300, 200).unwrap().commands().iter().filter_map(|c| if let Command::Fill { rect, .. } = c { Some(*rect) } else { None }).collect()
}
#[test]
fn rem_resolves_against_root_font_default_and_changed() {
    assert_eq!(m("", "margin-top:0.5rem;padding-left:0.5rem"), (false, [8, 0, 0, 0], [0, 0, 0, 8]));
    assert_eq!(m("html{font-size:20px}", "margin-top:0.5rem;padding-left:0.5rem"), (false, [10, 0, 0, 0], [0, 0, 0, 10]));
    assert_eq!(m("html{font-size:20px}", "margin:1rem 2rem;padding:0.25rem 1rem"), (false, [20, 40, 20, 40], [5, 20, 5, 20]));
}
#[test]
fn element_font_does_not_change_rem() {
    assert_eq!(m("", "font-size:20px;margin-top:0.5rem;padding-left:0.5rem"), (false, [8, 0, 0, 0], [0, 0, 0, 8]));
}
#[test]
fn negative_rem_margin_and_negative_rem_padding() {
    assert_eq!(m("", "margin-top:-0.5rem").1[0], -8);
    assert!(m("", "padding-left:-0.5rem").0, "negative padding stays refused");
}
#[test]
fn inexact_rem_refuses() {
    for d in ["margin-top:0.3rem", "padding-left:0.3rem", "margin:0.3rem 1rem", "padding:1rem 0.3rem"] {
        let t = tree("", &main_style(d));
        assert!(t.unsupported, "{d}");
        assert!(t.diagnostics.count(R::ComputedValue) > 0, "{d}");
    }
    // exact at root 20: 0.3rem = 6px
    assert_eq!(m("html{font-size:20px}", "margin-top:0.3rem;padding-left:0.3rem"), (false, [6, 0, 0, 0], [0, 0, 0, 6]));
    // inexact because root is 20 and 0.875rem=17.5
    assert!(m("html{font-size:20px}", "margin-top:0.875rem").0);
}
#[test]
fn rem_geometry_equals_px_twin() {
    assert_eq!(rects("", "padding:0.25rem 0.5rem"), rects("", "padding:4px 8px"));
    assert_eq!(rects("html{font-size:20px}", "padding:0.5rem 1rem"), rects("html{font-size:20px}", "padding:10px 20px"));
}
#[test]
fn other_units_stay_refused() {
    for d in ["margin-top:0rem1", "margin-top:1.00001rem", "padding-left:1ex", "margin-top:1vw"] {
        assert!(tree("", &main_style(d)).unsupported, "{d}");
    }
}
