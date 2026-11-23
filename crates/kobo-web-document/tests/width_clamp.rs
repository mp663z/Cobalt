//! PRIVATE candidate FL29/FL30/FL31 (ext entry only; FL31 adds border-box via b1-b10, private-evidence ul1/bb33; FL30 adds percent `width`, min/max stay px-only): CSS 2.1 10.4 `min-width`/`max-width` (px) on a plain in-flow static content-box LTR
//! block whose parent is a block. Provenance of the expected rects: each fixture's engine Command::Fill list was rasterised in
//! painter order over white (private-evidence ul1/mw31/cmp.py) and compared with a Chrome 156 headless screenshot at 800x600,
//! device scale 1: 0 differing pixels in all 34 fixtures (m*,n* FL29; p1-p7 FL30, private-evidence ul1/pw32) (constructed diagnostics, ext entry, MediaContext default, synthetic,
//! text-free). This test compares engine Fill rects with the recorded rects; it holds no stored pixels and no Chrome in the loop.
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
    ("m1", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:300px"></div>"####, &[(0, 0, 300, 30, 0xccddee)]),
    ("m2", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:300px;margin:0 auto"></div>"####, &[(250, 0, 300, 30, 0xccddee)]),
    ("m3", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="width:500px;max-width:300px"></div>"####, &[(0, 0, 300, 30, 0xccddee)]),
    ("m4", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="width:100px;min-width:200px"></div>"####, &[(0, 0, 200, 30, 0xccddee)]),
    ("m5", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div style="max-width:400px;margin:10px auto;background:#eee;padding:10px"><div class=c style="background:#fc0"></div></div>"####, &[(190, 10, 420, 50, 0xeeeeee), (200, 20, 400, 30, 0xffcc00)]),
    ("m6", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:1000px"></div>"####, &[(0, 0, 800, 30, 0xccddee)]),
    ("m7", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:300px;padding:10px 20px;border:5px solid #333;margin:0 auto"></div>"####, &[(225, 0, 350, 60, 0xccddee), (225, 0, 350, 5, 0x333333), (225, 55, 350, 5, 0x333333), (225, 5, 5, 50, 0x333333), (570, 5, 5, 50, 0x333333)]),
    ("m8", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:300px;margin-left:50px;margin-right:auto"></div>"####, &[(50, 0, 300, 30, 0xccddee)]),
    ("m10", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:300px;margin:0 auto 10px"></div><div class=c style="max-width:200px;margin:0 auto;background:#9c9"></div>"####, &[(250, 0, 300, 30, 0xccddee), (300, 40, 200, 30, 0x99cc99)]),
    ("n1", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div style="max-width:400px;margin:0 auto;background:#eee"><div class=c style="max-width:300px;margin:0 auto;background:#fc0"></div></div>"####, &[(200, 0, 400, 30, 0xeeeeee), (250, 0, 300, 30, 0xffcc00)]),
    ("n2", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:200px;min-width:300px"></div>"####, &[(0, 0, 300, 30, 0xccddee)]),
    ("n3", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:0"></div><div class=c style="min-width:0;margin-top:5px;background:#9c9"></div>"####, &[(0, 35, 800, 30, 0x99cc99)]),
    ("n4", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="min-width:900px"></div>"####, &[(0, 0, 900, 30, 0xccddee)]),
    ("n5", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:300px;margin:0 20px 0 auto;padding:0 10px"></div>"####, &[(460, 0, 320, 30, 0xccddee)]),
    ("n6", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:300px;max-width:none"></div>"####, &[(0, 0, 800, 30, 0xccddee)]),
    ("n7", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="min-width:400px;width:100px;margin:0 auto"></div>"####, &[(200, 0, 400, 30, 0xccddee)]),
    ("n8", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div style="width:500px;margin:10px"><div class=c style="max-width:300px;margin:0 auto"></div><div class=c style="min-width:600px;margin-top:4px;background:#9c9"></div></div>"####, &[(110, 10, 300, 30, 0xccddee), (10, 44, 600, 30, 0x99cc99)]),
    ("p1", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="width:50%;max-width:300px;margin:0 auto"></div>"####, &[(250, 0, 300, 30, 0xccddee)]),
    ("p2", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="width:25%;max-width:300px"></div>"####, &[(0, 0, 200, 30, 0xccddee)]),
    ("p3", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="width:25%;min-width:300px"></div>"####, &[(0, 0, 300, 30, 0xccddee)]),
    ("p4", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div style="width:400px;margin:10px"><div class=c style="width:50%;max-width:150px;margin:0 auto"></div><div class=c style="width:10%;min-width:100px;margin-top:3px;background:#9c9"></div></div>"####, &[(135, 10, 150, 30, 0xccddee), (10, 43, 100, 30, 0x99cc99)]),
    ("p5", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="width:100%;max-width:500px;padding:0 10px;margin:0 auto"></div>"####, &[(140, 0, 520, 30, 0xccddee)]),
    ("p6", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="width:33%;max-width:300px"></div>"####, &[(0, 0, 264, 30, 0xccddee)]),
    ("p7", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="width:50%;min-width:500px;max-width:450px;margin-left:auto"></div>"####, &[(300, 0, 500, 30, 0xccddee)]),
    ("b1", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde;box-sizing:border-box}</style><div class=c style="max-width:300px;padding:0 20px;border:5px solid #333;margin:0 auto"></div>"####, &[(250, 0, 300, 30, 0xccddee), (250, 0, 300, 5, 0x333333), (250, 25, 300, 5, 0x333333), (250, 5, 5, 20, 0x333333), (545, 5, 5, 20, 0x333333)]),
    ("b2", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde;box-sizing:border-box}</style><div class=c style="width:500px;max-width:300px;padding:0 20px"></div>"####, &[(0, 0, 300, 30, 0xccddee)]),
    ("b3", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde;box-sizing:border-box}</style><div class=c style="width:100px;min-width:300px;padding:0 20px;border:3px solid #a00"></div>"####, &[(0, 0, 300, 30, 0xccddee), (0, 0, 300, 3, 0xaa0000), (0, 27, 300, 3, 0xaa0000), (0, 3, 3, 24, 0xaa0000), (297, 3, 3, 24, 0xaa0000)]),
    ("b4", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde;box-sizing:border-box}</style><div class=c style="width:50%;max-width:200px;padding:0 10px;margin-left:30px"></div>"####, &[(30, 0, 200, 30, 0xccddee)]),
    ("b5", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde;box-sizing:border-box}</style><div class=c style="max-width:30px;padding:0 20px"></div>"####, &[(0, 0, 40, 30, 0xccddee)]),
    ("b6", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde;box-sizing:border-box}</style><div class=c style="width:200px;padding:0 10px;min-width:100px;max-width:400px;margin:0 auto"></div>"####, &[(300, 0, 200, 30, 0xccddee)]),
    ("b7", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde;box-sizing:border-box}</style><div class=c style="min-width:900px;padding:0 20px;border:5px solid #333"></div>"####, &[(0, 0, 900, 30, 0xccddee), (0, 0, 900, 5, 0x333333), (0, 25, 900, 5, 0x333333), (0, 5, 5, 20, 0x333333), (895, 5, 5, 20, 0x333333)]),
    ("b8", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde;box-sizing:border-box}</style><div class=c style="max-width:300px;padding:0 20px;box-sizing:content-box"></div>"####, &[(0, 0, 340, 30, 0xccddee)]),
    ("b9", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde;box-sizing:border-box}</style><div class=c style="width:320px;max-width:300px;padding:0 20px"></div>"####, &[(0, 0, 300, 30, 0xccddee)]),
    ("b10", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde;box-sizing:border-box}</style><div class=c style="width:100px;min-width:80px;padding:0 20px"></div>"####, &[(0, 0, 100, 30, 0xccddee)]),
];
#[test]
fn chrome_pixel_verified_fixtures_match_recorded_rects() {
    for (name, html, expected) in CHROME_VERIFIED {
        assert_eq!(fills(html).as_deref(), Some(*expected), "{name}");
    }
    // anchors: clamp changes geometry (max shrinks, min grows, margin auto centres) vs the unclamped widths
    let get = |n: &str| CHROME_VERIFIED.iter().find(|(m, _, _)| *m == n).unwrap().2;
    assert_eq!(get("m1")[0].2, 300);
    assert_eq!(get("m4")[0].2, 200);
    assert_eq!(get("m2")[0].0, 250);
    assert_eq!(get("n4")[0].2, 900);
    assert_eq!(get("b1")[0].2, 300); // border-box: max-width bounds the border box, not the content box
    assert_eq!(get("b8")[0].2, 340); // content-box control: 300 content + 40 edges
}
#[test]
fn default_entry_still_refuses_block_min_max_width() {
    for css in ["max-width:300px", "min-width:200px", "max-width:300px;margin:0 auto"] {
        let html = format!("<!doctype html><style>body{{margin:0}}</style><div style='height:30px;background:#cde;{css}'></div>");
        let t = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
        let admitted = !t.unsupported && { let b = BoxTree::from_style(&t); !b.unsupported && paint_backgrounds(&b, 800, 600).is_ok() };
        assert!(!admitted, "{css}");
    }
}
#[test]
fn unmodelled_shapes_stay_refused_in_ext_entry() {
    let pre = "<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style>";
    for (name, body) in [
        ("inexact_percent_width_with_max", "<div class=c style='width:33.3%;max-width:300px'></div>"),
        ("border_box_percent_min", "<div class=c style='box-sizing:border-box;min-width:10%'></div>"),
        ("floated", "<div class=c style='float:left;width:100px;max-width:50px'></div>"),
        ("relative", "<div class=c style='position:relative;max-width:300px'></div>"),
        ("absolute", "<div class=c style='position:absolute;width:400px;max-width:300px'></div>"),
        ("inline_block", "<div class=c style='display:inline-block;width:400px;max-width:300px'></div>"),
        ("rtl", "<div class=c style='direction:rtl;max-width:300px'></div>"),
        ("max_percent", "<div class=c style='max-width:50%'></div>"),
        ("min_em", "<div class=c style='min-width:20em'></div>"),
    ] {
        assert!(fills(&format!("{pre}{body}")).is_none(), "{name}");
    }
}
