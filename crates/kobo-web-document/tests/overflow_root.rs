//! PRIVATE candidate FL34 (ext entry only): a non-visible `overflow` on the ROOT element alone (it propagates to the viewport, the
//! root box stays visible) is admitted by BoxTree::stray_overflow_hidden; any second box with a non-visible overflow still takes
//! the existing proof (so `body` + root, nested, and abs shapes keep refusing). Provenance: each fixture's engine Fill list was
//! rasterised in painter order over white (private-evidence ul1/ov37/cmp.py) and compared with a Chrome 156 headless screenshot at
//! 800x600, device scale 1: 0 differing pixels (constructed diagnostics, ext entry, MediaContext default, synthetic, text-free).
//! This test compares engine Fill rects with recorded rects; no Chrome/stored pixels in the loop.
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
    ("o3", r####"<!doctype html><style>.c{height:30px;background:#cde}html{overflow-x:hidden}body{margin:0}</style><div class=c></div>"####, &[(0, 0, 800, 30, 0xccddee)]),
    ("o8", r####"<!doctype html><style>.c{height:30px;background:#cde}html{overflow:hidden}body{margin:0}</style><div class=c></div>"####, &[(0, 0, 800, 30, 0xccddee)]),
    ("r2", r####"<!doctype html><style>.c{height:30px;background:#cde}html{overflow:hidden}body{margin:0}</style><div class=c style="margin-top:20px"></div>"####, &[(0, 20, 800, 30, 0xccddee)]),
    ("r3", r####"<!doctype html><style>.c{height:30px;background:#cde}html{overflow-y:auto}body{margin:0}</style><div class=c></div><div style="height:900px;background:#eee"></div>"####, &[(0, 0, 800, 30, 0xccddee), (0, 30, 800, 900, 0xeeeeee)]),
    ("r7", r####"<!doctype html><style>.c{height:30px;background:#cde}html{overflow:hidden}body{margin:0;background:#eee}</style><div class=c style="margin-top:20px"></div>"####, &[(0, 0, 800, 600, 0xeeeeee), (0, 20, 800, 30, 0xccddee)]),
    ("r8", r####"<!doctype html><style>.c{height:30px;background:#cde}html{overflow:hidden;background:#fc0}body{margin:10px;background:#eee}</style><div class=c></div>"####, &[(0, 0, 800, 600, 0xffcc00), (10, 10, 780, 30, 0xeeeeee), (10, 10, 780, 30, 0xccddee)]),
];
const REFUSED: &[(&str, &str)] = &[
    ("o9", r####"<!doctype html><style>.c{height:30px;background:#cde}html{overflow:hidden}body{margin:0;overflow:hidden}</style><div class=c style="width:900px"></div>"####),
    ("r4", r####"<!doctype html><style>.c{height:30px;background:#cde}html{overflow:hidden}body{margin:0}</style><div class=c></div><div style="position:absolute;top:10px;left:700px;width:200px;height:50px;background:#f90"></div>"####),
    ("r5", r####"<!doctype html><style>.c{height:30px;background:#cde}html{overflow:hidden}body{margin:0;overflow:hidden}</style><div class=c style="margin-top:20px"></div>"####),
    ("r6", r####"<!doctype html><style>.c{height:30px;background:#cde}html{overflow:hidden}body{margin:0}</style><div style="overflow:hidden"><div class=c style="margin-top:20px"></div></div>"####),
];
#[test]
fn chrome_pixel_verified_root_overflow_fixtures_match_recorded_rects() {
    for (name, html, expected) in CHROME_VERIFIED {
        assert_eq!(fills(html).as_deref(), Some(*expected), "{name}");
    }
    assert!(CHROME_VERIFIED.iter().all(|(_, _, e)| !e.is_empty()));
}
#[test]
fn root_overflow_with_a_second_overflow_box_or_unmodelled_shape_stays_refused() {
    for (name, html) in REFUSED {
        assert!(fills(html).is_none(), "{name}");
    }
}
#[test]
fn default_entry_still_refuses_root_overflow() {
    let html = "<!doctype html><style>html{overflow:hidden}body{margin:0}</style><div style='height:30px;background:#cde'></div>";
    assert!(parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT).unsupported);
}
