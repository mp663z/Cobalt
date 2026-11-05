//! `@media` width queries evaluated against an explicit viewport context.
use kobo_web_document::{
    box_tree::BoxTree, css_background::paint_backgrounds, display_list::Rgb, parse_style_tree,
    parse_style_tree_with_media, style_diagnostics::RefusalReason, style_tree::StyleTree, Limits,
    MediaContext,
};
fn at(html: &str, width: u32) -> StyleTree {
    let media = MediaContext::new(width).expect("non-zero width");
    parse_style_tree_with_media(html.as_bytes(), &[], &Limits::DEFAULT, media)
}
fn page(css: &str) -> String {
    format!("<!doctype html><style>{css}</style><p>x</p>")
}
fn p_size(tree: &StyleTree) -> u32 {
    assert!(!tree.unsupported, "{:?}", tree.diagnostics);
    tree.nodes
        .iter()
        .find(|n| n.tag == "p")
        .unwrap()
        .style
        .font_size
}
#[test]
fn width_decides_which_block_applies_exactly_at_the_boundary() {
    let css = "p{font-size:10px}@media (min-width:641px){p{font-size:20px}}@media (max-width:640px){p{font-size:30px}}";
    assert_eq!(p_size(&at(&page(css), 640)), 30);
    assert_eq!(p_size(&at(&page(css), 641)), 20);
    assert_eq!(p_size(&at(&page(css), 1072)), 20);
    assert_eq!(p_size(&at(&page(css), 100)), 30);
}
#[test]
fn gov_fractional_em_values_apply_without_rounding() {
    // gov: min-width 40.0625em (641px) and max-width 40.0525em (640.84px).
    let css = "p{font-size:10px}@media (min-width: 40.0625em){p{font-size:20px}}@media (max-width: 40.0525em){p{font-size:30px}}";
    assert_eq!(p_size(&at(&page(css), 640)), 30);
    // At 641 only the min block applies: rounding 640.84 up would also apply
    // the max block later in source order and give 30.
    assert_eq!(p_size(&at(&page(css), 641)), 20);
    assert_eq!(p_size(&at(&page(css), 1264)), 20);
}
#[test]
fn reparse_at_another_width_is_the_resize_contract() {
    let html = page("@media (min-width:600px){p{font-size:22px}}p{font-size:11px}");
    // Later unconditional rule wins by source order: width changes whether
    // the earlier media rule participates at all.
    assert_eq!(p_size(&at(&html, 700)), 11);
    let html = page("p{font-size:11px}@media (min-width:600px){p{font-size:22px}}");
    assert_eq!(p_size(&at(&html, 599)), 11);
    assert_eq!(p_size(&at(&html, 600)), 22);
    assert_eq!(p_size(&at(&html, 600)), p_size(&at(&html, 600)));
}
#[test]
fn no_context_refuses_every_width_query_and_never_defaults_a_width() {
    let tree = parse_style_tree(
        page("@media (min-width:1px){p{font-size:20px}}").as_bytes(),
        &[],
        &Limits::DEFAULT,
    );
    assert!(tree.unsupported);
    assert_eq!(tree.diagnostics.count(RefusalReason::CssCondition), 1);
    assert!(MediaContext::new(0).is_none());
}
#[test]
fn unknown_conditions_still_refuse_even_with_a_context() {
    for css in [
        "@media (hover:hover){p{font-size:20px}}",
        "@media (min-width:1px) and (hover:hover){p{font-size:20px}}",
        "@media (min-width:1px),(max-width:2px){p{font-size:20px}}",
        "@media (min-width:1px) or (max-width:2px){p{font-size:20px}}",
        "@media not (min-width:9999px){p{font-size:20px}}",
        "@media (min-width:1rem){p{font-size:20px}}",
        "@media (min-width:-1px){p{font-size:20px}}",
        "@supports (display:grid){@media (min-width:1px){p{font-size:20px}}}",
        "@container (min-width:1px){p{font-size:20px}}",
    ] {
        let tree = at(&page(css), 700);
        assert!(tree.unsupported, "{css}");
    }
    let tree = at(&page("@media (hover:hover){p{font-size:20px}}"), 700);
    assert_eq!(tree.diagnostics.count(RefusalReason::CssCondition), 1);
}
#[test]
fn known_false_blocks_hide_nothing_else_and_true_blocks_keep_declaration_policy() {
    // Known-false: its unsupported declaration is not evaluated.
    let tree = at(&page("@media (min-width:641px){p{display:flex}}"), 100);
    assert!(!tree.unsupported);
    // Known-true: flex, a custom property and rem margins are NOT incidentally supported.
    // RECONSTRUCTION CANDIDATE: original list was display:flex, flex-direction:row, :root{--x:1},
    // margin-top:1rem. display:flex and rem margins became supported later in the chain
    // (art41/art33), so they are asserted supported in the next test instead. Still-refused
    // members were re-probed against this tree.
    for body in [
        "p{flex-direction:row}",
        ":root{--x:1}",
        "p{display:grid}",
        "p{margin-top:calc(1px + 2px)}",
    ] {
        let tree = at(&page(&format!("@media (min-width:641px){{{body}}}")), 700);
        assert!(tree.unsupported, "{body}");
        assert_eq!(
            tree.diagnostics.count(RefusalReason::CssDeclaration),
            1,
            "{body}"
        );
    }
    // The gov values: margin-right:20px is supported, so a true block is clean.
    let tree = at(
        &page("@media (min-width: 40.0625em){p{margin-right:20px}}"),
        700,
    );
    assert!(!tree.unsupported);
}
#[test]
fn known_true_blocks_with_later_supported_declarations_are_clean() {
    // display:flex and rem margins were refused at head7; the chain supports them now.
    for body in ["p{display:flex}", "p{margin-top:1rem}", "p{margin-top:1em}"] {
        let tree = at(&page(&format!("@media (min-width:641px){{{body}}}")), 700);
        assert!(!tree.unsupported, "{body}: {:?}", tree.diagnostics);
    }
}
#[test]
fn nested_media_blocks_combine_and_a_false_ancestor_wins() {
    let css =
        "p{font-size:10px}@media (min-width:100px){@media (max-width:900px){p{font-size:20px}}}";
    assert_eq!(p_size(&at(&page(css), 500)), 20);
    assert_eq!(p_size(&at(&page(css), 50)), 10);
    assert_eq!(p_size(&at(&page(css), 901)), 10);
}
// Replaced regression history: this test used to assert that a `<style>` media
// attribute with a width feature always refused, even with a MediaContext.
// Link and style media attributes now follow the context (tests/css_link_media.rs).
// The old assertion survives for the context-less parse, which still refuses.
#[test]
fn style_media_attribute_without_a_context_still_refuses() {
    let html = "<!doctype html><style media='(min-width:1px)'>p{font-size:20px}</style><p>x</p>";
    let tree = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
    assert!(tree.unsupported);
    assert_eq!(tree.diagnostics.count(RefusalReason::StylesheetMedia), 1);
    // With a context the same attribute is evaluated, not refused.
    let tree = at(html, 700);
    assert!(!tree.unsupported, "{:?}", tree.diagnostics);
    assert_eq!(p_size(&tree), 20);
}
fn raster(css: &str, width: u32) -> Option<Vec<u8>> {
    let html = format!(
        "<!doctype html><style>html,body{{margin:0}}{css}</style><html><body><div style='width:10px;height:10px'></div></body></html>"
    );
    let tree = at(&html, width);
    if tree.unsupported {
        return None;
    }
    let list = paint_backgrounds(&BoxTree::from_style(&tree), 40, 40).ok()?;
    list.rasterize(40, 40, Rgb(255, 255, 255)).ok()
}
fn px(p: &[u8]) -> [u8; 3] {
    p[(20 * 40 + 20) * 4..][..3].try_into().unwrap()
}
#[test]
fn pixels_follow_the_media_decision_and_unknown_never_paints() {
    let css =
        "html{background:rgb(0,0,250)}@media (min-width:641px){html{background:rgb(250,0,0)}}";
    assert_eq!(px(&raster(css, 700).unwrap()), [250, 0, 0]);
    assert_eq!(px(&raster(css, 640).unwrap()), [0, 0, 250]);
    let unknown =
        "html{background:rgb(0,0,250)}@media (hover:hover){html{background:rgb(250,0,0)}}";
    assert!(raster(unknown, 700).is_none());
    let comma = "html{background:rgb(0,0,250)}@media (min-width:1px),(max-width:2px){html{background:rgb(250,0,0)}}";
    assert!(raster(comma, 700).is_none());
}
