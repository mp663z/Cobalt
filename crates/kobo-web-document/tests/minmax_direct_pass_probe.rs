//! art84: direct pass constructors (public) must report unsupported for stray min/max-width, and not for flex-item min/max.
use kobo_web_document::{box_tree::{BoxTree, HeightPass, VerticalPass, WidthPass, UsedHeightPass}, parse_style_tree, Limits};
#[test]
fn direct_passes() {
    let f = |i: &str| format!("<!doctype html><html><body style='margin:0'>{i}</body></html>");
    for (n, h, want) in [
        ("block max-width", f("<div style='max-width:50px'><div style='height:10px'></div></div>"), true),
        ("flex item child min-width", f("<div style='display:flex;width:300px'><div style='flex:1 1 50px'><div style='min-width:20px;height:10px'></div></div></div>"), true),
        ("flex item min/max ok", f("<div style='display:flex;width:300px'><div style='flex:1 1 50px;min-width:20px;max-width:90px'><div style='height:10px'></div></div></div>"), false),
        ("no min/max", f("<div style='height:10px'></div>"), false),
    ] {
        let t = BoxTree::from_style(&parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT));
        let (w, v, hp, u) = (WidthPass::from_boxes(&t, 1000).unsupported, VerticalPass::from_boxes(&t, 1000, 200).unsupported, HeightPass::from_boxes(&t, 200).unsupported, UsedHeightPass::from_boxes(&t, 1000, 200).unsupported);
        eprintln!("DIRECT {n:28} width={w} vertical={v} height={hp} usedheight={u} want_all={want}");
        assert!(w == want && v == want && hp == want && u == want, "{n}");
    }
}
