//! usage: classify_refusals SHEETDIR (all decoded sheets of one page, document order)
use kobo_web_document::refusal_classify::{classify, Report};
use kobo_web_document::MediaContext;
fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    let mut t = Report::default();
    for f in &files {
        let css = String::from_utf8_lossy(&std::fs::read(f).unwrap()).into_owned();
        let r = classify(&css, MediaContext::new(800));
        t.rules_scanned += r.rules_scanned; t.rules_skipped_by_condition += r.rules_skipped_by_condition;
        t.rules_decl_refused += r.rules_decl_refused; t.decls_total += r.decls_total; t.decls_refused += r.decls_refused;
        for (k, v) in r.known_vals { *t.known_vals.entry(k).or_default() += v; }
        for (k, v) in r.cls { *t.cls.entry(k).or_default() += v; }
        for (k, v) in r.refused_names { *t.refused_names.entry(k).or_default() += v; }
        t.selectors_total += r.selectors_total; t.selectors_refused += r.selectors_refused;
        for (k, v) in r.sel_cls { *t.sel_cls.entry(k).or_default() += v; }
        t.rules_with_both += r.rules_with_both; t.rules_any_refusal += r.rules_any_refusal; t.rules_var_only_decl_refusal += r.rules_var_only_decl_refusal;
        t.unsupported_conditions += r.unsupported_conditions; t.unsupported_at_rule |= r.unsupported_at_rule; t.malformed |= r.malformed;
    }
    println!("sheets={} rules_scanned={} rules_skipped_by_condition(media/supports not screen-fit)={} unsupported_conditions={} unsupported_at_rule={} malformed={}", files.len(), t.rules_scanned, t.rules_skipped_by_condition, t.unsupported_conditions, t.unsupported_at_rule, t.malformed);
    println!("RULES with declaration refusals whose refused declarations are ALL var()/custom-property involved={}", t.rules_var_only_decl_refusal);
    println!("RULES: decl_refused={} selector_refused(count of rules with >=1 refused selector)=see selectors; any_refusal={} both={}", t.rules_decl_refused, t.rules_any_refusal, t.rules_with_both);
    println!("DECLS: total={} refused={} ; by class: {:?}", t.decls_total, t.decls_refused, t.cls);
    let mut names: Vec<_> = t.refused_names.iter().collect();
    names.sort_by(|a, b| b.1.cmp(a.1));
    let distinct: std::collections::BTreeSet<_> = t.refused_names.keys().map(|k| &k.0).collect();
    let mut kv: Vec<_> = t.known_vals.iter().collect(); kv.sort_by(|a, b| b.1.cmp(a.1));
    println!("KNOWN-NAME refused value tokens: distinct (name,value)={} ; TOP60: {:?}", kv.len(), kv.iter().take(60).map(|(k, v)| (format!("{}:{}", k.0, k.1), **v)).collect::<Vec<_>>());
    println!("DISTINCT refused property names={}", distinct.len());
    println!("TOP refused names (name,class)=count: {:?}", names.iter().take(30).map(|(k, v)| (&k.0[..], &k.1[..8.min(k.1.len())], **v)).collect::<Vec<_>>());
    let mut sc: Vec<_> = t.sel_cls.iter().collect(); sc.sort_by(|a, b| b.1.cmp(a.1));
    println!("SELECTORS: total={} refused={} ; top classes: {:?}", t.selectors_total, t.selectors_refused, sc.iter().take(15).collect::<Vec<_>>());
}

