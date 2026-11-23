//! PRIVATE, unadopted candidate (FL40): px `max-height` on a px-`height` static in-flow content-box block is folded into the
//! height (used = max(min, min(height, max))), ext entry only. Tests compare engine Fill rects of the folded page with a twin that declares
//! the folded height directly (no Chrome/stored-pixel comparison here; Chrome comparison is private-evidence ul1/xh56).
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
const T: &str = "<div class=d style='height:10px'></div>";
#[test]
fn clamps_height_to_max() {
    same(&format!("<div class=c style='height:60px;max-height:40px'></div>{T}"), &format!("<div class=c style='height:40px'></div>{T}"));
    same(&format!("<div class=c style='height:30px;max-height:40px'></div>{T}"), &format!("<div class=c style='height:30px'></div>{T}"));
    same(&format!("<div class=c style='height:40px;max-height:40px'></div>{T}"), &format!("<div class=c style='height:40px'></div>{T}"));
    // min wins over max (CSS 2 10.7)
    same(&format!("<div class=c style='height:60px;max-height:20px;min-height:40px'></div>{T}"), &format!("<div class=c style='height:40px'></div>{T}"));
    same(&format!("<div class=c style='height:10px;max-height:20px;min-height:40px'></div>{T}"), &format!("<div class=c style='height:40px'></div>{T}"));
    same(&format!("<div class=c style='height:60px;max-height:50px;min-height:30px'></div>{T}"), &format!("<div class=c style='height:50px'></div>{T}"));
    // max-height:none and px declarations that do not bind
    same(&format!("<div class=c style='height:60px;max-height:none'></div>{T}"), &format!("<div class=c style='height:60px'></div>{T}"));
    same(&format!("<div class=c style='height:60px;max-height:40px;max-height:none'></div>{T}"), &format!("<div class=c style='height:60px'></div>{T}"));
    same(&format!("<div class=c style='height:60px;max-height:80px;max-height:40px'></div>{T}"), &format!("<div class=c style='height:40px'></div>{T}"));
    // padding and border are outside a content-box max-height
    same(&format!("<div class=c style='height:60px;max-height:40px;padding:5px;border:2px solid #333'></div>{T}"), &format!("<div class=c style='height:40px;padding:5px;border:2px solid #333'></div>{T}"));
}
#[test]
fn nested_and_margin_shapes() {
    same("<div class=c style='height:60px;max-height:40px'><div class=d style='height:100px'></div></div><div class=e style='height:10px'></div>",
         "<div class=c style='height:40px'><div class=d style='height:100px'></div></div><div class=e style='height:10px'></div>");
    same("<div class=c style='height:60px;max-height:40px'><div class=d style='height:10px;margin:5px 0 70px'></div></div><div class=e style='height:10px'></div>",
         "<div class=c style='height:40px'><div class=d style='height:10px;margin:5px 0 70px'></div></div><div class=e style='height:10px'></div>");
    same("<div style='height:100px;background:#eee'><div class=c style='height:80px;max-height:30px;width:50%'></div></div>",
         "<div style='height:100px;background:#eee'><div class=c style='height:30px;width:50%'></div></div>");
    same("<div class=c style='height:60px;max-height:40px;margin:10px 0 15px'></div><div class=d style='height:10px;margin-top:20px'></div>",
         "<div class=c style='height:40px;margin:10px 0 15px'></div><div class=d style='height:10px;margin-top:20px'></div>");
    same("<div class=c style='height:60px;max-height:40px;margin-top:-15px'></div><div class=d style='height:10px'></div>",
         "<div class=c style='height:40px;margin-top:-15px'></div><div class=d style='height:10px'></div>");
    same("<div class=c style='height:60px;max-height:40px;display:none'></div><div class=d style='height:10px'></div>", "<div class=d style='height:10px'></div>");
}
#[test]
fn cascade_not_inherited_and_resets() {
    // not inherited: the child of a max-height box is not clamped by it
    same("<div style='height:100px;max-height:50px;background:#eee'><div class=c style='height:80px'></div></div><div class=d style='height:10px'></div>",
         "<div style='height:50px;background:#eee'><div class=c style='height:80px'></div></div><div class=d style='height:10px'></div>");
    // explicit inherit takes the parent's value; initial/unset reset to none; revert falls back to the UA (none)
    same("<div style='height:100px;max-height:30px;background:#eee'><div class=c style='height:80px;max-height:inherit'></div></div><div class=d style='height:10px'></div>",
         "<div style='height:30px;background:#eee'><div class=c style='height:30px'></div></div><div class=d style='height:10px'></div>");
    for kw in ["initial", "unset", "revert"] {
        same(&format!("<div class=c style='height:60px;max-height:40px;max-height:{kw}'></div>{T}"), &format!("<div class=c style='height:60px'></div>{T}"));
    }
    same(&format!("<style>.m{{max-height:40px}}</style><div class='c m' style='height:60px'></div>{T}"), &format!("<div class=c style='height:40px'></div>{T}"));
    same(&format!("<style>.m{{max-height:40px !important}}</style><div class='c m' style='height:60px;max-height:50px'></div>{T}"), &format!("<div class=c style='height:40px'></div>{T}"));
}
#[test]
fn out_of_scope_shapes_stay_refused() {
    for b in [
        "<div class=c style='max-height:40px'><div class=d style='height:100px'></div></div>",
        "<div class=c style='height:60px;max-height:40px;box-sizing:border-box;padding:5px'></div>",
        "<div class=c style='position:relative;height:60px;max-height:40px'></div>",
        "<div class=c style='position:absolute;height:60px;max-height:40px;width:10px'></div>",
        "<div style='display:flex'><div class=c style='height:60px;max-height:40px;width:50px'></div></div>",
        "<div class=c style='float:left;width:30px;height:60px;max-height:40px'></div>",
        "<div class=c style='height:60px;max-height:50%'></div>",
        // FL44 (disclosed edit): this entry used `calc(10px + 5px)`, which the ext entry now folds to px; replaced with a still-refused calc(10px * 2). Pre-change copy in the FL44 probe pack.
        "<div class=c style='height:60px;max-height:calc(10px * 2)'></div>",
        "<div class=c style='height:60px;max-height:2em'></div>",
        "<div class=c style='height:10%;max-height:40px'></div>",
        "<div class=c style='height:60px;max-height:-5px'></div>",
        "<div class=c style='height:60px;max-height:fit-content'></div>",
        "<div class=c style='height:60px;max-height:40px'><div class=d style='position:absolute;top:0;left:0;width:5px;height:5px'></div></div>",
    ] {
        assert!(fills(&page(&format!("{b}{T}"))).is_none(), "{b}");
    }
    // html and the auto-height body shape: no parent / auto height, refuse
    assert!(fills("<!doctype html><style>html{height:60px;max-height:40px}</style><body style='margin:0'><div style='height:5px;background:#cde'></div>").is_none());
    // zero-basis: max-height:0 on a px-height box folds to an empty block, which the pre-existing painter refuses (visible twin too)
    assert!(fills(&page(&format!("<div class=c style='height:60px;max-height:0'></div>{T}"))).is_none());
    assert!(fills(&page(&format!("<div class=c style='height:0'></div>{T}"))).is_none());
}
#[test]
fn float_kernel_members_and_parent_stay_refused_with_height_and_max() {
    let kernel = |pstyle: &str, fstyle: &str, cstyle: &str| {
        page(&format!("<div style='width:200px;{pstyle}'><div style='float:left;width:50px;height:20px;{fstyle}' class=c></div><div style='clear:both;height:10px;{cstyle}' class=d></div></div><div class=e style='height:5px'></div>"))
    };
    assert!(kfills(&kernel("", "", "")).is_some(), "anchor: plain kernel must paint");
    assert!(kfills(&kernel("", "max-height:10px", "")).is_none(), "float member");
    assert!(kfills(&kernel("", "", "max-height:5px")).is_none(), "cleared member");
    assert!(kfills(&kernel("height:60px;max-height:40px", "", "")).is_none(), "kernel parent");
    let vb = |cstyle: &str| page(&format!("<div style='width:200px'><div style='float:left;width:50px;height:20px' class=c></div><div style='height:10px;{cstyle}' class=d></div></div><div class=e style='height:5px'></div>"));
    assert!(kfills(&vb("")).is_some(), "anchor: variant B kernel paints");
    assert!(kfills(&vb("max-height:5px")).is_none(), "uncleared kernel member");
    let sib = page("<div style='width:200px'><div style='float:left;width:50px;height:20px' class=c></div><div style='clear:both;height:10px' class=d></div></div><div class=e style='height:50px;max-height:20px'></div>");
    let twin = page("<div style='width:200px'><div style='float:left;width:50px;height:20px' class=c></div><div style='clear:both;height:10px' class=d></div></div><div class=e style='height:20px'></div>");
    assert!(kfills(&sib).is_some());
    assert_eq!(kfills(&sib), kfills(&twin));
}
#[test]
fn other_entries_keep_refusing() {
    let h = page("<div class=c style='height:60px;max-height:40px'></div>");
    assert!(parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported);
    assert!(parse_style_tree_with_pinned_font_family(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL).unsupported);
    assert!(parse_style_tree_with_media(h.as_bytes(), &[], &Limits::DEFAULT, MediaContext::new(785).unwrap()).unsupported);
}
