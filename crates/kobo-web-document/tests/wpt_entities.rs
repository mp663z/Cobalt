//! Every named character reference in the HTML standard, read in page text.
//!
//! `tests/wpt/entities/entities.tsv` is the standard's `entities.json`
//! (source in `tests/wpt/include.txt`): each name, with or without its
//! semicolon as the standard lists it, and the code points it stands for.

use kobo_web_document::{parse_document, Limits, Url};

#[test]
fn every_named_character_reference_reads_as_its_characters() {
    let path = format!(
        "{}/tests/wpt/entities/entities.tsv",
        env!("CARGO_MANIFEST_DIR")
    );
    let table = std::fs::read_to_string(&path).expect("entities.tsv");
    let url = Url::parse("https://entities.example/").expect("url");
    let mut wrong = Vec::new();
    let mut seen = 0;
    for line in table.lines() {
        let (name, points) = line.split_once('\t').expect("two fields");
        let expected: String = points
            .split(' ')
            .map(|hex| char::from_u32(u32::from_str_radix(hex, 16).expect("hex")).expect("char"))
            .collect();
        // Inside `<pre>`, so white space entities are kept as they are; the
        // brackets stop a name without its semicolon from running on.
        let page = format!("<pre>[{name}]</pre>");
        let document = parse_document(page.as_bytes(), &url, &Limits::DEFAULT);
        let text = document.visible_text();
        let want = format!("[{expected}]\n");
        if text != want {
            wrong.push(format!("{name}: want {want:?}, got {text:?}"));
        }
        seen += 1;
    }
    assert_eq!(seen, 2231);
    assert!(
        wrong.is_empty(),
        "{} wrong:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

/// Numeric references and the standard's repairs to them: zero, surrogates
/// and values past Unicode become U+FFFD, and the C1 range is read as the
/// Windows-1252 characters pages meant by it.
#[test]
fn numeric_character_references_are_repaired_as_the_standard_says() {
    let url = Url::parse("https://entities.example/").expect("url");
    let cases = [
        ("&#65;", "A"),
        ("&#x41;", "A"),
        ("&#X41;", "A"),
        ("&#0;", "\u{FFFD}"),
        ("&#xD800;", "\u{FFFD}"),
        ("&#x110000;", "\u{FFFD}"),
        ("&#99999999999;", "\u{FFFD}"),
        ("&#x80;", "\u{20AC}"),
        ("&#x92;", "\u{2019}"),
        ("&#x9F;", "\u{178}"),
        ("&#x81;", "\u{81}"),
        ("&#65", "A"),
        ("&#;", "&#;"),
        ("&#x;", "&#x;"),
    ];
    for (reference, expected) in cases {
        let page = format!("<pre>[{reference}]</pre>");
        let document = parse_document(page.as_bytes(), &url, &Limits::DEFAULT);
        assert_eq!(
            document.visible_text(),
            format!("[{expected}]\n"),
            "{reference}"
        );
    }
}
