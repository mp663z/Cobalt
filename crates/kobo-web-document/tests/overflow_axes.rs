//! PRIVATE candidate FL33 (ext entry only): `overflow-x` / `overflow-y` longhands `hidden` or `auto`, and the `overflow:auto`
//! shorthand, typed onto the existing `overflow_hidden` state (either axis non-visible makes both axes non-visible; the box tree
//! still admits it only when nothing can overflow). Provenance: for each fixture below the engine's Command::Fill list was
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
    ("q1", r####"<!doctype html><style>.c{height:30px;background:#cde}body{margin:0}</style><div style="overflow-x:hidden"><div class=c></div><div class=c style="background:#9c9"></div></div>"####, &[(0, 0, 800, 30, 0xccddee), (0, 30, 800, 30, 0x99cc99)]),
    ("q2", r####"<!doctype html><style>.c{height:30px;background:#cde}body{margin:0}</style><div style="overflow:auto;background:#eee;padding:10px"><div class=c></div></div>"####, &[(0, 0, 800, 50, 0xeeeeee), (10, 10, 780, 30, 0xccddee)]),
    ("q3", r####"<!doctype html><style>.c{height:30px;background:#cde}body{margin:0}</style><div style="overflow-y:hidden;background:#eee"><div class=c style="margin-top:20px"></div></div><div class=c style="background:#9c9"></div>"####, &[(0, 0, 800, 50, 0xeeeeee), (0, 20, 800, 30, 0xccddee), (0, 50, 800, 30, 0x99cc99)]),
    ("q5", r####"<!doctype html><style>.c{height:30px;background:#cde}body{margin:0;overflow:hidden}</style><div class=c style="margin-top:20px"></div>"####, &[(0, 20, 800, 30, 0xccddee)]),
    ("q6", r####"<!doctype html><style>.c{height:30px;background:#cde}body{margin:0;overflow-x:hidden}</style><div class=c style="margin-top:20px"></div><div class=c style="background:#9c9"></div>"####, &[(0, 20, 800, 30, 0xccddee), (0, 50, 800, 30, 0x99cc99)]),
    ("q7", r####"<!doctype html><style>.c{height:30px;background:#cde}body{margin:0}</style><div style="overflow-x:hidden;margin-bottom:10px"><div class=c style="margin-bottom:30px"></div></div><div class=c style="margin-top:20px;background:#9c9"></div>"####, &[(0, 0, 800, 30, 0xccddee), (0, 80, 800, 30, 0x99cc99)]),
    ("q10", r####"<!doctype html><style>.c{height:30px;background:#cde}body{margin:0}</style><div style="overflow:auto;overflow:visible;background:#eee"><div class=c style="margin-top:20px"></div></div>"####, &[(0, 20, 800, 30, 0xeeeeee), (0, 20, 800, 30, 0xccddee)]),
];
const REFUSED: &[(&str, &str)] = &[
    ("q8", r####"<!doctype html><style>.c{height:30px;background:#cde}body{margin:0}</style><div style="overflow-x:hidden;overflow-y:visible;background:#eee"><div class=c></div></div>"####),
    ("q9", r####"<!doctype html><style>.c{height:30px;background:#cde}body{margin:0}</style><div style="overflow:hidden;overflow-x:visible;background:#eee"><div class=c></div></div>"####),
    ("q11", r####"<!doctype html><style>.c{height:30px;background:#cde}body{margin:0}</style><div style="overflow-x:scroll"><div class=c></div></div>"####),
    ("q12", r####"<!doctype html><style>.c{height:30px;background:#cde}body{margin:0}</style><div style="overflow-x:auto;width:100px"><div class=c style="width:300px"></div></div>"####),
    ("q13", r####"<!doctype html><style>.c{height:30px;background:#cde}body{margin:0}</style><div style="overflow-y:auto;height:20px"><div class=c></div></div>"####),
    ("q14", r####"<!doctype html><style>.c{height:30px;background:#cde}body{margin:0}</style><div style="overflow-x:hidden;width:100px;background:#eee"><div class=c style="width:300px"></div></div>"####),
];
#[test]
fn chrome_pixel_verified_fixtures_match_recorded_rects() {
    for (name, html, expected) in CHROME_VERIFIED {
        assert_eq!(fills(html).as_deref(), Some(*expected), "{name}");
    }
    assert!(CHROME_VERIFIED.iter().all(|(_, _, e)| !e.is_empty()));
}
#[test]
fn unmodelled_overflow_shapes_stay_refused() {
    for (name, html) in REFUSED {
        assert!(fills(html).is_none(), "{name}");
    }
    for v in ["overflow-x:visible", "overflow-y:visible", "overflow-x:scroll", "overflow-y:clip", "overflow-x:hidden hidden", "overflow:auto hidden"] {
        let html = format!("<!doctype html><style>body{{margin:0}}</style><div style='{v}'><div style='height:30px;background:#cde'></div></div>");
        assert!(fills(&html).is_none(), "{v}");
    }
}
#[test]
fn default_entry_still_refuses_overflow_longhands_and_auto() {
    for v in ["overflow-x:hidden", "overflow-y:auto", "overflow:auto"] {
        let html = format!("<!doctype html><style>body{{margin:0}}</style><div style='{v}'><div style='height:30px;background:#cde'></div></div>");
        assert!(parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT).unsupported, "{v}");
    }
}
