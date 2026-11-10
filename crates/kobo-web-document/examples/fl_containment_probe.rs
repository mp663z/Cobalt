//! PRIVATE scratch probe (not shipped): prints per-fixture VerticalPass
//! content_y + refusal bits for the frozen V1-V6 fixtures (delta9 Part D).
use kobo_web_document::box_tree::{BoxTree, VerticalPass};
use kobo_web_document::{parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget};

fn ext(body: &str) -> kobo_web_document::style_tree::StyleTree {
    let h = format!("<!doctype html><style>body{{margin:0}}div,section,article{{margin:0;padding:0}}</style><div>{body}</div>");
    parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
}

fn dump(label: &str, t: &kobo_web_document::style_tree::StyleTree) {
    let bt = BoxTree::from_style(t);
    let v = VerticalPass::from_boxes_with_direct_text(&bt, 800, 600, |_, _, _| Some(8), |_| Some(18));
    println!("== {label}: v.unsupported={} truncated={}", v.unsupported, v.truncated);
    for (i, b) in bt.boxes.iter().enumerate() {
        let tag = b.source.map(|s| t.nodes[s].tag.to_string()).unwrap_or_default();
        let txt = b.text.as_deref().unwrap_or("").chars().take(12).collect::<String>();
        println!("   box[{i}] {:?} {tag} '{txt}' y={:?} ref={:#06x}", b.kind, v.content_y[i], v.refusals[i].bits());
    }
}

fn main() {
    dump("V1", &ext("<section>alpha</section><article>bravo</article>"));
    dump("V2", &ext("<section>alpha</section><article>bravo <span style='color:#00f'>blue</span></article>"));
    dump("V3", &ext("<section>alpha</section><article><div>block</div>loose</article>"));
    dump("V4", &ext("<section>alpha</section><article><span style='display:inline-block'>ib</span></article>"));
    dump("V5", &ext("<section>alpha</section><article><i>clean</i> <span style='color:#00f'>fail</span></article>"));
    let f6 = parse_style_tree("<!doctype html><link rel=stylesheet href=/a><style>body{margin:0}div,section,article{margin:0;padding:0}</style><div><section>alpha</section><article>bravo</article></div>".as_bytes(), &[], &Limits::DEFAULT);
    dump("V6", &f6);
}
