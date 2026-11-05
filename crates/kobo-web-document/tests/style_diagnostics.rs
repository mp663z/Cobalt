use kobo_web_document::{parse_style_tree, style_diagnostics::RefusalReason as R, Limits};
fn parse(html: &str) -> kobo_web_document::style_tree::StyleTree {
    parse_style_tree(
        format!("<!doctype html>{html}").as_bytes(),
        &[],
        &Limits::DEFAULT,
    )
}
#[test]
fn clean_parse_has_no_refusal_and_exact_counts() {
    let t = parse("<style>p{color:blue}</style><p>ab</p>");
    let d = &t.diagnostics;
    assert_eq!(d.first_reason(), None);
    assert_eq!(d.expanded_declarations, 1);
    assert_eq!(d.retained_nodes, t.nodes.len());
    assert!(d.dom_nodes >= d.retained_nodes);
    assert!(d.match_attempts > 0);
    for reason in R::ALL {
        assert_eq!(d.count(reason), 0);
    }
    assert!(!t.unsupported && !t.truncated);
}
#[test]
fn attribute_loss_precedes_missing_resource_and_records_both() {
    let t = parse(&format!(
        "<link rel='stylesheet' href='data:text/css;base64,{}'><p>x</p>",
        "A".repeat(2100)
    ));
    assert_eq!(t.diagnostics.first_reason(), Some(R::DroppedAttributes));
    assert_eq!(t.diagnostics.dropped_attributes, 1);
    assert_eq!(t.diagnostics.count(R::MissingStylesheet), 1);
    assert_eq!(t.diagnostics.expanded_declarations, 0);
    assert!(t.unsupported && t.truncated);
}
#[test]
fn input_bytes_and_text_caps_are_distinct() {
    let raw = b"<!doctype html><p>abcdef</p>";
    let mut limits = Limits::DEFAULT;
    limits.max_input_bytes = 20;
    let t = parse_style_tree(raw, &[], &limits);
    assert_eq!(t.diagnostics.first_reason(), Some(R::InputBytes));
    assert_eq!(t.diagnostics.input_bytes, raw.len());
    assert!(t.truncated);
    limits = Limits::DEFAULT;
    limits.max_text_bytes = 2;
    let t = parse_style_tree(raw, &[], &limits);
    assert_eq!(t.diagnostics.first_reason(), Some(R::DroppedText));
    assert!(t.diagnostics.dropped_text && t.truncated);
    limits = Limits::DEFAULT;
    limits.max_nodes = 3;
    let t = parse_style_tree(raw, &[], &limits);
    assert_eq!(t.diagnostics.count(R::DomNodes), 1);
    assert!(t.truncated);
}
#[test]
fn stylesheet_semantics_are_counted_without_storing_external_text() {
    for (html, reason) in [
        (
            "<link rel=stylesheet media='(min-width:10px)' href=x>",
            R::StylesheetMedia,
        ),
        ("<style>@unknown x;p{color:red}</style>", R::CssAtRule),
        (
            "<style>@media (width:1px){p{color:red}}</style>",
            R::CssCondition,
        ),
        ("<style>p:hover{color:red}</style>", R::CssSelector),
        ("<style>p{width:nope}</style>", R::CssDeclaration),
        ("<p style='width:nope'>x</p>", R::CssDeclaration),
        ("<style>p{</style>", R::CssMalformed),
    ] {
        let t = parse(html);
        assert_eq!(t.diagnostics.count(reason), 1, "{html}");
        assert!(t.unsupported);
    }
}
#[test]
fn stylesheet_count_and_utf8_have_specific_reasons() {
    let html=b"<!doctype html><link rel=stylesheet href=a><link rel=stylesheet href=b><link rel=stylesheet href=c><p>x</p>";
    let t = parse_style_tree(
        html,
        &[
            b"p{color:red}".to_vec(),
            b"p{color:blue}".to_vec(),
            b"p{color:green}".to_vec(),
        ],
        &Limits::DEFAULT,
    );
    assert_eq!(t.diagnostics.count(R::StylesheetCount), 1);
    assert!(t.unsupported);
    let t = parse_style_tree(
        b"<!doctype html><link rel=stylesheet href=a>",
        &[vec![255]],
        &Limits::DEFAULT,
    );
    assert_eq!(t.diagnostics.count(R::StylesheetEncoding), 1);
    assert!(t.unsupported);
}
#[test]
fn style_complexity_budget_reason_does_not_claim_dom_truncation() {
    let t = parse(&format!(
        "<style>{}</style><p>x</p>",
        "p{color:red}".repeat(4097)
    ));
    assert_eq!(t.diagnostics.count(R::CssScanBudget), 1);
    assert!(t.unsupported);
    assert_eq!(t.diagnostics.dropped_attributes, 0);
    let t = parse(&format!(
        "<style>{}</style><p>x</p>",
        "p{color:red;color:blue}".repeat(2048)
    ));
    assert_eq!(t.diagnostics.count(R::ExpandedRules), 1);
    assert!(t.unsupported);
    let t = parse(&format!(
        "<style>{}</style><p>x</p>",
        "p{color:red}".repeat(256)
    ));
    assert_eq!(t.diagnostics.count(R::MatchedDeclarations), 1);
    assert!(t.truncated);
}
#[test]
fn quirks_and_legacy_encoding_are_not_silent() {
    let t = parse_style_tree(b"<p>x</p>", &[], &Limits::DEFAULT);
    assert_eq!(t.diagnostics.first_reason(), Some(R::Quirks));
    assert!(t.quirks);
    let t = parse_style_tree(b"<!doctype html><p>\xff</p>", &[], &Limits::DEFAULT);
    assert_eq!(t.diagnostics.first_reason(), Some(R::Encoding));
    assert!(t.unsupported);
}
