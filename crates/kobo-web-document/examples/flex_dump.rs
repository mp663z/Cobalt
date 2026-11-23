//! Scratch: paint an HTML file's backgrounds to a flat PPM. Prints REFUSED on error.
use kobo_web_document::{box_tree::BoxTree, css_background::paint_backgrounds, display_list::Command, parse_style_tree, Limits};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let html = std::fs::read(&a[1]).unwrap();
    let (w, h): (u32, u32) = (a[2].parse().unwrap(), a[3].parse().unwrap());
    let styled = parse_style_tree(&html, &[], &Limits::DEFAULT);
    let tree = BoxTree::from_style(&styled);
    match paint_backgrounds(&tree, w, h) {
        Err(e) => { println!("REFUSED {e:?}"); }
        Ok(list) => {
            let mut px = vec![255u8; (w * h * 3) as usize];
            for c in list.commands() {
                if let Command::Fill { rect, color, .. } = c {
                    for y in rect.y.max(0)..(rect.y + rect.height as i32).min(h as i32) {
                        for x in rect.x.max(0)..(rect.x + rect.width as i32).min(w as i32) {
                            let i = ((y as u32 * w + x as u32) * 3) as usize;
                            px[i] = color.0; px[i + 1] = color.1; px[i + 2] = color.2;
                        }
                    }
                } else { println!("NONFILL"); }
            }
            let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
            out.extend(px);
            std::fs::write(&a[4], out).unwrap();
            println!("OK");
        }
    }
}
