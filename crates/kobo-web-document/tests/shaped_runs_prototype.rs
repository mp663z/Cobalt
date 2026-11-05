//! Prototype controls for `place_runs_shaped`: legacy byte-identity on every
//! ineligible input, exact equivalence with the legacy placer when advances
//! are legacy*64, and the strict vs fitted comparison rule.
use kobo_web_document::inline_lines::{
    place_runs_normal, place_runs_shaped, FitRule, Lines, Placed, RunShaper, ShapedCluster,
};

fn legacy(ch: char, bold: bool) -> Option<u32> {
    Some(5 + (ch as u32 % 4) + u32::from(bold))
}
const STRICT: FitRule = FitRule { unit: 64, slack: 0 };

struct Scaled; // clusters = legacy advances * 64, 1:1
impl RunShaper for Scaled {
    fn shape_run(&self, text: &str, _s: u32, bold: bool) -> Option<Vec<ShapedCluster>> {
        Some(
            text.chars()
                .enumerate()
                .map(|(i, c)| ShapedCluster {
                    first_char: i as u32,
                    char_len: 1,
                    units: legacy(c, bold).unwrap() * 64,
                })
                .collect(),
        )
    }
}
struct Fixed(Option<Vec<ShapedCluster>>);
impl RunShaper for Fixed {
    fn shape_run(&self, _t: &str, _s: u32, _b: bool) -> Option<Vec<ShapedCluster>> {
        self.0.clone()
    }
}
// Wraps Scaled and corrupts one thing.
struct Corrupt(&'static str);
impl RunShaper for Corrupt {
    fn shape_run(&self, t: &str, s: u32, b: bool) -> Option<Vec<ShapedCluster>> {
        let mut v = Scaled.shape_run(t, s, b)?;
        match self.0 {
            "short" => {
                v.pop();
            }
            "long" => v.push(v[0]),
            "gap" => v[0].first_char = 1,
            "ligature" => v[0].char_len = 2,
            "zero" => v[0].units = 0,
            "huge" => v[0].units = u32::MAX,
            "reordered" => {
                if v.len() > 1 {
                    v.swap(0, 1)
                } else {
                    v[0].first_char = 1
                }
            }
            "none" => return None,
            _ => unreachable!(),
        }
        Some(v)
    }
}
fn legacy_result(runs: &[(&str, bool)], w: u32) -> Result<Placed, kobo_web_document::inline_lines::LineError> {
    place_runs_normal(runs, w, legacy).map(Placed::Legacy)
}
fn unscale(lines: &Lines) -> Vec<(char, u32, u32, u32, bool)> {
    lines.glyphs.iter().map(|g| (g.character, g.x, g.line, g.advance, g.bold)).collect()
}
const RUNS: &[&[(&str, bool)]] = &[
    &[("The quick brown fox jumps over the lazy dog", false)],
    &[("  leading and   collapsed\n whitespace\tmix  ", false)],
    &[("plain ", false), ("bold words", true), (" then plain again", false)],
    &[("ab", false), ("cd", true), ("ef gh", false)],
    &[("x", false)],
    &[("", false), ("one", true), (" ", false), ("two", false)],
];

#[test]
fn equivalent_to_legacy_when_advances_are_legacy_times_64() {
    for runs in RUNS {
        for w in [20_u32, 33, 47, 60, 90, 140, 400] {
            let want = place_runs_normal(runs, w, legacy);
            let got = place_runs_shaped(runs, w, 16, &Scaled, STRICT, legacy);
            match (want, got) {
                (Ok(want), Ok(Placed::Shaped(got))) => {
                    assert_eq!(want.count, got.count, "{runs:?} {w}");
                    let a = unscale(&want);
                    let b = unscale(&got);
                    assert_eq!(a.len(), b.len());
                    for (x, y) in a.iter().zip(&b) {
                        assert_eq!((x.0, x.2, x.4), (y.0, y.2, y.4), "{runs:?} {w}");
                        assert_eq!(x.1 * 64, y.1, "x {runs:?} {w}");
                        assert_eq!(x.3 * 64, y.3, "adv {runs:?} {w}");
                    }
                }
                (Err(e), Ok(Placed::Legacy(_))) => panic!("legacy err {e:?} but legacy path Ok"),
                (Err(e), Err(f)) => assert_eq!(e, f, "{runs:?} {w}"),
                (want, got) => panic!("mismatch {runs:?} {w}: {want:?} vs {got:?}"),
            }
        }
    }
}

#[test]
fn every_malformed_provider_output_falls_back_to_byte_identical_legacy() {
    for kind in ["short", "long", "gap", "ligature", "zero", "huge", "reordered", "none"] {
        for runs in RUNS {
            for w in [47_u32, 90, 400] {
                let got = place_runs_shaped(runs, w, 16, &Corrupt(kind), STRICT, legacy);
                // "reordered" swaps cluster 0/1: first_char check rejects it.
                assert_eq!(got, legacy_result(runs, w), "{kind} {runs:?} {w}");
                if let Ok(p) = &got {
                    assert!(matches!(p, Placed::Legacy(_)), "{kind}");
                }
            }
        }
    }
    let got = place_runs_shaped(RUNS[0], 90, 16, &Fixed(Some(vec![])), STRICT, legacy);
    assert_eq!(got, legacy_result(RUNS[0], 90));
}

#[test]
fn non_ascii_or_control_text_defers_to_legacy_whole_block() {
    for text in ["caf\u{e9} au lait", "non\u{a0}breaking space", "emoji \u{1F600}", "nul\u{0}byte", "tab\u{b}vt"] {
        let runs = [(text, false)];
        let got = place_runs_shaped(&runs, 200, 16, &Scaled, STRICT, legacy);
        assert_eq!(got, legacy_result(&runs, 200), "{text:?}");
        assert!(!matches!(got, Ok(Placed::Shaped(_))), "{text:?}");
    }
    // One bad char in a later run poisons the whole block, not just that run.
    let runs = [("fine ", false), ("caf\u{e9}", true)];
    assert_eq!(place_runs_shaped(&runs, 200, 16, &Scaled, STRICT, legacy), legacy_result(&runs, 200));
}

#[test]
fn degenerate_inputs_match_legacy() {
    let e: &[(&str, bool)] = &[];
    for runs in [e, &[("", false)][..], &[("   ", false)][..]] {
        assert_eq!(place_runs_shaped(runs, 50, 16, &Scaled, STRICT, legacy), legacy_result(runs, 50));
    }
    assert_eq!(place_runs_shaped(RUNS[0], 0, 16, &Scaled, STRICT, legacy), legacy_result(RUNS[0], 0));
    assert_eq!(place_runs_shaped(RUNS[0], 50, 0, &Scaled, STRICT, legacy), legacy_result(RUNS[0], 50));
}

#[test]
fn legacy_hook_is_never_consulted_on_the_shaped_path() {
    let mut calls = 0;
    let got = place_runs_shaped(RUNS[2], 90, 16, &Scaled, STRICT, |_, _| {
        calls += 1;
        Some(1)
    });
    assert!(matches!(got, Ok(Placed::Shaped(_))));
    assert_eq!(calls, 0);
}

#[test]
fn strict_vs_fitted_rule_is_explicit_and_changes_only_the_fit() {
    // "ab cd": a=b=space=64, c=65, d=64; exactly 321/64 px wide on one line.
    struct Edge;
    impl RunShaper for Edge {
        fn shape_run(&self, t: &str, _s: u32, _b: bool) -> Option<Vec<ShapedCluster>> {
            Some(t.chars().enumerate().map(|(i, c)| ShapedCluster {
                first_char: i as u32, char_len: 1, units: if c == 'c' { 65 } else { 64 } }).collect())
        }
    }
    let runs = [("ab cd", false)];
    // 5 px = 320/64: strict must wrap, +1/64 fits both words on one line.
    let strict = place_runs_shaped(&runs, 5, 16, &Edge, STRICT, legacy).unwrap();
    let fitted = place_runs_shaped(&runs, 5, 16, &Edge, FitRule { unit: 64, slack: 1 }, legacy).unwrap();
    let (Placed::Shaped(s), Placed::Shaped(f)) = (strict, fitted) else { panic!() };
    assert_eq!(s.count, 2);
    assert_eq!(f.count, 1);
}
