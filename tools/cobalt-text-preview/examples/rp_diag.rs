//! FL62 measurement-only (private pages): the content-free style-parse diagnostics the preview's
//! own parse path produces for a caller-supplied page. No production path; evidence only.
use kobo_web_document::style_diagnostics::RefusalReason;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let html = std::fs::read(&a[1])?;
    let w: u32 = a[2].parse()?;
    let media = kobo_web_document::MediaContext::new(w).ok_or("width")?;
    let st = kobo_web_document::parse_style_tree_with_media(&html, &[], &kobo_web_document::Limits::DEFAULT, media);
    let d = &st.diagnostics;
    println!("truncated={} unsupported={} quirks={} nodes={} input_bytes={} dom_nodes={} retained_nodes={} dropped_attrs={} dropped_text={} expanded_decl={} match_attempts={}",
        st.truncated, st.unsupported, st.quirks, st.nodes.len(), d.input_bytes, d.dom_nodes, d.retained_nodes, d.dropped_attributes, d.dropped_text, d.expanded_declarations, d.match_attempts);
    println!("first_reason={:?}", d.first_reason());
    for r in RefusalReason::ALL { let c = d.count(r); if c > 0 { println!("count {:?}={}", r, c); } }
    Ok(())
}
