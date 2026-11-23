//! PRIVATE candidate FL28 (ext entry only): solid `outline` (whole px width, explicit opaque colour, offset >= 0), painted after all in-flow fills.
//! Provenance of the expected rects: for each fixture below the engine's Command::Fill list was rasterised in painter order over
//! white (private-evidence ul1/sh28/cmp.py) and compared with a Chrome 156 headless screenshot at 800x600, device scale 1:
//! 0 differing pixels in all 7 fixtures (constructed diagnostics, ext entry, MediaContext default, synthetic, text-free). This
//! test compares engine Fill rects with the recorded rects; it holds no stored pixels and no Chrome in the loop.
use kobo_web_document::{
    box_tree::BoxTree,
    css_background::{paint_backgrounds, paint_backgrounds_float_kernel},
    css_paint_bridge::{paint_content_rectangles, paint_padding_rectangles, FillSpec},
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
const PRE: &str = "<!doctype html><style>body{margin:0}</style><body>";
fn one(style: &str) -> String {
    format!("{PRE}<div style='height:20px;width:80px;margin:20px 0 0 20px;background:#fff;{style}'></div>")
}
const CHROME_VERIFIED: &[(&str, &str, &[F])] = &[
    ("o1", r####"<!doctype html><style>body{margin:0}</style><body><div style="width:100px;height:40px;margin:20px 0 0 30px;background:#fff;outline:3px solid #f00"></div><div style="width:100px;height:40px;margin:2px 0 0 30px;background:#cde"></div>"####, &[(30, 20, 100, 40, 0xffffff), (30, 62, 100, 40, 0xccddee), (27, 17, 106, 3, 0xff0000), (27, 60, 106, 3, 0xff0000), (27, 20, 3, 40, 0xff0000), (130, 20, 3, 40, 0xff0000)]),
    ("o2", r####"<!doctype html><style>body{margin:0}</style><body><div style="width:100px;height:40px;margin:20px 0 0 30px;outline:4px solid #00f;outline-offset:5px"></div><div style="width:60px;height:20px;margin:20px 0 0 30px;padding:5px;border:3px solid #333;outline:2px solid #0a0"></div>"####, &[(30, 80, 76, 3, 0x333333), (30, 113, 76, 3, 0x333333), (30, 83, 3, 30, 0x333333), (103, 83, 3, 30, 0x333333), (21, 11, 118, 4, 0x0000ff), (21, 65, 118, 4, 0x0000ff), (21, 15, 4, 50, 0x0000ff), (135, 15, 4, 50, 0x0000ff), (28, 78, 80, 2, 0x00aa00), (28, 116, 80, 2, 0x00aa00), (28, 80, 2, 36, 0x00aa00), (106, 80, 2, 36, 0x00aa00)]),
    ("p1", r####"<!doctype html><style>body{margin:0}.a{width:100px;height:30px;margin:20px 0 0 30px}</style><div class=a style="outline:solid 2px #123456"></div><div class=a style="outline:#a00 solid 4px"></div><div class=a style="outline:3px #0a0 solid;outline-offset:2px"></div><div class=a style="outline:5px solid #00f;outline:none"></div><div class=a style="outline:0;outline-style:solid;outline-color:#f0f"></div>"####, &[(28, 18, 104, 2, 0x123456), (28, 50, 104, 2, 0x123456), (28, 20, 2, 30, 0x123456), (130, 20, 2, 30, 0x123456), (26, 66, 108, 4, 0xaa0000), (26, 100, 108, 4, 0xaa0000), (26, 70, 4, 30, 0xaa0000), (130, 70, 4, 30, 0xaa0000), (25, 115, 110, 3, 0x00aa00), (25, 152, 110, 3, 0x00aa00), (25, 118, 3, 34, 0x00aa00), (132, 118, 3, 34, 0x00aa00)]),
    ("p2", r####"<!doctype html><style>body{margin:0}.a{width:100px;height:30px;margin:20px 0 0 30px}.x{outline-width:4px}.y{outline-style:solid}.z{outline-color:#c60}#i{outline-style:none}</style><div class="a x y z"></div><div class="a x y z" id=i></div><div class=a style="outline-color:#00f" ><div style="height:10px;outline:2px solid #0a0;outline-color:#f00"></div></div><div class="a y" style="outline-width:1px;outline-color:#333;outline-offset:3px"></div>"####, &[(26, 16, 108, 4, 0xcc6600), (26, 50, 108, 4, 0xcc6600), (26, 20, 4, 30, 0xcc6600), (130, 20, 4, 30, 0xcc6600), (28, 118, 104, 2, 0xff0000), (28, 130, 104, 2, 0xff0000), (28, 120, 2, 10, 0xff0000), (130, 120, 2, 10, 0xff0000), (26, 166, 108, 1, 0x333333), (26, 203, 108, 1, 0x333333), (26, 167, 1, 36, 0x333333), (133, 167, 1, 36, 0x333333)]),
    ("p3", r####"<!doctype html><style>body{margin:0}.a{height:20px;width:80px;margin:0 0 0 30px;background:#fff;outline:6px solid #f00}.b{height:20px;width:80px;margin:0 0 0 30px;background:#cde}</style><div style="height:10px"></div><div class=a></div><div class=b></div><div class=a style="outline-color:#00f"></div><div style="width:60px;height:20px;margin:30px 0 0 40px;outline:3px solid #0a0;background:#ccc"></div><div style="width:50px;height:20px;margin:-8px 0 0 70px;background:#fa0"></div>"####, &[(30, 10, 80, 20, 0xffffff), (30, 30, 80, 20, 0xccddee), (30, 50, 80, 20, 0xffffff), (40, 100, 60, 20, 0xcccccc), (70, 112, 50, 20, 0xffaa00), (24, 4, 92, 6, 0xff0000), (24, 30, 92, 6, 0xff0000), (24, 10, 6, 20, 0xff0000), (110, 10, 6, 20, 0xff0000), (24, 44, 92, 6, 0x0000ff), (24, 70, 92, 6, 0x0000ff), (24, 50, 6, 20, 0x0000ff), (110, 50, 6, 20, 0x0000ff), (37, 97, 66, 3, 0x00aa00), (37, 120, 66, 3, 0x00aa00), (37, 100, 3, 20, 0x00aa00), (100, 100, 3, 20, 0x00aa00)]),
    ("p4", r####"<!doctype html><style>body{margin:0}.a{--w:4px;--c:#c0c;width:100px;height:30px;margin:20px 0 0 30px}</style><div class=a style="outline:var(--w) solid var(--c)"></div><div class=a style="outline:var(--none,2px solid #123)"></div><div class=a style="outline-width:var(--w);outline-style:solid;outline-color:var(--c)"></div>"####, &[(26, 16, 108, 4, 0xcc00cc), (26, 50, 108, 4, 0xcc00cc), (26, 20, 4, 30, 0xcc00cc), (130, 20, 4, 30, 0xcc00cc), (28, 68, 104, 2, 0x112233), (28, 100, 104, 2, 0x112233), (28, 70, 2, 30, 0x112233), (130, 70, 2, 30, 0x112233), (26, 116, 108, 4, 0xcc00cc), (26, 150, 108, 4, 0xcc00cc), (26, 120, 4, 30, 0xcc00cc), (130, 120, 4, 30, 0xcc00cc)]),
    ("p6", r####"<!doctype html><style>body{margin:0}.pa{width:200px;height:60px;margin:30px 0 0 40px;outline:3px solid #f00;outline-offset:4px}.ch{height:20px;margin:10px 20px;outline:inherit}.c2{height:10px;margin:10px 20px;outline-style:inherit;outline-width:2px;outline-color:#00f;outline-offset:inherit}.c3{height:10px;margin:6px 20px;outline:2px solid #0a0;outline-style:unset;outline-color:unset}</style><div class=pa><div class=ch></div><div class=c2></div><div class=c3></div></div>"####, &[(33, 23, 214, 3, 0xff0000), (33, 94, 214, 3, 0xff0000), (33, 26, 3, 68, 0xff0000), (244, 26, 3, 68, 0xff0000), (57, 27, 166, 3, 0xff0000), (57, 50, 166, 3, 0xff0000), (57, 30, 3, 20, 0xff0000), (220, 30, 3, 20, 0xff0000), (54, 54, 172, 2, 0x0000ff), (54, 74, 172, 2, 0x0000ff), (54, 56, 2, 18, 0x0000ff), (224, 56, 2, 18, 0x0000ff)]),
];
/// p5 (flex items) is recorded as a Chrome MISMATCH of the all-after model (60 px: a later flex item covers an earlier item's
/// outline), so the engine refuses it; the fixture is kept only as the refusal anchor.
const FLEX_REFUSED: &str = r####"<!doctype html><style>body{margin:0}.a{height:20px;width:80px;margin:0 0 0 30px;background:#fff;outline:6px solid #f00}.b{height:20px;width:80px;margin:0 0 0 30px;background:#cde}</style><div style="height:10px"></div><div class=a></div><div class=b></div><div class=a style="outline-color:#00f"></div><div style="display:flex;margin:30px 0 0 40px"><div style="width:50px;height:20px;outline:3px solid #0a0"></div><div style="width:50px;height:20px;background:#ccc"></div></div>"####;
#[test]
fn chrome_pixel_verified_fixtures_match_recorded_rects() {
    for (name, html, expected) in CHROME_VERIFIED {
        assert_eq!(fills(html).as_deref(), Some(*expected), "{name}");
    }
    assert!(CHROME_VERIFIED.iter().all(|(_, _, e)| e.len() >= 6));
    assert_eq!(fills(FLEX_REFUSED), None);
}
#[test]
fn spellings_reset_and_unmodelled_shapes() {
    let base = fills(&one("outline:2px solid #123456")).unwrap();
    assert_eq!(base.len(), 5, "anchor: box fill plus four ring bands");
    for v in ["outline:solid 2px #123456", "outline:#123456 2px solid", "outline:2px solid #123456 !important", "outline-width:2px;outline-style:solid;outline-color:#123456", "outline:2px solid #123456;outline-offset:0", "outline:2px solid currentcolor;outline-color:#123456"] {
        assert_eq!(fills(&one(v)).as_ref(), Some(&base), "{v}");
    }
    // the shorthand resets what it does not name; none / 0 / style none paint nothing
    for v in ["outline:2px solid #123456;outline:none", "outline:2px solid #123456;outline:0", "outline:2px solid #123456;outline-style:none", "outline:2px solid #123456;outline-width:0", "outline:2px solid #123456;outline:initial", "outline-width:2px;outline-color:#123456"] {
        assert_eq!(fills(&one(v)).map(|f| f.len()), Some(1), "{v}");
    }
    assert_eq!(fills(&one("outline-style:solid;outline-color:#123456;outline:2px solid #abc;outline-style:none")).map(|f| f.len()), Some(1));
    // no style in the shorthand means style none: width and colour alone paint nothing (Chrome: outline-style initial none)
    assert_eq!(fills(&one("outline:2px #123456")).map(|f| f.len()), Some(1));
    assert_eq!(fills(&one("outline:#123456")).map(|f| f.len()), Some(1));
    // a zero-width solid outline paints nothing and is NOT subject to the outline refusals (flex item, hidden ancestor)
    assert!(fills(&format!("{PRE}<div style='display:flex'><div style='width:20px;height:10px;outline:0 solid #000'></div></div>")).is_some());
    assert!(fills(&format!("{PRE}<section style='overflow:hidden'><article style='height:20px;outline:0 solid #000'></article></section>")).is_some());
    // a shorthand after a longhand resets the longhands it omits (width back to 3px = medium)
    assert_eq!(fills(&one("outline-width:9px;outline:solid #123456")).unwrap()[1], (20 - 3, 20 - 3, 80 + 6, 3, 0x123456));
    for v in ["outline:2px dashed #000", "outline:2px dotted #000", "outline:2px double #000", "outline:2px groove #000", "outline:2px auto #000", "outline:thin solid #000", "outline:medium solid #000", "outline:2px solid", "outline:2px solid invert",
        "outline:2px solid #000 3px", "outline:solid solid #000", "outline:2px solid rgba(0,0,0,.5)", "outline:2px solid #0008", "outline:1.5px solid #000", "outline:2em solid #000", "outline:2px solid #000, 1px solid #fff", "outline:",
        "outline-offset:-2px", "outline-offset:2em", "outline-offset:2%", "outline-offset:20000px", "outline-width:thick", "outline-style:hidden", "outline-color:invert", "outline:20000px solid #000"] {
        // `outline:2px solid` has no explicit colour: parses (colour = currentcolor) but the painter refuses
        assert_eq!(fills(&one(v)), None, "{v}");
    }
}
#[test]
fn paint_refusals() {
    let ol = "outline:2px solid #000";
    assert_eq!(fills(&format!("<!doctype html><style>html{{{ol}}}body{{margin:0}}</style><div style='height:10px'></div>")), None);
    assert_eq!(fills(&format!("<!doctype html><style>body{{margin:0;{ol}}}</style><div style='height:10px'></div>")), None);
    assert_eq!(fills(&format!("{PRE}<section style='overflow:hidden'><article style='height:20px;{ol}'></article></section>")), None);
    assert!(fills(&format!("{PRE}<section style='overflow:hidden;{ol}'><article style='height:20px'></article></section>")).is_some());
    assert_eq!(fills(&format!("{PRE}<div style='height:0;width:50px;{ol}'></div>")), None);
    assert_eq!(fills(&format!("{PRE}<div style='height:10px;{ol}'>text</div>")), None);
    assert_eq!(fills(&format!("{PRE}<div style='display:flex'><div style='width:20px;height:10px;{ol}'></div></div>")), None);
    assert!(fills(&format!("{PRE}<div style='display:flex;{ol}'><div style='width:20px;height:10px'></div></div>")).is_some(), "a flex container's own outline is in-flow");
    let pos = "main{width:60px;height:40px;margin:20px}.p{position:absolute;top:30px;left:40px;width:30px;height:20px;background:#fff}";
    assert!(fills(&format!("<!doctype html><style>body{{margin:0}}{pos}</style><main></main><div class=p></div>")).is_some(), "anchor: positioned page without outline paints");
    assert_eq!(fills(&format!("<!doctype html><style>body{{margin:0}}{pos}</style><main style='{ol}'></main><div class=p></div>")), None);
    let t = tree(&one(ol)).unwrap();
    assert!(paint_backgrounds_float_kernel(&t, 800, 600).is_err());
    let spec = [FillSpec { box_index: 2, color: Rgb(1, 2, 3) }];
    let rect_painters = |t: &BoxTree| {
        let w = kobo_web_document::box_tree::WidthPass::from_boxes(t, 800);
        let v = kobo_web_document::box_tree::VerticalPass::from_boxes(t, 800, 600);
        (paint_padding_rectangles(t, &w, &v, 800, 600, &spec).is_ok(), paint_content_rectangles(t, &w, &v, 800, 600, &spec).is_ok())
    };
    assert_eq!(rect_painters(&tree(&one("")).unwrap()), (true, true));
    assert_eq!(rect_painters(&t), (false, false));
}
#[test]
fn default_entry_unchanged() {
    for v in ["2px solid #000", "none", "0"] {
        let h = format!("<!doctype html><div style='height:10px;outline:{v}'></div>");
        assert!(parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported, "{v}");
    }
}
