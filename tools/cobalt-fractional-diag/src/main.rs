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
    let ceilpos = a.len() == 10 && a[9] == "ceilpos";
    if a.len() != 9 && !ceilpos {
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
        // ceilpos model (source-grounded hypothesis from Blink ShapeResult/ShapingLineBreaker at 156.0.8078.47):
        // per-glyph advance rounded to 1/65536 px, positions accumulated from the START OF THE TEXT,
        // each character position = ceil to 1/64 px of the accumulated raw sum; a line starting at
        // offset s with available width W fits every word whose end position <= pos(s) + W.
        let mut pos64: Vec<i64> = vec![0];
        let mut acc: i64 = 0;
        let chars: Vec<char> = words.join(" ").chars().collect();
        for &c in &chars {
            acc += (font.metrics(c, px).advance_width as f64 * 65536.0).round() as i64;
            pos64.push((acc + 1023) / 1024);
        }
        let mut starts = vec![]; // char offset of each word start
        let mut o = 0usize;
        for w in &words { starts.push(o); o += w.chars().count() + 1; }
        let mut w = w0;
        while w <= w1 {
            if ceilpos {
                let wd = w as f32 / 64.0;
                let mut brk = vec![];
                let mut line_start = 0usize; // word index
                let mut i = 1usize;
                let mut cur_first = 0usize;
                let _ = (&mut line_start, &mut i);
                let mut first = 0usize;
                loop {
                    let s0 = pos64[starts[first]];
                    let end_pos = s0 + w;
                    let mut last = first;
                    while last + 1 < words.len() {
                        let e = starts[last + 1] - 1 + 0; // offset of the space char after word `last+1`?? computed below
                        let _ = e;
                        let end_of_next = starts[last + 1] + words[last + 1].chars().count();
                        if pos64[end_of_next] <= end_pos { last += 1; } else { break; }
                    }
                    if last + 1 >= words.len() { break; }
                    brk.push(last + 1);
                    first = last + 1;
                    cur_first = first;
                }
                let _ = cur_first;
                println!(
                    "{{\"approximate\":true,\"model\":\"ceilpos64\",\"k\":{k},\"px\":{px},\"w64\":{w},\"width\":{wd},\"breaks\":[{}]}}",
                    brk.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(",")
                );
                w += ws;
                continue;
            }
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
