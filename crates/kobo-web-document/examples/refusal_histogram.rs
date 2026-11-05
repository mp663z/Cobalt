//! Measurement only (no render, never PASS): runs the public style-tree parse on a saved page under
//! three explicitly labeled configurations and prints the RefusalReason histogram.
//! usage: refusal_histogram PAGE.html SHEETDIR
use kobo_web_document::style_diagnostics::RefusalReason;
use kobo_web_document::{parse_style_tree_with_supply, Limits, MediaContext, SupplyBudget};
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let bytes = std::fs::read(&a[0]).unwrap();
    let mut files: Vec<_> = std::fs::read_dir(&a[1]).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    let sheets: Vec<Vec<u8>> = files.iter().map(|p| std::fs::read(p).unwrap()).collect();
    let media = MediaContext::new(800);
    let cand = SupplyBudget { max_linked_sheets: 64, max_sheet_bytes: 256 * 1024, max_aggregate_bytes: 64 * 1024 * 1024 };
    let big = Limits { max_input_bytes: 4 * 1024 * 1024, ..Limits::DEFAULT };
    let cfgs: [(&str, &[Vec<u8>], Limits, SupplyBudget); 3] = [
        ("A canonical, no host sheets", &[], Limits::DEFAULT, SupplyBudget::CANONICAL),
        ("B canonical budget, host sheets supplied", &sheets, Limits::DEFAULT, SupplyBudget::CANONICAL),
        ("C CANDIDATE budget(64 sheets,256KiB,64MiB)+input 4MiB (measurement only)", &sheets, big, cand),
    ];
    for (name, s, lim, b) in cfgs {
        let t = parse_style_tree_with_supply(&bytes, s, &lim, media, b);
        let d = &t.diagnostics;
        print!("{name}: unsupported={} truncated={} first={:?} nodes={} expanded_decl={} match_attempts={} dropped_attrs={} host_sheets_claimed={:?} counts:", t.unsupported, t.truncated, d.first_reason(), t.nodes.len(), d.expanded_declarations, d.match_attempts, d.dropped_attributes, t.host_sheets_claimed);
        for r in RefusalReason::ALL { let c = d.count(r); if c > 0 { print!(" {r:?}={c}"); } }
        println!();
    }
}
