//! MEASUREMENT ONLY (isolated): NARROW candidate = only link[href^="data:text/css"] may exceed the
//! 2048-byte attribute cap (<= per-href cap, <= aggregate cap), plus a stylesheet SupplyBudget.
use kobo_web_document::{measure, parse_style_tree_with_supply, style_diagnostics::RefusalReason, Limits, SupplyBudget};
use std::sync::atomic::Ordering;
fn main() {
    let max = SupplyBudget::CANONICAL.max_sheet_bytes;
    let per_href = 4 * (max + 2) / 3 + 64; // encoded size of one max sheet plus header
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path).unwrap();
        println!("== {}", path.rsplit('/').next().unwrap());
        for (name, sheets, agg) in [("off(canonical)", 2usize, 0usize), ("narrow 2 sheets", 2, 2 * per_href), ("narrow 8 sheets", 8, 8 * per_href), ("narrow 32 sheets", 32, 32 * per_href)] {
            measure::SHEET_HREF_CAP.store(if agg == 0 { 0 } else { per_href }, Ordering::Relaxed);
            measure::SHEET_AGG_CAP.store(agg, Ordering::Relaxed);
            let b = SupplyBudget { max_linked_sheets: sheets, max_sheet_bytes: max, max_aggregate_bytes: sheets * max };
            let t = parse_style_tree_with_supply(&bytes, &[], &Limits::DEFAULT, None, b);
            let counts: Vec<String> = RefusalReason::ALL.iter().filter(|r| t.diagnostics.count(**r) > 0).map(|r| format!("{r:?}={}", t.diagnostics.count(*r))).collect();
            println!("  {name}: unsupported={} truncated={} first={:?} [{}]", t.unsupported, t.truncated, t.diagnostics.first_reason(), counts.join(", "));
        }
    }
}
