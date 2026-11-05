//! art70: generate seeded flex rows with borders; write cases + ENGINE item fills (or REFUSE) to /tmp/art70_cases.tsv. Chrome comparison is done by a separate script.
use kobo_web_document::{box_tree::BoxTree, css_background::paint_backgrounds, display_list::Command, parse_style_tree, Limits};
struct Lcg(u64);
impl Lcg { fn pick(&mut self, n: u64) -> u64 { self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); (self.0 >> 33) % n } }
#[test]
fn gen() {
    let mut r = Lcg(0xb0a7d3);
    let mut out = String::new();
    for case in 0..400 {
        let n = 2 + r.pick(3) as usize;
        let w = 100 + r.pick(301);
        let mut html = format!("<!doctype html><html><body style='margin:0'><div style='display:flex;width:{w}px;background:#eee'>");
        for k in 0..n {
            let (b, g, s) = (r.pick(101), r.pick(4), r.pick(4));
            let (bd, pd, mg) = ([0, 1, 2, 3][r.pick(4) as usize], [0, 3][r.pick(2) as usize], [0, 2][r.pick(2) as usize]);
            html.push_str(&format!("<div style='flex:{g} {s} {b}px;border:{bd}px solid #000;padding:0 {pd}px;margin:0 {mg}px;background:#{:02x}{:02x}{:02x}'><div style='height:20px'></div></div>", 10 + k, 20 + k, 30 + k));
        }
        html.push_str("</div></body></html>");
        let t = BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT));
        let res = match paint_backgrounds(&t, 1000, 200) {
            Err(_) => "REFUSE".to_string(),
            Ok(l) => {
                let mut v = Vec::new();
                for k in 0..n {
                    let want = format!("Rgb({}, {}, {})", 10 + k, 20 + k, 30 + k);
                    for c in l.commands() { if let Command::Fill { rect, color, .. } = c { if format!("{:?}", color) == want { v.push(format!("{}:{}", rect.x, rect.width)); } } }
                    if v.len() < k + 1 { v.push("none".into()); }
                }
                v.join(",")
            }
        };
        out.push_str(&format!("{case}\t{res}\t{html}\n"));
    }
    std::fs::write("/tmp/art70_cases.tsv", out).unwrap();
}
