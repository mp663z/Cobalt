//! MEASUREMENT-ONLY (unadopted): padding in em, own-font basis, exact whole px or refusal; geometry compared with the px twin.
use kobo_web_document::{box_tree::BoxTree, css_background::paint_backgrounds, display_list::{Command, Rect}, parse_style_tree, style_diagnostics::RefusalReason as R, Limits};

fn tree(body: &str) -> kobo_web_document::style_tree::StyleTree {
    parse_style_tree(format!("<!doctype html><style>html,body{{margin:0}}</style><html style='height:200px'>{body}</html>").as_bytes(), &[], &Limits::DEFAULT)
}
fn rects(body: &str) -> Result<Vec<Rect>, String> {
    let t = tree(body);
    if t.unsupported { return Err("unsupported".into()); }
    let b = BoxTree::from_style(&t);
    paint_backgrounds(&b, 300, 200).map_err(|e| format!("{e:?}")).map(|l| l.commands().iter().filter_map(|c| if let Command::Fill { rect, .. } = c { Some(*rect) } else { None }).collect())
}
fn main_style(decl: &str) -> String { format!("<body style='height:150px'><main style='width:50px;height:20px;background-color:#123456;{decl}'></main></body>") }
fn p(html: &str) -> (bool, [u32; 4]) {
    let t = tree(html);
    let n = t.nodes.iter().find(|n| n.tag == "main").unwrap();
    (t.unsupported, [n.style.padding_top, n.style.padding_right, n.style.padding_bottom, n.style.padding_left])
}

#[test]
fn em_uses_the_elements_own_font_size_exactly() {
    assert_eq!(p(&main_style("padding-left:0.5em")), (false, [0, 0, 0, 8])); // font 16
    assert_eq!(p(&main_style("font-size:20px;padding-left:0.5em")), (false, [0, 0, 0, 10]));
    assert_eq!(p(&main_style("padding-left:0.5em;font-size:20px")), (false, [0, 0, 0, 10]), "declaration order irrelevant");
    assert_eq!(p(&main_style("font-size:1.25rem;padding:0.5em 1em")), (false, [10, 20, 10, 20]), "rem font then em padding");
}
#[test]
fn parent_font_is_not_the_basis_for_own_em_padding() {
    let h = "<body style='height:150px;font-size:20px'><main style='width:50px;height:20px;font-size:10px;padding-left:1em'></main></body>";
    assert_eq!(p(h), (false, [0, 0, 0, 10]));
}
#[test]
fn inexact_em_refuses_not_rounds() {
    for d in ["padding-left:0.3em", "padding:0.3em 0.5em", "padding:0.3em 0px", "padding-top:0.95em"] {
        let t = tree(&main_style(d));
        assert!(t.unsupported, "{d}");
        assert!(t.diagnostics.count(R::ComputedValue) > 0, "{d}");
    }
    // exact at a different font size: 0.3em * 20px = 6
    assert_eq!(p(&main_style("font-size:20px;padding-left:0.3em")), (false, [0, 0, 0, 6]));
}
#[test]
fn rem_and_other_units_stay_refused() {
    for d in ["padding-left:1%", "padding-left:-1em", "padding-left:1ex"] {
        assert!(tree(&main_style(d)).unsupported, "{d}");
    }
}
#[test]
fn geometry_equals_the_px_twin() {
    let em = rects(&main_style("padding:0.25em 0.5em")).unwrap();
    let px = rects(&main_style("padding:4px 8px")).unwrap();
    assert_eq!(em, px);
    assert_eq!((em[0].width, em[0].height), (66, 28), "Chrome 1x measured 66x28 (art32)");
    let em = rects(&main_style("font-size:20px;padding:0.5em 1em")).unwrap();
    let px = rects(&main_style("font-size:20px;padding:10px 20px")).unwrap();
    assert_eq!(em, px);
    assert_eq!(em.len(), 1);
    assert_eq!((em[0].width, em[0].height), (50 + 40, 20 + 20), "{em:?}");
}
