//! UA-lists slice (ext entry only): ul/ol UA-origin defaults (margin-block 1em when no
//! dl/ul/ol/menu/dir ancestor, padding-inline-start 40px) supplied in the same gated change that
//! drops their required-props audit arm, so ul/ol join the dl/dt/dd wildcard class. Values are
//! Chrome 156 computed-style-probe verified (private-evidence ua-lists-design: flat margins = 1em
//! of the element's OWN computed font size at 16/20/10/32px contexts, nested = 0 margins, constant
//! padding-left 40px, author own-size em resolution + per-property override + !important cases).
//! Style-layer admission and computed values only; list markers (disc/decimal) stay unmodelled;
//! no pixel/readiness claim. Default entry UNCHANGED. All other audited elements still refuse
//! (`ua_audit_ext`).
use kobo_web_document::computed_style::{Display, Margin};
use kobo_web_document::{
    parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget,
};
fn ext(css: &str, body: &str) -> kobo_web_document::style_tree::StyleTree {
    let h = format!("<!doctype html><style>{css}</style><div>{body}</div>");
    parse_style_tree_with_inline_sheets_and_selectors(
        h.as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    )
}
fn node<'a>(
    t: &'a kobo_web_document::style_tree::StyleTree,
    tag: &str,
) -> &'a kobo_web_document::style_tree::StyledNode {
    t.nodes.iter().find(|n| n.tag == tag).unwrap()
}

// Preregistered test 1: flat ul/ol at default 16px and in a 20px context.
#[test]
fn flat_lists_get_chrome_ua_defaults() {
    let t = ext("", "<ul><li>a</li></ul><ol><li>b</li></ol>");
    assert!(!t.unsupported, "bare ul/ol admitted under ext after UA supply");
    let (ul, ol) = (node(&t, "ul"), node(&t, "ol"));
    assert_eq!(
        (
            ul.style.display,
            ul.style.margin_top,
            ul.style.margin_bottom,
            ul.style.padding_left
        ),
        (Display::Block, Margin::Px(16), Margin::Px(16), 40)
    );
    assert_eq!(
        (ol.style.margin_top, ol.style.margin_bottom, ol.style.padding_left),
        (Margin::Px(16), Margin::Px(16), 40)
    );
    // 20px context: margins are 1em of the element's own (inherited) 20px, not a constant 16px.
    let t = ext(
        "",
        "<div style='font-size:20px'><ul id=x><li>a</li></ul></div>",
    );
    assert!(!t.unsupported);
    let uls: Vec<_> = t.nodes.iter().filter(|n| n.tag == "ul").collect();
    assert_eq!(
        (uls[0].style.margin_top, uls[0].style.margin_bottom),
        (Margin::Px(20), Margin::Px(20))
    );
}

// Preregistered test 2: nested lists (any dl/ul/ol/menu/dir ancestor) get zero margins, keep 40px padding.
#[test]
fn nested_lists_get_zero_margins() {
    let t = ext(
        "",
        "<ul><li>a<ul id=inner><li>b</li></ul></li></ul><ol><li>c<ul id=x><li>d</li></ul></li></ol>",
    );
    assert!(!t.unsupported);
    let uls: Vec<_> = t.nodes.iter().filter(|n| n.tag == "ul").collect();
    // outer ul (first in document order) keeps 16px; the two nested uls are zeroed
    assert_eq!(uls[0].style.margin_top, Margin::Px(16));
    for u in &uls[1..] {
        assert_eq!(
            (u.style.margin_top, u.style.margin_bottom, u.style.padding_left),
            (Margin::Px(0), Margin::Px(0), 40)
        );
    }
    // ol inside ul is also nested
    let t = ext("", "<ul><li>a<ol><li>b</li></ol></li></ul>");
    assert!(!t.unsupported);
    assert_eq!(
        (node(&t, "ol").style.margin_top, node(&t, "ol").style.padding_left),
        (Margin::Px(0), 40)
    );
}

// Preregistered test 3: author font-size on ul scales the UA 1em margins (own-size em resolution).
#[test]
fn author_font_size_scales_ua_em_margins() {
    let t = ext("ul{font-size:24px}", "<ul><li>a</li></ul>");
    assert!(!t.unsupported);
    assert_eq!(
        (node(&t, "ul").style.margin_top, node(&t, "ul").style.margin_bottom),
        (Margin::Px(24), Margin::Px(24))
    );
}

// Preregistered test 4: author margin-top only overrides that side; the other side keeps the UA value.
#[test]
fn author_margin_overrides_per_property() {
    let t = ext("ul{margin-top:5px}", "<ul><li>a</li></ul>");
    assert!(!t.unsupported);
    assert_eq!(
        (node(&t, "ul").style.margin_top, node(&t, "ul").style.margin_bottom),
        (Margin::Px(5), Margin::Px(16))
    );
}

// Preregistered test 5: !important author declarations override the UA supply.
#[test]
fn important_author_declarations_override() {
    let t = ext(
        "ul{margin-top:0!important;margin-bottom:0!important;padding-left:12px!important}",
        "<ul><li>a</li></ul>",
    );
    assert!(!t.unsupported);
    assert_eq!(
        (
            node(&t, "ul").style.margin_top,
            node(&t, "ul").style.margin_bottom,
            node(&t, "ul").style.padding_left
        ),
        (Margin::Px(0), Margin::Px(0), 12)
    );
}

// Preregistered test 6: fully author-satisfied lists are byte-identical to pre-change behaviour
// (the UA supply loses on every property; values exactly as authored).
#[test]
fn author_satisfied_lists_unchanged() {
    let t = ext(
        "ul{margin-top:3px;margin-bottom:7px;padding-left:9px}",
        "<ul><li>a</li></ul>",
    );
    assert!(!t.unsupported);
    assert_eq!(
        (
            node(&t, "ul").style.margin_top,
            node(&t, "ul").style.margin_bottom,
            node(&t, "ul").style.padding_left
        ),
        (Margin::Px(3), Margin::Px(7), 9)
    );
    // author revert returns the modelled UA value
    let t = ext("ul{margin-top:revert;padding-left:revert}", "<ul><li>a</li></ul>");
    assert!(!t.unsupported);
    assert_eq!(
        (node(&t, "ul").style.margin_top, node(&t, "ul").style.padding_left),
        (Margin::Px(16), 40)
    );
}

// Preregistered test 7: dl-in-ul and ul-in-dl both get zero margins (shared ancestor set).
#[test]
fn dl_and_lists_zero_each_other() {
    let t = ext("", "<ul><li>a<dl><dt>t</dt></dl></li></ul>");
    assert!(!t.unsupported);
    assert_eq!(node(&t, "dl").style.margin_top, Margin::Px(0));
    let t = ext("", "<dl><dd><ul><li>a</li></ul></dd></dl>");
    assert!(!t.unsupported);
    assert_eq!(
        (node(&t, "ul").style.margin_top, node(&t, "ul").style.padding_left),
        (Margin::Px(0), 40)
    );
}

// Preregistered test 8: computed-RTL ul/ol stays refused (mirroring dd); dir attr still refuses.
#[test]
fn computed_rtl_lists_still_refuse() {
    assert!(
        ext("", "<div style='direction:rtl'><ul><li>a</li></ul></div>").unsupported,
        "inherited RTL ul refuses"
    );
    assert!(
        ext("", "<div style='direction:rtl'><ol><li>a</li></ol></div>").unsupported,
        "inherited RTL ol refuses"
    );
    assert!(
        ext("", "<div dir='rtl'><ul><li>a</li></ul></div>").unsupported,
        "dir attr refuses"
    );
}

// Preregistered test 9: default entry unchanged — bare ul/ol keep the pre-change computed values
// (no UA margins/padding supply outside the ext entries).
#[test]
fn default_entry_unchanged() {
    let t = parse_style_tree(
        "<!doctype html><div><ul><li>a</li></ul><ol><li>b</li></ol></div>".as_bytes(),
        &[],
        &Limits::DEFAULT,
    );
    let (ul, ol) = (node(&t, "ul"), node(&t, "ol"));
    assert_eq!(
        (
            ul.style.margin_top,
            ul.style.margin_bottom,
            ul.style.padding_left
        ),
        (Margin::Px(0), Margin::Px(0), 0)
    );
    assert_eq!(
        (ol.style.margin_top, ol.style.padding_left),
        (Margin::Px(0), 0)
    );
}
