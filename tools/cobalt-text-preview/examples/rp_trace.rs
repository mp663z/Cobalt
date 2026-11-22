//! Research-only (private): dump the box-tree kind path from the root down
//! to depth 4 for a frozen page, to find where the basic walker stops.
use kobo_web_document::box_tree::{BoxKind, BoxTree};
use kobo_web_document::{parse_style_tree_with_inline_sheets_and_selectors, Limits, MediaContext, SupplyBudget};

fn kind_name(k: &BoxKind) -> &'static str {
    match k {
        BoxKind::Block => "Block",
        BoxKind::AnonymousBlock => "AnonBlock",
        BoxKind::ListItem => "ListItem",
        BoxKind::Text => "Text",
        BoxKind::Inline => "Inline",
        BoxKind::Flex => "Flex",
        BoxKind::InlineBlock => "InlineBlock",
        _ => "Other",
    }
}

fn subtree_size(bt: &BoxTree, i: usize) -> usize {
    let mut n = 1;
    for &c in &bt.boxes[i].children {
        n += subtree_size(bt, c);
    }
    n
}

fn dump(bt: &BoxTree, i: usize, depth: usize) {
    if depth > 4 {
        return;
    }
    let b = &bt.boxes[i];
    let positioned = b.style.positioned.position != kobo_web_document::computed_style::Position::Static;
    println!(
        "{}{} kind={} children={} subtree={} pos={} display_bg={}",
        "  ".repeat(depth),
        i,
        kind_name(&b.kind),
        b.children.len(),
        subtree_size(bt, i),
        positioned,
        b.style.used_background_color().is_some()
    );
    for &c in &b.children {
        dump(bt, c, depth + 1);
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let page = &a[1];
    let home = std::env::var("HOME").unwrap();
    let bytes = std::fs::read(format!("{home}/cb/rr74/rp/{page}.html")).unwrap();
    let budget = SupplyBudget { max_linked_sheets: 24, max_sheet_bytes: 512 * 1024, max_aggregate_bytes: 3 * 1024 * 1024 };
    let limits = Limits { max_input_bytes: 4 * 1024 * 1024, ..Limits::DEFAULT };
    let media = MediaContext::new(800).unwrap();
    let st = parse_style_tree_with_inline_sheets_and_selectors(&bytes, &[], &limits, Some(media), budget);
    let bt = BoxTree::from_style(&st);
    println!("total boxes={}", bt.boxes.len());
    for &r in &bt.roots {
        dump(&bt, r, 0);
    }
}
