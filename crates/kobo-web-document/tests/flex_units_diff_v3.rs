//! art67 differential v2 (v1 file kept unchanged; it fails only because zero-width items paint no fill). art67 differential (scratch, unadopted): the existing integer flex engine (via paint_backgrounds) versus
//! flex_units::resolve in Mode::ExactPx, on deterministic pseudo-random integer-px rows (content-box, empty items,
//! flex g s Bpx, padding, margin). Same accept/refuse decision and same rectangles are required.
//! Chrome is NOT involved here: this checks the new function against the OLD ENGINE on the admitted integer subset.
use kobo_web_document::{
    box_tree::BoxTree,
    css_background::paint_backgrounds,
    display_list::Command,
    flex_units_v2::{resolve, Item, Mode},
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
fn v3_units_v2_exactpx_matches_integer_engine_zero_width_omitted() {
    let mut rng = Lcg(0x5eed_f1ea);
    let (mut agree_ok, mut agree_refuse, mut bad) = (0, 0, Vec::new());
    for case in 0..20000 {
        let n = 2 + rng.pick(4) as usize;
        let w = 100 + rng.pick(401) as i64;
        let mut html = format!("<!doctype html><html><body style='margin:0'><div style='display:flex;width:{w}px;background:#eee'>");
        let mut items = Vec::new();
        for k in 0..n {
            let base = rng.pick(121) as i64;
            let (g, s) = (rng.pick(4) as u32, rng.pick(4) as u32);
            let (pad, mar) = ([0, 3, 4][rng.pick(3) as usize], [0, 2, 5][rng.pick(3) as usize]);
            html.push_str(&format!(
                "<div style='flex:{g} {s} {base}px;padding:0 {pad}px;margin:0 {mar}px;background:#{:02x}{:02x}{:02x}'><div style='height:20px'></div></div>",
                10 + k, 20 + k, 30 + k
            ));
            items.push(Item { base: base * 64, min: 0, max: None, grow: g, shrink: s, padb: 2 * pad * 64, ml: mar * 64, mr: mar * 64 });
        }
        html.push_str("</div></body></html>");
        let units = resolve(Mode::ExactPx, w * 64, &items).map(|r| {
            let mut v = vec![(0, w as u32)];
            // paint_backgrounds emits no Fill for a zero-width box: omit those from the expected list (v1 failed on exactly this)
            v.extend(r.iter().filter(|&&(_, bw)| bw != 0).map(|&(l, bw)| ((l / 64) as i32, (bw / 64) as u32)));
            v
        });
        let eng = engine(&html);
        if units == eng {
            if eng.is_some() { agree_ok += 1 } else { agree_refuse += 1 }
        } else if bad.len() < 8 {
            bad.push(format!("case {case}: engine {eng:?} units {units:?} html {html}"));
        } else {
            bad.push(String::new());
        }
    }
    eprintln!("shadow [agree_accept, agree_refuse, legacy_refuse_units_resolve, DISAGREE, skipped]: {:?}", kobo_web_document::shadow_counts());
    eprintln!("differential: agree_ok={agree_ok} agree_refuse={agree_refuse} disagree={}", bad.len());
    for b in bad.iter().filter(|b| !b.is_empty()) {
        eprintln!("{b}");
    }
    assert!(bad.is_empty(), "disagreements: {}", bad.len());
}
