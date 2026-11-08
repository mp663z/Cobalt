//! PRIVATE candidate (ext entry only), FL16 fix after reviewer production FAIL: `min-height:0|0px|auto` must emit an explicit
//! MinHeight(Auto) reset so cascade order, specificity, importance, inline style and resolved var() behave like any
//! other property. Section holds a 20px child: reset => used height 20 (aside@20); retained 50px => 50 (aside@50).
//! Expectations follow CSS cascade rules; engine used-height seam only, no Chrome/stored-pixel comparison.
use kobo_web_document::box_tree::{BoxTree, UsedHeightPass, VerticalPass};
use kobo_web_document::{parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget};
fn aside_y(head_css: &str, section_attrs: &str) -> Option<(u32, i64)> {
    let h = format!("<!doctype html><style>body{{margin:0}}div,section,article,aside{{margin:0;padding:0}}{head_css}</style><div><section {section_attrs}><article style='height:20px'></article></section><aside style='height:7px'></aside></div>");
    let t = parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    if t.unsupported { return None; }
    let bt = BoxTree::from_style(&t);
    if bt.unsupported { return None; }
    let uh = UsedHeightPass::from_boxes(&bt, 800, 600);
    let v = VerticalPass::from_boxes(&bt, 800, 600);
    if uh.unsupported || v.unsupported { return None; }
    let pos = |tag: &str| bt.boxes.iter().position(|b| b.text.is_none() && b.source.is_some_and(|s| t.nodes[s].tag == tag)).unwrap();
    let (s, a) = (pos("section"), pos("aside"));
    Some((uh.heights[s]?, v.content_y[a]? - v.content_y[pos("div")]?))
}
const RESET: (u32, i64) = (20, 20);
const KEPT: (u32, i64) = (50, 50);
#[test]
fn later_zero_auto_0px_reset_an_earlier_positive_min_height() {
    for z in ["0", "0px", "auto"] {
        assert_eq!(aside_y("", &format!("style='min-height:50px;min-height:{z}'")), Some(RESET), "{z}");
        assert_eq!(aside_y(&format!(".a{{min-height:50px}}.a{{min-height:{z}}}"), "class=a"), Some(RESET), "sheet {z}");
        assert_eq!(aside_y(&format!("section{{min-height:50px}}"), &format!("style='min-height:{z}'")), Some(RESET), "inline over sheet {z}");
    }
}
#[test]
fn later_positive_beats_earlier_zero_and_plain_positive_still_applies() {
    assert_eq!(aside_y("", "style='min-height:0;min-height:50px'"), Some(KEPT));
    assert_eq!(aside_y("", "style='min-height:50px'"), Some(KEPT));
    assert_eq!(aside_y("", "style='min-height:0'"), Some(RESET));
}
#[test]
fn specificity_and_importance_decide_not_the_reset_itself() {
    // higher-specificity positive survives a later lower-specificity reset
    assert_eq!(aside_y("#s{min-height:50px}.a{min-height:0}", "id=s class=a"), Some(KEPT));
    assert_eq!(aside_y("div section{min-height:50px}section{min-height:0}", ""), Some(KEPT));
    // lower-specificity reset before a higher one wins the other way
    assert_eq!(aside_y(".a{min-height:0}#s{min-height:50px}", "id=s class=a"), Some(KEPT));
    assert_eq!(aside_y("#s{min-height:0}.a{min-height:50px}", "id=s class=a"), Some(RESET));
    // !important
    assert_eq!(aside_y(".a{min-height:50px !important}", "class=a style='min-height:0'"), Some(KEPT));
    assert_eq!(aside_y(".a{min-height:0 !important}", "class=a style='min-height:50px'"), Some(RESET));
}
#[test]
fn resolved_var_zero_resets_and_resolved_positive_applies() {
    assert_eq!(aside_y("section{min-height:50px}.a{--m:0;min-height:var(--m)}", "class=a"), Some(RESET));
    assert_eq!(aside_y("section{min-height:50px}.a{min-height:var(--missing,0)}", "class=a"), Some(RESET));
    assert_eq!(aside_y(".a{--m:50px;min-height:var(--m)}", "class=a"), Some(KEPT));
}
#[test]
fn decimal_px_lexemes_follow_parse_width_and_apply_like_integers() {
    // parse_width admits 1.0px/1.00px style lexemes; `50.0px` is the same used minimum as 50px.
    assert_eq!(aside_y("", "style='min-height:50.0px'"), Some(KEPT));
    assert_eq!(aside_y("", "style='min-height:50.00px;min-height:0.0px'"), aside_y("", "style='min-height:0'"));
}
