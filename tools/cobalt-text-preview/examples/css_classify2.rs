//! FL64 measurement-only (v2: prints ALL refused names, no top-40 cut): the engine's own refusal_classify over decoded sheets of private pages.
//! Prints property/selector NAMES and counts only (no page content). Evidence only.
use kobo_web_document::refusal_classify::classify;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let media = kobo_web_document::MediaContext::new(a[1].parse().unwrap());
    for f in &a[2..] {
        let css = String::from_utf8_lossy(&std::fs::read(f).unwrap()).into_owned();
        let r = classify(&css, media);
        println!("== {f} bytes={} rules_scanned={} skipped_by_condition={} decls={} decls_refused={} rules_decl_refused={} selectors={} selectors_refused={} rules_any_refusal={} unsupported_conditions={} unsupported_at_rule={} malformed={}",
            css.len(), r.rules_scanned, r.rules_skipped_by_condition, r.decls_total, r.decls_refused, r.rules_decl_refused, r.selectors_total, r.selectors_refused, r.rules_any_refusal, r.unsupported_conditions, r.unsupported_at_rule, r.malformed);
        for (k, v) in &r.cls { println!("  class {k} = {v}"); }
        let mut names: Vec<_> = r.refused_names.iter().collect();
        names.sort_by(|a, b| b.1.cmp(a.1));
        for ((n, c), v) in names.iter() { println!("  refused {n} [{c}] x{v}"); }
        for (k, v) in &r.sel_cls { println!("  selector-class {k} = {v}"); }
    }
}
