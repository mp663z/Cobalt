//! PRIVATE, unadopted candidate (FL45): an OPPOSITE pair of border sides (no shared corner) may carry independent styles
//! (solid/dashed/dotted) under the ext entry; each side paints alone with the existing exact-fit pattern rule. Engine Fill rects
//! (no Chrome/stored-pixel comparison here; Chrome comparison is private-evidence ul1/bp63, my own rasteriser).
use kobo_web_document::{
    box_tree::BoxTree, css_background::paint_backgrounds, display_list::Command, parse_style_tree,
    parse_style_tree_with_inline_sheets_and_selectors, parse_style_tree_with_media, parse_style_tree_with_pinned_font_family, Limits,
    MediaContext, SupplyBudget,
};
type F = (i32, i32, u32, u32, u8);
fn page(body: &str) -> String {
    format!("<!doctype html><style>.c{{background:#cde}}.d{{background:#fda}}</style><body style='margin:0'>{body}")
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
fn dashed_top_solid_bottom_paints_each_side_alone() {
    let f = ok("<div class=c style='width:298px;height:20px;border-top:1px dashed #123;border-bottom:3px solid #345'></div><div class=d style='height:10px'></div>");
    let dashes: Vec<_> = f.iter().filter(|r| r.4 == 0x11 && r.1 == 0 && r.3 == 1).collect();
    assert_eq!(dashes.len(), 60);
    assert!(dashes.iter().enumerate().all(|(i, r)| r.0 == i32::try_from(i).unwrap() * 5 && r.2 == 3));
    let bottom: Vec<_> = f.iter().filter(|r| r.4 == 0x33).collect();
    assert_eq!(bottom, [&(0, 21, 298, 3, 0x33)], "solid bottom strip is the full border-box width");
    assert!(f.iter().any(|r| r.1 == 24 && r.3 == 10), "next block follows the 24px border box");
}
#[test]
fn left_right_pair_and_equal_styles() {
    let f = ok("<div class=c style='width:298px;height:48px;border-left:1px dashed #123;border-right:3px solid #345'></div><div class=d style='height:10px'></div>");
    let l: Vec<_> = f.iter().filter(|r| r.4 == 0x11).collect();
    assert_eq!(l.len(), 10); // 48 + gap 2 = 50 = 10 periods of 5
    assert!(l.iter().enumerate().all(|(i, r)| r.0 == 0 && r.1 == i32::try_from(i).unwrap() * 5 && r.2 == 1 && r.3 == 3));
    assert_eq!(f.iter().filter(|r| r.4 == 0x33).collect::<Vec<_>>(), [&(299, 0, 3, 48, 0x33)], "solid right strip at the border-box right edge");
    let m = ok("<div class=c style='width:298px;height:48px;border-left:3px solid #345;border-right:1px dashed #123'></div>");
    assert_eq!(m.iter().filter(|r| r.4 == 0x33).collect::<Vec<_>>(), [&(0, 0, 3, 48, 0x33)], "solid left strip spans the full border-box height");
    assert_eq!(m.iter().filter(|r| r.4 == 0x11).count(), 10);
    // same dashed style both sides, different colours
    let g = ok("<div class=c style='width:298px;height:20px;border-top:1px dashed #123;border-bottom:1px dashed #345'></div>");
    assert_eq!(g.iter().filter(|r| r.4 == 0x11).count(), 60);
    assert_eq!(g.iter().filter(|r| r.4 == 0x33).count(), 60);
}
#[test]
fn transparent_side_paints_nothing_and_misfits_refuse() {
    let f = ok("<div class=c style='width:298px;height:20px;border-top:1px dashed #123;border-bottom:3px solid transparent'></div><div class=d style='height:10px'></div>");
    assert_eq!(f.iter().filter(|r| r.4 == 0x11).count(), 60);
    assert!(!f.iter().any(|r| r.1 == 21 && r.3 == 3), "transparent bottom paints nothing");
    no("<div class=c style='width:297px;height:20px;border-top:1px dashed #123;border-bottom:3px solid #345'></div>"); // dashed does not fit
    no("<div class=c style='width:298px;height:20px;border-top:1px dashed #123;border-bottom:1px dotted #345'></div>"); // dotted 1px does not fit 298
    no("<div class=c style='width:298px;height:20px;border-top:1px dashed #123;border-bottom:3px solid #345;border-left:1px solid #000'></div>"); // adjacent side
    no("<div class=c style='width:298px;height:20px;border-top:1px dashed #123;border-left:1px solid #345'></div>"); // adjacent pair
    no("<div class=c style='width:298px;height:20px;border-top:1px dashed #123;border-left:1px solid #123'></div>"); // adjacent pair, same colour
    // adjacent SOLID same-colour pair keeps the original partitioned strips (top + middle-left = 2 rects), not the opposite-pair painter
    let a = ok("<div class=c style='width:100px;height:20px;border-top:2px solid #123;border-left:2px solid #123'></div>");
    assert_eq!(a.iter().filter(|r| r.4 == 0x11).count(), 2);
    no("<div class=c style='width:298px;height:20px;border-top:1px double #123;border-bottom:3px solid #345'></div>");
    no("<div class=c style='width:298px;height:20px;border-top:1px dashed #123;border-right:3px solid #345;border-bottom:1px dashed #123;border-left:1px solid #000'></div>"); // four sides mixed
    // display:none box paints nothing even with an unfitting pattern
    let h = ok("<div class=c style='width:297px;height:20px;border-top:1px dashed #123;border-bottom:3px solid #345;display:none'></div><div class=d style='height:10px'></div>");
    assert_eq!(h, ok("<div class=d style='height:10px'></div>"));
}
#[test]
fn other_entries_keep_refusing() {
    let h = page("<div class=c style='width:298px;height:20px;border-top:1px dashed #123;border-bottom:3px solid #345'></div>");
    for tree in [parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT), parse_style_tree_with_pinned_font_family(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL), parse_style_tree_with_media(h.as_bytes(), &[], &Limits::DEFAULT, MediaContext::new(785).unwrap())] {
        let admitted = !tree.unsupported && {
            let bt = BoxTree::from_style(&tree);
            !bt.unsupported && paint_backgrounds(&bt, 800, 600).is_ok()
        };
        assert!(!admitted);
    }
}

// ---- FL45b (tests only, additive): reviewer-proposed killers for V1/V2/V5 and an exact-geometry pin for the adjacent solid case.
struct Face;
impl kobo_web_document::css_text_page::FontProvider for Face {
    fn advance(&self, _: char, _: u32, _: bool) -> Option<u32> { Some(2) }
    fn line_height(&self, _: u32, _: bool) -> Option<u32> { Some(10) }
    fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> { Some(7) }
    fn raster(&self, ch: char, _: u32, _: bool) -> Option<kobo_web_document::inline_lines::GlyphBitmap> {
        Some(kobo_web_document::inline_lines::GlyphBitmap { left: 0, top: -3, width: u32::from(ch != ' '), height: if ch == ' ' { 0 } else { 2 }, coverage: if ch == ' ' { vec![] } else { vec![255; 2] } })
    }
}
fn ext_tree(html: &str) -> BoxTree {
    let t = parse_style_tree_with_inline_sheets_and_selectors(html.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    assert!(!t.unsupported);
    BoxTree::from_style(&t)
}
#[test]
fn v2_killer_adjacent_dashed_and_solid_same_colour_fits_yet_refuses() {
    // Reviewer fixture: border box 303, dashed span 305 fits the 1px pattern, so ONLY the opposite-pair guard in `supported` refuses it
    // (a mutant that drops the guard would paint the dashed top and omit the 5px left side).
    no("<div class=c style='width:298px;height:20px;border-top:1px dashed #123;border-left:5px solid #123'></div>");
    no("<div class=c style='width:298px;height:20px;border-top:1px dashed #123;border-left:5px solid #123;display:block'></div><div class=d style='height:10px'></div>");
}
#[test]
fn v5_exact_geometry_of_the_adjacent_solid_same_colour_pair() {
    let a = ok("<div class=c style='width:100px;height:20px;border-top:2px solid #123;border-left:2px solid #123'></div>");
    let b: Vec<_> = a.iter().filter(|r| r.4 == 0x11).collect();
    // partitioned strips: the full-width top strip and the left strip BELOW it (no overlap); the opposite-pair painter would draw the left
    // strip over the full height (0,0,2,22) and overlap the top strip's corner.
    assert_eq!(b, [&(0, 0, 102, 2, 0x11), &(0, 2, 2, 20, 0x11)]);
}
#[test]
fn v1_text_painters_use_the_non_ext_gate_and_refuse_a_dashed_opposite_pair() {
    use kobo_web_document::css_text_page::{paint_direct_text_blocks, paint_single_text_page};
    let one = |b: &str| ext_tree(&format!("<!doctype html><style>body{{margin:0}}</style><div style='width:298px;{b}'>ab</div>"));
    let anchor = one("border-top:1px solid #123;border-bottom:1px solid #123");
    assert!(paint_single_text_page(&anchor, 800, 600, &Face).is_ok(), "anchor: same-colour solid opposite pair paints on the text painter");
    let dashed = one("border-top:1px dashed #123;border-bottom:3px solid #123"); // same colour: only the ext gate separates it from the proven path
    assert!(paint_single_text_page(&dashed, 800, 600, &Face).is_err());
    assert!(paint_direct_text_blocks(&dashed, 800, 600, &Face).is_err());
}
