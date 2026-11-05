//! Generic retention must not silently admit an unbound font provider.
use kobo_web_document::{
    box_tree::BoxTree, computed_style::GenericFamily, css_background::paint_backgrounds,
    parse_style_tree, Limits,
};
fn styled(css: &str) -> kobo_web_document::style_tree::StyleTree {
    parse_style_tree(
        format!("<!doctype html><style>{css}</style><section><p>words</p></section>").as_bytes(),
        &[],
        &Limits::DEFAULT,
    )
}
#[test]
fn generic_family_retains_and_inherits_but_render_stays_refused() {
    for (name, family) in [
        ("serif", GenericFamily::Serif),
        ("sans-serif", GenericFamily::SansSerif),
        ("monospace", GenericFamily::Monospace),
    ] {
        let s = styled(&format!("section{{font-family:{name}}}"));
        assert!(s.unsupported);
        assert!(s
            .nodes
            .iter()
            .filter(|n| n.text.as_deref() == Some("words"))
            .all(|n| n.style.font_family == Some(family)));
        assert!(s.nodes.iter().any(|n| n.style.font_family == Some(family)));
        assert!(paint_backgrounds(&BoxTree::from_style(&s), 400, 300).is_err());
    }
}
#[test]
fn family_cascade_global_reset_importance_and_inline_are_retained() {
    for (decl, expected) in [
        (
            "font-family:serif;font-family:sans-serif",
            Some(GenericFamily::SansSerif),
        ),
        (
            "font-family:serif!important;font-family:monospace",
            Some(GenericFamily::Serif),
        ),
        ("font-family:serif;font-family:initial", None),
        (
            "font-family:serif;font-family:unset",
            Some(GenericFamily::Monospace),
        ),
        (
            "font-family:serif;font-family:inherit",
            Some(GenericFamily::Monospace),
        ),
        (
            "font-family:serif;font-family:revert",
            Some(GenericFamily::Monospace),
        ),
    ] {
        let s = styled(&format!("section{{font-family:monospace}}p{{{decl}}}"));
        let text = s
            .nodes
            .iter()
            .find(|n| n.text.as_deref() == Some("words"))
            .unwrap();
        assert_eq!(text.style.font_family, expected, "{decl}");
        assert!(s.unsupported);
    }
    let s = parse_style_tree(
        b"<!doctype html><p style='font-family:sans-serif'>words</p>",
        &[],
        &Limits::DEFAULT,
    );
    assert_eq!(
        s.nodes
            .iter()
            .find(|n| n.text.as_deref() == Some("words"))
            .unwrap()
            .style
            .font_family,
        Some(GenericFamily::SansSerif)
    );
    assert!(s.unsupported);
}
#[test]
fn named_lists_quotes_fontface_and_other_generics_stay_unsupported() {
    for value in [
        "Arial",
        "serif, sans-serif",
        "'serif'",
        "system-ui",
        "cursive",
        "fantasy",
        "ui-serif",
        "serif serif",
    ] {
        let s = styled(&format!("p{{font-family:{value}}}"));
        assert!(s.unsupported, "{value}");
        assert!(s.nodes.iter().all(|n| n.style.font_family.is_none()));
    }
    let s = styled("@font-face{font-family:test;src:url(test.ttf)}");
    assert!(s.unsupported);
}
#[test]
fn absent_family_stays_unresolved_with_no_new_ua_binding() {
    let s = styled("");
    assert!(!s.unsupported);
    assert!(s.nodes.iter().all(|n| n.style.font_family.is_none()));
}
