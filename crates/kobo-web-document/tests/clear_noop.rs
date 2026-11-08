//! PRIVATE, unadopted candidate (FL42): `clear:both` on a page with NO float anywhere is a no-op (no clearance without a float), so
//! the ext entry resets it before layout. Engine Fill rects against a twin without `clear` (no Chrome/stored-pixel comparison here;
//! Chrome comparison is private-evidence ul1/cl58). `clear:left|right` are still not parsed and refuse.
use kobo_web_document::{
    box_tree::BoxTree, css_background::{paint_backgrounds, paint_backgrounds_float_kernel}, display_list::Command, parse_style_tree,
    parse_style_tree_with_inline_sheets_and_selectors, parse_style_tree_with_media, parse_style_tree_with_pinned_font_family,
    Limits, MediaContext, SupplyBudget,
};
type F = (i32, i32, u32, u32, u8);
fn page(body: &str) -> String {
    format!("<!doctype html><style>.c{{background:#cde}}.d{{background:#fda}}.e{{background:#aed}}</style><body style='margin:0'>{body}")
}
fn run(h: &str, kernel: bool) -> Option<Vec<F>> {
    let t = parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    if t.unsupported {
        return None;
    }
    let bt = BoxTree::from_style(&t);
    if bt.unsupported {
        return None;
    }
    let l = if kernel { paint_backgrounds_float_kernel(&bt, 800, 600) } else { paint_backgrounds(&bt, 800, 600) }.ok()?;
    Some(l.commands().iter().filter_map(|c| if let Command::Fill { rect, color, .. } = c { Some((rect.x, rect.y, rect.width, rect.height, color.0)) } else { None }).collect())
}
fn same(a: &str, b: &str) {
    let (x, y) = (run(&page(a), false), run(&page(b), false));
    assert!(x.is_some(), "refused: {a}");
    assert_eq!(x, y, "{a} vs {b}");
}
#[test]
fn clear_without_floats_is_a_noop() {
    same("<div class=c style='height:20px;clear:both'></div><div class=d style='height:10px'></div>", "<div class=c style='height:20px'></div><div class=d style='height:10px'></div>");
    same("<div class=c style='height:20px;clear:both;margin:12px 0 7px'></div><div class=d style='height:10px;margin-top:3px'></div>", "<div class=c style='height:20px;margin:12px 0 7px'></div><div class=d style='height:10px;margin-top:3px'></div>");
    // nested, margins collapsing around a clear box
    same("<div class=c style='height:20px'><div class=d style='height:10px;clear:both;margin-top:9px'></div></div><div class=e style='height:10px'></div>", "<div class=c style='height:20px'><div class=d style='height:10px;margin-top:9px'></div></div><div class=e style='height:10px'></div>");
    same("<div style='clear:both;margin-top:11px'><div class=c style='height:20px;margin-top:6px'></div></div><div class=d style='height:10px'></div>", "<div style='margin-top:11px'><div class=c style='height:20px;margin-top:6px'></div></div><div class=d style='height:10px'></div>");
    same("<div class=c style='height:20px;margin-bottom:15px'></div><div class=e style='clear:both;margin-top:25px'><div class=d style='height:10px'></div></div>", "<div class=c style='height:20px;margin-bottom:15px'></div><div class=e style='margin-top:25px'><div class=d style='height:10px'></div></div>");
    // clear:none and several clears on one page
    same("<div class=c style='height:20px;clear:none'></div><div class=d style='height:10px;clear:both'></div>", "<div class=c style='height:20px'></div><div class=d style='height:10px'></div>");
    // global keywords are parsed by the shared keyword path; with no float they are no-ops too
    same("<div style='clear:both'><div class=c style='height:20px;clear:inherit'></div></div><div class=d style='height:10px;clear:initial'></div>", "<div><div class=c style='height:20px'></div></div><div class=d style='height:10px'></div>");
    // a display:none clear box creates no box
    same("<div class=c style='height:20px;clear:both;display:none'></div><div class=d style='height:10px'></div>", "<div class=d style='height:10px'></div>");
}
#[test]
fn zero_basis_and_unparsed_forms_stay_refused() {
    // an empty zero-height clear block: refused by the pre-existing empty-block rule (its clear-free twin refuses too)
    let z = page("<div class=c style='height:20px;margin-bottom:15px'></div><div style='height:0;clear:both'></div><div class=d style='height:10px;margin-top:25px'></div>");
    let zt = page("<div class=c style='height:20px;margin-bottom:15px'></div><div style='height:0'></div><div class=d style='height:10px;margin-top:25px'></div>");
    assert_eq!(run(&z, false).is_none(), run(&zt, false).is_none());
    assert!(run(&zt, false).is_none(), "anchor: the twin itself refuses");
    for v in ["left", "right", "inline-start", "both !important2"] {
        assert!(run(&page(&format!("<div class=c style='height:20px;clear:{v}'></div><div class=d style='height:10px'></div>")), false).is_none(), "{v}");
    }
}
#[test]
fn any_float_keeps_every_clear_as_written() {
    // a kernel (float + cleared block) is unchanged: admitted by the kernel painter only
    let k = page("<div style='width:200px'><div class=c style='float:left;width:50px;height:20px'></div><div class=d style='clear:both;height:10px'></div></div><div class=e style='height:5px'></div>");
    assert!(run(&k, true).is_some(), "anchor: plain kernel paints");
    assert!(run(&k, false).is_none(), "the generic painter still refuses a kernel");
    // a float elsewhere plus a stray clear:both outside any kernel: still refused
    let stray = page("<div style='width:200px'><div class=c style='float:left;width:50px;height:20px'></div><div class=d style='clear:both;height:10px'></div></div><div class=e style='height:5px;clear:both'></div>");
    assert!(run(&stray, true).is_none());
    // the same page with the float removed admits (the stray clear is a no-op)
    let nofloat = page("<div style='width:200px'><div class=c style='width:50px;height:20px'></div><div class=d style='clear:both;height:10px'></div></div><div class=e style='height:5px;clear:both'></div>");
    assert!(run(&nofloat, false).is_some());
}
#[test]
fn other_entries_keep_refusing() {
    let h = page("<div class=c style='height:20px;clear:both'></div><div class=d style='height:10px'></div>");
    for tree in [parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT), parse_style_tree_with_pinned_font_family(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL), parse_style_tree_with_media(h.as_bytes(), &[], &Limits::DEFAULT, MediaContext::new(785).unwrap())] {
        let ok = !tree.unsupported && {
            let bt = BoxTree::from_style(&tree);
            !bt.unsupported && paint_backgrounds(&bt, 800, 600).is_ok()
        };
        assert!(!ok);
    }
}
