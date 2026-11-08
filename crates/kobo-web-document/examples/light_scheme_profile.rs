//! MEASUREMENT ONLY: narrow 8-sheet supply with MediaContext 1072 wide: legacy (no scheme) vs host-asserted light scheme.
use kobo_web_document::{measure, parse_style_tree_with_supply, style_diagnostics::RefusalReason, Limits, MediaContext, SupplyBudget};
use std::sync::atomic::Ordering;
fn main() {
    let max = SupplyBudget::CANONICAL.max_sheet_bytes;
    let per_href = 4 * (max + 2) / 3 + 64;
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path).unwrap();
        println!("== {}", path.rsplit('/').next().unwrap());
        for (name, ctx) in [("ctx1072 legacy", MediaContext::new(1072)), ("ctx1072 light", MediaContext::new(1072).map(MediaContext::with_light_scheme))] {
            measure::SHEET_HREF_CAP.store(per_href, Ordering::Relaxed);
            measure::SHEET_AGG_CAP.store(8 * per_href, Ordering::Relaxed);
            let b = SupplyBudget { max_linked_sheets: 8, max_sheet_bytes: max, max_aggregate_bytes: 8 * max };
            let t = parse_style_tree_with_supply(&bytes, &[], &Limits::DEFAULT, ctx, b);
            let counts: Vec<String> = RefusalReason::ALL.iter().filter(|r| t.diagnostics.count(**r) > 0).map(|r| format!("{r:?}={}", t.diagnostics.count(*r))).collect();
            println!("  {name}: unsupported={} truncated={} first={:?} [{}]", t.unsupported, t.truncated, t.diagnostics.first_reason(), counts.join(", "));
        }
    }
}
