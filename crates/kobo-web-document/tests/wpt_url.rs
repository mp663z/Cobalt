//! The http and https cases of WPT's URL parsing tests.
//!
//! `tests/wpt/url/cases.tsv` is taken from `url/resources/urltestdata.json`
//! by `vendor.py`; the upstream commit is in `tests/wpt/include.txt`. Cases
//! this parser gets different from a browser on purpose, or not yet, are in
//! `tests/wpt/expected-failures.txt` with the reason. A case listed there
//! that starts passing fails the test too, so the list stays true.

use kobo_web_document::Url;
use std::collections::BTreeMap;

fn wpt(path: &str) -> String {
    let path = format!("{}/tests/wpt/{path}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn unescape(field: &str) -> String {
    let mut out = String::new();
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('u') => {
                let code: String = chars.by_ref().skip(1).take_while(|&c| c != '}').collect();
                let code = u32::from_str_radix(&code, 16).expect("hex escape");
                out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
            }
            other => panic!("bad escape {other:?} in {field}"),
        }
    }
    out
}

/// Expected failures for one suite: case key to reason.
fn expected_failures(suite: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut reason = String::new();
    for line in wpt("expected-failures.txt").lines() {
        if let Some(text) = line.strip_prefix("# ") {
            text.clone_into(&mut reason);
        } else if let Some(key) = line.strip_prefix(&format!("{suite} ")) {
            out.insert(key.to_owned(), reason.clone());
        }
    }
    out
}

#[test]
fn wpt_url_parsing() {
    let known = expected_failures("url");
    let mut unexpected = Vec::new();
    let mut fixed = Vec::new();
    let mut seen = 0;
    for line in wpt("url/cases.tsv").lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let [input, base, expected] = fields[..] else {
            panic!("bad line {line:?}");
        };
        seen += 1;
        let key = format!("{input} {base}");
        let result = if base == "-" {
            Url::parse(&unescape(input))
        } else {
            // A base this parser refuses (one with a password, say) leaves
            // nothing to resolve against.
            Url::parse(&unescape(base)).and_then(|base| base.join(&unescape(input)))
        };
        let actual = result.map_or_else(|_| "failure".to_owned(), |url| url.to_string());
        let passed = actual == unescape(expected);
        match (passed, known.contains_key(&key)) {
            (false, false) => {
                unexpected.push(format!("{key}\n    want {expected}\n    got  {actual}"));
            }
            (true, true) => fixed.push(key),
            _ => {}
        }
    }
    assert!(seen > 400, "only {seen} cases read");
    assert!(
        unexpected.is_empty() && fixed.is_empty(),
        "{} unexpected failures:\n{}\n{} listed failures now pass:\n{}",
        unexpected.len(),
        unexpected.join("\n"),
        fixed.len(),
        fixed.join("\n"),
    );
}
