//! PRIVATE candidate (ext/with_state entries only): `max-width:<n>rem` / `min-width:<n>rem` /
//! `min-height:<n>rem` resolved against the root computed font size (initial 16px; the root's own
//! rem resolves against the initial 16px, html font-size applies to descendants only, as in CSS)
//! and rewritten to px before the cascade, only when the result is whole px. `0rem` refuses (note
//! the asymmetry: `0`/`0px`/`auto` remain admitted resets), negative rem, >4 decimal places,
//! parse-overflowing values, calc(), and the default/pinned entries refuse. Provenance: each
//! fixture's engine Fill list was rasterised in painter order over white and compared with a
//! Chrome 156 headless screenshot at 800x600, device scale 1, hide-scrollbars: 0 differing pixels
//! (constructed diagnostics, ext entry, MediaContext default, synthetic, text-free).
//! Tests compare engine Fill rects with recorded rects; no Chrome/stored pixels in the loop.
use kobo_web_document::{
    box_tree::BoxTree,
    css_background::paint_backgrounds,
    display_list::{Command, Rgb},
    parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors,
    parse_style_tree_with_pinned_font_family, parse_style_tree_with_state, HistoryState,
    InputState, Limits, SnapshotState, SupplyBudget,
};
type F = (i32, i32, u32, u32, u32);
fn tree(html: &str) -> Option<BoxTree> {
    let t = parse_style_tree_with_inline_sheets_and_selectors(
        html.as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    );
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
                Command::Fill {
                    rect,
                    color: Rgb(r, g, b),
                    ..
                } => Some((
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    (u32::from(*r) << 16) | (u32::from(*g) << 8) | u32::from(*b),
                )),
                _ => None,
            })
            .collect(),
    )
}
const CHROME_VERIFIED: &[(&str, &str, &[F])] = &[
    (
        "w1",
        r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:20rem"></div>"####,
        &[(0, 0, 320, 30, 0xccddee)],
    ),
    (
        "w2",
        r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="width:500px;max-width:20rem"></div>"####,
        &[(0, 0, 320, 30, 0xccddee)],
    ),
    (
        "w3",
        r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><html style="font-size:62.5%"><body style="margin:0"><div class=c style="max-width:20rem"></div></body></html>"####,
        &[(0, 0, 200, 30, 0xccddee)],
    ),
    (
        "w4",
        r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="width:100px;min-width:15rem"></div>"####,
        &[(0, 0, 240, 30, 0xccddee)],
    ),
    (
        "w5",
        r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="min-width:60rem"></div>"####,
        &[(0, 0, 960, 30, 0xccddee)],
    ),
    (
        "w6",
        r####"<!doctype html><style>body{margin:0}.c{background:#cde}</style><div class=c style="min-height:2rem"></div>"####,
        &[(0, 0, 800, 32, 0xccddee)],
    ),
    (
        "w7",
        r####"<!doctype html><style>body{margin:0}.c{background:#cde}</style><div class=c style="height:10px;min-height:2rem"></div>"####,
        &[(0, 0, 800, 32, 0xccddee)],
    ),
    (
        "w8",
        r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:20rem;margin:0 auto"></div>"####,
        &[(240, 0, 320, 30, 0xccddee)],
    ),
    (
        "w9",
        r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><html style="font-size:20px"><body style="margin:0"><div class=c style="max-width:25rem"></div></body></html>"####,
        &[(0, 0, 500, 30, 0xccddee)],
    ),
    (
        "w10",
        r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:0.5rem"></div>"####,
        &[(0, 0, 8, 30, 0xccddee)],
    ),
];
const REFUSED: &[(&str, &str)] = &[
    (
        "x1",
        r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:0rem"></div>"####,
    ),
    (
        "x2",
        r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:-1rem"></div>"####,
    ),
    (
        "x3",
        r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:1.00001rem"></div>"####,
    ),
    (
        "x4",
        r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><html style="font-size:62.5%"><body style="margin:0"><div class=c style="max-width:0.125rem"></div></body></html>"####,
    ),
    (
        "x5",
        r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:calc(20rem)"></div>"####,
    ),
    (
        "x6",
        r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="min-width:0rem"></div>"####,
    ),
    (
        "x7",
        r####"<!doctype html><style>body{margin:0}.c{background:#cde}</style><div class=c style="min-height:0rem"></div>"####,
    ),
    (
        "x8",
        r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:500000rem"></div>"####,
    ),
];
#[test]
fn chrome_pixel_verified_fixtures_match_recorded_rects() {
    for (name, html, expected) in CHROME_VERIFIED {
        assert_eq!(fills(html).as_deref(), Some(*expected), "{name}");
    }
}
#[test]
fn rem_minmax_equals_resolved_px_twin() {
    for (rem, px) in [
        ("max-width:20rem", "max-width:320px"),
        ("min-width:15rem", "min-width:240px"),
        ("min-height:2rem", "min-height:32px"),
    ] {
        let a = fills(&format!("<!doctype html><style>body{{margin:0}}.c{{height:30px;background:#cde}}</style><div class=c style=\"{rem}\"></div>")).unwrap();
        let b = fills(&format!("<!doctype html><style>body{{margin:0}}.c{{height:30px;background:#cde}}</style><div class=c style=\"{px}\"></div>")).unwrap();
        assert_eq!(a, b, "{rem}");
    }
}
#[test]
fn later_declaration_wins_over_rem_and_vice_versa() {
    let base = "<!doctype html><style>body{margin:0}.c{height:30px;background:#cde;";
    let a = fills(&format!(
        "{base}max-width:20rem;max-width:100px}}</style><div class=c></div>"
    ))
    .unwrap();
    assert_eq!(a, vec![(0, 0, 100, 30, 0xccddee)]);
    let b = fills(&format!(
        "{base}max-width:100px;max-width:20rem}}</style><div class=c></div>"
    ))
    .unwrap();
    assert_eq!(b, vec![(0, 0, 320, 30, 0xccddee)]);
    let c = fills(&format!(
        "{base}max-width:20rem;max-width:none}}</style><div class=c></div>"
    ))
    .unwrap();
    assert_eq!(c, vec![(0, 0, 800, 30, 0xccddee)]);
}
#[test]
fn root_own_rem_resolves_against_initial_16_not_author_root_font() {
    let html = r####"<!doctype html><style>body{margin:0}</style><html style="font-size:20px;max-width:25rem"><body><div></div></body></html>"####;
    let t = parse_style_tree_with_inline_sheets_and_selectors(
        html.as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    );
    assert!(!t.unsupported);
    let root = t.nodes.iter().find(|n| n.tag == "html").unwrap();
    assert_eq!(root.style.font_size, 20);
    assert_eq!(
        root.style.max_width,
        kobo_web_document::computed_style::Length::Px(400)
    ); // 25 * 16, not 25 * 20
}
#[test]
fn unrepresentable_rem_lengths_stay_refused() {
    for (name, html) in REFUSED {
        assert_eq!(fills(html), None, "{name}");
    }
}
#[test]
fn default_and_pinned_entries_refuse_rem_minmax() {
    let html = r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:20rem"></div>"####;
    let d = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
    assert!(d.unsupported, "default entry must refuse rem max-width");
    let p = parse_style_tree_with_pinned_font_family(
        html.as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    );
    assert!(p.unsupported, "pinned entry must refuse rem max-width");
    let sane = r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:300px"></div>"####;
    assert!(
        !parse_style_tree(sane.as_bytes(), &[], &Limits::DEFAULT).unsupported,
        "px max-width stays admitted under default"
    );
}
#[test]
fn state_entry_admits_rem_minmax_like_ext() {
    let html = r####"<!doctype html><style>body{margin:0}.c{height:30px;background:#cde}</style><div class=c style="max-width:20rem"></div>"####;
    let snapshot = SnapshotState {
        history: Some(HistoryState::FreshNoHistory),
        input: Some(InputState::QuiescentCapture),
        focus: None,
    };
    let t = parse_style_tree_with_state(
        html.as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
        &snapshot,
    );
    assert!(!t.unsupported);
    let bt = BoxTree::from_style(&t);
    assert!(!bt.unsupported);
    let l = paint_backgrounds(&bt, 800, 600).unwrap();
    let got: Vec<F> = l
        .commands()
        .iter()
        .filter_map(|c| match c {
            Command::Fill {
                rect,
                color: Rgb(r, g, b),
                ..
            } => Some((
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                (u32::from(*r) << 16) | (u32::from(*g) << 8) | u32::from(*b),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(got, vec![(0, 0, 320, 30, 0xccddee)]);
}
