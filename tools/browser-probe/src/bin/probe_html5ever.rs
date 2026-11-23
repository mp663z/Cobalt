use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};

fn count(handle: &Handle, depth: usize, nodes: &mut usize, max_depth: &mut usize, links: &mut usize) {
    *nodes += 1;
    *max_depth = (*max_depth).max(depth);
    if let NodeData::Element { name, .. } = &handle.data {
        if &*name.local == "a" {
            *links += 1;
        }
    }
    for child in handle.children.borrow().iter() {
        count(child, depth + 1, nodes, max_depth, links);
    }
}

fn main() {
    let Some(bytes) = browser_probe::input() else {
        browser_probe::run_baseline();
        return;
    };
    let start = std::time::Instant::now();
    let dom = html5ever::parse_document(RcDom::default(), Default::default())
        .from_utf8()
        .one(&bytes[..]);
    let elapsed = start.elapsed();
    let (mut nodes, mut depth, mut links) = (0, 0, 0);
    count(&dom.document, 0, &mut nodes, &mut depth, &mut links);
    println!("html5ever nodes={nodes} depth={depth} links={links} parse_us={}", elapsed.as_micros());
}
