//! MEASUREMENT-ONLY diagnostic. Output is APPROXIMATE (every JSON record carries
//! "approximate":true and "model":"fontdue-unrounded-advance-sum").
//! It does not use or modify any engine crate. It is not a layout feature.
//!
//! usage: cobalt-fractional-diag FONT.ttf TEXT_FILE K0 K1 KSTEP W0_64 W1_64 WSTEP_64
//!   sizes are k/64 px for k in K0..=K1 step KSTEP; widths are w/64 px in W0_64..=W1_64 step WSTEP_64.
//! For each (size,width): greedy wrap (word fits if line+space+word <= width, all in f32 fontdue
//! advances, no kerning, no ligatures), break word indices, and for every break the
//! rejected-word overflow (line width including the rejected word minus container width) and
//! decision_slack[i] = container width - (model line width if word i were appended); the sign is the
//! model's fit decision, the magnitude is the margin of that decision (index 0 unused).
use fontdue::{Font, FontSettings};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 9 {
        eprintln!("usage: FONT TEXT K0 K1 KSTEP W0_64 W1_64 WSTEP_64");
        std::process::exit(2);
    }
    let font = Font::from_bytes(std::fs::read(&a[1]).unwrap(), FontSettings::default()).unwrap();
    let text = std::fs::read_to_string(&a[2]).unwrap();
    let words: Vec<&str> = text.trim_end_matches('\n').split(' ').collect();
    let n = |i: usize| -> i64 { a[i].parse().unwrap() };
    let (k0, k1, ks, w0, w1, ws) = (n(3), n(4), n(5), n(6), n(7), n(8));
    let mut k = k0;
    while k <= k1 {
        let px = k as f32 / 64.0;
        let adv = |s: &str| -> f32 { s.chars().map(|c| font.metrics(c, px).advance_width).sum() };
        let sp = adv(" ");
        let ww: Vec<f32> = words.iter().map(|w| adv(w)).collect();
        let mut w = w0;
        while w <= w1 {
            let wd = w as f32 / 64.0;
            let mut brk = vec![];
            let mut over = vec![];
            let mut slack = vec![0f32; ww.len()];
            let mut cur = ww[0];
            for i in 1..ww.len() {
                slack[i] = wd - (cur + sp + ww[i]);
                if cur + sp + ww[i] <= wd {
                    cur += sp + ww[i];
                } else {
                    brk.push(i);
                    over.push(cur + sp + ww[i] - wd);
                    cur = ww[i];
                }
            }
            let j = |v: &Vec<String>| v.join(",");
            println!(
                "{{\"approximate\":true,\"model\":\"fontdue-unrounded-advance-sum\",\"k\":{k},\"px\":{px},\"w64\":{w},\"width\":{wd},\"space\":{sp:.6},\"breaks\":[{}],\"rejected_word_overflow\":[{}],\"decision_slack\":[{}]}}",
                j(&brk.iter().map(|b| b.to_string()).collect()),
                j(&over.iter().map(|o| format!("{o:.6}")).collect()),
                j(&slack.iter().map(|o| format!("{o:.6}")).collect())
            );
            w += ws;
        }
        k += ks;
    }
}
