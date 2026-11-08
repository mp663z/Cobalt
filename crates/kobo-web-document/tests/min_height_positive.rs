//! PRIVATE candidate (ext entry only): POSITIVE px `min-height` on an in-flow static content-box auto-height block that is
//! not a flex item, float, abspos or float-kernel member and has no positioned child. Expected numbers are Chrome 156
//! constructed-fixture values (private-evidence ul1/mh2.json, mh3.json: tag@top+height inside each container). The engine
//! output is the used-height pass; no Chrome/stored-pixel comparison. Last-child bottom margin with a min-height block is
//! REFUSED (Chrome absorbs it when content < min and lets it escape otherwise: height-dependent, not modelled).
use kobo_web_document::box_tree::{BoxTree, UsedHeightPass, VerticalPass};
use kobo_web_document::{parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget};
fn ext(body: &str) -> kobo_web_document::style_tree::StyleTree {
    let h = format!("<!doctype html><style>body{{margin:0}}div,section,article,aside,nav{{margin:0;padding:0}}</style><div>{body}</div>");
    parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
}
/// (tag, content_y relative to the first div's content top, height) for the four handle tags, from the engine.
fn geom(body: &str) -> Option<Vec<(String, i64, u32)>> {
    let t = ext(body);
    if t.unsupported {
        return None;
    }
    let bt = BoxTree::from_style(&t);
    if bt.unsupported {
        return None;
    }
    let h = UsedHeightPass::from_boxes(&bt, 800, 600);
    let v = VerticalPass::from_boxes(&bt, 800, 600);
    if h.unsupported || v.unsupported {
        return None;
    }
    let root = bt.boxes.iter().position(|b| b.source.is_some_and(|s| t.nodes[s].tag == "div"))?;
    let base = v.content_y[root]?;
    let mut out = vec![];
    for (i, b) in bt.boxes.iter().enumerate() {
        let tag = b.source.map(|s| t.nodes[s].tag.to_string()).unwrap_or_default();
        if matches!(tag.as_str(), "section" | "article" | "aside" | "nav") && b.text.is_none() {
            out.push((tag, v.content_y[i]? - base, h.heights[i]?));
        }
    }
    Some(out)
}
fn g(v: &[(&str, i64, u32)]) -> Vec<(String, i64, u32)> {
    v.iter().map(|(t, y, h)| ((*t).to_owned(), *y, *h)).collect()
}
#[test]
fn positive_min_height_matches_chrome_fixture_numbers() {
    // c1: min 50 > child 20: section 50, aside after it at 50
    assert_eq!(geom("<section style='min-height:50px'><article style='height:20px'></article></section><aside style='height:7px'></aside>"),
        Some(g(&[("section", 0, 50), ("article", 0, 20), ("aside", 50, 7)])));
    // c2: child 60 > min 50: height 60
    assert_eq!(geom("<section style='min-height:50px'><article style='height:60px'></article></section><aside style='height:7px'></aside>"),
        Some(g(&[("section", 0, 60), ("article", 0, 60), ("aside", 60, 7)])));
    // c5: empty block with min 40 and margins 5/5 between siblings does not collapse through
    assert_eq!(geom("<nav style='height:9px'></nav><section style='min-height:40px;margin:5px 0'></section><aside style='height:7px'></aside>"),
        Some(g(&[("nav", 0, 9), ("section", 14, 40), ("aside", 59, 7)])));
    // c7: padding-bottom 4 with min 30 (content box): box 34, the child's 6px margin sits inside the padding
    assert_eq!(geom("<section style='min-height:30px;padding-bottom:4px'><article style='height:10px;margin-bottom:6px'></article></section><aside style='height:7px'></aside>"),
        Some(g(&[("section", 0, 30), ("article", 0, 10), ("aside", 34, 7)])));
}
#[test]
fn last_child_margin_with_min_height_is_refused_not_guessed() {
    // Chrome: c3/c8/c14 absorb it (content < min), c4/c9/c12 let it escape (content >= min): height-dependent.
    for body in [
        "<section style='min-height:50px'><article style='height:20px;margin-bottom:10px'></article></section><aside style='height:7px'></aside>",
        "<section style='min-height:50px'><article style='height:60px;margin-bottom:10px'></article></section><aside style='height:7px'></aside>",
        "<section style='min-height:50px'><article style='height:50px;margin-bottom:10px'></article></section><aside style='height:7px'></aside>",
    ] {
        assert_eq!(geom(body), None, "{body}");
    }
}
#[test]
fn inadmissible_positions_refuse() {
    for body in [
        "<section style='min-height:10px;box-sizing:border-box'><article style='height:5px'></article></section>",
        // FL39: `min-height` with a px `height` is no longer refused (folded into the height); see tests/min_height_fold.rs.
        "<section style='min-height:10px;float:left;width:5px'></section>",
        "<section style='min-height:10px;position:relative'></section>",
        "<section style='min-height:10px'><article style='position:absolute;top:0;left:0;height:3px;width:3px'></article></section>",
        "<section style='display:flex'><article style='min-height:10px;height:5px'></article></section>",
        "<section style='display:flex'><article style='min-height:10px'></article></section>",
    ] {
        assert_eq!(geom(body), None, "{body}");
        // Anchor, causal ONLY for the first body (border-box; the explicit-height body moved to FL39): the same markup without min-height
        // paints. The float/relative/abspos/flex bodies are refused by the surrounding harness even without min-height,
        // so those four refusals are NOT shown to be caused by the min-height gate.
        let plain = body.replace("min-height:10px;", "").replace("min-height:10px", "");
        let paints = |b: &str| {
            let t = ext(b);
            let bt = BoxTree::from_style(&t);
            !t.unsupported && !bt.unsupported && kobo_web_document::css_background::paint_backgrounds(&bt, 100, 100).is_ok()
        };
        if body.contains("border-box") {
            assert!(paints(&plain), "anchor: {plain}");
            assert!(!paints(body), "with min-height: {body}");
        }
    }
    let h = "<!doctype html><style>div{min-height:10px}</style><div>t</div>";
    assert!(parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported, "default entry refuses");
}
