//! PRIVATE, unadopted candidate (FL43): `overflow:hidden` on a static content-box block with an explicit px height is admitted (ext
//! only) when a conservative upper bound of its descendants' stacked margin-box extent fits in that height, so no clip can matter.
//! Engine Fill rects only (no Chrome/stored-pixel comparison here; Chrome comparison is private-evidence ul1/oh59, my own rasteriser).
use kobo_web_document::{
    box_tree::BoxTree, css_background::paint_backgrounds, display_list::Command, parse_style_tree,
    parse_style_tree_with_inline_sheets_and_selectors, parse_style_tree_with_media, parse_style_tree_with_pinned_font_family, Limits,
    MediaContext, SupplyBudget,
};
type F = (i32, i32, u32, u32, u8);
fn page(body: &str) -> String {
    format!("<!doctype html><style>.c{{background:#cde}}.d{{background:#fda}}.e{{background:#aed}}.h{{background:#eee;overflow:hidden}}</style><body style='margin:0'>{body}")
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
fn ok(b: &str) -> Vec<F> {
    run(&page(b)).unwrap_or_else(|| panic!("refused: {b}"))
}
fn no(b: &str) {
    assert!(run(&page(b)).is_none(), "admitted: {b}");
}
#[test]
fn fitting_children_admit_and_equal_the_visible_twin() {
    for (h, kids) in [
        (50, "<div class=c style='height:30px'></div>"),
        (50, "<div class=c style='height:30px'></div><div class=d style='height:20px'></div>"),
        (70, "<div class=c style='height:20px;margin-bottom:10px'></div><div class=d style='height:20px;margin-top:20px'></div>"),
        (50, "<div><div class=c style='height:20px'></div><div><div class=d style='height:20px'></div></div></div>"),
        (40, "<div class=c style='height:20px;display:none'></div><div class=d style='height:40px'></div>"),
    ] {
        let hid = ok(&format!("<div class=h style='height:{h}px'>{kids}</div><div class=e style='height:10px'></div>"));
        // twin: same page, hidden box NOT overflow:hidden (a background-only paint is identical when nothing overflows)
        let vis = ok(&format!("<div style='background:#eee;height:{h}px'>{kids}</div><div class=e style='height:10px'></div>"));
        assert_eq!(hid, vis, "{kids}");
        assert!(hid.contains(&(0, i32::try_from(h).unwrap(), 800, 10, 0xaa)) || hid.iter().any(|f| f.1 == i32::try_from(h).unwrap()), "next block follows the explicit height");
    }
}
#[test]
fn bound_exceeded_or_unmodelled_refuses() {
    no("<div class=h style='height:50px'><div class=c style='height:30px'></div><div class=d style='height:30px'></div></div>"); // overflow
    no("<div class=h style='height:65px'><div class=c style='height:25px;margin-bottom:10px'></div><div class=d style='height:25px;margin-top:20px'></div></div>"); // bound 80 > 65 (conservative)
    no("<div class=h style='height:40px'><div class=c style='height:30px;margin-top:20px'></div></div>");
    no("<div class=h style='height:50px'><div class=c style='height:30px'><div class=d style='height:25px'></div></div></div>"); // px child with children
    no("<div class=h style='height:49px'><div class=c style='height:25px'></div><div class=d style='height:25px'></div></div>"); // 1px over
    no("<div class=h style='height:50px'><div class=c style='height:30px;margin-top:-5px'></div></div>"); // negative margin
    no("<div class=h style='height:30px'><div><div class=c style='height:40px'></div></div></div>"); // overflow through an auto wrapper
    no("<div class=h style='height:30px'><div><div><div class=c style='height:20px'></div><div class=d style='height:20px'></div></div></div></div>");
    no("<div class=h style='height:50px'><div class=c style='height:50%'></div></div>");
    no("<div class=h style='height:50px;box-sizing:border-box'><div class=c style='height:30px'></div></div>");
    no("<div class=h style='height:50px'></div>"); // no children
    no("<div class=h style='height:50px;position:relative'><div class=c style='height:30px'></div></div>");
    no("<div class=h style='height:50px;float:left;width:20px'><div class=c style='height:30px'></div></div>");
    no("<div class=h style='height:50px;display:inline-block'><div class=c style='height:30px'></div></div>");
    no("<div class=h style='height:50px'>text</div>");
}
#[test]
fn min_height_is_folded_before_the_bound() {
    // FL39 folds a px min-height into the px height, so min-height<=height is a no-op and min-height>height raises the height.
    let base = ok("<div class=h style='height:50px'><div class=c style='height:30px'></div></div><div class=e style='height:5px'></div>");
    assert_eq!(base, ok("<div class=h style='height:50px;min-height:10px'><div class=c style='height:30px'></div></div><div class=e style='height:5px'></div>"));
    assert_eq!(ok("<div class=h style='height:60px'><div class=c style='height:30px'></div></div><div class=e style='height:5px'></div>"), ok("<div class=h style='height:50px;min-height:60px'><div class=c style='height:30px'></div></div><div class=e style='height:5px'></div>"));
    no("<div class=h style='height:20px;min-height:10px'><div class=c style='height:30px'></div></div>");
}
#[test]
fn padding_counts_in_child_bound_but_not_in_hidden_box_height() {
    ok("<div class=h style='height:60px;padding:5px 7px;width:300px'><div><div class=c style='height:20px;padding:3px'></div></div><div class=d style='height:10px;margin:4px 0'></div></div>");
    // child bound 20+6+10+8 = 44 fits 44, 43 refuses
    ok("<div class=h style='height:44px'><div class=c style='height:20px;padding:3px'></div><div class=d style='height:10px;margin:4px 0'></div></div>");
    no("<div class=h style='height:43px'><div class=c style='height:20px;padding:3px'></div><div class=d style='height:10px;margin:4px 0'></div></div>");
}
#[test]
fn other_entries_keep_refusing() {
    let h = page("<div class=h style='height:50px'><div class=c style='height:30px'></div></div>");
    for tree in [parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT), parse_style_tree_with_pinned_font_family(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL), parse_style_tree_with_media(h.as_bytes(), &[], &Limits::DEFAULT, MediaContext::new(785).unwrap())] {
        let admitted = !tree.unsupported && {
            let bt = BoxTree::from_style(&tree);
            !bt.unsupported && paint_backgrounds(&bt, 800, 600).is_ok()
        };
        assert!(!admitted);
    }
}
