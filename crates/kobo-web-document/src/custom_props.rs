//! CANDIDATE (unreviewed): CSS custom properties and `var()` substitution.
//!
//! Scope is deliberately narrow and strict. Grounded in CSS Custom Properties
//! for Cascading Variables Level 1 (W3C CR 2022-06-16): names are
//! case-sensitive, values keep their authored casing, the properties inherit,
//! `initial` is the guaranteed-invalid value, `inherit`/`unset` take the
//! parent value, `var()` takes a fallback that may contain commas and nested
//! `var()`. Every form not implemented here REFUSES instead of being accepted
//! wrongly: strings, `url(`, escapes, braces, `!`, `revert`/`revert-layer`,
//! var() glued to adjacent tokens (substitution is token-level, textual
//! concatenation would merge tokens), CSS-wide keywords arriving through a
//! substituted value, dependency cycles, and any invalid-at-computed-value
//! case (the spec would make the declaration `unset`; refusing is stricter).
//! Registered properties (`@property`) are unsupported at-rules and still refuse.

use std::collections::BTreeMap;

pub(crate) const MAX_VALUE_BYTES: usize = 1024;
pub(crate) const MAX_SUBSTITUTED_BYTES: usize = 4096;
pub(crate) const MAX_VARS_PER_VALUE: usize = 32;
pub(crate) const MAX_DEPTH: usize = 8;
pub(crate) const MAX_PROPS: usize = 256;

pub(crate) type Map = BTreeMap<String, String>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Fail {
    /// Not representable or not safely implemented: refuse the page.
    Refuse,
}

pub(crate) fn is_wide_keyword(value: &str) -> bool {
    ["inherit", "initial", "unset", "revert", "revert-layer"]
        .iter()
        .any(|k| value.trim().eq_ignore_ascii_case(k))
}

fn ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || !c.is_ascii()
}

pub(crate) fn valid_name(name: &str) -> bool {
    name.len() > 2 && name.starts_with("--") && name.chars().all(ident_char)
}

/// Bounded structural check of a custom property value (comments already
/// stripped, `!important` already removed). Empty is valid (`--x: ;`).
pub(crate) fn valid_definition(value: &str) -> bool {
    if value.len() > MAX_VALUE_BYTES {
        return false;
    }
    let mut stack: Vec<char> = Vec::new();
    for c in value.chars() {
        match c {
            '"' | '\'' | '\\' | '{' | '}' | ';' | '!' | '\0' => return false,
            '(' | '[' => stack.push(c),
            ')' => {
                if stack.pop() != Some('(') {
                    return false;
                }
            }
            ']' => {
                if stack.pop() != Some('[') {
                    return false;
                }
            }
            _ => {}
        }
    }
    if !stack.is_empty() {
        return false;
    }
    let lower = value.to_ascii_lowercase();
    !(lower.contains("url(") || lower.contains("/*"))
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct VarRef<'a> {
    pub start: usize,
    pub end: usize,
    pub name: &'a str,
    pub fallback: Option<&'a str>,
}

pub(crate) fn has_var(value: &str) -> bool {
    value.to_ascii_lowercase().contains("var(")
}

/// Top-level `var()` references. `None` = malformed, glued to a neighbouring
/// token, or over budget (all refuse).
pub(crate) fn find_vars(value: &str) -> Option<Vec<VarRef<'_>>> {
    let bytes = value.as_bytes();
    let lower = value.to_ascii_lowercase();
    let lb = lower.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= bytes.len() {
        if &lb[i..i + 4] == b"var(" {
            if i > 0 {
                let prev = value[..i].chars().next_back()?;
                if !(prev.is_whitespace() || matches!(prev, ',' | '(' | '/')) {
                    return None;
                }
            }
            let mut depth = 1_usize;
            let mut j = i + 4;
            let mut comma = None;
            while j < bytes.len() && depth > 0 {
                match bytes[j] {
                    b'(' | b'[' => depth += 1,
                    b')' | b']' => depth -= 1,
                    b',' if depth == 1 && comma.is_none() => comma = Some(j),
                    _ => {}
                }
                j += 1;
            }
            if depth != 0 {
                return None;
            }
            let close = j - 1;
            if let Some(&b) = bytes.get(j) {
                let next = value[j..].chars().next()?;
                if !(next.is_whitespace() || matches!(next, ',' | ')' | '/')) {
                    let _ = b;
                    return None;
                }
            }
            let (raw_name, fallback) = match comma {
                Some(c) => (&value[i + 4..c], Some(value[c + 1..close].trim())),
                None => (&value[i + 4..close], None),
            };
            let name = raw_name.trim();
            if !valid_name(name) {
                return None;
            }
            out.push(VarRef { start: i, end: j, name, fallback });
            if out.len() > MAX_VARS_PER_VALUE {
                return None;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    Some(out)
}

/// Substitute every `var()` using `lookup` (`Ok(None)` = guaranteed-invalid).
pub(crate) fn substitute(
    value: &str,
    lookup: &mut dyn FnMut(&str) -> Result<Option<String>, Fail>,
    depth: usize,
) -> Result<String, Fail> {
    if depth > MAX_DEPTH {
        return Err(Fail::Refuse);
    }
    let refs = find_vars(value).ok_or(Fail::Refuse)?;
    let mut out = String::new();
    let mut at = 0;
    for r in refs {
        out.push_str(&value[at..r.start]);
        let piece = match lookup(r.name)? {
            Some(found) => found,
            None => match r.fallback {
                Some(fallback) => substitute(fallback, lookup, depth + 1)?,
                None => return Err(Fail::Refuse),
            },
        };
        if is_wide_keyword(&piece) {
            return Err(Fail::Refuse);
        }
        out.push_str(&piece);
        at = r.end;
        if out.len() > MAX_SUBSTITUTED_BYTES {
            return Err(Fail::Refuse);
        }
    }
    out.push_str(&value[at..]);
    if out.len() > MAX_SUBSTITUTED_BYTES {
        return Err(Fail::Refuse);
    }
    Ok(out)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Def {
    Value(String),
    Initial,
    Inherit,
}

/// Classify the winning specified value of one custom property.
pub(crate) fn classify(value: &str) -> Result<Def, Fail> {
    let v = value.trim();
    if v.eq_ignore_ascii_case("initial") {
        return Ok(Def::Initial);
    }
    if v.eq_ignore_ascii_case("inherit") || v.eq_ignore_ascii_case("unset") {
        return Ok(Def::Inherit);
    }
    if is_wide_keyword(v) {
        return Err(Fail::Refuse);
    }
    Ok(Def::Value(v.to_owned()))
}

/// Every custom property name referenced by `value`, at any nesting depth
/// including fallbacks (the spec adds dependency edges for fallbacks too, even
/// when the fallback would not be used).
fn referenced_names(value: &str, depth: usize, out: &mut Vec<String>) -> Result<(), Fail> {
    if depth > MAX_DEPTH {
        return Err(Fail::Refuse);
    }
    for r in find_vars(value).ok_or(Fail::Refuse)? {
        out.push(r.name.to_owned());
        if let Some(fallback) = r.fallback {
            referenced_names(fallback, depth + 1, out)?;
        }
    }
    Ok(())
}

/// Bytes the computed map occupies (work accounting for the tree-wide budget).
pub(crate) fn map_bytes(map: &Map) -> usize {
    map.iter().map(|(k, v)| k.len() + v.len() + 48).sum()
}

/// Computed custom-property map of one element from the parent's map and the
/// winning local definitions (cascade already applied by the caller).
pub(crate) fn compute_map(parent: &Map, local: &BTreeMap<String, Def>) -> Result<Map, Fail> {
    struct Ctx<'a> {
        parent: &'a Map,
        local: &'a BTreeMap<String, Def>,
        done: BTreeMap<String, Option<String>>,
        visiting: Vec<String>,
    }
    impl Ctx<'_> {
        fn get(&mut self, name: &str) -> Result<Option<String>, Fail> {
            let Some(def) = self.local.get(name) else {
                return Ok(self.parent.get(name).cloned());
            };
            if let Some(known) = self.done.get(name) {
                return Ok(known.clone());
            }
            if self.visiting.iter().any(|n| n == name) {
                // Dependency cycle: spec makes all of them invalid; refuse.
                return Err(Fail::Refuse);
            }
            let resolved = match def {
                Def::Initial => None,
                Def::Inherit => self.parent.get(name).cloned(),
                Def::Value(raw) => {
                    if has_var(raw) {
                        self.visiting.push(name.to_owned());
                        let raw = raw.clone();
                        let r = substitute(&raw, &mut |n| self.get(n), 0);
                        self.visiting.pop();
                        Some(r?.trim().to_owned())
                    } else {
                        Some(raw.clone())
                    }
                }
            };
            self.done.insert(name.to_owned(), resolved.clone());
            Ok(resolved)
        }
    }
    // Static dependency graph over this element's own definitions, fallbacks
    // included: any cycle refuses, whether or not it would be exercised.
    let mut graph: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (name, def) in local {
        let mut names = Vec::new();
        if let Def::Value(raw) = def {
            if has_var(raw) {
                referenced_names(raw, 0, &mut names)?;
            }
        }
        names.retain(|n| local.contains_key(n.as_str()));
        graph.insert(name.as_str(), names);
    }
    // 0 = unvisited, 1 = on stack, 2 = done (iterative DFS).
    let mut state: BTreeMap<&str, u8> = BTreeMap::new();
    for &start in graph.keys() {
        if state.get(start).copied().unwrap_or(0) != 0 {
            continue;
        }
        let mut stack: Vec<(&str, usize)> = vec![(start, 0)];
        state.insert(start, 1);
        while let Some(&(node, next)) = stack.last() {
            let edges = &graph[node];
            if next < edges.len() {
                if let Some(top) = stack.last_mut() {
                    top.1 += 1;
                }
                let target = edges[next].as_str();
                match state.get(target).copied().unwrap_or(0) {
                    1 => return Err(Fail::Refuse),
                    0 => {
                        state.insert(target, 1);
                        stack.push((target, 0));
                    }
                    _ => {}
                }
            } else {
                state.insert(node, 2);
                stack.pop();
            }
        }
    }
    let mut ctx = Ctx { parent, local, done: BTreeMap::new(), visiting: Vec::new() };
    let mut map = parent.clone();
    let names: Vec<String> = local.keys().cloned().collect();
    for name in names {
        match ctx.get(&name)? {
            Some(v) => {
                map.insert(name, v);
            }
            None => {
                map.remove(&name);
            }
        }
    }
    if map.len() > MAX_PROPS {
        return Err(Fail::Refuse);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(value: &str, pairs: &[(&str, &str)]) -> Result<String, Fail> {
        let map: Map = pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        substitute(value, &mut |n| Ok(map.get(n).cloned()), 0)
    }

    #[test]
    fn names_are_case_sensitive_and_values_keep_case() {
        assert_eq!(sub("var(--Foo)", &[("--Foo", "ReD")]).as_deref(), Ok("ReD"));
        assert_eq!(sub("var(--foo)", &[("--Foo", "red")]), Err(Fail::Refuse));
    }

    #[test]
    fn fallback_may_contain_commas_and_nested_var() {
        assert_eq!(sub("var(--a, 1px, 2px)", &[]).as_deref(), Ok("1px, 2px"));
        assert_eq!(sub("var(--a, var(--b, red))", &[]).as_deref(), Ok("red"));
        assert_eq!(sub("var(--a, var(--b))", &[("--b", "blue")]).as_deref(), Ok("blue"));
        assert_eq!(sub("var(--a, )", &[]).as_deref(), Ok(""));
        // A defined value ignores the fallback entirely.
        assert_eq!(sub("var(--a, red)", &[("--a", "blue")]).as_deref(), Ok("blue"));
    }

    #[test]
    fn missing_without_fallback_refuses() {
        assert_eq!(sub("var(--a)", &[]), Err(Fail::Refuse));
    }

    #[test]
    fn substitution_is_token_level_never_glued() {
        assert_eq!(sub("var(--n)px", &[("--n", "10")]), Err(Fail::Refuse));
        assert_eq!(sub("1var(--n)", &[("--n", "0")]), Err(Fail::Refuse));
        assert_eq!(sub("0 var(--n) 2px", &[("--n", "1px")]).as_deref(), Ok("0 1px 2px"));
        assert_eq!(sub("a,var(--n)", &[("--n", "b")]).as_deref(), Ok("a,b"));
    }

    #[test]
    fn css_wide_keywords_through_var_refuse() {
        assert_eq!(sub("var(--a)", &[("--a", "inherit")]), Err(Fail::Refuse));
        assert_eq!(sub("var(--a, initial)", &[]), Err(Fail::Refuse));
    }

    #[test]
    fn malformed_and_foreign_function_names() {
        assert_eq!(sub("var(--a", &[]), Err(Fail::Refuse));
        assert_eq!(sub("var(a)", &[]), Err(Fail::Refuse));
        assert_eq!(sub("var(--)", &[]), Err(Fail::Refuse));
        // "myvar(" is some other function name ending in var: refuse, never pass through.
        assert_eq!(sub("myvar(--a)", &[("--a", "1")]), Err(Fail::Refuse));
    }

    #[test]
    fn budgets_refuse() {
        let many = "var(--a) ".repeat(MAX_VARS_PER_VALUE + 1);
        assert_eq!(sub(&many, &[("--a", "1")]), Err(Fail::Refuse));
        let deep = format!("{}red{}", "var(--x, ".repeat(MAX_DEPTH + 2), ")".repeat(MAX_DEPTH + 2));
        assert_eq!(sub(&deep, &[]), Err(Fail::Refuse));
    }

    #[test]
    fn definitions_accept_only_bounded_plain_tokens() {
        assert!(valid_definition(""));
        assert!(valid_definition("rgb(1, 2, 3)"));
        assert!(valid_definition("var(--x, 1px)"));
        for bad in ["\"s\"", "url(x)", "a\\b", "{a}", "a;b", "a !b", "(", "a)", "[a)"] {
            assert!(!valid_definition(bad), "{bad}");
        }
        assert!(!valid_definition(&"a".repeat(MAX_VALUE_BYTES + 1)));
    }

    fn local(pairs: &[(&str, Def)]) -> BTreeMap<String, Def> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
    }

    #[test]
    fn map_inherits_overrides_and_resolves_local_references() {
        let parent: Map = [("--a".to_owned(), "red".to_owned())].into();
        let m = compute_map(&parent, &local(&[])).unwrap();
        assert_eq!(m.get("--a").map(String::as_str), Some("red"));
        let m = compute_map(&parent, &local(&[("--b", Def::Value("var(--a)".into()))])).unwrap();
        assert_eq!(m.get("--b").map(String::as_str), Some("red"));
        // A local redefinition is what a same-element reference sees.
        let m = compute_map(
            &parent,
            &local(&[("--a", Def::Value("blue".into())), ("--b", Def::Value("var(--a)".into()))]),
        )
        .unwrap();
        assert_eq!(m.get("--b").map(String::as_str), Some("blue"));
        let m = compute_map(&parent, &local(&[("--a", Def::Initial)])).unwrap();
        assert!(!m.contains_key("--a"));
        let m = compute_map(&parent, &local(&[("--a", Def::Inherit)])).unwrap();
        assert_eq!(m.get("--a").map(String::as_str), Some("red"));
    }

    #[test]
    fn cycles_and_unresolvable_custom_values_refuse() {
        let p = Map::new();
        let cyc = local(&[("--a", Def::Value("var(--b)".into())), ("--b", Def::Value("var(--a)".into()))]);
        assert_eq!(compute_map(&p, &cyc), Err(Fail::Refuse));
        let selfref = local(&[("--a", Def::Value("var(--a, 1)".into()))]);
        assert_eq!(compute_map(&p, &selfref), Err(Fail::Refuse));
        // Fallback edges count even when the primary value is defined.
        let fb = local(&[("--x", Def::Value("1".into())), ("--a", Def::Value("var(--x, var(--a))".into()))]);
        assert_eq!(compute_map(&p, &fb), Err(Fail::Refuse));
        let fb2 = local(&[("--a", Def::Value("var(--x, var(--b))".into())), ("--b", Def::Value("var(--a)".into())), ("--x", Def::Value("1".into()))]);
        assert_eq!(compute_map(&p, &fb2), Err(Fail::Refuse));
        // A diamond is not a cycle.
        let dia = local(&[("--a", Def::Value("1".into())), ("--b", Def::Value("var(--a)".into())), ("--c", Def::Value("var(--a) var(--b)".into()))]);
        assert!(compute_map(&p, &dia).is_ok());
        let missing = local(&[("--a", Def::Value("var(--zz)".into()))]);
        assert_eq!(compute_map(&p, &missing), Err(Fail::Refuse));
        assert_eq!(classify("revert"), Err(Fail::Refuse));
        assert_eq!(classify("Revert-Layer"), Err(Fail::Refuse));
        assert_eq!(classify("INITIAL"), Ok(Def::Initial));
        assert_eq!(classify(" unset "), Ok(Def::Inherit));
    }
}
