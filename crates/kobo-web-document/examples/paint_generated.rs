//! EXPERIMENTAL, UNREVIEWED, opt-in: paint the extended-CSS entry through the
//! generated-block painter to a binary PPM for paired visual evidence (engine
//! vs Chrome screenshot). Order/geometry evidence only, never a parity claim.
use kobo_web_document::{
    box_tree::BoxTree, css_background::paint_backgrounds_generated_block, display_list::Command,
    parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget,
};
use std::io::Write as _;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("html path");
    let out_path = args.next().expect("ppm out path");
    let width: u32 = args.next().expect("width").parse().expect("width num");
    let height: u32 = args.next().expect("height").parse().expect("height num");
    let bytes = std::fs::read(path).expect("read");
    let tree = parse_style_tree_with_inline_sheets_and_selectors(
        &bytes,
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    );
    let list = paint_backgrounds_generated_block(&BoxTree::from_style(&tree), width, height)
        .expect("paint_backgrounds_generated_block refused");
    let mut img = vec![255u8; (width * height * 3) as usize];
    for c in list.commands() {
        if let Command::Fill { rect, color, .. } = c {
            eprintln!(
                "FILL x={} y={} w={} h={} rgb=({},{},{})",
                rect.x, rect.y, rect.width, rect.height, color.0, color.1, color.2
            );
            for y in rect.y.max(0)..(rect.y + rect.height as i32).min(height as i32) {
                for x in rect.x.max(0)..(rect.x + rect.width as i32).min(width as i32) {
                    let o = ((y as u32 * width + x as u32) * 3) as usize;
                    img[o] = color.0;
                    img[o + 1] = color.1;
                    img[o + 2] = color.2;
                }
            }
        }
    }
    let mut f = std::fs::File::create(out_path).expect("create");
    write!(f, "P6\n{} {}\n255\n", width, height).expect("header");
    f.write_all(&img).expect("pixels");
}
