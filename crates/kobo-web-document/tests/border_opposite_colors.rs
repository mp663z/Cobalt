//! PRIVATE, unadopted candidate (FL41): different side colours are admitted by the ext entry ONLY when the painted (nonzero) sides
//! are an opposite pair or one side, so no two painted sides meet at a corner (no diagonal join). Engine Fill rects against explicit
//! expected rects (no Chrome/stored-pixel comparison here; the Chrome comparison is private-evidence ul1/bc57).
use kobo_web_document::{
    box_tree::BoxTree, css_background::paint_backgrounds, display_list::Command, parse_style_tree,
    parse_style_tree_with_inline_sheets_and_selectors, parse_style_tree_with_media, parse_style_tree_with_pinned_font_family,
    Limits, MediaContext, SupplyBudget,
};
type F = (i32, i32, u32, u32, u8, u8, u8);
fn page(body: &str) -> String {
    format!("<!doctype html><style>.c{{background:#cde}}.d{{background:#fda}}</style><body style='margin:0'>{body}")
}
fn fills(h: &str) -> Option<Vec<F>> {
    let t = parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    if t.unsupported {
        return None;
    }
    let bt = BoxTree::from_style(&t);
    if bt.unsupported {
        return None;
    }
    let l = paint_backgrounds(&bt, 800, 600).ok()?;
    Some(l.commands().iter().filter_map(|c| if let Command::Fill { rect, color, .. } = c { Some((rect.x, rect.y, rect.width, rect.height, color.0, color.1, color.2)) } else { None }).collect())
}
const TAIL: &str = "<div class=d style='height:10px'></div>";
#[test]
fn top_bottom_pair_paints_each_side_in_its_colour() {
    let f = fills(&page(&format!("<div class=c style='height:20px;border-top:5px solid #f00;border-bottom:5px solid #00f'></div>{TAIL}"))).unwrap();
    // canvas is white (no fill); box background covers the border box, then top, bottom strips
    assert!(f.contains(&(0, 0, 800, 5, 255, 0, 0)), "{f:?}");
    assert!(f.contains(&(0, 25, 800, 5, 0, 0, 255)), "{f:?}");
    assert!(f.contains(&(0, 0, 800, 30, 0xcc, 0xdd, 0xee)), "{f:?}");
    assert!(f.contains(&(0, 30, 800, 10, 0xff, 0xdd, 0xaa)), "{f:?}");
    assert_eq!(f.len(), 4);
}
#[test]
fn left_right_pair_and_unequal_widths_and_padding() {
    let f = fills(&page(&format!("<div class=c style='height:20px;padding:4px 6px;border-left:7px solid #f00;border-right:2px solid #00f;margin:5px 9px'></div>{TAIL}"))).unwrap();
    // border box: x 9, y 5, w 800-18=782, h 20+8=28
    assert!(f.contains(&(9, 5, 7, 28, 255, 0, 0)), "{f:?}");
    assert!(f.contains(&(9 + 782 - 2, 5, 2, 28, 0, 0, 255)), "{f:?}");
    let g = fills(&page(&format!("<div class=c style='height:20px;border-top:3px solid #f00;border-bottom:8px solid #0a0'></div>{TAIL}"))).unwrap();
    assert!(g.contains(&(0, 0, 800, 3, 255, 0, 0)) && g.contains(&(0, 3 + 20, 800, 8, 0, 0xaa, 0)), "{g:?}");
}
#[test]
fn transparent_and_currentcolor_sides_in_the_pair() {
    let f = fills(&page(&format!("<div class=c style='height:20px;border-top:5px solid #f00;border-bottom:5px solid transparent'></div>{TAIL}"))).unwrap();
    assert!(f.contains(&(0, 0, 800, 5, 255, 0, 0)) && !f.iter().any(|r| r.1 == 25 && r.3 == 5), "{f:?}");
    let g = fills(&page(&format!("<div class=c style='height:20px;color:#080;border-top:5px solid;border-bottom:5px solid #00f'></div>{TAIL}"))).unwrap();
    assert!(g.contains(&(0, 0, 800, 5, 0, 0x88, 0)) && g.contains(&(0, 25, 800, 5, 0, 0, 255)), "{g:?}");
}
#[test]
fn shorthand_forms_with_zero_width_sides() {
    // right/left colours are irrelevant when their widths are zero
    let f = fills(&page(&format!("<div class=c style='height:20px;border-style:solid;border-width:5px 0 5px 0;border-color:#f00 #0f0 #00f #0ff'></div>{TAIL}"))).unwrap();
    assert!(f.contains(&(0, 0, 800, 5, 255, 0, 0)) && f.contains(&(0, 25, 800, 5, 0, 0, 255)), "{f:?}");
    assert_eq!(f.len(), 4);
}
#[test]
fn single_colour_cases_are_unchanged() {
    let f = fills(&page(&format!("<div class=c style='height:20px;border-top:5px solid #f00;border-bottom:5px solid #f00'></div>{TAIL}"))).unwrap();
    assert!(f.contains(&(0, 0, 800, 5, 255, 0, 0)) && f.contains(&(0, 25, 800, 5, 255, 0, 0)));
    let g = fills(&page(&format!("<div class=c style='height:20px;border:5px solid #f00'></div>{TAIL}"))).unwrap();
    assert_eq!(g.iter().filter(|r| (r.4, r.5, r.6) == (255, 0, 0)).count(), 4, "{g:?}");
}
#[test]
fn adjacent_or_patterned_different_colours_stay_refused() {
    for b in [
        "<div class=c style='height:20px;border-top:5px solid #f00;border-left:5px solid #00f'></div>",
        "<div class=c style='height:20px;border:5px solid;border-color:#f00 #00f'></div>",
        "<div class=c style='height:20px;border-top:5px solid #f00;border-right:5px solid #00f;border-bottom:5px solid #0a0'></div>",
        // each adjacent pair on its own, different colours (one corner each)
        "<div class=c style='height:20px;border-top:5px solid #f00;border-right:5px solid #00f'></div>",
        "<div class=c style='height:20px;border-right:5px solid #f00;border-bottom:5px solid #00f'></div>",
        "<div class=c style='height:20px;border-bottom:5px solid #f00;border-left:5px solid #00f'></div>",
        "<div class=c style='height:20px;border-left:5px solid #f00;border-top:5px solid #00f'></div>",
        "<div class=c style='height:20px;border-top:5px solid #f00;border-bottom:5px dashed #00f'></div>",
        "<div class=c style='height:20px;border-left:5px dotted #f00;border-right:5px dotted #00f'></div>",
        "<div class=c style='height:20px;border-top:5px solid #f00;border-bottom:5px solid rgba(0,0,255,.5)'></div>",
        "<div class=c style='height:20px;border-top:5px solid #f00;border-bottom:5px solid #00f;border-radius:4px'></div>",
        "<div class=c style='height:20px;border-top:5px double #f00;border-bottom:5px double #00f'></div>",
    ] {
        assert!(fills(&page(&format!("{b}{TAIL}"))).is_none(), "{b}");
    }
}
#[test]
fn no_box_and_zero_basis_shapes() {
    // display:none creates no box, so its borders paint nothing
    let f = fills(&page(&format!("<div class=c style='display:none;height:20px;border-top:5px solid #f00;border-bottom:5px solid #00f'></div>{TAIL}"))).unwrap();
    assert_eq!(f.len(), 1, "{f:?}");
    // zero content height: only the two strips remain (pre-existing empty-block rules apply to a borderless twin, not this)
    let g = fills(&page(&format!("<div class=c style='height:0;border-top:5px solid #f00;border-bottom:5px solid #00f'></div>{TAIL}")));
    if let Some(g) = g {
        assert!(g.contains(&(0, 0, 800, 5, 255, 0, 0)) && g.contains(&(0, 5, 800, 5, 0, 0, 255)), "{g:?}");
    }
    // an inline span border never reaches the box painter
    assert!(fills(&page("<div class=d style='height:10px'><span style='border-top:2px solid #f00;border-bottom:2px solid #00f'></span></div>")).map_or(true, |f| f.iter().all(|r| (r.4, r.5, r.6) != (255, 0, 0))));
}
#[test]
fn other_entries_keep_refusing() {
    let h = page(&format!("<div class=c style='height:20px;border-top:5px solid #f00;border-bottom:5px solid #00f'></div>{TAIL}"));
    // default and pinned-family entries refuse at style or paint level
    for tree in [parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT), parse_style_tree_with_pinned_font_family(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL), parse_style_tree_with_media(h.as_bytes(), &[], &Limits::DEFAULT, MediaContext::new(785).unwrap())] {
        let ok = !tree.unsupported && {
            let bt = BoxTree::from_style(&tree);
            !bt.unsupported && paint_backgrounds(&bt, 800, 600).is_ok()
        };
        assert!(!ok);
    }
}
