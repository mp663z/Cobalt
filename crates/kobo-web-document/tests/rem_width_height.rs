//! PRIVATE candidate FL36 (ext entry only): `width:<n>rem` / `height:<n>rem` resolved against the root computed font size
//! (initial 16px; html font-size applies to descendants only, as in CSS) and rewritten to px before the cascade, only when the result is
//! whole px. Fractional results, `0rem`, calc(), and the default entry refuse. Provenance: each fixture's engine Fill list was rasterised
//! in painter order over white (private-evidence ul1/rm49 cmp.py) and compared with a Chrome 156 headless screenshot at 800x600, device
//! scale 1, hide-scrollbars: 0 differing pixels (constructed diagnostics, ext entry, MediaContext default, synthetic, text-free).
//! Tests compare engine Fill rects with recorded rects; no Chrome/stored pixels in the loop.
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
    ("a1", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='width:10rem;height:2rem'></div>"####, &[(0, 0, 160, 32, 0xccddee)]),
    ("a2", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='width:10rem;height:30px;margin:1rem 2rem'></div>"####, &[(32, 16, 160, 30, 0xccddee)]),
    ("a4", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><html style='font-size:20px'><body style='margin:0'><div class=c style='width:5rem;height:1.5rem'></div>"####, &[(0, 0, 100, 30, 0xccddee)]),
    ("a8", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='width:12.5rem;height:30px'></div>"####, &[(0, 0, 200, 30, 0xccddee)]),
    ("a9", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><html style='font-size:50%'><body style='margin:0'><div class=c style='width:10rem;height:30px'></div>"####, &[(0, 0, 80, 30, 0xccddee)]),
    ("m3", r####"<!doctype html><style>.c{background:#cde}</style><body style='margin:0'><div class=c style='width:10rem;height:30px'></div>"####, &[(0, 0, 160, 30, 0xccddee)]),
    ("m4", r####"<!doctype html><style>.c{background:#cde}</style><body style='margin:0'><div class=c style='width:200px;height:2rem'></div>"####, &[(0, 0, 200, 32, 0xccddee)]),
    ("n1", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='width:12rem;height:2rem'></div><div class=d style='width:6.25rem;height:3rem'></div>"####, &[(0, 0, 192, 32, 0xccddee), (0, 32, 100, 48, 0xffddaa)]),
    ("n2", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><html style='font-size:62.5%'><body style='margin:0'><div class=c style='width:20rem;height:5rem;margin:1rem'></div>"####, &[(10, 10, 200, 50, 0xccddee)]),
    ("n5", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0;font-size:30px'><div class=c style='width:10rem;height:1rem'></div>"####, &[(0, 0, 160, 16, 0xccddee)]),
    ("n6", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='width:10rem;height:30px;position:relative'></div>"####, &[(0, 0, 160, 30, 0xccddee)]),
    ("n9", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='width:10rem;height:2rem;min-width:100px'></div>"####, &[(0, 0, 160, 32, 0xccddee)]),
    ("n10", r####"<!doctype html><style>.c{background:#cde}</style><body style='margin:0'><div class=c style='width:1.5rem;height:1.25rem'></div>"####, &[(0, 0, 24, 20, 0xccddee)]),
];
const REFUSED: &[(&str, &str)] = &[
    ("n3", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='width:.3rem;height:30px'></div>"####),
    ("n4", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='width:200px;height:.3rem'></div>"####),
    ("n7", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='width:calc(10rem);height:30px'></div>"####),
    ("n8", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='width:0rem;height:30px'></div>"####),
];
#[test]
fn chrome_pixel_verified_fixtures_match_recorded_rects() {
    for (name, html, expected) in CHROME_VERIFIED {
        assert_eq!(fills(html).as_deref(), Some(*expected), "{name}");
    }
}
#[test]
fn rem_equals_its_resolved_px_twin() {
    let a = fills("<!doctype html><style>body{margin:0}.c{background:#cde;width:10rem;height:2rem}</style><div class=c></div>").unwrap();
    let b = fills("<!doctype html><style>body{margin:0}.c{background:#cde;width:160px;height:32px}</style><div class=c></div>").unwrap();
    assert_eq!(a, b);
    assert_eq!(a, vec![(0, 0, 160, 32, 0xccddee)]);
}
#[test]
fn later_declaration_wins_over_rem_and_vice_versa() {
    let base = "<!doctype html><style>body{margin:0}.c{background:#cde;height:20px;";
    let a = fills(&format!("{base}width:10rem;width:50px}}</style><div class=c></div>")).unwrap();
    assert_eq!(a, vec![(0, 0, 50, 20, 0xccddee)]);
    let b = fills(&format!("{base}width:50px;width:10rem}}</style><div class=c></div>")).unwrap();
    assert_eq!(b, vec![(0, 0, 160, 20, 0xccddee)]);
    let c = fills(&format!("{base}width:10rem;width:auto}}</style><div class=c></div>")).unwrap();
    assert_eq!(c, vec![(0, 0, 800, 20, 0xccddee)]);
}
#[test]
fn unrepresentable_rem_lengths_stay_refused() {
    for (name, html) in REFUSED {
        assert!(fills(html).is_none(), "{name}");
    }
    for v in ["width:1.55rem", "height:0.1rem", "width:-1rem", "width:10rem 5px", "height:1.1rem"] {
        let html = format!("<!doctype html><style>body{{margin:0}}</style><div style='background:#cde;height:20px;{v}'></div>");
        assert!(fills(&html).is_none(), "{v}");
    }
}
#[test]
fn default_entry_does_not_recognise_rem_width_height() {
    for v in ["width:10rem", "height:2rem"] {
        let html = format!("<!doctype html><style>body{{margin:0}}</style><div style='background:#cde;height:20px;{v}'></div>");
        assert!(parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT).unsupported, "{v}");
    }
}
