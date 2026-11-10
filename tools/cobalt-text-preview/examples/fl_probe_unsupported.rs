use cobalt_text_preview::LocalFace;
use kobo_web_document::{box_tree::BoxTree, css_text_page::{paint_direct_text_blocks, FontProvider}, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget};
fn main() {
    let face = LocalFace::from_bytes(&std::fs::read("/usr/share/fonts/truetype/liberation2/LiberationSerif-Regular.ttf").unwrap()).unwrap();
    for page in ["gov", "blog", "docs", "news-npr", "wikipedia"] {
        let path = format!("{}/cb/rr74/rp/{page}.html", std::env::var("HOME").unwrap());
        let bytes = std::fs::read(&path).unwrap();
        let st = parse_style_tree_with_inline_sheets_and_selectors(&bytes, &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
        let bt = BoxTree::from_style(&st);
        let r = paint_direct_text_blocks(&bt, 800, 600, &face);
        println!("{page}: style_unsupported={} box_unsupported={} truncated={} paint_direct_text_blocks={}", st.unsupported, bt.unsupported, bt.truncated, if r.is_ok() { "Ok(PAINTS)" } else { "Err(REFUSES)" });
    }
}
