//! WPT's HTML tree-construction tests, through this crate's own tree.
//!
//! html5ever decides where each node goes; `dom::Sink` stores what it is
//! told. These tests check the two together: the tree the sink ends up with
//! must be the one the standard says. The `.dat` files are in
//! `tests/wpt/html5lib` (source in `tests/wpt/include.txt`).
//!
//! The sink keeps no comments or doctype, so those lines of the expected
//! trees are left out of the comparison. Fragment cases
//! (`#document-fragment`) and cases for a browser that runs scripts
//! (`#script-on`) are skipped: this browser parses whole documents with
//! scripting off.

use crate::dom::{Data, Handle, Node, Sink, DOCUMENT};
use html5ever::tendril::TendrilSink;
use std::collections::BTreeSet;
use std::fmt::Write as _;

struct Case {
    data: String,
    document: String,
    skip: bool,
}

fn cases(file: &str) -> Vec<Case> {
    let mut out = Vec::new();
    let mut section = String::new();
    let mut data = String::new();
    let mut document = String::new();
    let mut skip = false;
    let mut finish = |data: &mut String, document: &mut String, skip: &mut bool| {
        if data.ends_with('\n') {
            data.pop();
        }
        out.push(Case {
            data: std::mem::take(data),
            document: std::mem::take(document).trim_end_matches('\n').to_owned(),
            skip: std::mem::take(skip),
        });
    };
    let mut started = false;
    for line in file.split('\n') {
        if line == "#data" && (section.is_empty() || section == "#document") {
            if started {
                finish(&mut data, &mut document, &mut skip);
            }
            started = true;
            section = line.to_owned();
            continue;
        }
        // Inside `#data` only `#errors` ends the input; a page may start a
        // line with `#`.
        let in_data = section == "#data";
        if line.starts_with('#') && (!in_data || line.starts_with("#errors")) {
            if matches!(line, "#document-fragment" | "#script-on") {
                skip = true;
            }
            section = line.to_owned();
            continue;
        }
        match section.as_str() {
            "#data" => {
                data.push_str(line);
                data.push('\n');
            }
            "#document" => {
                document.push_str(line);
                document.push('\n');
            }
            _ => {}
        }
    }
    if started {
        finish(&mut data, &mut document, &mut skip);
    }
    out
}

/// Splits a tree dump into its items, dropping comments, doctypes and
/// processing instructions, none of which the sink keeps.
fn items(dump: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in dump.split('\n') {
        if let Some(item) = line.strip_prefix("| ") {
            out.push(item.to_owned());
        } else if let Some(last) = out.last_mut() {
            last.push('\n');
            last.push_str(line);
        }
    }
    out.retain(|item| {
        let item = item.trim_start();
        !item.starts_with("<!-- ") && !item.starts_with("<!DOCTYPE") && !item.starts_with("<?")
    });
    out
}

fn dump(nodes: &[Node], handle: Handle, depth: usize, out: &mut Vec<String>) {
    let indent = "  ".repeat(depth);
    match &nodes[handle].data {
        Data::Document => {}
        Data::Text(text) => out.push(format!("{indent}\"{text}\"")),
        Data::Other => return,
        Data::Element {
            name,
            attrs,
            template,
        } => {
            let prefix = match &*name.ns {
                "http://www.w3.org/2000/svg" => "svg ",
                "http://www.w3.org/1998/Math/MathML" => "math ",
                _ => "",
            };
            out.push(format!("{indent}<{prefix}{}>", name.local));
            let mut sorted: Vec<_> = attrs.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            for (key, value) in sorted {
                out.push(format!("{indent}  {key}=\"{value}\""));
            }
            if let Some(contents) = template {
                out.push(format!("{indent}  content"));
                for &child in &nodes[*contents].children {
                    dump(nodes, child, depth + 2, out);
                }
                return;
            }
        }
    }
    let next = if handle == DOCUMENT { depth } else { depth + 1 };
    for &child in &nodes[handle].children {
        dump(nodes, child, next, out);
    }
}

fn parse(data: &str) -> Vec<String> {
    let options = html5ever::ParseOpts {
        tree_builder: html5ever::tree_builder::TreeBuilderOpts {
            scripting_enabled: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let sink = html5ever::parse_document(Sink::new(usize::MAX), options).one(data);
    let nodes = sink.into_nodes();
    let mut out = Vec::new();
    dump(&nodes, DOCUMENT, 0, &mut out);
    out
}

fn expected_failures() -> BTreeSet<String> {
    let path = format!(
        "{}/tests/wpt/expected-failures.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path)
        .expect("expected-failures.txt")
        .lines()
        .filter_map(|line| line.strip_prefix("tree "))
        .map(str::to_owned)
        .collect()
}

#[test]
fn wpt_html_tree_construction() {
    let directory = format!("{}/tests/wpt/html5lib", env!("CARGO_MANIFEST_DIR"));
    let mut files: Vec<_> = std::fs::read_dir(&directory)
        .expect("html5lib directory")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "dat"))
        .collect();
    files.sort();
    let known = expected_failures();
    let (mut run, mut skipped) = (0, 0);
    let mut unexpected = String::new();
    let mut fixed = Vec::new();
    for path in files {
        let name = path
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        let text = std::fs::read_to_string(&path).expect("dat");
        for (index, case) in cases(&text).into_iter().enumerate() {
            if case.skip {
                skipped += 1;
                continue;
            }
            run += 1;
            let key = format!("{name}#{}", index + 1);
            let (actual, expected) = (parse(&case.data), items(&case.document));
            let passed = actual == expected;
            if !passed && std::env::var_os("TREE_DIFF").is_some() {
                eprintln!(
                    "== {key}\n{:?}\nwant:\n{}\ngot:\n{}",
                    case.data,
                    expected.join("\n"),
                    actual.join("\n")
                );
            }
            match (passed, known.contains(&key)) {
                (false, false) => {
                    let _ = writeln!(unexpected, "{key}");
                }
                (true, true) => fixed.push(key),
                _ => {}
            }
        }
    }
    assert!(run > 1000, "only {run} cases run ({skipped} skipped)");
    assert!(
        unexpected.is_empty() && fixed.is_empty(),
        "unexpected failures:\n{unexpected}listed failures now pass:\n{}",
        fixed.join("\n")
    );
}
