//! Research-only (private): intake profile experiment for frozen real pages.
//! Admits ALL decoded data: stylesheets under an explicit non-canonical supply
//! budget and raised input limits where a page needs them. Never a shipping
//! path; prints diagnostics only.
use cobalt_text_preview::LocalFace;
use kobo_web_document::{box_tree::BoxTree, css_text_page::{paint_direct_text_blocks}, parse_style_tree_with_inline_sheets_and_selectors, Limits, MediaContext, SupplyBudget};

fn main() {
    let face = LocalFace::from_bytes(&std::fs::read("/usr/share/fonts/truetype/liberation2/LiberationSerif-Regular.ttf").unwrap()).unwrap();
    let home = std::env::var("HOME").unwrap();
    // Explicit research profile: named, recorded, not canonical.
    let budget = SupplyBudget { max_linked_sheets: 24, max_sheet_bytes: 512 * 1024, max_aggregate_bytes: 3 * 1024 * 1024 };
    let limits = Limits { max_input_bytes: 4 * 1024 * 1024, ..Limits::DEFAULT };
    let media = MediaContext::new(800).unwrap();
    for page in ["blog", "docs", "gov", "news-npr", "wikipedia"] {
        let bytes = std::fs::read(format!("{home}/cb/rr74/rp/{page}.html")).unwrap();
        let st = parse_style_tree_with_inline_sheets_and_selectors(&bytes, &[], &limits, Some(media), budget);
        let d = &st.diagnostics;
        let bt = BoxTree::from_style(&st);
        let r = paint_direct_text_blocks(&bt, 800, 600, &face);
        println!("== {page}: truncated={} unsupported={} retained={} expanded_decl={} match_attempts={} | box_unsupported={} box_truncated={} paint={}",
            st.truncated, st.unsupported, d.retained_nodes, d.expanded_declarations, d.match_attempts,
            bt.unsupported, bt.truncated, if r.is_ok() { "Ok(PAINTS)" } else { "Err(REFUSES)" });
        println!("   first_reason={:?}", d.first_reason());
    }
}
