//! PRIVATE host probe: paint_backgrounds of an HTML file. Usage: bg_probe page.html out.rgba w h
use kobo_web_document::{box_tree::BoxTree, css_background::paint_backgrounds, display_list::Rgb, parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, Limits, MediaContext, SupplyBudget};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let html = std::fs::read(&a[1]).unwrap();
    let (w, h): (u32, u32) = (a[3].parse().unwrap(), a[4].parse().unwrap());
    let ext = std::env::var("EXT").is_ok();
    let st = if ext { parse_style_tree_with_inline_sheets_and_selectors(&html, &[], &Limits::DEFAULT, MediaContext::new(785), SupplyBudget::CANONICAL) } else { parse_style_tree(&html, &[], &Limits::DEFAULT) };
    if std::env::var("DIAG").is_ok() { eprintln!("unsupported={} nodes={} diag={:?}", st.unsupported, st.nodes.len(), st.diagnostics); }
    if st.unsupported && std::env::var("DIAG").is_err() { eprintln!("intake refused: {:?}", st.diagnostics); std::process::exit(2); }
    let t = BoxTree::from_style(&st);
    let l = paint_backgrounds(&t, w, h).expect("paint");
    std::fs::write(&a[2], l.rasterize(w, h, Rgb(255, 255, 255)).unwrap()).unwrap();
    println!("ok {} commands", l.commands().len());
}
