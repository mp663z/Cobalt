//! U2 (ext entry only): dl/dt/dd required-props audit arms removed after the U1 defaults (UA-origin, Chrome 156 oracle in
//! private-evidence ul1/ua95 + ul1/u1impl). Style-layer admission and computed values only; no pixel/readiness claim.
//! Default entry UNCHANGED (dl/dt/dd stay Inline there). All other audited elements still refuse (`ua_audit_ext`).
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
#[test]
fn bare_dl_dt_dd_are_admitted_under_ext_with_chrome_defaults() {
    let t = ext("", "<dl><dt>a</dt><dd>b</dd></dl>");
    assert!(
        !t.unsupported,
        "bare dl/dt/dd admitted under ext after U1 defaults"
    );
    let (dl, dt, dd) = (node(&t, "dl"), node(&t, "dt"), node(&t, "dd"));
    assert_eq!(
        (
            dl.style.display,
            dl.style.margin_top,
            dl.style.margin_bottom
        ),
        (Display::Block, Margin::Px(16), Margin::Px(16))
    );
    assert_eq!(
        (dt.style.display, dt.style.margin_top),
        (Display::Block, Margin::Px(0))
    );
    assert_eq!(
        (dd.style.display, dd.style.margin_left),
        (Display::Block, Margin::Px(40))
    );
}
#[test]
fn nested_and_outside_and_override_cases() {
    let t = ext("", "<dl><dd><dl><dt>a</dt></dl></dd></dl>");
    assert!(!t.unsupported);
    let dls: Vec<_> = t.nodes.iter().filter(|n| n.tag == "dl").collect();
    assert_eq!(
        (dls[0].style.margin_top, dls[1].style.margin_top),
        (Margin::Px(16), Margin::Px(0))
    );
    assert!(!ext("", "<dd>o</dd><dt>o</dt>").unsupported);
    let t = ext("dl{margin-top:3px}", "<dl><dd>b</dd></dl>");
    assert_eq!(node(&t, "dl").style.margin_top, Margin::Px(3));
    // author revert returns the modelled UA value
    let t = ext(
        "dd{display:revert;margin-left:revert}",
        "<dl><dd>b</dd></dl>",
    );
    assert!(!t.unsupported);
    assert_eq!(
        (
            node(&t, "dd").style.display,
            node(&t, "dd").style.margin_left
        ),
        (Display::Block, Margin::Px(40))
    );
}
#[test]
fn computed_rtl_dd_and_other_u1_guards_still_refuse() {
    assert!(
        ext("", "<div style='direction:rtl'><dl><dd>b</dd></dl></div>").unsupported,
        "inherited RTL dd refuses"
    );
    assert!(
        ext("", "<div dir='rtl'><dl><dd>b</dd></dl></div>").unsupported,
        "dir attr refuses"
    );
    assert!(
        ext(
            "",
            "<div style='writing-mode:vertical-rl'><dl><dd>b</dd></dl></div>"
        )
        .unsupported
    );
    assert!(ext("dl{all:unset}", "<dl><dd>b</dd></dl>").unsupported);
    // dl/dt only under RTL is admitted (no dd): matches the S1 matrix (c24)
    assert!(!ext("", "<div style='direction:rtl'><dl><dt>a</dt></dl></div>").unsupported);
}
#[test]
fn neighbour_audited_elements_still_refuse_and_default_entry_is_unchanged() {
    for body in [
        "<ul><li>t</li></ul>",
        "<ol><li>t</li></ol>",
        "<blockquote>t</blockquote>",
        "<figure>t</figure>",
        "<h3>t</h3>",
        "<pre>t</pre>",
        "<address>t</address>",
        "<cite>t</cite>",
        "<dl><dd><ul><li>x</li></ul></dd></dl>",
    ] {
        assert!(ext("", body).unsupported, "must still refuse: {body}");
    }
    let h = "<!doctype html><div><dl><dt>a</dt><dd>b</dd></dl></div>";
    let t = parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT);
    assert!(!t.unsupported);
    for tag in ["dl", "dt", "dd"] {
        assert_eq!(
            node(&t, tag).style.display,
            Display::Inline,
            "default entry keeps Inline <{tag}>"
        );
    }
    assert_eq!(node(&t, "dd").style.margin_left, Margin::Px(0));
}

// Replacement coverage for the deleted ua_audit_ext dl/dt/dd cases (each previously asserted "missing property => refuse").
// Positive: every former missing-property shape and an author revert of each former required property is now admitted and
// computes the modelled UA value. Neighbour: the same shapes on still-audited elements keep refusing.
#[test]
fn former_missing_property_shapes_are_admitted_per_element() {
    for css in [
        "dl{margin:0}",
        "dl{display:block}",
        "dt{margin-top:2px}",
        "dd{display:block}",
        "dd{margin-left:0}",
        "dl{display:revert}",
        "dl{margin-top:revert;margin-bottom:revert}",
        "dt{display:revert}",
        "dd{display:revert;margin-left:revert}",
    ] {
        let t = ext(css, "<dl><dt>a</dt><dd>b</dd></dl>");
        assert!(!t.unsupported, "admitted: {css}");
    }
    let t = ext(
        "dl{display:revert;margin-top:revert}",
        "<dl><dd>b</dd></dl>",
    );
    assert_eq!(
        (
            node(&t, "dl").style.display,
            node(&t, "dl").style.margin_top
        ),
        (Display::Block, Margin::Px(16))
    );
}
#[test]
fn neighbour_missing_property_shapes_still_refuse() {
    for (css, body) in [
        ("ul{margin:0}", "<ul><li>t</li></ul>"),
        ("ol{padding-left:0}", "<ol><li>t</li></ol>"),
        ("h3{font-size:10px}", "<h3>t</h3>"),
        ("address{display:block}", "<address>t</address>"),
        ("ul{margin-top:revert}", "<ul><li>t</li></ul>"),
        ("blockquote{margin-top:0}", "<blockquote>t</blockquote>"),
    ] {
        assert!(ext(css, body).unsupported, "must still refuse: {css}");
    }
}
