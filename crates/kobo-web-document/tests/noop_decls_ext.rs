//! PRIVATE candidate (ext entry only): zero-paint declarations cursor / border-image:none / box-shadow:none.
//! Style-layer equality test: a node styled with them computes identically to the bare node under ext; the default
//! entry refuses them (unmasked); other values refuse even under ext.
use kobo_web_document::{parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget};
fn ext(h: &str) -> kobo_web_document::style_tree::StyleTree {
    parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
}
const BASE: &str = "background-color:#123456;height:20px";
fn page(extra: &str) -> String {
    format!("<!doctype html><div style='{BASE};{extra}'>x</div>")
}
#[test]
fn admitted_values_compute_like_bare_under_ext_and_refuse_by_default() {
    let bare = ext(&page(""));
    assert!(!bare.unsupported);
    let bare_style = bare.nodes.iter().find(|n| n.tag == "div").unwrap().style;
    for d in ["cursor:pointer", "cursor:not-allowed", "cursor:auto", "border-image:none", "box-shadow:none", "cursor:pointer;box-shadow:none;border-image:none"] {
        let t = ext(&page(d));
        assert!(!t.unsupported, "{d}");
        assert_eq!(t.nodes.iter().find(|n| n.tag == "div").unwrap().style, bare_style, "{d}");
        let def = parse_style_tree(page(d).as_bytes(), &[], &Limits::DEFAULT);
        assert!(def.unsupported, "default must refuse {d}");
    }
}
#[test]
fn other_values_still_refuse_under_ext() {
    for d in [
        "cursor:url(a.cur),auto", "cursor:pointer pointer", "cursor:inherit", "cursor:", "cursor:Pointer2", "cursor:var(--x)",
        "border-image:url(a.png) 1", "border-image:none 1", "border-image:initial", "box-shadow:0 0 1px red",
        "box-shadow:none,none", "box-shadow:inset none", "box-shadow:NONE x",
        // FL26: `box-shadow:initial` was refused here before; it is now the typed reset (see tests/box_shadow_hard.rs i2).
    ] {
        assert!(ext(&page(d)).unsupported, "{d}");
    }
}
#[test]
fn every_cursor_keyword_important_and_defined_vars_compute_like_bare() {
    let bare = ext(&page(""));
    let bs = bare.nodes.iter().find(|n| n.tag == "div").unwrap().style;
    for k in ["auto", "default", "pointer", "text", "not-allowed", "move", "grab", "grabbing", "help", "wait", "progress", "crosshair", "none", "context-menu", "cell", "vertical-text", "alias", "copy", "no-drop", "all-scroll", "zoom-in", "zoom-out", "col-resize", "row-resize", "n-resize", "e-resize", "s-resize", "w-resize", "ew-resize", "ns-resize"] {
        let t = ext(&page(&format!("cursor:{k}")));
        assert!(!t.unsupported, "{k}");
        assert_eq!(t.nodes.iter().find(|n| n.tag == "div").unwrap().style, bs, "{k}");
    }
    for d in ["cursor:pointer !important", "box-shadow:none!important", "border-image:none !important"] {
        let t = ext(&page(d));
        assert!(!t.unsupported, "{d}");
        assert_eq!(t.nodes.iter().find(|n| n.tag == "div").unwrap().style, bs, "{d}");
    }
    let v = ext("<!doctype html><style>div{--c:pointer;--s:none}</style><div style='background-color:#123456;height:20px;cursor:var(--c);box-shadow:var(--s);border-image:var(--missing,none)'>x</div>");
    assert!(!v.unsupported, "defined vars / defined fallback");
    assert_eq!(v.nodes.iter().find(|n| n.tag == "div").unwrap().style, bs);
}
#[test]
fn painted_border_is_unaffected_and_competing_unsupported_values_still_refuse() {
    let b = ext(&page("border:3px solid #c00"));
    assert!(!b.unsupported, "anchor: bordered div admitted");
    let bs = b.nodes.iter().find(|n| n.tag == "div").unwrap().style;
    assert_ne!(bs, ext(&page("")).nodes.iter().find(|n| n.tag == "div").unwrap().style, "anchor: border changes style");
    let t = ext(&page("border:3px solid #c00;border-image:none;box-shadow:none;cursor:pointer"));
    assert!(!t.unsupported);
    assert_eq!(t.nodes.iter().find(|n| n.tag == "div").unwrap().style, bs, "border still computed the same");
    for d in ["border-image:none;border-image:url(a.png) 1", "box-shadow:none;box-shadow:0 0 1px red", "cursor:pointer;box-shadow:0 0 2px #000", "border-image:none;border-image-source:url(a.png)"] {
        assert!(ext(&page(d)).unsupported, "{d}");
    }
}
