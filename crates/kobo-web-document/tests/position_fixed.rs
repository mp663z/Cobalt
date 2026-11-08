//! PRIVATE candidate FL32 (ext entry only): `position:fixed` evaluated at scroll offset 0 is laid out and painted as `absolute`
//! against the initial containing block, and ONLY when no ancestor is positioned. Provenance: for each fixture below the engine's
//! Command::Fill list was rasterised in painter order over white (private-evidence ul1/fx36/cmp.py) and compared with a Chrome 156
//! headless screenshot at 800x600, device scale 1 of the FIXED page: 0 differing pixels (constructed diagnostics, ext entry,
//! MediaContext default, synthetic, text-free). In Chrome the position:absolute twin of each fixture is pixel-identical to the fixed
//! page except h3 (fixed under a position:relative ancestor), which the engine refuses. This test compares engine Fill rects with
//! recorded rects; no Chrome/stored pixels in the loop.
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
    ("g4", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div style="margin:40px;padding:20px;background:#eee"><div style="position:fixed;top:5px;right:5px;width:60px;height:60px;background:#00f"></div><div class=c></div></div>"####, &[(40, 40, 720, 70, 0xeeeeee), (60, 60, 680, 30, 0xccddee), (735, 5, 60, 60, 0x0000ff)]),
    ("h1", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div style="margin:40px;padding:20px;background:#eee"><div class=c></div><div style="position:fixed;top:5px;left:300px;width:60px;height:60px;background:#00f"></div><div class=c style="background:#9c9;margin-top:10px"></div></div>"####, &[(40, 40, 720, 110, 0xeeeeee), (60, 60, 680, 30, 0xccddee), (60, 100, 680, 30, 0x99cc99), (300, 5, 60, 60, 0x0000ff)]),
    ("h2", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div style="margin:40px;padding:20px;background:#eee"><div style="position:fixed;top:5px;left:5px;width:60px;height:60px;background:#00f"></div><div style="position:fixed;top:25px;left:25px;width:60px;height:60px;background:#f90"></div><div class=c></div></div>"####, &[(40, 40, 720, 70, 0xeeeeee), (60, 60, 680, 30, 0xccddee), (5, 5, 60, 60, 0x0000ff), (25, 25, 60, 60, 0xff9900)]),
    ("h4", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div style="margin:40px;padding:20px;background:#eee"><div style="position:fixed;top:5px;left:200px;width:100px;height:100px;background:#00f"><div style="position:absolute;top:10px;left:10px;width:30px;height:30px;background:#f90"></div></div><div class=c></div></div>"####, &[(40, 40, 720, 70, 0xeeeeee), (60, 60, 680, 30, 0xccddee), (200, 5, 100, 100, 0x0000ff), (210, 15, 30, 30, 0xff9900)]),
    ("h6", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div style="margin:10px"><div style="padding:10px;background:#eee"><div style="position:fixed;bottom:5px;right:5px;width:60px;height:60px;background:#00f"></div><div class=c></div></div></div>"####, &[(10, 10, 780, 50, 0xeeeeee), (20, 20, 760, 30, 0xccddee), (735, 535, 60, 60, 0x0000ff)]),
    ("h7", r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div style="margin:40px;padding:20px;background:#eee"><div style="position:fixed;top:-20px;left:-20px;width:60px;height:60px;background:#00f"></div><div class=c></div></div>"####, &[(40, 40, 720, 70, 0xeeeeee), (60, 60, 680, 30, 0xccddee), (-20, -20, 60, 60, 0x0000ff)]),
];
#[test]
fn chrome_pixel_verified_fixed_fixtures_match_recorded_rects() {
    for (name, html, expected) in CHROME_VERIFIED {
        assert_eq!(fills(html).as_deref(), Some(*expected), "{name}");
        // the absolute twin paints identically in the engine
        let twin = html.replace("position:fixed", "position:absolute");
        assert_eq!(fills(&twin).as_deref(), Some(*expected), "{name} twin");
    }
    assert!(CHROME_VERIFIED.iter().all(|(_, _, e)| e.len() >= 3));
}
const PRE: &str = "<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style>";
#[test]
fn fixed_under_a_positioned_ancestor_stays_refused() {
    for (name, body) in [
        ("under_relative", "<div style='position:relative;margin:40px;padding:20px;background:#eee'><div style='position:fixed;top:5px;right:5px;width:60px;height:60px;background:#00f'></div><div class=c></div></div>"),
        ("under_relative_via_static_parent", "<div style='position:relative;margin:10px'><div style='margin:40px;padding:20px;background:#eee'><div style='position:fixed;top:5px;left:5px;width:60px;height:60px;background:#00f'></div><div class=c></div></div></div>"),
        ("under_absolute", "<div style='position:absolute;top:10px;left:10px;width:300px;height:300px;background:#eee'><div style='position:fixed;top:5px;left:5px;width:60px;height:60px;background:#00f'></div></div>"),
        ("under_fixed", "<div style='position:fixed;top:10px;left:10px;width:300px;height:300px;background:#eee'><div style='position:fixed;top:5px;left:5px;width:60px;height:60px;background:#00f'></div></div>"),
    ] {
        assert!(fills(&format!("{PRE}{body}")).is_none(), "{name}");
    }
    // anchor: the same shape without the positioned ancestor is admitted
    assert!(fills(&format!("{PRE}<div style='margin:40px;padding:20px;background:#eee'><div style='position:fixed;top:5px;left:5px;width:60px;height:60px;background:#00f'></div><div class=c></div></div>")).is_some());
}
#[test]
fn default_entry_does_not_recognise_position_fixed() {
    let html = format!("{PRE}<div style='margin:40px;background:#eee'><div style='position:fixed;top:5px;left:5px;width:60px;height:60px;background:#00f'></div><div class=c></div></div>");
    assert!(parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT).unsupported);
}
#[test]
fn fixed_cascade_reset_to_static_is_a_typed_override() {
    let html = format!("{PRE}<style>.f{{position:fixed;top:5px;left:5px;width:60px;height:60px;background:#00f}}</style><div style='margin:40px;padding:20px;background:#eee'><div class=f style='position:static'></div><div class=c></div></div>");
    // static wins by order (inline after sheet): the box is an ordinary 60px-wide, 60px-tall block
    let f = fills(&html).expect("admitted");
    assert!(f.iter().any(|r| r.0 == 60 && r.1 == 60 && r.2 == 60 && r.3 == 60 && r.4 == 0x0000ff), "{f:?}");
}
