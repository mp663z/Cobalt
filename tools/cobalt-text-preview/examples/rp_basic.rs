//! Research-only (private): render a frozen page through the BASIC partial
//! painter and write raw RGBA + a census line. Never a shipping path; the
//! census must travel with the image.
use cobalt_text_preview::LocalFace;
use kobo_web_document::{basic_page::paint_basic_page, box_tree::BoxTree, parse_style_tree_with_inline_sheets_and_selectors, Limits, MediaContext, SupplyBudget};
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
    let face = LocalFace::from_bytes(&std::fs::read("/usr/share/fonts/truetype/liberation2/LiberationSerif-Regular.ttf").unwrap()).unwrap();
    let (list, stats, content_height) = paint_basic_page(&bt, 800, 600, &face).unwrap();
    let height = (content_height.max(1) as u32).min(1024);
    let rgba = list.rasterize(800, height, kobo_web_document::display_list::Rgb(255, 255, 255)).unwrap();
    std::fs::write(format!("/tmp/basic-{page}.rgba"), &rgba).unwrap();
    println!("{page}: 800x{height} complete={} output_truncated={} blocks={} text_blocks={} glyph_runs={} fills={}", stats.complete(), stats.output_truncated, stats.blocks_painted, stats.text_blocks_painted, stats.glyph_runs, stats.fills);
    println!("  skips: {:?}", stats.skips);
    println!("  approximations: {:?}", stats.approximations);
    println!("  bytes={}", rgba.len());
}
