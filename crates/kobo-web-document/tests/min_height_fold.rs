//! PRIVATE, unadopted candidate (FL39): positive px `min-height` on a px-`height` static in-flow content-box block is folded into the
//! height (used = max(height, min)), ext entry only. Tests compare engine Fill rects of the folded page with a twin that declares
//! the folded height directly (no Chrome/stored-pixel comparison here; Chrome comparison is private-evidence ul1/mh54).
use kobo_web_document::{
    box_tree::BoxTree, css_background::paint_backgrounds, display_list::Command, parse_style_tree,
    parse_style_tree_with_inline_sheets_and_selectors, parse_style_tree_with_media, parse_style_tree_with_pinned_font_family,
    Limits, MediaContext, SupplyBudget,
};
fn page(body: &str) -> String {
    format!("<!doctype html><style>.c{{background:#cde}}.d{{background:#fda}}.e{{background:#aed}}</style><body style='margin:0'>{body}")
}
fn fills(h: &str) -> Option<Vec<(i32, i32, u32, u32, u8)>> {
    let t = parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    if t.unsupported {
        return None;
    }
    let bt = BoxTree::from_style(&t);
    if bt.unsupported {
        return None;
    }
    let l = paint_backgrounds(&bt, 800, 600).ok()?;
    Some(l.commands().iter().filter_map(|c| if let Command::Fill { rect, color, .. } = c { Some((rect.x as i32, rect.y as i32, rect.width as u32, rect.height as u32, color.0)) } else { None }).collect())
}
/// Like `fills` but through the explicit float-kernel painter (the only painter that may draw a float kernel).
fn kfills(h: &str) -> Option<Vec<(i32, i32, u32, u32, u8)>> {
    let t = parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    if t.unsupported {
        return None;
    }
    let bt = BoxTree::from_style(&t);
    if bt.unsupported {
        return None;
    }
    let l = kobo_web_document::css_background::paint_backgrounds_float_kernel(&bt, 800, 600).ok()?;
    Some(l.commands().iter().filter_map(|c| if let Command::Fill { rect, color, .. } = c { Some((rect.x as i32, rect.y as i32, rect.width as u32, rect.height as u32, color.0)) } else { None }).collect())
}
fn same(a: &str, b: &str) {
    let (x, y) = (fills(&page(a)), fills(&page(b)));
    assert!(x.is_some(), "refused: {a}");
    assert_eq!(x, y, "{a} vs {b}");
}
#[test]
fn folds_to_max_of_height_and_min() {
    let tail = "<div class=d style='height:10px'></div>";
    same(&format!("<div class=c style='height:20px;min-height:40px'></div>{tail}"), &format!("<div class=c style='height:40px'></div>{tail}"));
    same(&format!("<div class=c style='height:50px;min-height:40px'></div>{tail}"), &format!("<div class=c style='height:50px'></div>{tail}"));
    same(&format!("<div class=c style='height:40px;min-height:40px'></div>{tail}"), &format!("<div class=c style='height:40px'></div>{tail}"));
    // zero basis: height 0 with a min does not collapse its margins through
    same(&format!("<div class=c style='height:0;min-height:30px'></div>{tail}"), &format!("<div class=c style='height:30px'></div>{tail}"));
    same(&format!("<div class=c style='height:0;min-height:20px;margin-top:30px'></div>{tail}"), &format!("<div class=c style='height:20px;margin-top:30px'></div>{tail}"));
    // padding and border do not count towards a content-box min-height
    same(&format!("<div class=c style='height:20px;min-height:40px;padding:5px 7px;border:2px solid #333'></div>{tail}"), &format!("<div class=c style='height:40px;padding:5px 7px;border:2px solid #333'></div>{tail}"));
    // later declaration order and !important
    same(&format!("<div class=c style='min-height:40px;height:20px'></div>{tail}"), &format!("<div class=c style='height:40px'></div>{tail}"));
    same(&format!("<div class=c style='height:20px;min-height:30px;min-height:50px'></div>{tail}"), &format!("<div class=c style='height:50px'></div>{tail}"));
}
#[test]
fn nested_and_margin_shapes() {
    same("<div class=c style='height:10px;min-height:60px'><div class=d style='height:30px;margin-bottom:50px'></div></div><div class=e style='height:10px'></div>",
         "<div class=c style='height:60px'><div class=d style='height:30px;margin-bottom:50px'></div></div><div class=e style='height:10px'></div>");
    same("<div class=c style='height:20px;min-height:40px'><div class=d style='height:100px'></div></div><div class=e style='height:10px'></div>",
         "<div class=c style='height:40px'><div class=d style='height:100px'></div></div><div class=e style='height:10px'></div>");
    same("<div style='height:100px;background:#eee'><div class=c style='height:10px;min-height:30px;width:50%'></div></div>",
         "<div style='height:100px;background:#eee'><div class=c style='height:30px;width:50%'></div></div>");
    same("<div class=c style='height:20px;min-height:40px;margin:10px 0 15px'></div><div class=d style='height:10px;margin-top:20px'></div>",
         "<div class=c style='height:40px;margin:10px 0 15px'></div><div class=d style='height:10px;margin-top:20px'></div>");
    same("<div class=c style='height:20px;min-height:40px;margin-top:-15px'></div><div class=d style='height:10px'></div>",
         "<div class=c style='height:40px;margin-top:-15px'></div><div class=d style='height:10px'></div>");
    // a display:none box with min-height creates no box
    same("<div class=c style='height:20px;min-height:40px;display:none'></div><div class=d style='height:10px'></div>", "<div class=d style='height:10px'></div>");
}
#[test]
fn out_of_scope_shapes_stay_refused() {
    let tail = "<div class=d style='height:10px'></div>";
    for b in [
        "<div class=c style='height:20px;min-height:40px;box-sizing:border-box;padding:5px'></div>",
        "<div class=c style='position:relative;height:20px;min-height:40px'></div>",
        "<div class=c style='position:absolute;height:20px;min-height:40px;width:10px'></div>",
        "<div style='display:flex'><div class=c style='height:20px;min-height:40px;width:50px'></div></div>",
        "<div class=c style='float:left;width:30px;height:20px;min-height:40px'></div>",
        "<div class=c style='height:20px;min-height:50%'></div>",
        // FL44 (disclosed edit): was `calc(10px + 5px)` (now folded to px under ext); replaced with a still-refused calc(10px * 2). Pre-change copy in the FL44 probe pack.
        "<div class=c style='height:20px;min-height:calc(10px * 2)'></div>",
        "<div class=c style='height:10%;min-height:40px'></div>",
        // FL40: `min-height` + `max-height` + px height is now admitted (folded); see tests/max_height_fold.rs.
        "<div class=c style='height:20px;min-height:-5px'></div>",
        "<div class=c style='height:0;min-height:20px;margin:30px 0'></div><div class=e style='height:0;margin:30px 0'></div>",
    ] {
        assert!(fills(&page(&format!("{b}{tail}"))).is_none(), "{b}");
    }
}
#[test]
fn zero_min_and_auto_height_unchanged() {
    let tail = "<div class=d style='height:10px'></div>";
    same(&format!("<div class=c style='height:20px;min-height:0'></div>{tail}"), &format!("<div class=c style='height:20px'></div>{tail}"));
    same(&format!("<div class=c style='height:20px;min-height:auto'></div>{tail}"), &format!("<div class=c style='height:20px'></div>{tail}"));
    same(&format!("<div class=c style='min-height:40px'></div>{tail}"), &format!("<div class=c style='height:40px'></div>{tail}"));
}
#[test]
fn other_entries_keep_refusing() {
    let h = page("<div class=c style='height:20px;min-height:40px'></div>");
    assert!(parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported);
    assert!(parse_style_tree_with_pinned_font_family(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL).unsupported);
    assert!(parse_style_tree_with_media(h.as_bytes(), &[], &Limits::DEFAULT, MediaContext::new(785).unwrap()).unsupported);
}
/// FL39 coverage follow-through (reviewer): the fold keeps the old `stray_min_height` float-kernel exclusions. Kernel shape: a
/// zero-edge px-width parent with exactly two plain children, a px float and a cleared (or uncleared) block.
#[test]
fn float_kernel_members_and_parent_stay_refused_with_height_and_min() {
    let kernel = |pstyle: &str, fstyle: &str, cstyle: &str| {
        page(&format!("<div style='width:200px;{pstyle}'><div style='float:left;width:50px;height:20px;{fstyle}' class=c></div><div style='clear:both;height:10px;{cstyle}' class=d></div></div><div class=e style='height:5px'></div>"))
    };
    // control: the kernel itself is admitted without min-height (anchor, so the refusals below are about min-height)
    assert!(kfills(&kernel("", "", "")).is_some(), "anchor: plain kernel must paint");
    assert!(kfills(&kernel("", "min-height:40px", "")).is_none(), "float member, height+min");
    assert!(kfills(&kernel("", "", "min-height:30px")).is_none(), "cleared member, height+min");
    // variant B: second block uncleared (neither float nor clear, so only the kernel-parent exclusion keeps it out)
    let vb = |cstyle: &str| page(&format!("<div style='width:200px'><div style='float:left;width:50px;height:20px' class=c></div><div style='height:10px;{cstyle}' class=d></div></div><div class=e style='height:5px'></div>"));
    assert!(kfills(&vb("")).is_some(), "anchor: variant B kernel paints");
    assert!(kfills(&vb("min-height:30px")).is_none(), "uncleared kernel member, height+min");
    assert!(kfills(&kernel("height:60px;min-height:80px", "", "")).is_none(), "kernel parent, height+min");
    assert!(kfills(&kernel("min-height:80px", "", "")).is_none(), "kernel parent, auto height + min (old rule)");
    // a non-kernel box next to a kernel is not part of it but the page has a kernel: the fold applies to the sibling only
    let sib = page("<div style='width:200px'><div style='float:left;width:50px;height:20px' class=c></div><div style='clear:both;height:10px' class=d></div></div><div class=e style='height:5px;min-height:20px'></div>");
    let twin = page("<div style='width:200px'><div style='float:left;width:50px;height:20px' class=c></div><div style='clear:both;height:10px' class=d></div></div><div class=e style='height:20px'></div>");
    assert!(kfills(&sib).is_some(), "sibling fold beside a kernel");
    assert_eq!(kfills(&sib), kfills(&twin));
}
#[test]
fn root_and_absolute_children_stay_refused() {
    // root-level box has a parent (body) in this tree: the fold needs a parent, never the html/body roots
    // body has a parent (html), so it folds like any box: equal to the twin that declares the folded height
    let bf = fills("<!doctype html><style>body{height:20px;min-height:40px;background:#cde;margin:0}</style><div style='height:5px;background:#fda'></div>");
    assert!(bf.is_some());
    assert_eq!(bf, fills("<!doctype html><style>body{height:40px;background:#cde;margin:0}</style><div style='height:5px;background:#fda'></div>"));
    assert!(fills("<!doctype html><style>html{height:20px;min-height:40px}</style><body style='margin:0'><div style='height:5px;background:#cde'></div>").is_none());
    // old all-static-children restriction retained: an absolute child keeps refusing (conservative, uncovered by Chrome here)
    assert!(fills(&page("<div class=c style='height:20px;min-height:40px'><div class=d style='position:absolute;top:0;left:0;width:5px;height:5px'></div></div><div class=e style='height:5px'></div>")).is_none());
}
