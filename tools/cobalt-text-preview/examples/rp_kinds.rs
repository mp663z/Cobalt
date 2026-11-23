use kobo_web_document::{box_tree::{BoxKind, BoxTree}, parse_style_tree_with_inline_sheets_and_selectors, Limits, MediaContext, SupplyBudget};
fn main() {
    let home = std::env::var("HOME").unwrap();
    let budget = SupplyBudget { max_linked_sheets: 24, max_sheet_bytes: 512 * 1024, max_aggregate_bytes: 3 * 1024 * 1024 };
    let limits = Limits { max_input_bytes: 4 * 1024 * 1024, ..Limits::DEFAULT };
    let media = MediaContext::new(800).unwrap();
    for page in ["blog", "docs", "gov", "news-npr", "wikipedia"] {
        let bytes = std::fs::read(format!("{home}/cb/rr74/rp/{page}.html")).unwrap();
        let st = parse_style_tree_with_inline_sheets_and_selectors(&bytes, &[], &limits, Some(media), budget);
        let bt = BoxTree::from_style(&st);
        let mut kinds = std::collections::BTreeMap::new();
        let mut flex_text = 0usize;
        for b in &bt.boxes {
            *kinds.entry(format!("{:?}", b.kind)).or_insert(0) += 1;
        }
        // count text leaves inside flex subtrees
        let mut stack: Vec<usize> = Vec::new();
        for (i, b) in bt.boxes.iter().enumerate() {
            if b.kind == BoxKind::Flex && !bt.boxes.iter().any(|x| x.children.contains(&i) && x.kind == BoxKind::Flex) { stack.push(i); }
        }
        let mut seen = std::collections::BTreeSet::new();
        while let Some(i) = stack.pop() {
            if !seen.insert(i) { continue; }
            if bt.boxes[i].kind == BoxKind::Text { flex_text += 1; }
            for &c in &bt.boxes[i].children { stack.push(c); }
        }
        println!("{page}: boxes={} {:?} text_leaves_under_flex={}", bt.boxes.len(), kinds, flex_text);
    }
}
