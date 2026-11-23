//! PRIVATE candidate FL35 (ext entry only). (1) SOUNDNESS FIX in BoxTree::collapsed_margin: a `position:relative` block is in flow
//! and its vertical margins collapse like a static block's; the search for the first/last in-flow child only looked at
//! `position == Static`, so a relative first child's margin-top did not collapse through its parent (t1 and t3: the engine painted
//! at y=0, Chrome 156 at y=20; 8000 and 32000 differing pixels BEFORE the fix, 0 after; a pre-existing defect in the FL15-FL34 relative
//! lane). (2) `position:sticky` at scroll offset 0 with all-auto offsets or exactly `top:0` is laid out as `relative`; other
//! offsets refuse (sticky left:5px and top:10px differ from relative in Chrome). Provenance: each fixture's engine Fill list was
//! rasterised in painter order over white (private-evidence ul1/sk38 and ul1/rl39 cmp.py) and compared with a Chrome 156 headless
//! screenshot at 800x600, device scale 1: 0 differing pixels (constructed diagnostics, ext entry, MediaContext default, synthetic,
//! text-free). Tests compare engine Fill rects with recorded rects; no Chrome/stored pixels in the loop.
use kobo_web_document::{
    box_tree::BoxTree,
    css_background::paint_backgrounds,
    display_list::{Command, Rgb},
    parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget,
};
type F = (i32, i32, u32, u32, u32);
fn tree(html: &str) -> Option<BoxTree> {
    let t = parse_style_tree_with_inline_sheets_and_selectors(html.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    if t.unsupported {
        return None;
    }
    let bt = BoxTree::from_style(&t);
    (!bt.unsupported).then_some(bt)
}
fn fills(html: &str) -> Option<Vec<F>> {
    let l = paint_backgrounds(&tree(html)?, 800, 600).ok()?;
    Some(
        l.commands()
            .iter()
            .filter_map(|c| match c {
                Command::Fill { rect, color: Rgb(r, g, b), .. } => Some((rect.x, rect.y, rect.width, rect.height, (u32::from(*r) << 16) | (u32::from(*g) << 8) | u32::from(*b))),
                _ => None,
            })
            .collect(),
    )
}
const CHROME_VERIFIED: &[(&str, &str, &[F])] = &[
    ("s1", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c></div><div class=c style="position:sticky;top:0;background:#9c9"></div><div class=c></div>"####, &[(0, 0, 800, 30, 0xccddee), (0, 60, 800, 30, 0xccddee), (0, 30, 800, 30, 0x99cc99)]),
    ("s2", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div style="position:sticky;top:0;height:50px;background:#333"></div><div class=c></div>"####, &[(0, 50, 800, 30, 0xccddee), (0, 0, 800, 50, 0x333333)]),
    ("s3", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="position:sticky"></div><div class=c style="background:#9c9"></div>"####, &[(0, 30, 800, 30, 0x99cc99), (0, 0, 800, 30, 0xccddee)]),
    ("s4", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div style="margin:20px;padding:10px;background:#eee"><div class=c style="position:sticky;top:0"></div><div class=c style="background:#9c9;margin-top:10px"></div></div>"####, &[(20, 20, 760, 90, 0xeeeeee), (30, 70, 740, 30, 0x99cc99), (30, 30, 740, 30, 0xccddee)]),
    ("s5", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="position:sticky;top:0;margin:20px 30px;width:200px"></div>"####, &[(30, 20, 200, 30, 0xccddee)]),
    ("t1", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="position:relative;margin:20px 30px;width:200px"></div>"####, &[(30, 20, 200, 30, 0xccddee)]),
    ("t3", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="position:relative;margin-top:20px"></div>"####, &[(0, 20, 800, 30, 0xccddee)]),
    ("t4", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div style="height:10px"></div><div class=c style="position:relative;margin-top:20px"></div>"####, &[(0, 30, 800, 30, 0xccddee)]),
    ("t5", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div style="padding:1px"><div class=c style="position:relative;margin-top:20px"></div></div>"####, &[(1, 21, 798, 30, 0xccddee)]),
    ("t6", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="position:relative;margin-bottom:20px"></div><div class=c style="background:#9c9"></div>"####, &[(0, 50, 800, 30, 0x99cc99), (0, 0, 800, 30, 0xccddee)]),
    ("t7", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="background:#9c9"></div><div style="background:#eee"><div class=c style="position:relative;margin-bottom:20px"></div></div><div class=c></div>"####, &[(0, 0, 800, 30, 0x99cc99), (0, 30, 800, 30, 0xeeeeee), (0, 80, 800, 30, 0xccddee), (0, 30, 800, 30, 0xccddee)]),
    ("t8", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div style="margin-top:10px"><div class=c style="position:relative;margin-top:30px"></div></div>"####, &[(0, 30, 800, 30, 0xccddee)]),
    ("t9", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div style="margin-top:30px"><div class=c style="position:relative;margin-top:10px"></div></div>"####, &[(0, 30, 800, 30, 0xccddee)]),
    ("t10", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div style="margin:5px 0"><div class=c style="position:relative;margin:25px 0 15px"></div></div><div class=c style="background:#9c9;margin-top:5px"></div>"####, &[(0, 70, 800, 30, 0x99cc99), (0, 25, 800, 30, 0xccddee)]),
    ("t11", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div style="background:#eee;padding-top:1px"><div class=c style="position:relative;margin-top:30px"></div></div>"####, &[(0, 0, 800, 61, 0xeeeeee), (0, 31, 800, 30, 0xccddee)]),
];
const REFUSED: &[(&str, &str)] = &[
    ("s6", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="position:sticky;top:10px"></div>"####),
    ("s7", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="position:sticky;left:5px"></div>"####),
    ("s8", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div style="height:20px"></div><div class=c style="position:sticky;top:0;margin-top:-10px;background:#9c9"></div><div class=c></div>"####),
];
#[test]
fn chrome_pixel_verified_fixtures_match_recorded_rects() {
    for (name, html, expected) in CHROME_VERIFIED {
        assert_eq!(fills(html).as_deref(), Some(*expected), "{name}");
    }
    assert!(CHROME_VERIFIED.iter().all(|(_, _, e)| !e.is_empty()));
}
#[test]
fn relative_margin_top_collapses_through_parent_like_static() {
    // anchors: the relative box lands exactly where the static twin does (t1/t3 were wrong before the fix)
    let rel = fills("<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style='position:relative;margin-top:20px'></div>").unwrap();
    let sta = fills("<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style='margin-top:20px'></div>").unwrap();
    assert_eq!(rel, sta);
    assert_eq!(rel, vec![(0, 20, 800, 30, 0xccddee)]);
}
#[test]
fn unmodelled_sticky_offsets_stay_refused() {
    for (name, html) in REFUSED {
        assert!(fills(html).is_none(), "{name}");
    }
    for v in ["position:sticky;bottom:0", "position:sticky;top:1px", "position:sticky;right:0", "position:sticky;top:0;left:0", "position:sticky;top:1em"] {
        let html = format!("<!doctype html><style>body{{margin:0}}</style><div style='height:30px;background:#cde;{v}'></div>");
        assert!(fills(&html).is_none(), "{v}");
    }
}
#[test]
fn default_entry_does_not_recognise_sticky() {
    let html = "<!doctype html><style>body{margin:0}</style><div style='position:sticky;top:0;height:30px;background:#cde'></div>";
    assert!(parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT).unsupported);
}
