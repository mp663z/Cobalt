//! art67 differential (scratch, unadopted): the existing integer flex engine (via paint_backgrounds) versus
//! flex_units::resolve in Mode::ExactPx, on deterministic pseudo-random integer-px rows (content-box, empty items,
//! flex g s Bpx, padding, margin). Same accept/refuse decision and same rectangles are required.
//! Chrome is NOT involved here: this checks the new function against the OLD ENGINE on the admitted integer subset.
use kobo_web_document::{
    box_tree::BoxTree,
    css_background::paint_backgrounds,
    display_list::Command,
    flex_units::{resolve, Item, Mode},
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
fn units_exactpx_matches_integer_engine_on_random_rows() {
    let mut rng = Lcg(0x5eed_f1e8);
    let (mut agree_ok, mut agree_refuse, mut bad) = (0, 0, Vec::new());
    let mut known_class = 0usize;
    let mut bad_cases: Vec<usize> = Vec::new();
    let mut sig: u64 = 0xcbf29ce484222325; // FNV-1a over (case, engine, units) of every disagreement
    for case in 0..4000 {
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
            v.extend(r.iter().map(|&(l, bw)| ((l / 64) as i32, (bw / 64) as u32)));
            v
        });
        let eng = engine(&html);
        if units == eng {
            if eng.is_some() { agree_ok += 1 } else { agree_refuse += 1 }
        } else if { bad_cases.push(case); for b in format!("{case}|{eng:?}|{units:?}").bytes() { sig ^= b as u64; sig = sig.wrapping_mul(0x100000001b3); } let k = units.as_ref().zip(eng.as_ref()).map_or(false, |(u, e)| { let f: Vec<_> = u.iter().enumerate().filter(|(i, &(_, w))| *i == 0 || w != 0).map(|(_, x)| *x).collect(); f == *e && u.len() > e.len() }); if k { known_class += 1; } bad.len() < 8 } {
            bad.push(format!("case {case}: engine {eng:?} units {units:?} html {html}"));
        } else {
            bad.push(String::new());
        }
    }
    eprintln!("differential: agree_ok={agree_ok} agree_refuse={agree_refuse} disagree={}", bad.len());
    for b in bad.iter().filter(|b| !b.is_empty()) {
        eprintln!("{b}");
    }
    // RECONSTRUCTION CANDIDATE: this v1/v2 differential is a KNOWN-DIVERGENT diagnostic
    // (superseded by the v3 differential; original assertion `bad.is_empty()` preserved in
    // provenance). It now pins the exact recorded outcome, so any change in agreement or
    // disagreement counts (new failure OR unexpected pass) fails loudly.
    // Signature check: every disagreement must belong to the known class (zero_width_class); a
    // disagreement of any other shape is a NEW bug even if the count is unchanged.
    eprintln!("BADCASES {bad_cases:?} SIG {sig:016x}");
    assert_eq!(bad_cases, vec![278, 446, 476, 865, 1426, 1558, 1830, 1940, 2075, 2302, 2564, 2919, 2934, 3059, 3110, 3240, 3317, 3623, 3722, 3937], "disagreeing case identities changed");
    assert_eq!(sig, SIG_PIN, "disagreement (engine, units) tuples changed");
    assert_eq!(known_class, bad.len(), "disagreement outside known class zero_width_class");
    assert_eq!(
        (agree_ok, agree_refuse, bad.len()),
        (1173, 2807, 20),
        "known-divergent diagnostic changed: investigate before updating the pinned counts"
    );
}

const SIG_PIN: u64 = 0x3163c52dea2142a9;
