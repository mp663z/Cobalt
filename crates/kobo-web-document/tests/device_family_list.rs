//! PRIVATE candidate (ext entries only): a NAMED `font-family` list is admitted only when
//! `font_family::device_generic` resolves it: at least one named entry, every named entry before
//! the first generic is known-absent on the basic painter (its bundle has three faces and no
//! host lookup), and the first generic is `sans-serif` or `monospace`. Every other list keeps
//! the declaration refusal. Bare generic keywords keep their existing handling.
use kobo_web_document::computed_style::GenericFamily;
use kobo_web_document::font_family::{device_generic, Generic, Refusal};
use kobo_web_document::style_diagnostics::RefusalReason;
use kobo_web_document::{
    parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget,
};

fn ext(h: &str) -> kobo_web_document::style_tree::StyleTree {
    parse_style_tree_with_inline_sheets_and_selectors(
        h.as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    )
}
fn page(value: &str) -> String {
    format!("<!doctype html><style>div{{font-family:{value}}}</style><div>x</div>")
}
fn family(t: &kobo_web_document::style_tree::StyleTree) -> Option<GenericFamily> {
    t.nodes.iter().find(|n| n.tag == "div").unwrap().style.font_family
}
fn css_decl(t: &kobo_web_document::style_tree::StyleTree) -> usize {
    t.diagnostics.count(RefusalReason::CssDeclaration)
}

const PY_SANS: &str = "-apple-system, BlinkMacSystemFont, avenir next, avenir, segoe ui, helvetica neue, helvetica, Cantarell, Ubuntu, roboto, noto, arial, sans-serif";
const PY_MONO: &str = "Menlo, Consolas, Monaco, Liberation Mono, Lucida Console, monospace";

#[test]
fn corpus_lists_are_admitted_and_resolve_to_their_generic() {
    for (v, g) in [
        (PY_SANS, GenericFamily::SansSerif),
        (PY_MONO, GenericFamily::Monospace),
        ("MENLO, Consolas, MONOSPACE", GenericFamily::Monospace),
        ("\"Menlo\", 'Courier New', monospace", GenericFamily::Monospace),
        ("arial, sans-serif", GenericFamily::SansSerif),
        ("menlo, monospace !important", GenericFamily::Monospace),
        // The first generic wins, as in CSS fallback; later entries are never consulted.
        ("menlo, monospace, anything-else", GenericFamily::Monospace),
    ] {
        let t = ext(&page(v));
        assert!(!t.unsupported, "{v}");
        assert_eq!(css_decl(&t), 0, "{v}");
        assert_eq!(family(&t), Some(g), "{v}");
    }
}

#[test]
fn every_other_list_keeps_the_refusal() {
    let sixteen_plus: String = (0..17).map(|i| format!("menlo{i}")).collect::<Vec<_>>().join(", ") + ", monospace";
    let long_name = format!("{}, monospace", "a".repeat(260));
    for v in [
        // a name outside the absent set is undecidable (could be a face the bundle answers to)
        "foo, monospace",
        "menlo, foo, monospace",
        "dejavu sans mono, monospace",
        "atkinson hyperlegible, sans-serif",
        "\"open sans\", sans-serif",
        "\"source code pro\", monospace",
        // no generic reached
        "menlo, consolas",
        "menlo",
        "arial",
        // serif or an unmapped generic as the first honorable entry
        "menlo, serif",
        "helvetica, serif, sans-serif",
        "menlo, cursive, monospace",
        "system-ui, sans-serif",
        "menlo, ui-monospace, monospace",
        "menlo, emoji",
        // grammar: var(), escapes, css-wide keywords, bad quotes, empty entries, size caps
        "var(--m), monospace",
        "menlo, var(--m)",
        "menlo, \\6d onospace",
        "menlo, inherit",
        "menlo, initial, monospace",
        "inherit",
        "menlo, \"consolas, monospace",
        "menlo,, monospace",
        "menlo, monospace,",
        sixteen_plus.as_str(),
        long_name.as_str(),
    ] {
        let t = ext(&page(v));
        assert!(t.unsupported, "must keep refusing {v}");
        // var() and unbalanced quotes are refused by other paths than the declaration parser,
        // so only the property outcome is pinned for them.
        if !v.contains("var(") && v.matches('"').count() % 2 == 0 {
            assert_eq!(css_decl(&t), 1, "{v}");
        }
        assert_eq!(family(&t), None, "{v}");
    }
}

#[test]
fn bare_generics_keep_their_existing_refusal() {
    // No named entry: unchanged, including the retained-but-refused family value.
    for v in ["monospace", "serif", "monospace, monospace", "sans-serif, sans-serif"] {
        let t = ext(&page(v));
        assert!(t.unsupported, "{v}");
        assert_eq!(css_decl(&t), 1, "{v}");
    }
}

#[test]
fn bare_sans_serif_keeps_its_existing_admission() {
    let t = ext(&page("sans-serif"));
    assert!(!t.unsupported);
    assert_eq!(family(&t), Some(GenericFamily::SansSerif));
}

#[test]
fn admission_is_ext_entries_only() {
    for v in [PY_SANS, PY_MONO] {
        let t = parse_style_tree(page(v).as_bytes(), &[], &Limits::DEFAULT);
        assert!(t.unsupported, "non-ext entry must keep refusing {v}");
        assert_eq!(css_decl(&t), 1, "{v}");
    }
}

#[test]
fn resolver_reports_why_it_refuses() {
    assert_eq!(device_generic("menlo, monospace"), Ok(Generic::Monospace));
    assert_eq!(device_generic("arial, sans-serif"), Ok(Generic::SansSerif));
    assert_eq!(device_generic("monospace"), Err(Refusal::Undecidable));
    assert_eq!(device_generic("foo, monospace"), Err(Refusal::Undecidable));
    assert_eq!(device_generic("menlo, serif"), Err(Refusal::Undecidable));
    assert_eq!(device_generic("menlo, cursive"), Err(Refusal::UnsupportedGeneric));
    assert_eq!(device_generic("menlo, consolas"), Err(Refusal::NothingAvailable));
    assert_eq!(device_generic("menlo, inherit"), Err(Refusal::CssWideKeyword));
    assert_eq!(device_generic("var(--m), monospace"), Err(Refusal::Syntax));
}

#[test]
fn admitted_style_differs_only_in_family() {
    let a = ext(&page(PY_MONO));
    let b = ext(&page("monospace, monospace")); // refused: family stays unset
    let (sa, sb) = (
        a.nodes.iter().find(|n| n.tag == "div").unwrap().style,
        b.nodes.iter().find(|n| n.tag == "div").unwrap().style,
    );
    let mut sa = sa;
    sa.font_family = sb.font_family;
    sa.font_family_list = sb.font_family_list;
    assert_eq!(sa, sb);
}
