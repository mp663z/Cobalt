//! PRIVATE, unadopted candidate (FL44): a whole-value `calc()` of whole-px terms with `+`/`-` is folded to a constant `Npx` for
//! width/height/min-/max- size, margin and padding LONGHANDS (ext entry only). Engine Fill rects vs literal-px twins (no
//! Chrome/stored-pixel comparison here; Chrome comparison is private-evidence ul1/cc62, my own rasteriser).
use kobo_web_document::{
    box_tree::BoxTree, css_background::paint_backgrounds, display_list::Command, parse_style_tree,
    parse_style_tree_with_inline_sheets_and_selectors, parse_style_tree_with_media, parse_style_tree_with_pinned_font_family, Limits,
    MediaContext, SupplyBudget,
};
type F = (i32, i32, u32, u32, u8);
fn page(body: &str) -> String {
    format!("<!doctype html><style>.c{{background:#cde}}.d{{background:#fda}}.e{{background:#aed}}</style><body style='margin:0'>{body}")
}
fn run(h: &str) -> Option<Vec<F>> {
    let t = parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    if t.unsupported {
        return None;
    }
    let bt = BoxTree::from_style(&t);
    if bt.unsupported {
        return None;
    }
    let l = paint_backgrounds(&bt, 800, 600).ok()?;
    Some(l.commands().iter().filter_map(|c| if let Command::Fill { rect, color, .. } = c { Some((rect.x, rect.y, rect.width, rect.height, color.0)) } else { None }).collect())
}
fn same(calc: &str, lit: &str) {
    let a = run(&page(calc));
    assert!(a.is_some(), "refused: {calc}");
    assert_eq!(a, run(&page(lit)), "{calc}");
}
fn refused(b: &str) {
    assert!(run(&page(b)).is_none(), "admitted: {b}");
}
#[test]
fn folded_calc_equals_literal_px() {
    same("<div class=c style='height:20px;width:calc(50px + 10px)'></div><div class=d style='height:calc(5px + 5px)'></div>", "<div class=c style='height:20px;width:60px'></div><div class=d style='height:10px'></div>");
    same("<div class=c style='height:calc(30px - 10px);margin-left:calc(10px + 10px);margin-top:calc(4px + 4px)'></div><div class=d style='height:10px;padding-left:calc(2px + 3px);padding-top:calc(1px + 1px)'></div>", "<div class=c style='height:20px;margin-left:20px;margin-top:8px'></div><div class=d style='height:10px;padding-left:5px;padding-top:2px'></div>");
    same("<div class=c style='height:calc(10px + 10px + 10px - 5px)'></div><div class=d style='height:10px;width:calc(100px - 100px)'></div>", "<div class=c style='height:25px'></div><div class=d style='height:10px;width:0px'></div>");
    same("<div class=c style='height:10px;max-width:calc(30px + 20px)'></div><div class=d style='height:10px;min-width:calc(30px + 20px);width:10px'></div>", "<div class=c style='height:10px;max-width:50px'></div><div class=d style='height:10px;min-width:50px;width:10px'></div>");
    same("<div class=c style='height:20px;width:100px;width:calc(40px + 10px)'></div><div class=d style='height:10px;min-height:calc(15px + 5px)'></div>", "<div class=c style='height:20px;width:100px;width:50px'></div><div class=d style='height:10px;min-height:20px'></div>");
    // cascade: !important, later declaration, case-insensitive function name, display:none box
    same("<div class=c style='height:20px;width:calc(10px - 5px) !important;width:100px'></div>", "<div class=c style='height:20px;width:5px !important;width:100px'></div>");
    same("<div class=c style='height:20px;width:CALC(10px + 5px)'></div>", "<div class=c style='height:20px;width:15px'></div>");
    same("<div class=c style='height:calc(7px + 3px);display:none'></div><div class=d style='height:10px'></div>", "<div class=d style='height:10px'></div>");
}
#[test]
fn anything_else_keeps_refusing() {
    for v in [
        "width:calc(10px - 20px)", "width:calc(100px*2)", "width:calc(100px / 2)", "width:calc(100px+10px)", "width:calc(100px + 10px", "width:calc(50% + 10px)",
        "width:calc(10px + 5em)", "width:calc(10px + 5)", "width:calc(10px + 0)", "width:calc(10px)", "width:calc( 10px + 5px)", "width:calc(10px  + 5px)", "width:calc(10px + (5px))",
        "width:calc(calc(1px + 1px) + 1px)", "width:calc(1.5px + 1px)", "width:calc(-1px + 5px)", "width:calc(10px + 5px) calc(1px + 1px)", "margin:calc(1px + 1px)",
        "padding:calc(1px + 1px)", "width:calc(1px + 1px) 3px", "width:calc(9999999px + 1px)", "border-top-width:calc(1px + 1px)", "left:calc(1px + 1px)",
        "width:calc(var(--a) + 1px)", "margin-left:calc(10px - 20px)", "margin-top:calc(1px - 2px)", "width:calc(10px + 5px +)", "width:calc(10px + 5px + )", "width:calc(10px + 5px -)", "width:calc(999999px + 999999px)", "width:calc(1000000px + 1px)", "width:calc(0000001px + 1px)", "width:calc(1000000px + 0px)", "height:calc(10px - 20px)", "padding-left:calc(2px - 3px)",
    ] {
        refused(&format!("<div class=c style='height:20px;{v}'></div>"));
    }
}
#[test]
fn other_entries_keep_refusing_calc() {
    let h = page("<div class=c style='height:20px;width:calc(50px + 10px)'></div>");
    for tree in [parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT), parse_style_tree_with_pinned_font_family(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL), parse_style_tree_with_media(h.as_bytes(), &[], &Limits::DEFAULT, MediaContext::new(785).unwrap())] {
        assert!(tree.unsupported);
    }
}
