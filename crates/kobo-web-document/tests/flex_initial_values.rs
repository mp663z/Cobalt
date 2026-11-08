//! PRIVATE candidate (ext entry only): `flex-direction: row` and `flex-wrap: nowrap` (initial values) are no-ops.
//! Background-fill rectangles only (css_background::paint_backgrounds); no text, no real-glyph claim.
use kobo_web_document::box_tree::BoxTree;
use kobo_web_document::{parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget};
fn ext(css: &str, body: &str) -> kobo_web_document::style_tree::StyleTree {
    let h = format!("<!doctype html><style>body{{margin:0}}.t{{font-size:16px;line-height:16px;background:#eee;width:60px}}{css}</style>{body}");
    parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
}
fn fills(css: &str, body: &str) -> Option<Vec<(i32, i32, u32, u32)>> {
    let s = ext(css, body);
    if s.unsupported { return None; }
    let bt = BoxTree::from_style(&s);
    if bt.unsupported { return None; }
    let list = kobo_web_document::css_background::paint_backgrounds(&bt, 200, 100).ok()?;
    Some(list.commands().iter().filter_map(|c| if let kobo_web_document::display_list::Command::Fill { rect, .. } = c { Some((rect.x, rect.y, rect.width, rect.height)) } else { None }).collect())
}
#[test]
fn initial_values_lay_out_exactly_like_bare_on_flex_and_non_flex() {
    let flex = "<div class=t style='display:flex'><div style='width:20px;height:10px;background:#c00'></div><div style='width:30px;height:10px;background:#0c0'></div></div>";
    let bare = fills("", flex).expect("bare flex row admitted");
    assert!(bare.iter().any(|f| f.2 == 20 && f.0 == 0) && bare.iter().any(|f| f.2 == 30 && f.0 == 20), "anchor: row placement {bare:?}");
    for css in [".t{flex-direction:row}", ".t{flex-wrap:nowrap}", ".t{flex-direction:row;flex-wrap:nowrap}"] {
        assert_eq!(fills(css, flex).as_ref(), Some(&bare), "{css}");
    }
    let plain = "<div class=t style='height:10px'></div>";
    let b2 = fills("", plain).expect("bare plain");
    assert!(!b2.is_empty());
    assert_eq!(fills(".t{flex-direction:row;flex-wrap:nowrap}", plain).as_ref(), Some(&b2));
}
#[test]
fn other_values_and_default_entry_refuse() {
    for d in ["flex-direction:column", "flex-direction:row-reverse", "flex-direction:column-reverse", "flex-direction:inherit", "flex-direction:", "flex-wrap:wrap", "flex-wrap:wrap-reverse", "flex-wrap:initial", "flex-flow:row nowrap", "flex-direction:row x"] {
        assert!(ext(&format!(".t{{{d}}}"), "<div class=t>a</div>").unsupported, "{d}");
    }
    for d in ["flex-direction:row", "flex-wrap:nowrap"] {
        let h = format!("<!doctype html><style>.t{{{d}}}</style><div class=t>a</div>");
        assert!(parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported, "default {d}");
    }
}
