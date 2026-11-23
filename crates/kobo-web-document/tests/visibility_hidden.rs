//! PRIVATE, unadopted candidate (FL38): `visibility:hidden|visible` accepted only by the ext opt-in entry. A hidden box keeps its
//! layout and paints no background; any other paint source, a visible descendant of a hidden box, html/body hidden all refuse.
//! Engine Fill rects only (no Chrome/stored-pixel comparison here; the Chrome comparison is in private-evidence ul1/vs52).
use kobo_web_document::{
    box_tree::BoxTree, css_background::paint_backgrounds, display_list::Command, parse_style_tree,
    parse_style_tree_with_inline_sheets_and_selectors, parse_style_tree_with_media, parse_style_tree_with_pinned_font_family,
    Limits, MediaContext, SupplyBudget,
};
const CSS: &str = ".c{background:#cde}.d{background:#fda}";
fn page(body: &str) -> String {
    format!("<!doctype html><style>{CSS}</style><body style='margin:0'>{body}")
}
fn ext(h: &str) -> kobo_web_document::style_tree::StyleTree {
    parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
}
/// None = refused at style or tree level; Some(fills) = admitted.
fn fills(h: &str) -> Option<Vec<(i32, i32, u32, u32, u8)>> {
    let t = ext(h);
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
fn count(h: &str) -> usize {
    fills(h).unwrap_or_else(|| panic!("refused: {h}")).len()
}
#[test]
fn hidden_box_keeps_space_and_paints_nothing() {
    let hid = page("<div class=c style='height:30px;visibility:hidden'></div><div class=d style='height:10px'></div>");
    let vis = page("<div class=c style='height:30px'></div><div class=d style='height:10px'></div>");
    let h = fills(&hid).unwrap();
    let v = fills(&vis).unwrap();
    assert_eq!(h.len() + 1, v.len());
    // the following box starts at y=30 in both: layout unchanged
    assert!(h.iter().any(|f| f.1 == 30 && f.3 == 10), "{h:?}");
    assert!(v.iter().any(|f| f.1 == 30 && f.3 == 10), "{v:?}");
    assert!(!h.iter().any(|f| f.1 == 0 && f.3 == 30), "{h:?}");
}
#[test]
fn inherited_by_descendants_and_explicit_forms() {
    let one = "<div style='visibility:hidden'><div class=c style='height:30px'><div class=d style='height:10px'></div></div></div><div class=d style='height:10px'></div>";
    assert_eq!(count(&page(one)), count(&page("<div class=d style='height:10px'></div>")));
    // inherit / hidden repeated / visible on a visible parent are admitted
    assert!(fills(&page("<div style='visibility:hidden'><div class=c style='height:30px;visibility:inherit'></div></div>")).is_some());
    assert_eq!(count(&page("<div class=c style='height:30px;visibility:hidden;visibility:visible'></div>")), count(&page("<div class=c style='height:30px'></div>")));
    assert_eq!(count(&page("<div class=c style='height:30px;visibility:visible'></div>")), count(&page("<div class=c style='height:30px'></div>")));
}
#[test]
fn hidden_non_inherit_reset_and_visible_child_refuse() {
    for b in [
        "<div style='visibility:hidden'><div class=d style='height:20px;visibility:visible'></div></div>",
        "<div style='visibility:hidden'><div class=d style='height:20px;visibility:initial'></div></div>",
        "<div style='visibility:hidden'><div style='visibility:visible'><div class=d style='height:5px'></div></div></div>",
        "<div class=c style='height:30px;visibility:collapse'></div>",
        "<div class=c style='height:30px;visibility:Hidden2'></div>",
        "<div class=c style='height:30px;visibility:hidden !important;visibility:visible'></div>x",
    ] {
        assert!(fills(&page(b)).is_none(), "{b}");
    }
}
#[test]
fn other_paint_sources_refuse() {
    for b in [
        "<div class=c style='height:30px;visibility:hidden;border:2px solid #000'></div>",
        "<div class=c style='height:30px;visibility:hidden;outline:2px solid red'></div>",
        "<div class=c style='height:30px;visibility:hidden;box-shadow:4px 4px 0 #333'></div>",
        "<div class=c style='height:30px;visibility:hidden'>text</div>",
        "<div style='visibility:hidden'><div class=c style='height:30px'>text</div></div>",
        "<div style='visibility:hidden'><div class=c style='height:30px'><span>t</span></div></div>",
    ] {
        assert!(fills(&page(b)).is_none(), "{b}");
    }
    // border-style:none / zero width is no border and admitted
    assert!(fills(&page("<div class=c style='height:30px;visibility:hidden;border:0 solid #000'></div>")).is_some());
    assert!(fills(&page("<div class=c style='height:30px;visibility:hidden;border:3px none #000'></div>")).is_some());
}
#[test]
fn html_and_body_hidden_refuse() {
    assert!(fills("<!doctype html><style>html{visibility:hidden;background:#cde}</style><body>").is_none());
    assert!(fills("<!doctype html><style>body{visibility:hidden;background:#cde}</style><div style='height:5px'></div>").is_none());
    assert!(fills("<!doctype html><style>body{visibility:hidden}</style><div style='height:5px'></div>").is_none());
    assert!(fills("<!doctype html><style>html{visibility:hidden}</style><div style='height:5px'></div>").is_none());
}
#[test]
fn no_box_and_empty_inline_shapes() {
    // display:none boxes (hidden or not) create no box: nothing to refuse or paint
    assert_eq!(count(&page("<div class=c style='height:30px;display:none;visibility:hidden'></div><div class=d style='height:10px'></div>")), count(&page("<div class=d style='height:10px'></div>")));
    // conservative: a visible display:none child of a hidden parent still refuses (the box exists in the tree)
    assert!(fills(&page("<div style='visibility:hidden'><div style='display:none;visibility:visible'></div></div><div class=d style='height:10px'></div>")).is_none());
    // zero-height hidden box keeps a zero-size gap and paints nothing
    assert_eq!(fills(&page("<div class=c style='visibility:hidden'></div><div class=d style='height:10px'></div>")).is_some(), fills(&page("<div class=c></div><div class=d style='height:10px'></div>")).is_some());
    // hidden positioned (absolute) child: not painted, others unchanged
    assert_eq!(count(&page("<div class=c style='position:absolute;top:5px;left:5px;width:20px;height:20px;visibility:hidden'></div><div class=d style='height:10px'></div>")), count(&page("<div class=d style='height:10px'></div>")));
    // hidden inline-level empty span with background: still refused or unpainted, never painted
    if let Some(f) = fills(&page("<div class=d style='height:10px'><span class=c style='visibility:hidden'></span></div>")) {
        assert_eq!(f.len(), count(&page("<div class=d style='height:10px'></div>")));
    }
}
#[test]
fn other_entries_keep_refusing() {
    let h = page("<div class=c style='height:30px;visibility:hidden'></div>");
    let v = page("<div class=c style='height:30px;visibility:visible'></div>");
    for s in [&h, &v] {
        assert!(parse_style_tree(s.as_bytes(), &[], &Limits::DEFAULT).unsupported);
        assert!(parse_style_tree_with_pinned_font_family(s.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL).unsupported);
        assert!(parse_style_tree_with_media(s.as_bytes(), &[], &Limits::DEFAULT, MediaContext::new(785).unwrap()).unsupported);
    }
}
#[test]
fn unset_and_revert_inherit_the_hidden_parent() {
    // visibility is inherited: `unset` = inherit; `revert` rolls back to the UA sheet (no declaration) = inherited.
    let base = count(&page("<div class=d style='height:10px'></div>"));
    for kw in ["unset", "revert", "inherit"] {
        let h = format!("<div style='visibility:hidden'><div class=c style='height:20px;visibility:{kw}'></div></div><div class=d style='height:10px'></div>");
        assert_eq!(count(&page(&h)), base, "{kw}");
    }
}
#[test]
fn display_none_child_under_hidden_parent_is_not_a_box() {
    // FL38b correction: the earlier assertion that refused an EMPTY hidden parent was vacuous. An empty zero-height block
    // (visible or hidden) is refused by the pre-existing painter, which is the real refusing layer there. With a height on the
    // parent, a display:none child (visible or not) creates no box, so the tree is admitted and paints like the plain page.
    let base = count(&page("<div class=d style='height:10px'></div>"));
    for inner in ["<div style='display:none;visibility:visible'></div>", "<div style='display:none'></div>"] {
        let h = format!("<div style='height:5px;visibility:hidden'>{inner}</div><div class=d style='height:10px'></div>");
        assert_eq!(count(&page(&h)), base, "{inner}");
    }
    // the empty-parent shape is refused for the visible twin too (pre-existing painter layer, not visibility)
    assert!(fills(&page("<div></div><div class=d style='height:10px'></div>")).is_none());
}
