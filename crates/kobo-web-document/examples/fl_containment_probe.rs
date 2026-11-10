//! PRIVATE scratch probe (not shipped): prints per-fixture UsedHeightPass
//! heights + refusal bits for the frozen F1-F6 containment fixtures so the
//! executable test assertions match actual contained semantics.
use kobo_web_document::box_tree::{BoxTree, UsedHeightPass};
use kobo_web_document::{parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget};

fn ext(body: &str) -> kobo_web_document::style_tree::StyleTree {
    let h = format!("<!doctype html><style>body{{margin:0}}div,section,article{{margin:0;padding:0}}</style><div>{body}</div>");
    parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
}

fn dump(label: &str, t: &kobo_web_document::style_tree::StyleTree) {
    println!("== {label}: style_tree.unsupported={} truncated={}", t.unsupported, t.truncated);
    let bt = BoxTree::from_style(t);
    println!("   box_tree.unsupported={} truncated={}", bt.unsupported, bt.truncated);
    let h = UsedHeightPass::from_boxes_with_direct_text(&bt, 800, 600, |_, _, _| Some(8), |_| Some(18));
    println!("   used.unsupported={} truncated={}", h.unsupported, h.truncated);
    for (i, b) in bt.boxes.iter().enumerate() {
        let tag = b.source.map(|s| t.nodes[s].tag.to_string()).unwrap_or_default();
        if matches!(tag.as_str(), "section" | "article") || b.text.is_some() || !h.refusals[i].is_empty() {
            let txt = b.text.as_deref().unwrap_or("").chars().take(20).collect::<String>();
            println!("   box[{i}] kind={:?} tag={tag} text='{txt}' height={:?} refusals={:#06x}", b.kind, h.heights[i], h.refusals[i].bits());
        }
    }
}

fn main() {
    dump("F1 clean siblings", &ext("<section>alpha</section><article>bravo</article>"));
    dump("F2 color-fail sibling", &ext("<section>alpha</section><article>bravo <span style='color:#00f'>blue</span></article>"));
    dump("F3 no-inline-owner sibling", &ext("<section>alpha</section><article><div>block</div>loose</article>"));
    dump("F4 inline-block present", &ext("<section>alpha</section><article><span style='display:inline-block'>ib</span></article>"));
    dump("F5 mixed clean+fail inline", &ext("<section>alpha</section><article><i>clean</i> <span style='color:#00f'>fail</span></article>"));
    let f6 = parse_style_tree("<!doctype html><link rel=stylesheet href=/a><style>body{margin:0}div,section,article{margin:0;padding:0}</style><div><section>alpha</section><article>bravo</article></div>".as_bytes(), &[], &Limits::DEFAULT);
    dump("F6 missing stylesheet", &f6);
}
