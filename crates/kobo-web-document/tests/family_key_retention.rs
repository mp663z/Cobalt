//! FL50 increment 1: interned family-list handles are retained, inherited, tree-owned, and unconsumed.
use kobo_web_document::{
    computed_style::GenericFamily, parse_style_tree, parse_style_tree_with_pinned_font_family, Limits, SupplyBudget,
};
fn styled(css: &str, body: &str) -> kobo_web_document::style_tree::StyleTree {
    parse_style_tree(format!("<!doctype html><style>{css}</style>{body}").as_bytes(), &[], &Limits::DEFAULT)
}
const BODY: &str = "<section><p>words</p></section>";
fn words_key(s: &kobo_web_document::style_tree::StyleTree) -> Vec<Option<kobo_web_document::font_family::FamilyKey>> {
    s.nodes.iter().filter(|n| n.text.as_deref() == Some("words")).map(|n| n.style.font_family_list).collect()
}

#[test]
fn list_is_retained_inherited_and_resolves_in_owning_tree_only() {
    let a = styled("section{font-family:SERIF}", BODY);
    let b = styled("section{font-family:monospace}", BODY);
    let (ka, kb) = (words_key(&a), words_key(&b));
    assert!(ka.iter().all(|k| k.is_some()) && !ka.is_empty());
    let (fa, fb) = (a.families.as_ref().unwrap(), b.families.as_ref().unwrap());
    assert_eq!(fa.get(ka[0].unwrap()), Some("serif")); // lowercased by the declaration layer
    assert_eq!(fb.get(kb[0].unwrap()), Some("monospace"));
    // keys never resolve against another tree's view, even at the same index
    assert_eq!(fa.get(kb[0].unwrap()), None);
    assert_eq!(fb.get(ka[0].unwrap()), None);
    // clone of the view resolves the same keys
    assert_eq!(fa.clone().get(ka[0].unwrap()), Some("serif"));
    // the render stays refused and the generic output is unchanged
    assert!(a.unsupported);
    assert!(a.nodes.iter().any(|n| n.style.font_family == Some(GenericFamily::Serif)));
}

#[test]
fn keyword_reset_and_unrecognized_lists_carry_no_key_and_no_new_acceptance() {
    let r = styled("section{font-family:serif} p{font-family:initial}", BODY);
    assert!(r.unsupported);
    assert_eq!(words_key(&r), vec![None]);
    // a list that never produced a FontFamily value still does not (no new acceptance, no key)
    let u = styled("section{font-family:\"Foo\", serif}", BODY);
    assert!(u.unsupported);
    assert_eq!(words_key(&u), vec![None]);
    // no declaration: no key
    let n = styled("", BODY);
    assert!(!n.unsupported);
    assert_eq!(words_key(&n), vec![None]);
}

#[test]
fn var_deferred_path_interns_the_substituted_list_in_the_same_tree() {
    let v = styled("section{--f:serif;font-family:var(--f)}", BODY);
    let k = words_key(&v);
    let key = k[0].expect("var() substituted list retained");
    assert_eq!(v.families.as_ref().unwrap().get(key), Some("serif"));
    let direct = styled("section{font-family:serif}", BODY);
    assert!(v.unsupported && direct.unsupported);
    // same text in a different tree: equal text, but keys are not interchangeable
    assert_eq!(v.families.as_ref().unwrap().get(words_key(&direct)[0].unwrap()), None);
}

#[test]
fn pinned_entry_retains_key_for_both_pinned_lists_with_unchanged_admission() {
    for list in ["sans-serif", "\"GDS Transport\", arial, sans-serif"] {
        let html = format!("<!doctype html><style>section{{font-family:{list}}}</style>{BODY}");
        let s = parse_style_tree_with_pinned_font_family(html.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
        assert!(!s.unsupported, "{list}");
        let k = words_key(&s)[0].unwrap();
        let got = s.families.as_ref().unwrap().get(k).unwrap();
        assert_eq!(got, list.to_ascii_lowercase());
        assert!(s.nodes.iter().any(|n| n.style.font_family == Some(GenericFamily::SansSerif)));
    }
}
