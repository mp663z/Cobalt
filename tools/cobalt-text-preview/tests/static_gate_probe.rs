//! Verification-only probe: reports which paint gate each frozen real page
//! trips before `paint_direct_text_blocks` refuses. Not part of the shipping
//! or benchmark surfaces; run explicitly with --nocapture.
use kobo_web_document::{box_tree::BoxTree, display_list::MAX_COMMANDS, parse_style_tree, Limits};

fn structure_valid(tree: &BoxTree, limit: usize) -> bool {
    let count = tree.boxes.len();
    if count == 0 || count > limit || tree.roots.len() != 1 || tree.roots[0] >= count {
        return false;
    }
    let mut edges = 0_usize;
    for (index, node) in tree.boxes.iter().enumerate() {
        edges = edges.saturating_add(node.children.len());
        if edges > limit
            || node.source.is_none()
            || node.parent.is_some_and(|p| p >= index)
            || node.children.iter().any(|&c| c <= index || c >= count)
        {
            return false;
        }
    }
    let mut seen = vec![false; count];
    let mut stack = vec![(tree.roots[0], None)];
    while let Some((index, parent)) = stack.pop() {
        let node = &tree.boxes[index];
        if seen[index] || node.parent != parent {
            return false;
        }
        seen[index] = true;
        for &child in &node.children {
            stack.push((child, Some(index)));
        }
    }
    seen.into_iter().all(|v| v)
}

// Explicit opt-in diagnostic probe: it reads the private frozen corpus under
// /downloads/cobalt-real-corpus, which exists only on the owner's measurement
// host, so it is ignored by default. Run it deliberately with
// `cargo test -- --ignored` on a host holding the corpus; a portable
// `cargo test --all-targets` must never pass or fail on fixture absence.
#[test]
#[ignore = "requires the private frozen corpus under /downloads/cobalt-real-corpus"]
fn report_real_page_paint_gates() {
    let corpus = std::path::Path::new("/downloads/cobalt-real-corpus");
    for name in ["wikipedia", "gov", "docs", "blog", "news-npr"] {
        let path = corpus.join(format!("{name}.complete.html"));
        let html = std::fs::read(&path).expect("read page");
        let styled = parse_style_tree(&html, &[], &Limits::DEFAULT);
        let tree = BoxTree::from_style(&styled);
        let painted: Vec<String> = tree
            .boxes
            .iter()
            .filter(|b| b.style.decor.has_painted_border())
            .filter_map(|b| b.source)
            .map(|s| styled.nodes[s].tag.clone())
            .collect();
        eprintln!(
            "{name}: bytes={} boxes={} (max {MAX_COMMANDS}) roots={} truncated={} unsupported={} quirks={} structure_ok={} painted_border_boxes={} tags={:?}",
            html.len(),
            tree.boxes.len(),
            tree.roots.len(),
            tree.truncated,
            tree.unsupported,
            tree.quirks,
            structure_valid(&tree, MAX_COMMANDS),
            painted.len(),
            &painted[..painted.len().min(8)],
        );
    }
}
