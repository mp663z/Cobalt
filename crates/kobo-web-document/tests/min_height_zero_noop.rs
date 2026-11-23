//! PRIVATE candidate (ext entry only): `min-height: 0|0px|auto` has USED minimum 0 in the admitted slice (block, abspos, row-flex
//! cross axis; inline is inapplicable so no inline claim). Compared by engine layout (rasterised backgrounds), not Chrome computed style; no Chrome/stored-pixel comparison.
use kobo_web_document::box_tree::BoxTree;
use kobo_web_document::css_background::paint_backgrounds;
use kobo_web_document::display_list::Rgb;
use kobo_web_document::{parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget};
fn ext(css: &str, body: &str) -> kobo_web_document::style_tree::StyleTree {
    let h = format!("<!doctype html><style>body{{margin:0}}div{{margin:0;padding:0}}{css}</style>{body}");
    parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
}
fn raster(css: &str, body: &str) -> Vec<u8> {
    let t = ext(css, body);
    assert!(!t.unsupported, "{css}");
    let bt = BoxTree::from_style(&t);
    assert!(!bt.unsupported, "{css}");
    paint_backgrounds(&bt, 120, 80).expect("paints").rasterize(120, 80, Rgb(255, 255, 255)).unwrap()
}
const SHAPES: [&str; 4] = [
    "<div class=a style='background-color:#cde;height:20px'></div><div class=a style='background-color:#fc9;height:10px'></div>",
    "<div class=f style='display:flex;width:100px;background-color:#cde'><div class=a style='background-color:#fc9;height:15px;width:30px'></div><div class=a style='background-color:#9cf;width:40px;height:25px'></div></div>",
    "<div class=r style='position:relative;height:30px;background-color:#cde'><div class=a style='position:absolute;top:2px;left:3px;width:20px;height:10px;background-color:#fc9'></div></div>",
    "<div class=a style='background-color:#cde;height:12px;width:50px'><div class=a style='background-color:#fc9;height:6px'></div></div>",
];
#[test]
fn zero_and_auto_min_height_lay_out_like_absent() {
    for (i, shape) in SHAPES.iter().enumerate() {
        let want = raster("", shape);
        assert!(want.iter().any(|&b| b != 255), "anchor: shape {i} paints something");
        for v in ["0", "0px", "auto"] {
            assert_eq!(raster(&format!("div{{min-height:{v}}}"), shape), want, "shape {i} min-height:{v}");
        }
    }
}
#[test]
fn positive_and_other_min_height_values_refuse_and_default_entry_refuses() {
    // EXPECTATION CHANGE (disclosed, fl16): positive px values (parse_width lexemes, e.g. 1px, 1.0px) ("1px", "40px") are now parsed under ext; the box tree
    // admits or refuses the box (tests/min_height_positive.rs). Everything else still refuses here.
    // FL44 (disclosed edit): the list had "calc(1px + 1px)" (now folded to px under ext); replaced with a still-refused "calc(1px * 2)". Pre-change copy in the FL44 probe pack.
    for v in ["10%", "calc(1px * 2)", "min-content", "fit-content", "inherit", "initial", "unset", "revert", "0%", "-1px", "1em", "var(--x)", ""] {
        assert!(ext(&format!("div{{min-height:{v}}}"), "<div>t</div>").unsupported, "{v:?}");
    }
    let h = "<!doctype html><style>div{min-height:0}</style><div>t</div>";
    assert!(parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported);
}
