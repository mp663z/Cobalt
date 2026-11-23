//! PRIVATE, unadopted candidate: `rgba(R, G, B, 0)` as a border colour is accepted as exactly
//! `transparent` (shape: 3 decimal ints 0..=255, literal alpha 0). Everything else stays refused.
use kobo_web_document::{
    parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, parse_style_tree_with_media,
    parse_style_tree_with_pinned_font_family, Limits, SupplyBudget,
};

fn html(style: &str) -> String { format!("<!doctype html><p style='{style}'>x</p>") }
/// The extended-CSS opt-in entry (the only entry that accepts the new form).
fn tree(style: &str) -> kobo_web_document::style_tree::StyleTree {
    parse_style_tree_with_inline_sheets_and_selectors(html(style).as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
}
fn node(t: &kobo_web_document::style_tree::StyleTree) -> &kobo_web_document::style_tree::StyledNode {
    t.nodes.iter().find(|n| n.tag == "p").unwrap()
}

#[test]
fn zero_alpha_rgba_equals_transparent_in_every_border_form() {
    for (rgba, plain) in [
        ("border:2px solid rgba(0, 0, 0, 0)", "border:2px solid transparent"),
        ("border-bottom:10px solid rgba(0, 0, 0, 0)", "border-bottom:10px solid transparent"),
        ("border:rgba(0,0,0,0) 3px dashed", "border:transparent 3px dashed"),
        ("border-color:rgba(1, 2, 3, 0)", "border-color:transparent"),
        ("border-color:rgba(255,255,255,0) red blue rgba(9, 9, 9, 0)", "border-color:transparent red blue transparent"),
        ("border-left-color:rgba( 0 , 0 , 0 , 0 )", "border-left-color:transparent"),
    ] {
        let (a, b) = (tree(rgba), tree(plain));
        assert!(!a.unsupported, "{rgba}");
        assert_eq!(node(&a).style.decor, node(&b).style.decor, "{rgba}");
        assert_eq!(format!("{:?}", node(&a).style), format!("{:?}", node(&b).style), "full computed style {rgba}");
    }
}

#[test]
fn everything_else_stays_refused() {
    for bad in [
        "border:2px solid rgba(0, 0, 0, 0.5)",
        "border:2px solid rgba(0, 0, 0, 0.0)",
        "border:2px solid rgba(0, 0, 0, 0%)",
        "border:2px solid rgba(0, 0, 0, 00)",
        "border:2px solid rgba(0, 0, 0, +0)",
        "border:2px solid rgba(0, 0, 0)",
        "border:2px solid rgba(0, 0, 0, 0, 0)",
        "border:2px solid rgba(256, 0, 0, 0)",
        "border:2px solid rgba(-1, 0, 0, 0)",
        "border:2px solid rgba(1.5, 0, 0, 0)",
        "border:2px solid rgba(0 0 0 / 0)",
        "border:2px solid rgba(0, 0, 0, 0",
        "border:2px solid rgba(0, 0, 0, 0))",
        "border:2px solid rgba (0, 0, 0, 0)",
        "border:2px solid rgba(0%, 0, 0, 0)",
        "border:2px solid rgb(0, 0, 0, 0)",
        "border:2px solid rgba(0,,0, 0)",
        "border-color:rgba(0,0,0,0) rgba(0,0,0,0) rgba(0,0,0,0) rgba(0,0,0,0) rgba(0,0,0,0)",
        "color:rgba(0, 0, 0, 0)",
        "background-color:rgba(0, 0, 0, 0)",
    ] {
        assert!(tree(bad).unsupported, "{bad}");
    }
}

#[test]
fn other_entries_keep_refusing_the_new_form() {
    for style in ["border:2px solid rgba(0, 0, 0, 0)", "border-color:rgba(0,0,0,0)", "border-bottom-color:rgba(1, 2, 3, 0)"] {
        let h = html(style);
        assert!(parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported, "default {style}");
        assert!(
            parse_style_tree_with_pinned_font_family(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL).unsupported,
            "pinned-family entry {style}"
        );
        if let Some(m) = kobo_web_document::MediaContext::new(785) {
            assert!(parse_style_tree_with_media(h.as_bytes(), &[], &Limits::DEFAULT, m).unsupported, "media entry {style}");
        }
        assert!(!tree(style).unsupported, "opt-in entry {style}");
    }
    // transparent itself is unchanged everywhere
    let h = html("border:2px solid transparent");
    assert!(!parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported);
}
