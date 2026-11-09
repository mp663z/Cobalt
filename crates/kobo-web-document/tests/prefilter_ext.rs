//! Ext-only rightmost-key match prefilter (bounded conditional approval
//! 2026-11-09 design review, corrections D1-D3 + CV1-6): candidate subsetting
//! must be observationally identical to the flat scan (same matched set, same
//! source-order test sequence, same cap trip points); the ONLY intended delta
//! is fewer charged match_work units in the ext entry. Default entry never
//! builds or consults the index.
use kobo_web_document::{
    parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors as ext,
    style_diagnostics::RefusalReason as R, style_tree::StyleTree, Limits, SupplyBudget,
};

fn parse(html: &str) -> StyleTree {
    ext(
        html.as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    )
}
fn color_of(t: &StyleTree, tag: &str) -> u32 {
    t.nodes.iter().find(|n| n.tag == tag).unwrap().style.color
}

// D1: duplicate class tokens on the element produce ONE lookup, ONE charge,
// ONE hit. html/body/head carry no class `a`, so exactly one candidate test
// happens in the whole walk.
#[test]
fn duplicate_class_tokens_single_charge() {
    let t = parse("<style>.a{color:rgb(1,2,3)}</style><p class=\"a a\">x</p>");
    assert_eq!(color_of(&t, "p"), 0x010203);
    assert_eq!(t.diagnostics.match_attempts, 1);
    assert!(!t.truncated);
}

// D2/CV4: ext tag comparison folds ASCII case (namespace-agnostic local name),
// so the TAG bucket key must fold identically - including SVG camelCase.
#[test]
fn svg_camelcase_tag_key_folds() {
    let t = parse(
        "<style>lineargradient{color:rgb(4,5,6)}</style><svg><linearGradient id=g></linearGradient></svg>",
    );
    assert_eq!(color_of(&t, "linearGradient"), 0x040506);
}

// D2/CV1+CV2: attribute names compare ASCII case-insensitively ALWAYS (also
// on SVG), so ATTR bucket keys fold identically; a missing attribute is false
// for ALL 7 ops (empty-value operators included), and empty `want` is false
// even when the attribute is present.
#[test]
fn attr_key_fold_and_missing_attr_ops() {
    let t = parse(
        "<style>[viewbox]{color:rgb(7,8,9)}[data-z$=\"\"]{color:rgb(9,9,9)}[data-z*=\"\"]{color:rgb(8,8,8)}[data-z^=\"\"]{color:rgb(7,7,7)}</style>\
         <svg viewBox=\"0 0 1 1\"></svg><p>a</p><q data-z=\"\">b</q>",
    );
    assert_eq!(color_of(&t, "svg"), 0x070809);
    // p has no data-z: never a candidate for the three data-z rules.
    assert_eq!(color_of(&t, "p"), 0);
    // q HAS data-z (empty value): candidate, but every empty-`want` op is false.
    assert_eq!(color_of(&t, "q"), 0);
    // Charges: svg tests [viewbox] only (no data-z attr) = 1; p = 0; q tests
    // all three data-z rules = 3.
    assert_eq!(t.diagnostics.match_attempts, 4);
}

// D3: the matcher takes no quirks input - class tokens stay case-sensitive in
// a quirks document, so class keys must not fold either.
#[test]
fn quirks_document_class_stays_case_sensitive() {
    let t = parse("<style>.X{color:rgb(1,2,3)}</style><p class=\"x\">a</p><p class=\"X\">b</p>");
    assert!(t.quirks, "no doctype should land in quirks mode");
    let mut it = t.nodes.iter().filter(|n| n.tag == "p");
    assert_eq!(it.next().unwrap().style.color, 0);
    assert_eq!(it.next().unwrap().style.color, 0x010203);
}

// Cap order: 300 single-declaration rules all matching one element exceed
// MAX_DECLARATIONS_PER_ELEMENT (256); the walk breaks at that element. The
// DEFAULT entry (flat scan) and the EXT entry (indexed candidates) must
// truncate at the SAME walk position: same retained tag sequence, same
// refusal, same truncated flag.
#[test]
fn declaration_cap_trips_at_same_position_both_entries() {
    let mut css = String::new();
    for i in 0..300 {
        css.push_str(&format!(".k{{color:rgb({i},0,0)}}"));
    }
    let html = format!("<style>{css}</style><p class='k'>x</p><i>after</i>");
    let d = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
    let e = parse(&html);
    let tags = |t: &StyleTree| t.nodes.iter().map(|n| n.tag.clone()).collect::<Vec<_>>();
    assert!(d.truncated && e.truncated);
    assert!(d.diagnostics.count(R::MatchedDeclarations) >= 1);
    assert!(e.diagnostics.count(R::MatchedDeclarations) >= 1);
    assert_eq!(tags(&d), tags(&e));
}

// Cascade source order through the index: 255 rules (under the cap) all
// matching one element - the LAST one in source order must win.
#[test]
fn cascade_source_order_preserved_through_index() {
    let mut css = String::new();
    for i in 0..255 {
        css.push_str(&format!(".k{{color:rgb({i},0,0)}}"));
    }
    let t = parse(&format!("<style>{css}</style><p class='k'>x</p>"));
    assert!(!t.truncated);
    assert_eq!(color_of(&t, "p"), 254 << 16);
}

// Reachability: universal-keyed selectors (bare :first-child rightmost
// compound) cannot be filtered, so the 4M MatchWork budget still trips on a
// pathological sheet. `p + p :first-child` never matches (so the declaration
// cap stays out of the way) but still costs one charge per rule per element.
#[test]
fn matchwork_refusal_stays_reachable() {
    let mut css = String::new();
    for _ in 0..4096 {
        css.push_str("p+p :first-child{color:rgb(1,2,3)}");
    }
    let mut body = String::new();
    for _ in 0..1100 {
        body.push_str("<p>x</p>");
    }
    let t = parse(&format!("<style>{css}</style>{body}"));
    assert!(t.truncated);
    assert!(t.diagnostics.count(R::MatchWork) >= 1);
    assert!(t.nodes.len() < 2206, "walk must not complete");
}

// Default entry: no index, identical flat-scan admission (guard: the same
// document parses untruncated and unmatched-by-class as before).
#[test]
fn default_entry_unchanged() {
    let t = parse_style_tree(
        b"<style>.a{color:rgb(1,2,3)}</style><p class=\"a a\">x</p>",
        &[],
        &Limits::DEFAULT,
    );
    // Base grammar supports .a; duplicate tokens do not change the result.
    assert_eq!(
        t.nodes.iter().find(|n| n.tag == "p").unwrap().style.color,
        0x010203
    );
    assert!(!t.truncated);
}

// B1 (review): class CANDIDATE keys split on Unicode whitespace, a superset of
// the ext matcher's ASCII-only split. Element class "a\u{00A0}b" therefore
// still nominates a `.b` rule as a candidate (match_attempts==1), and the
// matcher - ASCII split in the ext grammar, Unicode split in the base grammar
// - owns the match decision either way. With ASCII-only keys the token
// "a\u{00A0}b" would never nominate `.b`, a false negative for any
// Unicode-splitting rule interpretation.
#[test]
fn unicode_ws_class_token_still_candidate() {
    let t = parse("<style>.b{color:rgb(1,2,3)}</style><p class=\"a\u{00A0}b\">x</p>");
    // Candidate inclusion happened (charged once)...
    assert_eq!(t.diagnostics.match_attempts, 1);
    // ...and the ext matcher (ASCII split) decides: "a\u{00A0}b" != "b".
    assert_eq!(color_of(&t, "p"), 0);
    assert!(!t.truncated);
}

// B1 grammar alignment (review): a class literal can NEVER contain a Unicode
// whitespace byte - take_ident admits only [A-Za-z0-9_-] and stops at the
// first 0xC2 of a UTF-8 NBSP, so both grammars REFUSE such a selector
// (parse error, rule never enrolled). Therefore the whole-ASCII-token case
// has no reachable rule key today; the union keys in ext_element_keys keep it
// covered by construction anyway.
#[test]
fn unicode_ws_class_literal_refused_by_both_grammars() {
    let sel = ".a\u{00A0}b{color:rgb(1,2,3)}";
    let t = parse(&format!(
        "<style>{sel}</style><p class=\"a\u{00A0}b\">x</p>"
    ));
    // Rule refused at parse: CssSelector recorded, no match, no candidate charge.
    assert!(t.diagnostics.count(R::CssSelector) >= 1);
    assert_eq!(t.diagnostics.match_attempts, 0);
    assert_eq!(color_of(&t, "p"), 0);
}

// B1 base-grammar control: through the DEFAULT entry (no index, base matcher
// Unicode split), class="a\u{00A0}b" DOES match .b - the semantics the union
// keys protect against false negatives for, and proof the default path is
// untouched by the prefilter.
#[test]
fn base_grammar_unicode_ws_class_matches() {
    let t = parse_style_tree(
        "<style>.b{color:rgb(1,2,3)}</style><p class=\"a\u{00A0}b\">x</p>".as_bytes(),
        &[],
        &Limits::DEFAULT,
    );
    assert_eq!(
        t.nodes.iter().find(|n| n.tag == "p").unwrap().style.color,
        0x010203
    );
    assert!(!t.truncated);
}
