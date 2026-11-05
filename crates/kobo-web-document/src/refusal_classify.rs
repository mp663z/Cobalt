//! Measurement only: per-declaration/per-selector refusal classification using the real parse functions.
use crate::style_syntax;
use crate::style_tree::properties;
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Report {
    pub rules_scanned: usize,
    pub rules_skipped_by_condition: usize,
    pub rules_decl_refused: usize,
    pub decls_total: usize,
    pub decls_refused: usize,
    pub cls: BTreeMap<&'static str, usize>,
    pub refused_names: BTreeMap<(String, &'static str), usize>,
    pub selectors_total: usize,
    pub selectors_refused: usize,
    pub sel_cls: BTreeMap<String, usize>,
    pub rules_with_both: usize,
    pub rules_any_refusal: usize,
    pub known_vals: BTreeMap<(String, String), usize>,
    pub rules_var_only_decl_refusal: usize,
    pub unsupported_conditions: usize,
    pub unsupported_at_rule: bool,
    pub malformed: bool,
}

fn sel_class(s: &str) -> String {
    let mut out = Vec::new();
    if s.contains('[') { out.push("attr".to_string()); }
    for p in ["::", ":not(", ":is(", ":where(", ":has(", ":nth-", ":first-child", ":last-child", ":hover", ":focus", ":active", ":visited", ":before", ":after", ":root", ":checked", ":disabled", ":link", ":lang(", ":only-child", ":first-of-type", ":last-of-type"] {
        if s.contains(p) { out.push(format!("pseudo{p}")); }
    }
    if s.contains('>') { out.push("child".into()); }
    if s.contains('+') { out.push("adjacent".into()); }
    if s.contains('~') { out.push("sibling".into()); }
    if s.contains('*') { out.push("universal".into()); }
    if s.contains('|') { out.push("namespace".into()); }
    if s.contains('&') { out.push("nesting&".into()); }
    if out.is_empty() { out.push("other".into()); }
    out.sort();
    out.join("+")
}

pub fn classify(css: &str, media: Option<crate::MediaContext>) -> Report {
    let mut r = Report::default();
    let sheet = style_syntax::scan_in_budget(css, media, 8 * 1024 * 1024);
    r.unsupported_conditions = sheet.unsupported_conditions;
    r.unsupported_at_rule = sheet.unsupported_at_rule;
    r.malformed = sheet.malformed;
    for rule in sheet.rules {
        match style_syntax::conditions_fit(&rule.conditions, media) {
            style_syntax::MediaFit::Screen => {}
            _ => { r.rules_skipped_by_condition += 1; continue; }
        }
        r.rules_scanned += 1;
        let mut rule_decl_refused = false;
        let mut non_var_refusal = false;
        for (name, value) in style_syntax::declarations(rule.declarations) {
            r.decls_total += 1;
            let one = format!("{}:{}", name.trim(), value.trim());
            let (_, unknown) = properties(&one);
            if !unknown { continue; }
            r.decls_refused += 1;
            rule_decl_refused = true;
            let n = name.trim().to_ascii_lowercase();
            let class: &'static str = if n.starts_with("--") { "custom-property-decl" }
                else if value.contains("var(") { "value-has-var()" }
                else {
                    let (_, u2) = properties(&format!("{n}:inherit"));
                    if u2 { "property-name-unrecognized(inherit also refused)" } else { "known-name,value-form-refused" }
                };
            if !(class == "custom-property-decl" || class == "value-has-var()") { non_var_refusal = true; }
            if class == "known-name,value-form-refused" { *r.known_vals.entry((n.clone(), value.trim().to_ascii_lowercase())).or_default() += 1; }
            *r.cls.entry(class).or_default() += 1;
            *r.refused_names.entry((n, class)).or_default() += 1;
        }
        if rule_decl_refused { r.rules_decl_refused += 1; if !non_var_refusal { r.rules_var_only_decl_refusal += 1; } }
        let mut sel_refused = false;
        for s in rule.prelude.split(',') {
            r.selectors_total += 1;
            if crate::css::selector(s.trim()).is_none() {
                r.selectors_refused += 1; sel_refused = true;
                *r.sel_cls.entry(sel_class(s)).or_default() += 1;
            }
        }
        if rule_decl_refused && sel_refused { r.rules_with_both += 1; }
        if rule_decl_refused || sel_refused { r.rules_any_refusal += 1; }
    }
    r
}
