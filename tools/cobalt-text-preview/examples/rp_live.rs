//! FL72 measurement-only: the style-parse diagnostics for a LIVE-shaped page (linked sheets supplied through sheets[] in link order, host-supply path).
//! Usage: rp_live page.html width sheet1.css sheet2.css ... ; runs E1 (parse_style_tree_with_media) and E2 (ext entry) with the SAME supplied sheets.
use kobo_web_document::style_diagnostics::RefusalReason;
fn show(tag: &str, st: &kobo_web_document::style_tree::StyleTree) {
    let d = &st.diagnostics;
    println!("{tag}: truncated={} unsupported={} nodes={} dropped_attrs={} first_reason={:?} host_sheets_claimed={:?}", st.truncated, st.unsupported, st.nodes.len(), d.dropped_attributes, d.first_reason(), st.host_sheets_claimed);
    for r in RefusalReason::ALL { let c = d.count(r); if c > 0 { println!("   count {:?}={}", r, c); } }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let html = std::fs::read(&a[1])?;
    let w: u32 = a[2].parse()?;
    let sheets: Vec<Vec<u8>> = a[3..].iter().map(|p| std::fs::read(p)).collect::<Result<_, _>>()?;
    let media = kobo_web_document::MediaContext::new(w).ok_or("width")?;
    show("E1 parse_style_tree_with_media", &kobo_web_document::parse_style_tree_with_media(&html, &sheets, &kobo_web_document::Limits::DEFAULT, media));
    show("E2 ext", &kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(&html, &sheets, &kobo_web_document::Limits::DEFAULT, Some(media), kobo_web_document::SupplyBudget::CANONICAL));
    Ok(())
}
