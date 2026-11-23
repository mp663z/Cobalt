//! PRIVATE candidate FL37 (ext entry only): `width:<n>em` / `height:<n>em` resolved at the end of the cascade against the element's OWN
//! computed font size (exact whole px only); inexact products, `0em`, calc() and the default entry refuse. Carrier: a `Length::Percent`
//! payload with bit 31 set (no new pub variant); an unresolved carrier makes the style tree refuse the page. `parse_width` now refuses a percentage
//! >= 2^31 hundredths (would collide with the flag): a behaviour change for absurd percentages, disclosed. Provenance: each fixture's engine Fill
//! list was rasterised over white (private-evidence ul1/em50 cmp.py) and compared with a Chrome 156 headless screenshot at 800x600, dsf 1,
//! hide-scrollbars: 0 differing pixels (constructed diagnostics, ext entry, MediaContext default, synthetic, text-free).
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
    ("e1", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='width:10em;height:2em'></div>"####, &[(0, 0, 160, 32, 0xccddee)]),
    ("e2", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0;font-size:10px'><div class=c style='width:10em;height:2em;margin:1em'></div>"####, &[(10, 10, 100, 20, 0xccddee)]),
    ("e3", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div style='font-size:20px'><div class=c style='width:5em;height:1em'></div></div>"####, &[(0, 0, 100, 20, 0xccddee)]),
    ("e4", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='font-size:20px;width:5em;height:1.5em'></div>"####, &[(0, 0, 100, 30, 0xccddee)]),
    ("e5", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='font-size:20px;width:5em;height:30px'><div class=d style='height:1em;width:2em'></div></div>"####, &[(0, 0, 100, 30, 0xccddee), (0, 0, 40, 20, 0xffddaa)]),
    ("e6", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='font-size:12px;width:12.5em;height:2.5em'></div>"####, &[(0, 0, 150, 30, 0xccddee)]),
    ("e7", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><html style='font-size:20px'><body style='margin:0'><div class=c style='width:5em;height:1em'></div>"####, &[(0, 0, 100, 20, 0xccddee)]),
    ("e8", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='font-size:150%;width:10em;height:2em'></div>"####, &[(0, 0, 240, 48, 0xccddee)]),
    ("e9", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='font-size:2rem;width:4em;height:1em'></div>"####, &[(0, 0, 128, 32, 0xccddee)]),
    ("g1", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='width:10em;height:2em;position:relative'></div>"####, &[(0, 0, 160, 32, 0xccddee)]),
];
const REFUSED: &[(&str, &str)] = &[
    ("f1", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='width:10.3em;height:30px'></div>"####),
    ("f2", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='width:200px;height:.3em'></div>"####),
    ("f3", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='width:0em;height:30px'></div>"####),
    ("f4", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='font-size:15px;width:1.5em;height:1em'></div>"####),
    ("g2", r####"<!doctype html><style>.c{background:#cde}.d{background:#fda}</style><body style='margin:0'><div class=c style='width:calc(10em);height:30px'></div>"####),
];
fn page(style: &str) -> String {
    format!("<!doctype html><style>body{{margin:0}}.c{{background:#cde;{style}}}</style><div class=c></div>")
}
#[test]
fn chrome_pixel_verified_fixtures_match_recorded_rects() {
    for (name, html, expected) in CHROME_VERIFIED {
        assert_eq!(fills(html).as_deref(), Some(*expected), "{name}");
    }
}
#[test]
fn em_equals_its_resolved_px_twin_using_own_font_size() {
    let a = fills(&page("font-size:20px;width:5em;height:1.5em")).unwrap();
    let b = fills(&page("font-size:20px;width:100px;height:30px")).unwrap();
    assert_eq!(a, b);
    assert_eq!(a, vec![(0, 0, 100, 30, 0xccddee)]);
    // font-size declared AFTER width in source order still applies (own computed size, not source order)
    let c = fills(&page("width:5em;height:30px;font-size:20px")).unwrap();
    assert_eq!(c, vec![(0, 0, 100, 30, 0xccddee)]);
}
#[test]
fn later_declaration_wins_between_em_rem_and_px() {
    let a = fills(&page("height:20px;width:5em;width:50px")).unwrap();
    assert_eq!(a, vec![(0, 0, 50, 20, 0xccddee)]);
    let b = fills(&page("height:20px;width:50px;width:5em")).unwrap();
    assert_eq!(b, vec![(0, 0, 80, 20, 0xccddee)]);
    let c = fills(&page("height:20px;width:5em;width:auto")).unwrap();
    assert_eq!(c, vec![(0, 0, 800, 20, 0xccddee)]);
    let d = fills(&page("height:20px;width:5em;width:2rem")).unwrap();
    assert_eq!(d, vec![(0, 0, 32, 20, 0xccddee)]);
}
#[test]
fn unrepresentable_em_lengths_stay_refused() {
    for (name, html) in REFUSED {
        assert!(fills(html).is_none(), "{name}");
    }
    for v in ["width:1.55em;height:20px", "height:0.1em;width:20px", "width:-1em;height:20px", "width:5em 5px;height:20px", "width:5EMS;height:20px"] {
        assert!(fills(&page(v)).is_none(), "{v}");
    }
    // inexact on the element's own size even if the parent's would be exact
    let html = "<!doctype html><style>body{margin:0;font-size:16px}.c{background:#cde;font-size:15px;width:1.5em;height:20px}</style><div class=c></div>";
    assert!(fills(html).is_none());
}
#[test]
fn unresolved_em_on_after_pseudo_refuses() {
    let html = "<!doctype html><style>body{margin:0}.c::after{content:\"\";display:block;width:.3em;height:10px;background:#cde}</style><div class=c></div>";
    assert!(fills(html).is_none());
}
#[test]
fn percent_width_still_works_and_flag_collision_refuses() {
    let a = fills(&page("width:50%;height:20px")).unwrap();
    assert_eq!(a, vec![(0, 0, 400, 20, 0xccddee)]);
    assert!(fills(&page("width:30000000%;height:20px")).is_none());
}
#[test]
fn default_entry_does_not_recognise_em_width_height() {
    for v in ["width:10em", "height:2em"] {
        let html = format!("<!doctype html><style>body{{margin:0}}</style><div style='background:#cde;height:20px;{v}'></div>");
        assert!(parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT).unsupported, "{v}");
    }
}
// ADDITIVE regression tests after reviewer correction of the FL37 mutant claims (V3/V5/V6/V7 were NOT redundant).
#[test]
fn percent_colliding_with_em_flag_with_fractional_digits_refuses() {
    // 21474842.73% = 2147484273 hundredths = 2^31 + 625: without the parse guard this carrier resolves to 1px at font 16px.
    assert!(fills(&page("width:21474842.73%;height:20px")).is_none());
    assert!(fills(&page("height:21474842.73%;width:20px")).is_none());
}
#[test]
fn unresolved_em_is_refused_even_where_width_or_height_would_be_ignored() {
    // 0.1002em at 16px = 1.6032px: inexact. The carrier payload (1002*... low bits) must never leak as 0px in a zero basis or inline.
    let w = "width:0.1002em";
    let h = "height:0.1002em";
    // zero percent basis: parent width:0 and height:0 with the inexact em child
    let zero_w = format!("<!doctype html><style>body{{margin:0}}.p{{width:0;height:30px}}.c{{background:#cde;height:10px;{w}}}</style><div class=p><div class=c></div></div>");
    assert!(fills(&zero_w).is_none());
    let zero_h = format!("<!doctype html><style>body{{margin:0}}.p{{width:100px;height:0}}.c{{background:#cde;width:10px;{h}}}</style><div class=p><div class=c></div></div>");
    assert!(fills(&zero_h).is_none());
    // inline element: width/height ignored by layout, carrier must still refuse
    let inl_w = format!("<!doctype html><style>body{{margin:0}}span{{{w}}}</style><div style='height:10px'><span></span></div>");
    assert!(fills(&inl_w).is_none());
    let inl_h = format!("<!doctype html><style>body{{margin:0}}span{{{h}}}</style><div style='height:10px'><span></span></div>");
    assert!(fills(&inl_h).is_none());
    // display:none element with an inexact em carrier: refused as well (the page cannot be trusted)
    let none = format!("<!doctype html><style>body{{margin:0}}.n{{display:none;{w}}}</style><div class=n></div><div style='height:10px;background:#cde'></div>");
    assert!(fills(&none).is_none());
}
#[test]
fn unresolved_em_on_after_pseudo_refuses_in_inline_and_zero_basis() {
    let w = "width:0.1002em";
    let h = "height:0.1002em";
    for decl in [w, h] {
        let inline = format!("<!doctype html><style>body{{margin:0}}.c::after{{content:\"\";display:inline;{decl}}}</style><div class=c style='height:10px'></div>");
        assert!(fills(&inline).is_none(), "{decl} inline");
        let zero = format!("<!doctype html><style>body{{margin:0}}.p{{width:0;height:0}}.c::after{{content:\"\";display:block;background:#cde;height:5px;width:5px;{decl}}}</style><div class=p><div class=c></div></div>");
        assert!(fills(&zero).is_none(), "{decl} zero");
    }
}
// FL37c: tests only. display:none boxes make no box, so only the style-tree carrier refusal can reject them (reviewer-traced).
#[test]
fn inexact_em_height_on_display_none_element_refuses_with_visible_sibling() {
    let html = "<!doctype html><style>body{margin:0}.n{display:none;height:0.1002em}</style><div class=n></div><div style='height:10px;background:#cde'></div>";
    assert!(fills(html).is_none());
}
#[test]
fn inexact_em_on_after_that_makes_no_box_refuses() {
    for decl in ["width:0.1002em", "height:0.1002em"] {
        let normal = format!("<!doctype html><style>body{{margin:0}}.c::after{{{decl}}}</style><div class=c style='height:10px;background:#cde'></div>");
        assert!(fills(&normal).is_none(), "{decl} content normal");
        let none = format!("<!doctype html><style>body{{margin:0}}.c::after{{content:\"\";display:none;{decl}}}</style><div class=c style='height:10px;background:#cde'></div>");
        assert!(fills(&none).is_none(), "{decl} display:none");
    }
}
