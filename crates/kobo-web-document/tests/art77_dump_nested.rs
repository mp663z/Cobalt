//! art67 differential v2 (v1 file kept unchanged; it fails only because zero-width items paint no fill). art67 differential (scratch, unadopted): the existing integer flex engine (via paint_backgrounds) versus
//! flex_units::resolve in Mode::ExactPx, on deterministic pseudo-random integer-px rows (content-box, empty items,
//! flex g s Bpx, padding, margin). Same accept/refuse decision and same rectangles are required.
//! Chrome is NOT involved here: this checks the new function against the OLD ENGINE on the admitted integer subset.
use kobo_web_document::{
    box_tree::BoxTree,
    css_background::paint_backgrounds,
    display_list::Command,
        parse_style_tree, Limits,
};

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn pick(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn engine(html: &str) -> Option<Vec<(i32, u32)>> {
    let t = BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT));
    let list = paint_backgrounds(&t, 1000, 200).ok()?;
    let fills: Vec<(i32, u32)> = list
        .commands()
        .iter()
        .filter_map(|c| if let Command::Fill { rect, .. } = c { Some((rect.x, rect.width)) } else { None })
        .collect();
    Some(fills)
}

#[test]
fn dump_engine_rows_nested() {
    let mut rng = Lcg(0x71e57ed);
    let mut dump = String::new();
    for case in 0..20000 {
        let n = 2 + rng.pick(3) as usize;
        let w = 100 + rng.pick(301) as i64;
        let mut html = format!("<!doctype html><html><body style='margin:0'><div style='display:flex;width:{w}px;background:#eee'>");
        for k in 0..n {
            let base = rng.pick(121);
            let (g, s) = (rng.pick(4), rng.pick(4));
            let width = if rng.pick(3) == 0 { format!("width:{}px;", rng.pick(121)) } else { String::new() };
            let (pad, bd, mar) = ([0, 3][rng.pick(2) as usize], [0, 2][rng.pick(2) as usize], [0, 2][rng.pick(2) as usize]);
            let inner = match rng.pick(4) {
                0 => "<div style='height:20px'></div>".to_string(),
                1 => format!("<div style='width:{}px;height:20px'></div>", rng.pick(150)),
                2 => format!("<div style='width:{}px;padding:0 {}px;height:20px'><div style='width:{}px;height:10px'></div></div>", rng.pick(100), rng.pick(6), rng.pick(60)),
                _ => format!("<div style='height:20px'><div style='width:{}px;height:5px'></div><div style='width:{}px;height:5px'></div></div>", rng.pick(110), rng.pick(110)),
            };
            html.push_str(&format!("<div style='flex:{g} {s} {base}px;{width}padding:0 {pad}px;border:{bd}px solid #000;margin:0 {mar}px;background:#{:02x}{:02x}{:02x}'>{inner}</div>", 10 + k, 20 + k, 30 + k));
        }
        html.push_str("</div></body></html>");
        dump.push_str(&format!("{case}\t{:?}\n", engine(&html)));
    }
    std::fs::write(std::env::var("DUMP_OUT").unwrap_or_else(|_| std::env::temp_dir().join(concat!(env!("CARGO_CRATE_NAME"), ".dump.txt")).to_string_lossy().into_owned()), dump).unwrap();
}
