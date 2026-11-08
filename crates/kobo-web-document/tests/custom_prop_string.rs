//! PRIVATE, unadopted candidate: simple quoted strings in custom property DEFINITIONS, ext entry only.
use kobo_web_document::{
    parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, parse_style_tree_with_media,
    parse_style_tree_with_pinned_font_family, Limits, MediaContext, SupplyBudget,
};
fn page(css: &str) -> String { format!("<!doctype html><style>{css}</style><div class=a><p>x</p></div>") }
fn ext(css: &str) -> kobo_web_document::style_tree::StyleTree {
    parse_style_tree_with_inline_sheets_and_selectors(page(css).as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
}
fn dump(t: &kobo_web_document::style_tree::StyleTree) -> String {
    t.nodes.iter().map(|n| format!("{}:{:?}", n.tag, n.style)).collect::<Vec<_>>().join("|")
}

#[test]
fn simple_string_definitions_change_no_computed_style() {
    let base = ext(".a{margin:3px}");
    for css in [
        ":root{--v:\"6.1.0\"}.a{margin:3px}",
        ":root{--e:\"\"}.a{margin:3px}",
        ":root{--s:'a b'}.a{margin:3px}",
        ".a{--k:\"x-y_z/1:2+3#4%5@6~7=8\";margin:3px}",
        ":root{--l:1px \"a\" 2px}.a{margin:3px}",
    ] {
        let t = ext(css);
        assert!(!t.unsupported, "{css}");
        assert_eq!(dump(&t), dump(&base), "{css}");
    }
}

#[test]
fn unsafe_or_malformed_strings_stay_refused() {
    for css in [
        ":root{--v:\"unterminated}",
        ":root{--v:\"a'b\"}",
        ":root{--v:'a\"b'}",
        ":root{--v:\"a\\\\b\"}",
        ":root{--v:\"a(b\"}",
        ":root{--v:\"a)b\"}",
        ":root{--v:\"a,b\"}",
        ":root{--v:\"a;b\"}",
        ":root{--v:\"a{b\"}",
        ":root{--v:\"a!b\"}",
        ":root{--v:\"a*b\"}",
        ":root{--v:\"caf\u{e9}\"}",
        ":root{--v:\"a\" url(x)}",
        ":root{--b:1px;--v:\"a\" var(--b)}",
        ":root{--v:var(--b,\"a\")}",
        ":root{--v:\"a\"\"b\" ( }",
    ] {
        assert!(ext(css).unsupported, "{css}");
    }
    let long = format!(":root{{--v:\"{}\"}}", "a".repeat(1100));
    assert!(ext(&long).unsupported);
}

#[test]
fn substitution_of_a_string_value_is_parsed_by_the_receiving_property() {
    // ::before is itself refused, so only a recognised ::after proves the consumer path.
    let lit = ext(".a::after{content:\"\"}.a{margin:3px}");
    let var = ext(":root{--e:\"\"}.a::after{content:var(--e)}.a{margin:3px}");
    assert!(!lit.unsupported, "literal empty content must be admitted");
    // ::after content takes no var() at all (pseudo_after.rs policy): even a non-string value refuses,
    // so the empty-string var() consumer path is NOT reachable; this documents refusal, not acceptance.
    assert!(var.unsupported, "var() into ::after content stays refused");
    assert!(ext(":root{--n:normal}.a::after{content:var(--n)}.a{margin:3px}").unsupported);
    assert_eq!(lit.nodes.iter().filter(|n| n.tag == "::after").count(), 1);
    // Non-empty string on the same accepted base: refused with literal and with var().
    assert!(ext(".a::after{content:\"x\"}.a{margin:3px}").unsupported);
    assert!(ext(":root{--s:\"x\"}.a::after{content:var(--s)}.a{margin:3px}").unsupported);
    // Other consumers: each refused, and the same property with the plain literal is also refused.
    for (prop, lit) in [("margin", "\"x\""), ("color", "\"x\""), ("font-family", "\"x\"")] {
        let v = ext(&format!(":root{{--s:\"x\"}}.a{{{prop}:var(--s)}}"));
        assert!(v.unsupported, "{prop}");
        if prop != "font-family" {
            assert!(ext(&format!(".a{{{prop}:{lit}}}")).unsupported, "{prop} literal");
        }
    }
}

#[test]
fn other_entries_keep_refusing_string_definitions() {
    let h = page(":root{--v:\"6.1.0\"}");
    assert!(parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported);
    assert!(parse_style_tree_with_pinned_font_family(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL).unsupported);
    assert!(parse_style_tree_with_media(h.as_bytes(), &[], &Limits::DEFAULT, MediaContext::new(785).unwrap()).unsupported);
}
