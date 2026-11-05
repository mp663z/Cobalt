//! art69 admission-boundary probe (scratch): which flex constructs the CURRENT engine admits (paint ok + fills) and whether flex_targets reaches the shadow. Descriptive only.
use kobo_web_document::{box_tree::BoxTree, css_background::paint_backgrounds, display_list::Command, parse_style_tree, shadow_counts, Limits};
#[test]
fn probe() {
    let cases: &[(&str, &str)] = &[
        ("px baseline", "flex:1 1 50px"),
        ("percent basis", "flex:1 1 25%"),
        ("min-width", "flex:1 1 50px;min-width:70px"),
        ("max-width", "flex:1 1 50px;max-width:60px"),
        ("negative margin", "flex:1 1 50px;margin:0 -3px"),
        ("border-box", "flex:1 1 50px;box-sizing:border-box;padding:0 4px"),
        ("border width", "flex:1 1 50px;border:2px solid #000"),
        ("basis auto no width", "flex:1 1 auto"),
        ("fractional grow", "flex:1.5 1 50px"),
        ("em basis", "flex:1 1 3em"),
        ("flex-basis 0", "flex:1 1 0px"),
        ("text content", "flex:1 1 50px\">ab<\"") ,
    ];
    for (name, st) in cases {
        let item = if st.contains('>') { format!("<div style='{st}/div>") } else { format!("<div style='{st}'><div style='height:20px'></div></div>") };
        let html = format!("<!doctype html><html><body style='margin:0'><div style='display:flex;width:300px;background:#eee'>{item}{item}</div></body></html>");
        let before = shadow_counts();
        let t = BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT));
        let r = paint_backgrounds(&t, 1000, 200);
        let after = shadow_counts();
        let fills = r.as_ref().map(|l| l.commands().iter().filter(|c| matches!(c, Command::Fill { .. })).count());
        let d: Vec<i64> = (0..5).map(|k| after[k] as i64 - before[k] as i64).collect();
        eprintln!("PROBE {name:22} paint_ok={} fills={:?} shadow_delta[aa,ar,lr,dis,skip]={:?}", r.is_ok(), fills.ok(), d);
    }
}
