//! PRIVATE candidate (ext entries only): exactly generic `font-family: sans-serif`
//! drops the declaration refusal under the ext entries; every other value keeps it.
//! Retention is not painting: the family-bound painter with proven face hashes is
//! unchanged and remains the only paint path for retained families.
use kobo_web_document::computed_style::GenericFamily;
use kobo_web_document::style_diagnostics::RefusalReason;
use kobo_web_document::{
    parse_style_tree, parse_style_tree_with_inline_sheets,
    parse_style_tree_with_inline_sheets_and_selectors, parse_style_tree_with_media,
    parse_style_tree_with_pinned_font_family, parse_style_tree_with_state,
    parse_style_tree_with_supply, parse_style_tree_with_text_decoration, Limits, SupplyBudget,
};
use kobo_web_document::{HistoryState, InputState, SnapshotState};
const SNAPSHOT: SnapshotState = SnapshotState {
    history: Some(HistoryState::FreshNoHistory),
    input: Some(InputState::QuiescentCapture),
    focus: None,
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
fn css_decl(t: &kobo_web_document::style_tree::StyleTree) -> usize {
    t.diagnostics.count(RefusalReason::CssDeclaration)
}
const BASE: &str = "background-color:#123456;height:20px";
fn page(extra: &str) -> String {
    format!("<!doctype html><div style='{BASE};{extra}'>x</div>")
}
fn sheet_page(rule: &str) -> String {
    format!("<!doctype html><style>{rule}</style><div>x</div>")
}

#[test]
fn exact_generic_sans_serif_is_admitted_under_ext() {
    for v in [
        "sans-serif",
        "SANS-SERIF",
        " sans-serif  ",
        "sans-serif !important",
        "Sans-Serif !IMPORTANT",
    ] {
        let t = ext(&page(&format!("font-family:{v}")));
        assert!(!t.unsupported, "inline {v}");
        assert_eq!(css_decl(&t), 0, "inline {v}");
        let st = t.nodes.iter().find(|n| n.tag == "div").unwrap().style;
        assert_eq!(st.font_family, Some(GenericFamily::SansSerif), "inline {v}");
        let s = ext(&sheet_page(&format!("div{{font-family:{v}}}")));
        assert!(!s.unsupported, "sheet {v}");
        assert_eq!(css_decl(&s), 0, "sheet {v}");
        assert_eq!(
            s.nodes
                .iter()
                .find(|n| n.tag == "div")
                .unwrap()
                .style
                .font_family,
            Some(GenericFamily::SansSerif),
            "sheet {v}"
        );
    }
}

#[test]
fn admission_is_ext_entries_only() {
    let h = page("font-family:sans-serif");
    for t in [
        parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT),
        parse_style_tree_with_media(
            h.as_bytes(),
            &[],
            &Limits::DEFAULT,
            kobo_web_document::MediaContext::new(800).unwrap(),
        ),
        parse_style_tree_with_supply(
            h.as_bytes(),
            &[],
            &Limits::DEFAULT,
            None,
            SupplyBudget::CANONICAL,
        ),
        parse_style_tree_with_inline_sheets(
            h.as_bytes(),
            &[],
            &Limits::DEFAULT,
            None,
            SupplyBudget::CANONICAL,
        ),
        parse_style_tree_with_text_decoration(h.as_bytes(), &[], &Limits::DEFAULT),
    ] {
        assert!(t.unsupported, "non-ext entry must keep refusing");
        assert_eq!(
            css_decl(&t),
            1,
            "one refusal event for the single declaration"
        );
    }
}

#[test]
fn other_values_keep_the_refusal_under_ext() {
    for v in [
        // named families (no catalog, all Unknown)
        "arial",
        "\"gds transport\", arial, sans-serif",
        // multi-item lists, including repeated generic
        "sans-serif, sans-serif",
        "monospace, monospace",
        "arial, sans-serif",
        // other generics (no proven profile)
        "serif",
        "monospace",
        // global keywords
        "inherit",
        "initial",
        "unset",
        "revert",
        // near-miss spellings
        "sansserif",
        "sans-",
        "\"sans-serif\"",
        "sans-serif,",
        "sans-serif x",
    ] {
        let t = ext(&page(&format!("font-family:{v}")));
        assert!(t.unsupported, "ext must keep refusing {v}");
        assert_eq!(css_decl(&t), 1, "{v}");
    }
}

#[test]
fn pinned_lane_is_unchanged() {
    // The pinned lists still resolve through the pinned arm only.
    let pinned_list = "\"gds transport\", arial, sans-serif";
    let p = parse_style_tree_with_pinned_font_family(
        sheet_page(&format!("div{{font-family:{pinned_list}}}")).as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    );
    assert!(
        !p.unsupported,
        "pinned list still admitted under the pinned entry"
    );
    // Pre-existing: the pinned arm's first pinned list IS exactly `sans-serif`,
    // so the pinned entry already admitted it before this change - unchanged.
    let g = parse_style_tree_with_pinned_font_family(
        sheet_page("div{font-family:sans-serif}").as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    );
    assert!(
        !g.unsupported,
        "pinned entry already admitted bare sans-serif (pinned arm, pre-existing)"
    );
    // Under ext the GDS list stays refused (named heads are Unknown).
    assert!(ext(&sheet_page(&format!("div{{font-family:{pinned_list}}}"))).unsupported);
    // The pinned entry keeps refusing a non-pinned named list.
    let n = parse_style_tree_with_pinned_font_family(
        sheet_page("div{font-family:arial}").as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    );
    assert!(
        n.unsupported,
        "pinned entry keeps refusing non-pinned named list"
    );
}

#[test]
fn admitted_computed_style_differs_only_in_family() {
    let bare = ext(&page(""));
    let bare_style = bare.nodes.iter().find(|n| n.tag == "div").unwrap().style;
    let t = ext(&page("font-family:sans-serif"));
    let st = t.nodes.iter().find(|n| n.tag == "div").unwrap().style;
    assert_eq!(
        kobo_web_document::computed_style::Computed {
            font_family: None,
            font_family_list: None,
            ..st
        },
        bare_style,
        "only the family fields change"
    );
    assert!(
        st.font_family_list.is_some(),
        "retained interned list key present"
    );
}

#[test]
fn with_state_shares_the_ext_gate() {
    // with_state passes the same ext_selectors=true flag (lib.rs entry calls
    // parse_style_tree_in(.., true, true, false, false, Some(snapshot))).
    let pos = parse_style_tree_with_state(
        sheet_page("div{font-family:sans-serif}").as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
        &SNAPSHOT,
    );
    assert!(
        !pos.unsupported,
        "with_state admits exact generic sans-serif"
    );
    assert_eq!(css_decl(&pos), 0);
    assert_eq!(
        pos.nodes
            .iter()
            .find(|n| n.tag == "div")
            .unwrap()
            .style
            .font_family,
        Some(GenericFamily::SansSerif)
    );
    let neg = parse_style_tree_with_state(
        sheet_page("div{font-family:arial}").as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
        &SNAPSHOT,
    );
    assert!(neg.unsupported, "with_state keeps refusing named families");
    assert_eq!(css_decl(&neg), 1);
}

#[test]
fn var_produced_sans_serif_follows_the_same_flag() {
    // Scope (explicit, corpus has no such case): a var() whose whole
    // substituted value is exactly `sans-serif` is admitted under ext, exactly
    // like the literal keyword - the var-substitution path passes the same
    // ext_selectors flag into properties_with (style_tree.rs:839).
    let doc = |v: &str| {
        format!("<!doctype html><style>div{{--f:{v}}}</style><div style='background-color:#123456;height:20px;font-family:var(--f)'>x</div>")
    };
    let t = ext(&doc("sans-serif"));
    assert!(!t.unsupported, "var-produced sans-serif admitted under ext");
    assert_eq!(css_decl(&t), 0);
    assert_eq!(
        t.nodes
            .iter()
            .find(|n| n.tag == "div")
            .unwrap()
            .style
            .font_family,
        Some(GenericFamily::SansSerif)
    );
    for v in ["serif", "arial", "sans-serif, sans-serif"] {
        assert!(
            ext(&doc(v)).unsupported,
            "var-produced {v} still refused under ext"
        );
    }
    let d = parse_style_tree(doc("sans-serif").as_bytes(), &[], &Limits::DEFAULT);
    assert!(
        d.unsupported,
        "var-produced sans-serif still refused by default"
    );
}
