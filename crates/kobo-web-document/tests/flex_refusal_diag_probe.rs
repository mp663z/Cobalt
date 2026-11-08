//! art75 probe (scratch): per-construct StyleTree.unsupported + diagnostics Debug + paint ok. ONE case each; descriptive.
use kobo_web_document::{box_tree::BoxTree, css_background::paint_backgrounds, parse_style_tree, Limits};
#[test]
fn diag() {
    for (name, st) in [("shorthand 0%", "flex:1 1 0%"), ("longhand flex-basis:0%", "flex-grow:1;flex-basis:0%"), ("longhand flex-basis:50px", "flex-grow:1;flex-basis:50px"), ("min-width", "flex:1 1 50px;min-width:70px"), ("max-width", "flex:1 1 50px;max-width:60px"), ("percent 25%", "flex:1 1 25%"), ("negative margin", "flex:1 1 50px;margin:0 -3px"), ("border-box", "flex:1 1 50px;box-sizing:border-box"), ("frac grow", "flex:1.5 1 50px"), ("em", "flex:1 1 3em")] {
        let html = format!("<!doctype html><html><body style='margin:0'><div style='display:flex;width:300px;background:#eee'><div style='{st}'><div style='height:20px'></div></div><div style='{st}'><div style='height:20px'></div></div></div></body></html>");
        let s = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
        let t = BoxTree::from_style(&s);
        let p = paint_backgrounds(&t, 1000, 200).is_ok();
        eprintln!("DIAG {name:26} unsupported={} paint_ok={p} diag={:?}", s.unsupported, s.diagnostics);
    }
}
